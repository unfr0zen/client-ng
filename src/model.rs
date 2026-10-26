//! The thermostat as the UI sees it, and the settings the menus change.
//!
//! There is no HVAC control yet: the current temperature is fixed and the
//! heating and cooling states follow from it, so the UI can be designed
//! against every state.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// A setting's value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Number(f64),
    Text(String),
}

impl Value {
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Value::Bool(b) => (*b).into(),
            Value::Number(n) => (*n).into(),
            Value::Text(t) => t.clone().into(),
        }
    }

    pub fn from_json(value: &serde_json::Value) -> Option<Value> {
        match value {
            serde_json::Value::Bool(b) => Some(Value::Bool(*b)),
            serde_json::Value::Number(n) => n.as_f64().map(Value::Number),
            serde_json::Value::String(s) => Some(Value::Text(s.clone())),
            _ => None,
        }
    }
}

/// Settings by key. What was set is kept in a JSON file when one is given;
/// defaults come from the menus each start, so a changed default applies.
pub struct Settings {
    values: BTreeMap<String, Value>,
    defaults: BTreeMap<String, Value>,
    file: Option<PathBuf>,
    writer: Option<SettingsWriter>,
    /// Counts the changes, so what is derived from the settings can be kept
    /// until they change.
    revision: u64,
}

enum Save {
    Values(BTreeMap<String, Value>),
    Flush(std::sync::mpsc::Sender<()>),
}

/// How long the writer waits for the changes to stop before it writes,
/// and the longest it holds them: a turn of the wheel changes the target
/// at every detent, and each write goes to the unit's NAND, mounted sync,
/// whose work on the one core would stall the wheel. A flush writes at once.
const WRITE_AFTER: std::time::Duration = std::time::Duration::from_secs(2);
const WRITE_WITHIN: std::time::Duration = std::time::Duration::from_secs(10);

/// Serializes and replaces the settings file off the rendering thread.
/// Dropping the writer drains pending changes before joining it.
pub struct SettingsWriter {
    sender: Option<std::sync::mpsc::Sender<Save>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl SettingsWriter {
    fn new(file: PathBuf) -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            while let Ok(first) = receiver.recv() {
                let (mut values, mut waiting) = (None, Vec::new());
                let take = |message, values: &mut Option<_>, waiting: &mut Vec<_>| match message {
                    Save::Values(v) => *values = Some(v),
                    Save::Flush(done) => waiting.push(done),
                };
                let started = std::time::Instant::now();
                take(first, &mut values, &mut waiting);
                // Until the changes have stopped for a while, or a flush asks
                // for the write now.
                while waiting.is_empty() {
                    let due = WRITE_AFTER.min(WRITE_WITHIN.saturating_sub(started.elapsed()));
                    if due.is_zero() { break }
                    match receiver.recv_timeout(due) {
                        Ok(message) => take(message, &mut values, &mut waiting),
                        Err(_) => break,
                    }
                }
                for message in receiver.try_iter() {
                    take(message, &mut values, &mut waiting);
                }
                if let Some(values) = values {
                    let map: serde_json::Map<String, serde_json::Value> =
                        values.iter().map(|(k, v)| (k.clone(), v.to_json())).collect();
                    let tmp = file.with_extension("tmp");
                    let written = serde_json::to_vec_pretty(&map).map_err(std::io::Error::other)
                        .and_then(|data| std::fs::write(&tmp, data))
                        .and_then(|()| std::fs::rename(&tmp, &file));
                    if let Err(e) = written {
                        error!("settings: {}: not saved: {e}", crate::log::path(&file));
                    }
                }
                for done in waiting { let _ = done.send(()); }
            }
        });
        Self { sender: Some(sender), thread: Some(thread) }
    }
}

impl Drop for SettingsWriter {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() { let _ = thread.join(); }
    }
}

impl Settings {
    /// Where the settings are kept.
    pub fn file(&self) -> Option<&std::path::Path> {
        self.file.as_deref()
    }

