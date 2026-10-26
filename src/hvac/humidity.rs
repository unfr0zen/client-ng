//! Humidity control, as stock nlclient's humidistat does it (its energy
//! manager's 0x51cb24 config, 0x51cce4 decision, the Cool to Dry state
//! machine at 0x5169e8 and 0x516520): a humidifier or a stand-alone
//! dehumidifier on the STAR terminal around `target_humidity`, and Cool to
//! Dry, which runs the air conditioner to dry a humid room.

/// Humidifier types (`humidifier_type`): steam runs on its own, bypass only
/// while heating.
pub const STEAM: u8 = 1;
pub const BYPASS: u8 = 2;
/// Dehumidifier types (`dehumidifier_type`): stand-alone, or integrated in
/// the air conditioner (it dehumidifies only through Cool to Dry).
pub const STAND_ALONE: u8 = 1;
pub const AC_INTEGRATED: u8 = 2;

/// How far past the target humidity a run goes (%).
const HYSTERESIS: f32 = 5.0;
/// Cool to Dry: how far below the cool target it may cool, the lowest it
/// cools a room without an integrated dehumidifier, the humidity it acts
/// at, its unconditional first run, the slowest drying it accepts (% a
/// minute) and its rests after a run: integrated, ineffective (three
/// times), then longer.
const OVERCOOL: f32 = 1.67;
const FLOOR: f32 = 23.89;
const C2D_HUMIDITY: f32 = 75.0;
const C2D_RUN: f64 = 600.0;
const C2D_RATE: f32 = 0.03;
const C2D_REST: (f64, f64, f64) = (900.0, 1800.0, 3600.0);
const C2D_TRIES: u8 = 3;

/// What the humidistat does (stock's HumidistatOperation).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Op {
    #[default]
    Off,
    Humidify,
    Dehumidify,
    AcIntegratedDehumidify,
}

/// The equipment and settings it works from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Config {
    /// 0 none, STEAM or BYPASS; 0 none, STAND_ALONE or AC_INTEGRATED.
    pub humidifier: u8,
    pub dehumidifier: u8,
    /// `target_humidity` (%), when enabled.
    pub target: Option<f32>,
    /// The quiet time (`humidity_control_lockout_*`), seconds of the day.
    pub quiet: Option<(u32, u32)>,
    /// `auto_dehum_enabled`.
    pub cool_to_dry: bool,
    /// A cool stage is wired.
    pub cooling: bool,
}

/// Cool to Dry's state: idle, running since `start` from `rh`, or resting
/// after a run.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Dry {
    #[default]
    Idle,
    Running { start: f64, rh: f32 },
    Resting { until: f64 },
}

#[derive(Debug, Default)]
pub struct Humidistat {
    pub config: Config,
    op: Op,
    dry: Dry,
    /// Ineffective Cool to Dry runs in a row.
    failures: u8,
    /// Cool to Dry's start threshold, when it may run.
    dry_start: Option<f32>,
}

/// The humidistat's decision: its operation, and whether Cool to Dry runs
/// the first cool stage.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Decision {
    pub op: Op,
    pub cool_to_dry: bool,
}

/// The thermostat's targets, as the humidistat sees them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Targets {
    pub heat: Option<f32>,
    pub cool: Option<f32>,
    /// The mode is off, or cool (not heat-cool).
    pub off: bool,
    pub cool_mode: bool,
}

