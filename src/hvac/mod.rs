//! Heating and cooling, as stock nlclient runs them (client-ng/BACKPLATE.md).

pub mod balance;
pub mod control;
pub mod furnace;
pub mod headsup;
pub mod humidity;
pub mod overshoot;
pub mod report;
pub mod shortcycle;
pub mod ttt;
pub mod wiring;

use std::time::{Duration, Instant};

use control::{Action, Control, Decision, Equipment, Mode};
use wiring::{Switch, Wiring};

/// Stock's suspend timer's period, the longest it lets the unit sleep.
const MAX_SLEEP: u64 = 300;

/// The thermostat's heating and cooling: decisions, and the circuits
/// they switch.
pub struct Hvac {
    control: Control,
    pub wiring: Option<Wiring>,
    pub equipment: Equipment,
    /// The switches on.
    on: Vec<Switch>,
    pub decision: Option<Decision>,
    /// The lockout last logged: what it held and the seconds it had left,
    /// so one lockout is logged once however often the decision returns
    /// to it.
    logged_lockout: Option<(control::Lockout, f64)>,
    /// The control's clock starts here.
    epoch: Instant,
    /// Whether the targets are the user's or the schedule's, not Eco's,
    /// and whether they were last set by hand.
    pub own_target: bool,
    pub manual_target: bool,
    /// The outside temperature, from the weather.
    pub outdoor: Option<f32>,
    /// Stock's time-to-temperature model.
    pub ttt: ttt::TimeToTemp,
    /// When the room's last reading goes stale, while it has not yet.
    pub room_due: Option<Instant>,
    /// Heat Pump Balance's learning.
    pub balance: balance::Balance,
    /// The wiring as last logged.
    logged_wiring: String,
    /// Stock's short-cycle detection.
    pub short_cycle: shortcycle::ShortCycle,
    /// Stock's furnace shutdown detection.
    pub furnace: furnace::Furnace,
    /// HvacBatteryDrained (stock's ThermostatManager +0x3b9), and whether
    /// it last forced the thermostat to drift.
    pub battery_drained: bool,
    forced_drift: bool,
    /// A system test's request, while one runs (crate::systemtest).
    pub test: Option<control::Test>,
}

impl Default for Hvac {
    fn default() -> Hvac {
        Hvac { control: Control::default(), wiring: None, equipment: Equipment::default(), on: Vec::new(), decision: None, logged_lockout: None, epoch: Instant::now(), own_target: true, manual_target: false, outdoor: None, ttt: ttt::TimeToTemp::default(), room_due: None, balance: balance::Balance::default(), logged_wiring: String::new(), short_cycle: shortcycle::ShortCycle::default(),
              furnace: furnace::Furnace::default(), battery_drained: false, forced_drift: false, test: None }
    }
}

/// The circuits power stealing must leave alone in a `power_steal_mode`
/// (stock's 0x315f44, MSG_DISABLE_PS_WHILE_SWITCHED): all of them when
/// disabled, else the wired heat or cool stages' while heating or cooling.
pub fn power_steal_mask(mode: u8, circuits: &[Option<u8>; 19]) -> u32 {
    use Switch::*;
    let switches: &[Switch] = match mode {
        1 => return u32::MAX,
        2 => &[EmergencyHeat, Stage2AltHeat, Stage1AltHeat, AuxHeat, Stage3Heat, Stage2Heat, Stage1Heat],
        3 => &[Stage3Cool, Stage2Cool, Stage1Cool],
        _ => return 0,
    };
    switches.iter().filter_map(|&s| circuits[s as usize]).filter(|&c| c < 32).fold(0, |m, c| m | 1 << c)
}

/// How long a room reading stays usable.
const ROOM_STALE: Duration = Duration::from_secs(900);

impl Hvac {
    /// A room reading came: it stays usable for 900 s.
    pub fn room_read(&mut self) {
        self.room_due = Some(Instant::now() + ROOM_STALE);
        self.control.stale = false;
    }