    /// The settings saved in `file`, or none yet.
    pub fn load(file: Option<PathBuf>) -> Settings {
        let mut values = BTreeMap::new();
        let data = file.as_ref().and_then(|f| std::fs::read(f).ok().map(|d| (f, d)));
        let saved = data.as_ref().and_then(|(f, data)| serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(data)
            .map_err(|e| warn!("settings: {}: {e}; starting from the defaults", crate::log::path(f))).ok());
        if let Some(saved) = saved {
            for (key, value) in saved {
                if let Some(value) = Value::from_json(&value) {
                    values.insert(key, value);
                }
            }
        }
        match &file {
            Some(f) if data.is_some() => info!("settings: {} ({} set)", crate::log::path(f), values.len()),
            Some(f) => info!("settings: {} (none yet)", crate::log::path(f)),
            None => info!("settings: not kept"),
        }
        Settings { values, defaults: BTreeMap::new(), writer: file.clone().map(SettingsWriter::new), file, revision: 0 }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key).or_else(|| self.defaults.get(key))
    }

    /// The value a key has until it is set.
    pub fn default(&mut self, key: &str, value: Value) {
        if self.defaults.get(key) != Some(&value) {
            self.defaults.insert(key.to_string(), value);
            self.revision += 1;
        }
    }

    /// Changes so far: equal while nothing has changed.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Drops a key's set value, back to its default.
    pub fn forget(&mut self, key: &str) {
        if let Some(was) = self.values.remove(key) {
            debug!("settings: {key}: {} -> default {}", shown(Some(&was)), shown(self.defaults.get(key)));
            self.revision += 1;
            self.save();
        }
    }

    /// Sets a key's value; the engine's own bookkeeping, logged at debug.
    pub fn set(&mut self, key: &str, value: Value) {
        self.put(key, value, crate::log::Level::Debug);
    }

    /// Sets a key's value as someone chose it (the menus, a keyboard, a
    /// dialog), logged at info.
    pub fn change(&mut self, key: &str, value: Value) {
        self.put(key, value, crate::log::Level::Info);
    }

    fn put(&mut self, key: &str, value: Value, level: crate::log::Level) {
        if self.get(key) == Some(&value) {
            return;
        }
        if crate::log::enabled(level) {
            crate::log::write(level, format_args!("settings: {key}: {} -> {}", shown(self.get(key)), shown(Some(&value))));
        }
        self.values.insert(key.to_string(), value);
        self.revision += 1;
        self.save();
    }

    fn save(&self) {
        if let Some(sender) = self.writer.as_ref().and_then(|w| w.sender.as_ref()) {
            let _ = sender.send(Save::Values(self.values.clone()));
        }
    }

    /// Completes queued writes before a requested restart.
    pub fn flush(&self) {
        if let Some(sender) = self.writer.as_ref().and_then(|w| w.sender.as_ref()) {
            let (done, wait) = std::sync::mpsc::channel();
            if sender.send(Save::Flush(done)).is_ok() { let _ = wait.recv(); }
        }
    }

    /// Stops accepting writes while All Settings erases the saved file.
    pub fn take_writer(&mut self) -> Option<SettingsWriter> {
        self.writer.take()
    }

    pub fn text(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Value::Text(t)) => Some(t),
            _ => None,
        }
    }
}

/// A value as the log shows it: on one line, long ones cut short.
fn shown(value: Option<&Value>) -> String {
    let text = match value {
        None => return "unset".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Text(t)) => format!("{t:?}"),
    };
    match text.char_indices().nth(60) {
        Some((at, _)) => format!("{}... ({} chars)", &text[..at], text.chars().count()),
        None => text,
    }
}

/// What the HVAC is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hvac {
    Idle,
    Heating,
    Cooling,
}

impl Hvac {
    pub fn name(self) -> &'static str {
        match self {
            Hvac::Idle => "idle",
            Hvac::Heating => "heating",
            Hvac::Cooling => "cooling",
        }
    }
}

/// The settings key the thermostat's mode lives under.
pub const MODE: &str = "thermostat.mode";
/// The settings key for the displayed temperature scale, "C" or "F".
pub const UNITS: &str = "display.units";
/// The settings key for the click on each detent and press.
pub const CLICK: &str = "display.click";
/// The settings key for the skin drawn: its folder's name.
pub const SKIN: &str = "display.skin";

