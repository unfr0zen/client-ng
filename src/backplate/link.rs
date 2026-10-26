//! The link to the backplate as stock nlclient's `nlBackplateController`
//! runs it (client-ng/BACKPLATE.md): an event queue driving a state machine,
//! with the reset and description handshake, the live reports and their
//! buffers, switching and its acks, the error queries, the periodic tick
//! and the watches on a silent backplate and a missed temperature. It does
//! no I/O: it takes the frames received and the time, and gives the frames
//! to send and what the backplate reported.

use std::collections::VecDeque;

use super::frame::Frame;
use crate::hvac::wiring::Switch;

/// Frame types, as on the wire.
pub mod kind {
    pub const TEMP: u16 = 0x02;
    pub const WIRES: u16 = 0x04;
    pub const PIR: u16 = 0x05;
    pub const NEAR_PIR: u16 = 0x07;
    pub const ACK_SWITCH: u16 = 0x06;
    pub const PINS: u16 = 0x09;
    pub const ALS: u16 = 0x0a;
    pub const ALS_BUF: u16 = 0x29;
    pub const PWR: u16 = 0x0b;
    pub const FW_ID: u16 = 0x10;
    pub const UPDATE_ACK: u16 = 0x11;
    pub const FW_ID2: u16 = 0x12;
    pub const WAKEUP: u16 = 0x13;
    pub const WAKEUP_VEC: u16 = 0x14;
    pub const TP: u16 = 0x15;
    pub const BP_STATUS: u16 = 0x17;
    pub const FW_VERS: u16 = 0x18;
    pub const FW_INFO: u16 = 0x19;
    pub const BSL_VERS: u16 = 0x1b;
    pub const BSL_INFO: u16 = 0x1c;
    pub const BSL_ID: u16 = 0x1d;
    pub const MODEL: u16 = 0x1e;
    pub const SERIAL: u16 = 0x1f;
    pub const END_BUF: u16 = 0x2f;
    pub const TEMP2: u16 = 0x23;
    pub const ERROR_VECTOR: u16 = 0x30;
    pub const OC_ERRORS: u16 = 0x31;
    pub const TP_ERRORS: u16 = 0x32;
    pub const SAFETY_TEMP_DATA: u16 = 0x35;
    pub const IPS_STATUS: u16 = 0x21;
    pub const MCU_MODE_ACK: u16 = 0x36;

    pub const SWITCH: u16 = 0x82;
    pub const TICK: u16 = 0x83;
    pub const GET_BP_STATUS: u16 = 0x85;
    pub const GET_SAFETY_TEMP_DATA: u16 = 0x8a;
    pub const SET_SAFETY_TEMP: u16 = 0x89;
    pub const GET_IPS_STATUS: u16 = 0x86;
    pub const SET_MCU_MODE: u16 = 0x8b;
    pub const BP_STATUS_ACK: u16 = 0x8c;
    pub const WIRES_ECHO: u16 = 0x8f;
    pub const GET_FW_ID: u16 = 0x90;
    pub const GET_FW_VERS: u16 = 0x98;
    pub const GET_FW_INFO: u16 = 0x99;
    pub const GET_BSL_VERS: u16 = 0x9b;
    pub const GET_BSL_INFO: u16 = 0x9c;
    pub const GET_BSL_ID: u16 = 0x9d;
    pub const GET_MODEL: u16 = 0x9e;
    pub const GET_SERIAL: u16 = 0x9f;
    pub const UPDATE_BEGIN: u16 = 0x91;
    pub const UPDATE_RECORD: u16 = 0x92;
    pub const UPDATE_END: u16 = 0x93;
    pub const QUIET: u16 = 0xa1;
    pub const FLUSH: u16 = 0xa2;
    pub const END_BUF_ACK: u16 = 0xa3;
    pub const WAKE_SOURCES: u16 = 0xa4;
    pub const TEMPERATURE_LOCK: u16 = 0xb1;
    pub const HUMIDITY_WAKE: u16 = 0xb3;
    pub const NEAR_PIR_THRESHOLD: u16 = 0xb5;
    pub const TP_THRESHOLDS: u16 = 0xb6;
    pub const PIR_SENSITIVITY: u16 = 0xb7;
    pub const ALS_WAKE: u16 = 0xb9;
    pub const TEMP_COMP_MODE: u16 = 0xc3;
    pub const VERGENCE_WAKE: u16 = 0xba;
    pub const POWER_STEAL: u16 = 0xc0;
    pub const TEMP_WAKE: u16 = 0xc2;
    pub const GET_ERROR: u16 = 0xc4;
    pub const GET_OC_ERRORS: u16 = 0xc5;
    pub const GET_TP_ERRORS: u16 = 0xc6;
    pub const RESET: u16 = 0xff;

    /// Whether stock maps a received type to an event (0x309174): others
    /// are dropped unseen and do not count as hearing the backplate.
    pub fn known(kind: u16) -> bool {
        !matches!(kind, 0x00 | 0x03 | 0x0d | 0x0f | 0x1a | 0x20 | 0x2a | 0x2c..=0x2e | 0x33 | 0x34 | 0x37) && kind <= 0x38
    }

    /// A type's name for the log.
    pub fn name(kind: u16) -> &'static str {
        match kind {
            TEMP => "temp",
            WIRES => "wires",
            PIR => "pir",
            ACK_SWITCH => "switch ack",
            NEAR_PIR => "near pir",
            PINS => "pins",
            ALS => "als",
            PWR => "power",
            FW_ID => "fw id",
            UPDATE_ACK => "update ack",
            FW_ID2 => "fw id2",
            WAKEUP => "wakeup",
            WAKEUP_VEC => "wakeup vector",
            TP => "thermopile",
            BP_STATUS => "bp status",
            FW_VERS => "fw version",
            FW_INFO => "fw info",
            BSL_VERS => "bsl version",
            BSL_INFO => "bsl info",
            BSL_ID => "bsl id",
            MODEL => "model",
            SERIAL => "serial",
            IPS_STATUS => "ips status",
            0x22 => "temp buf",
            TEMP2 => "temp2 buf",
            0x27 => "pir buf",
            0x28 => "prox buf",
            ALS_BUF => "als buf",
            END_BUF => "end of buffers",
            ERROR_VECTOR => "error vector",
            OC_ERRORS => "oc errors",
            TP_ERRORS => "tp errors",
            SAFETY_TEMP_DATA => "safety temp data",
            MCU_MODE_ACK => "mcu mode ack",
            SWITCH => "switch",
            TICK => "tick",
            GET_BP_STATUS => "get bp status",
            GET_IPS_STATUS => "get ips status",
            SET_SAFETY_TEMP => "set safety temp",
            GET_SAFETY_TEMP_DATA => "get safety temp data",
            SET_MCU_MODE => "set mcu mode",
            BP_STATUS_ACK => "bp status ack",
            WIRES_ECHO => "wires echo",
            GET_FW_ID => "get fw id",
            UPDATE_BEGIN => "update begin",
            UPDATE_RECORD => "update record",
            UPDATE_END => "update end",
            GET_FW_VERS => "get fw version",
            GET_FW_INFO => "get fw info",
            GET_BSL_VERS => "get bsl version",
            GET_BSL_INFO => "get bsl info",
            GET_BSL_ID => "get bsl id",
            GET_MODEL => "get model",
            GET_SERIAL => "get serial",
            QUIET => "quiet",
            FLUSH => "flush",
            END_BUF_ACK => "end of buffers ack",
            WAKE_SOURCES => "wake sources",
            TEMPERATURE_LOCK => "temperature lock",
            HUMIDITY_WAKE => "humidity wake",
            NEAR_PIR_THRESHOLD => "near pir threshold",
            TP_THRESHOLDS => "tp thresholds",
            PIR_SENSITIVITY => "pir sensitivity",
            ALS_WAKE => "als wake",
            VERGENCE_WAKE => "vergence wake",
            POWER_STEAL => "power steal",
            TEMP_WAKE => "temp wake",
            TEMP_COMP_MODE => "temp comp mode",
            GET_ERROR => "get error",
            GET_OC_ERRORS => "get oc errors",
            GET_TP_ERRORS => "get tp errors",
            RESET => "reset",
            _ => "unknown",
        }
    }

    /// The buffered reports an end of buffers counts: TEMP_BUF, TEMP2_BUF,
    /// PIR_BUF, PROX_BUF and ALS_BUF.
    pub fn counted(kind: u16) -> bool {
        matches!(kind, 0x22 | 0x23 | 0x27 | 0x28 | 0x29)
    }
}

/// The controller's states that the link uses (BACKPLATE.md has them all).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    WaitingAcks,
    WaitingWireReport,
    WaitingPinReport,
    WaitingFwId,
    WaitingFwVers,
    WaitingFwInfo,
    WaitingBslId,
    WaitingBslVers,
    WaitingBslInfo,
    WaitingSerial,
    WaitingModel,
    WaitingBPStatus,
    Error,
    WaitingError,
    WaitingErrorAcks,
    WaitingSafetyTempData,
    WaitingSafetyTempSetAck,
    WaitingMCUModeSetAck,
    WaitingIPSStatus,
    WaitingUpdateBeginAck,
    WaitingUpdateSRecordAcks,
    WaitingUpdateEndAck,
    Exiting,
}

impl State {
    /// The wire and pin waits, where stock neither ticks nor counts frames
    /// as heard.
    fn resetting(self) -> bool {
        matches!(self, State::WaitingWireReport | State::WaitingPinReport)
    }

    fn updating(self) -> bool {
        matches!(self, State::WaitingUpdateBeginAck | State::WaitingUpdateSRecordAcks | State::WaitingUpdateEndAck)
    }

    fn describing(self) -> bool {
        matches!(self, State::WaitingFwId | State::WaitingFwVers | State::WaitingFwInfo | State::WaitingBslId
            | State::WaitingBslVers | State::WaitingBslInfo | State::WaitingSerial | State::WaitingModel)
    }
}

/// What to put on the line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Out {
    /// A UART break, which precedes a reset request.
    Break,
    Send(Frame),
    /// A 20 ms pulse on the reset line: the backplate restarts.
    HardReset,
}

/// The backplate's description, from the handshake.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Identity {
    pub firmware_id: Vec<u8>,
    pub firmware_version: String,
    pub firmware_info: String,
    pub bsl_id: u16,
    pub bsl_version: String,
    pub bsl_info: String,
    pub serial: String,
    pub model: String,
}

/// Why stock resets the backplate (its nlBackplateResetReason).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    TimeoutComms = 1,
    Exit = 2,
    CorruptType = 3,
    Retries = 6,
    UpdateComplete = 8,
    Attach = 9,
    MissedTemp = 10,
    I2cDown = 11,
    TpError = 12,
    CorruptedTempMsg = 13,
    BpPowerMismatch = 14,
    GrilleTempErrorMsg = 15,
    HuTempErrorMsg = 16,
    TempSensorIssue = 17,
    TempSensorIssueDelayed = 18,
}

/// What the backplate reported.
#[derive(Clone, Debug, PartialEq)]
pub enum Report {
    /// The room as stock keeps it: the last valid temperature (20 °C until
    /// one arrives), and the humidity, None while stock holds 0.
    Climate { celsius: f32, humidity: Option<f32> },
    /// Someone moved in front of the unit; `near`, close to it (the near
    /// PIR or the thermopile, stock's approach), else across the room (the
    /// PIR).
    Motion { near: bool },
    /// The ambient light's visible count, the last valid one.
    Light { visible: u16 },
    /// The terminals with a wire (MSG_WIRES order: W1, Y1, G, O/B, AUX/W2,
    /// -, -, Y2, C, Rc, Rh, STAR, -).
    Wires([u8; 13]),
    Pins([u8; 15]),
    /// The handshake is over: the link is up.
    Ready(Identity),
    /// A switch request was acknowledged: circuit and state.
    Switched { circuit: u8, on: bool },
    /// The backplate is being reset, `hard` with the reset line.
    Reset { reason: Reason, hard: bool },
    /// Resets brought nothing back: stock calls the link down and stops
    /// resetting until the backplate answers again.
    Down,
    /// The detect line settled, attached or not.
    Attached(bool),
    /// ALS_BUF's samples, a 5 minutes, the newest last (0xffff unread).
    LightBuffer(Vec<u16>),
    /// The temperature crossed the vergence threshold Sunblock set.
    Vergence,
    /// A firmware image ('B' bootloader, 'M' firmware) went over, its end
    /// acknowledged.
    UpdateSent(u8),
    /// The firmware update started, and how it ended (from the update
    /// driver, not the link).
    Updating,
    UpdateDone(bool),
    /// The backplate's power report (MSG_PWR, 0x308974): the input voltage
    /// in mV (falling back to the second when 0), the second voltage in
    /// 10 mV, the current drawn in mA as stock estimates it, and its power
    /// state (0 to 9, the last valid one).
    Power { vusb: u16, vin: u16, current: u16, state: u8 },
    /// Over-current errors went on after ten re-tries: stock clears the
    /// switches and raises its alert; false once the errors clear.
    SwitchFault(bool),
    /// The sensors read nothing after a reset: stock's reattach alert; false
    /// once they read again or the backplate was put back.
    I2cDown(bool),
    /// Whether that alert may be raised (`i2c_alert_allowed_to_trigger`),
    /// to be kept.
    I2cAllowed(bool),
    /// The circuits with an over-current fault, as they change.
    Faults([bool; 13]),
}

/// What may wake the unit while it sleeps (stock's sleep settings, sent by
/// 0x314860): room temperatures, each held for a duration (0: until
/// woken), and the longest the unit may sleep.
#[derive(Clone, Debug, PartialEq)]
pub struct Wake {
    /// (seconds, low °C, high °C); None leaves a side unwatched.
    pub steps: Vec<(u16, Option<f32>, Option<f32>)>,
    pub max_sleep: u16,
    /// Relative humidity (%), low and high; None leaves a side unwatched.
    pub humidity: (Option<f32>, Option<f32>),
    /// Sunblock's ALS_WAKE (low, high) and VERGENCE_WAKE (mode, centi-°C,
    /// second).
    pub als: (u16, u16),
    pub vergence: (u16, i16, i16),
}

/// A humidity wake threshold as stock sends it (0x313f44): tenths of a
/// percent, 0xffff unwatched.
fn per_mille(h: Option<f32>) -> [u8; 2] {
    h.map_or(0xffff, |h| (h * 10.0 + 0.5).clamp(0.0, 65534.0) as u16).to_le_bytes()
}

/// Bytes as hex for the log.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

/// A frame for the log: its type's name and number, and its payload.
pub fn frame(f: &Frame) -> String {
    format!("{} (0x{:02x}) [{}] {}", kind::name(f.kind), f.kind, f.payload.len(), hex(&f.payload))
}

/// The terminals MSG_WIRES shows wired, for the log.
fn wires(p: &[u8]) -> String {
    let names = ["W1", "Y1", "G", "O/B", "AUX/W2", "-", "-", "Y2", "C", "Rc", "Rh", "STAR", "-"];
    let on: Vec<&str> = names.iter().zip(p).filter(|&(_, &b)| b & 1 != 0).map(|(n, _)| *n).collect();
    if on.is_empty() { "none".into() } else { on.join(" ") }
}

/// A wake threshold for the log.
fn side(t: Option<f32>) -> String {
    t.map_or_else(|| "-".to_string(), |t| format!("{t:.2}"))
}

