//! Stock nlclient's backplate simulator (`nlbackplatesimulator.cpp`,
//! client-ng/BACKPLATE.md), redone. It speaks no frames: like stock's, it
//! writes the room straight into the client and runs from the HVAC
//! control's switches. `backplatesimulationrun` runs it instead of the
//! backplate; `backplatesimulatetempandhum` lets it supply the room over a
//! real backplate, and `backplatesimulationhvactype` the wiring.
//!
//! The room is stepped every 30 seconds in 16.16 fixed point as stock
//! steps it: towards an ambient temperature (constant, or a daily curve
//! between a minimum and a maximum) at the building's inefficiency, plus
//! what the equipment on adds or takes away, plus noise; humidity follows
//! the absolute humidity it is given.

use std::collections::HashMap;

use crate::hvac::wiring::Switch;

/// 16.16 fixed point, as FreeType's FT_MulFix and FT_DivFix compute it.
type Fixed = i32;

fn mul(a: Fixed, b: Fixed) -> Fixed {
    let p = i64::from(a) * i64::from(b);
    let r = (p.abs() + 0x8000) >> 16;
    (if p < 0 { -r } else { r }) as Fixed
}

fn div(a: Fixed, b: Fixed) -> Fixed {
    if b == 0 {
        return 0x7fff_ffff;
    }
    let (s, a, b) = ((a < 0) != (b < 0), i64::from(a).abs(), i64::from(b).abs());
    let q = ((a << 16) + (b >> 1)) / b;
    (if s { -q } else { q }) as Fixed
}

/// A float to fixed point, truncating as `vcvt.s32.f32 #16` does.
fn fixed(x: f64) -> Fixed {
    (x * 65536.0) as Fixed
}

fn float(x: Fixed) -> f64 {
    f64::from(x) / 65536.0
}

/// The terminals stock simulates for each `backplatesimulationhvactype`.
pub fn hvac_type(t: i32) -> Option<&'static [&'static str]> {
    Some(match t {
        1 => &["W1", "Y1", "G", "Rh", "C"],
        2 => &["W1", "Y1", "Y2", "G", "Rh", "C"],
        3 => &["W1", "AUX/W2", "Y1", "G", "Rh"],
        4 => &["W1", "AUX/W2", "Y1", "Y2", "G", "Rh"],
        5 => &["W1", "AUX/W2", "Y1", "G", "STAR", "Rh"],
        6 => &["W1", "AUX/W2", "Y1", "Y2", "G", "STAR", "Rh"],
        7 => &["Y1", "G", "O/B", "Rh"],
        8 => &["Y1", "G", "O/B", "AUX/W2", "Rh"],
        9 => &["Y1", "G", "O/B", "STAR", "Rh"],
        10 => &["Y1", "G", "O/B", "AUX/W2", "STAR", "Rh"],
        11 => &["Y1", "Y2", "G", "O/B", "Rh"],
        12 => &["Y1", "Y2", "G", "O/B", "AUX/W2", "Rh"],
        13 => &["Y1", "Y2", "G", "O/B", "STAR", "Rh"],
        14 => &["Y1", "Y2", "G", "O/B", "AUX/W2", "STAR", "Rh"],
        15 => &["W1", "G", "Rh"],
        16 => &["Y1", "G", "Rc"],
        17 => &["W1", "AUX/W2", "G", "Rh"],
        18 => &["Y1", "Y2", "G", "Rc"],
        19 => &["W1", "AUX/W2", "G", "STAR", "Rh"],
        _ => return None,
    })
}

/// A terminal's byte in MSG_WIRES and in MSG_PINS.
fn terminal(name: &str) -> Option<(usize, usize)> {
    Some(match name {
        "W1" => (0, 0),
        "Y1" => (1, 1),
        "G" => (2, 5),
        "O/B" => (3, 6),
        "AUX/W2" => (4, 7),
        "Y2" => (7, 9),
        "C" => (8, 2),
        "Rc" => (9, 3),
        "Rh" => (10, 4),
        "STAR" => (11, 11),
        _ => return None,
    })
}

