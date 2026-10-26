//! Deciding to heat or cool, as stock nlclient's `nest::energy` does
//! (client-ng/BACKPLATE.md): on each temperature report, the safety
//! temperatures first, whatever the mode; then the band around the
//! target says whether the system runs (its learning band: start below
//! target - band, keep going below target + band), the overshoot gate ends
//! a heat or cool cycle early, Airwave ends a long cooling cycle's
//! compressor early and cools on with the fan, the lockout controller holds the equipment
//! to its minimum cycles and the compressor off for its lockout, the
//! equipment helper turns the action into an operation, and the operation
//! into switches.

use super::humidity::{self, Humidistat};
use super::wiring::Switch;

/// What the thermostat is doing (stock's Action).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Drift,
    Heat,
    Cool,
}

/// How it does it (stock's Operation, the stages it drives so far).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    None,
    Off,
    Stage1Heat,
    Stage2Heat,
    Stage3Heat,
    Stage1Cool,
    Stage2Cool,
    Stage3Cool,
    /// A heat pump's compressor stages with auxiliary heat, auxiliary heat
    /// alone, and emergency heat (stock's operations 8 to 11).
    Stage1HeatAux,
    Stage2HeatAux,
    AuxOnly,
    EmergencyHeat,
    /// A dual-fuel system's alternate heat, one or two stages (stock's 12
    /// and 13), without the compressor.
    Stage1AltHeat,
    Stage2AltHeat,
}

/// Which of stock's controllers made the decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Controller {
    None,
    Airwave,
    LearningBand,
    Lockout,
    Overshoot,
    SafetyTemperatures,
    CoolToDry,
    EmergencyHeat,
    Overeager,
    CompressorHeadsUp,
    SystemTest,
}

impl Controller {
    pub fn name(self) -> &'static str {
        match self {
            Controller::None => "NONE",
            Controller::Airwave => "AIRWAVE",
            Controller::LearningBand => "LEARNING_BAND",
            Controller::Lockout => "LOCKOUT",
            Controller::Overshoot => "OVERSHOOT",
            Controller::SafetyTemperatures => "SAFETY_TEMPERATURES",
            Controller::CoolToDry => "COOL_TO_DRY",
            Controller::EmergencyHeat => "EMERGENCY_HEAT",
            Controller::Overeager => "OVEREAGER",
            Controller::CompressorHeadsUp => "COMPRESSOR_HEADS_UP",
            Controller::SystemTest => "SYSTEM_TEST",
        }
    }
}

/// Why the lockout controller holds the system (stock's lockout types).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lockout {
    /// The compressor stays off for a while after it stops.
    Compressor,
    /// A cycle, on or off, runs for a minimum time.
    MinimumCycle,
}

impl Lockout {
    pub fn name(self) -> &'static str {
        match self {
            Lockout::Compressor => "COMPRESSOR",
            Lockout::MinimumCycle => "MINIMUM_CYCLE",
        }
    }
}

/// The band's half-width when nothing has been learned: 0x63d7 in 16.16
/// fixed point, 0.39 °C (0.7 °F).
pub const DEFAULT_BAND: f32 = 0x63d7 as f32 / 65536.0;
/// A heat pump's settings (stock's energy config at +0x298, built by
/// 0x37eb64): whether it has auxiliary heat, the outdoor temperature below
/// which the compressor is locked out (`heat_pump_comp_threshold`) and
/// above which aux is (`heat_pump_aux_threshold`), how far into a cycle aux
/// may start, the cycle time before it steps up, and aux's offset below
/// the target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeatPump {
    pub aux: bool,
    pub compressor_lockout: Option<f32>,
    pub aux_lockout: Option<f32>,
    pub aux_delay: f64,
    pub upstage: f64,
    pub aux_offset: f32,
}

impl HeatPump {
    /// With `heatpump_savings` (0 max savings, 1 balanced, 2 max comfort,
    /// 3 off) and aux, Heat Pump Balance sets the aux lockout, delay,
    /// upstage time and offset from its defaults (0x4e00d4), the lockout
    /// at least the compressor's.
    pub fn new(aux: bool, compressor_lockout: Option<f32>, aux_lockout: Option<f32>, savings: u8) -> HeatPump {
        let mut hp = HeatPump { aux, compressor_lockout, aux_lockout, aux_delay: 900.0, upstage: 3600.0, aux_offset: 0.0 };
        let balance = match savings {
            0 => Some((1.7, 2700.0, 7200.0, 1.11)),
            1 => Some((4.5, 1800.0, 5400.0, 0.55)),
            2 => Some((7.2, 900.0, 3600.0, 0.0)),
            _ => None,
        };
        if let (true, Some((lockout, delay, upstage, offset))) = (aux, balance) {
            let lockout: f32 = compressor_lockout.map_or(lockout, |c: f32| c.max(lockout));
            hp = HeatPump { aux_lockout: Some(lockout), aux_delay: delay, upstage, aux_offset: offset, ..hp };
        }
        hp
    }
}

/// A dual-fuel system (stock's energy config +0x24..+0x3f): its alternate
/// heat's stages, the outdoor temperature below which it is used
/// (`dual_fuel_breakpoint`, -1 °C when unset), and the override
/// (`dual_fuel_breakpoint_override`: 0 none, 1 always alternate, 2 always
/// the heat pump).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DualFuel {
    pub stages: u8,
    pub breakpoint: f32,
    pub override_: u8,
}

/// Without an outdoor temperature, alternate heat takes over at the top
/// compressor stage after 2 hours of the cycle, or 15 minutes into the
/// stage with the room up less than 1 °C (0x4ec380).
const ALT_CYCLE: f64 = 7200.0;
const ALT_STAGE: f64 = 900.0;
const ALT_RISE: f32 = 1.0;

/// How far below its threshold aux counts as satisfied from where its
/// stage began, and how long aux runs before that is checked again.
const AUX_SATISFIED: f32 = 0.3;
const AUX_RECHECK: f64 = 60.0;
/// Emergency heat's fixed band (0x51c0b4).
const EMERGENCY_BAND: f32 = DEFAULT_BAND;

/// Stock's band learning (energy+0x8e8, 0x519774): each cycle's
/// temperature change and length feed averages (α 0.02, from 0.78 °C over
/// 10 minutes, at most 120); the band follows their rate, heat with k = 3
/// and cool with k = 5, and changes at most every 6 hours.
const BAND_ALPHA: f32 = 0.02;
const BAND_DEFAULT: (f32, f32) = (0.78, 10.0);
const BAND_MINUTES_MAX: f32 = 120.0;
const BAND_HOLD: f64 = 6.0 * 3600.0;

/// A side's learned cycles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandModel {
    /// The average temperature change (°C) and cycle length (minutes).
    pub change: f32,
    pub minutes: f32,
    /// The last cycle's, fed in when the next starts.
    pending: Option<(f32, f32)>,
    /// When the band last changed.
    changed_at: Option<f64>,
}

impl Default for BandModel {
    fn default() -> BandModel {
        BandModel { change: BAND_DEFAULT.0, minutes: BAND_DEFAULT.1, pending: None, changed_at: None }
    }
}

impl BandModel {
    /// The band the averages give: slow cycles get a narrow band.
    fn band(&self, k: f32) -> f32 {
        let rate = self.change / self.minutes.max(f32::EPSILON);
        let run = self.minutes;
        if rate < 0.01133 && run > 30.0 {
            0.17
        } else if rate < 0.0373 && run > 15.0 {
            0.28
        } else if rate < BAND_DEFAULT.0 / k && run > k {
            DEFAULT_BAND
        } else {
            0.5
        }
    }
}

/// The widest band the overshoot gate counts (stock's 0x4e0f70).
pub const BAND_MAX: f32 = 0.54;
/// How far past a safety temperature the system runs once it started
/// (measured on nest-qemu: heat from 5.0 °C to 6.0 °C).
pub const SAFETY_BAND: f32 = 1.0;
/// How far inside a safety temperature the last safety run is remembered
/// (stock's 0x51e228, 0x40133333).
pub const SAFETY_HOLD: f32 = 2.3;
/// The lockout controller's defaults (stock's 0x51d920, in microseconds
/// there): the minimum cycle without and with a compressor, and how long
/// the compressor stays off once stopped.
pub const MINIMUM_CYCLE: f64 = 180.0;
pub const MINIMUM_COMPRESSOR_CYCLE: f64 = 300.0;
pub const COMPRESSOR_LOCKOUT: f64 = 150.0;
/// Airwave (stock's 0x4db53c update, 0x4db7e4 decision, 0x4db944
/// bookkeeping; constants at 0x663f14): how long the compressor must have
/// run, the most humid room it acts in (%), the bounds of the offset above
/// the band's bottom at which it stops the compressor, how far past that
/// the fan stops, and the bounds of the fan's run (hours).
pub const AIRWAVE_RUN: f64 = 300.0;
pub const AIRWAVE_HUMIDITY: f32 = 45.0;
pub const AIRWAVE_OFFSET: (f32, f32) = (0.139, 0.56);
pub const AIRWAVE_RISE: f32 = 0.056;
pub const AIRWAVE_FAN: (f32, f32) = (0.017, 0.17);
/// Airwave's offset and fan run before it has learned any, as stock runs
/// on nest-qemu: 0.5 °F, and six minutes.
pub const AIRWAVE_DEFAULT: (f32, f32) = (0.5 / 1.8, 0.1);

/// Stock's MultistageHelper (0x4dcef8 update, 0x4dd304 decision): a gap
/// to the target past which a cycle starts at stage 2 or 3, how long a
/// stage runs before stepping up, how far the room may move the wrong way
/// since the cycle began before it does, and the longest heat and cool
/// cycle before it does anyway.
pub const STAGE_GAPS: (f32, f32) = (1.83, 3.9);
pub const STAGE_TIME: f64 = 900.0;
pub const STAGE_FALLBACK: f32 = 0.56;
pub const STAGE_CYCLE: (f64, f64) = (3600.0, 7200.0);

/// Stock's cycle tracker (energy+0x180; 0x511404 state, 0x511518
/// bookkeeping): when and at what temperature the cycle and its stage
/// began, and the targets' state, 2 when a manual or scheduled change put
/// the room on the calling side.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Tracker {
    state: u8,
    targets: (Option<f32>, Option<f32>),
    cycle: Option<(f64, f32)>,
    stage: Option<(f64, f32)>,
}

impl Operation {
    /// The stage it runs, 0 for none.
    pub fn stage(self) -> u8 {
        match self {
            Operation::Stage1Heat | Operation::Stage1Cool => 1,
            Operation::Stage2Heat | Operation::Stage2Cool => 2,
            Operation::Stage3Heat | Operation::Stage3Cool => 3,
            _ => 0,
        }
    }

    /// A heat pump's stage: its compressor stages, then the one with aux.
    fn heat_pump_stage(self) -> u8 {
        match self {
            Operation::Stage1Heat => 1,
            Operation::Stage2Heat | Operation::Stage1HeatAux => 2,
            Operation::Stage2HeatAux => 3,
            _ => 0,
        }
    }

    fn heat(stage: u8) -> Operation {
        [Operation::Stage1Heat, Operation::Stage2Heat, Operation::Stage3Heat][usize::from(stage.clamp(1, 3) - 1)]
    }

    fn cool(stage: u8) -> Operation {
        [Operation::Stage1Cool, Operation::Stage2Cool, Operation::Stage3Cool][usize::from(stage.clamp(1, 3) - 1)]
    }

    /// Whether the operation runs a compressor (stock's 0x4e7b68): the
    /// cool stages and a heat pump's stages with aux always, its first two
    /// heat stages with a heat pump.
    pub fn compressor(self, heat_pump: bool) -> bool {
        match self {
            Operation::Stage1Cool | Operation::Stage2Cool | Operation::Stage3Cool | Operation::Stage1HeatAux | Operation::Stage2HeatAux => true,
            Operation::Stage1Heat | Operation::Stage2Heat => heat_pump,
            _ => false,
        }
    }
}

/// A temperature or humidity for the log, "none" when there is none.
pub(super) fn opt(v: Option<f32>) -> String {
    v.map_or_else(|| "none".to_string(), |v| format!("{v:.2}"))
}