/// Centi-degrees as stock rounds them; i16::MIN for an unwatched side.
fn centi(t: Option<f32>) -> [u8; 2] {
    t.map_or(i16::MIN, |t| (f64::from(t) * 100.0 + 0.5) as i16).to_le_bytes()
}

/// The controller's events (its Nuovations::Osm events, those the client
/// raises).
#[derive(Clone, Debug, PartialEq)]
enum Event {
    Message(Frame),
    Reset { hard: bool },
    Switch { switch: Switch, on: bool },
    ClearSwitches,
    Flush,
    Quiet(Wake),
    Tick,
    Restart,
    TemperatureLock,
    PowerSteal,
    RequestBPStatus,
    GetSafetyTempData,
    SetSafetyTemp,
    SetMCUMode,
    RequestIPSStatus,
    BeginUpdate,
    ReceiveError,
    ErrorClear,
    GetError,
    AllAcksIn,
    Retry,
    Exit,
}

impl Event {
    /// Events stock keeps for later when the state has no use for them;
    /// the rest it drops.
    fn deferrable(&self) -> bool {
        matches!(self, Event::Reset { .. } | Event::Switch { .. } | Event::ClearSwitches | Event::Flush | Event::Quiet(_)
            | Event::Restart | Event::BeginUpdate | Event::RequestBPStatus | Event::GetSafetyTempData | Event::SetSafetyTemp | Event::SetMCUMode | Event::RequestIPSStatus | Event::GetError
            | Event::Exit)
    }
}

/// How long the backplate may be silent before a reset; how many resets go
/// out over the UART before the reset line is used (with the battery at
/// `HARD_RESET_VOLTS` or more), and how many in all before the link is
/// down (nlBackplateController 0x318424, 0x317b94).
pub const SILENCE: f64 = 5.0;
pub const SOFT_RESETS: u8 = 3;
pub const RESETS: u8 = 6;
pub const HARD_RESET_VOLTS: f32 = 3.7;
/// How long after its last send stock repeats what it awaits, how many
/// times before a reset, and how long a flush may go unanswered before it
/// is sent again (0x3162d0, 0x317e48, 0x31311c).
pub const RETRY: f64 = 2.0;
pub const RETRIES: u8 = 6;
/// The IPS status is asked for every 7 days, and given up on after three
/// retries.
const IPS_EVERY: f64 = 7.0 * 24.0 * 3600.0;
const IPS_RETRIES: u8 = 3;

/// The backplate's version as stock numbers its models (0xb9058): 4 and
/// up for Backplate-5 and later, -1 unknown.
pub fn backplate_version(model: &str) -> i32 {
    match model.strip_prefix("Backplate-").map(|v| v.split('.').next().unwrap_or("")) {
        Some("1") if model.starts_with("Backplate-1.9a") => 1,
        Some("1") => 0,
        Some("2") => 2,
        Some("3") => 3,
        Some("5") => 4,
        Some("6") => 5,
        Some("7") => 6,
        Some("8") => 7,
        _ => -1,
    }
}
pub const FLUSH_TIMEOUT: f64 = 5.0;
/// What one unit's stock controller does that the other's does not, from
/// the same `nlbackplatecontroller.cpp` (client-ng/BACKPLATE.md).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    /// After each description it asks for the MCU mode and sends the
    /// safety temperatures (the gen 3's `0x305d80`); else only the
    /// power-steal mode (the gen 2's `0x1bd160`).
    pub describe_all: bool,
    /// It asks for the safety temperature data every day.
    pub daily_safety: bool,
    /// Temperatures 0x8001 and 0x8002 are the grille's and the head unit's
    /// errors, the error resets count as resets, and the TEMP2 samples are
    /// checked for a sensor issue; on the gen 2 only 0x8000 is an error,
    /// reset with the line at once.
    pub sensor_checks: bool,
    /// The wake sources quiet always sets.
    pub wake_sources: u16,
    /// It sends the PIR's sensitivity (`b7`) on going quiet.
    pub pir_wake: bool,
}

impl Profile {
    /// The gen 3's controller.
    pub const GEN3: Profile = Profile { describe_all: true, daily_safety: true, sensor_checks: true, wake_sources: 0x0f6f, pir_wake: true };
    /// The gen 2's: base wake sources 0x567 with VBAT (`0x1c7a04`).
    pub const GEN2: Profile = Profile { describe_all: false, daily_safety: false, sensor_checks: false, wake_sources: 0x0767, pir_wake: false };
}

/// The MCU mode stock asks for once identified (`0x305d80`: 1, or 2 under
/// a setting the client does not have).
pub const MCU_MODE: u8 = 1;
/// Seconds between ticks (nlBackplateController: 30 s).
pub const TICK: f64 = 30.0;
/// The longest stock's controller loop sleeps: its poll timeout. Retries,
/// ticks and the silence watch run each time it wakes.
pub const LOOP: f64 = 2.0;
/// How often stock asks for the backplate's safety temperature data.
pub const SAFETY_DATA: f64 = 86400.0;
/// How long after waking stock waits for a temperature before a reset.
pub const MISSED_TEMP: f64 = 35.0;
/// The order stock switches off in, and on in (0x319580).
const OFF_ORDER: [Switch; 17] = [
    Switch::WaterHeater, Switch::EmergencyHeat, Switch::AuxHeat, Switch::Stage2Cool, Switch::Stage3Cool, Switch::Stage2Heat,
    Switch::Stage3Heat, Switch::Stage1AltHeat, Switch::Stage2AltHeat, Switch::Stage1Cool, Switch::Stage1Heat, Switch::Ob,
    Switch::Humidifier, Switch::Dehumidifier, Switch::Fan, Switch::Stage2Fan, Switch::Stage3Fan,
];
const ON_ORDER: [Switch; 16] = [
    Switch::Fan, Switch::Stage1Cool, Switch::Stage1Heat, Switch::Stage2Cool, Switch::Stage3Cool, Switch::Stage2Heat,
    Switch::Stage3Heat, Switch::Stage1AltHeat, Switch::Stage2AltHeat, Switch::AuxHeat, Switch::EmergencyHeat,
    Switch::Humidifier, Switch::Dehumidifier, Switch::Stage2Fan, Switch::Stage3Fan, Switch::WaterHeater,
];
/// The thermopile's thresholds when the configuration has none (stock's
/// sleep logic and controller defaults): left-right and up-down maxima,
/// then minima.
const TP_MAX: i16 = 100;
const TP_MIN: (i16, i16) = (-44, -69);
/// The deferred events stock keeps at most.
const DEFERRED: usize = 32;
/// Silence resets skipped while bytes wait unread.
const SKIPS: u8 = 2;
/// Over-current re-tries before stock gives up on the switches.
const OC_RETRIES: u32 = 10;
/// Thermopile-error resets allowed within `TP_WINDOW` seconds.
const TP_RESETS: u16 = 5;
const TP_WINDOW: f64 = 7.0 * 24.0 * 3600.0;
/// Power-mismatch resets in a row before stock stops trying.
const MISMATCH_RESETS: u8 = 5;

/// The current a power report's px0 byte stands for, in mA (0x30c1e8):
/// none when its 0x10 bit is set.
fn current(px0: u8) -> u16 {
    if px0 & 0x10 != 0 {
        return 0;
    }
    match px0 {
        3 => 8,
        5 => 30,
        7 => 40,
        10 => 200,
        11 => 20,
        15 => 100,
        _ => 0,
    }
}

/// A firmware image going over: its kind, its S-records, and the
/// sequence number awaiting its ack.
struct Transfer {
    kind: u8,
    records: Vec<Vec<u8>>,
    seq: u16,
}

pub struct Link {
    state: State,
    transfer: Option<Transfer>,
    queue: VecDeque<Event>,
    deferred: VecDeque<Event>,
    wires: [u8; 13],
    pins: [u8; 15],
    /// The last valid power state, and the power-mismatch resets in a row.
    power_state: u8,
    mismatches: u8,
    /// The TEMP2 watches, the samples since the last end of buffers, and
    /// when the backplate was last asked to reset.
    i2c: super::sensors::I2c,
    sanity: super::sensors::Sanity,
    temp2: Vec<super::sensors::Sample>,
    last_reset: f64,
    identity: Identity,
    /// Switch requests awaiting their ack, and the state asked of each
    /// circuit.
    acks: [bool; 13],
    switched: [bool; 13],
    /// The HVAC control's switches as last asked, and as last switched
    /// (stock's 0x7b8, forgotten with every reset), and each switch's
    /// circuit.
    wanted: [bool; 19],
    old: [bool; 19],
    circuits: [Option<u8>; 19],
    /// Circuits asked on again for over-current errors.
    oc_flagged: [bool; 13],
    /// The circuits faulted when stock gave up on them (the give-up
    /// message's bytes, kept until the errors clear), and as last reported.
    faults: [bool; 13],
    faults_reported: [bool; 13],
    /// The dehumidifier's circuit runs inverted (`dehumidifier_orientation_selected`).
    dehumidifier_reversed: bool,
    /// A PTAC or fan coil (`y2_type` G2): stage-2 cool and stage-2 and 3
    /// heat are never sent.
    ptac: bool,
    /// The thermopile's motion thresholds, left/right and up/down
    /// (`thermopileleftrightmaxthreshold`, `thermopileupdownmaxthreshold`).
    tp_max: (i16, i16),
    tp_min: (i16, i16),
    /// The thermopile may wake the unit: the unit has one and wakes on
    /// approach.
    tp_wake: bool,
    /// The near PIR's threshold, on a unit that has one: a reading at or
    /// above it is someone approaching.
    near_pir: Option<u16>,
    profile: Profile,
    /// `power_steal_mode` as stock numbers it (0x0cf554): 0 normal, 1
    /// disabled, 2 disabled while heating, 3 disabled while cooling.
    power_steal_mode: u8,
    /// A flush awaits its end of buffers, sent at `flushed`; `buffered`
    /// counts the buffered reports since.
    flushing: bool,
    flushed: f64,
    buffered: u16,
    /// The room as stock keeps it.
    celsius: f32,
    humidity: f32,
    visible: u16,
    /// A temperature arrived since the unit last went quiet.
    temp_seen: bool,
    /// When the watch on a missed temperature last started, and when the
    /// daily safety data is next due.
    awake_since: f64,
    safety_due: f64,
    /// Told the backplate the unit sleeps: no ticks and no watch on
    /// silence until the next send or read.
    quiet: bool,
    heard: f64,
    ticked: f64,
    skips: u8,
    /// The time as of the last call, of the last send, and of the last
    /// turn of stock's loop.
    clock: f64,
    sent: f64,
    turned: f64,
    /// Bytes arrived since the last turn: stock's loop wakes for them.
    read: bool,
    /// Repeats since the link last had nothing to wait for.
    retries: u8,
    /// The MCU mode last asked for (0: none yet).
    mcu_mode: u8,
    /// Resets since the backplate last identified itself.
    resets: u8,
    /// Whether resets still go out (stock's comm up).
    up: bool,
    /// The detect line, and whether it has ever said attached.
    attached: bool,
    attached_once: bool,
    /// The reason for the reset that follows the BP status, and whether
    /// it waits for the safety temperatures first.
    status_reason: Reason,
    status_hold: bool,
    /// The backplate's own safety temperatures (0x89's payload), when the
    /// unit gives it any (outside the Heat Link gate).
    safety: Option<[u8; 16]>,
    /// When the IPS status was last considered.
    ips_at: f64,
    /// The error query: errors to ask for, the reasons reported, the
    /// over-current re-tries and whether one is running, whether the
    /// alert is up, and whether a ReceiveError was already deferred.
    need_errors: bool,
    error_reasons: u16,
    oc_retries: u32,
    oc_running: bool,
    alerted: bool,
    error_deferred: bool,
    /// Thermopile errors: resets in the current window and its start, and
    /// whether thermopile wake is off.
    tp_resets: u16,
    tp_window: f64,
    pub tp_wake_disabled: bool,
    /// The simulator supplies the room (stock's +0x410): temperature
    /// reports only flush and count as heard; or the wiring (+0x414): the
    /// wires and pins reported go unused and switches are not sent.
    sim_temperature: bool,
    sim_wiring: bool,
    out: Vec<Out>,
    reports: Vec<Report>,
    /// What the log last said of the room, the light, the power state
    /// and motion, so it says it again only on a change.
    logged_climate: (f32, f32),
    logged_visible: Option<u16>,
    logged_power: Option<u8>,
    logged_motion: f64,
}

impl Link {
    /// A link starting at `now`, the detect line as read: stock asks for
    /// the power-steal mode, then once attached the backplate's status,
    /// and once it answers the reset handshake.
    pub fn new(now: f64, attached: bool) -> Link {
        let mut link = Link {
            state: State::Idle,
            transfer: None,
            queue: VecDeque::new(),
            deferred: VecDeque::new(),
            wires: [0; 13],
            pins: [0; 15],
            power_state: 0,
            mismatches: 0,
            i2c: super::sensors::I2c::new(now, true),
            sanity: super::sensors::Sanity::new(now),
            temp2: Vec::new(),
            last_reset: f64::NEG_INFINITY,
            identity: Identity::default(),
            acks: [false; 13],
            switched: [false; 13],
            wanted: [false; 19],
            old: [false; 19],
            circuits: [None; 19],
            oc_flagged: [false; 13],
            faults: [false; 13],
            faults_reported: [false; 13],
            dehumidifier_reversed: false,
            ptac: false,
            power_steal_mode: 0,
            tp_max: (TP_MAX, TP_MAX),
            tp_min: TP_MIN,
            tp_wake: true,
            near_pir: None,
            profile: Profile::GEN3,
            flushing: false,
            flushed: now,
            buffered: 0,
            celsius: 20.0,
            humidity: 0.0,
            visible: 0,
            temp_seen: false,
            awake_since: now,
            safety_due: now + SAFETY_DATA,
            quiet: true,
            heard: now,
            ticked: f64::NEG_INFINITY,
            skips: 0,
            clock: now,
            sent: now,
            turned: now,
            read: false,
            retries: 0,
            mcu_mode: 0,
            resets: 0,
            up: true,
            attached: false,
            attached_once: false,
            status_reason: Reason::Attach,
            status_hold: false,
            safety: None,
            ips_at: 0.0,
            need_errors: false,
            error_reasons: 0,
            oc_retries: 0,
            oc_running: false,
            alerted: false,
            error_deferred: false,
            tp_resets: 0,
            tp_window: f64::NEG_INFINITY,
            tp_wake_disabled: false,
            sim_temperature: false,
            sim_wiring: false,
            out: Vec::new(),
            reports: Vec::new(),
            logged_climate: (f32::NAN, f32::NAN),
            logged_visible: None,
            logged_power: None,
            logged_motion: f64::NEG_INFINITY,
        };
        link.raise(Event::PowerSteal);
        if attached {
            link.detected(true, || 0.0);
        }
        link
    }

    #[cfg(test)]
    pub fn state(&self) -> State {
        self.state
    }

    /// The simulator supplies the room and/or the wiring over this
    /// backplate.
    pub fn simulating(mut self, temperature: bool, wiring: bool) -> Link {
        self.sim_temperature = temperature;
        self.sim_wiring = wiring;
        self
    }

    /// Frames to send, and breaks, in order.
    pub fn take_out(&mut self) -> Vec<Out> {
        std::mem::take(&mut self.out)
    }

    pub fn take_reports(&mut self) -> Vec<Report> {
        std::mem::take(&mut self.reports)
    }

    fn raise(&mut self, e: Event) {
        self.queue.push_back(e);
    }