/// Temperatures in Celsius; the UI converts for display.
pub struct Thermostat {
    pub target: f32,
    pub current: f32,
    /// The heat-cool range's ends.
    pub low: f32,
    pub high: f32,
    /// Relative humidity in percent, when known.
    pub humidity: Option<f32>,
    /// The leaf's thresholds, Celsius, from the device bucket when it
    /// has them (`leaf_threshold_heat`, `leaf_threshold_cool`).
    pub leaf: (Option<f32>, Option<f32>),
    pub can_heat: bool,
    pub can_cool: bool,
}

impl Thermostat {
    pub const MIN: f32 = 9.0;
    pub const MAX: f32 = 32.0;

    pub fn demo() -> Thermostat {
        Thermostat { target: 21.0, current: 20.5, low: 20.0, high: 24.0, humidity: None, leaf: (None, None), can_heat: true, can_cool: true }
    }

    /// Turns the target by wheel detents: half a degree Celsius, or one
    /// degree Fahrenheit, a detent.
    pub fn turn(&mut self, detents: i32, fahrenheit: bool) {
        self.target = stepped(self.target, detents, fahrenheit);
    }

    /// Turns one end of the heat-cool range, as `turn` does the target; the
    /// other end keeps at least `RANGE_GAP` away, pushed along if need be.
    pub fn turn_range(&mut self, side: Side, detents: i32, fahrenheit: bool) {
        let gap = if fahrenheit { RANGE_GAP_F * 5.0 / 9.0 } else { RANGE_GAP };
        match side {
            Side::Low => {
                self.low = stepped(self.low, detents, fahrenheit).min(Self::MAX - gap);
                self.high = self.high.max(self.low + gap);
            }
            Side::High => {
                self.high = stepped(self.high, detents, fahrenheit).max(Self::MIN + gap);
                self.low = self.low.min(self.high - gap);
            }
        }
    }

    pub fn hvac(&self, mode: &str) -> Hvac {
        match mode {
            "heat" if self.target > self.current + 0.25 => Hvac::Heating,
            "cool" if self.target < self.current - 0.25 => Hvac::Cooling,
            "heat-cool" if self.low > self.current + 0.25 => Hvac::Heating,
            "heat-cool" if self.high < self.current - 0.25 => Hvac::Cooling,
            _ => Hvac::Idle,
        }
    }
}

/// The heat-cool range's ends, the heating one and the cooling one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Low,
    High,
}

impl Side {
    pub fn name(self) -> &'static str {
        match self {
            Side::Low => "low",
            Side::High => "high",
        }
    }
}

/// The least the heat-cool range spans, as stock keeps it: degrees Celsius,
/// or Fahrenheit when shown in Fahrenheit.
pub const RANGE_GAP: f32 = 1.5;
pub const RANGE_GAP_F: f32 = 3.0;

/// A temperature moved by wheel detents and kept on the dial: half degrees
/// Celsius, or whole degrees Fahrenheit.
pub fn stepped(celsius: f32, detents: i32, fahrenheit: bool) -> f32 {
    if fahrenheit {
        let f = (to_fahrenheit(celsius).round() + detents as f32).clamp(to_fahrenheit(Thermostat::MIN), to_fahrenheit(Thermostat::MAX));
        (f - 32.0) * 5.0 / 9.0
    } else {
        ((celsius * 2.0).round() / 2.0 + detents as f32 * 0.5).clamp(Thermostat::MIN, Thermostat::MAX)
    }
}