/// A system test's request (stock's systemtestlayer, through globals
/// +0x1498 and +0x149c): the operation, humidifier or dehumidifier and fan
/// speed it runs in place of the band's decision. The safety temperatures
/// and the lockout controller still come first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Test {
    pub operation: Operation,
    pub humidity: humidity::Op,
    pub fan: Option<u8>,
}

impl Test {
    /// What the thermostat does for it: heat or cool for a stage, else drift.
    pub fn action(&self) -> Action {
        use Operation::*;
        match self.operation {
            Stage1Heat | Stage2Heat | Stage3Heat | Stage1HeatAux | Stage2HeatAux | AuxOnly | EmergencyHeat | Stage1AltHeat | Stage2AltHeat => Action::Heat,
            Stage1Cool | Stage2Cool | Stage3Cool => Action::Cool,
            None | Off => Action::Drift,
        }
    }
}

/// The thermostat's mode, as the band sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Off,
    Heat(f32),
    /// Emergency heat to a target (stock's mode 4).
    Emergency(f32),
    Cool(f32),
    Range(f32, f32),
}

/// One decision: what stock logs as `Tstat:… Act:… Op:…`, and with a
/// lockout `Lockout:… Op:… Sec:…`: what it holds back and for how long.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decision {
    pub controller: Controller,
    pub action: Action,
    pub operation: Operation,
    pub lockout: Option<(Lockout, Operation, f64)>,
    /// The fan runs on its own: Airwave cooling with it.
    pub fan: bool,
    /// The humidistat's operation.
    pub humidity: humidity::Op,
}

impl Decision {
    pub const fn new(controller: Controller, action: Action, operation: Operation) -> Decision {
        Decision { controller, action, operation, lockout: None, fan: false, humidity: humidity::Op::Off }
    }
}

/// The equipment the setup described (the device bucket): the `has_*`
/// flags follow the wiring once the backplate reports it.
#[derive(Clone, Debug, PartialEq)]
pub struct Equipment {
    pub has_fan: bool,
    /// The fan's speeds (`fan_capabilities`: stage1 to stage3), the speed
    /// heating and cooling run it at (`fan_heat_cool_speed`, None for
    /// auto: the stage's own), the fan timer's (`fan_timer_speed`) and the
    /// fan schedule's (`fan_schedule_speed`).
    pub fan_stages: u8,
    pub fan_heat_cool_speed: Option<u8>,
    pub fan_timer_speed: u8,
    pub fan_schedule_speed: u8,
    /// Second and third stages of heat and cool (`has_x2_heat`, …).
    pub x2_heat: bool,
    pub x3_heat: bool,
    pub x2_cool: bool,
    pub x3_cool: bool,
    /// A heat pump (O/B), with auxiliary heat on W2/AUX (`has_aux_heat`)
    /// and emergency heat (`has_emer_heat`).
    pub heat_pump: bool,
    pub aux_heat: bool,
    pub emergency_heat: bool,
    /// Per switch (`Switch as usize`): how its heat is delivered
    /// (`*_delivery`: 0 forced air, 1 in-floor radiant, 2 radiator, 3
    /// electric strip), its source (`*_source`: 0 gas, 1 electric, 2 oil, 3
    /// LP, 4 geothermal, 5 pellets, 6 district heating) and the setup's fan
    /// type; -1 unknown, as stock starts.
    pub delivery: [i8; 19],
    pub source: [i8; 19],
    pub fan_type: [i8; 19],
    /// Airwave is on (`fan_cooling_enabled`).
    pub fan_cooling: bool,
    /// O/B is energised to cool (`ob_orientation` O, stock's default).
    pub ob_cool: bool,
    /// `power_steal_mode`: 0 normal, 1 disabled, 2 disabled while heating,
    /// 3 disabled while cooling (stock's 0x0cf554; 4 for an unknown one).
    pub power_steal: u8,
    /// The safety temperatures, when enabled (`lower_safety_temp`,
    /// `upper_safety_temp` and their `_enabled`).
    pub safety: (Option<f32>, Option<f32>),
    /// The humidistat's equipment and settings, and whether the fan runs
    /// with the humidifier and with the dehumidifier.
    pub humidity: humidity::Config,
    pub humidifier_fan: bool,
    pub dehumidifier_fan: bool,
    /// The Y2 terminal's function (`y2_type`: 0 heating and cooling, 1
    /// heating, 2 cooling, 3 G2, -1 none set), the STAR terminal's
    /// (`star_type`: 0 emergency heat, 1 humidifier, 2 dehumidifier, 3 W3,
    /// 4 water heater, 5 G3, 6 O/B, 7 Y2, 8 W2, -1 none set), a heat
    /// pump's W terminals as a furnace (`dual_fuel_selected`), and the
    /// dehumidifier's orientation reversed
    /// (`dehumidifier_orientation_selected`).
    pub y2: i32,
    pub star: i32,
    pub dual_fuel: bool,
    /// The dual-fuel breakpoint (°C) and override (0 none, 1 always
    /// alternate, 2 always the heat pump), and the alternate heat's stages
    /// the wiring gives.
    pub dual_fuel_breakpoint: f32,
    pub dual_fuel_override: u8,
    pub alt_stages: u8,
    pub dehumidifier_reversed: bool,
    /// The heat pump's outdoor lockouts (`heat_pump_comp_threshold`,
    /// `heat_pump_aux_threshold`, when enabled), `heatpump_savings` (0 max
    /// savings, 1 balanced, 2 max comfort, 3 off), and whether O/B holds
    /// its state while idle (`ob_persistence`).
    pub compressor_lockout: Option<f32>,
    pub aux_lockout: Option<f32>,
    pub savings: u8,
    pub ob_persistence: bool,
    /// The backplate's own compressor lockout, 0x89's last field: the low
    /// 16 bits of `compressor_lockout_timeout`, as stock sends it.
    pub backplate_lockout: u16,
}

impl Equipment {
    /// Whether a heat switch needs the thermostat to run the fan (stock's
    /// 0x2f018c): forced air from gas by the setup's fan type, forced air
    /// from electricity, or an electric strip.
    pub fn heat_fan(&self, s: Switch) -> bool {
        let i = s as usize;
        match (self.delivery[i], self.source[i]) {
            (0, 0) => self.fan_type[i] == 0,
            (0, source) => source == 1,
            (delivery, _) => delivery == 3,
        }
    }

    /// Whether an operation runs the fan (stock's fan flags, 0x37eb64, and
    /// 0x51c1c0): conventional cooling always, heat by its stage; a heat
    /// pump's stages, cooling included, by its heat stages', and aux,
    /// emergency and alternate heat by theirs.
    pub fn op_fan(&self, op: Operation) -> bool {
        use Operation::*;
        let f = |s: Switch| self.heat_fan(s);
        if !self.heat_pump {
            return match op {
                Stage1Heat => f(Switch::Stage1Heat),
                Stage2Heat => f(Switch::Stage2Heat),
                Stage3Heat => f(Switch::Stage3Heat),
                Stage1Cool | Stage2Cool | Stage3Cool => true,
                _ => false,
            };
        }
        match op {
            Stage1Heat | Stage1Cool => f(Switch::Stage1Heat),
            Stage2Heat | Stage2Cool => f(Switch::Stage2Heat),
            Stage1HeatAux => f(Switch::Stage1Heat) || f(Switch::AuxHeat),
            Stage2HeatAux => f(Switch::Stage2Heat) || f(Switch::AuxHeat),
            AuxOnly => f(Switch::AuxHeat),
            EmergencyHeat => f(Switch::EmergencyHeat),
            Stage1AltHeat => f(Switch::Stage1AltHeat),
            Stage2AltHeat => f(Switch::Stage2AltHeat),
            _ => false,
        }
    }
}

impl Default for Equipment {
    fn default() -> Equipment {
        // Stock's defaults: 40 °F below, nothing above.
        Equipment { has_fan: true, fan_stages: 1, fan_heat_cool_speed: None, fan_timer_speed: 1, fan_schedule_speed: 1, x2_heat: false, x3_heat: false, x2_cool: false, x3_cool: false, heat_pump: false, aux_heat: false,
                    emergency_heat: false, delivery: [-1; 19], source: [-1; 19], fan_type: [-1; 19], fan_cooling: true, ob_cool: true, power_steal: 0, safety: (Some((40.0 - 32.0) / 1.8), None),
                    humidity: humidity::Config::default(), humidifier_fan: false, dehumidifier_fan: false, y2: -1, star: -1, dual_fuel: false, dual_fuel_breakpoint: -1.0, dual_fuel_override: 0, alt_stages: 0, dehumidifier_reversed: false,
                    compressor_lockout: None, aux_lockout: None, savings: 2, ob_persistence: false, backplate_lockout: 0 }
    }
}

/// Where Airwave is in a cooling cycle.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Airwave {
    Idle,
    /// The compressor started at `start`; it stops at or below `stop`, the
    /// band's bottom (`floor`) plus the offset.
    Cooling { start: f64, floor: f32, stop: f32 },
    /// The fan cools on alone until `until`, the room reaching the floor,
    /// or rising past the stop by AIRWAVE_RISE.
    Fan { floor: f32, stop: f32, until: f64 },
}

/// Airwave's learning under way: from when and what room the fan took
/// over, when the fan stopped, and the lowest room seen and when.
#[derive(Clone, Copy, Debug, PartialEq)]
struct AirwaveTrack {
    start: f64,
    from: f32,
    ended: Option<f64>,
    low: Option<(f64, f32)>,
}

/// The lockout controller's state (stock's 0x51d5b0 update, 0x51d948
/// decision, 0x51dc94 bookkeeping).
#[derive(Default)]
struct Lockouts {
    /// When the timers were last counted down.
    at: Option<f64>,
    /// What ran before the running action started.
    before: Option<Action>,
    /// Seconds left of the running cycle's minimum, and of the compressor's.
    cycle: Option<f64>,
    compressor: Option<f64>,
    /// The last decision held the compressor off.
    held: bool,
}

pub struct Control {
    /// The heat and cool bands, and what they are learned from.
    pub bands: [f32; 2],
    pub band_models: [BandModel; 2],
    last: Decision,
    mode: Option<Mode>,
    lockouts: Lockouts,
    airwave: Airwave,
    /// Airwave's learned offset (°C) and fan run (hours), before bounds,
    /// and the fan run's variance.
    pub airwave_offset: f32,
    pub airwave_fan: f32,
    pub airwave_fan_var: f32,
    airwave_track: Option<AirwaveTrack>,
    /// Whether Airwave may act (it is on and there is a fan).
    pub fan_cooling: bool,
    /// The safety temperatures, low and high, when enabled.
    pub safety: (Option<f32>, Option<f32>),
    /// The last safety run's action, kept while the room stays within
    /// SAFETY_HOLD of its safety temperature (stock's helper +0x20, held
    /// by +0x24).
    safety_run: Option<Action>,
    /// The most heat and cool stages the equipment has.
    pub stages: (u8, u8),
    /// Whether a change of target counts as the user's or the schedule's
    /// (not Eco's) for starting a cycle at a higher stage.
    pub own_target: bool,
    tracker: Tracker,
    pub humidistat: Humidistat,
    /// Whether the targets were last set by hand (stock's MANUAL source),
    /// and OVEREAGER's state (1 heat, 2 cool, 3 running on, 4 off) with
    /// the room temperatures that end it, high and low.
    pub manual_target: bool,
    overeager: (u8, Option<f32>, Option<f32>),
    /// A heat pump, and the outside temperature from the weather.
    pub heat_pump: Option<HeatPump>,
    pub dual_fuel: Option<DualFuel>,
    pub outdoor: Option<f32>,
    /// The local time's seconds of the day, for the humidistat's quiet time.
    pub day: u32,
    /// The room's reading is too old to use (stock's "Local sensor is
    /// stale."): the thermostat drifts.
    pub stale: bool,
    /// What the overshoot gate has learned.
    pub overshoot: super::overshoot::Learner,
    /// COMPRESSOR_HEADS_UP.
    pub heads_up: super::headsup::HeadsUp,
    /// A system test running, in place of the mode's decisions.
    pub test: Option<Test>,
    /// The decision as last logged, and when; what the helpers noted
    /// on the way to the one being made.
    explained: Option<(String, f64)>,
    notes: std::cell::RefCell<Vec<String>>,
}