    /// The backplate's own safety temperatures (stock's 0x316c84), or none
    /// for a unit that gives it none: sent at once while comm is up, and
    /// after every description. The gen 2 sends none: a change only sends
    /// the power-steal mode again (`0x1bd1b0`).
    pub fn safety(&mut self, payload: Option<[u8; 16]>) {
        self.safety = payload;
        let Some(p) = payload else { return };
        if !(self.attached && self.up) {
            return;
        }
        if !self.profile.describe_all {
            self.raise(Event::PowerSteal);
            return;
        }
        let v = |i: usize| i16::from_le_bytes([p[i], p[i + 1]]);
        debug!("backplate controller: SetSafetyTempSettings Enable: 0x{:02X} Heat: {}-{} Cir: 0x{:04X} Cool: {}-{} Cir:0x{:04X} lockout: {}",
                  p[0], v(2), v(4), u16::from_le_bytes([p[6], p[7]]), v(8), v(10), u16::from_le_bytes([p[12], p[13]]), u16::from_le_bytes([p[14], p[15]]));
        self.raise(Event::SetSafetyTemp);
    }

    /// Whether the sensors' alert may be raised, as kept.
    pub fn set_i2c_allowed(&mut self, allowed: bool) {
        self.i2c = super::sensors::I2c::new(self.clock, allowed);
    }

    /// The detect line changed, once settled (stock's 0x315bf4 and
    /// 0x30573c). Detached, the link stops and forgets the backplate;
    /// attached, it asks for the status the first time and resets after.
    pub fn detected(&mut self, attached: bool, battery: impl Fn() -> f32) {
        debug!("backplate: detect line {}, {}", if attached { "attached" } else { "detached" },
               if !attached { "forgetting the backplate" } else if self.attached_once { "resetting it" } else { "asking its status" });
        self.attached = attached;
        self.reports.push(Report::Attached(attached));
        if !attached {
            self.resets = 0;
            self.identity = Identity::default();
            self.cleanup();
            return;
        }
        self.up = true;
        self.awake_since = self.clock;
        // Put back while the sensors' alert is up: it goes until a sample
        // says otherwise.
        if self.i2c.state == super::sensors::I2cState::AlertPosted {
            self.i2c.reattached(self.clock);
            self.reports.push(Report::I2cDown(false));
        }
        if self.attached_once {
            self.reset_now(Reason::Attach, false, &battery);
        } else {
            self.attached_once = true;
            self.status_reason = Reason::Attach;
            self.raise(Event::RequestBPStatus);
        }
    }

    /// Sends a firmware image, its S-records a line each (stock's
    /// 0x312bd8 senders and 0x316494 acks), once the link is idle.
    pub fn update(&mut self, kind: u8, records: Vec<Vec<u8>>) {
        info!("backplate: sending the {} image, {} records", if kind == b'B' { "bsl" } else { "mono" }, records.len());
        self.transfer = Some(Transfer { kind, records, seq: 0 });
        self.raise(Event::BeginUpdate);
    }

    /// The update frame awaiting its ack: the begin (seq 0), a record
    /// (1..=N) or the end (N + 1).
    fn send_update(&mut self) {
        let Some(t) = &self.transfer else { return };
        let seq = t.seq;
        let n = t.records.len() as u16;
        let frame = match seq {
            0 => (kind::UPDATE_BEGIN, [&seq.to_le_bytes()[..], &[t.kind]].concat()),
            s if s <= n => (kind::UPDATE_RECORD, [&seq.to_le_bytes()[..], &t.records[usize::from(s) - 1]].concat()),
            _ => (kind::UPDATE_END, seq.to_le_bytes().to_vec()),
        };
        if seq == 0 {
            debug!("backplate controller: sending begin update request");
        } else if seq == n + 1 {
            debug!("backplate controller: last SRecord ACK received");
        } else if n >= 10 && seq % (n / 10) == 0 && self.retries == 0 {
            info!("backplate: update {}% ({seq} of {n} records)", u32::from(seq) * 100 / u32::from(n));
        }
        self.send(frame.0, &frame.1);
    }

    /// An update ack: the next frame for the one awaited, or the end's
    /// report; a bootloader then restarts the backplate (reason 8).
    fn update_ack(&mut self, p: &[u8], battery: &impl Fn() -> f32) {
        let Some(t) = &mut self.transfer else { return };
        if p.len() != 2 {
            warn!("backplate controller: got bad length for MSG_UPDATE_ACK ({})", p.len());
            return;
        }
        if u16::from_le_bytes([p[0], p[1]]) != t.seq {
            warn!("backplate controller: out of sequence update ack: {} for {}", u16::from_le_bytes([p[0], p[1]]), t.seq);
            return;
        }
        self.retries = 0;
        let n = t.records.len() as u16;
        if t.seq == n + 1 {
            let kind = t.kind;
            self.transfer = None;
            self.state = State::Idle;
            info!("backplate: {} image sent", if kind == b'B' { "bsl" } else { "mono" });
            self.reports.push(Report::UpdateSent(kind));
            if kind == b'B' {
                self.reset_now(Reason::UpdateComplete, false, battery);
            }
            return;
        }
        t.seq += 1;
        self.state = if t.seq == n + 1 { State::WaitingUpdateEndAck } else { State::WaitingUpdateSRecordAcks };
        self.send_update();
    }

    /// Sunblock's temperature compensation on or off (stock's 0x314470).
    pub fn temp_comp(&mut self, on: bool) {
        debug!("backplate controller: setting temp comp mode to {}", u8::from(on));
        self.send(kind::TEMP_COMP_MODE, &u16::from(on).to_le_bytes());
    }

    /// The unit is about to sleep: tells the backplate what should wake it
    /// (stock's Quiet), in Idle or Error, later otherwise.
    pub fn quiet(&mut self, wake: Wake) {
        self.raise(Event::Quiet(wake));
    }

    /// The unit woke: the watch on a missed temperature starts over, and
    /// stock restarts the messages (a tick) while attached.
    pub fn woke(&mut self) {
        self.awake_since = self.clock;
        if self.attached {
            self.raise(Event::Restart);
        }
        if backplate_version(&self.identity.model) >= 4 && self.clock - self.ips_at > IPS_EVERY {
            debug!("backplate controller: been {} secs since last IPS status - sending request", (self.clock - self.ips_at) as u64);
            self.request_ips();
        }
    }

    /// Asks for the IPS status (0x3169bc) when nothing powers the
    /// thermostat but the HVAC circuits: no C wire, and no heating,
    /// cooling or humidity switch on.
    fn request_ips(&mut self) {
        self.ips_at = self.clock;
        let demand = Switch::ALL.iter().any(|&s| {
            self.old[s as usize] && !matches!(s, Switch::Fan | Switch::FanCool | Switch::Stage2Fan | Switch::Stage3Fan | Switch::Ob | Switch::WaterHeater)
        });
        if self.pins[2] != 1 && self.wires[8] & 1 == 0 && !demand {
            debug!("backplate controller: posting IPS Status Request.");
            self.raise(Event::RequestIPSStatus);
        }
    }

    /// The HVAC control's switches on now, and each switch's circuit
    /// (stock's HandleSwitching, 0x319580): what changed goes out a switch
    /// at a time, those going off first in stock's order, O/B among them
    /// whichever way it goes, then those coming on.
    pub fn switches(&mut self, on: &[Switch], circuits: [Option<u8>; 19], dehumidifier_reversed: bool) {
        self.circuits = circuits;
        self.dehumidifier_reversed = dehumidifier_reversed;
        self.wanted = Switch::ALL.map(|s| on.contains(&s));
        self.handle_switching();
    }

    /// The thermopile's motion thresholds from the configuration.
    pub fn thermopile(mut self, max: (i16, i16), min: (i16, i16), present: bool) -> Link {
        self.tp_max = max;
        self.tp_min = min;
        self.tp_wake = present;
        self
    }

    /// The unit's controller, and its near PIR's threshold when it has one.
    pub fn profile(mut self, profile: Profile, near_pir: Option<u16>) -> Link {
        self.profile = profile;
        self.near_pir = near_pir;
        self
    }

    /// Whether the thermopile may wake the unit (stock's TP wake: a
    /// thermopile and `wake_on_proximity`).
    pub fn thermopile_wake(&mut self, on: bool) {
        self.tp_wake = on;
    }

    /// The power-steal mode changed: stock sends its mask again.
    pub fn power_steal(&mut self, mode: u8) {
        self.power_steal_mode = mode;
        self.raise(Event::PowerSteal);
    }

    /// The circuits power stealing must leave alone (0x315f44): all of
    /// them when disabled, else the wired heat or cool stages' while
    /// heating or cooling.
    fn power_steal_mask(&self) -> u32 {
        crate::hvac::power_steal_mask(self.power_steal_mode, &self.circuits)
    }

    fn handle_switching(&mut self) {
        for s in OFF_ORDER {
            let (old, new) = (self.old[s as usize], self.wanted[s as usize]);
            if s == Switch::Ob {
                if old != new && self.circuits[s as usize].is_some() {
                    self.raise(Event::Switch { switch: s, on: new });
                }
            } else if s == Switch::Dehumidifier && self.dehumidifier_reversed {
                // Inverted, it is compared with its circuit (0x30c274), so
                // it goes out at idle too.
                let circuit = self.circuits[s as usize].filter(|&c| c < 13);
                if circuit.is_some_and(|c| self.switched[usize::from(c)] == new) {
                    self.raise(Event::Switch { switch: s, on: new });
                }
            } else if old && !new {
                self.raise(Event::Switch { switch: s, on: false });
            }
        }
        for s in ON_ORDER {
            if s == Switch::Dehumidifier && self.dehumidifier_reversed {
                continue;
            }
            if !self.old[s as usize] && self.wanted[s as usize] {
                self.raise(Event::Switch { switch: s, on: true });
            }
        }
        self.old = self.wanted;
        // Over-current flags clear once their switch is off (0x316244).
        let mut cleared = false;
        for c in 0..13u8 {
            let on = Switch::ALL.iter().any(|&s| self.wanted[s as usize] && self.circuits[s as usize] == Some(c));
            if self.oc_flagged[usize::from(c)] && !on {
                self.oc_flagged[usize::from(c)] = false;
                cleared = true;
            }
        }
        if cleared && !self.oc_flagged.iter().any(|&f| f) {
            self.raise(Event::ErrorClear);
        }
    }

    /// The client is exiting: stock resets the backplate on its way out
    /// while attached.
    pub fn exit(&mut self) {
        if self.attached {
            self.raise(Event::Exit);
        }
    }

    /// Tells the backplate the user is at the unit (stock's
    /// TemperatureLock, on input).
    pub fn temperature_lock(&mut self) {
        self.raise(Event::TemperatureLock);
    }

    /// Bytes arrived: any read ends quiet mode ("exiting quiet mode, Read").
    pub fn read(&mut self, now: f64) {
        self.clock = now;
        self.read = true;
        if self.quiet {
            self.leave_quiet();
        }
    }

    /// A frame from the backplate.
    pub fn receive(&mut self, frame: &Frame, now: f64) {
        if crate::log::enabled(crate::log::Level::Trace) {
            trace!("backplate: rx {}", self::frame(frame));
        }
        self.clock = now;
        if !self.attached || !kind::known(frame.kind) {
            return;
        }
        if !self.state.resetting() {
            self.heard = now;
            self.skips = 0;
        }
        self.raise(Event::Message(frame.clone()));
    }

    /// A frame failed its CRC: stock resets the backplate when it claimed
    /// to be the wires, the pins, or a type past the ones it knows.
    pub fn corrupt(&mut self, kind: u16, battery: impl Fn() -> f32) {
        debug!("backplate: frame of type 0x{kind:02x} failed its crc");
        if self.attached && (kind == kind::WIRES || kind == kind::PINS || kind > 0x32) {
            self.reset_now(Reason::CorruptType, false, &battery);
        }
    }

    /// A turn of stock's controller loop, when it is due: the events
    /// queued, then the repeat of what goes unanswered, the tick and the
    /// watch on silence, then the events those raised. `unread` is the
    /// bytes waiting in the UART, `battery` reads the battery's voltage
    /// for a hard reset. Detached, nothing runs.
    pub fn poll(&mut self, now: f64, unread: usize, battery: impl Fn() -> f32) {
        self.clock = now;
        // The daily safety data (0x3168c0), asked while attached and up.
        if self.profile.daily_safety && now >= self.safety_due {
            self.safety_due = now + SAFETY_DATA;
            if self.attached && self.up {
                self.raise(Event::GetSafetyTempData);
            }
        }
        if !self.attached {
            return;
        }
        let due = std::mem::take(&mut self.read) || !self.queue.is_empty() || now - self.turned >= LOOP;
        if !due {
            return;
        }
        self.turned = now;
        self.pump(&battery);
        // The loop wakes from its 2 s poll a little after the 2 s are up.
        if now - self.sent >= RETRY {
            self.raise(Event::Retry);
        }
        self.check_silence(unread, &battery);
        self.pump(&battery);
        if self.faults != self.faults_reported {
            self.faults_reported = self.faults;
            self.reports.push(Report::Faults(self.faults));
        }
    }

    /// Handles every queued event, then one deferred event in Idle or
    /// Error (0x31af48).
    fn pump(&mut self, battery: &impl Fn() -> f32) {
        while let Some(e) = self.queue.pop_front() {
            self.handle(e, battery);
            self.missed_temp(battery);
        }
        if matches!(self.state, State::Idle | State::Error) {
            if let Some(e) = self.deferred.pop_front() {
                self.handle(e, battery);
                self.missed_temp(battery);
            }
        }
    }

    /// The ticks and the watch on silence (0x318424).
    fn check_silence(&mut self, unread: usize, battery: &impl Fn() -> f32) {
        let now = self.clock;
        if self.quiet || now - self.heard <= SILENCE {
            if now - self.ticked > TICK && (self.state.resetting() || !self.quiet) {
                self.ticked = now;
                if !self.state.resetting() {
                    self.raise(Event::Tick);
                }
            }
            return;
        }
        if unread > 0 && self.skips < SKIPS {
            debug!("backplate: silent for {:.1} s with {unread} bytes unread: waiting", now - self.heard);
            self.skips += 1;
            self.heard = now;
            return;
        }
        debug!("backplate: silent for {:.1} s", now - self.heard);
        self.reset_now(Reason::TimeoutComms, false, battery);
    }

    /// The watch on a missed temperature (0x317ccc): awake and attached,
    /// a reset when none came within 35 s.
    fn missed_temp(&mut self, battery: &impl Fn() -> f32) {
        // A bootloader taking an image sends no temperatures.
        if self.state.updating() {
            self.awake_since = self.clock;
        } else if !self.temp_seen && !self.quiet && self.clock - self.awake_since > MISSED_TEMP {
            debug!("backplate: no temperature within {MISSED_TEMP} s of waking");
            self.awake_since = self.clock;
            self.reset_now(Reason::MissedTemp, false, battery);
        }
    }

    /// A reset with the reset line, whatever the battery.
    fn reset_hard(&mut self, reason: Reason) {
        self.reset_now(reason, true, &|| 0.0);
    }

    /// The gen 2's reset for a corrupted temperature or a power mismatch
    /// (`0x1ca858`): the reset line at once, not counted and with no
    /// effect on whether the link is up.
    fn reset_uncounted(&mut self) {
        debug!("backplate: reset with the reset line, uncounted");
        self.raise(Event::Reset { hard: true });
        self.heard = self.clock;
    }