impl Humidistat {
    /// Decides after the thermostat: `heating`/`idle` are its action, `room`
    /// and `rh` the readings, `day` the seconds of the day, `compressor_rest`
    /// whether the compressor is held off.
    #[allow(clippy::too_many_arguments)]
    pub fn decide(&mut self, t: Targets, heating: bool, cooling: bool, room: f32, rh: Option<f32>, day: u32, compressor_rest: bool, now: f64) -> Decision {
        if t.off {
            self.op = Op::Off;
            self.dry = Dry::Idle;
            self.dry_start = None;
            return Decision::default();
        }
        let c = self.config.clone();
        let quiet = c.quiet.is_some_and(|(a, b)| if a <= b { (a..b).contains(&day) } else { day >= a || day < b });
        let humidifier = c.target.is_some() && (c.humidifier == STEAM || (c.humidifier == BYPASS && !t.cool_mode));
        let dehumidifier = c.target.is_some() && c.dehumidifier == STAND_ALONE;
        let was = self.op;
        self.op = match (rh, c.target) {
            (Some(rh), Some(target)) if !quiet && humidifier && !cooling && (c.humidifier == STEAM || heating) => {
                let limit = if was == Op::Humidify { target + HYSTERESIS } else { target };
                if rh < limit { Op::Humidify } else { Op::Off }
            }
            (Some(rh), Some(target)) if !quiet && dehumidifier => {
                let limit = if was == Op::Dehumidify { target - HYSTERESIS } else { target };
                if rh > limit { Op::Dehumidify } else { Op::Off }
            }
            _ => Op::Off,
        };
        if self.op != was {
            debug!("hvac: humidistat: humidity {} against target {}{}: {:?}", super::control::opt(rh), super::control::opt(c.target),
                   if quiet { " (quiet time)" } else { "" }, self.op);
        }
        let mut d = Decision { op: self.op, cool_to_dry: false };
        if self.op == Op::Off {
            d.cool_to_dry = self.cool_to_dry(t, heating || cooling, room, rh, compressor_rest, now);
            if d.cool_to_dry && c.dehumidifier == AC_INTEGRATED {
                d.op = Op::AcIntegratedDehumidify;
            }
        }
        d
    }

    /// Cool to Dry (0x5169e8 config, 0x516520 stop): from a humid room in
    /// cool or heat-cool, while the thermostat drifts, down to its floor.
    fn cool_to_dry(&mut self, t: Targets, active: bool, room: f32, rh: Option<f32>, compressor_rest: bool, now: f64) -> bool {
        let c = &self.config;
        let integrated = c.dehumidifier == AC_INTEGRATED;
        let start = if integrated {
            c.target
        } else {
            let threshold = if c.humidifier != 0 { c.target.map_or(C2D_HUMIDITY, |h| C2D_HUMIDITY.max(h + 15.0)) } else { C2D_HUMIDITY };
            Some(threshold.clamp(50.0, 90.0))
        };
        let floor = t.cool.map(|cool| {
            let f = if integrated { cool - OVERCOOL } else { (cool - OVERCOOL).max(FLOOR) };
            t.heat.map_or(f, |h| f.max(h + OVERCOOL))
        });
        let enabled = c.cool_to_dry && t.cool.is_some() && (c.cooling || integrated);
        self.dry_start = start.filter(|_| enabled && floor.is_some());
        let (Some(start), Some(floor), Some(rh), true) = (start, floor, rh, enabled) else {
            self.dry = Dry::Idle;
            return false;
        };
        let stop = start - HYSTERESIS;
        if let Dry::Resting { until } = self.dry {
            if now < until {
                return false;
            }
            self.dry = Dry::Idle;
        }
        match self.dry {
            Dry::Idle if !active && !compressor_rest && room >= floor && rh > start => {
                debug!("hvac: cool to dry starts: humidity {rh:.1} % over {start:.1} %, room {room:.2} °C at or above its floor {floor:.2}");
                self.dry = Dry::Running { start: now, rh };
                true
            }
            Dry::Running { start: began, rh: from } => {
                let minutes = ((now - began) / 60.0) as f32;
                let ineffective = now - began >= C2D_RUN
                    && (integrated || room < floor || (minutes > 0.0 && (from - rh) / minutes <= C2D_RATE));
                if active || room < floor || ineffective || rh < stop {
                    let effective = rh < stop && !ineffective;
                    debug!("hvac: cool to dry stops: {}, humidity {from:.1} % to {rh:.1} % in {minutes:.1} min",
                           if active { "the thermostat runs" } else if rh < stop { "dry enough" } else if room < floor { "room at its floor" } else { "ineffective" });
                    self.dry = if effective {
                        self.failures = 0;
                        Dry::Idle
                    } else if integrated {
                        Dry::Resting { until: now + C2D_REST.0 }
                    } else if self.failures < C2D_TRIES {
                        self.failures += 1;
                        Dry::Resting { until: now + C2D_REST.1 }
                    } else {
                        self.failures = 0;
                        Dry::Resting { until: now + C2D_REST.2 }
                    };
                    return false;
                }
                true
            }
            _ => false,
        }
    }