impl Default for Control {
    fn default() -> Control {
        Control {
            bands: [DEFAULT_BAND; 2],
            band_models: [BandModel::default(); 2],
            last: Decision::new(Controller::None, Action::Drift, Operation::Off),
            mode: None,
            lockouts: Lockouts::default(),
            airwave: Airwave::Idle,
            airwave_offset: AIRWAVE_DEFAULT.0,
            airwave_fan: AIRWAVE_DEFAULT.1,
            airwave_fan_var: 0.00028,
            airwave_track: None,
            fan_cooling: false,
            safety: (None, None),
            safety_run: None,
            stages: (1, 1),
            own_target: true,
            tracker: Tracker::default(),
            humidistat: Humidistat::default(),
            manual_target: false,
            overeager: (0, None, None),
            heat_pump: None,
            dual_fuel: None,
            outdoor: None,
            day: 0,
            stale: false,
            overshoot: super::overshoot::Learner::default(),
            heads_up: super::headsup::HeadsUp::default(),
            test: None,
            explained: None,
            notes: Default::default(),
        }
    }
}

impl Control {
    /// Decides on a temperature report, or when a lockout ends, at `now`
    /// seconds on a monotonic clock.
    pub fn decide(&mut self, mode: Mode, room: f32, humidity: Option<f32>, now: f64) -> Decision {
        let l = &mut self.lockouts;
        let elapsed = l.at.map(|at| now - at);
        l.at = Some(now);
        for t in [&mut l.cycle, &mut l.compressor] {
            *t = match (elapsed, *t) {
                (Some(e), Some(left)) if left - e > 0.0 => Some(left - e),
                _ => None,
            };
        }
        // A new mode or target is acted on at once, minimum cycle or not.
        let changed = self.mode.is_some_and(|m| m != mode);
        if changed {
            debug!("hvac: mode or target now {mode:?}: acted on at once, minimum cycle or not");
        }
        self.mode = Some(mode);
        let running = self.last;
        let cools = matches!(mode, Mode::Cool(_) | Mode::Range(..));
        if !(self.fan_cooling && cools) {
            self.airwave = Airwave::Idle;
        }
        // Airwave may stop the compressor once it has run its time, and no
        // minimum cycle holds it, in a room dry enough.
        let early = match self.airwave {
            Airwave::Cooling { start, .. } => running.action == Action::Cool && running.operation != Operation::Off
                && now >= start + AIRWAVE_RUN && self.lockouts.cycle.is_none()
                && humidity.is_none_or(|h| h <= AIRWAVE_HUMIDITY),
            _ => false,
        };
        self.track_overeager(mode, room);
        self.track_targets(mode, room);
        let hp = self.heat_pump.is_some();
        // COMPRESSOR_HEADS_UP's checks (0x4de490), before the controllers.
        let (heat_t, cool_t) = match mode {
            Mode::Heat(t) | Mode::Emergency(t) => (Some(t), None),
            Mode::Cool(t) => (None, Some(t)),
            Mode::Range(l, h) => (Some(l), Some(h)),
            Mode::Off => (None, None),
        };
        if !self.stale {
            let r = super::headsup::Room { room, heat: heat_t, cool: cool_t, safety: self.safety, outdoor: self.outdoor };
            self.heads_up.before(running.action, r, now);
        }
        // Whether the last safety run is still remembered (0x51e228).
        let safety_hold = !self.stale && self.safety_bounds(mode).is_some_and(|(low, high)| match self.safety_run {
            Some(Action::Heat) => low.is_some_and(|t| room < t + SAFETY_HOLD),
            Some(Action::Cool) => high.is_some_and(|t| room > t - SAFETY_HOLD),
            _ => false,
        });
        // A system test's request, after the safety temperatures.
        let tested = self.test.filter(|_| !self.stale)
            .map(|t| self.safety(room, mode).unwrap_or(Decision::new(Controller::SystemTest, t.action(), t.operation)));
        let wanted = match mode {
            // No room temperature (0x4f76ac): drift, the lockouts and the
            // fan still followed.
            _ if self.stale => Decision::new(Controller::None, Action::Drift, Operation::Off),
            _ if tested.is_some() => tested.unwrap_or(running),
            // Emergency heat (0x51c0b4): its own fixed band, no safety
            // temperatures, overshoot gate or staging.
            Mode::Emergency(t) => {
                let heat = if running.operation == Operation::EmergencyHeat { room < t + EMERGENCY_BAND } else { room <= t - EMERGENCY_BAND };
                if heat {
                    Decision::new(Controller::EmergencyHeat, Action::Heat, Operation::EmergencyHeat)
                } else {
                    Decision::new(Controller::EmergencyHeat, Action::Drift, Operation::Off)
                }
            }
            _ => {
                let safety = self.safety(room, mode);
                if let Some(s) = safety {
                    self.note(format!("safety temperatures {}: low {} high {}", match s.action { Action::Heat => "heat", _ => "cool" },
                                      opt(self.safety.0), opt(self.safety.1)));
                }
                let wanted = safety.unwrap_or_else(|| self.band(mode, room, early, now));
                let staged = self.stage_up(wanted, running, mode, room, now);
                if staged.operation != wanted.operation {
                    let t = self.tracker;
                    self.note(format!("staged {:?} to {:?}: cycle {} s, stage {} s, room {:+.2} °C since the cycle began, targets state {}",
                                      wanted.operation, staged.operation, t.cycle.map_or(0, |(at, _)| (now - at) as i64), t.stage.map_or(0, |(at, _)| (now - at) as i64),
                                      t.cycle.map_or(0.0, |(_, r)| room - r), t.state));
                }
                if let Some(n) = self.heads_up.note.take() {
                    self.note(n);
                }
                // Heads-up after the band and safety chain (0x4df1cc).
                let d = self.heads_up.control(staged, hp, now);
                if d.controller == Controller::CompressorHeadsUp && staged.controller != Controller::CompressorHeadsUp {
                    self.note(format!("compressor heads-up {:?} stops {:?}", self.heads_up.state, staged.operation));
                }
                d
            }
        };
        let mut d = self.lock(wanted, running, changed);
        if let (Controller::Lockout, Some((kind, op, left))) = (d.controller, d.lockout) {
            self.note(format!("lockout {} holds {:?} back for {left:.0} s (wanted {} {:?})", kind.name(), op, wanted.controller.name(), wanted.action));
        }
        // A heat pump's compressor going straight from heating to cooling,
        // or back, rests first (0x51d948).
        let reversing = running.operation.compressor(hp) && d.operation.compressor(hp)
            && matches!((running.action, d.action), (Action::Heat, Action::Cool) | (Action::Cool, Action::Heat));
        if reversing {
            debug!("hvac: heat pump reversing from {:?} to {:?}: the compressor rests {COMPRESSOR_LOCKOUT} s", running.action, d.action);
            self.lockouts.compressor = Some(COMPRESSOR_LOCKOUT);
            d = Decision {
                lockout: Some((Lockout::Compressor, d.operation, COMPRESSOR_LOCKOUT)),
                ..Decision::new(Controller::Lockout, Action::Drift, Operation::Off)
            };
        }
        // The humidistat after the thermostat (0x51cce4): Cool to Dry runs
        // the first cool stage while the thermostat drifts.
        if let (Some(t), Controller::SystemTest) = (self.test, d.controller) {
            d.humidity = t.humidity;
        } else if d.controller != Controller::SafetyTemperatures && self.test.is_none() {
            let targets = humidity::Targets {
                heat: match mode { Mode::Heat(t) | Mode::Emergency(t) | Mode::Range(t, _) => Some(t), _ => None },
                cool: match mode { Mode::Cool(t) | Mode::Range(_, t) => Some(t), _ => None },
                off: mode == Mode::Off,
                cool_mode: matches!(mode, Mode::Cool(_)),
            };
            let rest = self.lockouts.compressor.is_some();
            let h = self.humidistat.decide(targets, d.action == Action::Heat, d.action == Action::Cool, room, humidity, self.day, rest, now);
            d.humidity = h.op;
            if h.cool_to_dry && d.action == Action::Drift && d.operation == Operation::Off {
                debug!("hvac: cool to dry runs stage one cool: room {room:.2} °C, humidity {}", opt(humidity));
                d = Decision { humidity: h.op, ..Decision::new(Controller::CoolToDry, Action::Drift, Operation::Stage1Cool) };
            }
        }
        let l = &mut self.lockouts;
        if running.operation.compressor(hp) && !d.operation.compressor(hp) {
            l.compressor = Some(COMPRESSOR_LOCKOUT);
        }
        l.held = matches!(d.lockout, Some((Lockout::Compressor, _, _)));
        if running.action != d.action {
            l.before = Some(running.action);
            l.cycle = Some(if running.operation.compressor(hp) || d.operation.compressor(hp) { MINIMUM_COMPRESSOR_CYCLE } else { MINIMUM_CYCLE });
        }
        // OVEREAGER after the decision (0x51b024).
        self.overeager.0 = match (self.overeager.0, d.action) {
            (1, Action::Heat) | (2, Action::Cool) => if d.controller == Controller::Overeager { 3 } else { 0 },
            (3 | 4, Action::Drift) => 0,
            (s, _) => s,
        };
        // A test's cycles are not the system's own to learn from.
        if self.test.is_none() {
            self.learn_airwave(d, running, room, now);
            if !self.stale {
                self.overshoot.update(running, &d, room, now, self.tracker.state);
            }
        }
        self.heads_up.after(&d, hp, now);
        // The safety run remembered after the decision (0x51e78c).
        if !self.stale {
            self.safety_run = match (d.controller, d.action) {
                (Controller::SafetyTemperatures, a @ (Action::Heat | Action::Cool)) => Some(a),
                _ if safety_hold => self.safety_run,
                _ => None,
            };
        }
        self.follow(d, running, mode, early, now);
        self.track_cycle(d, running, room, now);
        if crate::log::enabled(crate::log::Level::Debug) {
            self.explain(mode, room, humidity, running, &d, now);
        }
        self.last = d;
        d
    }

    /// Keeps a helper's reason for the decision's log.
    fn note(&self, what: String) {
        if crate::log::enabled(crate::log::Level::Debug) {
            self.notes.borrow_mut().push(what);
        }
    }

    /// Logs a decision with what it was made from: the room, the
    /// thresholds the band uses from the running action, and the timers;
    /// when it or its thresholds change, else once a minute.
    fn explain(&mut self, mode: Mode, room: f32, humidity: Option<f32>, running: Decision, d: &Decision, now: f64) {
        let [hb, cb] = self.bands;
        let op = running.operation;
        let heat = |t: f32| if running.action == Action::Heat {
            format!("heat runs below {:.2}, overshoot gate at {:.2}", t + hb.min(BAND_MAX), self.overshoot_limit(true, op, t))
        } else {
            format!("heat starts at {:.2}", t - hb)
        };
        let cool = |t: f32| if running.action == Action::Cool {
            format!("cool runs above {:.2}, overshoot gate at {:.2}", t - cb.min(BAND_MAX), self.overshoot_limit(false, op, t))
        } else {
            format!("cool starts at {:.2}", t + cb)
        };
        let why = match mode {
            _ if self.stale => "no room reading".to_string(),
            Mode::Off => "off".to_string(),
            Mode::Heat(t) => heat(t),
            Mode::Emergency(t) => format!("emergency heat band {:.2} to {:.2}", t - EMERGENCY_BAND, t + EMERGENCY_BAND),
            Mode::Cool(t) => cool(t),
            Mode::Range(l, h) => format!("{}; {}", heat(l), cool(h)),
        };
        let lockout = d.lockout.map_or(String::new(), |(l, op, _)| format!(", {} holds {op:?}", l.name()));
        let what = format!("{mode:?}: {why} -> {} {:?} {:?}{lockout}", d.controller.name(), d.action, d.operation);
        let notes = std::mem::take(&mut *self.notes.borrow_mut());
        if self.explained.as_ref().is_some_and(|(w, at)| *w == what && now - at < 60.0) {
            return;
        }
        debug!("hvac: room {room:.2} °C, humidity {}, {what}", opt(humidity));
        for n in notes {
            debug!("hvac: {n}");
        }
        self.explained = Some((what, now));
        let l = &self.lockouts;
        if l.cycle.is_some() || l.compressor.is_some() {
            debug!("hvac: timers: minimum cycle {}, compressor lockout {}", l.cycle.map_or("none".into(), |c| format!("{c:.0} s left")),
                   l.compressor.map_or("none".into(), |c| format!("{c:.0} s left")));
        }
    }