    /// Asks for a reset (0x317b94): over the UART for the first three,
    /// then with the reset line while the battery allows or when `force`;
    /// the seventh time the link is down and resets stop until the
    /// backplate identifies itself. Every call restarts the watch on
    /// silence.
    fn reset_now(&mut self, reason: Reason, force: bool, battery: &impl Fn() -> f32) {
        self.last_reset = self.clock;
        if self.up {
            if self.resets == RESETS {
                debug!("backplate: {RESETS} resets brought nothing back ({reason:?}): the link is down until the backplate answers");
                self.up = false;
                self.reports.push(Report::Down);
            } else {
                let hard = force || (self.resets >= SOFT_RESETS && battery() >= HARD_RESET_VOLTS);
                debug!("backplate: reset {} of {RESETS} ({reason:?}) {}, in {:?}", self.resets + 1, if hard { "with the reset line" } else { "over the uart" }, self.state);
                self.reports.push(Report::Reset { reason, hard });
                self.raise(Event::Reset { hard });
            }
            self.resets = self.resets.saturating_add(1);
        } else {
            debug!("backplate: link down, reset skipped ({reason:?})");
        }
        self.heard = self.clock;
    }

    /// Leaving quiet mode (0x3111ac) restarts the watches on a missed
    /// temperature and, outside the wire and pin waits, on silence.
    fn leave_quiet(&mut self) {
        self.quiet = false;
        self.awake_since = self.clock;
        if !self.state.resetting() {
            self.heard = self.clock;
        }
    }

    fn send(&mut self, kind: u16, payload: &[u8]) {
        if self.quiet {
            self.leave_quiet();
        }
        self.sent = self.clock;
        self.out.push(Out::Send(Frame::new(kind, payload)));
    }

    /// What a reset, a detach or a backplate's own reset abandons
    /// (0x31531c): the acks and the flush awaited, or the error query.
    fn cleanup(&mut self) {
        self.old = [false; 19];
        match self.state {
            State::WaitingAcks => {
                self.acks = [false; 13];
                self.flushing = false;
            }
            State::Error | State::WaitingErrorAcks => self.clear_errors(),
            // The next description starts the update again.
            s if s.updating() => self.transfer = None,
            _ => {}
        }
    }

    /// One event: a transition's action then its state, or else stock's
    /// stateless handling, which keeps for later or drops what it has no
    /// use for (0x31ad9c).
    fn handle(&mut self, e: Event, battery: &impl Fn() -> f32) {
        use State::*;
        let from = self.state;
        let message = match &e {
            Event::Message(f) => Some(f.kind),
            _ => None,
        };
        if message == Some(kind::UPDATE_ACK) && from.updating() {
            if let Event::Message(f) = e {
                self.update_ack(&f.payload, battery);
            }
            return;
        }
        let to = match (&e, message, from) {
            (Event::Reset { .. }, ..) => Some(WaitingWireReport),
            (_, Some(kind::WIRES), _) => Some(WaitingPinReport),
            (Event::Quiet(_), _, Idle | Error) => Some(from),
            (Event::Restart, _, Idle | Error) => Some(Idle),
            (Event::Switch { .. } | Event::Flush | Event::ClearSwitches, _, Idle | WaitingAcks) => Some(WaitingAcks),
            (Event::Switch { .. } | Event::Flush | Event::ClearSwitches, _, Error) => Some(WaitingErrorAcks),
            (Event::Exit, _, Idle | Error) => Some(Exiting),
            (Event::ReceiveError, _, Idle | Error) => Some(Error),
            (Event::GetError, _, Error) => Some(WaitingError),
            (Event::ErrorClear, _, Error) => Some(Idle),
            (Event::RequestBPStatus, _, Idle) => Some(WaitingBPStatus),
            (Event::GetSafetyTempData, _, Idle) => Some(WaitingSafetyTempData),
            (Event::SetSafetyTemp, _, Idle) => Some(WaitingSafetyTempSetAck),
            (Event::SetMCUMode, _, Idle) => Some(WaitingMCUModeSetAck),
            (Event::RequestIPSStatus, _, Idle) => Some(WaitingIPSStatus),
            (Event::BeginUpdate, _, Idle) => Some(WaitingUpdateBeginAck),
            (Event::AllAcksIn, _, WaitingAcks) => Some(Idle),
            (Event::AllAcksIn, _, WaitingErrorAcks) => Some(Error),
            (_, Some(kind::PINS), WaitingPinReport) => Some(WaitingFwId),
            (_, Some(kind::FW_ID), WaitingFwId) => Some(WaitingFwVers),
            (_, Some(kind::FW_ID2), WaitingFwId) => Some(Idle),
            (_, Some(kind::FW_VERS), WaitingFwVers) => Some(WaitingFwInfo),
            (_, Some(kind::FW_INFO), WaitingFwInfo) => Some(WaitingBslId),
            (_, Some(kind::BSL_ID), WaitingBslId) => Some(WaitingBslVers),
            (_, Some(kind::BSL_VERS), WaitingBslVers) => Some(WaitingBslInfo),
            (_, Some(kind::BSL_INFO), WaitingBslInfo) => Some(WaitingSerial),
            (_, Some(kind::SERIAL), WaitingSerial) => Some(WaitingModel),
            (_, Some(kind::MODEL), WaitingModel) => Some(Idle),
            (_, Some(kind::BP_STATUS), WaitingBPStatus) => Some(Idle),
            (_, Some(kind::SAFETY_TEMP_DATA), WaitingSafetyTempData | WaitingSafetyTempSetAck) => Some(Idle),
            (_, Some(kind::MCU_MODE_ACK), WaitingMCUModeSetAck) => Some(Idle),
            (_, Some(kind::IPS_STATUS), WaitingIPSStatus) => Some(Idle),
            (_, Some(kind::ERROR_VECTOR | kind::OC_ERRORS | kind::TP_ERRORS), WaitingError) => Some(Error),
            _ => None,
        };
        let Some(to) = to else {
            return self.stateless(e, battery);
        };
        if to != from {
            // Switching and flushing come and go with every report.
            let routine = matches!((from, to), (Idle | Error, WaitingAcks | WaitingErrorAcks) | (WaitingAcks | WaitingErrorAcks, Idle | Error));
            if routine { trace!("backplate: {from:?} -> {to:?}") } else { debug!("backplate: {from:?} -> {to:?}") }
        }
        match e {
            Event::Reset { hard } => {
                self.cleanup();
                if hard {
                    debug!("backplate: pulsing the reset line");
                    self.out.push(Out::HardReset);
                } else {
                    debug!("backplate: sending a break and a reset request");
                    self.out.push(Out::Break);
                    self.send(kind::RESET, &[]);
                }
            }
            Event::Quiet(wake) => self.send_quiet(&wake),
            Event::Restart => {
                if self.quiet {
                    self.leave_quiet();
                }
                self.send(kind::TICK, &[]);
                if from == Error && self.oc_retries >= OC_RETRIES {
                    // Error recovery (0x31a348): everything switched again.
                    self.clear_errors();
                    self.old = [false; 19];
                    self.handle_switching();
                }
            }
            Event::Switch { switch, on } => self.set_switch(switch, on),
            Event::ClearSwitches if self.sim_wiring => {}
            Event::ClearSwitches => {
                // Off in stock's order, the switching state left as it is
                // (0x3128dc).
                for s in OFF_ORDER {
                    if self.old[s as usize] {
                        self.set_switch(s, false);
                    }
                }
            }
            Event::Flush => self.flush(),
            Event::Exit => self.reset_now(Reason::Exit, false, battery),
            Event::ReceiveError => {
                self.need_errors = true;
                self.raise(Event::GetError);
            }
            Event::GetError => {
                self.get_error();
            }
            Event::ErrorClear => {
                self.clear_errors();
                if std::mem::take(&mut self.alerted) {
                    self.reports.push(Report::SwitchFault(false));
                }
            }
            Event::RequestBPStatus => self.send(kind::GET_BP_STATUS, &[]),
            Event::GetSafetyTempData => self.send(kind::GET_SAFETY_TEMP_DATA, &[1, 0, 0, 0]),
            Event::SetSafetyTemp => self.send(kind::SET_SAFETY_TEMP, &self.safety.unwrap_or_default()),
            Event::SetMCUMode => self.send(kind::SET_MCU_MODE, &[self.mcu_mode, 0, 0, 0]),
            Event::RequestIPSStatus => self.send(kind::GET_IPS_STATUS, &[0, 0]),
            Event::BeginUpdate => self.send_update(),
            Event::Message(f) => self.answer(&f, from, battery),
            _ => {}
        }
        self.state = to;
    }

    /// A message's action in its transition.
    fn answer(&mut self, f: &Frame, from: State, battery: &impl Fn() -> f32) {
        let p = f.payload.as_slice();
        // Stock reads the description's text as a C string.
        let text = || String::from_utf8_lossy(p.split(|&b| b == 0).next().unwrap_or_default()).into_owned();
        match f.kind {
            kind::WIRES => {
                if from != State::WaitingWireReport {
                    // The backplate reset itself (0x31551c).
                    info!("backplate: the backplate reset itself");
                    self.identity = Identity::default();
                    self.cleanup();
                }
                if p.len() == 13 {
                    debug!("backplate: wires {}", wires(p));
                    self.skips = 0;
                    self.wires.copy_from_slice(p);
                    if !self.sim_wiring {
                        self.reports.push(Report::Wires(self.wires));
                    }
                }
            }
            kind::PINS => {
                if p.len() == 15 {
                    debug!("backplate: pins {}", hex(p));
                    self.pins.copy_from_slice(p);
                    if !self.sim_wiring {
                        self.reports.push(Report::Pins(self.pins));
                    }
                    let wires = self.wires;
                    self.send(kind::WIRES_ECHO, &wires);
                    self.send(kind::TICK, &[]);
                    self.send(kind::GET_FW_ID, &[]);
                }
            }
            kind::FW_ID | kind::FW_ID2 => {
                self.resets = 0;
                self.up = true;
                if p.len() == 3 {
                    debug!("backplate: firmware id {}", hex(p));
                    self.identity.firmware_id = p.to_vec();
                    if f.kind == kind::FW_ID {
                        self.send(kind::GET_FW_VERS, &[]);
                    } else {
                        self.reports.push(Report::Ready(self.identity.clone()));
                    }
                }
            }
            kind::FW_VERS => {
                self.identity.firmware_version = text();
                debug!("backplate: firmware version {}", self.identity.firmware_version);
                self.send(kind::GET_FW_INFO, &[]);
            }
            kind::FW_INFO => {
                self.identity.firmware_info = text();
                debug!("backplate: firmware info {}", self.identity.firmware_info);
                self.send(kind::GET_BSL_ID, &[]);
            }
            kind::BSL_ID => {
                if p.len() == 2 {
                    self.identity.bsl_id = u16::from_le_bytes([p[0], p[1]]);
                    debug!("backplate: bsl id 0x{:04x}", self.identity.bsl_id);
                    self.send(kind::GET_BSL_VERS, &[]);
                }
            }
            kind::BSL_VERS => {
                self.identity.bsl_version = text();
                debug!("backplate: bsl version {}", self.identity.bsl_version);
                self.send(kind::GET_BSL_INFO, &[]);
            }
            kind::BSL_INFO => {
                self.identity.bsl_info = text();
                debug!("backplate: bsl info {}", self.identity.bsl_info);
                self.send(kind::GET_SERIAL, &[]);
            }
            kind::SERIAL => {
                self.identity.serial = text();
                debug!("backplate: serial {}", self.identity.serial);
                self.send(kind::GET_MODEL, &[]);
            }
            kind::MODEL => {
                self.identity.model = text();
                debug!("backplate: model {}", self.identity.model);
                self.reports.push(Report::Ready(self.identity.clone()));
                // What stock's controller owner asks once described.
                if self.profile.describe_all && self.mcu_mode != MCU_MODE {
                    self.mcu_mode = MCU_MODE;
                    self.raise(Event::SetMCUMode);
                }
                if self.profile.describe_all && self.safety.is_some() {
                    self.raise(Event::SetSafetyTemp);
                }
                self.raise(Event::PowerSteal);
                if backplate_version(&self.identity.model) >= 4 {
                    self.request_ips();
                }
            }
            // Only logged (0x30f0c4): its type and value.
            kind::IPS_STATUS if p.len() == 4 => {
                debug!("backplate controller: got IPS Status (Type: {} Val: {})", u16::from_le_bytes([p[0], p[1]]), u16::from_le_bytes([p[2], p[3]]));
            }
            kind::BP_STATUS => {
                if p.len() == 2 {
                    let status = u16::from_le_bytes([p[0], p[1]]);
                    debug!("backplate: bp status 0x{status:04x}{}", if status & 8 != 0 { ", safety temperature data first" } else { "" });
                    if status != 0 {
                        if status & 8 != 0 {
                            self.raise(Event::GetSafetyTempData);
                            self.status_hold = true;
                        }
                        self.send(kind::BP_STATUS_ACK, &[]);
                    }
                }
                if !self.status_hold {
                    self.reset_now(self.status_reason, false, battery);
                }
            }
            // Safety data (0x31a1c4): a held status reset goes first;
            // answering new safety temperatures, every switch on goes again.
            kind::SAFETY_TEMP_DATA if p.len() == 0x18 => {
                debug!("backplate: safety temperature data {}", hex(p));
                if std::mem::take(&mut self.status_hold) {
                    self.reset_now(self.status_reason, false, battery);
                } else if from == State::WaitingSafetyTempSetAck {
                    self.old = [false; 19];
                    self.handle_switching();
                }
            }
            kind::ERROR_VECTOR => {
                if p.len() == 2 {
                    debug!("backplate: error vector 0x{:04x}", u16::from_le_bytes([p[0], p[1]]));
                    self.need_errors = false;
                    self.error_reasons |= u16::from_le_bytes([p[0], p[1]]);
                    self.raise(if self.error_reasons != 0 { Event::GetError } else { Event::ErrorClear });
                }
            }
            kind::OC_ERRORS => self.oc_errors(p),
            kind::TP_ERRORS => self.tp_errors(p, battery),
            _ => {}
        }
    }

    /// Events no transition takes (0x318cac).
    fn stateless(&mut self, e: Event, battery: &impl Fn() -> f32) {
        match e {
            Event::Message(f) => self.report(&f, battery),
            Event::Tick => self.send(kind::TICK, &[]),
            Event::TemperatureLock => self.send(kind::TEMPERATURE_LOCK, &[]),
            Event::PowerSteal => {
                let mask = self.power_steal_mask();
                debug!("backplate: power steal mode {}, mask 0x{mask:08x}", self.power_steal_mode);
                self.send(kind::POWER_STEAL, &mask.to_le_bytes());
            }
            Event::Retry => self.retry(battery),
            Event::ReceiveError if !self.error_deferred => {
                self.error_deferred = true;
                self.defer(e);
            }
            e if e.deferrable() => self.defer(e),
            _ => {}
        }
    }

    fn defer(&mut self, e: Event) {
        if self.deferred.len() < DEFERRED {
            trace!("backplate: {:?} deferred in {:?}", std::mem::discriminant(&e), self.state);
            self.deferred.push_back(e);
        } else {
            debug!("backplate: deferred events full, one dropped in {:?}", self.state);
        }
    }

    /// Someone moved, from `what`, close to the unit when `near`: logged
    /// once a minute at most.
    fn motion(&mut self, what: &str, near: bool) {
        if self.clock - self.logged_motion >= 60.0 {
            self.logged_motion = self.clock;
            debug!("backplate: motion ({what})");
        }
        self.reports.push(Report::Motion { near });
    }