/// Stock's π for the daily curve, truncated as stock has it.
#[allow(clippy::approx_constant)]
const PI: f64 = 3.141592;

/// The leading number of a string, as strtod and strtol read it: 0 when
/// there is none.
fn strtod(v: &str) -> f64 {
    let v = v.trim_start();
    (1..=v.len()).rev().filter(|&n| v.is_char_boundary(n)).find_map(|n| v[..n].parse::<f64>().ok()).unwrap_or(0.0)
}

fn strtol(v: &str) -> i64 {
    let v = v.trim_start();
    (1..=v.len()).rev().filter(|&n| v.is_char_boundary(n)).find_map(|n| v[..n].parse::<i64>().ok()).unwrap_or(0)
}

/// The simulator's settings, by `client.config` key.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// `backplatesimulationrun`: simulate the backplate entirely.
    pub run: bool,
    /// `backplatesimulatetempandhum`: the room comes from the simulator.
    pub temp_and_hum: bool,
    /// `backplatesimulationhvactype`, absent when None.
    pub hvac_type: Option<i64>,
    pub wires: Vec<String>,
    pub pins: Vec<String>,
    /// `backplatesimulationmodel`.
    pub model: Option<String>,
    pub daily: bool,
    pub inefficiency: Fixed,
    pub actual_humidity: Fixed,
    pub noise: f32,
    pub overundershoot: bool,
    pub radiant: bool,
    pub temperature_min: Fixed,
    pub temperature_max: Fixed,
    pub ambient: Fixed,
    pub start: Fixed,
}

impl Default for Settings {
    /// Stock's own defaults, for a setting the configuration lacks.
    fn default() -> Settings {
        Settings {
            run: false,
            temp_and_hum: false,
            hvac_type: None,
            wires: Vec::new(),
            pins: Vec::new(),
            model: None,
            daily: true,
            inefficiency: 0x10,
            actual_humidity: 0x20000,
            noise: 0.0033,
            overundershoot: false,
            radiant: false,
            temperature_min: 0,
            temperature_max: 0x1d_8000,
            ambient: 0xa_0000,
            start: 0x14_0000,
        }
    }
}

impl Settings {
    /// The settings in stock's configuration files (`<a key="…"
    /// value="…"/>`), a later key replacing an earlier one.
    pub fn parse(config: &str) -> Settings {
        let mut keys = HashMap::new();
        for tag in config.split("<a ").skip(1) {
            let get = |name: &str| tag.split(&format!("{name}=\"")).nth(1).and_then(|r| r.split('"').next());
            if let (Some(k), Some(v)) = (get("key"), get("value")) {
                keys.insert(k.to_string(), v.to_string());
            }
        }
        let mut s = Settings::default();
        let key = |k: &str| keys.get(&format!("backplatesimulation{k}"));
        let number = |k: &str| key(k).map(|v| strtod(v));
        // strtok's tokens: no trimming, empty ones skipped.
        let list = |k: &str| key(k).map(|v| v.split(',').filter(|t| !t.is_empty()).map(str::to_string).collect());
        s.run = key("run").is_some_and(|v| strtol(v) != 0);
        s.temp_and_hum = keys.get("backplatesimulatetempandhum").is_some_and(|v| strtol(v) != 0);
        s.hvac_type = key("hvactype").map(|v| strtol(v));
        if let Some(w) = list("hvacwires") { s.wires = w; }
        if let Some(p) = list("hvacpins") { s.pins = p; }
        s.model = key("model").cloned();
        if let Some(m) = key("mode") { s.daily = m != "hourly"; }
        if let Some(v) = number("buildinginefficiency") { s.inefficiency = fixed(v); }
        if let Some(v) = number("actualhumidity") { s.actual_humidity = fixed(v); }
        if let Some(v) = number("noisemagnitude") { s.noise = v as f32; }
        if let Some(v) = key("overundershoot") { s.overundershoot = strtol(v) != 0; }
        if let Some(v) = key("radiantheating") { s.radiant = strtol(v) != 0; }
        if let Some(v) = number("temperaturemin") { s.temperature_min = fixed(v); }
        if let Some(v) = number("temperaturemax") { s.temperature_max = fixed(v); }
        if let Some(v) = number("ambienttemperature") { s.ambient = fixed(v); }
        if let Some(v) = number("starttemperature") { s.start = fixed(v); }
        s
    }