    /// OVEREAGER (0x51b168): a manual change of target with the room
    /// inside the new band acts at once, heating (or cooling) until the room
    /// moves 0.2 °C, or stopping; it ends when the room leaves its bounds.
    fn track_overeager(&mut self, mode: Mode, room: f32) {
        let targets = match mode {
            Mode::Heat(t) => (Some(t), None),
            Mode::Cool(t) => (None, Some(t)),
            Mode::Range(low, high) => (Some(low), Some(high)),
            _ => (None, None),
        };
        let (old, prev) = (self.tracker.targets, self.overeager.0);
        let kind = |t: (Option<f32>, Option<f32>)| (t.0.is_some(), t.1.is_some());
        if !self.manual_target || kind(old) != kind(targets) {
            self.overeager = (0, None, None);
            return;
        }
        if targets != old {
            let [hb, cb] = self.bands;
            let mut set = None;
            if let (Some(new), old_h) = (targets.0, old.0) {
                let (up, low) = (new + hb.min(BAND_MAX), new - hb);
                if low < room && room < up {
                    if room <= new && ((old_h.is_some_and(|o| new > o && room > o)) || prev == 1) {
                        set = Some((1, Some(room + 0.2), Some(low)));
                    } else if room >= new && ((old_h.is_some_and(|o| new < o && room < o)) || prev == 4) {
                        set = Some((4, Some(up), Some(low)));
                    }
                }
            }
            let heat_unchanged = targets.0 == old.0;
            if let (None, Some(new), old_c) = (set, targets.1, old.1) {
                let heat_out = targets.0.is_none_or(|h| room <= h - hb || room >= h + hb.min(BAND_MAX));
                let (up, low) = (new + cb, new - cb.min(BAND_MAX));
                if (heat_unchanged || heat_out) && low < room && room < up {
                    if room >= new && ((old_c.is_some_and(|o| new < o && room < o)) || prev == 2) {
                        set = Some((2, Some(up), Some(room - 0.2)));
                    } else if room <= new && ((old_c.is_some_and(|o| new > o && room > o)) || prev == 4) {
                        set = Some((4, Some(up), Some(low)));
                    }
                }
            }
            if let Some(s) = set {
                if s.0 != prev {
                    debug!("hvac: overeager {} after a manual change of target: until the room leaves {} to {}",
                           match s.0 { 1 => "heats", 2 => "cools", _ => "stops" }, opt(s.2), opt(s.1));
                }
                self.overeager = s;
            }
        }
        let (state, up, low) = self.overeager;
        if state != 0 && (up.zip(low).is_none() || low.is_some_and(|l| room <= l) || up.is_some_and(|u| room >= u)) {
            debug!("hvac: overeager ends: room {room:.2} °C left {} to {}", opt(low), opt(up));
            self.overeager = (0, None, None);
        }
    }

    /// The targets' state, as the multistage helper and the time to
    /// temperature use it.
    pub fn tracker_state(&self) -> u8 {
        self.tracker.state
    }

    /// The targets' state (0x511404): a change with the room on the
    /// calling side is 2 when the user or the schedule made it, else 1; a
    /// change elsewhere is 3; staying elsewhere after a change, 4.
    fn track_targets(&mut self, mode: Mode, room: f32) {
        let targets = match mode {
            Mode::Off => (None, None),
            Mode::Heat(t) | Mode::Emergency(t) => (Some(t), None),
            Mode::Cool(t) => (None, Some(t)),
            Mode::Range(low, high) => (Some(low), Some(high)),
        };
        let t = &mut self.tracker;
        let changed = targets != t.targets;
        t.targets = targets;
        let demand = targets.0.is_some_and(|h| room < h) || targets.1.is_some_and(|c| room > c);
        t.state = match (demand, changed, t.state) {
            (true, true, _) => if self.own_target { 2 } else { 1 },
            (true, false, 3) => 4,
            (false, true, _) => 3,
            (false, false, 1 | 2) => 4,
            (_, _, s) => s,
        };
    }

    /// The cycle's and the stage's start (0x511518).
    fn track_cycle(&mut self, d: Decision, running: Decision, room: f32, now: f64) {
        let active = |a: Action| matches!(a, Action::Heat | Action::Cool);
        let side = |a: Action| usize::from(a == Action::Cool);
        // A cycle's end gives the band learning its sample; the next cycle
        // from drift feeds it in and may move the band.
        if d.action == Action::Drift && active(running.action) {
            if let Some((at, from)) = self.tracker.cycle {
                let m = &mut self.band_models[side(running.action)];
                m.pending = Some(((room - from).abs(), (((now - at) / 60.0) as f32).min(BAND_MINUTES_MAX)));
                debug!("hvac: {:?} cycle ended: room {:+.2} °C in {:.1} min", running.action, room - from, (now - at) / 60.0);
            }
        }
        if active(d.action) && running.action == Action::Drift {
            let i = side(d.action);
            let m = &mut self.band_models[i];
            if let Some((change, minutes)) = m.pending.take() {
                m.change += BAND_ALPHA * (change - m.change);
                m.minutes += BAND_ALPHA * (minutes - m.minutes);
            }
            let band = m.band(if i == 0 { 3.0 } else { 5.0 });
            if band != self.bands[i] && m.changed_at.is_none_or(|at| now - at >= BAND_HOLD) {
                info!("hvac: {} band learned: {:.2} °C, was {:.2} (cycles average {:.2} °C over {:.1} min)",
                      if i == 0 { "heat" } else { "cool" }, band, self.bands[i], m.change, m.minutes);
                m.changed_at = Some(now);
                self.bands[i] = band;
            }
        }
        let t = &mut self.tracker;
        if active(d.action) && d.action != running.action {
            t.cycle = Some((now, room));
            t.stage = Some((now, room));
        } else if active(d.action) && d.operation != running.operation {
            t.stage = Some((now, room));
        } else if d.action == Action::Drift && active(running.action) {
            t.cycle = Some((now, room));
            t.stage = None;
        }
    }

    /// The multistage helper's stage (0x4dd304): never lower than the
    /// running stage within a cycle; a manual or scheduled change of target
    /// far from the room starts at stage 2 or 3; otherwise one stage up
    /// after 15 minutes in a stage with the room 0.56 °C the wrong way
    /// since the cycle began, or once the cycle has run its hour (heat) or
    /// two (cool) with 15 minutes in the stage.
    fn stage_up(&self, wanted: Decision, running: Decision, mode: Mode, room: f32, now: f64) -> Decision {
        let (heat, max) = match wanted.action {
            Action::Heat => (true, self.stages.0),
            Action::Cool => (false, self.stages.1),
            _ => return wanted,
        };
        if wanted.operation == Operation::Off || wanted.controller == Controller::Overshoot {
            return wanted;
        }
        if let (true, Some(df)) = (heat, self.dual_fuel) {
            return self.dual_fuel_stage(df, wanted, running, mode, room, now);
        }
        if let (true, Some(hp)) = (heat, self.heat_pump) {
            return self.heat_pump_stage(hp, wanted, running, mode, room, now);
        }
        let cur = if running.action == wanted.action { running.operation.stage() } else { 0 };
        let stage = self.multistage(heat, cur, max, mode, room, now);
        let operation = if heat { Operation::heat(stage) } else { Operation::cool(stage) };
        Decision { operation, ..wanted }
    }

    /// The multistage helper's stage (0x4ed304) for a cycle at `cur` (0
    /// when starting) with `max` stages.
    fn multistage(&self, heat: bool, cur: u8, max: u8, mode: Mode, room: f32, now: f64) -> u8 {
        let max = max.max(1);
        if max <= 1 {
            1
        } else {
            let target = match (mode, heat) {
                (Mode::Heat(t) | Mode::Range(t, _), true) | (Mode::Cool(t) | Mode::Range(_, t), false) => Some(t),
                _ => None,
            };
            let gap = target.map(|t| if heat { t - room } else { room - t });
            let jump = gap.filter(|_| self.tracker.state == 2).and_then(|g| {
                if g > STAGE_GAPS.1 { Some(3) } else if g > STAGE_GAPS.0 { Some(2) } else { None }
            });
            let s = match jump {
                Some(j) if j != cur => j,
                _ => {
                    let t = self.tracker;
                    let timed = (cur > 0).then_some(()).and(t.cycle.zip(t.stage));
                    let up = timed.is_some_and(|((cycle_at, cycle_room), (stage_at, _))| {
                        let in_stage = now - stage_at >= STAGE_TIME;
                        let wrong = if heat { room - cycle_room <= -STAGE_FALLBACK } else { room - cycle_room >= STAGE_FALLBACK };
                        let limit = if heat { STAGE_CYCLE.0 } else { STAGE_CYCLE.1 };
                        in_stage && (wrong || now - cycle_at >= limit)
                    });
                    (if up { cur + 1 } else { cur }).max(1)
                }
            };
            s.max(cur).min(max)
        }
    }

    /// The dual-fuel helper (0x4ec380, its operation 0x4ec55c): alternate
    /// heat instead of the compressor when the override says so, once it
    /// runs in the cycle, below the breakpoint outside, or, with no outdoor
    /// temperature, when the top compressor stage is not keeping up; each
    /// side staged as the multistage helper stages.
    fn dual_fuel_stage(&self, df: DualFuel, wanted: Decision, running: Decision, mode: Mode, room: f32, now: f64) -> Decision {
        let alt_running = running.action == Action::Heat && matches!(running.operation, Operation::Stage1AltHeat | Operation::Stage2AltHeat);
        let compressor_stage = if running.action == Action::Heat { running.operation.stage() } else { 0 };
        let max = self.stages.0.max(1);
        let t = self.tracker;
        let use_alt = df.stages > 0 && match df.override_ {
            1 => true,
            2 => false,
            _ if alt_running => true,
            _ => match self.outdoor {
                Some(o) => o < df.breakpoint,
                None => compressor_stage == max && t.cycle.zip(t.stage).is_some_and(|((cycle_at, cycle_room), (stage_at, _))| {
                    now - cycle_at > ALT_CYCLE || (now - stage_at > ALT_STAGE && room - cycle_room < ALT_RISE)
                }),
            },
        };
        self.note(format!("dual fuel: {} (override {}, outdoor {}, breakpoint {:.1} °C, alternate running {alt_running})",
                          if use_alt { "alternate heat" } else { "heat pump" }, df.override_, opt(self.outdoor), df.breakpoint));
        let operation = if use_alt {
            let cur = match running.operation {
                Operation::Stage1AltHeat if alt_running => 1,
                Operation::Stage2AltHeat if alt_running => 2,
                _ => 0,
            };
            match self.multistage(true, cur, df.stages, mode, room, now) {
                1 => Operation::Stage1AltHeat,
                2 => Operation::Stage2AltHeat,
                _ => Operation::Off,
            }
        } else {
            let cur = if alt_running { 0 } else { compressor_stage };
            Operation::heat(self.multistage(true, cur, max, mode, room, now))
        };
        Decision { operation, ..wanted }
    }

    /// Where the overshoot gate ends a cycle (0x4e0f70): heat at
    /// max(t + min(band, 0.54) - overshoot, t - band + 0.2), cool at
    /// min(t - min(band, 0.54) + overshoot, t + band - 0.2), the overshoot
    /// the running operation's group has learned.
    fn overshoot_limit(&self, heat: bool, op: Operation, t: f32) -> f32 {
        let b = self.bands[usize::from(!heat)];
        let group = super::overshoot::group(op).unwrap_or(if heat { 0 } else { 2 });
        let ov = self.overshoot.overshoot(group);
        if heat {
            (t + b.min(BAND_MAX) - ov).max(t - b + 0.2)
        } else {
            (t - b.min(BAND_MAX) + ov).min(t + b - 0.2)
        }
    }