    /// The live reports, handled in any state.
    fn report(&mut self, f: &Frame, battery: &impl Fn() -> f32) {
        let p = f.payload.as_slice();
        match f.kind {
            kind::TEMP2 if !p.is_empty() && p.len().is_multiple_of(6) => {
                self.buffered = self.buffered.wrapping_add(1);
                let v = |c: &[u8], i: usize| u16::from_le_bytes([c[2 * i], c[2 * i + 1]]);
                let samples: Vec<(u16, u16)> = p.chunks(6).map(|c| (v(c, 0), v(c, 1))).collect();
                let c = |x: u16| f32::from(x as i16) / 100.0;
                if crate::log::enabled(crate::log::Level::Trace) {
                    let all: Vec<String> = p.chunks(6).map(|ch| format!("{:.2}/{:.2}/{:.2}", c(v(ch, 0)), c(v(ch, 1)), c(v(ch, 2)))).collect();
                    trace!("backplate: temp2 hu/grille/bp {}", all.join(" "));
                }
                self.temp2.extend(p.chunks(6).map(|ch| super::sensors::Sample { hu: c(v(ch, 0)), grille: c(v(ch, 1)), bp: c(v(ch, 2)) }));
                for action in self.i2c.samples(&samples, self.clock) {
                    match action {
                        super::sensors::I2cAction::Reset => self.reset_now(Reason::I2cDown, false, battery),
                        super::sensors::I2cAction::Up(up) => self.reports.push(Report::I2cDown(!up)),
                        super::sensors::I2cAction::Allowed(a) => self.reports.push(Report::I2cAllowed(a)),
                    }
                }
            }
            kind::TEMP if p.len() == 4 => self.temperature(i16::from_le_bytes([p[0], p[1]]), u16::from_le_bytes([p[2], p[3]])),
            kind::PIR if p.len() == 4 => {
                if p.iter().any(|&b| b != 0) {
                    self.motion("pir", false);
                }
            }
            // The near PIR (the gen 2's `0x1c3b18`): at or past its
            // threshold, someone approaching.
            kind::NEAR_PIR if p.len() == 2 => {
                if self.near_pir.is_some_and(|t| u16::from_le_bytes([p[0], p[1]]) >= t) {
                    self.motion("near pir", true);
                }
            }
            kind::ALS if p.len() == 4 => {
                let visible = u16::from_le_bytes([p[0], p[1]]);
                if visible != 0xffff {
                    self.visible = visible;
                }
                // A change of a tenth, or of 20 counts in the dark.
                if self.logged_visible.is_none_or(|l| l.abs_diff(self.visible) > (l / 10).max(20)) {
                    self.logged_visible = Some(self.visible);
                    debug!("backplate: light {}{}", self.visible, if visible == 0xffff { " (unread)" } else { "" });
                }
                self.reports.push(Report::Light { visible: self.visible });
            }
            kind::ACK_SWITCH if p.len() == 2 => {
                let waiting = self.acks.iter().any(|&a| a);
                let circuit = usize::from(p[0]);
                if circuit < 13 && self.acks[circuit] && self.switched[circuit] == (p[1] != 0) {
                    debug!("backplate: circuit {circuit} {} acknowledged", if p[1] != 0 { "on" } else { "off" });
                    self.acks[circuit] = false;
                    self.reports.push(Report::Switched { circuit: p[0], on: p[1] != 0 });
                }
                if waiting && !self.acks.iter().any(|&a| a) && !self.flushing {
                    self.raise(Event::AllAcksIn);
                }
            }
            kind::END_BUF => {
                let complete = match p.len() {
                    // A compatible backplate's end, without a count.
                    0 => true,
                    2 if u16::from_le_bytes([p[0], p[1]]) == self.buffered => {
                        self.send(kind::END_BUF_ACK, &[]);
                        true
                    }
                    _ => false,
                };
                if complete && std::mem::take(&mut self.flushing) && !self.acks.iter().any(|&a| a) {
                    self.raise(Event::AllAcksIn);
                }
                // The buffered TEMP2 samples, checked as stock publishes them.
                let checks = self.profile.sensor_checks;
                for sample in std::mem::take(&mut self.temp2).into_iter().filter(|_| checks) {
                    if let Some(issue) = self.sanity.sample(sample, self.clock, self.last_reset) {
                        self.reset_hard(if issue { Reason::TempSensorIssue } else { Reason::TempSensorIssueDelayed });
                    }
                }
            }
            // Thermopile (0x30f994): left, right and up/down maxima and
            // minima; a maximum past its threshold is someone moving.
            kind::TP if p.len() == 12 => {
                let v = |i: usize| i16::from_le_bytes([p[2 * i], p[2 * i + 1]]);
                if v(0) > self.tp_max.0 || v(2) > self.tp_max.0 || v(4) > self.tp_max.1 {
                    self.motion("thermopile", true);
                }
            }
            kind::PWR if p.len() == 16 => {
                let (first, second) = (u16::from_le_bytes([p[6], p[7]]), u16::from_le_bytes([p[8], p[9]]));
                if p[0] < 10 {
                    self.power_state = p[0];
                } else {
                    warn!("backplate controller: Invalid power state: {}", p[0]);
                }
                if self.logged_power != Some(self.power_state) {
                    self.logged_power = Some(self.power_state);
                    debug!("backplate: power state {}, {} mV, {} mV, about {} mA", self.power_state, if first == 0 { second } else { first }, second, current(p[2]));
                }
                self.reports.push(Report::Power { vusb: if first == 0 { second } else { first }, vin: second, current: current(p[2]), state: self.power_state });
                // Pins on R the wires do not show, with the backplate
                // powered: a forced reset, five in a row at most.
                let pins_r = self.pins[3] == 1 || self.pins[4] == 1;
                let wires_r = self.wires[9] & 1 != 0 || self.wires[10] & 1 != 0;
                if pins_r && !wires_r && p[0] != 0 {
                    if self.mismatches < MISMATCH_RESETS {
                        warn!("backplate controller: HU and BP power state mismatch hvac_power: 0 pwr_state: {}, resetting the BP", p[0]);
                        self.mismatches += 1;
                        if self.profile.sensor_checks {
                            self.reset_hard(Reason::BpPowerMismatch);
                        } else {
                            self.reset_uncounted();
                        }
                    } else if self.mismatches == MISMATCH_RESETS {
                        // Said once: the count stays past the limit until the mismatch ends.
                        error!("backplate controller: HU and BP power state mismatch hvac_power: 0 pwr_state: {}, but max retry exceeded", p[0]);
                        self.mismatches += 1;
                    }
                } else {
                    self.mismatches = 0;
                }
            }
            kind::WAKEUP if p.len() == 1 => {
                debug!("backplate: wakeup {}", match p[0] { 1 => "to flush", 10 => "for an error", _ => "of another kind" });
                match p[0] {
                    1 => self.raise(Event::Flush),
                    10 => self.raise(Event::ReceiveError),
                    _ => {}
                }
            }
            kind::WAKEUP_VEC if p.len() == 2 => {
                let v = u16::from_le_bytes([p[0], p[1]]);
                debug!("backplate: woken by 0x{v:04x}");
                // The thermopile or the near PIR on the bit they share,
                // someone close; else the PIR (0x317264, the gen 2's 0x1ca0c8).
                if v & 0x80 != 0 && (self.tp_wake || self.near_pir.is_some()) {
                    self.motion("wake", true);
                } else if v & 0x08 != 0 {
                    self.motion("wake", false);
                }
                if v & 0x100 != 0 {
                    self.reports.push(Report::Vergence);
                }
                if v & 0x400 != 0 {
                    self.raise(Event::ReceiveError);
                }
            }
            kind::ALS_BUF => {
                debug!("backplate: light buffer, {} samples", p.len() / 6);
                self.buffered = self.buffered.wrapping_add(1);
                self.reports.push(Report::LightBuffer(p.as_chunks::<6>().0.iter().map(|r| u16::from_le_bytes([r[4], r[5]])).collect()));
            }
            k if kind::counted(k) => self.buffered = self.buffered.wrapping_add(1),
            _ => {}
        }
    }

    /// A temperature report (0x318724): stock keeps the last valid values,
    /// resets the backplate with the reset line on its error codes, and
    /// flushes the buffers either way.
    fn temperature(&mut self, raw: i16, rh: u16) {
        if self.sim_temperature {
            self.raise(Event::Flush);
            self.temp_seen = true;
            self.reports.push(Report::Climate { celsius: self.celsius, humidity: None });
            return;
        }
        if rh != 0xffff {
            self.humidity = f32::from(rh.min(1000)) / 10.0;
        }
        let checks = self.profile.sensor_checks;
        let error = match raw as u16 {
            0x8000 => Some(Reason::CorruptedTempMsg),
            _ if rh == 0xffff => Some(Reason::CorruptedTempMsg),
            0x8001 if checks => Some(Reason::GrilleTempErrorMsg),
            0x8002 if checks => Some(Reason::HuTempErrorMsg),
            _ => None,
        };
        if raw != -0x8000 && !(checks && (-0x7fff..=-0x7ffe).contains(&raw)) {
            self.celsius = f32::from(raw) / 100.0;
        }
        trace!("backplate: temperature raw 0x{:04x} humidity raw 0x{rh:04x}", raw as u16);
        if let Some(reason) = error {
            warn!("backplate: temperature report 0x{:04x} humidity 0x{rh:04x} is an error ({reason:?}): resetting", raw as u16);
        }
        // A change of 0.1 °C or 1 % since the log last said.
        let (c, h) = self.logged_climate;
        if !((self.celsius - c).abs() < 0.1 && (self.humidity - h).abs() < 1.0) {
            self.logged_climate = (self.celsius, self.humidity);
            debug!("backplate: temperature {:.2} °C, humidity {:.1} %", self.celsius, self.humidity);
        }
        match error {
            Some(reason) if checks => self.reset_hard(reason),
            Some(_) => self.reset_uncounted(),
            None => {}
        }
        self.raise(Event::Flush);
        self.temp_seen = true;
        self.reports.push(Report::Climate { celsius: self.celsius, humidity: (self.humidity != 0.0).then_some(self.humidity) });
    }

    fn flush(&mut self) {
        self.flushing = true;
        self.send(kind::FLUSH, &[]);
        self.buffered = 0;
        self.flushed = self.clock;
    }

    /// A PTAC or fan coil, as the setup says (Y2 as the fan's second speed).
    pub fn set_ptac(&mut self, ptac: bool) {
        self.ptac = ptac;
    }

    /// A switch's request (0x31245c): to its circuit, except stage-3 cool,
    /// fan cool and the AC dehumidifier, which stock never sends, and on a
    /// PTAC or fan coil stage-2 cool and stage-2 and 3 heat (0x311934).
    fn set_switch(&mut self, switch: Switch, on: bool) {
        if matches!(switch, Switch::Stage3Cool | Switch::FanCool | Switch::AcDehumidifier) {
            return;
        }
        if self.ptac && matches!(switch, Switch::Stage2Cool | Switch::Stage2Heat | Switch::Stage3Heat) {
            debug!("backplate controller: Ignoring request from PTAC/fan coil system for {}", switch.name());
            return;
        }
        let Some(circuit) = self.circuits[switch as usize].filter(|&c| c < 13) else {
            warn!("backplate: no circuit for {}", switch.name());
            return;
        };
        // A reversed dehumidifier's circuit is the opposite (0x312154).
        let on = if switch == Switch::Dehumidifier && self.dehumidifier_reversed { !on } else { on };
        self.switched[usize::from(circuit)] = on;
        debug!("backplate: circuit {circuit} ({}) {}{}", switch.name(), if on { "on" } else { "off" }, if self.sim_wiring { " (simulated wiring: not sent)" } else { "" });
        if !self.sim_wiring {
            self.send_switch(circuit);
        }
    }

    /// Asks for a circuit's state as last chosen, and awaits its ack.
    fn send_switch(&mut self, circuit: u8) {
        let on = self.switched[usize::from(circuit)];
        self.acks[usize::from(circuit)] = true;
        self.send(kind::SWITCH, &[circuit, u8::from(on)]);
    }

    /// The sleep settings, then quiet (0x314860), as stock sends them with
    /// its default sensor settings.
    fn send_quiet(&mut self, wake: &Wake) {
        debug!("backplate: quiet for up to {} s, waking on temperature {}, humidity {} to {}, light {} to {}, vergence {:?}", wake.max_sleep,
               wake.steps.iter().map(|(s, l, h)| format!("{} to {} ({s} s)", side(*l), side(*h))).collect::<Vec<_>>().join(", "),
               side(wake.humidity.0), side(wake.humidity.1), wake.als.0, wake.als.1, wake.vergence);
        let mut steps = Vec::new();
        for (seconds, low, high) in &wake.steps {
            steps.extend_from_slice(&seconds.to_le_bytes());
            steps.extend_from_slice(&centi(*low));
            steps.extend_from_slice(&centi(*high));
        }
        self.send(kind::TEMP_WAKE, &steps);
        if self.profile.pir_wake {
            self.send(kind::PIR_SENSITIVITY, &[0x64, 0x00]);
        }
        if let Some(threshold) = self.near_pir {
            self.send(kind::NEAR_PIR_THRESHOLD, &threshold.to_le_bytes());
        }
        // The thermopile's thresholds, only while it may wake the unit.
        let tp_wake = self.tp_wake && !self.tp_wake_disabled;
        if self.tp_wake {
            let t: Vec<u8> = [self.tp_max.0, self.tp_max.1, self.tp_min.0, self.tp_min.1].iter().flat_map(|v| v.to_le_bytes()).collect();
            self.send(kind::TP_THRESHOLDS, &t);
        }
        let (low, high) = (per_mille(wake.humidity.0), per_mille(wake.humidity.1));
        self.send(kind::HUMIDITY_WAKE, &[low[0], low[1], high[0], high[1]]);
        let (low, high) = wake.als;
        self.send(kind::ALS_WAKE, &[low.to_le_bytes(), high.to_le_bytes()].concat());
        let (mode, t1, t2) = wake.vergence;
        self.send(kind::VERGENCE_WAKE, &[mode.to_le_bytes(), t1.to_le_bytes(), t2.to_le_bytes()].concat());
        // Timer, buffers, temperature, humidity, ALS and the rest, PIR,
        // VBAT, HE; the thermopile's bit while it may wake (0x314714).
        let sources: u16 = self.profile.wake_sources | if tp_wake || self.near_pir.is_some() { 0x80 } else { 0 };
        self.send(kind::WAKE_SOURCES, &sources.to_le_bytes());
        self.send(kind::QUIET, &wake.max_sleep.max(1).to_le_bytes());
        self.temp_seen = false;
        self.quiet = true;
    }

    /// Asks for the errors (0x314a88): the vector first, then the
    /// over-current and thermopile details it names. False when there is
    /// nothing to ask.
    fn get_error(&mut self) -> bool {
        if self.need_errors {
            self.error_deferred = false;
            self.send(kind::GET_ERROR, &[]);
            return true;
        }
        if self.error_reasons == 0 {
            return false;
        }
        let mut sent = false;
        if self.error_reasons & 1 != 0 {
            self.send(kind::GET_OC_ERRORS, &[]);
            sent = true;
        }
        if self.error_reasons & 2 != 0 {
            self.send(kind::GET_TP_ERRORS, &[]);
            sent = true;
        }
        // Stock handles no other error.
        self.error_reasons &= 3;
        sent
    }