    /// The wiring simulated (stock's 0x30d928, 0x31b744), as MSG_WIRES and
    /// MSG_PINS: an hvac type from 1 to 19, or for a negative type the
    /// wires and pins listed, both needed; none for type 0, 20 and up, or
    /// no type.
    pub fn wiring(&self) -> Option<([u8; 13], [u8; 15])> {
        let t = self.hvac_type?;
        let (wires, pins): (Vec<&str>, Vec<&str>) = if t >= 0 {
            let names = hvac_type(i32::try_from(t).ok()?)?;
            (names.to_vec(), names.to_vec())
        } else {
            if self.wires.is_empty() || self.pins.is_empty() {
                return None;
            }
            (self.wires.iter().map(String::as_str).collect(), self.pins.iter().map(String::as_str).collect())
        };
        let (mut w, mut p) = ([0u8; 13], [0u8; 15]);
        for name in wires {
            match terminal(name) {
                Some((i, _)) => w[i] = 1,
                None => warn!("backplate simulator: unknown wire {name}"),
            }
        }
        for name in pins {
            match terminal(name) {
                Some((_, i)) => p[i] = 1,
                None => warn!("backplate simulator: unknown pin {name}"),
            }
        }
        Some((w, p))
    }

    /// The model the backplate is shown as, from `backplatesimulationmodel`.
    pub fn model(&self) -> Option<String> {
        self.model.as_deref().map(|m| match m {
            "Backplate-Demo-NonAmber" => "Backplate-5".to_string(),
            "Backplate-Demo-AmberOrAgate" => "Backplate-6".to_string(),
            m => m.to_string(),
        })
    }
}

/// Seconds between steps, and each step's length in the model (stock: 30).
pub const STEP: f64 = 30.0;
const DT: Fixed = 0x1e_0000;
/// The sensor's values before anything writes them, and the humidity the
/// simulation starts at.
const SENSOR_CELSIUS: Fixed = 0x14_0000;
const START_HUMIDITY: Fixed = 0x28_0000;
/// The radiator starts this much above the reading stock takes at start.
const RADIATOR_START: Fixed = 0x2_8000;

pub struct Sim {
    settings: Settings,
    /// The HVAC control's switches on.
    on: Vec<Switch>,
    pub celsius: Fixed,
    pub humidity: Fixed,
    /// Per step: the equipment's temperature and humidity change.
    rate: Fixed,
    humidifying: Fixed,
    /// The radiant heater's own temperature.
    radiator: Fixed,
    /// The over- and undershoot's recent rates, newest first.
    history: [Fixed; 9],
    shifted: f64,
    stepped: f64,
    /// The local time's seconds since midnight, for the daily curve.
    clock: fn() -> u32,
    random: fn() -> u32,
}

fn local_seconds() -> u32 {
    // SAFETY: localtime_r fills the tm it is given.
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        (tm.tm_hour * 3600 + tm.tm_min * 60 + tm.tm_sec) as u32
    }
}

extern "C" {
    /// The C library's random() and srandom(), which stock's noise draws
    /// from.
    fn random() -> libc::c_long;
    fn srandom(seed: libc::c_uint);
}

fn libc_random() -> u32 {
    // SAFETY: random() has no preconditions.
    unsafe { random() as u32 }
}