    /// The heat pump helper (0x4ec9f0, its operation 0x4dcd50): aux alone
    /// below the compressor's outdoor lockout; otherwise the compressor
    /// stages (at most two), and one more with aux once a cycle is under
    /// way, aux not locked out and the room short of its threshold. Stages
    /// step up as the multistage helper's do, the cycle limit being the
    /// upstage time (shortened while compressor stages remain).
    fn heat_pump_stage(&self, hp: HeatPump, wanted: Decision, running: Decision, mode: Mode, room: f32, now: f64) -> Decision {
        let target = match mode {
            Mode::Heat(t) | Mode::Range(t, _) => t,
            _ => return wanted,
        };
        let outdoor = self.outdoor;
        let compressor_out = hp.aux
            && outdoor.zip(hp.compressor_lockout).is_some_and(|(o, c)| o < hp.aux_lockout.map_or(c, |a| c.min(a)));
        if compressor_out {
            self.note(format!("heat pump: compressor locked out at outdoor {}, aux alone", opt(outdoor)));
            return Decision { operation: Operation::AuxOnly, ..wanted };
        }
        let compressor = self.stages.0.clamp(1, 2);
        let cur = if running.action == Action::Heat { running.operation.heat_pump_stage() } else { 0 };
        let t = self.tracker;
        let in_cycle = t.cycle.map(|(at, _)| now - at);
        let in_stage = t.stage.map(|(at, _)| now - at);
        let aux_locked = if !hp.aux {
            true
        } else if hp.aux_lockout.is_none() || self.safety.0.is_some_and(|low| room < low) {
            false
        } else if outdoor.zip(hp.aux_lockout).is_some_and(|(o, a)| o > a) {
            true
        } else {
            in_cycle.is_none_or(|c| c < hp.aux_delay)
        };
        let threshold = target - hp.aux_offset;
        let aux_running = cur > compressor;
        let satisfied = if aux_running && in_stage.is_some_and(|s| s < AUX_RECHECK) {
            false
        } else {
            room >= threshold || t.stage.is_some_and(|(_, from)| from >= threshold && room > threshold - AUX_SATISFIED)
        };
        let max = if !aux_locked && !satisfied && cur != 0 { compressor + 1 } else { compressor };
        self.note(format!("heat pump: aux {}, aux threshold {threshold:.2}{}, outdoor {}, cycle {} s",
                          if aux_locked { "locked out" } else { "allowed" }, if satisfied { " (satisfied)" } else { "" }, opt(outdoor),
                          in_cycle.map_or(0, |c| c as i64)));
        let gap = target - room;
        let jump = match self.tracker.state {
            2 if gap > STAGE_GAPS.1 => Some(3),
            2 if gap > STAGE_GAPS.0 => Some(2),
            _ => None,
        };
        let s = match jump {
            Some(j) if j != cur => j,
            _ => {
                let limit = if cur >= compressor { hp.upstage } else { (hp.upstage - STAGE_TIME).min(3600.0) };
                let up = cur > 0 && t.cycle.zip(t.stage).is_some_and(|((cycle_at, cycle_room), (stage_at, _))| {
                    now - stage_at >= STAGE_TIME && (room - cycle_room <= -STAGE_FALLBACK || now - cycle_at >= limit)
                });
                (if up { cur + 1 } else { cur }).max(1)
            }
        };
        let stage = s.max(cur).min(max);
        let operation = match (stage > compressor, compressor) {
            (false, _) => Operation::heat(stage),
            (true, 1) => Operation::Stage1HeatAux,
            (true, _) => Operation::Stage2HeatAux,
        };
        Decision { operation, ..wanted }
    }

    /// When the multistage helper would step up next, if the room does not
    /// get there first (0x4dd8fc): 15 minutes into the stage, or the
    /// cycle's limit.
    fn stage_deadline(&self) -> Option<f64> {
        let d = self.last;
        let max = match d.action {
            Action::Heat => self.stages.0,
            Action::Cool => self.stages.1,
            _ => return None,
        };
        if d.operation.stage() >= max {
            return None;
        }
        let (cycle_at, _) = self.tracker.cycle?;
        let (stage_at, _) = self.tracker.stage?;
        let limit = if d.action == Action::Heat { STAGE_CYCLE.0 } else { STAGE_CYCLE.1 };
        Some((stage_at + STAGE_TIME).min((cycle_at + limit).max(stage_at + STAGE_TIME)))
    }

    /// Airwave's bookkeeping after a decision: a cooling cycle starting,
    /// the fan taking over, the cycle ending.
    fn follow(&mut self, d: Decision, running: Decision, mode: Mode, early: bool, now: f64) {
        if !matches!(self.airwave, Airwave::Idle) && d.action != Action::Cool {
            self.airwave = Airwave::Idle;
            return;
        }
        let airwave_fan = d.controller == Controller::Airwave && d.action == Action::Cool && d.fan;
        match self.airwave {
            Airwave::Fan { .. } if !airwave_fan => {
                debug!("hvac: airwave: fan run ended");
                self.airwave = Airwave::Idle;
            }
            Airwave::Fan { .. } => {}
            Airwave::Cooling { floor, stop, .. } if running.action == Action::Cool => {
                if early && airwave_fan {
                    let hours = self.airwave_fan.clamp(AIRWAVE_FAN.0, AIRWAVE_FAN.1);
                    debug!("hvac: airwave: compressor off at or below {stop:.2} °C, the fan cools on for {:.0} min", hours * 60.0);
                    self.airwave = Airwave::Fan { floor, stop, until: now + f64::from(hours * 3600.0) };
                }
            }
            _ if d.action == Action::Cool && running.action != Action::Cool => {
                let floor = match mode {
                    Mode::Cool(t) | Mode::Range(_, t) => t - self.bands[1],
                    _ => return,
                };
                let offset = self.airwave_offset.clamp(AIRWAVE_OFFSET.0, AIRWAVE_OFFSET.1);
                debug!("hvac: airwave: cooling started, the compressor may stop after {AIRWAVE_RUN} s at {:.2} °C (band floor {floor:.2})", floor + offset);
                self.airwave = Airwave::Cooling { start: now, floor, stop: floor + offset };
            }
            _ => {}
        }
    }

    /// Airwave's learning (0x4dbee0, 0x4e1988): from the fan taking over
    /// from the compressor, the room's lowest point and how long it took
    /// feed the fan run's and the offset's averages (α 0.05), the offset
    /// only for a run within three deviations.
    fn learn_airwave(&mut self, d: Decision, running: Decision, room: f32, now: f64) {
        let fan = |x: &Decision| x.controller == Controller::Airwave && x.fan;
        if fan(&d) && !fan(&running) && running.operation.compressor(self.heat_pump.is_some()) {
            self.airwave_track = Some(AirwaveTrack { start: now, from: room, ended: None, low: None });
            return;
        }
        let Some(AirwaveTrack { start, from, mut ended, mut low }) = self.airwave_track else { return };
        if !fan(&d) && ended.is_none() {
            ended = Some(now);
        }
        let hours = (now - start) / 3600.0;
        if ended.is_some_and(|e| now - e > 180.0) || hours > 0.17 || (low.is_none() && hours > 0.017 && room > from) {
            self.airwave_track = None;
            return;
        }
        if room <= from - 0.1 && low.is_none_or(|(_, l)| room < l) {
            low = Some((now, room));
        }
        if let Some((at, l)) = low {
            if room > l + 0.01 {
                let dt = ((at - start) / 3600.0) as f32;
                let e = dt - self.airwave_fan;
                self.airwave_fan += 0.05 * e;
                self.airwave_fan_var = 0.95 * (self.airwave_fan_var + 0.05 * e * e);
                if (dt - self.airwave_fan).abs() <= 3.0 * self.airwave_fan_var.sqrt() {
                    self.airwave_offset = (self.airwave_offset + 0.05 * ((from - l).abs() - self.airwave_offset)).min(AIRWAVE_OFFSET.1);
                }
                debug!("hvac: airwave learned: fan run {:.3} h, offset {:.2} °C (this run's low {l:.2} °C after {dt:.3} h)", self.airwave_fan, self.airwave_offset);
                self.airwave_track = None;
                return;
            }
        }
        self.airwave_track = Some(AirwaveTrack { start, from, ended, low });
    }

    /// When the compressor lockout ends, on `decide`'s clock, while it runs.
    pub fn compressor_lockout_end(&self) -> Option<f64> {
        Some(self.lockouts.at? + self.lockouts.compressor?)
    }

    /// When a lockout or Airwave's fan run ends, on `decide`'s clock: stock
    /// decides again then.
    pub fn deadline(&self) -> Option<f64> {
        let l = &self.lockouts;
        let at = l.at?;
        let fan = match self.airwave {
            Airwave::Fan { until, .. } => Some(until),
            _ => None,
        };
        [l.cycle, l.compressor].into_iter().flatten().map(|left| at + left).chain(fan).chain(self.stage_deadline()).reduce(f64::min)
    }

    /// The room temperatures that should wake a sleeping unit, low and
    /// high, as stock gives the backplate: where a cycle would start, or
    /// end, and otherwise the safety temperatures (measured on nest-qemu:
    /// heat at 21 idle wakes at 20.61, heating to 22 at 22.19, cooling to
    /// 20 at 19.66, off at the 5 °C safety).
    pub fn wake(&self, mode: Mode) -> (Option<f32>, Option<f32>) {
        let (low, high) = self.safety;
        let [hb, cb] = self.bands;
        // A stage that may step up wakes the unit once the room moves
        // 0.56 °C the wrong way from where the stage began.
        let base = self.stage_deadline().and(self.tracker.stage).map(|(_, room)| room);
        let op = self.last.operation;
        let max = |a: Option<f32>, b: Option<f32>| match (a, b) { (Some(a), Some(b)) => Some(a.max(b)), (a, b) => a.or(b) };
        let min = |a: Option<f32>, b: Option<f32>| match (a, b) { (Some(a), Some(b)) => Some(a.min(b)), (a, b) => a.or(b) };
        match (self.last.action, mode) {
            (Action::Heat, Mode::Heat(t) | Mode::Range(t, _)) => (max(low, base.map(|r| r - STAGE_FALLBACK)), Some(self.overshoot_limit(true, op, t))),
            (Action::Cool, Mode::Cool(t) | Mode::Range(_, t)) => (Some(self.overshoot_limit(false, op, t)), min(high, base.map(|r| r + STAGE_FALLBACK))),
            (_, Mode::Heat(t)) => (Some(t - hb), high),
            (_, Mode::Cool(t)) => (low, Some(t + cb)),
            (_, Mode::Range(lo, hi)) => (Some(lo - hb), Some(hi + cb)),
            _ => (low, high),
        }
    }

    /// The safety temperatures in use, low and high, none when they cross.
    fn safety_bounds(&self, mode: Mode) -> Option<(Option<f32>, Option<f32>)> {
        let (mut low, mut high) = self.safety;
        if low.zip(high).is_some_and(|(l, h)| h <= l) {
            return None;
        }
        // They keep 2 °C from the targets (0x51e228).
        let (heat, cool) = match mode {
            Mode::Heat(t) => (Some(t), None),
            Mode::Cool(t) => (None, Some(t)),
            Mode::Range(l, h) => (Some(l), Some(h)),
            _ => (None, None),
        };
        if let (Some(l), Some(t)) = (low, heat) {
            if t - l > 0.0 && t - l <= 2.0 {
                low = Some(t - 2.0);
            }
        }
        if let (Some(h), Some(t)) = (high, cool) {
            if t - h >= -2.0 && t - h < 0.0 {
                high = Some(t + 2.0);
            }
        }
        Some((low, high))
    }