    /// Over-current errors (0x31a458): the faulted circuits that should be
    /// on are asked on again, ten times at most; then the switches are
    /// cleared and the alert raised.
    fn oc_errors(&mut self, p: &[u8]) {
        if p.len() != 13 || self.error_reasons & 1 == 0 {
            return;
        }
        self.oc_running = true;
        self.error_reasons &= !1;
        let faulted: Vec<String> = (0..13).filter(|&c| p[c] != 0).map(|c| c.to_string()).collect();
        if self.oc_retries < OC_RETRIES {
            warn!("backplate: over-current on circuits {}: asking them again ({} of {OC_RETRIES})", faulted.join(" "), self.oc_retries + 1);
        } else {
            error!("backplate: over-current on circuits {} after {OC_RETRIES} retries: clearing the switches", faulted.join(" "));
        }
        if self.oc_retries < OC_RETRIES {
            let mut any = false;
            for c in 0..13u8 {
                if p[usize::from(c)] != 0 && self.switched[usize::from(c)] {
                    self.oc_flagged[usize::from(c)] = true;
                    self.send_switch(c);
                    any = true;
                }
            }
            if any {
                self.oc_retries += 1;
            } else {
                self.raise(Event::ErrorClear);
            }
        } else {
            self.oc_running = false;
            self.faults = std::array::from_fn(|c| p[c] != 0);
            self.raise(Event::ClearSwitches);
            if !std::mem::replace(&mut self.alerted, true) {
                self.reports.push(Report::SwitchFault(true));
            }
        }
    }

    /// Thermopile errors (0x3181c0): inaccessible turns thermopile wake
    /// off; the others reset the backplate, five times a week at most.
    fn tp_errors(&mut self, p: &[u8], battery: &impl Fn() -> f32) {
        if p.len() != 1 || self.error_reasons & 2 == 0 {
            return;
        }
        self.error_reasons &= !2;
        debug!("backplate: thermopile errors 0x{:02x}", p[0]);
        if p[0] & 1 != 0 {
            warn!("backplate: thermopile inaccessible: thermopile wake off");
            self.tp_wake_disabled = true;
        } else if p[0] & 6 != 0 && !self.tp_wake_disabled {
            if self.clock - self.tp_window > TP_WINDOW {
                self.tp_resets = 0;
            }
            if self.tp_resets < TP_RESETS {
                self.reset_now(Reason::TpError, false, battery);
                self.tp_window = self.clock;
                self.tp_resets += 1;
            } else {
                warn!("backplate: {TP_RESETS} thermopile resets this week: thermopile wake off");
                self.tp_wake_disabled = true;
            }
        }
        if self.error_reasons == 0 {
            self.raise(Event::ErrorClear);
        }
    }

    /// Forgets the error query (0x315274).
    fn clear_errors(&mut self) {
        self.oc_flagged = [false; 13];
        self.faults = [false; 13];
        self.need_errors = false;
        self.oc_retries = 0;
        self.oc_running = false;
        self.error_reasons = 0;
    }

    /// Repeats what the link waits for (0x317e48); six in a row reset the
    /// backplate.
    fn retry(&mut self, battery: &impl Fn() -> f32) {
        use State::*;
        match self.state {
            Idle | WaitingWireReport | WaitingPinReport | Error => {
                self.retries = 0;
                self.sent = self.clock;
                return;
            }
            WaitingAcks | WaitingErrorAcks => {
                if !self.retry_acks() {
                    self.sent = self.clock;
                    return;
                }
            }
            WaitingError => {
                if !self.get_error() {
                    self.sent = self.clock;
                    return;
                }
            }
            WaitingBPStatus => self.reset_now(self.status_reason, false, battery),
            WaitingSafetyTempData => self.send(kind::GET_SAFETY_TEMP_DATA, &[1, 0, 0, 0]),
            WaitingSafetyTempSetAck => self.send(kind::SET_SAFETY_TEMP, &self.safety.unwrap_or_default()),
            WaitingMCUModeSetAck => self.send(kind::SET_MCU_MODE, &[self.mcu_mode, 0, 0, 0]),
            WaitingUpdateBeginAck | WaitingUpdateSRecordAcks | WaitingUpdateEndAck => self.send_update(),
            // Never asked again; the third retry gives up.
            WaitingIPSStatus => {
                self.retries += 1;
                self.sent = self.clock;
                if self.retries == IPS_RETRIES {
                    warn!("backplate controller: Timed out for waiting for IPS Status response. Keep going.");
                    self.retries = 0;
                    self.state = Idle;
                }
                return;
            }
            Exiting => self.reset_now(Reason::Exit, false, battery),
            s => {
                debug_assert!(s.describing());
                let again = match s {
                    WaitingFwId => kind::GET_FW_ID,
                    WaitingFwVers => kind::GET_FW_VERS,
                    WaitingFwInfo => kind::GET_FW_INFO,
                    WaitingBslId => kind::GET_BSL_ID,
                    WaitingBslVers => kind::GET_BSL_VERS,
                    WaitingBslInfo => kind::GET_BSL_INFO,
                    WaitingSerial => kind::GET_SERIAL,
                    _ => kind::GET_MODEL,
                };
                self.send(again, &[]);
            }
        }
        self.retries += 1;
        debug!("backplate: no answer in {:?}: retry {} of {RETRIES}", self.state, self.retries);
        if self.retries == RETRIES {
            self.retries = 0;
            self.reset_now(Reason::Retries, false, battery);
        }
    }