/// Seeds random() as stock does at startup: from /dev/urandom, or the time.
pub fn seed() {
    use std::io::Read;
    let mut b = [0u8; 4];
    let seed = match std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)) {
        Ok(()) => u32::from_ne_bytes(b),
        // SAFETY: time(NULL) has no preconditions.
        Err(_) => unsafe { libc::time(std::ptr::null_mut()) as u32 },
    };
    // SAFETY: srandom() has no preconditions.
    unsafe { srandom(seed) };
}

impl Sim {
    /// The simulation started at `now` (stock's 0x32bd40): hourly, the room
    /// at the start temperature; daily, at the sensor's 20 °C; humidity at
    /// 40 %, the radiator 2.5 °C above the room.
    pub fn new(settings: Settings, now: f64) -> Sim {
        let celsius = if settings.daily { SENSOR_CELSIUS } else { settings.start };
        Sim {
            celsius,
            humidity: START_HUMIDITY,
            settings,
            on: Vec::new(),
            rate: 0,
            humidifying: 0,
            radiator: celsius + RADIATOR_START,
            history: [0; 9],
            shifted: now,
            stepped: now,
            clock: local_seconds,
            random: libc_random,
        }
    }

    /// The HVAC control's switches now on.
    pub fn set_equipment(&mut self, on: &[Switch]) {
        if self.on != on {
            debug!("backplate simulator: equipment on: {}", if on.is_empty() { "none".to_string() } else { on.iter().map(|s| s.name()).collect::<Vec<_>>().join(", ") });
        }
        self.on = on.to_vec();
    }

    /// What the switches on do (stock's 0x32ba6c): cooling, when any
    /// cooling runs, takes away 0x222 a second for the first stage and
    /// 0x111 for fan cooling and the others; otherwise heating adds 0x222
    /// for each first stage and 0x111 for the others, emergency heat
    /// nothing. A humidifier adds 0x111 to the humidity, else a
    /// dehumidifier takes it away.
    fn equipment(&mut self) {
        use Switch::*;
        let on = |s: Switch| self.on.contains(&s);
        let cooling = [FanCool, Stage1Cool, Stage2Cool, Stage3Cool, AcDehumidifier].into_iter().any(on);
        let heating = [Stage1Heat, Stage2Heat, Stage3Heat, Stage1AltHeat, Stage2AltHeat, AuxHeat, EmergencyHeat].into_iter().any(on);
        let rate: Fixed = if cooling {
            [(FanCool, -0x111), (Stage1Cool, -0x222), (Stage2Cool, -0x111), (Stage3Cool, -0x111)]
                .into_iter().filter(|&(s, _)| on(s)).map(|(_, r)| r).sum()
        } else if heating {
            [(Stage1Heat, 0x222), (Stage2Heat, 0x111), (Stage3Heat, 0x111), (Stage1AltHeat, 0x222), (Stage2AltHeat, 0x111), (AuxHeat, 0x111)]
                .into_iter().filter(|&(s, _)| on(s)).map(|(_, r)| r).sum()
        } else {
            0
        };
        self.rate = mul(rate, DT);
        self.humidifying = if on(Humidifier) {
            mul(0x111, DT)
        } else if on(Dehumidifier) {
            mul(-0x111, DT)
        } else {
            0
        };
    }

    /// The ambient temperature now (stock's FUN_0032b624).
    fn ambient(&self) -> Fixed {
        let s = &self.settings;
        if !s.daily {
            return s.ambient;
        }
        let secs = (self.clock)();
        let hour = secs / 3600;
        let (min, max) = (s.temperature_min, s.temperature_max);
        if hour < 5 {
            return min;
        }
        let span = f64::from(max - min);
        if hour - 5 < 8 {
            let x = (secs as f32 - 14400.0) / 28800.0;
            let a = ((1.0 - x) * 90.0) as f64 * PI / 180.0;
            min + ((1.0 - a.sin()) * span) as Fixed
        } else if hour != 13 {
            let x = (secs as f32 - 46800.0) / 39600.0;
            let a = (x * 90.0) as f64 * PI / 180.0;
            min + (span * a.sin()) as Fixed
        } else {
            max
        }
    }