    /// The humidity that should wake a sleeping unit, low and high
    /// (0x51cee4): a run's end, or where one would start; Cool to Dry's
    /// stop while it runs, its start while idle.
    pub fn wake(&self) -> (Option<f32>, Option<f32>) {
        let c = &self.config;
        match (self.op, c.target) {
            (Op::Humidify, Some(t)) => return (None, Some(t + HYSTERESIS)),
            (Op::Dehumidify, Some(t)) => return (Some(t - HYSTERESIS), None),
            (_, Some(t)) if c.humidifier != 0 => return (Some(t), None),
            (_, Some(t)) if c.dehumidifier == STAND_ALONE => return (None, Some(t)),
            _ => {}
        }
        match self.dry {
            Dry::Running { .. } => (self.dry_start.map(|s| s - HYSTERESIS), None),
            Dry::Idle => (None, self.dry_start),
            Dry::Resting { .. } => (None, None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAT: Targets = Targets { heat: Some(20.0), cool: None, off: false, cool_mode: false };
    const COOL: Targets = Targets { heat: None, cool: Some(24.0), off: false, cool_mode: true };

    #[test]
    fn a_steam_humidifier_runs_below_the_target_to_five_above() {
        let mut h = Humidistat { config: Config { humidifier: STEAM, target: Some(40.0), ..Config::default() }, ..Humidistat::default() };
        assert_eq!(h.decide(HEAT, false, false, 20.0, Some(40.0), 0, false, 0.0).op, Op::Off);
        assert_eq!(h.decide(HEAT, false, false, 20.0, Some(39.9), 0, false, 1.0).op, Op::Humidify);
        assert_eq!(h.decide(HEAT, false, false, 20.0, Some(44.9), 0, false, 2.0).op, Op::Humidify);
        assert_eq!(h.decide(HEAT, false, false, 20.0, Some(45.0), 0, false, 3.0).op, Op::Off);
        h.config.humidifier = BYPASS;
        assert_eq!(h.decide(HEAT, false, false, 20.0, Some(30.0), 0, false, 4.0).op, Op::Off, "bypass needs heat");
        assert_eq!(h.decide(HEAT, true, false, 20.0, Some(30.0), 0, false, 5.0).op, Op::Humidify);
        h.config.quiet = Some((0, 3600));
        assert_eq!(h.decide(HEAT, true, false, 20.0, Some(30.0), 60, false, 6.0).op, Op::Off, "quiet time");
    }

    #[test]
    fn cool_to_dry_cools_a_humid_room_to_its_floor_and_rests_when_it_fails() {
        let mut h = Humidistat { config: Config { cool_to_dry: true, cooling: true, ..Config::default() }, ..Humidistat::default() };
        assert!(!h.decide(COOL, false, false, 24.5, Some(75.0), 0, false, 0.0).cool_to_dry);
        assert!(h.decide(COOL, false, false, 24.5, Some(76.0), 0, false, 1.0).cool_to_dry);
        // Dried too slowly after ten minutes: a 30-minute rest.
        assert!(!h.decide(COOL, false, false, 24.2, Some(75.9), 0, false, 601.0).cool_to_dry);
        assert!(!h.decide(COOL, false, false, 24.5, Some(80.0), 0, false, 1000.0).cool_to_dry);
        assert!(h.decide(COOL, false, false, 24.5, Some(80.0), 0, false, 2402.0).cool_to_dry);
        // Below the floor (75 °F) it stops.
        assert!(!h.decide(COOL, false, false, 23.8, Some(80.0), 0, false, 2500.0).cool_to_dry);
    }
}
