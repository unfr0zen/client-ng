//! The panel's backlight (the gen 3's lm3695), through sysfs: off is a
//! brightness of 0, and on restores the brightness it had. The chip ramps
//! between brightnesses itself, at the rise and fall times set before each
//! change, as stock does.

use std::path::PathBuf;

/// Rise and fall times in ms.
type Ramp = (u32, u32);

/// Stock's ramps (product.config displayfadeuptime and displayfadedowntime):
/// the panel lights in 250 ms and darkens in 2 s, and Auto's steps with the
/// light glide over 5 s up and 10 s down. The driver settles on the chip's
/// nearest steps: 255 ms, 2.05 s, 8.2 s and 16.4 s.
const SWITCH: Ramp = (250, 2000);
const GLIDE: Ramp = (5000, 10000);

pub struct Backlight {
    brightness: PathBuf,
    /// The brightness to come back to.
    lit: String,
    max: u32,
    /// Changes to write, written by a thread: each goes to the panel's
    /// driver over I2C, which the render loop does not wait for.
    writes: std::sync::mpsc::Sender<(Ramp, String)>,
}

/// Stock's brightness table (/nestlabs/share/client/brightness/brightness.plist):
/// Low, Medium and High, and Auto's (illuminance, percent) pairs, followed
/// linearly between them.
const LEVELS: [(&str, f32); 3] = [("low", 50.0), ("medium", 90.0), ("high", 100.0)];
const AUTO: [(f32, f32); 11] = [(0.0, 50.0), (1.0, 70.0), (2.0, 77.0), (3.0, 80.0), (8.0, 83.0), (25.0, 86.0),
                                 (40.0, 88.0), (100.0, 90.0), (300.0, 94.0), (530.0, 98.0), (1100.0, 100.0)];

/// The backlight's share of its maximum for a Brightness choice, given the
/// ambient light sensor's visible count when known. Auto takes the count,
/// less 2 (at least 1 when there is any light), as the illuminance: measured
/// against stock on nest-qemu.
pub fn level(choice: &str, visible: Option<u16>) -> f32 {
    if let Some(&(_, pct)) = LEVELS.iter().find(|(name, _)| *name == choice) {
        return pct / 100.0;
    }
    let lux = match visible {
        None => return 1.0,
        Some(0) => 0.0,
        Some(v) => (f32::from(v) - 2.0).max(1.0),
    };
    let pct = match AUTO.iter().position(|&(l, _)| lux < l) {
        Some(0) => AUTO[0].1,
        Some(i) => {
            let ((l0, p0), (l1, p1)) = (AUTO[i - 1], AUTO[i]);
            p0 + (lux - l0) / (l1 - l0) * (p1 - p0)
        }
        None => AUTO[AUTO.len() - 1].1,
    };
    pct / 100.0
}

impl Backlight {
    /// The first backlight the kernel has.
    pub fn open() -> Result<Backlight, String> {
        let dir = std::fs::read_dir("/sys/class/backlight").map_err(|e| format!("/sys/class/backlight: {e}"))?
            .flatten().map(|e| e.path()).min().ok_or("no backlight")?;
        let brightness = dir.join("brightness");
        let lit = std::fs::read_to_string(&brightness).map_err(|e| format!("{}: {e}", brightness.display()))?.trim().to_string();
        let max: u32 = std::fs::read_to_string(dir.join("max_brightness")).ok().and_then(|m| m.trim().parse().ok()).unwrap_or(2047);
        let lit = if lit == "0" { max.to_string() } else { lit };
        let ramps = ["ramp_rise_time", "ramp_fall_time"].map(|f| dir.join(f));
        let ramped = ramps.iter().all(|f| f.exists());
        info!("backlight: {} at {lit} of {max}{}", crate::log::path(&dir), if ramped { "" } else { ", no ramp" });
        let (writes, changes) = std::sync::mpsc::channel::<(Ramp, String)>();
        let file = brightness.clone();
        std::thread::Builder::new().name("backlight".into()).spawn(move || {
            let write = |file: &PathBuf, value: &str| {
                if let Err(e) = std::fs::write(file, value) {
                    error!("backlight: {}: {e}", crate::log::path(file));
                }
            };
            let mut ramp = None;
            while let Ok(first) = changes.recv() {
                // Of a run of changes at one ramp only the last matters.
                let mut pending: Vec<(Ramp, String)> = Vec::new();
                for change in std::iter::once(first).chain(changes.try_iter()) {
                    if pending.last().is_some_and(|p| p.0 == change.0) {
                        pending.pop();
                    }
                    pending.push(change);
                }
                for ((rise, fall), value) in pending {
                    if ramped && ramp != Some((rise, fall)) {
                        write(&ramps[0], &rise.to_string());
                        write(&ramps[1], &fall.to_string());
                        ramp = Some((rise, fall));
                    }
                    write(&file, &value);
                }
            }
        }).map_err(|e| format!("backlight thread: {e}"))?;
        Ok(Backlight { brightness, lit, max, writes })
    }

    /// Lights the panel at a share of its maximum (when lit), gliding there.
    pub fn level(&mut self, share: f32, lit: bool) {
        let value = ((self.max as f32 * share).round() as u32).to_string();
        if value == self.lit {
            return;
        }
        // Auto's small steps with the light are only for a closer look.
        let step = self.lit.parse::<u32>().map_or(self.max, |was| was.abs_diff(value.parse().unwrap_or(0)));
        let level = if step * 50 >= self.max { crate::log::Level::Info } else { crate::log::Level::Debug };
        crate::log::write(level, format_args!("backlight: level {value} of {} ({:.0}%){}", self.max, share * 100.0,
                                              if lit { "" } else { ", kept for when lit" }));
        if lit {
            let _ = self.writes.send((GLIDE, value.clone()));
        }
        self.lit = value;
    }

    /// Lights the panel at its brightness, or darkens it: stock's fade in
    /// and out.
    pub fn set(&mut self, on: bool) {
        if !on {
            if let Ok(now) = std::fs::read_to_string(&self.brightness).map(|b| b.trim().to_string()) {
                if now != "0" {
                    self.lit = now;
                }
            }
        }
        let value = if on { self.lit.as_str() } else { "0" };
        debug!("backlight: {} ({value})", if on { "on" } else { "off" });
        let _ = self.writes.send((SWITCH, value.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_follows_stocks_table() {
        // Stock on nest-qemu: Low 1024, Medium 1842, High 2047 of 2047.
        assert_eq!([level("low", None), level("medium", None), level("high", None)], [0.5, 0.9, 1.0]);
        // Auto, by the sensor's visible count: 0 → 1024, 1–3 → 1433, 4 → 1576, 5 → 1638.
        for (visible, stock) in [(0, 1024), (2, 1433), (4, 1576), (5, 1638), (1000, 2040)] {
            let ours = (2047.0 * level("auto", Some(visible))).round() as i32;
            assert!((ours - stock).abs() <= 10, "{visible}: {ours} against {stock}");
        }
    }
}