    fn noise(&self) -> Fixed {
        let r = ((self.random)() & 0x7fff_ffff) as f32 / 2147483648.0;
        mul(fixed(f64::from((r - 0.5 + (r - 0.5)) * self.settings.noise)), DT)
    }

    /// The equipment's effect this step (stock's 0x32b7ec). A radiant
    /// heater stores the heat and gives the room what it radiates; with
    /// over- and undershoot the effect is the mean of the last ten steps'.
    fn heating(&mut self, now: f64) -> Fixed {
        let rate = if self.settings.radiant {
            self.radiator += self.rate;
            if self.celsius < self.radiator {
                let k = |t: Fixed| (f64::from((t + 0x111_2666) >> 16)).powi(4);
                let loss = fixed(f64::from(((k(self.radiator) - k(self.celsius)) * 1.6699999673061572e-12) as f32));
                let given = mul(loss, DT);
                self.radiator -= given;
                given
            } else {
                self.rate
            }
        } else if self.settings.overundershoot {
            // The last nine steps' rates and this one's, averaged over ten;
            // the history moves on after it is summed.
            let sum: Fixed = self.history.iter().sum();
            if now - self.shifted > STEP {
                self.shifted = now;
                self.history.copy_within(0..8, 1);
                self.history[0] = self.rate;
            }
            div(self.rate + sum, 0xa_0000)
        } else {
            self.rate
        };
        rate + self.noise()
    }

    /// Relative humidity's next value (stock's FUN_0032b990).
    fn humidity_step(&self, celsius: Fixed) -> Fixed {
        let t = float(celsius) as f32;
        let saturation = 5.018 + t * 0.32321 + t * 0.0081847 * t + t * 0.00031243 * t * t;
        let target = mul(0x64_0000, div(self.settings.actual_humidity, fixed(f64::from(saturation))));
        mul(self.settings.inefficiency, target - self.humidity) + self.humidifying + self.noise()
    }

    /// One step of the room (stock's 0x32bc18).
    fn step(&mut self, now: f64) {
        self.equipment();
        let drift = mul(mul(self.settings.inefficiency, self.ambient() - self.celsius), DT);
        let celsius = self.celsius + drift + self.heating(now);
        self.humidity += self.humidity_step(celsius);
        self.celsius = celsius;
    }

    /// The room as the simulation holds it: °C and relative humidity.
    pub fn room(&self) -> (f32, f32) {
        (float(self.celsius) as f32, float(self.humidity) as f32)
    }