    /// The safety temperatures (stock's 0x51e654): heat at or below the low
    /// one until SAFETY_BAND above it, cool at or above the high one until
    /// SAFETY_BAND below it, whatever the mode. While a safety run is
    /// remembered the bands follow the last decision's action, whichever
    /// controller made it; otherwise they start from drift.
    fn safety(&self, room: f32, mode: Mode) -> Option<Decision> {
        let (low, high) = self.safety_bounds(mode)?;
        let was = self.safety_run.map(|_| self.last.action);
        let heat = low.is_some_and(|t| if was == Some(Action::Heat) { room < t + SAFETY_BAND } else { room <= t });
        let cool = high.is_some_and(|t| if was == Some(Action::Cool) { room > t - SAFETY_BAND } else { room >= t });
        let (action, operation) = if heat {
            (Action::Heat, Operation::Stage1Heat)
        } else if cool {
            (Action::Cool, Operation::Stage1Cool)
        } else {
            return None;
        };
        Some(Decision::new(Controller::SafetyTemperatures, action, operation))
    }

    /// The lockout controller: holds the running cycle to its minimum, then
    /// keeps a stopped compressor off. Safety temperatures skip the minimum,
    /// as does whatever follows them.
    fn lock(&self, wanted: Decision, running: Decision, changed: bool) -> Decision {
        let l = &self.lockouts;
        // Safety temperatures and emergency heat skip the minimum cycle.
        // Nor does it undo COMPRESSOR_HEADS_UP's stop.
        if wanted.controller == Controller::CompressorHeadsUp {
            return wanted;
        }
        let safety = [wanted.controller, running.controller].iter().any(|c| matches!(c, Controller::SafetyTemperatures | Controller::EmergencyHeat));
        if !changed && !safety && !l.held && running.action != wanted.action && Some(running.action) != l.before {
            if let Some(left) = l.cycle {
                let held = Decision {
                    lockout: Some((Lockout::MinimumCycle, wanted.operation, left)),
                    ..Decision::new(Controller::Lockout, running.action, running.operation)
                };
                if l.compressor.is_none_or(|c| left >= c) {
                    return held;
                }
            }
        }
        match l.compressor {
            Some(left) if wanted.operation.compressor(self.heat_pump.is_some()) => Decision {
                lockout: Some((Lockout::Compressor, wanted.operation, left)),
                ..Decision::new(Controller::Lockout, Action::Drift, Operation::Off)
            },
            _ => wanted,
        }
    }

    /// The band decision (stock's 0x51df40), then Airwave or the overshoot
    /// gate (0x4e11ac), and the single-stage helper's operations.
    fn band(&self, mode: Mode, room: f32, early: bool, now: f64) -> Decision {
        let was = self.last.action;
        let heat = |target: f32| -> bool {
            let b = self.bands[0];
            if was == Action::Heat { room < target + b.min(BAND_MAX) } else { room <= target - b }
        };
        let cool = |target: f32| -> bool {
            let b = self.bands[1];
            if was == Action::Cool { room > target - b.min(BAND_MAX) } else { room >= target + b }
        };
        let (action, target) = match mode {
            Mode::Off => (Action::Drift, None),
            Mode::Heat(t) | Mode::Emergency(t) => (if heat(t) { Action::Heat } else { Action::Drift }, Some(t)),
            Mode::Cool(t) => (if cool(t) { Action::Cool } else { Action::Drift }, Some(t)),
            Mode::Range(low, high) => {
                if heat(low) { (Action::Heat, Some(low)) } else if cool(high) { (Action::Cool, Some(high)) } else { (Action::Drift, None) }
            }
        };
        // OVEREAGER overrides the band's action (0x51afc0).
        let (action, target, overeager) = match (self.overeager.0, mode) {
            (1, Mode::Heat(t) | Mode::Range(t, _)) if action != Action::Heat => (Action::Heat, Some(t), true),
            (2, Mode::Cool(t) | Mode::Range(_, t)) if action != Action::Cool => (Action::Cool, Some(t), true),
            (4, _) if action != Action::Drift => (Action::Drift, None, true),
            _ => (action, target, false),
        };
        let mut d = Decision::new(if overeager { Controller::Overeager } else { Controller::LearningBand }, action, Operation::Off);
        match (action, self.airwave) {
            (Action::Cool, Airwave::Cooling { stop, .. }) if early => {
                if room <= stop {
                    return Decision { fan: true, ..Decision::new(Controller::Airwave, Action::Cool, Operation::Off) };
                }
                d.operation = Operation::Stage1Cool;
                return d;
            }
            (Action::Cool, Airwave::Fan { floor, stop, until }) => {
                if now < until && room > floor && room < stop + AIRWAVE_RISE {
                    return Decision { fan: true, ..Decision::new(Controller::Airwave, Action::Cool, Operation::Off) };
                }
                return Decision::new(Controller::Airwave, Action::Drift, Operation::Off);
            }
            _ => {}
        }
        // The gate applies to a cycle already running, with OVEREAGER idle.
        let op = self.last.operation;
        let gate = self.overeager.0 == 0;
        match (action, target) {
            (Action::Heat, Some(t)) if gate && was == Action::Heat && room >= self.overshoot_limit(true, op, t) => {
                d = Decision::new(Controller::Overshoot, Action::Drift, Operation::Off)
            }
            (Action::Cool, Some(t)) if gate && was == Action::Cool && room <= self.overshoot_limit(false, op, t) => {
                d = Decision::new(Controller::Overshoot, Action::Drift, Operation::Off)
            }
            (Action::Heat, _) => d.operation = Operation::Stage1Heat,
            (Action::Cool, _) => d.operation = Operation::Stage1Cool,
            _ if !overeager => d.controller = Controller::None,
            _ => {}
        }
        d
    }
}