    /// Asks again for the switches awaiting an ack, and flushes again
    /// when the end of buffers is overdue (0x31311c).
    fn retry_acks(&mut self) -> bool {
        let mut again = false;
        for c in 0..13u8 {
            if self.acks[usize::from(c)] {
                self.send_switch(c);
                again = true;
            }
        }
        if self.flushing && self.clock - self.flushed > FLUSH_TIMEOUT {
            self.flush();
            again = true;
        }
        again
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// nest-qemu's default wiring: W1, Y1, G, Rh, plus O/B on circuit 3.
    const MAP: [Option<u8>; 19] = {
        let mut m = [None; 19];
        m[Switch::Fan as usize] = Some(2);
        m[Switch::Stage1Cool as usize] = Some(1);
        m[Switch::Stage1Heat as usize] = Some(0);
        m[Switch::Stage2Heat as usize] = Some(4);
        m[Switch::Ob as usize] = Some(3);
        m
    };

    const BREAK: u16 = 0x100;
    const PULSE: u16 = 0x101;

    fn sent(link: &mut Link) -> Vec<u16> {
        link.take_out().into_iter().map(|o| match o { Out::Break => BREAK, Out::HardReset => PULSE, Out::Send(f) => f.kind }).collect()
    }

    fn frame(kind: u16, payload: &[u8]) -> Frame {
        Frame::new(kind, payload)
    }

    /// A frame arriving at `t`, and the loop's turn that follows.
    fn rx(link: &mut Link, kind: u16, payload: &[u8], t: f64) {
        link.read(t);
        link.receive(&frame(kind, payload), t);
        link.poll(t, 0, || 3.8);
    }

    fn turn(link: &mut Link, t: f64) {
        link.poll(t, 0, || 3.8);
    }

    fn temp(c: i16, rh: u16) -> Vec<u8> {
        c.to_le_bytes().iter().chain(&rh.to_le_bytes()).copied().collect()
    }

    fn identify(link: &mut Link, t: f64) {
        rx(link, kind::WIRES, &[1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0], t);
        rx(link, kind::PINS, &[1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0], t);
        rx(link, kind::FW_ID, &[0x34, 0x12, 0], t);
        rx(link, kind::FW_VERS, b"2.3.27\0", t);
        rx(link, kind::FW_INFO, b"x", t);
        rx(link, kind::BSL_ID, &[1, 0], t);
        for k in [kind::BSL_VERS, kind::BSL_INFO, kind::SERIAL] {
            rx(link, k, b"x", t);
        }
        rx(link, kind::MODEL, b"Backplate-6.0", t);
    }

    /// The cold start stock nlclient makes, frame for frame.
    fn handshake(link: &mut Link) {
        turn(link, 0.0);
        assert_eq!(sent(link), [kind::POWER_STEAL, kind::GET_BP_STATUS, kind::TICK]);
        rx(link, kind::BP_STATUS, &[0, 0], 0.0);
        assert_eq!(sent(link), [BREAK, kind::RESET]);
        rx(link, kind::WIRES, &[1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0], 0.0);
        rx(link, kind::PINS, &[1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0], 0.0);
        assert_eq!(sent(link), [kind::WIRES_ECHO, kind::TICK, kind::GET_FW_ID]);
        rx(link, kind::FW_ID, &[0x34, 0x12, 0], 0.0);
        assert_eq!(sent(link), [kind::GET_FW_VERS]);
        for (answer, request) in [
            (kind::FW_VERS, kind::GET_FW_INFO), (kind::FW_INFO, kind::GET_BSL_ID), (kind::BSL_ID, kind::GET_BSL_VERS),
            (kind::BSL_VERS, kind::GET_BSL_INFO), (kind::BSL_INFO, kind::GET_SERIAL), (kind::SERIAL, kind::GET_MODEL),
        ] {
            rx(link, answer, b"x\x01", 0.0);
            assert_eq!(sent(link), [request]);
        }
        rx(link, kind::MODEL, b"Backplate-6.0\0junk", 0.0);
        assert_eq!(sent(link), [kind::SET_MCU_MODE, kind::POWER_STEAL]);
        assert_eq!(link.state(), State::WaitingMCUModeSetAck);
        rx(link, kind::MCU_MODE_ACK, &[], 0.0);
        assert_eq!(link.state(), State::Idle);
    }

    /// Identified, with a first temperature in.
    fn ready() -> Link {
        let mut link = Link::new(0.0, true);
        handshake(&mut link);
        rx(&mut link, kind::TEMP, &temp(2150, 450), 0.0);
        rx(&mut link, kind::END_BUF, &[], 0.0);
        sent(&mut link);
        link.take_reports();
        link
    }

    #[test]
    fn the_cold_start_is_stocks() {
        let mut link = Link::new(0.0, true);
        handshake(&mut link);
        let reports = link.take_reports();
        assert!(reports.contains(&Report::Reset { reason: Reason::Attach, hard: false }));
        assert!(matches!(reports.last(), Some(Report::Ready(id)) if id.model == "Backplate-6.0" && id.firmware_id == [0x34, 0x12, 0]));
    }

    #[test]
    fn a_temperature_flushes_the_buffers_and_their_counted_end_is_acked() {
        let mut link = ready();
        rx(&mut link, kind::TEMP, &temp(2150, 450), 1.0);
        assert_eq!(link.take_reports(), [Report::Climate { celsius: 21.5, humidity: Some(45.0) }]);
        assert_eq!(sent(&mut link), [kind::FLUSH]);
        assert_eq!(link.state(), State::WaitingAcks);
        rx(&mut link, 0x22, &[0; 4], 1.0);
        rx(&mut link, 0x24, &[0; 16], 1.0);
        rx(&mut link, kind::END_BUF, &[2, 0], 1.0);
        assert!(sent(&mut link).is_empty(), "a count that does not match is ignored");
        assert_eq!(link.state(), State::WaitingAcks);
        rx(&mut link, kind::END_BUF, &[1, 0], 1.1);
        assert_eq!(sent(&mut link), [kind::END_BUF_ACK]);
        assert_eq!(link.state(), State::Idle);
    }

    #[test]
    fn an_unanswered_flush_goes_again_after_five_seconds() {
        let mut link = ready();
        rx(&mut link, kind::TEMP, &temp(2150, 450), 1.0);
        sent(&mut link);
        let mut out = Vec::new();
        for i in 1..=8 {
            let t = 1.0 + f64::from(i);
            rx(&mut link, kind::ALS, &[0; 4], t);
            out.extend(sent(&mut link).into_iter().filter(|&k| k != kind::TICK).map(|k| (t, k)));
        }
        assert_eq!(out, [(7.0, kind::FLUSH)]);
    }

    #[test]
    fn work_asked_for_during_the_handshake_waits_for_idle() {
        let mut link = Link::new(0.0, true);
        turn(&mut link, 0.0);
        link.switches(&[Switch::Fan], MAP, false);
        rx(&mut link, kind::BP_STATUS, &[0, 0], 0.0);
        identify(&mut link, 0.0);
        assert!(sent(&mut link).iter().all(|&k| k != kind::SWITCH), "not before the MCU mode's ack");
        rx(&mut link, kind::MCU_MODE_ACK, &[], 0.0);
        assert_eq!(sent(&mut link), [kind::SWITCH]);
        assert_eq!(link.state(), State::WaitingAcks);
    }

    #[test]
    fn switching_waits_for_an_ack_of_the_state_asked() {
        let mut link = ready();
        link.switches(&[Switch::Stage1Heat], MAP, false);
        turn(&mut link, 0.1);
        assert_eq!(link.take_out(), [Out::Send(frame(kind::SWITCH, &[0, 1]))]);
        assert_eq!(link.state(), State::WaitingAcks);
        rx(&mut link, kind::ACK_SWITCH, &[0, 0], 0.2);
        assert_eq!(link.state(), State::WaitingAcks, "an ack of the other state is not the one awaited");
        rx(&mut link, kind::ACK_SWITCH, &[0, 1], 0.3);
        assert_eq!(link.state(), State::Idle);
        assert_eq!(link.take_reports(), [Report::Switched { circuit: 0, on: true }]);
    }

    #[test]
    fn an_unacknowledged_switch_goes_again() {
        let mut link = ready();
        link.switches(&[Switch::Stage1Cool], MAP, false);
        turn(&mut link, 0.0);
        assert_eq!(sent(&mut link), [kind::SWITCH]);
        turn(&mut link, 1.0);
        assert!(sent(&mut link).is_empty());
        rx(&mut link, kind::ALS, &[0; 4], 2.5);
        assert_eq!(link.take_out(), [Out::Send(frame(kind::SWITCH, &[1, 1]))]);
        rx(&mut link, kind::ACK_SWITCH, &[1, 1], 2.6);
        turn(&mut link, 5.0);
        assert!(sent(&mut link).is_empty());
    }

    #[test]
    fn ticks_come_every_thirty_seconds_but_not_while_waiting_for_the_wires() {
        let mut link = ready();
        let mut ticks = Vec::new();
        let mut t = 0.0;
        while t < 100.0 {
            t += 1.0;
            rx(&mut link, kind::ALS, &[0; 4], t);
            if sent(&mut link).contains(&kind::TICK) {
                ticks.push(t);
            }
        }
        assert_eq!(ticks, [31.0, 62.0, 93.0]);
        link.raise(Event::Reset { hard: false });
        rx(&mut link, kind::ALS, &[0; 4], 101.0);
        let mut out = Vec::new();
        while t < 140.0 {
            t += 1.0;
            rx(&mut link, kind::ALS, &[0; 4], t);
            out.extend(sent(&mut link));
        }
        assert!(!out.contains(&kind::TICK));
    }

    #[test]
    fn silence_resets_the_link_three_times_soft_three_hard_then_it_is_down() {
        let mut link = ready();
        let mut resets = Vec::new();
        let mut t = 0.0;
        for _ in 0..40 {
            t += LOOP;
            turn(&mut link, t);
            resets.extend(sent(&mut link).into_iter().filter(|&k| k == kind::RESET || k == PULSE).map(|k| (t, k)));
        }
        assert_eq!(resets, [(6.0, kind::RESET), (12.0, kind::RESET), (18.0, kind::RESET), (24.0, PULSE), (30.0, PULSE), (36.0, PULSE)]);
        assert_eq!(link.take_reports().iter().filter(|r| **r == Report::Down).count(), 1);
        rx(&mut link, kind::WIRES, &[0; 13], t);
        rx(&mut link, kind::PINS, &[0; 15], t);
        rx(&mut link, kind::FW_ID, &[0; 3], t);
        assert!(link.up, "an identified backplate brings the link up");
    }

    #[test]
    fn a_low_battery_keeps_resets_off_the_reset_line() {
        let mut link = ready();
        let mut out = Vec::new();
        for i in 1..=40 {
            link.poll(f64::from(i) * LOOP, 0, || 3.6);
            out.extend(sent(&mut link));
        }
        assert!(!out.contains(&PULSE));
        assert_eq!(out.iter().filter(|&&k| k == kind::RESET).count(), 6);
    }

    #[test]
    fn bytes_waiting_unread_put_off_a_silence_reset_twice() {
        let mut link = ready();
        let mut resets = Vec::new();
        for i in 1..=10 {
            let t = f64::from(i) * LOOP;
            link.poll(t, 5, || 3.8);
            resets.extend(sent(&mut link).into_iter().filter(|&k| k == kind::RESET).map(|_| t));
        }
        assert_eq!(resets, [18.0]);
    }

    #[test]
    fn an_unanswered_request_goes_again_every_two_seconds_then_resets() {
        let mut link = Link::new(0.0, true);
        turn(&mut link, 0.0);
        rx(&mut link, kind::BP_STATUS, &[0, 0], 0.0);
        rx(&mut link, kind::WIRES, &[0; 13], 0.0);
        rx(&mut link, kind::PINS, &[0; 15], 0.0);
        sent(&mut link);
        let mut out = Vec::new();
        let mut t = 0.0;
        while t < 13.0 {
            t += 0.5;
            rx(&mut link, kind::ALS, &[0; 4], t);
            out.extend(sent(&mut link).into_iter().map(|k| (t, k)));
        }
        let ids: Vec<_> = out.iter().filter(|o| o.1 == kind::GET_FW_ID).map(|o| o.0).collect();
        assert_eq!(ids, [2.0, 4.0, 6.0, 8.0, 10.0, 12.0]);
        assert!(out.contains(&(12.0, kind::RESET)), "the sixth repeat is followed by a reset");
        assert!(link.take_reports().contains(&Report::Reset { reason: Reason::Retries, hard: false }));
    }

    #[test]
    fn the_mcu_mode_is_asked_once_and_repeated_until_acked() {
        // Stock on nest-qemu, whose backplate never acks it: 8b after the
        // identity, repeated, a reset after six, and not asked again.
        let mut link = Link::new(0.0, true);
        turn(&mut link, 0.0);
        rx(&mut link, kind::BP_STATUS, &[0, 0], 0.0);
        identify(&mut link, 0.0);
        assert!(sent(&mut link).ends_with(&[kind::SET_MCU_MODE, kind::POWER_STEAL]));
        let mut out = Vec::new();
        let mut t = 0.0;
        while t < 30.0 && !out.contains(&kind::RESET) {
            t += 2.0;
            rx(&mut link, kind::TEMP, &temp(2156, 450), t);
            out.retain(|&k| k == kind::SET_MCU_MODE || k == kind::RESET);
            out.extend(sent(&mut link).into_iter().filter(|&k| k == kind::SET_MCU_MODE || k == kind::RESET));
        }
        assert_eq!(out, [&[kind::SET_MCU_MODE; 6][..], &[kind::RESET]].concat());
        identify(&mut link, t);
        assert!(!sent(&mut link).contains(&kind::SET_MCU_MODE));
        assert_ne!(link.state(), State::WaitingMCUModeSetAck);
    }

    #[test]
    fn a_corrupt_wires_frame_resets_the_backplate() {
        let mut link = ready();
        link.corrupt(kind::TEMP, || 3.8);
        turn(&mut link, 0.1);
        assert!(sent(&mut link).is_empty());
        link.corrupt(kind::WIRES, || 3.8);
        turn(&mut link, 0.2);
        assert_eq!(sent(&mut link), [BREAK, kind::RESET]);
        assert!(link.take_reports().contains(&Report::Reset { reason: Reason::CorruptType, hard: false }));
    }

    /// A power report: state, px0, then Voc and Vin in 10 mV.
    fn power(state: u8, px0: u8, voc: u16, vin: u16) -> Vec<u8> {
        let mut p = vec![0; 16];
        p[0] = state;
        p[2] = px0;
        p[6..8].copy_from_slice(&voc.to_le_bytes());
        p[8..10].copy_from_slice(&vin.to_le_bytes());
        p
    }

    #[test]
    fn a_power_report_estimates_the_current_and_keeps_the_last_valid_state() {
        let mut link = ready();
        rx(&mut link, kind::PWR, &power(4, 10, 0, 2400), 1.0);
        assert_eq!(link.take_reports(), [Report::Power { vusb: 2400, vin: 2400, current: 200, state: 4 }]);
        rx(&mut link, kind::PWR, &power(12, 0x13, 500, 2400), 2.0);
        assert_eq!(link.take_reports(), [Report::Power { vusb: 500, vin: 2400, current: 0, state: 4 }]);
        assert!(sent(&mut link).is_empty(), "Rh is wired: no mismatch");
    }

    #[test]
    fn r_pins_without_r_wires_force_a_reset_five_times_in_a_row() {
        let mut link = ready();
        // A wires report without Rh, the pins still showing it.
        rx(&mut link, kind::WIRES, &[1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0], 1.0);
        sent(&mut link);
        link.take_reports();
        rx(&mut link, kind::PWR, &power(0, 0, 0, 2400), 2.0);
        assert!(!link.take_reports().iter().any(|r| matches!(r, Report::Reset { .. })), "unpowered: no mismatch");
        rx(&mut link, kind::PWR, &power(4, 0, 0, 2400), 3.0);
        assert!(link.take_reports().contains(&Report::Reset { reason: Reason::BpPowerMismatch, hard: true }));
        assert_eq!(link.mismatches, 1);
        link.mismatches = MISMATCH_RESETS;
        link.resets = 0;
        rx(&mut link, kind::PWR, &power(4, 0, 0, 2400), 4.0);
        assert!(!link.take_reports().iter().any(|r| matches!(r, Report::Reset { .. })), "max retry exceeded");
    }

    #[test]
    fn sensors_reading_nothing_reset_the_backplate_then_raise_the_alert() {
        let mut link = ready();
        rx(&mut link, kind::TEMP2, &[0; 12], 1.0);
        assert_eq!(link.take_reports(), [Report::Reset { reason: Reason::I2cDown, hard: false }]);
        assert_eq!(link.buffered, 1, "counted for the end of buffers");
        rx(&mut link, kind::TEMP2, &[0; 12], 2.0);
        assert_eq!(link.take_reports(), [Report::I2cDown(true), Report::I2cAllowed(false)]);
        link.detected(true, || 3.8);
        assert!(link.take_reports().contains(&Report::I2cDown(false)), "put back: the alert goes");
    }

    #[test]
    fn without_a_c_wire_a_newer_backplate_is_asked_its_ips_status() {
        let mut link = Link::new(0.0, true);
        turn(&mut link, 0.0);
        rx(&mut link, kind::BP_STATUS, &[0, 0], 0.0);
        rx(&mut link, kind::WIRES, &[1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0], 0.0);
        rx(&mut link, kind::PINS, &[1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0], 0.0);
        for k in [kind::FW_ID, kind::FW_VERS, kind::FW_INFO, kind::BSL_ID, kind::BSL_VERS, kind::BSL_INFO, kind::SERIAL] {
            rx(&mut link, k, b"x\x01", 0.0);
        }
        rx(&mut link, kind::MODEL, b"Backplate-6.0", 0.0);
        sent(&mut link);
        rx(&mut link, kind::MCU_MODE_ACK, &[], 0.0);
        assert!(sent(&mut link).contains(&kind::GET_IPS_STATUS));
        assert_eq!(link.state(), State::WaitingIPSStatus);
        rx(&mut link, kind::IPS_STATUS, &[1, 0, 7, 0], 0.5);
        assert_eq!(link.state(), State::Idle);
        // A week on, a wake asks again; unanswered, three retries give up.
        rx(&mut link, kind::TEMP, &temp(2150, 450), IPS_EVERY + 1.0);
        link.woke();
        turn(&mut link, IPS_EVERY + 1.0);
        assert!(!sent(&mut link).contains(&kind::GET_IPS_STATUS), "after the day's safety data");
        rx(&mut link, kind::SAFETY_TEMP_DATA, &[0; 8], IPS_EVERY + 1.0);
        // The temperature's flush ends, then the wake's deferred restart
        // and request go, one a turn.
        rx(&mut link, kind::END_BUF, &[], IPS_EVERY + 1.0);
        turn(&mut link, IPS_EVERY + 1.0);
        turn(&mut link, IPS_EVERY + 1.0);
        assert!(sent(&mut link).contains(&kind::GET_IPS_STATUS));
        for i in 1..=3 {
            rx(&mut link, kind::TEMP, &temp(2150, 450), IPS_EVERY + 1.0 + RETRY * f64::from(i));
        }
        assert!(!sent(&mut link).contains(&kind::GET_IPS_STATUS), "never asked again");
        assert_ne!(link.state(), State::WaitingIPSStatus, "given up");
    }

    #[test]
    fn a_ptac_never_gets_its_second_stages() {
        let mut link = ready();
        link.set_ptac(true);
        link.switches(&[Switch::Fan, Switch::Stage1Heat, Switch::Stage2Heat], MAP, false);
        let mut frames = Vec::new();
        for (i, ack) in [[2u8, 1], [0, 1], [4, 1]].iter().enumerate() {
            turn(&mut link, 0.1 + i as f64);
            frames.extend(switch_frames(&mut link));
            rx(&mut link, kind::ACK_SWITCH, ack, 0.2 + i as f64);
        }
        assert!(frames.contains(&(0, 1)) && frames.contains(&(2, 1)));
        assert!(!frames.iter().any(|f| f.0 == 4), "stage 2 heat (circuit 4) never sent: {frames:?}");
    }

    #[test]
    fn safety_temperatures_go_out_and_their_answer_switches_again() {
        let mut link = ready();
        link.switches(&[Switch::Fan, Switch::Stage1Heat], MAP, false);
        rx(&mut link, kind::ACK_SWITCH, &[2, 1], 0.5);
        rx(&mut link, kind::ACK_SWITCH, &[0, 1], 0.5);
        sent(&mut link);
        let payload = [3, 0, 0x95, 1, 0xe3, 1, 5, 0, 0x74, 0x0d, 0xe3, 0x0d, 6, 0, 0, 0];
        link.safety(Some(payload));
        turn(&mut link, 1.0);
        assert_eq!(sent(&mut link), [kind::SET_SAFETY_TEMP]);
        assert_eq!(link.state(), State::WaitingSafetyTempSetAck);
        rx(&mut link, kind::SAFETY_TEMP_DATA, &[0; 0x18], 1.5);
        assert_eq!(switch_frames(&mut link), [(2, 1), (0, 1)], "every switch on goes again");
    }

    #[test]
    fn a_temperature_error_code_pulses_the_reset_line_and_the_last_reading_stays() {
        let mut link = ready();
        rx(&mut link, kind::TEMP, &temp(2150, 450), 1.0);
        rx(&mut link, kind::END_BUF, &[], 1.0);
        link.take_reports();
        sent(&mut link);
        rx(&mut link, kind::TEMP, &temp(-0x7fff, 470), 2.0);
        assert_eq!(link.take_reports(), [
            Report::Reset { reason: Reason::GrilleTempErrorMsg, hard: true },
            Report::Climate { celsius: 21.5, humidity: Some(47.0) },
        ]);
        assert_eq!(sent(&mut link), [PULSE]);
        let mut fresh = ready();
        rx(&mut fresh, kind::TEMP, &temp(2200, 0xffff), 1.0);
        assert_eq!(fresh.take_reports(), [
            Report::Reset { reason: Reason::CorruptedTempMsg, hard: true },
            Report::Climate { celsius: 22.0, humidity: Some(45.0) },
        ]);
    }

    #[test]
    fn a_sleep_tells_the_backplate_what_wakes_the_unit_as_stock_does() {
        let mut link = ready();
        link.quiet(Wake { steps: vec![(0, Some(21.0 - 0x63d7 as f32 / 65536.0), None)], max_sleep: 57, humidity: (None, None), als: (0, 0xffff), vergence: (0, 0, 0) });
        turn(&mut link, 0.0);
        let out: Vec<(u16, Vec<u8>)> = link.take_out().into_iter().filter_map(|o| match o { Out::Send(f) => Some((f.kind, f.payload)), _ => None }).collect();
        // Stock on nest-qemu, heat at 21 °C, idle.
        let hex = |s: &str| (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect::<Vec<u8>>();
        assert_eq!(out, [
            (0xc2, hex("00000d080080")), (0xb7, hex("6400")), (0xb6, hex("64006400d4ffbbff")), (0xb3, hex("ffffffff")),
            (0xb9, hex("0000ffff")), (0xba, hex("000000000000")), (0xa4, hex("ef0f")), (0xa1, hex("3900")),
        ]);
        // Quiet: no tick, no repeat and no watch on silence.
        for i in 1..=50 {
            turn(&mut link, f64::from(i) * LOOP);
        }
        assert_eq!(sent(&mut link), Vec::<u16>::new());
        // Waking restarts the messages with a tick, and the tick overdue
        // follows.
        link.woke();
        turn(&mut link, 101.0);
        assert_eq!(sent(&mut link), [kind::TICK, kind::TICK]);
    }

    /// A gen 2's link, its backplate described as a Backplate-2.21.
    fn gen2() -> Link {
        let mut link = Link::new(0.0, true).profile(Profile::GEN2, Some(15));
        turn(&mut link, 0.0);
        rx(&mut link, kind::BP_STATUS, &[0, 0], 0.0);
        rx(&mut link, kind::WIRES, &[1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0], 0.0);
        rx(&mut link, kind::PINS, &[1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0], 0.0);
        for k in [kind::FW_ID, kind::FW_VERS, kind::FW_INFO, kind::BSL_ID, kind::BSL_VERS, kind::BSL_INFO, kind::SERIAL] {
            rx(&mut link, k, b"x\x01\x00", 0.0);
        }
        sent(&mut link);
        link
    }

    #[test]
    fn a_gen_2_asks_only_the_power_steal_mode_once_described() {
        let mut link = gen2();
        rx(&mut link, kind::MODEL, b"Backplate-2.21", 0.0);
        assert_eq!(sent(&mut link), [kind::POWER_STEAL]);
        link.safety(Some([1; 16]));
        turn(&mut link, 0.5);
        assert_eq!(sent(&mut link), [kind::POWER_STEAL], "a change sends the power-steal mode again");
        assert_eq!(link.state(), State::Idle);
        // No daily safety data either.
        rx(&mut link, kind::TEMP, &temp(2150, 450), SAFETY_DATA + 1.0);
        assert!(!sent(&mut link).contains(&kind::GET_SAFETY_TEMP_DATA));
    }

    #[test]
    fn a_gen_2s_near_pir_past_its_threshold_is_motion_and_wakes_it() {
        let mut link = gen2();
        rx(&mut link, kind::MODEL, b"Backplate-2.21", 0.0);
        link.take_reports();
        rx(&mut link, kind::NEAR_PIR, &14u16.to_le_bytes(), 1.0);
        assert!(!link.take_reports().contains(&Report::Motion { near: true }));
        rx(&mut link, kind::NEAR_PIR, &15u16.to_le_bytes(), 1.0);
        assert!(link.take_reports().contains(&Report::Motion { near: true }));
        rx(&mut link, kind::WAKEUP_VEC, &0x80u16.to_le_bytes(), 1.0);
        assert!(link.take_reports().contains(&Report::Motion { near: true }));
        sent(&mut link);
        link.quiet(Wake { steps: Vec::new(), max_sleep: 60, humidity: (None, None), als: (0, 0xffff), vergence: (0, 0, 0) });
        turn(&mut link, 2.0);
        let out = link.take_out();
        let payload = |k: u16| out.iter().find_map(|o| match o { Out::Send(f) if f.kind == k => Some(f.payload.clone()), _ => None });
        assert_eq!(payload(kind::NEAR_PIR_THRESHOLD), Some(15u16.to_le_bytes().to_vec()));
        assert_eq!(payload(kind::PIR_SENSITIVITY), None);
        assert_eq!(payload(kind::WAKE_SOURCES), Some(0x07e7u16.to_le_bytes().to_vec()), "as stock gen 2 sends it");
    }

    #[test]
    fn a_gen_2_takes_0x8001_as_a_temperature_and_resets_uncounted_on_0x8000() {
        let mut link = gen2();
        rx(&mut link, kind::MODEL, b"Backplate-2.21", 0.0);
        sent(&mut link);
        link.take_reports();
        rx(&mut link, kind::TEMP, &temp(-0x7fff, 450), 1.0);
        assert!(!sent(&mut link).contains(&PULSE));
        rx(&mut link, kind::TEMP, &temp(-0x8000, 450), 2.0);
        assert!(sent(&mut link).contains(&PULSE));
        assert!(!link.take_reports().iter().any(|r| matches!(r, Report::Reset { .. })), "not counted as a reset");
    }

    #[test]
    fn any_read_ends_quiet_mode() {
        let mut link = ready();
        link.quiet(Wake { steps: Vec::new(), max_sleep: 60, humidity: (None, None), als: (0, 0xffff), vergence: (0, 0, 0) });
        turn(&mut link, 0.0);
        sent(&mut link);
        link.read(50.0);
        turn(&mut link, 50.0);
        assert_eq!(sent(&mut link), [kind::TICK], "ticks resume once awake");
    }

    #[test]
    fn a_wake_without_a_temperature_within_35_seconds_resets_the_backplate() {
        let mut link = ready();
        link.quiet(Wake { steps: Vec::new(), max_sleep: 60, humidity: (None, None), als: (0, 0xffff), vergence: (0, 0, 0) });
        turn(&mut link, 0.0);
        link.woke();
        let mut t = 100.0;
        while t < 140.0 {
            rx(&mut link, kind::ALS, &[0; 4], t);
            t += 1.0;
        }
        assert!(link.take_reports().contains(&Report::Reset { reason: Reason::MissedTemp, hard: false }));
    }

    #[test]
    fn an_unsolicited_wires_report_drops_the_acks_awaited() {
        let mut link = ready();
        link.switches(&[Switch::Stage1Cool], MAP, false);
        turn(&mut link, 0.0);
        rx(&mut link, kind::WIRES, &[0; 13], 0.5);
        assert_eq!(link.state(), State::WaitingPinReport);
        rx(&mut link, kind::PINS, &[0; 15], 0.6);
        identify(&mut link, 0.7);
        rx(&mut link, kind::MCU_MODE_ACK, &[], 0.7);
        sent(&mut link);
        turn(&mut link, 5.0);
        assert!(!sent(&mut link).contains(&kind::SWITCH));
    }

    #[test]
    fn an_image_goes_a_record_an_ack_and_a_bootloader_restarts_the_backplate() {
        let mut link = ready();
        sent(&mut link);
        link.take_reports();
        link.update(b'M', vec![b"S003".to_vec(), b"S903".to_vec()]);
        turn(&mut link, 1.0);
        assert_eq!(sent(&mut link), [kind::UPDATE_BEGIN]);
        rx(&mut link, kind::UPDATE_ACK, &1u16.to_le_bytes(), 1.1);
        assert!(sent(&mut link).is_empty(), "out of sequence");
        for (seq, next) in [(0u16, kind::UPDATE_RECORD), (1, kind::UPDATE_RECORD), (2, kind::UPDATE_END)] {
            rx(&mut link, kind::UPDATE_ACK, &seq.to_le_bytes(), 1.2 + f64::from(seq));
            assert_eq!(sent(&mut link), [next]);
        }
        // Unanswered, the end goes again.
        turn(&mut link, 6.0);
        assert_eq!(sent(&mut link), [kind::UPDATE_END]);
        rx(&mut link, kind::UPDATE_ACK, &3u16.to_le_bytes(), 6.1);
        assert!(link.take_reports().contains(&Report::UpdateSent(b'M')));
        assert!(sent(&mut link).is_empty(), "a firmware restarts itself");
        link.update(b'B', vec![b"S003".to_vec()]);
        turn(&mut link, 7.0);
        for seq in 0u16..3 {
            rx(&mut link, kind::UPDATE_ACK, &seq.to_le_bytes(), 7.1 + f64::from(seq));
        }
        assert_eq!(sent(&mut link), [kind::UPDATE_BEGIN, kind::UPDATE_RECORD, kind::UPDATE_END, BREAK, kind::RESET]);
    }

    #[test]
    fn detached_the_link_stands_still_and_a_new_attach_resets() {
        let mut link = ready();
        link.detected(false, || 3.8);
        link.read(1.0);
        link.receive(&frame(kind::TEMP, &temp(2150, 450)), 1.0);
        for i in 1..=20 {
            turn(&mut link, f64::from(i));
        }
        assert!(sent(&mut link).is_empty());
        assert_eq!(link.take_reports(), [Report::Attached(false)], "only the line itself");
        link.detected(true, || 3.8);
        turn(&mut link, 21.0);
        assert_eq!(sent(&mut link), [BREAK, kind::RESET]);
        assert_eq!(link.take_reports(), [Report::Attached(true), Report::Reset { reason: Reason::Attach, hard: false }]);
    }

    #[test]
    fn a_status_asking_for_safety_temperatures_holds_the_reset_until_they_come() {
        let mut link = Link::new(0.0, true);
        turn(&mut link, 0.0);
        sent(&mut link);
        rx(&mut link, kind::BP_STATUS, &[8, 0], 0.0);
        assert_eq!(sent(&mut link), [kind::BP_STATUS_ACK, kind::GET_SAFETY_TEMP_DATA]);
        rx(&mut link, kind::SAFETY_TEMP_DATA, &[0; 0x18], 0.1);
        assert_eq!(sent(&mut link), [BREAK, kind::RESET]);
    }

    #[test]
    fn errors_are_asked_for_and_over_current_switches_retried_then_cleared() {
        let mut link = ready();
        link.switches(&[Switch::Stage1Heat], MAP, false);
        turn(&mut link, 0.0);
        rx(&mut link, kind::ACK_SWITCH, &[0, 1], 0.0);
        sent(&mut link);
        link.take_reports();
        rx(&mut link, kind::WAKEUP, &[10], 0.1);
        assert_eq!(sent(&mut link), [kind::GET_ERROR]);
        assert_eq!(link.state(), State::WaitingError);
        let mut faults = [0u8; 13];
        faults[0] = 1;
        for i in 0..10 {
            let t = 1.0 + f64::from(i);
            rx(&mut link, kind::ERROR_VECTOR, &[1, 0], t);
            assert_eq!(sent(&mut link), [kind::GET_OC_ERRORS]);
            rx(&mut link, kind::OC_ERRORS, &faults, t);
            assert_eq!(link.take_out(), [Out::Send(frame(kind::SWITCH, &[0, 1]))], "the faulted circuit is asked on again");
            rx(&mut link, kind::ACK_SWITCH, &[0, 1], t);
            link.raise(Event::GetError);
        }
        rx(&mut link, kind::ERROR_VECTOR, &[1, 0], 20.0);
        sent(&mut link);
        link.take_reports();
        rx(&mut link, kind::OC_ERRORS, &faults, 20.0);
        assert_eq!(link.take_out(), [Out::Send(frame(kind::SWITCH, &[0, 0]))], "then the switches are cleared");
        assert!(link.take_reports().contains(&Report::SwitchFault(true)));
        rx(&mut link, kind::ACK_SWITCH, &[0, 0], 20.1);
        link.raise(Event::GetError);
        rx(&mut link, kind::ERROR_VECTOR, &[0, 0], 21.0);
        assert_eq!(link.state(), State::Idle);
        assert!(link.take_reports().contains(&Report::SwitchFault(false)));
    }

    #[test]
    fn a_bootloader_only_backplate_is_described_by_its_id_alone() {
        let mut link = Link::new(0.0, true);
        turn(&mut link, 0.0);
        rx(&mut link, kind::BP_STATUS, &[0, 0], 0.0);
        rx(&mut link, kind::WIRES, &[0; 13], 0.0);
        rx(&mut link, kind::PINS, &[0; 15], 0.0);
        link.take_reports();
        rx(&mut link, kind::FW_ID2, &[1, 2, 3], 0.0);
        assert_eq!(link.state(), State::Idle);
        assert!(matches!(link.take_reports().as_slice(), [Report::Ready(id)] if id.firmware_id == [1, 2, 3]));
    }

    fn switch_frames(link: &mut Link) -> Vec<(u8, u8)> {
        link.take_out().into_iter().filter_map(|o| match o { Out::Send(f) if f.kind == kind::SWITCH => Some((f.payload[0], f.payload[1])), _ => None }).collect()
    }

    #[test]
    fn switches_go_off_then_on_in_stocks_order_with_o_b_among_the_offs() {
        let mut link = ready();
        link.switches(&[Switch::Stage2Heat, Switch::Stage1Heat, Switch::Fan], MAP, false);
        turn(&mut link, 0.1);
        assert_eq!(switch_frames(&mut link), [(2, 1), (0, 1), (4, 1)], "fan, stage 1, stage 2");
        for c in [2, 0, 4] {
            rx(&mut link, kind::ACK_SWITCH, &[c, 1], 0.2);
        }
        link.switches(&[Switch::Fan, Switch::Stage1Cool, Switch::Ob], MAP, false);
        turn(&mut link, 0.3);
        assert_eq!(switch_frames(&mut link), [(4, 0), (0, 0), (3, 1), (1, 1)], "stage 2, stage 1, O/B, then cool");
    }

    #[test]
    fn a_reset_forgets_the_switches_so_the_next_decision_sends_them_again() {
        let mut link = ready();
        link.switches(&[Switch::Fan, Switch::Stage1Heat], MAP, false);
        turn(&mut link, 0.1);
        assert_eq!(switch_frames(&mut link), [(2, 1), (0, 1)]);
        rx(&mut link, kind::ACK_SWITCH, &[2, 1], 0.2);
        rx(&mut link, kind::ACK_SWITCH, &[0, 1], 0.2);
        link.switches(&[Switch::Fan, Switch::Stage1Heat], MAP, false);
        turn(&mut link, 0.3);
        assert!(switch_frames(&mut link).is_empty());
        rx(&mut link, kind::WIRES, &[0; 13], 0.4);
        link.switches(&[Switch::Fan, Switch::Stage1Heat], MAP, false);
        rx(&mut link, kind::PINS, &[0; 15], 0.5);
        identify(&mut link, 0.5);
        // Deferred, they go one a turn, each once Idle again.
        turn(&mut link, 0.6);
        assert_eq!(switch_frames(&mut link), [(2, 1)]);
        rx(&mut link, kind::ACK_SWITCH, &[2, 1], 0.7);
        assert_eq!(switch_frames(&mut link), [(0, 1)]);
    }

    #[test]
    fn stage_three_cool_is_never_sent() {
        let mut link = ready();
        let mut map = MAP;
        map[Switch::Stage3Cool as usize] = Some(11);
        link.switches(&[Switch::Stage3Cool], map, false);
        turn(&mut link, 0.1);
        assert!(switch_frames(&mut link).is_empty());
    }

    #[test]
    fn the_power_steal_mask_leaves_the_wired_heat_or_cool_stages_alone() {
        let mut link = ready();
        link.switches(&[], MAP, false);
        let mask = |link: &mut Link| link.take_out().into_iter().find_map(|o| match o {
            Out::Send(f) if f.kind == kind::POWER_STEAL => Some(u32::from_le_bytes([f.payload[0], f.payload[1], f.payload[2], f.payload[3]])),
            _ => None,
        });
        for (mode, want) in [(2, 1 << 0 | 1 << 4), (3, 1 << 1), (1, u32::MAX), (0, 0)] {
            link.power_steal(mode);
            turn(&mut link, 0.1);
            assert_eq!(mask(&mut link), Some(want), "mode {mode}");
        }
    }

    #[test]
    fn the_safety_data_is_asked_for_daily() {
        let mut link = ready();
        let mut t = 0.0;
        let mut asked = Vec::new();
        while t < 2.0 * SAFETY_DATA + 10.0 {
            t += 60.0;
            rx(&mut link, kind::TEMP, &temp(2150, 450), t);
            rx(&mut link, kind::END_BUF, &[], t);
            for k in sent(&mut link) {
                if k == kind::GET_SAFETY_TEMP_DATA {
                    asked.push(t);
                    rx(&mut link, kind::SAFETY_TEMP_DATA, &[0; 0x18], t);
                }
            }
        }
        assert_eq!(asked, [86400.0, 172800.0]);
    }

    #[test]
    fn a_thermopile_maximum_past_its_threshold_is_motion() {
        let mut link = ready().thermopile((100, 100), (-44, -69), true);
        let tp = |v: [i16; 6]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        rx(&mut link, kind::TP, &tp([100, -44, 90, -10, 100, -69]), 1.0);
        assert!(link.take_reports().is_empty());
        rx(&mut link, kind::TP, &tp([10, 0, 101, 0, 0, 0]), 2.0);
        assert_eq!(link.take_reports(), [Report::Motion { near: true }]);
        // Waking on its bit of the wake vector, while it may.
        rx(&mut link, kind::WAKEUP_VEC, &[0x80, 0], 3.0);
        assert_eq!(link.take_reports(), [Report::Motion { near: true }]);
        link.thermopile_wake(false);
        rx(&mut link, kind::WAKEUP_VEC, &[0x80, 0], 4.0);
        assert!(link.take_reports().is_empty());
        // The PIR's bit is someone across the room.
        rx(&mut link, kind::WAKEUP_VEC, &[0x88, 0], 5.0);
        assert_eq!(link.take_reports(), [Report::Motion { near: false }]);
    }

    #[test]
    fn a_reversed_dehumidifier_is_energised_at_idle_and_off_while_dehumidifying() {
        let mut link = ready();
        let mut map = MAP;
        map[Switch::Dehumidifier as usize] = Some(11);
        link.switches(&[], map, true);
        turn(&mut link, 0.1);
        assert_eq!(switch_frames(&mut link), [(11, 1)]);
        rx(&mut link, kind::ACK_SWITCH, &[11, 1], 0.2);
        link.switches(&[Switch::Dehumidifier], map, true);
        turn(&mut link, 0.3);
        assert_eq!(switch_frames(&mut link), [(11, 0)]);
    }
}
