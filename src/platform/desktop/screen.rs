//! Frames drawn on the CPU (the gen 2's painter) into the window's round
//! display, upright, with the frame rate of the last second under it.
//! Each frame shown is also kept for `control`. The face is drawn at the
//! display's own pixels (316 points in a unit 480 across, twice that on a
//! Retina display, less where the window is made smaller to fit), or at
//! `--pixels`: 480, the gen 3's, to compare with stock's frames.

use std::path::Path;
use std::sync::OnceLock;
use std::time::Instant;

use crate::display::Geometry;
use crate::paint::Painter;
use crate::platform;

use super::{control, input};

/// The face's pixels each way, when `--pixels` sets them.
pub static PIXELS: OnceLock<usize> = OnceLock::new();

pub struct Screen {
    renderer: Box<platform::gen2::soft::Renderer>,
    /// The face's pixels each way.
    side: usize,
}

impl Screen {
    pub fn open(_device: platform::Device, _cpu: bool) -> Result<Screen, String> {
        let headless = super::HEADLESS.load(std::sync::atomic::Ordering::Relaxed);
        let shown = super::open(if headless { 1.0 } else { input::scroll_sign() })?;
        let side = PIXELS.get().copied().unwrap_or(shown);
        let renderer = platform::gen2::soft::Renderer::new(side as u32, side as u32)?;
        info!("display: CPU renderer, {side}x{side} pixels{}", if side == shown { String::new() } else { format!(" (--pixels; {shown} shown)") });
        Ok(Screen { renderer: Box::new(renderer), side })
    }

    pub fn paced(&self) -> bool {
        true
    }

    pub fn size(&self) -> (u32, u32) {
        (self.side as u32, self.side as u32)
    }

    pub fn use_fonts(&mut self, fonts: &[(String, std::path::PathBuf)]) -> Result<(), String> {
        self.renderer.use_fonts(fonts)
    }

    pub fn draw<R>(&mut self, geometry: &Geometry, design: f32, dir: &Path, draw: impl FnOnce(&mut dyn Painter) -> R) -> R {
        self.renderer.begin(geometry, design);
        draw(&mut self.renderer.painter(dir))
    }

    pub fn finish(&mut self) {}

    /// The CPU painter decodes its images as it draws: nothing comes later.
    pub fn take_decoded(&mut self) -> bool {
        false
    }

    /// The frame into the window, with the frame rate under it; `control`
    /// keeps it too, as nest-qemu's screenshots are.
    pub fn swap(&mut self) -> Result<(), String> {
        let rgba = self.renderer.pixmap.data();
        let now = Instant::now();
        control::shown(rgba, self.side);
        let frames = super::show(rgba, self.side);
        if crate::log::enabled(crate::log::Level::Trace) {
            trace!("display: swap {:.1} ms, {frames} frames in the last second", now.elapsed().as_secs_f64() * 1000.0);
        }
        Ok(())
    }

    pub fn take_counts(&mut self) -> crate::paint::Counts {
        crate::paint::Counts {
            shapes: std::mem::take(&mut self.renderer.shapes), painting: std::mem::take(&mut self.renderer.painting),
            ..Default::default()
        }
    }
}