pub fn to_fahrenheit(c: f32) -> f32 {
    c * 9.0 / 5.0 + 32.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wheel_moves_half_degrees_celsius_within_the_limits() {
        let mut t = Thermostat::demo();
        t.turn(3, false);
        assert_eq!(t.target, 22.5);
        t.turn(-100, false);
        assert_eq!(t.target, Thermostat::MIN);
        t.turn(1000, false);
        assert_eq!(t.target, Thermostat::MAX);
    }

    #[test]
    fn fahrenheit_moves_whole_degrees() {
        let mut t = Thermostat::demo();
        t.turn(2, true);
        assert_eq!(to_fahrenheit(t.target).round(), 72.0);
    }

    #[test]
    fn the_range_keeps_its_gap_by_pushing_the_other_end() {
        let mut t = Thermostat::demo();
        t.turn_range(Side::Low, 5, false);
        assert_eq!((t.low, t.high), (22.5, 24.0));
        t.turn_range(Side::Low, 2, false);
        assert_eq!((t.low, t.high), (23.5, 25.0));
        t.turn_range(Side::High, -10, false);
        assert_eq!((t.low, t.high), (18.5, 20.0));
        t.turn_range(Side::High, -100, false);
        assert_eq!((t.low, t.high), (Thermostat::MIN, Thermostat::MIN + RANGE_GAP));
    }

    #[test]
    fn heating_and_cooling_follow_the_mode_and_target() {
        let mut t = Thermostat::demo();
        t.target = 23.0;
        assert_eq!(t.hvac("heat"), Hvac::Heating);
        assert_eq!(t.hvac("cool"), Hvac::Idle);
        t.target = 18.0;
        assert_eq!(t.hvac("cool"), Hvac::Cooling);
        assert_eq!(t.hvac("off"), Hvac::Idle);
    }

    #[cfg(unix)]
    #[test]
    fn a_blocked_settings_write_does_not_block_the_caller() {
        let file = std::env::temp_dir().join(format!("client-settings-blocked-{}.json", std::process::id()));
        let tmp = file.with_extension("tmp");
        let path = std::ffi::CString::new(tmp.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: path is a live, NUL-terminated pathname.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        let (sent, received) = std::sync::mpsc::channel();
        let output = file.clone();
        let caller = std::thread::spawn(move || {
            let mut settings = Settings::load(Some(output));
            settings.set("target", Value::Number(21.0));
            sent.send(()).unwrap();
        });
        // Opening the FIFO for writing waits until a reader opens it.
        let returned = received.recv_timeout(std::time::Duration::from_secs(2));
        let data = std::fs::read(&tmp).unwrap();
        caller.join().unwrap();
        std::fs::remove_file(file).unwrap();
        assert!(returned.is_ok(), "setting a value waited for disk I/O");
        let saved: serde_json::Value = serde_json::from_slice(&data).unwrap();
        assert_eq!(saved["target"].as_f64(), Some(21.0));
    }

    #[test]
    fn queued_settings_keep_the_latest_value_and_forgetting() {
        let file = std::env::temp_dir().join(format!("client-settings-queue-{}.json", std::process::id()));
        let mut settings = Settings::load(Some(file.clone()));
        settings.set("removed", Value::Bool(true));
        for n in 0..100 { settings.set("target", Value::Number(n as f64)); }
        settings.forget("removed");
        settings.flush();
        let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(saved["target"].as_f64(), Some(99.0));
        assert!(saved.get("removed").is_none());
        drop(settings);
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn resetting_settings_cannot_recreate_the_erased_file() {
        let file = std::env::temp_dir().join(format!("client-settings-reset-{}.json", std::process::id()));
        let mut settings = Settings::load(Some(file.clone()));
        settings.set("target", Value::Number(21.0));
        drop(settings.take_writer());
        std::fs::remove_file(&file).unwrap();
        settings.set("target", Value::Number(22.0));
        drop(settings);
        assert!(!file.exists());
    }

    #[test]
    fn settings_survive_a_restart_and_defaults_do_not_override() {
        let file = std::env::temp_dir().join(format!("client-settings-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&file);
        let mut s = Settings::load(Some(file.clone()));
        s.default(MODE, Value::Text("heat".into()));
        s.set(UNITS, Value::Text("F".into()));
        drop(s);
        let mut again = Settings::load(Some(file.clone()));
        again.default(UNITS, Value::Text("C".into()));
        assert_eq!(again.text(UNITS), Some("F"));
        assert_eq!(again.get(MODE), None, "only changed values are saved");
        again.default(MODE, Value::Text("cool".into()));
        assert_eq!(again.text(MODE), Some("cool"));
        drop(again);
        let _ = std::fs::remove_file(&file);
    }
}