/// The switches an operation turns on, given the equipment.
pub fn switches(op: Operation, humidity: humidity::Op, cool_to_dry: bool, equipment: &Equipment) -> Vec<Switch> {
    // A higher stage runs with the ones below it (stock on nest-qemu: W1,
    // G, then W2 for its second heat stage), the fan as the operation's
    // flags say.
    let stages: &[Switch] = match op {
        Operation::Stage1Heat => &[Switch::Stage1Heat],
        Operation::Stage2Heat => &[Switch::Stage1Heat, Switch::Stage2Heat],
        Operation::Stage3Heat => &[Switch::Stage1Heat, Switch::Stage2Heat, Switch::Stage3Heat],
        Operation::Stage1Cool => &[Switch::Stage1Cool],
        Operation::Stage2Cool => &[Switch::Stage1Cool, Switch::Stage2Cool],
        Operation::Stage3Cool => &[Switch::Stage1Cool, Switch::Stage2Cool, Switch::Stage3Cool],
        // A heat pump's (stock's 0x5b366c).
        Operation::Stage1HeatAux => &[Switch::Stage1Heat, Switch::AuxHeat],
        Operation::Stage2HeatAux => &[Switch::Stage1Heat, Switch::Stage2Heat, Switch::AuxHeat],
        Operation::AuxOnly => &[Switch::AuxHeat],
        Operation::EmergencyHeat => &[Switch::EmergencyHeat],
        Operation::Stage1AltHeat => &[Switch::Stage1AltHeat],
        Operation::Stage2AltHeat => &[Switch::Stage1AltHeat, Switch::Stage2AltHeat],
        _ => &[],
    };
    let mut on = Vec::new();
    if !stages.is_empty() && equipment.has_fan && equipment.op_fan(op) {
        on.push(Switch::Fan);
    }
    on.extend_from_slice(stages);
    // The humidistat's switches (0x5b366c), and the fan when its
    // `*_fan_activation` asks for it (0x51c38c).
    let (switch, fan) = match humidity {
        humidity::Op::Humidify => (Some(Switch::Humidifier), equipment.humidifier_fan),
        humidity::Op::Dehumidify => (Some(Switch::Dehumidifier), equipment.dehumidifier_fan),
        humidity::Op::AcIntegratedDehumidify => (Some(Switch::Dehumidifier), equipment.dehumidifier_fan),
        humidity::Op::Off => (None, false),
    };
    if fan && equipment.has_fan && !on.contains(&Switch::Fan) {
        on.insert(0, Switch::Fan);
    }
    on.extend(switch);
    if humidity == humidity::Op::AcIntegratedDehumidify || (op == Operation::Stage1Cool && cool_to_dry) {
        on.push(Switch::AcDehumidifier);
    }
    on
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dual_fuel_burns_below_the_breakpoint_and_keeps_to_it_for_the_cycle() {
        let dual = |outdoor: Option<f32>, override_: u8| {
            let mut c = Control { heat_pump: Some(HeatPump::new(false, None, None, 3)), outdoor, ..Control::default() };
            c.dual_fuel = Some(DualFuel { stages: 1, breakpoint: 5.0, override_ });
            c
        };
        let mut c = dual(Some(0.0), 0);
        assert_eq!(c.decide(Mode::Heat(21.0), 19.0, None, 0.0).operation, Operation::Stage1AltHeat);
        c.outdoor = Some(10.0);
        assert_eq!(c.decide(Mode::Heat(21.0), 19.1, None, 60.0).operation, Operation::Stage1AltHeat, "sticky in the cycle");
        let mut c = dual(Some(10.0), 0);
        assert_eq!(c.decide(Mode::Heat(21.0), 19.0, None, 0.0).operation, Operation::Stage1Heat);
        assert_eq!(dual(Some(10.0), 1).decide(Mode::Heat(21.0), 19.0, None, 0.0).operation, Operation::Stage1AltHeat);
        assert_eq!(dual(Some(0.0), 2).decide(Mode::Heat(21.0), 19.0, None, 0.0).operation, Operation::Stage1Heat);
        // No weather: the compressor, until two hours of the cycle.
        let mut c = dual(None, 0);
        assert_eq!(c.decide(Mode::Heat(21.0), 19.0, None, 0.0).operation, Operation::Stage1Heat);
        assert_eq!(c.decide(Mode::Heat(21.0), 19.0, None, 600.0).operation, Operation::Stage1Heat);
        assert_eq!(c.decide(Mode::Heat(21.0), 19.5, None, 1000.0).operation, Operation::Stage1AltHeat, "15 min in, up under 1 °C");
        assert_eq!(switches(Operation::Stage1AltHeat, humidity::Op::Off, false, &Equipment { heat_pump: true, ..electric() }), [Switch::Fan, Switch::Stage1AltHeat]);
    }

    #[test]
    fn an_air_conditioner_losing_ground_is_stopped_by_heads_up() {
        let mut c = Control::default();
        c.heads_up.enabled = true;
        let (mut room, mut now) = (27.0, 0.0);
        let mut d = c.decide(Mode::Cool(22.0), room, None, now);
        assert_eq!(d.action, Action::Cool);
        while d.controller != Controller::CompressorHeadsUp && now < 8000.0 {
            now += 60.0;
            room += 5.0 / 60.0;
            c.heads_up.sample(room, now);
            d = c.decide(Mode::Cool(22.0), room, None, now);
        }
        assert!((7200.0..7300.0).contains(&now), "{now}");
        assert_eq!((d.action, d.operation), (Action::Drift, Operation::Off));
        // The retest holds the compressor off.
        now += 60.0;
        let d = c.decide(Mode::Cool(22.0), room, None, now);
        assert_eq!(d.controller, Controller::CompressorHeadsUp);
    }

    #[test]
    fn a_stale_room_drifts_after_the_minimum_cycle() {
        let mut c = Control::default();
        assert_eq!(c.decide(Mode::Heat(21.0), 18.0, None, 0.0).action, Action::Heat);
        c.stale = true;
        let d = c.decide(Mode::Heat(21.0), 18.0, None, 30.0);
        assert_eq!(d.controller, Controller::Lockout, "the minimum cycle still holds the heat");
        let d = c.decide(Mode::Heat(21.0), 18.0, None, 600.0);
        assert_eq!((d.controller, d.action, d.operation), (Controller::None, Action::Drift, Operation::Off));
    }

    #[test]
    fn heat_starts_below_the_band_and_the_overshoot_gate_ends_it() {
        // Target 21 °C: heats from 18.99, stops at stock's limit of
        // t + 0.39 - 0.2 (nest-qemu showed 21.2; re-measure).
        let mut c = Control::default();
        let mut log = Vec::new();
        let mut room = 18.99;
        let mut now = 0.0;
        while room < 21.3 {
            now += 30.0;
            let d = c.decide(Mode::Heat(21.0), room, None, now);
            log.push((room, d.controller, d.action));
            room += 0.01;
            room = (room * 100.0f32).round() / 100.0;
        }
        assert_eq!(log[0].1, Controller::LearningBand);
        let stop = log.iter().find(|(_, c, _)| *c == Controller::Overshoot).unwrap();
        assert!((stop.0 - 21.19).abs() < 0.005, "{}", stop.0);
        // Falling again: no heat until below target - band.
        let mut room = 21.2;
        let mut restart = None;
        while room > 20.0 {
            now += 30.0;
            if c.decide(Mode::Heat(21.0), room, None, now).action == Action::Heat {
                restart = Some(room);
                break;
            }
            room -= 0.01;
            room = (room * 100.0f32).round() / 100.0;
        }
        assert!((restart.unwrap() - (21.0 - DEFAULT_BAND)).abs() < 0.01, "{restart:?}");
    }

    #[test]
    fn a_cooling_cycle_runs_its_minimum_and_so_does_the_rest_after_it() {
        // Stock on nest-qemu, cooling to 19 °C from 20.3 °C: past the
        // overshoot limit the compressor runs on until five minutes are
        // up; stopped, it stays off five minutes too, outlasting its own
        // two and a half minute lockout.
        let mut c = Control::default();
        let cool = Mode::Cool(19.0);
        assert_eq!(c.decide(cool, 20.3, None, 0.0).operation, Operation::Stage1Cool);
        let held = c.decide(cool, 18.64, None, 200.0);
        assert_eq!((held.controller, held.operation), (Controller::Lockout, Operation::Stage1Cool));
        assert_eq!(held.lockout.map(|l| (l.0, l.1)), Some((Lockout::MinimumCycle, Operation::Off)));
        assert_eq!(c.deadline(), Some(300.0));
        let stop = c.decide(cool, 18.64, None, 300.0);
        assert_eq!((stop.controller, stop.operation), (Controller::Overshoot, Operation::Off));
        assert_eq!(c.deadline(), Some(450.0));
        let rest = c.decide(cool, 20.0, None, 460.0);
        assert_eq!((rest.controller, rest.action, rest.operation), (Controller::Lockout, Action::Drift, Operation::Off));
        assert_eq!(rest.lockout.map(|l| (l.0, l.1)), Some((Lockout::MinimumCycle, Operation::Stage1Cool)));
        assert_eq!(c.deadline(), Some(600.0));
        assert_eq!(c.decide(cool, 20.0, None, 600.0).operation, Operation::Stage1Cool);
    }

    #[test]
    fn a_new_target_ends_a_cycle_at_once_but_not_the_compressor_lockout() {
        // Stock: cooling to 18 °C, the target raised to 20 °C a minute in
        // stops it at once; lowered again, cooling waits out the 150 s.
        let mut c = Control::default();
        assert_eq!(c.decide(Mode::Cool(18.0), 18.9, None, 0.0).operation, Operation::Stage1Cool);
        let stop = c.decide(Mode::Cool(20.0), 18.45, None, 50.0);
        assert_eq!((stop.controller, stop.operation), (Controller::None, Operation::Off));
        let wait = c.decide(Mode::Cool(17.0), 18.45, None, 52.0);
        assert_eq!(wait.lockout.map(|l| l.0), Some(Lockout::Compressor));
        assert_eq!(c.decide(Mode::Cool(17.0), 18.45, None, 199.0).lockout.map(|l| l.0), Some(Lockout::Compressor));
        assert_eq!(c.decide(Mode::Cool(17.0), 18.45, None, 200.0).operation, Operation::Stage1Cool);
    }

    #[test]
    fn heat_off_and_on_again_waits_three_minutes() {
        let mut c = Control::default();
        let heat = Mode::Heat(21.0);
        assert_eq!(c.decide(heat, 20.0, None, 0.0).action, Action::Heat);
        assert_eq!(c.decide(heat, 21.2, None, 600.0).action, Action::Drift);
        let held = c.decide(heat, 20.5, None, 700.0);
        assert_eq!((held.controller, held.action), (Controller::Lockout, Action::Drift));
        assert_eq!(held.lockout.map(|l| (l.0, l.1)), Some((Lockout::MinimumCycle, Operation::Stage1Heat)));
        assert_eq!(c.decide(heat, 20.5, None, 780.0).action, Action::Heat);
    }

    #[test]
    fn airwave_stops_the_compressor_and_cools_on_with_the_fan() {
        // Stock on nest-qemu, cooling to 19 °C from 20.3 °C: the minimum
        // cycle holds the compressor past the overshoot limit; when it
        // ends Airwave stops the compressor, runs the fan alone for six
        // minutes, then drifts.
        let mut c = Control { fan_cooling: true, ..Control::default() };
        let cool = Mode::Cool(19.0);
        let d = c.decide(cool, 20.3, Some(45.0), 0.0);
        assert_eq!((d.controller, d.operation, d.fan), (Controller::LearningBand, Operation::Stage1Cool, false));
        assert_eq!(c.decide(cool, 18.64, Some(45.0), 290.0).controller, Controller::Lockout);
        let d = c.decide(cool, 18.64, Some(45.0), 300.0);
        assert_eq!((d.controller, d.action, d.operation, d.fan), (Controller::Airwave, Action::Cool, Operation::Off, true));
        let d = c.decide(cool, 18.89, Some(45.0), 659.0);
        assert_eq!((d.controller, d.action, d.fan), (Controller::Airwave, Action::Cool, true));
        let d = c.decide(cool, 18.89, Some(45.0), 660.0);
        assert_eq!((d.controller, d.action, d.operation, d.fan), (Controller::Airwave, Action::Drift, Operation::Off, false));
    }

    #[test]
    fn airwave_waits_for_a_dry_room_and_the_minimum_cycle() {
        let mut c = Control { fan_cooling: true, ..Control::default() };
        let cool = Mode::Cool(19.0);
        c.decide(cool, 20.3, Some(60.0), 0.0);
        // Humid: the overshoot gate ends the cycle instead.
        assert_eq!(c.decide(cool, 18.65, Some(60.0), 300.0).controller, Controller::Overshoot);
        // Dry, on a later cycle: Airwave, once the compressor has run 300 s.
        c.decide(cool, 19.5, Some(40.0), 900.0);
        assert_eq!(c.decide(cool, 18.64, Some(40.0), 1100.0).controller, Controller::Lockout);
        assert_eq!(c.decide(cool, 18.64, Some(40.0), 1200.0).controller, Controller::Airwave);
    }

    #[test]
    fn safety_heat_runs_from_the_low_safety_temperature_one_degree_up_whatever_the_mode() {
        // Stock on nest-qemu, off, 5 °C safety: heat at 5.0, off at 6.0.
        let mut c = Control { safety: (Some(5.0), None), ..Control::default() };
        assert_eq!(c.decide(Mode::Off, 5.01, None, 0.0).action, Action::Drift);
        let d = c.decide(Mode::Off, 5.0, None, 2.0);
        assert_eq!((d.controller, d.action, d.operation), (Controller::SafetyTemperatures, Action::Heat, Operation::Stage1Heat));
        assert_eq!(c.decide(Mode::Off, 5.99, None, 300.0).action, Action::Heat);
        // Off at once: no minimum cycle after the safety temperatures.
        let d = c.decide(Mode::Off, 6.0, None, 302.0);
        assert_eq!((d.controller, d.action), (Controller::None, Action::Drift));
        // Cooling too, with the high one.
        let mut c = Control { safety: (None, Some(35.0)), ..Control::default() };
        assert_eq!(c.decide(Mode::Heat(20.0), 35.0, None, 0.0).operation, Operation::Stage1Cool);
        assert_eq!(c.decide(Mode::Heat(20.0), 34.01, None, 10.0).operation, Operation::Stage1Cool);
        assert_eq!(c.decide(Mode::Heat(20.0), 34.0, None, 20.0).action, Action::Drift);
    }

    #[test]
    fn the_wake_thresholds_are_stocks() {
        // Measured on nest-qemu, safety 5 °C below and none above.
        let centi = |t: Option<f32>| t.map(|t| (f64::from(t) * 100.0 + 0.5) as i32);
        let mut c = Control { safety: (Some(5.0), None), ..Control::default() };
        c.decide(Mode::Heat(21.0), 21.3, None, 0.0);
        let w = c.wake(Mode::Heat(21.0));
        assert_eq!((centi(w.0), centi(w.1)), (Some(2061), None));
        c.decide(Mode::Heat(22.0), 21.3, None, 10.0);
        let w = c.wake(Mode::Heat(22.0));
        assert_eq!((centi(w.0), centi(w.1)), (Some(500), Some(2219)));
        let mut c = Control { safety: (Some(5.0), None), ..Control::default() };
        c.decide(Mode::Cool(20.0), 21.3, None, 0.0);
        let w = c.wake(Mode::Cool(20.0));
        assert_eq!((centi(w.0), centi(w.1)), (Some(1966), None));
        assert_eq!(c.wake(Mode::Off).0, Some(5.0));
    }

    #[test]
    fn electric_heat_runs_the_fan_first() {
        assert_eq!(switches(Operation::Stage1Heat, humidity::Op::Off, false, &electric()), [Switch::Fan, Switch::Stage1Heat]);
        let mut gas = electric();
        gas.source[Switch::Stage1Heat as usize] = 0;
        assert_eq!(switches(Operation::Stage1Heat, humidity::Op::Off, false, &gas), [Switch::Stage1Heat]);
        gas.fan_type[Switch::Stage1Heat as usize] = 0;
        assert_eq!(switches(Operation::Stage1Heat, humidity::Op::Off, false, &gas), [Switch::Fan, Switch::Stage1Heat], "fan type 0");
        assert_eq!(switches(Operation::Stage1Heat, humidity::Op::Off, false, &Equipment::default()), [Switch::Stage1Heat], "unknown: no fan");
        assert_eq!(switches(Operation::Stage1Cool, humidity::Op::Off, false, &Equipment::default()), [Switch::Fan, Switch::Stage1Cool]);
        let hp = Equipment { heat_pump: true, ..Equipment::default() };
        assert_eq!(switches(Operation::Stage1Cool, humidity::Op::Off, false, &hp), [Switch::Stage1Cool], "a heat pump cools by its heat stage's flag");
    }

    /// nest-qemu's store: electric forced air everywhere.
    fn electric() -> Equipment {
        Equipment { delivery: [0; 19], source: [1; 19], ..Equipment::default() }
    }

    fn staged() -> Control {
        Control { stages: (3, 2), ..Control::default() }
    }

    #[test]
    fn a_manual_target_far_above_the_room_starts_at_a_higher_stage() {
        let mut c = staged();
        c.decide(Mode::Heat(18.0), 18.0, None, 0.0);
        assert_eq!(c.decide(Mode::Heat(20.5), 18.5, None, 1.0).operation, Operation::Stage2Heat, "2 °C short");
        let mut c = staged();
        c.decide(Mode::Heat(18.0), 18.0, None, 0.0);
        assert_eq!(c.decide(Mode::Heat(23.0), 18.5, None, 1.0).operation, Operation::Stage3Heat, "4.5 °C short");
        let mut c = staged();
        c.own_target = false;
        c.decide(Mode::Heat(18.0), 18.0, None, 0.0);
        assert_eq!(c.decide(Mode::Heat(23.0), 18.5, None, 1.0).operation, Operation::Stage1Heat, "Eco's targets do not count");
    }

    #[test]
    fn a_stage_steps_up_when_the_room_slips_or_the_cycle_runs_long_and_never_drops() {
        let mut c = staged();
        assert_eq!(c.decide(Mode::Heat(21.0), 20.5, None, 0.0).operation, Operation::Stage1Heat);
        assert_eq!(c.decide(Mode::Heat(21.0), 19.9, None, 600.0).operation, Operation::Stage1Heat, "not 15 minutes yet");
        assert_eq!(c.deadline(), Some(900.0));
        assert_eq!(c.decide(Mode::Heat(21.0), 19.9, None, 900.0).operation, Operation::Stage2Heat, "0.6 °C the wrong way");
        assert_eq!(c.decide(Mode::Heat(21.0), 20.8, None, 1000.0).operation, Operation::Stage2Heat, "no drop within the cycle");
        let mut c = staged();
        c.decide(Mode::Cool(22.0), 22.5, None, 0.0);
        assert_eq!(c.decide(Mode::Cool(22.0), 22.4, None, 3600.0).operation, Operation::Stage1Cool, "cool's limit is two hours");
        assert_eq!(c.decide(Mode::Cool(22.0), 22.4, None, 7200.0).operation, Operation::Stage2Cool);
        assert_eq!(c.decide(Mode::Cool(22.0), 22.4, None, 9000.0).operation, Operation::Stage2Cool, "two cool stages at most");
    }

    #[test]
    fn slow_cycles_narrow_the_band_at_most_every_six_hours() {
        let mut c = Control::default();
        let mut now = 0.0;
        // Hour-long cycles gaining 0.9 °C (20.5 to 21.4, past any stop the
        // overshoot gate learns): the averages drift towards them.
        for _ in 0..200 {
            c.decide(Mode::Heat(21.0), 20.5, None, now);
            c.decide(Mode::Heat(21.0), 20.9, None, now + 3600.0);
            c.decide(Mode::Heat(21.0), 21.4, None, now + 3601.0);
            now += 4000.0;
        }
        assert!(c.band_models[0].minutes > 50.0, "{:?}", c.band_models[0]);
        assert_eq!(c.bands, [0.28, DEFAULT_BAND], "heat narrows, cool untouched");
        assert_eq!(BandModel::default().band(3.0), DEFAULT_BAND);
        assert_eq!(BandModel { change: 2.0, minutes: 2.0, ..BandModel::default() }.band(3.0), 0.5, "fast short cycles widen it");
    }

    fn heat_pump() -> Control {
        Control { heat_pump: Some(HeatPump::new(true, Some(-10.0), None, 3)), ..Control::default() }
    }

    #[test]
    fn a_heat_pump_heats_on_its_compressor_and_adds_aux_when_the_room_slips() {
        let mut c = heat_pump();
        c.outdoor = Some(0.0);
        assert_eq!(c.decide(Mode::Heat(21.0), 20.5, None, 0.0).operation, Operation::Stage1Heat, "never aux from off");
        assert_eq!(c.decide(Mode::Heat(21.0), 19.9, None, 900.0).operation, Operation::Stage1HeatAux);
        assert_eq!(switches(Operation::Stage1HeatAux, humidity::Op::Off, false, &Equipment { heat_pump: true, aux_heat: true, ..Equipment::default() }),
                   [Switch::Stage1Heat, Switch::AuxHeat]);
        assert_eq!(c.decide(Mode::Heat(21.0), 21.0, None, 1000.0).operation, Operation::Stage1Heat, "satisfied, aux drops out");
    }

    #[test]
    fn below_the_compressor_lockout_aux_heats_alone() {
        let mut c = heat_pump();
        c.outdoor = Some(-12.0);
        assert_eq!(c.decide(Mode::Heat(21.0), 20.5, None, 0.0).operation, Operation::AuxOnly);
    }

    #[test]
    fn emergency_heat_has_its_own_band_and_switch() {
        let mut c = heat_pump();
        let d = c.decide(Mode::Emergency(21.0), 20.6, None, 0.0);
        assert_eq!((d.controller, d.operation), (Controller::EmergencyHeat, Operation::EmergencyHeat));
        assert_eq!(c.decide(Mode::Emergency(21.0), 21.38, None, 60.0).operation, Operation::EmergencyHeat);
        assert_eq!(c.decide(Mode::Emergency(21.0), 21.39, None, 70.0).action, Action::Drift, "no minimum cycle");
    }

    #[test]
    fn a_heat_pump_rests_between_heating_and_cooling() {
        let mut c = heat_pump();
        c.decide(Mode::Range(20.0, 24.0), 19.5, None, 0.0);
        let d = c.decide(Mode::Range(20.0, 24.0), 24.5, None, 400.0);
        assert_eq!((d.controller, d.action), (Controller::Lockout, Action::Drift));
        assert_eq!(d.lockout.map(|l| l.0), Some(Lockout::Compressor));
    }

    #[test]
    fn a_safety_temperature_keeps_two_degrees_from_the_target() {
        let mut c = Control { safety: (Some(19.0), None), ..Control::default() };
        // Heating to 20, the low safety moves down to 18.
        assert_eq!(c.decide(Mode::Heat(20.0), 18.5, None, 0.0).controller, Controller::LearningBand);
        assert_eq!(c.decide(Mode::Heat(20.0), 18.0, None, 10.0).controller, Controller::SafetyTemperatures);
    }

    #[test]
    fn a_safety_run_is_remembered_within_two_point_three_degrees() {
        let heat = Mode::Heat(12.0);
        let mut c = Control { safety: (Some(10.0), None), ..Control::default() };
        assert_eq!(c.decide(heat, 9.9, None, 0.0).controller, Controller::SafetyTemperatures);
        // A degree up the band heats on.
        let d = c.decide(heat, 11.0, None, 600.0);
        assert_eq!((d.controller, d.action), (Controller::LearningBand, Action::Heat));
        // Still within 2.3 °C: the safety band follows the heating.
        let d = c.decide(heat, 10.5, None, 1200.0);
        assert_eq!((d.controller, d.action), (Controller::SafetyTemperatures, Action::Heat));
        // Past 12.3 °C it is forgotten: back at 10.5 the band heats.
        c.decide(heat, 12.4, None, 1800.0);
        c.decide(heat, 11.5, None, 2400.0);
        let d = c.decide(heat, 10.5, None, 3000.0);
        assert_eq!((d.controller, d.action), (Controller::LearningBand, Action::Heat));
        assert_eq!(c.decide(heat, 10.0, None, 3600.0).controller, Controller::SafetyTemperatures);
    }

    #[test]
    fn a_manual_raise_inside_the_band_heats_at_once_until_two_tenths_up() {
        let mut c = Control { manual_target: true, ..Control::default() };
        c.decide(Mode::Heat(20.0), 20.1, None, 0.0);
        let d = c.decide(Mode::Heat(20.3), 20.1, None, 10.0);
        assert_eq!((d.controller, d.action), (Controller::Overeager, Action::Heat), "20.1 is inside 20.3's band");
        assert_eq!(c.decide(Mode::Heat(20.3), 20.25, None, 400.0).action, Action::Heat);
        // 0.2 up from 20.1 OVEREAGER ends; the overshoot gate is back and
        // stops the cycle at t + 0.19.
        assert_eq!(c.decide(Mode::Heat(20.3), 20.31, None, 500.0).action, Action::Heat);
        assert_eq!(c.decide(Mode::Heat(20.3), 20.5, None, 600.0).controller, Controller::Overshoot);
        let mut c = Control::default();
        c.decide(Mode::Heat(20.0), 20.1, None, 0.0);
        assert_eq!(c.decide(Mode::Heat(20.3), 20.1, None, 10.0).action, Action::Drift, "a schedule's raise waits for the band");
    }

    #[test]
    fn airwave_learns_from_the_lowest_room_after_the_compressor_stops() {
        let mut c = Control { fan_cooling: true, ..Control::default() };
        let cool = Mode::Cool(19.0);
        c.decide(cool, 20.3, Some(40.0), 0.0);
        c.decide(cool, 18.85, Some(40.0), 400.0);
        assert!(c.airwave_track.is_some(), "the fan took over");
        c.decide(cool, 18.7, Some(40.0), 600.0);
        c.decide(cool, 18.6, Some(40.0), 700.0);
        c.decide(cool, 18.62, Some(40.0), 760.0);
        assert!(c.airwave_track.is_none(), "the room turned");
        assert!(c.airwave_offset < AIRWAVE_DEFAULT.0, "a 0.25 °C drop pulls the offset towards it: {}", c.airwave_offset);
        assert!(c.airwave_fan < AIRWAVE_DEFAULT.1, "300 s pulls the fan run down: {}", c.airwave_fan);
    }

    fn test(operation: Operation) -> Option<Test> {
        Some(Test { operation, humidity: humidity::Op::Off, fan: None })
    }

    #[test]
    fn a_system_test_runs_its_stage_whatever_the_mode_until_withdrawn() {
        let mut c = Control { test: test(Operation::Stage1Heat), ..Control::default() };
        let d = c.decide(Mode::Off, 22.0, None, 0.0);
        assert_eq!((d.controller, d.action, d.operation), (Controller::SystemTest, Action::Heat, Operation::Stage1Heat));
        c.test = None;
        let d = c.decide(Mode::Off, 22.0, None, 600.0);
        assert_eq!((d.action, d.operation), (Action::Drift, Operation::Off));
    }

    #[test]
    fn a_system_test_keeps_to_the_minimum_cycle_and_the_compressor_lockout() {
        let mut c = Control::default();
        assert_eq!(c.decide(Mode::Cool(20.0), 26.0, None, 0.0).operation, Operation::Stage1Cool);
        // Off at once (a new mode), the compressor resting.
        assert_eq!(c.decide(Mode::Off, 26.0, None, 400.0).operation, Operation::Off);
        c.test = test(Operation::Stage1Cool);
        let d = c.decide(Mode::Off, 26.0, None, 410.0);
        assert_eq!((d.controller, d.operation), (Controller::Lockout, Operation::Off));
        assert_eq!(d.lockout.map(|l| (l.0, l.1)), Some((Lockout::MinimumCycle, Operation::Stage1Cool)), "the rest's five minutes outlast the compressor's 150 s");
        let d = c.decide(Mode::Off, 26.0, None, 560.0);
        assert_eq!(d.controller, Controller::Lockout);
        let d = c.decide(Mode::Off, 26.0, None, 700.0);
        assert_eq!((d.controller, d.operation), (Controller::SystemTest, Operation::Stage1Cool));
        // Withdrawn, the compressor still runs its minimum.
        c.test = None;
        let d = c.decide(Mode::Off, 26.0, None, 710.0);
        assert_eq!((d.controller, d.operation), (Controller::Lockout, Operation::Stage1Cool));
        assert_eq!(c.decide(Mode::Off, 26.0, None, 1000.0).operation, Operation::Off);
        c.test = test(Operation::Stage1Cool);
        let d = c.decide(Mode::Off, 26.0, None, 1300.0);
        assert_eq!((d.controller, d.operation), (Controller::SystemTest, Operation::Stage1Cool));
        c.test = test(Operation::Stage1Heat);
        let d = c.decide(Mode::Off, 26.0, None, 1700.0);
        assert_eq!((d.controller, d.action), (Controller::SystemTest, Action::Heat));
        // Another stage soon after holds on to the heat for its minimum.
        c.test = test(Operation::Stage1Cool);
        let d = c.decide(Mode::Off, 26.0, None, 1750.0);
        assert_eq!((d.controller, d.action), (Controller::Lockout, Action::Heat));
        assert_eq!(d.lockout.map(|l| (l.0, l.1)), Some((Lockout::MinimumCycle, Operation::Stage1Cool)));
    }

    #[test]
    fn the_safety_temperatures_come_before_a_system_test() {
        let mut c = Control { safety: (Some(5.0), None), test: test(Operation::Stage1Cool), ..Control::default() };
        let d = c.decide(Mode::Off, 4.0, None, 0.0);
        assert_eq!((d.controller, d.action), (Controller::SafetyTemperatures, Action::Heat));
    }

    #[test]
    fn a_system_test_sets_the_humidity_and_the_humidistat_stands_aside() {
        let mut c = Control::default();
        c.humidistat.config.dehumidifier = 1;
        c.humidistat.config.target = Some(30.0);
        c.test = Some(Test { operation: Operation::Off, humidity: humidity::Op::Humidify, fan: None });
        let d = c.decide(Mode::Off, 22.0, Some(60.0), 0.0);
        assert_eq!((d.controller, d.humidity), (Controller::SystemTest, humidity::Op::Humidify));
        let d = Control::default().decide(Mode::Off, 22.0, Some(60.0), 0.0);
        assert_eq!(d.humidity, humidity::Op::Off);
    }
}