    /// The overshoot gate's learning, as stored, and taken back.
    pub fn overshoot(&self) -> String {
        self.control.overshoot.save()
    }

    pub fn load_overshoot(&mut self, text: &str) {
        self.control.overshoot.load(text);
    }

    /// The backplate's wiring with the functions the setup gave it.
    pub fn wiring(&self) -> Option<Wiring> {
        let e = &self.equipment;
        self.wiring.clone().map(|w| Wiring { y2: e.y2, star: e.star, dual_fuel: e.dual_fuel, ..w })
    }

    /// The equipment as stock publishes it (its `has_*` keys, from
    /// 0x2e34d0), once the wiring is known.
    pub fn has(&self) -> Option<[(&'static str, bool); 14]> {
        let w = self.wiring()?;
        let x = |s: Switch| w.exists(s);
        Some([
            ("has_alt_heat", x(Switch::Stage1AltHeat)),
            ("has_aux_heat", x(Switch::AuxHeat)),
            ("has_dehumidifier", x(Switch::Dehumidifier)),
            ("has_dual_fuel", x(Switch::Stage1AltHeat)),
            ("has_emer_heat", x(Switch::EmergencyHeat)),
            ("has_fan", x(Switch::Fan)),
            // Seen true with dual fuel and false with aux on nest-qemu.
            ("has_fossil_fuel", x(Switch::Stage1AltHeat)),
            ("has_heat_pump", x(Switch::Ob)),
            ("has_humidifier", x(Switch::Humidifier)),
            ("has_x2_alt_heat", x(Switch::Stage2AltHeat)),
            ("has_x2_cool", x(Switch::Stage2Cool)),
            ("has_x3_cool", x(Switch::Stage3Cool)),
            ("has_x2_heat", x(Switch::Stage2Heat)),
            ("has_x3_heat", x(Switch::Stage3Heat)),
        ])
    }

    /// Decides on a temperature report, setting the switches on; `fan` is
    /// the speed the fan timer or schedule runs the fan at, while one does.
    pub fn update(&mut self, mode: Mode, room: f32, humidity: Option<f32>, fan: Option<u8>) {
        // A reading older than 900 s is none (stock's 0x4edb5c).
        if self.room_due.is_some_and(|at| Instant::now() >= at) {
            self.room_due = None;
            self.control.stale = true;
            warn!("hvac: local sensor is stale: no room reading for {} s, drifting", ROOM_STALE.as_secs());
        }
        let humidity = humidity.filter(|_| !self.control.stale);
        if self.control.stale {
            debug!("hvac: active temperature is none, force drift");
        }
        // The equipment follows the wiring and the setup's terminal functions.
        if let Some(w) = self.wiring() {
            let e = &mut self.equipment;
            e.has_fan = w.exists(Switch::Fan);
            e.x2_heat = w.exists(Switch::Stage2Heat) || w.exists(Switch::Stage3Heat);
            e.x3_heat = w.exists(Switch::Stage3Heat);
            e.x2_cool = w.exists(Switch::Stage2Cool) || w.exists(Switch::Stage3Cool);
            e.x3_cool = w.exists(Switch::Stage3Cool);
            e.heat_pump = w.exists(Switch::Ob);
            e.aux_heat = w.exists(Switch::AuxHeat);
            e.emergency_heat = w.exists(Switch::EmergencyHeat);
            e.alt_stages = if w.exists(Switch::Stage2AltHeat) { 2 } else { u8::from(w.exists(Switch::Stage1AltHeat)) };
            let wired: Vec<String> = Switch::ALL.iter().filter(|&&s| w.exists(s))
                .map(|&s| w.circuit(s).map_or_else(|| s.name().to_string(), |c| format!("{} on {c}", s.name()))).collect();
            let wired = wired.join(", ");
            if wired != self.logged_wiring {
                info!("hvac: wiring: {}", if wired.is_empty() { "nothing" } else { &wired });
                debug!("hvac: equipment: heat stages {}, cool stages {}, heat pump {}, aux {}, emergency {}, alternate stages {}, fan {} (y2 {} star {} dual fuel {})",
                       1 + u8::from(e.x2_heat) + u8::from(e.x3_heat), 1 + u8::from(e.x2_cool) + u8::from(e.x3_cool), e.heat_pump, e.aux_heat,
                       e.emergency_heat, e.alt_stages, e.has_fan, w.y2, w.star, w.dual_fuel);
                self.logged_wiring = wired;
            }
            self.wiring = Some(w);
        }
        // Airwave needs a cool stage and the fan wired (stock's 0x4db53c).
        let wired = |s: Switch| self.wiring.as_ref().is_some_and(|w| w.circuit(s).is_some());
        self.control.fan_cooling = self.equipment.fan_cooling && wired(Switch::Fan) && wired(Switch::Stage1Cool);
        self.control.safety = self.equipment.safety;
        self.control.own_target = self.own_target;
        self.control.manual_target = self.manual_target;
        let mut config = self.equipment.humidity.clone();
        config.cooling = self.wiring.as_ref().is_some_and(|w| w.circuit(Switch::Stage1Cool).is_some());
        self.control.humidistat.config = config;
        // SAFETY: localtime_r fills the tm it is given.
        self.control.day = unsafe {
            let now = libc::time(std::ptr::null_mut());
            let mut tm: libc::tm = std::mem::zeroed();
            libc::localtime_r(&now, &mut tm);
            (tm.tm_hour * 3600 + tm.tm_min * 60 + tm.tm_sec) as u32
        };
        let e = &self.equipment;
        let stages = |x2: bool, x3: bool| if x3 { 3 } else if x2 { 2 } else { 1 };
        self.control.stages = (stages(e.x2_heat, e.x3_heat), stages(e.x2_cool, e.x3_cool));
        self.control.heat_pump = e.heat_pump.then(|| {
            let mut hp = control::HeatPump::new(e.aux_heat, e.compressor_lockout, e.aux_lockout, e.savings);
            // Balance's lockout, learned from the runs (0x4dfae8).
            if e.aux_heat {
                if let Some(l) = self.balance.lockout(e.savings, e.compressor_lockout) {
                    hp.aux_lockout = Some(l);
                }
            }
            hp
        });
        self.control.dual_fuel = (e.alt_stages > 0).then_some(control::DualFuel { stages: e.alt_stages, breakpoint: e.dual_fuel_breakpoint, override_: e.dual_fuel_override });
        self.control.outdoor = self.outdoor;
        if self.control.test != self.test {
            match self.test {
                Some(t) => info!("hvac: system test requests {:?}, humidity {:?}, fan {:?}", t.operation, t.humidity, t.fan),
                None => info!("hvac: system test request withdrawn"),
            }
        }
        self.control.test = self.test;
        let now = self.epoch.elapsed().as_secs_f64();
        // Armed with a cool stage wired (stock's compressor_heads_up_enabled
        // is on unless set off; nothing sets it here).
        self.control.heads_up.enabled = wired(Switch::Stage1Cool);
        if !self.control.stale {
            self.control.heads_up.sample(room, now);
        }
        let d = self.control.decide(mode, room, humidity, now);
        if !self.control.stale && self.test.is_none() {
            self.ttt.update(&d, room, now, self.control.tracker_state());
            if self.equipment.aux_heat {
                let previous = self.decision.map_or(control::Action::Drift, |p| p.action);
                self.balance.update(&d, previous, room, self.outdoor, now);
            }
        }
        let same = |a: &Decision| (a.controller, a.action, a.operation, a.lockout.map(|l| (l.0, l.1)));
        if self.decision.is_none_or(|old| old.humidity != d.humidity) {
            info!("hvac: Hstat Op:{:?}", d.humidity);
        }
        // What runs changing is news; the controller deciding the same
        // thing (a target crossing the room during a lockout, say) is not.
        if self.decision.is_none_or(|old| (old.action, old.operation) != (d.action, d.operation)) {
            info!("hvac: Tstat:{} Act:{:?} Op:{:?}", d.controller.name(), d.action, d.operation);
        } else if self.decision.is_some_and(|old| same(&old) != same(&d)) {
            debug!("hvac: Tstat:{} Act:{:?} Op:{:?}", d.controller.name(), d.action, d.operation);
        }
        if let Some((lockout, op, left)) = d.lockout {
            // A new lockout: none logged, another kind, or more time left.
            if self.logged_lockout.is_none_or(|(kind, logged)| kind != lockout || left > logged + 1.0) {
                info!("hvac: Lockout:{} Op:{op:?} Sec:{left:.1}", lockout.name());
            } else if self.decision.is_none_or(|old| old.lockout.is_none()) {
                debug!("hvac: Lockout:{} Op:{op:?} Sec:{left:.1}", lockout.name());
            }
            self.logged_lockout = Some((lockout, left));
        }
        self.decision = Some(d);
        let mut want = control::switches(d.operation, d.humidity, d.controller == control::Controller::CoolToDry, &self.equipment);
        // Airwave and the fan timer or schedule run the fan whatever the
        // system does, the timer and schedule not during a safety run
        // (0x51c42c).
        let fan = fan.filter(|_| !(d.controller == control::Controller::SafetyTemperatures && d.action != control::Action::Drift));
        // A system test runs the fan only at its own request.
        let fan = match self.test {
            Some(t) => t.fan.filter(|_| d.controller == control::Controller::SystemTest),
            None => fan,
        };
        if (fan.is_some() || d.fan) && self.equipment.has_fan && !want.contains(&Switch::Fan) {
            if !self.on.contains(&Switch::Fan) {
                debug!("hvac: fan runs for {}", if d.fan { "airwave" } else { "the fan timer or schedule" });
            }
            want.insert(0, Switch::Fan);
        }
        // A multi-speed fan runs at the stage's speed, or the speed set
        // for heating and cooling, and the fan timer and schedule each at
        // their own (stock's 0x51c1c0, 0x51c42c; its HVAC input carries
        // both, 0x37eb64); each speed is its own switch.
        let e = &self.equipment;
        let test_speed = self.test.and(fan).filter(|&s| s > 1);
        if let Some(speed) = test_speed {
            // A test of a fan speed runs that speed's own switch.
            let switch = [Switch::Fan, Switch::Stage2Fan, Switch::Stage3Fan][usize::from(speed.min(3) - 1)];
            if wired(switch) {
                want.retain(|s| *s != Switch::Fan);
                want.insert(0, switch);
            }
        } else if e.fan_stages > 1 && want.contains(&Switch::Fan) {
            let stage = want.iter().filter(|s| matches!(s, Switch::Stage1Heat | Switch::Stage2Heat | Switch::Stage3Heat
                | Switch::Stage1Cool | Switch::Stage2Cool | Switch::Stage3Cool | Switch::Stage1AltHeat | Switch::Stage2AltHeat)).count() as u8;
            let speed = if stage > 0 { e.fan_heat_cool_speed.unwrap_or(stage) } else { fan.unwrap_or(e.fan_timer_speed) }.clamp(1, e.fan_stages);
            let switch = [Switch::Fan, Switch::Stage2Fan, Switch::Stage3Fan][usize::from(speed - 1)];
            if switch != Switch::Fan && wired(switch) {
                if !self.on.contains(&switch) {
                    debug!("hvac: fan at speed {speed} of {} ({} stages running)", e.fan_stages, stage);
                }
                want.retain(|s| *s != Switch::Fan);
                want.insert(0, switch);
            }
        }
        // O/B as stock sets it (0x330150): its orientation while cooling,
        // the opposite while a first or second heat stage runs, else off.
        let cooling = want.iter().any(|s| matches!(s, Switch::Stage1Cool | Switch::Stage2Cool | Switch::Stage3Cool));
        let heating = want.iter().any(|s| matches!(s, Switch::Stage1Heat | Switch::Stage2Heat));
        let idle_hold = !cooling && !heating && self.equipment.ob_persistence && self.on.contains(&Switch::Ob);
        if (cooling && self.equipment.ob_cool) || (!cooling && heating && !self.equipment.ob_cool) || idle_hold {
            if !self.on.contains(&Switch::Ob) {
                debug!("hvac: O/B energised: {} with O/B on {}", if cooling { "cooling" } else { "heating" }, if self.equipment.ob_cool { "cool" } else { "heat" });
            }
            want.push(Switch::Ob);
        }
        // A drained battery with power stealing off for a circuit that
        // would switch: the thermostat drifts, whatever decided it (stock's
        // Rethink, 0x37e764).
        let circuits = self.circuits();
        let switched = want.iter().filter_map(|&s| circuits[s as usize]).filter(|&c| c < 32).fold(0u32, |m, c| m | 1 << c);
        let forced = self.battery_drained && switched & power_steal_mask(self.equipment.power_steal, &circuits) != 0;
        if forced {
            if !self.forced_drift {
                warn!("hvac: battery too low while power steal is disabled - forcing drift (wanted {})",
                      want.iter().map(|s| s.name()).collect::<Vec<_>>().join(", "));
            }
            want.clear();
            self.decision = Some(Decision::new(control::Controller::None, Action::Drift, control::Operation::Off));
        } else if self.forced_drift {
            info!("hvac: battery drift lifted");
        }
        self.forced_drift = forced;
        for &s in &self.on {
            if !want.contains(&s) {
                info!("hvac: {} off", s.name());
            }
        }
        for &s in &want {
            if !self.on.contains(&s) {
                info!("hvac: {} on", s.name());
            }
        }
        if want != self.on {
            debug!("hvac: switches on: {}", if want.is_empty() { "none".to_string() } else { want.iter().map(|s| s.name()).collect::<Vec<_>>().join(", ") });
        }
        self.on = want;
    }

    /// What the control has learned, as stored: the heat and cool bands,
    /// each side's average change and cycle minutes, then Airwave's offset,
    /// fan run and its variance.
    pub fn learned(&self) -> [f32; 9] {
        let c = &self.control;
        let [h, k] = c.band_models;
        [c.bands[0], c.bands[1], h.change, h.minutes, k.change, k.minutes, c.airwave_offset, c.airwave_fan, c.airwave_fan_var]
    }

    /// Takes back what was learned, as stock loads it: bands outside
    /// 0.16-0.51 °C are left at their default.
    pub fn set_learned(&mut self, l: [f32; 9]) {
        let c = &mut self.control;
        if l[6] > 0.0 && l[7] > 0.0 && l[8] > 0.0 {
            (c.airwave_offset, c.airwave_fan, c.airwave_fan_var) = (l[6], l[7], l[8]);
        }
        for i in 0..2 {
            if (0.16..=0.51).contains(&l[i]) {
                c.bands[i] = l[i];
            }
            if l[2 + 2 * i] > 0.0 && l[3 + 2 * i] > 0.0 {
                c.band_models[i].change = l[2 + 2 * i];
                c.band_models[i].minutes = l[3 + 2 * i];
            }
        }
    }

    /// Seconds until the room reaches the running cycle's target, when
    /// stock's model shows it.
    pub fn time_to_target(&self, mode: Mode, room: f32) -> Option<f64> {
        let heating = match self.decision?.action {
            Action::Heat => true,
            Action::Cool => false,
            _ => return None,
        };
        let target = match (mode, heating) {
            (Mode::Heat(t) | Mode::Emergency(t) | Mode::Range(t, _), true) | (Mode::Cool(t) | Mode::Range(_, t), false) => t,
            _ => return None,
        };
        self.ttt.estimate(room, target, heating)
    }

    /// The circuit of each switch, once the wiring is known.
    pub fn circuits(&self) -> [Option<u8>; 19] {
        self.wiring.as_ref().map_or([None; 19], |w| w.circuits())
    }

    /// What should wake the unit if it sleeps now: the room leaving the
    /// thresholds where the control would act, or the next of `soonest`, a
    /// lockout's end and stock's suspend timer, which comes round every
    /// 300 s.
    pub fn wake(&self, mode: Mode, soonest: Option<Instant>) -> crate::backplate::link::Wake {
        let (low, high) = self.control.wake(mode);
        let now = Instant::now();
        let cycle = MAX_SLEEP - self.epoch.elapsed().as_secs() % MAX_SLEEP;
        let due = [self.deadline(), soonest].into_iter().flatten().map(|at| at.saturating_duration_since(now).as_secs()).fold(cycle, u64::min);
        crate::backplate::link::Wake { steps: vec![(0, low, high)], max_sleep: due as u16, humidity: self.control.humidistat.wake(),
                                         als: (0, 0xffff), vergence: (0, 0, 0) }
    }

    /// How long until the system starts, while a lockout holds back the
    /// heating or cooling the control wants.
    pub fn starts_in(&self) -> Option<Duration> {
        let d = self.decision?;
        let (_, wanted, _) = d.lockout?;
        if d.action != Action::Drift || wanted == control::Operation::Off {
            return None;
        }
        Some(self.deadline()?.saturating_duration_since(Instant::now()))
    }

    /// How long the heating or cooling runs on, while a lockout holds it
    /// past where the control would stop it.
    pub fn delayed_for(&self) -> Option<Duration> {
        let d = self.decision?;
        let (_, wanted, _) = d.lockout?;
        if !matches!(d.action, Action::Heat | Action::Cool) || wanted != control::Operation::Off {
            return None;
        }
        Some(self.deadline()?.saturating_duration_since(Instant::now()))
    }

    /// The lockout a held system counts down, and when it ends: the
    /// compressor's while its timer runs, as stock checks that one first
    /// (0x1cd760), else the one holding it.
    pub fn lockout(&self) -> Option<(control::Lockout, Instant)> {
        let (kind, _, _) = self.decision?.lockout?;
        match self.control.compressor_lockout_end() {
            Some(end) => Some((control::Lockout::Compressor, self.epoch + Duration::from_secs_f64(end))),
            None => Some((kind, self.deadline()?)),
        }
    }

    /// How long the lockout controller holds a system test's request back,
    /// while it does.
    pub fn test_held(&self) -> Option<Duration> {
        self.test?;
        let d = self.decision?;
        if d.controller != control::Controller::Lockout {
            return None;
        }
        Some(self.deadline()?.saturating_duration_since(Instant::now()))
    }

    /// The compressor heads-up's state.
    pub fn heads_up(&self) -> headsup::State {
        self.control.heads_up.state
    }

    /// When a lockout ends and the control decides again.
    pub fn deadline(&self) -> Option<Instant> {
        let control = self.control.deadline().map(|at| self.epoch + Duration::from_secs_f64(at));
        [control, self.room_due].into_iter().flatten().min()
    }

    /// Whether Airwave is cooling with the fan alone (`fan_cooling_state`).
    pub fn fan_cooling(&self) -> bool {
        self.decision.is_some_and(|d| d.fan && self.is_on(Switch::Fan))
    }

    /// The stage the system runs, 1 to 3 (1 when idle).
    pub fn stage(&self) -> u8 {
        use control::Operation::*;
        match self.decision.map(|d| d.operation) {
            Some(Stage2Heat | Stage2Cool) => 2,
            Some(Stage3Heat | Stage3Cool) => 3,
            _ => 1,
        }
    }

    /// Short-cycle detection's update at `now` (wall seconds): heating
    /// is a first stage, alternate or auxiliary heat switch on (stock's
    /// 0x3fa350), cooling the first cool stage (0x3fa3a4); `powered` the
    /// HVAC power.
    pub fn short_cycle_update(&mut self, now: f64, powered: bool) {
        let heating = [Switch::Stage1Heat, Switch::Stage1AltHeat, Switch::AuxHeat].iter().any(|s| self.on.contains(s));
        self.short_cycle.update(now, heating, self.on.contains(&Switch::Stage1Cool), powered);
    }

    /// Furnace shutdown detection's update at `now` (wall seconds): the
    /// furnace is the first heat stage, or a heat pump's first alternate
    /// stage; `powered` the HVAC power. Some(true) when its alert rises,
    /// Some(false) when it goes.
    pub fn furnace_update(&mut self, now: f64, powered: bool) -> Option<bool> {
        let furnace = if self.equipment.heat_pump { Switch::Stage1AltHeat } else { Switch::Stage1Heat };
        let heating = self.on.contains(&furnace);
        self.furnace.update(now, heating, powered)
    }

    /// The switches on.
    pub fn switches_on(&self) -> &[Switch] {
        &self.on
    }

    /// Whether a switch is on.
    pub fn is_on(&self, s: Switch) -> bool {
        self.on.contains(&s)
    }

    /// What the system is doing, for the screen: "heating", "cooling" or
    /// "idle".
    /// Cool to Dry runs the air conditioner while the thermostat drifts:
    /// stock shows it cooling.
    pub fn state(&self) -> &'static str {
        match self.decision.map(|d| (d.action, d.controller)) {
            Some((Action::Heat, _)) => "heating",
            Some((Action::Cool, _) | (_, control::Controller::CoolToDry)) => "cooling",
            _ => "idle",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_multi_speed_fan_runs_at_the_set_speed_and_the_timer_at_its_own() {
        // W1, Y1, C, Rh, G, and Y2 as G2.
        let mut pins = [0; 15];
        for p in [0, 1, 2, 4, 5, 9] {
            pins[p] = 1;
        }
        let mut h = Hvac { wiring: Some(Wiring { pins, y2: -1, star: -1, dual_fuel: false }), ..Hvac::default() };
        h.equipment.y2 = 3;
        h.equipment.fan_stages = 2;
        h.room_read();
        h.update(Mode::Cool(20.0), 26.0, None, None);
        assert_eq!(h.decision.map(|d| d.operation), Some(control::Operation::Stage2Cool));
        assert!(h.is_on(Switch::Stage2Fan) && !h.is_on(Switch::Fan), "auto: stage two, fan two");
        h.equipment.fan_heat_cool_speed = Some(1);
        h.update(Mode::Cool(20.0), 26.0, None, None);
        assert!(h.is_on(Switch::Fan) && !h.is_on(Switch::Stage2Fan), "set to the first speed");
        h.update(Mode::Off, 26.0, None, Some(2));
        assert_eq!(h.on, [Switch::Stage2Fan], "the timer or schedule at its speed");
        h.update(Mode::Off, 26.0, None, Some(1));
        assert_eq!(h.on, [Switch::Fan]);
    }

    #[test]
    fn a_drained_battery_drifts_what_power_stealing_cannot_carry() {
        // nest-qemu's wiring: Rh, C, W1, Y1, G.
        let mut pins = [0; 15];
        for p in [0, 1, 2, 4, 5] {
            pins[p] = 1;
        }
        let mut h = Hvac { wiring: Some(Wiring { pins, y2: -1, star: -1, dual_fuel: false }), ..Hvac::default() };
        h.equipment.power_steal = 2;
        h.battery_drained = true;
        h.room_read();
        h.update(Mode::Heat(22.0), 18.0, None, None);
        assert!(h.on.is_empty(), "heating forced to drift: {:?}", h.on);
        assert_eq!(h.state(), "idle");
        // Cooling is not what power stealing is off for.
        h.update(Mode::Cool(20.0), 26.0, None, None);
        assert!(h.is_on(Switch::Stage1Cool));
        // Back to normal power stealing, the heat runs.
        h.equipment.power_steal = 0;
        h.update(Mode::Heat(22.0), 18.0, None, None);
        assert!(h.is_on(Switch::Stage1Heat));
    }

    /// nest-qemu's wiring (Rh, C, W1, Y1, G), Y2 as `y2` and STAR as `star`
    /// when wired.
    fn tested(y2: i32, star: i32) -> Hvac {
        let mut pins = [0; 15];
        for p in [0, 1, 2, 4, 5] {
            pins[p] = 1;
        }
        if y2 >= 0 {
            pins[9] = 1;
        }
        if star >= 0 {
            pins[11] = 1;
        }
        let mut h = Hvac { wiring: Some(Wiring { pins, y2, star, dual_fuel: false }), ..Hvac::default() };
        h.equipment.y2 = y2;
        h.equipment.star = star;
        h.room_read();
        h
    }

    fn request(operation: control::Operation, humidity: humidity::Op, fan: Option<u8>) -> Option<control::Test> {
        Some(control::Test { operation, humidity, fan })
    }

    #[test]
    fn a_system_test_switches_its_stage_through_the_control() {
        use control::Operation;
        let mut h = tested(-1, -1);
        h.test = request(Operation::Stage1Heat, humidity::Op::Off, None);
        h.update(Mode::Off, 22.0, None, None);
        assert_eq!(h.on, [Switch::Stage1Heat], "W1 alone, whatever the mode");
        assert_eq!(h.test_held(), None);
        // Withdrawn at once, the heat runs its minimum cycle.
        h.test = None;
        h.update(Mode::Off, 22.0, None, None);
        assert_eq!(h.on, [Switch::Stage1Heat]);
        assert_eq!(h.decision.map(|d| d.controller), Some(control::Controller::Lockout));
        // Cooling asked for meanwhile is held back, and says how long.
        h.test = request(Operation::Stage1Cool, humidity::Op::Off, None);
        h.update(Mode::Off, 22.0, None, None);
        assert!(!h.is_on(Switch::Stage1Cool));
        assert!(h.test_held().is_some_and(|d| d > std::time::Duration::from_secs(170)));
    }

    #[test]
    fn a_system_test_runs_the_fan_at_its_speed_and_the_humidity_equipment() {
        use control::Operation;
        // Y2 as G2.
        let mut h = tested(3, -1);
        h.test = request(Operation::Off, humidity::Op::Off, Some(2));
        h.update(Mode::Off, 22.0, None, Some(1));
        assert_eq!(h.on, [Switch::Stage2Fan], "the second speed's switch, not the timer's speed");
        h.test = request(Operation::Off, humidity::Op::Off, Some(1));
        h.update(Mode::Off, 22.0, None, None);
        assert_eq!(h.on, [Switch::Fan]);
        // A humidifier on STAR.
        let mut h = tested(-1, 1);
        h.test = request(Operation::Off, humidity::Op::Humidify, None);
        h.update(Mode::Off, 22.0, Some(40.0), None);
        assert_eq!(h.on, [Switch::Humidifier]);
        // An AC-integrated dehumidifier: the compressor, its fan and both
        // dehumidifier switches.
        let mut h = tested(-1, 2);
        h.test = request(Operation::Stage1Cool, humidity::Op::AcIntegratedDehumidify, None);
        h.update(Mode::Off, 22.0, Some(60.0), None);
        assert_eq!(h.on[..4], [Switch::Fan, Switch::Stage1Cool, Switch::Dehumidifier, Switch::AcDehumidifier]);
        // The fan timer does not run during a test of another stage.
        let mut h = tested(-1, 1);
        h.test = request(Operation::Off, humidity::Op::Humidify, None);
        h.update(Mode::Off, 22.0, Some(40.0), Some(1));
        assert!(!h.is_on(Switch::Fan));
    }
}