    /// Steps the room every 30 s from the start; true when it stepped.
    pub fn poll(&mut self, now: f64) -> bool {
        if now - self.stepped < STEP {
            return false;
        }
        self.stepped = now;
        self.step(now);
        debug!("backplate simulator: room {:.2} °C, humidity {:.1} %, ambient {:.2} °C, equipment {:+.3} °C a step",
               float(self.celsius), float(self.humidity), float(self.ambient()), float(self.rate));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_point_is_freetypes() {
        assert_eq!(mul(0x10000, 0x10000), 0x10000);
        assert_eq!(mul(-0x18000, 0x10000), -0x18000);
        assert_eq!(mul(0x8000, 3), 2, "rounds half away from zero");
        assert_eq!(div(0x10000, 0x20000), 0x8000);
        assert_eq!(div(1, 0), 0x7fff_ffff);
    }

    #[test]
    fn settings_read_as_stock_reads_them() {
        let s = Settings::parse(r#"<a key="backplatesimulationhvactype" value="0"/>
            <a key="backplatesimulationhvacwires" value="Rh,W1,Y1,G"/>
            <a key="backplatesimulationmode" value="daily"/>
            <a key="backplatesimulationbuildinginefficiency" value="0.00025"/>
            <a key="backplatesimulationactualhumidity" value="10"/>
            <a key="backplatesimulationoverundershoot" value="0.5"/>
            <a key="backplatesimulationtemperaturemax" value="29.5x"/>
            <a key="backplatesimulationrun" value="0"/>"#);
        assert_eq!(s.wires, ["Rh", "W1", "Y1", "G"]);
        assert_eq!(s.inefficiency, 16, "0.00025 truncates to 16/65536");
        assert_eq!(s.actual_humidity, 10 << 16);
        assert_eq!(s.temperature_max, 0x1d_8000, "strtod stops at the junk");
        assert!(!s.overundershoot, "strtol reads 0.5 as 0");
        assert!(!s.run);
        assert_eq!(s.start, 20 << 16, "absent: stock's default");
        assert_eq!(s.wiring(), None, "type 0 simulates no wiring");
        assert_eq!(hvac_type(7), Some(&["Y1", "G", "O/B", "Rh"][..]));
    }

    #[test]
    fn the_wiring_is_a_types_or_both_lists() {
        let parse = |t: &str, w: &str, p: &str| Settings::parse(&format!(
            r#"<a key="backplatesimulationhvactype" value="{t}"/><a key="backplatesimulationhvacwires" value="{w}"/><a key="backplatesimulationhvacpins" value="{p}"/>"#)).wiring();
        let (w, p) = parse("1", "", "").unwrap();
        assert_eq!(w, [1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
        assert_eq!(p, [1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(parse("20", "W1", "W1"), None);
        assert_eq!(parse("-1", "W1,Rh", ""), None, "both lists are needed");
        let (w, _) = parse("-1", "W1, Rh", "W1").unwrap();
        assert_eq!(w[0], 1);
        assert_eq!(w[10], 0, "\" Rh\" is not a wire");
        assert_eq!(Settings::default().wiring(), None);
    }

    #[test]
    fn the_room_drifts_to_ambient_and_heating_warms_it() {
        let settings = Settings { daily: false, noise: 0.0, ..Settings::default() };
        let mut sim = Sim::new(settings, 0.0);
        assert_eq!(sim.room(), (20.0, 40.0), "hourly starts at the start temperature");
        assert!(!sim.poll(29.0));
        let mut t = 0.0;
        for _ in 0..20 {
            t += STEP;
            assert!(sim.poll(t));
        }
        let before = sim.celsius;
        assert!(before < 20 << 16, "cooling towards 10 °C");
        sim.set_equipment(&[Switch::Fan, Switch::Stage1Heat]);
        for _ in 0..20 {
            t += STEP;
            sim.poll(t);
        }
        // Twenty steps of 0x222 * 30 is 5 °C, less 0.07 °C a step of drift.
        let rise = float(sim.celsius - before);
        assert!((3.3..3.7).contains(&rise), "{rise}");
        sim.set_equipment(&[Switch::Stage1Heat, Switch::Stage1Cool]);
        sim.step(t);
        assert!(sim.rate < 0, "cooling wins over heating");
    }

    #[test]
    fn a_daily_simulation_starts_at_the_sensors_twenty_degrees() {
        let sim = Sim::new(Settings { start: 15 << 16, ..Settings::default() }, 0.0);
        assert_eq!(sim.celsius, 20 << 16);
        assert_eq!(sim.radiator, (22 << 16) + 0x8000);
    }

    #[test]
    fn the_model_names_map_as_stocks() {
        let m = |v: &str| Settings { model: Some(v.into()), ..Settings::default() }.model();
        assert_eq!(m("Backplate-Demo-NonAmber").as_deref(), Some("Backplate-5"));
        assert_eq!(m("Backplate-Demo-AmberOrAgate").as_deref(), Some("Backplate-6"));
        assert_eq!(m("X").as_deref(), Some("X"));
    }
}
