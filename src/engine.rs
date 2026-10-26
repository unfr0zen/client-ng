//! The UI's behaviour: what the wheel and the button do on each screen,
//! and the view a skin draws.

use std::time::{Duration, Instant};

use crate::menu::{Nav, Outcome, Tree};
use crate::platform::Device;
use crate::model::{to_fahrenheit, Settings, Side, Thermostat, Value, MODE, UNITS};
use crate::skin::{ThermostatView, Ui};

/// Input from the wheel and the button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    /// Wheel detents, clockwise positive.
    Wheel(i32),
    /// A mouse's notches (the desktop's), clockwise positive: steps as the
    /// wheel's, but a list, carousel or ring moves an entry for each.
    #[cfg_attr(not(feature = "desktop"), allow(dead_code))]
    Notch(i32),
    Press,
    Release,
}

/// The gen 3's dial counts to one step of the wheel, clockwise being
/// negative: two of its detents (about 120 counts each), so a step
/// takes a deliberate turn.
pub const DIAL_STEP: i32 = -240;

/// Stock's alert when the backplate stopped answering
/// (BackplateConnectionsPleaseReAttach).
/// The Reset pages while they work (failsafelayer), and All Settings'
/// goodbye before its restart (safetyswitchlayer.goodbyemsg).
const RESETTING_NETWORK: &str = "Resetting network";
const RESETTING_ALL: &str = "Resetting to factory\ndefaults";
const GOODBYE: &str = "The thermostat is\nrestarting. Back in a bit.";
/// Stock's failsafelayer counts a reset down from this many seconds,
/// once a second, a press cancelling it (0x13bf88, 0x13c768).
const RESET_COUNT: f64 = 10.0;
/// Each reset's countdown, by its menu action (stock's failsafelayer
/// kinds 0 to 4, 0x19d28c).
const RESET_COUNTDOWNS: [(&str, &str); 6] = [
    ("reset.autoschedule", "@failsafelayer.resettinginseconds.autoschedule.aw|Resetting Auto-Schedule in %d seconds"),
    ("reset.schedule.clear", "@failsafelayer.resettinginseconds.schedule.aw|Resetting schedule in %d seconds"),
    ("reset.schedule.basic", "@failsafelayer.resettinginseconds.schedule.aw|Resetting schedule in %d seconds"),
    ("reset.away", "@failsafelayer.resettinginseconds.away.aw|Resetting Away in %d seconds"),
    ("reset.network", "@failsafelayer.resettinginseconds.network.aw|Resetting network in %d seconds"),
    ("reset.all", "@failsafelayer.resettinginseconds.defaults.aw|Resetting to factory defaults in %d seconds"),
];

pub const REATTACH: &str = "Please remove the\nthermostat from its\nbase, then reattach it.";
/// A screen's text on one line, for the log.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// An error page coming up or going.
fn log_error_page(page: Option<&crate::skin::ErrorPage>) {
    match page {
        Some(p) => info!("wiring: {} {}", p.code, p.title.to_lowercase()),
        None => info!("wiring: error page gone"),
    }
}

/// A time's 5-minute bin of the local day.
fn day_bin(t: f64) -> usize {
    (crate::sunblock::Local::at(t).seconds / 300) as usize
}

/// Up while the backplate's firmware is updated: stock's update layer's
/// text (updatelayer.updatingnest), which its update_backplate step is taken
/// to show.
pub const UPDATING: &str = "Updating the thermostat.";

/// Stock's alert when the detect line finds no backplate
/// (BackplateConnectionsPleaseAttach).
pub const ATTACH: &str = "To use the thermostat,\nattach the display\nto its base.";

/// The backplate's connection as stock's connections manager sees it
/// (0x2320cc): comm, I2C and the detect line, each up until told otherwise.
#[derive(Clone, Copy, Debug)]
pub struct Connections {
    pub comm: bool,
    pub i2c: bool,
    pub attached: bool,
    /// Re-attached while the reattach alert was up: I2C down alone no
    /// longer keeps it.
    pub reattached: bool,
}

impl Default for Connections {
    fn default() -> Connections {
        Connections { comm: true, i2c: true, attached: true, reattached: false }
    }
}

impl Connections {
    /// The alert they call for: reattach with comm down, or I2C down
    /// unless re-attached since; else attach with the line detached.
    pub fn alert(&self) -> Option<&'static str> {
        if !self.comm || (!self.i2c && !self.reattached) {
            Some(REATTACH)
        } else if !self.attached {
            Some(ATTACH)
        } else {
            None
        }
    }

    /// The detect line settled; attaching while the reattach alert is up
    /// latches `reattached`.
    pub fn set_attached(&mut self, attached: bool) {
        if attached && !self.attached && self.alert() == Some(REATTACH) {
            self.reattached = true;
        }
        self.attached = attached;
    }

    pub fn set_i2c(&mut self, up: bool) {
        self.i2c = up;
        self.reattached = false;
    }
}

/// The notice when the backplate's wires differ from those last seen.
pub const WIRING_CHANGED: &str = "The wiring to your\nequipment has changed.";
/// The notice instead when the new wiring cannot heat (stock with no W1).
pub const CANNOT_CONTROL: &str = "Your thermostat\ncannot control this\nequipment properly.";
/// The wires last seen, a digit a pin, for telling a change.
const WIRING: &str = "wiring.pins";
/// Where the HVAC control's learning is kept: the heat and cool bands,
/// the heat and the cool side's average change and cycle minutes, then
/// Airwave's offset, fan run and its variance.
/// Early-On's plan: the setpoint it works towards (epoch seconds and
/// temperature), heating or cooling, when it started and from what room,
/// and whether a change by hand cancelled it.
#[derive(Clone, Debug, PartialEq)]
struct EarlyOn {
    due: f64,
    next: f32,
    heat: bool,
    /// When it started.
    start: Option<f64>,
    cancelled: bool,
}

/// Emergency heat as the mode (stock's mode 4, `emer_heat_enable`): "on"
/// or "off"; what entering it turned off, to give back on leaving; and
/// whether the wiring has it, for Equipment Settings.
pub const EMERGENCY: &str = "emergency";
const PRE_EMERGENCY: [(&str, &str); 2] = [("sense.learning", "emergency.pre.learning"), ("away.assist", "emergency.pre.away")];
const HAS_EMERGENCY: &str = "equipment.emergency_heat";
/// What the wiring has, for the menus' conditions (stock's wiring
/// capabilities 0, the fan, and 2, a cool stage): the fan, a cool stage,
/// both (Airwave's), and Early-On's entry (beside True Radiant's, only
/// with a cool stage).
const HAS_FAN: &str = "equipment.fan";
const HAS_COOLING: &str = "equipment.cooling";
const HAS_AIRWAVE: &str = "equipment.airwave";
const HAS_EARLY_ON: &str = "equipment.early_on";
/// Equipment Settings' pages for conventional wiring, and the STAR
/// terminal's byte in MSG_PINS.
const HAS_HEAT_TYPE: &str = "equipment.heat_type";
const HAS_HEAT_PUMP: &str = "equipment.heat_pump";
const HAS_DUAL_FUEL: &str = "equipment.dual_fuel_system";
const HAS_HUMIDIFIER: &str = "equipment.humidifier";
const HAS_DEHUMIDIFIER: &str = "equipment.dehumidifier";
const HAS_STAR: &str = "equipment.star";
const STAR_PIN: usize = 11;
const HAS_DRY: &str = "equipment.dry";
const HAS_BALANCE: &str = "equipment.balance";
const HAS_FILTER: &str = "equipment.filter";
/// Heat Pump Balance's level (stock's vt+0x264/0x268): heatpump_savings,
/// the control's 0 to 3.
pub const BALANCE_LEVEL: &str = "sense.balance";
const BALANCE_LEVELS: [&str; 4] = ["max-savings", "balanced", "max-comfort", "off"];
/// Technical Info's names for the backplate's circuits (MSG_WIRES) and its
/// pins (MSG_PINS), as stock's 0x1bb3fc and 0x1bb644 name them; the star
/// terminal, stock's glyph U+E035, as "*".
const SENSED_NAMES: [&str; 13] = ["W1", "Y1", "G", "O/B", "W2/AUX", "", "", "Y2", "C", "Rc", "Rh", "*", ""];
const INSERTED_NAMES: [&str; 15] = ["W1", "Y1", "C", "Rc", "Rh", "G", "O/B", "W2/AUX", "", "Y2", "", "*", "", "", ""];
/// What the system runs: "both", "heat", "cool" or "none".
const SYSTEM: &str = "equipment.system";
/// Why Display › Weather can't show offline.
const WEATHER_OFFLINE: &str = "weather.offline";
/// The entries stock guards while emergency heat is on: in the top menu,
/// and in Settings.
const EMERGENCY_GUARDED: [(usize, &str); 5] = [(1, "Eco"), (1, "Schedule"), (1, "Mode"), (2, "Nest Sense"), (2, "Equipment")];

/// Where stock's time-to-temperature curves are kept, and Heat Pump
/// Balance's outdoor fit.
const TTT: &str = "learn.ttt";
const BALANCE: &str = "learn.balance";
/// What the overshoot gate learned (stock's overshoot_parameters).
const OVERSHOOT: &str = "learn.overshoot";
const LEARNED: [&str; 9] = ["learn.band.heat", "learn.band.cool", "learn.heat.change", "learn.heat.minutes", "learn.cool.change",
                            "learn.cool.minutes", "learn.airwave.offset", "learn.airwave.hours", "learn.airwave.hours.var"];
/// Auto-Schedule's days on in each mode (`.heat`, `.cool`, `.heat-cool`),
/// and the local day last counted (`.<mode>.day`, days since the epoch).
const LEARNING_DAYS: &str = "sense.learning.days";
/// Stock's Heat Link wiring errors (`0x2e4678`, as nest-qemu showed them):
/// H72 without Rh or C, H71 without W1.
pub const UNPOWERED: crate::skin::ErrorPage = crate::skin::ErrorPage {
    code: "H72", title: "HEAT LINK", text: "The power to this thermostat is off.", link: "nest.com/h72",
    choices: &["TECH INFO"],
};
pub const NO_HEAT: crate::skin::ErrorPage = crate::skin::ErrorPage {
    code: "H71", title: "HEAT LINK",
    text: "Your thermostat can't connect to the Heat Link and can't control your heat. Try moving your thermostat closer to the Heat Link, then choose CHECK AGAIN.",
    link: "nest.com/h71", choices: &["CHECK AGAIN", "RESET PAIRING", "REPLACE HEAT LINK", "TECH INFO"],
};

/// Stock's alerts outside the Heat Link gate: wiring it cannot use
/// (BackplateConnectionsUnsupported), and no power on R for 150 s
/// (PowerOff).
pub const UNSUPPORTED: &str = "Your thermostat\ncannot control this\nequipment properly\nwithout wiring changes.";
pub const POWER_OFF: &str = "The power from your\nequipment to the\nthermostat may be off.";
/// The power-off alert when the wiring report's last code is 46, Y1
/// unpowered by a compressor lockout in the cold
/// (BackplateConnectionsPowerOffDueToCompressorLockout, 0x2320cc), its
/// text taken as backplateconnectionsalert.poweroffduetotemp.
pub const POWER_OFF_LOCKOUT: &str = "Your thermostat isn't\ngetting power.\nTroubleshoot it to\navoid losing heating.";
const POWER_OFF_DELAY: Duration = Duration::from_secs(150);
/// Stock's HvacShortCycle alert (0x32, 0x156a60): the cloud found the
/// heating short cycling.
pub const SHORT_CYCLE_TITLE: &str = "POTENTIAL SYSTEM ISSUE";
pub const SHORT_CYCLE_TEXT: &str = "If your system isn't working normally, learn more about solutions at";
/// When the cloud last said so, in seconds since the epoch, 0 for not
/// (stock's `heating_short_cycle_detection_timestamp`); it holds for 48 h
/// from then (0x233b5c), across restarts.
const SHORT_CYCLE_SINCE: &str = "hvac.short_cycle.since";
const SHORT_CYCLE_HOLD: f64 = 172_800.0;
/// Stock's HvacBatteryDrained alert (0x33, 0x157814): heating with power
/// stealing off has run the battery down.
pub const BATTERY_DRAINED_TITLE: &str = "SYSTEM ISSUE";
pub const BATTERY_DRAINED_TEXT: &str = "Your thermostat can't control your heating system since its battery is too low. To restart heating as soon as possible, remove your thermostat from the wall and charge it using the micro USB port on the back. Once it's charged, reattach it to the wall. For a long-term solution, follow the instructions at";
/// Stock's DateTime alert (nlDateTimeAlert): at start, Wi-Fi off and
/// the time wrong (globals vtable +0x1b0: now before the build date).
pub const DATE_TIME_TEXT: &str = "Your thermostat's date and time are incorrect.";
/// This software's date, 2026-10-19 00:00 UTC: a clock before it is wrong,
/// as stock's is before its build date (0xb7ef8).
const BUILD_DATE: f64 = 1_792_368_000.0;
/// Set once the date and time were set by hand (stock's
/// `date_time_adj_completed`).
pub const CLOCK_ADJUSTED: &str = "clock.adjusted";
/// Stock's Switchover alert (switchoveralert.message.aw).
pub const SWITCHOVER_TEXT: &str = "To adjust the temperature, change the thermostat mode.";
/// Stock's Airwave alert (nlAirwaveAlert, glyph U+E046).
pub const AIRWAVE_TEXT: &str = "Airwave has reduced energy use while cooling";
/// Stock's FurnaceShutdown alert (0x1f, 0x1475c4): the furnace keeps
/// shutting down within 15 minutes of heating
/// (furnaceshutdownalert.message.aw, .furnaceshutoffurl.aw).
pub const FURNACE_SHUTDOWN_TEXT: &str = "Your thermostat noticed that your furnace shuts down within 15 minutes of heating. Learn how to troubleshoot this problem:";
/// The detection's data, kept across restarts (stock's
/// `furnaceshutdownrecoveryfile`).
const FURNACE: &str = "hvac.furnace";
/// Stock's `show_furnace_headsup_test`: kept, never cleared.
const FURNACE_HEADSUP_TEST: &str = "hvac.furnace.headsup_test";
/// How often the battery is read for it.
const BATTERY_EVERY: Duration = Duration::from_secs(60);
/// How long the HVAC control uses an outside temperature.
const WEATHER_STALE: Duration = Duration::from_secs(7200);

/// The menus as shown, and the settings that affect their structure.
struct ShownCache {
    tree_revision: u64,
    settings_revision: u64,
    values: Vec<(String, Option<Value>)>,
    tree: std::rc::Rc<Tree>,
}

fn menu_keys(items: &[crate::menu::Item], keys: &mut std::collections::BTreeSet<String>) {
    for item in items {
        if let Some((key, _)) = &item.shown.when {
            keys.insert(key.split_once('=').map_or(key.as_str(), |(key, _)| key).to_string());
        }
        for (key, _) in &item.shown.prompts {
            keys.insert(key.split_once('=').map_or(key.as_str(), |(key, _)| key).to_string());
        }
        if let Some(key) = &item.shown.fill { keys.insert(key.clone()); }
        match &item.kind {
            crate::menu::Kind::Menu(children) => menu_keys(children, keys),
            crate::menu::Kind::Set { key, .. } => { keys.insert(key.clone()); }
            _ => {}
        }
    }
}

/// Whether the clock can be right: not before `BUILD_DATE` (stock's
/// 0xb7ef8, globals vtable +0x1e4; the gen 2's +0x1b0).
fn time_valid(now: f64) -> bool {
    now >= BUILD_DATE
}

/// Whether stock's Heat Link wiring checks apply to the unit, by its
/// nlmodel and its backplate's version: on the gen 3 (`0xbf7a0`,
/// `0xb6f34`) Display-3 and Display-5, and Display-2 on a Backplate-3; on
/// the gen 2 (`0x4e200`) Display-5, and any on a Backplate-3 (Amber) or -6.
fn heat_link_unit(device: Device, model: &str, backplate_version: i32) -> bool {
    match device {
        Device::Gen3 => model.contains("Display-3.") || model.contains("Display-5.")
            || (model.contains("Display-2.") && backplate_version == 3),
        Device::Gen2 => model.contains("Display-5.") || matches!(backplate_version, 3 | 5),
    }
}

/// Stock's Heat Link wiring error (`0x2e4678`) for the wires (MSG_WIRES)
/// and pins (MSG_PINS): no Rh or C wire, H72; then no W1, H71.
fn heat_link_error(wires: &[u8; 13], pins: &[u8; 15]) -> Option<crate::skin::ErrorPage> {
    if wires[10] & 1 == 0 || wires[8] & 1 == 0 {
        Some(UNPOWERED)
    } else if pins[0] != 1 || wires[0] & 1 == 0 {
        Some(NO_HEAT)
    } else {
        None
    }
}

/// The settings key the Fan choice keeps its minutes under, and when the
/// run ends (seconds since the epoch, 0 for none).
pub const FAN_TIMER: &str = "fan.timer";
/// How long a change sent from elsewhere keeps the display lit.
pub const REMOTE_WAKE: Duration = Duration::from_secs(30);
/// The mode last in use other than off, which turning on gives back.
const MODE_ON: &str = "thermostat.mode.on";
/// Eco: whether it is on, and its temperatures (each a number or off).
pub const ECO: &str = "eco";
/// What the display does when someone is near: a Farsight view, or
/// "approach" (light the dial) or "press" (stay dark).
pub const FARSIGHT: &str = "display.farsight";
pub const ECO_HEAT: &str = "eco.heat";
pub const ECO_COOL: &str = "eco.cool";
/// The safety temperatures, each a number in Celsius or off.
pub const SAFETY_LOW: &str = "safety.low";
/// Eco set by Home/Away Assist (published as auto-eco), not by hand.
pub const ECO_AUTO: &str = "eco.auto";
/// The local days Home/Away Assist saw someone, as JSON (its training).
const AWAY_DAYS: &str = "away.days";
/// Home/Away Assist trains this many days with activity before acting,
/// and calls the home empty after this long without any (stock's 7200 s,
/// 0x3370dc).
const AWAY_TRAINING: usize = 7;
const AWAY_AFTER: f64 = 7200.0;

/// Stock's Wi-Fi alerts (wifinotificationalert, the disconnected and
/// fault alerts), each on the configured network.
#[derive(Clone, Debug, PartialEq)]
pub enum WifiAlert {
    /// 0x1c: the recoverer gave up on the network.
    NotFound(String),
    /// 0x1a: the network refused its passphrase.
    Password(String),
    /// 0x19: offline this many hours.
    Disconnected(u64),
    /// 0x30: Wi-Fi itself is not working.
    Fault,
}

impl WifiAlert {
    fn text(&self) -> String {
        match self {
            WifiAlert::NotFound(n) => format!("Your WiFi network {n}\ncould not be found"),
            WifiAlert::Password(n) => format!("The password has changed\nfor the network {n}"),
            WifiAlert::Disconnected(h) => format!("Your thermostat has been disconnected from the network for {}.", match h {
                1 => "1 hour".to_string(),
                2..=23 => format!("{h} hours"),
                24..=47 => "1 day".to_string(),
                h => format!("{} days", h / 24),
            }),
            WifiAlert::Fault => "Your thermostat is having trouble connecting to Wi-Fi right now.".into(),
        }
    }

    fn options(&self) -> &'static [&'static str] {
        match self {
            WifiAlert::Fault => &["DONE"],
            _ => &["WIFI SETTINGS", "DONE"],
        }
    }
}

/// The background recoverer (stock's network recoverer): an attempt on
/// the configured network this often while offline; this many failures
/// before the not-found alert, then this many more, the next try waiting
/// this long after it.
const RECOVER_EVERY: f64 = 30.0;
const RECOVER_LIMIT: u32 = 20;
const RECOVER_AFTER_ALERT: u32 = 3;
const RECOVER_WAIT: f64 = 600.0;

/// Plugins' values: key and what its command printed.
type Values = Vec<(String, String)>;
/// Technical Info's system facts and network state, read off the UI thread.
type TechRead = (String, Vec<(&'static str, String)>, Vec<crate::wifi::Service>, bool);
/// connman's services and `wifi::working`, read off the UI thread.
type NetworkResults = std::sync::Arc<std::sync::Mutex<Option<(Vec<crate::wifi::Service>, bool)>>>;

/// What the keyboard is typing for Wi-Fi: a network's name, its passphrase.
const WIFI_NAME: &str = "wifi.name";
const WIFI_PSK: &str = "wifi.passphrase";
/// The network the thermostat is configured for: the last one it was on.
const WIFI_HOME: &str = "net.home";

/// A custom room's name for Placement, and the device's label.
pub const PLACE_CUSTOM: &str = "placement.custom";
/// PRO SETUP's answers, as the device bucket keeps them (net.rs).
pub const BOILER: &str = "equipment.boiler_type";
pub const HEAT_CONTROL: &str = "equipment.heat_control";
pub const HEAT_SOURCE: &str = "equipment.heat_source";
pub const HEAT_DELIVERY: &str = "equipment.heat_delivery";
pub const HOT_WATER_CONTROL: &str = "equipment.hot_water_control";

/// What the top menu's Settings says under its name (empty for nothing).
pub const SETTINGS_NOTICE: &str = "settings.notice";
/// What Settings' Equipment tile says under its name (empty for nothing):
/// stock's System Issue while the short-cycle flag stands (0x139a9c).
pub const EQUIPMENT_NOTICE: &str = "equipment.notice";
const EQUIPMENT_ISSUE: &str = "@equipmentscreen.systemissue|System Issue";
/// The software as last run: its version, when it last changed (seconds
/// since the epoch), and the change last seen under the top menu's
/// Settings (stock's setting).
pub const SOFTWARE_VERSION: &str = "software.version";
pub const SOFTWARE_UPDATED: &str = "software.updated";
pub const SOFTWARE_SEEN: &str = "last_seen_time_of_swu";
/// Location: the country's ISO code, and its postal code (or Ireland's
/// county, the UAE's city).
pub const COUNTRY: &str = "location.country";
pub const POSTAL: &str = "location.postal";
/// The place the weather's answer names, as Location's tile gives it: its
/// city, state and postal code, a line each.
pub const PLACE: &str = "location.place";
/// That place as Location asks to confirm it ("Mountain View, CA" over its
/// postal code), and whether one is known.
pub const PLACE_ASKED: &str = "location.asked";
pub const PLACE_KNOWN: &str = "location.known";
/// The keyboard's text is for a menu entry's ask.
const ASKED: &str = "";
pub const LABEL: &str = "device.label";

/// The filter reminder: the replacement threshold and the second
/// reminder's (seconds of run time, from the service), the level reached,
/// whether a filter is due, the run time and when it was last changed.
pub const FILTER_THRESHOLD: &str = "filter.threshold";
pub const FILTER_SECOND: &str = "filter.second";
pub const FILTER_LEVEL: &str = "filter.level";
pub const FILTER_NEEDED: &str = "filter.needed";
pub const FILTER_RUNTIME: &str = "filter.runtime";
pub const FILTER_CHANGED: &str = "filter.changed";
/// The replacement threshold stock's bucket holds when the service set
/// none (500 hours).
const FILTER_DEFAULT: u64 = 1_800_000;

/// Sunblock's learning (stock's recovery file), ready and active.
pub const SUNBLOCK_LEARNED: &str = "sunblock.learned";
pub const SUNBLOCK_READY: &str = "sunblock.ready";
pub const SUNBLOCK_ACTIVE: &str = "sunblock.active";
/// When the outdoor temperature was last below -10 °C, seconds since 1970.
pub const COLD_AT: &str = "weather.cold_at";
pub const SAFETY_HIGH: &str = "safety.high";
/// Airwave: whether the fan finishes cooling after the air conditioner.
pub const AIRWAVE: &str = "sense.airwave";
/// Whether Airwave has run (stock's `fan_cooling_ready`), and its status
/// as More info shows it.
pub const AIRWAVE_READY: &str = "airwave.ready";
const AIRWAVE_STATUS: &str = "airwave.status";
/// The fan schedule: on or off, minutes an hour (60: always) and its
/// window's first and last hours of the day.
pub const FAN_SCHEDULE: &str = "fan.schedule";
pub const FAN_DUTY: &str = "fan.duty";
pub const FAN_START: &str = "fan.start";
pub const FAN_END: &str = "fan.end";
/// The minute of the hour the schedule's runs start: when it was set.
const FAN_SINCE: &str = "fan.since";
/// A fan of two or three speeds: the speed the timer runs it at, the
/// schedule's, and heating's and cooling's ("stage1" to "stage3", or
/// "auto" for heating's and cooling's own), as nestlocal's
/// `fan_timer_speed`, `fan_schedule_speed` and `fan_heat_cool_speed`;
/// and the run the timer's page sets before it starts.
pub const FAN_TIMER_SPEED: &str = "fan.timer_speed";
pub const FAN_SCHEDULE_SPEED: &str = "fan.schedule_speed";
pub const FAN_HEAT_COOL_SPEED: &str = "fan.heat_cool_speed";
const FAN_RUN: &str = "fan.run";
/// For the menus' conditions: the fan's speeds ("1" to "3", "0" without
/// a fan), which of the top menu's Fan entries shows ("dial" while the
/// fan runs or has one speed, else its speeds' count; "none" without a
/// fan), and whether the system heats, cools or both ("heat", "cool",
/// "both").
const FAN_SPEEDS: &str = "equipment.fan_speeds";
const FAN_ENTRY: &str = "fan.entry";
const HEATS_COOLS: &str = "equipment.heats_cools";

/// The fan schedule: `duty` minutes an hour (60: always), from the
/// `anchor` minute of each hour, between the `start` and `end` hours of
/// the day (the whole day when they are the same).
#[derive(Clone, Copy, Debug, PartialEq)]
struct FanSchedule {
    duty: u32,
    start: u32,
    end: u32,
    anchor: u32,
}

impl FanSchedule {
    /// Whether the fan runs at `minute` of the day.
    fn runs(&self, minute: u32) -> bool {
        let hour = minute / 60;
        let window = match self.start.cmp(&self.end) {
            std::cmp::Ordering::Equal => true,
            std::cmp::Ordering::Less => (self.start..self.end).contains(&hour),
            std::cmp::Ordering::Greater => hour >= self.start || hour < self.end,
        };
        window && (minute % 60 + 60 - self.anchor) % 60 < self.duty
    }
}

/// The minute of the local day.
fn local_minute(at: std::time::SystemTime) -> u32 {
    let (hour, minute, _) = local_time(at);
    u32::from(hour) * 60 + u32::from(minute)
}
const FAN_ENDS: &str = "fan.ends";


/// How long the menus stay open without input.
const MENU_IDLE: Duration = Duration::from_secs(30);
/// How long the display stays lit without input, by default.
pub const DISPLAY_IDLE: Duration = Duration::from_secs(40);
/// Farsight's far view shows this long before someone near brings its near
/// view, which comes in over `NEAR_TIME` (stock's 1000 ms timer, 0x144bac,
/// and its 500 ms state 5, 0x144950), in seconds.
const FAR_HOLD: f64 = 1.0;
pub const NEAR_TIME: f64 = 0.5;
/// The input that leaves Farsight's clocks or weather fades them out over
/// this long, the analog clock's over `LEAVE_ANALOG` (stock's state 6,
/// 0x1450a0); its temperatures give way to the home screen over it too
/// (state 5), in seconds.
const LEAVE_TIME: f64 = 0.5;
const LEAVE_ANALOG: f64 = 0.75;

/// The weather as nestlocal gives it: the outside temperature (°C), the
/// condition and the sun's rise and set (epoch s).
type Weather = (f32, Option<String>, Option<f64>, Option<f64>);

/// The fan schedule's settings as last seen: mode, duty, start and end.
type ScheduleSeen = (Option<String>, Option<Value>, Option<Value>, Option<Value>);

pub struct Engine {
    pub thermostat: Thermostat,
    pub settings: Settings,
    tree: Tree,
    /// The menus as shown, kept until the tree or a setting used by it changes.
    tree_revision: u64,
    menus_cache: std::cell::RefCell<Option<ShownCache>>,
    nav: Nav,
    start: Instant,
    last_input: Instant,
    /// Until when a change sent from elsewhere keeps the display lit.
    remote_until: Option<Instant>,
    changed_screen: f64,
    changed_target: f64,
    /// When the wheel last moved the target, as against nestlocal.
    turned_target: f64,
    /// The ring's turn towards its next step, -0.5 to 0.5 of one.
    nudge: f32,
    changed_menu: f64,
    /// The display is dark; the next input only lights it.
    asleep: bool,
    /// The press that lit the display: its release does nothing else.
    ignore_release: bool,
    /// In heat-cool, the end of the range the wheel picked on the home
    /// screen, and whether the button has it being changed.
    range: Option<(Side, bool)>,
    display_idle: Duration,
    /// The room's readings come from the backplate, not nestlocal.
    local_sensor: bool,
    /// Heating and cooling, once the backplate measures the room.
    pub hvac: crate::hvac::Hvac,
    /// When the fan timer ends.
    fan_until: Option<Instant>,
    /// Seen by the backplate: motion reports, and the visible light.
    pub motions: u32,
    pub light: u16,
    /// The backplate's power report: Vusb in mV, Vin in 10 mV, the current
    /// in mA, its state.
    pub power: (u16, u16, u16, u8),
    /// When Technical Info's facts were last gathered, and the model
    /// (read once).
    tech_at: Option<Instant>,
    model: Option<String>,
    tech_results: std::sync::Arc<std::sync::Mutex<Option<TechRead>>>,
    tech_pending: bool,
    /// The fan schedule as last seen, and whether the fan ran.
    schedule_seen: Option<ScheduleSeen>,
    fan_was_on: bool,
    /// The alert up, which no input dismisses.
    alert: Option<&'static str>,
    /// A notice up, with CONTINUE: the wiring changed.
    notice: Option<&'static str>,
    /// The second Farsight's analog clock last showed.
    far_second: Option<u8>,
    /// The targets as of the last decision, and whether the schedule set
    /// them since (not a manual change).
    last_targets: Option<(f32, f32, f32)>,
    schedule_set: bool,
    /// Early-On's plan for the next setpoint.
    early: Option<EarlyOn>,
    /// The energy history, and when it last sampled.
    pub history: crate::history::History,
    history_at: Option<Instant>,
    /// The weather nestlocal gave: the outside temperature (°C), the
    /// condition and the sun's times (epoch s).
    weather: Option<Weather>,
    /// When the weather came: the HVAC control uses it for 2 hours.
    weather_at: Option<Instant>,
    /// Waiting for the backplate's first answer: nothing shows yet, as
    /// stock shows no home screen before it.
    starting: bool,
    /// No R wire: the error page shows once any notice is gone, its
    /// TECH INFO highlighted by a turn and opening Technical Info, the
    /// one menu shown over it.
    wiring_error: Option<crate::skin::ErrorPage>,
    /// The error page's choice highlighted, once the wheel turns.
    error_choice: Option<usize>,
    /// The backplate's wires, as MSG_WIRES gives them, and whether they
    /// come from the unit's backplate rather than the simulator.
    wires: Option<[u8; 13]>,
    pub real_backplate: bool,
    /// The circuits with an over-current fault, the backplate's version
    /// (stock's numbering), and since when R has had no power.
    faults: [bool; 13],
    backplate_version: i32,
    unpowered_since: Option<Instant>,
    /// The Heat Link disconnected page's delay at start.
    heat_link_hold: crate::heatlink::Hold,
    /// The unit, whose stock gates Heat Link its own way.
    pub device: Device,
    /// The control's learning, and the time-to-temperature curves, were
    /// taken from the settings.
    learned_loaded: bool,
    ttt_loaded: bool,
    tech_open: bool,
    /// The week's setpoints, and the Schedule screen while it is open.
    pub schedule: crate::schedule::Schedule,
    /// The filter reminder's alert while up: 1 or 2, and the choice.
    filter_alert: Option<(u8, usize)>,
    /// When someone was last seen (motion or input), epoch seconds.
    activity: f64,
    sunblock_comp: bool,
    /// Sunblock, from the first tick with the clock; whether it was on.
    sunblock: Option<crate::sunblock::Sunblock>,
    sunblock_on: bool,
    /// SetTempCompMode to send, when Sunblock changes it.
    pub temp_comp: Option<bool>,
    /// The schedule type last seen, to swap on a change.
    schedule_kind_seen: Option<&'static str>,
    editor: Option<crate::schedule::Editor>,
    /// The wheel's counts moved the schedule since its last steps.
    editor_counted: bool,
    /// The wheel's counts moved History's cards since its last steps.
    energy_counted: bool,
    /// A reset under way: set once Network is done, or All Settings is
    /// about to restart.
    resetting: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// A reset counting down: its action, when the count started, and
    /// the seconds last shown.
    countdown: Option<(&'static str, Instant, u32)>,
    /// The History screen while open.
    energy: Option<crate::history::View>,
    /// connman's Wi-Fi services as last read, the Wi-Fi pages, the network
    /// a passphrase is being typed for (and its hidden security), when the
    /// services were last read and a scan last asked for, and when the
    /// list first showed nothing.
    networks: Vec<crate::wifi::Service>,
    pub wifi: crate::network::Flow,
    /// A Wi-Fi alert up and its choice; when the unit went offline, the
    /// recoverer's failures, limit and next attempt, and the disconnected
    /// alert's last hours.
    pub wifi_alert: Option<(WifiAlert, usize)>,
    /// Stock's HvacShortCycleDueToCompressorLockout (globals+0x18a7),
    /// when it goes by itself (wall seconds), its alert up, and whether
    /// the one kept from before this start was looked at.
    short_cycle: bool,
    short_cycle_until: Option<f64>,
    pub short_cycle_alert: bool,
    short_cycle_loaded: bool,
    /// Stock's battery readings (0xcf934): the last five, in volts, the
    /// next one's place, and when one was last taken; HvacBatteryDrained
    /// (globals flag 0x200) and its alert up.
    battery: [f32; 5],
    battery_next: usize,
    battery_read: Option<Instant>,
    battery_drained: bool,
    /// Wi-Fi off for a low battery (stock's flag 2 at globals+0x1500,
    /// 0xcf934): from under 3.6 V until over 3.8 V.
    battery_low: bool,
    pub battery_alert: bool,
    /// How many lines the HVAC alerts' page is turned down.
    alert_scrolled: usize,
    /// Stock's `is_furnace_shutdown` (globals+0xc24), its alert up, and
    /// whether the detection's data was taken back.
    furnace_shutdown: bool,
    pub furnace_alert: bool,
    furnace_loaded: bool,
    /// The HVAC alert opening Equipment put up while the short-cycle flag
    /// stands (stock's 0x1b007c), and its answer chosen.
    pub equipment_alert: Option<usize>,
    /// The power-off alert was answered: it stays down until R has power
    /// again (stock posts it on a change, 0x23284c); and the wiring report
    /// its press opened.
    power_off_answered: bool,
    report: Option<crate::skin::ErrorPage>,
    /// The wall clock the tests run at, in place of the system's.
    #[cfg(test)]
    wall_at: Option<f64>,
    offline_since: Option<f64>,
    recover: (u32, u32, f64),
    offline_told: u64,
    joining: Option<(String, Option<&'static str>)>,
    wifi_read: Option<Instant>,
    network_results: NetworkResults,
    network_pending: bool,
    wifi_scan: Option<Instant>,
    searching: Option<Instant>,
    /// The keyboard while open, and the setting its text is for.
    keyboard: Option<(crate::keyboard::Keyboard, &'static str)>,
    /// The wheel's counts moved the keyboard since its last steps.
    keyboard_counted: bool,
    /// Plugins' values as their commands last gave them, and when they
    /// were last asked for.
    values: std::sync::Arc<std::sync::Mutex<Option<Values>>>,
    values_at: Option<Instant>,
    /// Location's postal-code entry while open.
    postal: Option<crate::postal::Entry>,
    /// What a menu entry asked for while its keyboard or boxes are open.
    ask: Option<crate::menu::Ask>,
    /// Equipment's PRO SETUP while open, over Equipment Settings.
    pro: Option<crate::prosetup::ProSetup>,
    /// An Equipment Settings page for conventional wiring while open.
    equip: Option<crate::equipment::Page>,
    /// The system test while open (Equipment's and Pro Setup's TEST).
    pub systest: Option<crate::systemtest::SystemTest>,
    /// The week minute last ticked, for applying setpoints as they come.
    week_minute: Option<u32>,
    /// Someone in the room lit the display from afar: stock's Farsight,
    /// until an input.
    far: bool,
    /// Someone came near Farsight's clocks or weather: when their near view
    /// starts coming in, in `now`'s seconds (stock's nlfarsighthandler
    /// state 5, then 3 until an input).
    near: Option<f64>,
    /// The input that left Farsight's clocks or weather, or that or someone
    /// near leaving its temperatures: when they started giving way to the
    /// home screen (stock's state 6, the temperatures' 5), until they have.
    leaving: Option<f64>,
    /// The hourly forecast nestlocal gave, by hour from now (1 to 48): its
    /// time (epoch s) and temperature (°C).
    forecast: Vec<Option<(i64, f32)>>,
    /// When the backplate last saw someone.
    last_presence: Option<Instant>,
    /// Turning the home screen in Eco asks to stop it: whether Stop (or
    /// Cancel) is highlighted.
    eco_prompt: Option<bool>,
    /// "EMERGENCY HEAT IS ON" over the menus, and its choice (0 LEAVE ON,
    /// 1 TURN OFF); and emergency heat as last seen.
    emergency_alert: Option<usize>,
    emergency_seen: Option<bool>,
    /// Stock's nlTurnOffAwayAndOffAlert over Settings, from its Fan tile
    /// pressed while the thermostat is off, and its choice (0 LEAVE OFF,
    /// 1 TURN ON).
    pub off_alert: Option<usize>,
    /// Stock's DateTime alert at start (0x40000128) and its choice (0
    /// DONE, 1 SET DATE/TIME).
    pub date_alert: Option<usize>,
    /// Stock's Switchover alert over the ring turned while off
    /// (nlSwitchoverAlert) and its choice among `switchover_options`.
    pub switchover_alert: Option<usize>,
    /// Stock's Airwave alert, once, when Airwave first runs.
    pub airwave_alert: bool,
    /// The mode the targets were last kept for (with emergency heat).
    mode_seen: Option<String>,
    /// Settings says when the software was updated: an update not yet
    /// seen there as the menus opened.
    software_unseen: bool,
}

impl Engine {
    pub fn new(tree: Tree, defaults: Vec<(String, Value)>, mut settings: Settings) -> Engine {
        for (key, value) in defaults {
            settings.default(&key, value);
        }
        // No Wi-Fi known yet: the system test waits.
        settings.set("test.gate", Value::Text("wifi".into()));
        settings.set("test.blocked", Value::Bool(true));
        let group = zones(settings.text(COUNTRY)).0;
        settings.default(CLOCK_ZONES, Value::Text(group.into()));
        // The time zone Set time last chose.
        if let Some(zone) = settings.text(TIME_ZONE).filter(|_| !cfg!(any(test, feature = "desktop"))) {
            use_time_zone(zone);
        }
        let now = Instant::now();
        Engine {
            thermostat: Thermostat::demo(),
            settings,
            tree,
            tree_revision: 0,
            menus_cache: std::cell::RefCell::new(None),
            nav: Nav::default(),
            start: now,
            last_input: now,
            remote_until: None,
            changed_screen: 0.0,
            changed_target: 0.0,
            turned_target: -1.0,
            nudge: 0.0,
            changed_menu: 0.0,
            asleep: false,
            ignore_release: false,
            range: None,
            display_idle: DISPLAY_IDLE,
            local_sensor: false,
            hvac: crate::hvac::Hvac::default(),
            fan_until: None,
            schedule_seen: None,
            fan_was_on: false,
            motions: 0,
            light: 0,
            power: (0, 0, 0, 0),
            tech_at: None,
            model: None,
            tech_results: Default::default(),
            tech_pending: false,
            alert: None,
            notice: None,
            starting: false,
            far_second: None,
            last_targets: None,
            schedule_set: false,
            early: None,
            history: crate::history::History::default(),
            history_at: None,
            weather: None,
            weather_at: None,
            wiring_error: None,
            error_choice: None,
            wires: None,
            real_backplate: false,
            faults: [false; 13],
            backplate_version: -1,
            unpowered_since: None,
            heat_link_hold: Default::default(),
            device: Device::current(),
            learned_loaded: false,
            ttt_loaded: false,
            tech_open: false,
            schedule: Default::default(),
            filter_alert: None,
            activity: 0.0,
            energy: None,
            resetting: None,
            countdown: None,
            keyboard: None,
            keyboard_counted: false,
            values: Default::default(),
            values_at: None,
            postal: None,
            ask: None,
            pro: None,
            systest: None,
            equip: None,
            networks: Vec::new(),
            wifi: Default::default(),
            wifi_alert: None,
            short_cycle: false,
            short_cycle_until: None,
            short_cycle_alert: false,
            short_cycle_loaded: false,
            battery: [0.0; 5],
            battery_next: 0,
            battery_read: None,
            battery_drained: false,
            battery_low: false,
            battery_alert: false,
            alert_scrolled: 0,
            furnace_shutdown: false,
            furnace_alert: false,
            furnace_loaded: false,
            equipment_alert: None,
            power_off_answered: false,
            report: None,
            #[cfg(test)]
            wall_at: None,
            offline_since: None,
            recover: (0, RECOVER_LIMIT, 0.0),
            offline_told: 0,
            joining: None,
            wifi_read: None,
            network_results: Default::default(),
            network_pending: false,
            wifi_scan: None,
            searching: None,
            sunblock: None,
            sunblock_comp: false,
            sunblock_on: false,
            temp_comp: None,
            schedule_kind_seen: None,
            editor: None,
            editor_counted: false,
            energy_counted: false,
            week_minute: None,
            far: false,
            last_presence: None,
            near: None,
            leaving: None,
            forecast: Vec::new(),
            eco_prompt: None,
            emergency_alert: None,
            emergency_seen: None,
            off_alert: None,
            date_alert: None,
            switchover_alert: None,
            airwave_alert: false,
            mode_seen: None,
            software_unseen: false,
        }
    }

    /// Replaces the menus, after the menus file changed.
    pub fn set_menus(&mut self, tree: Tree, defaults: Vec<(String, Value)>) {
        for (key, value) in defaults {
            self.settings.default(&key, value);
        }
        self.tree = tree;
        self.tree_revision += 1;
        if self.nav.is_open() {
            self.nav.open(&self.menus());
            self.changed_menu = self.now();
        }
    }

    /// The ring's turn towards its next step (a fraction of one): on the
    /// home screen it moves the dial, as the wheel does. True when it shows.
    pub fn set_nudge(&mut self, nudge: f32) -> bool {
        if nudge == self.nudge {
            return false;
        }
        self.nudge = nudge;
        if self.nav.is_open() || self.asleep {
            return false;
        }
        self.turned_target = self.now();
        true
    }

    fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    fn fahrenheit(&self) -> bool {
        self.settings.text(UNITS) == Some("F")
    }

    /// The backplate's wires: the HVAC control's wiring, and a notice when
    /// they differ from those last seen (the first sight is only kept).
    /// True when the screen changed.
    pub fn set_wiring(&mut self, pins: [u8; 15]) -> bool {
        let wiring = crate::hvac::wiring::Wiring { pins, y2: -1, star: -1, dual_fuel: false };
        let model = self.model.get_or_insert_with(crate::techinfo::model);
        let heat_link = heat_link_unit(self.device, model, self.backplate_version);
        let error = match self.wires {
            Some(wires) if heat_link => heat_link_error(&wires, &pins),
            Some(wires) => self.wiring_report_error(&wires, &pins),
            _ => None,
        };
        // The unit's backplate's first report: H71 waits stock's delay.
        let held = heat_link && self.real_backplate && self.heat_link_hold.report(error == Some(NO_HEAT), Instant::now());
        if held {
            debug!("wiring: heat link disconnected page held for {} s", crate::heatlink::DISCONNECTED_DELAY.as_secs());
        }
        let error = error.filter(|_| !held);
        let error_changed = error != self.wiring_error;
        if error_changed {
            log_error_page(error.as_ref());
        }
        self.wiring_error = error;
        self.error_choice = None;
        self.hvac.wiring = Some(wiring);
        let now: String = pins.iter().map(|p| if *p == 0 { '0' } else { '1' }).collect();
        match self.settings.text(WIRING) {
            None => {
                self.settings.set(WIRING, Value::Text(now));
                false
            }
            Some(known) if known == now || self.notice.is_some() => error_changed,
            // A wired Heat Link: its own pages instead of the notice
            // (stock's "Heat Link connected / wired only"); an error page
            // waits behind them.
            Some(known) if error != Some(NO_HEAT) && self.heat_link_connection() == Some(1) => {
                debug!("wiring: {known} -> {now}");
                info!("heatlink: wiring changed; heat link connected, wired only");
                self.remember_wiring();
                self.editor = None;
                self.tech_open = true;
                self.settings.default("equipment.heat_link", Value::Text("on".into()));
                self.pick_saying();
                self.nav.open_entry(&self.menus(), &crate::heatlink::SETUP);
                self.changed_screen = self.now();
                self.changed_menu = self.now();
                true
            }
            Some(known) => {
                debug!("wiring: {known} -> {now}");
                self.notice = Some(match error {
                    Some(NO_HEAT) => CANNOT_CONTROL,
                    Some(e) if e.title == "WIRING REPORT" => UNSUPPORTED,
                    _ => WIRING_CHANGED,
                });
                info!("notice: {}", one_line(self.notice.unwrap_or_default()));
                self.nav.close();
                self.editor = None;
                self.changed_screen = self.now();
                true
            }
        }
    }

    /// Stock's wiring report outside the Heat Link gate (0x2e3eec), its
    /// page for the last code (0x2070b0): with power on R and an error,
    /// that page; without power, the power-off alert follows instead
    /// (`power_tick`).
    fn wiring_report_error(&self, wires: &[u8; 13], pins: &[u8; 15]) -> Option<crate::skin::ErrorPage> {
        self.wiring_report(wires, pins).filter(|(code, _)| *code <= crate::hvac::report::LAST_ERROR).map(|(_, page)| page)
    }

    /// The wiring report's last code and its page, notes included.
    fn wiring_report(&self, wires: &[u8; 13], pins: &[u8; 15]) -> Option<(u8, crate::skin::ErrorPage)> {
        if wires[9] & 1 == 0 && wires[10] & 1 == 0 {
            return None;
        }
        self.report_page(wires, pins)
    }

    /// The wiring report's page for its last code, with R powered or not.
    fn report_page(&self, wires: &[u8; 13], pins: &[u8; 15]) -> Option<(u8, crate::skin::ErrorPage)> {
        use crate::hvac::report;
        let code = *self.report_codes(wires, pins).last()?;
        let entry = report::entry(code, self.backplate_version);
        let (title, choices): (_, &'static [&'static str]) = match code {
            46 => ("NO POWER", &["TECH INFO"]),
            c if c > report::LAST_ERROR => ("WIRING REPORT", &["TECH INFO", "CONTINUE"]),
            _ => ("WIRING REPORT", &["TECH INFO"]),
        };
        Some((code, crate::skin::ErrorPage { code: entry.label, title, text: entry.text, link: entry.link, choices }))
    }

    /// The wiring report's codes (0x2e3eec), a compressor lockout's 46
    /// included.
    fn report_codes(&self, wires: &[u8; 13], pins: &[u8; 15]) -> Vec<u8> {
        use crate::hvac::report;
        let e = &self.hvac.equipment;
        let w = crate::hvac::wiring::Wiring { pins: *pins, y2: e.y2, star: e.star, dual_fuel: e.dual_fuel };
        let mut codes = report::codes(&w, wires, &self.faults);
        // SAFETY: time with a null pointer only returns the time.
        let now = unsafe { libc::time(std::ptr::null_mut()) } as f64;
        let cold = matches!(self.settings.get(COLD_AT), Some(&Value::Number(at)) if now - at < 6.0 * 3600.0);
        report::lockout_power(&mut codes, pins, self.backplate_version, cold);
        codes
    }

    /// The circuits with an over-current fault changed: the wiring report
    /// follows.
    pub fn set_faults(&mut self, faults: [bool; 13]) -> bool {
        self.faults = faults;
        match (self.hvac.wiring.as_ref().map(|w| w.pins), self.wires) {
            (Some(pins), Some(wires)) if !self.heat_link() => {
                let error = self.wiring_report_error(&wires, &pins);
                let changed = error != self.wiring_error;
                // A new fault: stock's "cannot control" alert first.
                if changed {
                    log_error_page(error.as_ref());
                }
                if changed && error.is_some() && self.notice.is_none() {
                    info!("notice: {}", one_line(UNSUPPORTED));
                    self.notice = Some(UNSUPPORTED);
                    self.nav.close();
                    self.changed_screen = self.now();
                }
                self.wiring_error = error;
                changed
            }
            _ => false,
        }
    }

    /// Each mode keeps its own setpoint, as stock's last heat, cool, range
    /// and emergency setpoints: leaving a mode keeps its target, coming
    /// back gives it again (seen on stock: heat 21, cool 24, back to 21).
    pub fn setpoint_tick(&mut self) {
        if let Some(on) = self.settings.text(MODE).filter(|m| *m != "off" && self.settings.text(MODE_ON) != Some(m)).map(str::to_string) {
            self.settings.set(MODE_ON, Value::Text(on));
        }
        let mode = match self.settings.text(MODE).unwrap_or("heat") {
            "heat" if self.settings.text(EMERGENCY) == Some("on") && self.hvac.equipment.emergency_heat => "emergency".to_string(),
            m => m.to_string(),
        };
        let kind = self.schedule_kind();
        if let Some(old) = self.schedule_kind_seen.replace(kind).filter(|old| *old != kind) {
            self.swap_schedule(old, kind);
        }
        let Some(old) = self.mode_seen.replace(mode.clone()).filter(|old| *old != mode) else { return };
        debug!("setpoints: {old} kept, {mode}'s given back");
        let t = &self.thermostat;
        let kept: Vec<(String, f32)> = match old.as_str() {
            "heat-cool" => vec![("setpoint.heat-cool.low".into(), t.low), ("setpoint.heat-cool.high".into(), t.high)],
            "off" => vec![],
            m => vec![(format!("setpoint.{m}"), t.target)],
        };
        for (key, v) in kept {
            self.settings.set(&key, Value::Number(f64::from(v)));
        }
        let get = |key: &str| match self.settings.get(key) { Some(&Value::Number(n)) => Some(n as f32), _ => None };
        match mode.as_str() {
            "heat-cool" => {
                if let (Some(l), Some(h)) = (get("setpoint.heat-cool.low"), get("setpoint.heat-cool.high")) {
                    (self.thermostat.low, self.thermostat.high) = (l, h);
                }
            }
            "off" => {}
            m => {
                if let Some(t) = get(&format!("setpoint.{m}")) {
                    self.thermostat.target = t;
                }
            }
        }
        self.changed_target = self.now();
    }

    /// Emergency heat coming on or going (stock's mode setter, 0x11500c):
    /// on, Auto-Schedule and Home/Away Assist are kept aside and turned
    /// off; off, each comes back unless turned on since. Equipment
    /// Settings offers it with the equipment for it.
    fn emergency_tick(&mut self) {
        let has = self.hvac.equipment.emergency_heat;
        if self.settings.get(HAS_EMERGENCY) != Some(&Value::Bool(has)) {
            self.settings.set(HAS_EMERGENCY, Value::Bool(has));
        }
        let on = self.settings.text(EMERGENCY) == Some("on");
        match self.emergency_seen.replace(on) {
            Some(false) if on => {
                info!("emergency heat: on; auto-schedule and home/away assist set aside");
                for (key, pre) in PRE_EMERGENCY {
                    let was = self.settings.text(key).unwrap_or("on").to_string();
                    self.settings.set(pre, Value::Text(was));
                    self.settings.set(key, Value::Text("off".into()));
                }
            }
            Some(true) if !on => {
                info!("emergency heat: off; auto-schedule and home/away assist back as they were");
                for (key, pre) in PRE_EMERGENCY {
                    if self.settings.text(key) == Some("off") {
                        if let Some(was) = self.settings.text(pre).map(str::to_string) {
                            self.settings.set(key, Value::Text(was));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Publishes what the wiring has for the menus' conditions; all of it
    /// until the wiring is known.
    fn equipment_tick(&mut self) {
        use crate::hvac::wiring::Switch;
        let has = |s: Switch| self.hvac.wiring.as_ref().is_none_or(|w| w.exists(s));
        let (fan, cooling) = (has(Switch::Fan), has(Switch::Stage1Cool));
        let radiant = self.settings.text(HEAT_CONTROL) == Some("opentherm");
        let wired = |s: Switch| self.hvac.wiring.as_ref().is_some_and(|w| w.exists(s));
        // Cool to Dry (stock's 0x162120): a cool stage and no dehumidifier,
        // or a dehumidifier that is not stand-alone (vt+0x280,
        // dehumidifier_type 1).
        let dehumidifier = wired(Switch::Dehumidifier);
        let dry = if dehumidifier { self.hvac.equipment.humidity.dehumidifier != 1 } else { cooling };
        // Heat Pump Balance (vt+0x26c): a heat pump with auxiliary heat, no
        // dual fuel, not beside True Radiant.
        let balance = !radiant && wired(Switch::Ob) && wired(Switch::AuxHeat) && !wired(Switch::Stage1AltHeat);
        // Reminders (0xc519c): a wired stage, the water heater aside, that
        // moves air (its delivery, vt+0x31c, neither 1 nor 2).
        let filter = Switch::ALL.iter().any(|&s| s != Switch::WaterHeater && has(s)
            && !matches!(self.hvac.equipment.delivery[s as usize], 1 | 2));
        for (key, on) in [(HAS_FAN, fan), (HAS_COOLING, cooling), (HAS_AIRWAVE, fan && cooling), (HAS_EARLY_ON, cooling || !radiant),
                          (HAS_DRY, dry), (HAS_BALANCE, balance), (HAS_FILTER, filter)] {
            self.settings.default(key, Value::Bool(on));
        }
        // The fan's speeds as the settings keep them.
        let stage = |s: &Settings, key: &str| s.text(key).and_then(|v| v.strip_prefix("stage")?.parse::<u8>().ok()).filter(|n| (1..=3).contains(n));
        let eq = &mut self.hvac.equipment;
        eq.fan_timer_speed = stage(&self.settings, FAN_TIMER_SPEED).unwrap_or(1);
        eq.fan_schedule_speed = stage(&self.settings, FAN_SCHEDULE_SPEED).unwrap_or(1);
        eq.fan_heat_cool_speed = stage(&self.settings, FAN_HEAT_COOL_SPEED);
        let speeds = if fan { eq.fan_stages.clamp(1, 3) } else { 0 };
        let entry = match speeds {
            0 => "none".to_string(),
            1 => "dial".to_string(),
            _ if self.fan_running() => "dial".to_string(),
            n => n.to_string(),
        };
        // The running fan's page gives way to the timer's page of speeds
        // once the run ends: the menus close.
        if self.settings.text(FAN_ENTRY) == Some("dial") && entry != "dial" && self.nav.editing_key(&self.menus()) == Some(FAN_TIMER) {
            debug!("fan: the run ended on its page; the menus close");
            self.nav.close();
            self.changed_screen = self.now();
        }
        let both = match self.heats_cools() {
            (true, false) => "heat",
            (false, true) => "cool",
            _ => "both",
        };
        self.settings.default(FAN_SPEEDS, Value::Text(speeds.to_string()));
        self.settings.default(FAN_ENTRY, Value::Text(entry));
        self.settings.default(HEATS_COOLS, Value::Text(both.into()));
        // Equipment Settings' entries, as stock's equipmentlayer offers
        // them (g2 0xbb9c0): HEAT TYPE with conventional heat, HEAT PUMP
        // with O/B, DUAL FUEL with alternate heat, HUMIDIFIER (else
        // DEHUMIDIFIER) with either, and the STAR wire's type with it wired;
        // none on a Heat Link unit, whose list is its own.
        let conventional = !self.model.as_deref().is_some_and(|m| heat_link_unit(self.device, m, self.backplate_version));
        let star = self.hvac.wiring.as_ref().is_some_and(|w| w.pins[STAR_PIN] == 1);
        let humidifier = has(Switch::Humidifier);
        for (key, on) in [(HAS_HEAT_TYPE, has(Switch::Stage1Heat)), (HAS_HEAT_PUMP, has(Switch::Ob)), (HAS_DUAL_FUEL, has(Switch::Stage1AltHeat)),
                          (HAS_HUMIDIFIER, humidifier), (HAS_DEHUMIDIFIER, has(Switch::Dehumidifier) && !humidifier), (HAS_STAR, star)] {
            self.settings.default(key, Value::Bool(on && conventional));
        }
        // Heat Pump Balance's level, as heatpump_savings names it.
        if let Some(level) = self.settings.text(BALANCE_LEVEL).and_then(|l| BALANCE_LEVELS.iter().position(|&b| b == l)) {
            self.hvac.equipment.savings = level as u8;
        }
        // Its More info page's status (0x148e84): off, or ready once the
        // outdoor fit has runs to go on (vt+0x258), else learning.
        let o = &self.hvac.balance.outdoor;
        let status = if self.hvac.equipment.savings == 3 { "Off" } else if o.mx.is_some() && o.vx > 0.0 { "Ready" } else { "Learning" };
        self.settings.default("balance.status", Value::Text(status.into()));
        // What the system runs, for Eco's and Safety Temp's pages.
        let system = match self.heats_cools() {
            (true, true) => "both",
            (true, false) => "heat",
            (false, true) => "cool",
            (false, false) => "none",
        };
        self.settings.default(SYSTEM, Value::Text(system.into()));
        self.equipment_settings();
    }

    /// What Equipment Settings' pages set, given to the HVAC control (as
    /// the device bucket's fields would give it).
    fn equipment_settings(&mut self) {
        use crate::equipment as q;
        let s = &self.settings;
        let e = &mut self.hvac.equipment;
        let number = |key: &str| match s.get(key) {
            Some(&Value::Number(n)) => Some(Some(n as f32)),
            Some(Value::Text(t)) if t == "off" => Some(None),
            _ => None,
        };
        if let Some(o) = s.text(q::OB) {
            e.ob_cool = o != "B";
        }
        if let Some(v) = number(q::COMPRESSOR) {
            e.compressor_lockout = v;
        }
        if let Some(v) = number(q::AUX) {
            e.aux_lockout = v;
        }
        if let Some(Some(v)) = number(q::BREAKPOINT) {
            e.dual_fuel_breakpoint = v;
        }
        if let Some(o) = s.text(q::OVERRIDE) {
            e.dual_fuel_override = match o.to_ascii_uppercase().as_str() { "ALWAYS-ALT" => 1, "ALWAYS-PRIMARY" => 2, _ => 0 };
        }
        if let Some(v) = number(q::HUMIDITY) {
            e.humidity.target = v;
        }
        if let Some(on) = s.text(q::QUIET) {
            let hour = |k: &str| number(k).flatten().map(|h| (h as u32 % 24) * 3600);
            e.humidity.quiet = (on == "on").then(|| hour(q::QUIET_START).zip(hour(q::QUIET_END))).flatten();
        }
        if let Some(t) = s.text(q::STAR) {
            // The device bucket's names as net.rs reads them.
            e.star = match t.to_ascii_lowercase().as_str() {
                "emer-heat" | "emergency heat" => 0, "hum" | "humidifier" => 1, "dehum" | "dehumidifier" => 2, "w3" => 3, "waterheater" => 4,
                "g3" => 5, "ob" => 6, "y2" => 7, "w2" => 8, _ => -1,
            };
        }
        if let Some(t) = s.text(q::Y2) {
            e.y2 = match t.to_ascii_lowercase().as_str() { "heatingandcooling" => 0, "heating" => 1, "cooling" => 2, "g2" => 3, _ => -1 };
        }
        if let Some(on) = s.text(q::DUAL_FUEL) {
            e.dual_fuel = on == "on";
        }
        // The furnace's delivery and source (HEAT TYPE, Pro Setup's W1).
        let furnace = crate::hvac::wiring::Switch::Stage1Heat as usize;
        if let Some(d) = s.text(HEAT_DELIVERY) {
            e.delivery[furnace] = match d { "forced-air" => 0, "in-floor-radiant" => 1, "radiator" => 2, "electric-strip" => 3, _ => -1 };
        }
        if let Some(f) = s.text(HEAT_SOURCE) {
            e.source[furnace] = match f { "gas" => 0, "electric" => 1, "oil" => 2, "lp" | "propane" => 3, "geothermal" => 4, "pellets" => 5, _ => -1 };
        }
        if let Some(t) = s.text(q::DEHUMIDIFIER_TYPE).filter(|_| e.star == 2) {
            e.humidity.dehumidifier = if t.eq_ignore_ascii_case("STAND-ALONE") { 1 } else { 2 };
        }
    }

    /// The terminals Pro Setup lists, by stock's names and MSG_PINS' bytes,
    /// in its order.
    fn wired_terminals(&self) -> Vec<&'static str> {
        let Some(w) = self.hvac.wiring.as_ref() else { return Vec::new() };
        [("W1", 0), ("Y1", 1), ("G", 5), ("O/B", 6), ("Rc", 3), ("Rh", 4), ("C", 2), ("W2", 7), ("Y2", 9), ("STAR", STAR_PIN)]
            .into_iter().filter(|&(_, i)| w.pins[i] == 1).map(|(name, _)| name).collect()
    }

    /// The wiring is a heat pump's (O/B wired), as the HVAC control's
    /// equipment has it once it runs.
    fn heat_pump(&self) -> bool {
        self.hvac.wiring.as_ref().map_or(self.hvac.equipment.heat_pump, |w| w.exists(crate::hvac::wiring::Switch::Ob))
    }

    /// The backplate's own safety temperatures (stock's 0x305c38): only
    /// outside the Heat Link gate, on the unit's backplate, with R powered
    /// or equipment wired; none while a wiring error stands.
    pub fn backplate_safety(&self) -> Option<[u8; 16]> {
        let model = self.model.as_deref()?;
        let wires = self.wires?;
        let w = self.hvac.wiring.as_ref()?;
        let e = &self.hvac.equipment;
        let w = crate::hvac::wiring::Wiring { y2: e.y2, star: e.star, dual_fuel: e.dual_fuel, ..w.clone() };
        let powered = wires[9] & 1 != 0 || wires[10] & 1 != 0;
        let equipped = crate::hvac::wiring::Switch::ALL.iter().any(|&s| w.exists(s));
        if heat_link_unit(self.device, model, self.backplate_version) || !self.real_backplate || !(powered || equipped) {
            return None;
        }
        // A wiring error clears the temperatures and the lockout.
        let (low, high, lockout) = match self.wiring_error.filter(|p| !p.code.starts_with('N')) {
            Some(_) => (None, None, 0),
            None => { let (l, h) = self.temperatures(SAFETY_LOW, SAFETY_HIGH); (l, h, e.backplate_lockout) }
        };
        // The fan with heat as the chosen heat switch needs it (stock's
        // globals+0x1204, its fan type).
        Some(w.safety_payload(low, high, e.has_fan && e.heat_fan(crate::hvac::wiring::Switch::Stage1Heat), e.ob_cool, lockout))
    }

    /// Opening Equipment with only notes: the last note's page first
    /// (stock's report layer, mode 2), CONTINUE to the menu.
    fn equipment_notes(&mut self, now: f64) {
        if self.wiring_error.is_some() {
            return;
        }
        let note = match (self.wires, self.hvac.wiring.as_ref()) {
            (Some(wires), Some(w)) if !self.heat_link() => self.wiring_report(&wires, &w.pins),
            _ => None,
        };
        if let Some((_, page)) = note.filter(|(c, _)| *c > crate::hvac::report::LAST_ERROR) {
            self.wiring_error = Some(page);
            self.error_choice = None;
            self.changed_screen = now;
        }
    }

    /// The power-off alert answered: down until R has power again, and
    /// Equipment's wiring report open at its first page, the report's last
    /// code (stock's wirereportlayer reset, 0x208efc), R's own codes
    /// included. It only shows: the HVAC control keeps to `wiring_error`.
    fn open_report(&mut self, now: f64) {
        info!("alert: power off answered; the wiring report");
        self.set_alert(None);
        self.power_off_answered = true;
        self.report = match (self.wires, self.hvac.wiring.as_ref()) {
            (Some(wires), Some(w)) => self.report_page(&wires, &w.pins).map(|(_, page)| page),
            _ => None,
        };
        log_error_page(self.report.as_ref());
        self.error_choice = None;
        self.editor = None;
        self.nav.open_entry(&self.menus(), &["Settings", "Equipment"]);
        self.changed_screen = now;
        self.changed_menu = now;
    }

    /// Outside the Heat Link gate, R without power for 150 s raises
    /// stock's power-off alert, gone once R has power: the compressor
    /// lockout's when the wiring report ends on code 46 (0x2320cc).
    fn power_tick(&mut self) -> bool {
        let unpowered = self.real_backplate && !self.heat_link()
            && self.wires.is_some_and(|w| w[9] & 1 == 0 && w[10] & 1 == 0);
        let shown = matches!(self.alert, Some(POWER_OFF | POWER_OFF_LOCKOUT));
        if !unpowered {
            self.unpowered_since = None;
            self.power_off_answered = false;
            // The report the alert opened goes with it; the wiring's own
            // page, if any, follows.
            let report = self.report.take().is_some();
            if report {
                self.changed_screen = self.now();
            }
            return (shown && self.set_alert(None)) || report;
        }
        if self.power_off_answered {
            return false;
        }
        let since = *self.unpowered_since.get_or_insert_with(Instant::now);
        let lockout = match (self.wires, self.hvac.wiring.as_ref()) {
            (Some(wires), Some(w)) => self.report_codes(&wires, &w.pins).last() == Some(&46),
            _ => false,
        };
        let alert = if lockout { POWER_OFF_LOCKOUT } else { POWER_OFF };
        since.elapsed() >= POWER_OFF_DELAY && (self.alert.is_none() || shown) && self.set_alert(Some(alert))
    }

    /// The Heat Link disconnected page's delay ran out: the page as the
    /// wiring now gives it. True when that changed the screen.
    fn heat_link_tick(&mut self) -> bool {
        if !self.heat_link_hold.expired(Instant::now()) {
            return false;
        }
        let (Some(wires), Some(w)) = (self.wires, self.hvac.wiring.as_ref()) else { return false };
        let error = heat_link_error(&wires, &w.pins);
        if error == self.wiring_error {
            return false;
        }
        log_error_page(error.as_ref());
        self.wiring_error = error;
        self.error_choice = None;
        self.changed_screen = self.now();
        true
    }

    /// Stock's HvacShortCycleDueToCompressorLockout (0xbea00), from the
    /// cloud or taken back at start: not set in cool mode; set by the
    /// cloud, it holds 48 h from then, kept across restarts; its alert
    /// comes with it and goes with it. True when that changed the screen.
    pub fn set_short_cycle(&mut self, on: bool, cloud: bool) -> bool {
        if on && self.settings.text(MODE) == Some("cool") {
            debug!("hvac: short cycle due to compressor lockout ignored in cool mode");
            return false;
        }
        if on == self.short_cycle {
            return false;
        }
        self.short_cycle = on;
        let now = self.wall();
        if !on || cloud {
            self.settings.set(SHORT_CYCLE_SINCE, Value::Number(if on { now } else { 0.0 }));
        }
        let since = self.short_cycle_since();
        self.short_cycle_until = (on && since > 0.0 && since <= now && now - since < SHORT_CYCLE_HOLD).then_some(since + SHORT_CYCLE_HOLD);
        info!("hvac: short cycle due to compressor lockout {}{}", if on { "set" } else { "cleared" }, if cloud { " by the cloud" } else { "" });
        if on {
            info!("alert: {SHORT_CYCLE_TITLE}: {SHORT_CYCLE_TEXT} nest.com/m13");
        } else if self.short_cycle_alert {
            info!("alert: short cycle taken down");
        }
        self.short_cycle_alert = on;
        self.settings.default(EQUIPMENT_NOTICE, Value::Text(if on { EQUIPMENT_ISSUE } else { "" }.into()));
        self.changed_menu = self.now();
        // The top menu's Settings says so under its name.
        self.settings_notice();
        self.read_networks();
        self.changed_screen = self.now();
        true
    }

    fn short_cycle_since(&self) -> f64 {
        match self.settings.get(SHORT_CYCLE_SINCE) {
            Some(Value::Number(n)) => *n,
            _ => 0.0,
        }
    }

    /// The short-cycle flag as time passes: taken back at start while the
    /// cloud's 48 h last, with power stealing off while heating (stock's
    /// 0x233c5c); gone at their end, power stealing back to normal
    /// (0x233d0c). The detection follows the heating, cooling and HVAC
    /// power. True when that changed the screen.
    fn short_cycle_tick(&mut self) -> bool {
        let now = self.wall();
        let powered = self.wires.is_none_or(|w| w[9] & 1 != 0 || w[10] & 1 != 0);
        // Detection waits for a clock that can be right (stock's 0x3b0cd8).
        if time_valid(now) {
            self.hvac.short_cycle_update(now, powered);
        }
        if !self.short_cycle_loaded {
            self.short_cycle_loaded = true;
            let since = self.short_cycle_since();
            if since > 0.0 && since < now && now - since < SHORT_CYCLE_HOLD {
                debug!("hvac: short cycle detection from {:.0} s ago kept; power stealing off while heating", now - since);
                self.hvac.equipment.power_steal = 2;
                return self.set_short_cycle(true, false);
            }
        }
        if self.short_cycle_until.is_some_and(|at| now >= at) {
            info!("hvac: heating short cycle detection timed out");
            self.hvac.equipment.power_steal = 0;
            return self.set_short_cycle(false, false);
        }
        false
    }

    /// Furnace shutdown detection as time passes, with the heating and the
    /// HVAC power: its data taken back at start, the flag with it (stock's
    /// 0x3b044c, no alert then); the alert as the detection raises it and
    /// takes it down. True when that changed the screen.
    fn furnace_tick(&mut self) -> bool {
        let now = self.wall();
        if !self.furnace_loaded {
            self.furnace_loaded = true;
            if let Some(text) = self.settings.text(FURNACE) {
                self.hvac.furnace.load(text);
            }
            self.furnace_shutdown = self.hvac.furnace.start(now);
            if self.furnace_shutdown {
                info!("hvac: furnace shutdown kept from before this start");
            }
        }
        // Detection waits for a clock that can be right (stock's 0x3b0728).
        if !time_valid(now) {
            return false;
        }
        let powered = self.wires.is_none_or(|w| w[9] & 1 != 0 || w[10] & 1 != 0);
        let change = self.hvac.furnace_update(now, powered);
        let data = self.hvac.furnace.save();
        if self.settings.text(FURNACE) != Some(data.as_str()) {
            self.settings.set(FURNACE, Value::Text(data));
        }
        let Some(on) = change else { return false };
        self.furnace_flag(on);
        true
    }

    /// The detection raised or took down its flag, and its alert with it.
    fn furnace_flag(&mut self, on: bool) {
        self.furnace_shutdown = on;
        if on {
            info!("alert: {FURNACE_SHUTDOWN_TEXT} nest.com/furnace");
            // The detection's flag also offers System Test's Furnace
            // Heads-Up, once (stock's 0xbdd60, `show_furnace_headsup_test`).
            if self.settings.get(FURNACE_HEADSUP_TEST) != Some(&Value::Bool(true)) {
                self.settings.set(FURNACE_HEADSUP_TEST, Value::Bool(true));
            }
        } else if self.furnace_alert {
            info!("alert: furnace shutdown taken down");
        }
        self.furnace_alert = on;
        self.changed_screen = self.now();
    }

    /// The unit woke: the battery check runs on the next tick, as stock's
    /// runs on every wake (0xa237c, event 0x40000115).
    pub fn battery_due(&mut self) {
        self.battery_read = None;
    }

    /// Stock's battery check (0xcf934), once a minute, `read` giving the
    /// battery in volts (0 when there is none to read): the highest of the
    /// last five readings. HvacBatteryDrained rises while power stealing
    /// is off while heating, the short-cycle flag stands and that is under
    /// 3.55 V; it goes with power stealing back to normal or over 3.65 V.
    /// Its alert comes with it and goes with it. True when that changed
    /// the screen.
    pub fn battery_tick(&mut self, read: impl Fn() -> f32) -> bool {
        if self.battery_read.is_some_and(|at| at.elapsed() < BATTERY_EVERY) {
            return false;
        }
        self.battery_read = Some(Instant::now());
        let volts = read();
        if volts <= 0.0 {
            return false;
        }
        self.battery[self.battery_next] = volts;
        self.battery_next = (self.battery_next + 1) % self.battery.len();
        let level = self.battery.iter().copied().fold(0.0, f32::max);
        if !(3.6..=3.8).contains(&level) {
            self.battery_low = level < 3.6;
        }
        self.weather_offline();
        let steal = self.hvac.equipment.power_steal;
        let drained = if steal == 2 && self.short_cycle && level < 3.55 { true }
                      else if steal == 0 || level > 3.65 { false }
                      else { self.battery_drained };
        if drained == self.battery_drained {
            return false;
        }
        self.battery_drained = drained;
        // It also holds the HVAC control to drift while power stealing is off
        // for what it would switch (stock's ThermostatManager +0x3b9).
        self.hvac.battery_drained = drained;
        if drained {
            info!("hvac: battery drained at {level:.2} V with power stealing off while heating");
            info!("alert: {BATTERY_DRAINED_TITLE}: {BATTERY_DRAINED_TEXT} nest.com/m13");
        } else {
            info!("hvac: battery drained cleared at {level:.2} V");
            if self.battery_alert {
                info!("alert: battery drained taken down");
            }
        }
        self.battery_alert = drained;
        self.changed_screen = self.now();
        true
    }

    /// Why Display › Weather can't show offline (stock's 0x1a4c44): Wi-Fi
    /// is off for a low battery, else it needs connecting. Stock's third
    /// reason, an unpaired unit on Google's service (vt+0x504, 0xc5630
    /// "Phoenix"), has no service here.
    fn weather_offline(&mut self) {
        let why = if self.battery_low {
            "Wi-Fi is needed to show\nweather. Can't connect to\nWi-Fi because the battery\nis low."
        } else {
            "Connect to Wi-Fi to\nshow the weather."
        };
        self.settings.default(WEATHER_OFFLINE, Value::Text(crate::i18n::tr(why)));
    }

    /// Whether the Heat Link wiring checks apply, the model known.
    fn heat_link(&self) -> bool {
        self.model.as_deref().is_some_and(|m| heat_link_unit(self.device, m, self.backplate_version))
    }

    /// The unit's model (nlmodel), when known before Technical Info reads it.
    pub fn set_model(&mut self, model: &str) {
        self.model = Some(model.to_string());
    }

    /// Stock's `heat_link_connection` (`0x307bb8`) on a Heat Link unit: 1,
    /// wired, once the unit's backplate reports any wire, else 0; the
    /// wireless side (2) is not there.
    pub fn heat_link_connection(&self) -> Option<i64> {
        let model = self.model.as_deref()?;
        heat_link_unit(self.device, model, self.backplate_version).then(|| i64::from(self.real_backplate && self.wires.is_some_and(|w| w.iter().any(|&b| b & 1 != 0))))
    }

    /// The backplate's wires, ahead of its pins.
    pub fn set_wires(&mut self, wires: [u8; 13]) {
        self.wires = Some(wires);
    }

    fn remember_wiring(&mut self) {
        if let Some(w) = &self.hvac.wiring {
            let now: String = w.pins.iter().map(|p| if *p == 0 { '0' } else { '1' }).collect();
            self.settings.set(WIRING, Value::Text(now));
        }
    }

    /// The weather nestlocal gave; true when that changes the screen.
    pub fn set_weather(&mut self, temp: Option<f32>, icon: Option<&str>, sunrise: Option<f64>, sunset: Option<f64>) -> bool {
        self.weather_at = temp.map(|_| Instant::now());
        // Below -10 °C outdoors: kept for the wiring report's code 46.
        if temp.is_some_and(|t| t < -10.0) {
            // SAFETY: time with a null pointer only returns the time.
            let now = unsafe { libc::time(std::ptr::null_mut()) } as f64;
            self.settings.set(COLD_AT, Value::Number(now));
        }
        let new = temp.map(|t| (t, icon.map(str::to_string), sunrise, sunset));
        let changed = new != self.weather;
        self.weather = new;
        self.test_gate();
        changed
    }

    /// Where the weather's answer says it is; true when that changes.
    pub fn set_place(&mut self, city: Option<&str>, state: Option<&str>, zip: Option<&str>) -> bool {
        let Some(city) = city else { return false };
        let place = [Some(city), state, zip].into_iter().flatten().collect::<Vec<_>>().join("\n");
        if self.settings.text(PLACE) == Some(place.as_str()) {
            return false;
        }
        self.settings.set(PLACE, Value::Text(place));
        let named = [Some(city), state].into_iter().flatten().collect::<Vec<_>>().join(", ");
        self.settings.set(PLACE_ASKED, Value::Text([Some(named.as_str()), zip].into_iter().flatten().collect::<Vec<_>>().join("\n")));
        self.settings.set(PLACE_KNOWN, Value::Bool(true));
        true
    }

    /// Where to ask the weather for: the location set, as
    /// "<postal code>,<country>".
    pub fn weather_place(&self) -> Option<String> {
        Some(format!("{},{}", self.settings.text(POSTAL).filter(|p| !p.is_empty())?, self.settings.text(COUNTRY)?))
    }

    /// Stock's picture for the weather (one of the unit's Weather_*.png
    /// sheets): the condition as stock's weather parser groups it
    /// (0x39d388), by day or night when it has both.
    fn weather_image(&self) -> Option<&'static str> {
        let (_, icon, sunrise, sunset) = self.weather.as_ref()?;
        // SAFETY: time with a null pointer only returns the time.
        let now = unsafe { libc::time(std::ptr::null_mut()) } as f64;
        let day = match (sunrise, sunset) {
            (Some(rise), Some(set)) => *rise < now && now < *set,
            _ => true,
        };
        Some(match icon.as_deref() {
            Some("sunny" | "clear") => if day { "Weather_ClearDay.png" } else { "Weather_ClearNight.png" },
            Some("mostlysunny" | "partlycloudy" | "partlysunny") => if day { "Weather_PartlyCloudyDay.png" } else { "Weather_PartlyCloudyNight.png" },
            Some("chanceflurries" | "chancerain" | "chancesleet" | "chancesnow" | "chancetstorms" | "cloudy" | "fog" | "hazy" | "mostlycloudy") => "Weather_Cloudy.png",
            Some("rain" | "sleet" | "tstorms") => "Weather_Rain.png",
            Some("flurries" | "snow") => "Weather_Snow.png",
            _ => "Weather_Default.png",
        })
    }

    /// Minutes until the room reaches the target, as stock's model gives
    /// them (only for a run a manual change started).
    fn time_to_target(&self) -> Option<u32> {
        let seconds = self.hvac.time_to_target(self.hvac_mode(), self.thermostat.current)?;
        Some((seconds / 60.0).floor() as u32)
    }

    /// The same, in seconds, for the bucket's `time_to_target`.
    pub fn time_to_target_seconds(&self) -> Option<f64> {
        self.hvac.time_to_target(self.hvac_mode(), self.thermostat.current)
    }

    /// Whether the screen waits for the backplate's first answer.
    pub fn set_starting(&mut self, starting: bool) -> bool {
        let changed = self.starting != starting;
        self.starting = starting;
        if changed {
            self.last_input = Instant::now();
        self.seen();
            self.changed_screen = self.now();
        }
        changed
    }

    /// Puts up an alert, or takes it down; true when that changed it.
    pub fn alert(&self) -> Option<&'static str> {
        self.alert
    }

    /// Shows the connections' alert, or takes down one of theirs.
    /// Held off, as stock's 0x232278, in the Equipment menus and at a
    /// battery of 3.7 V or less (0 when there is none to read).
    pub fn connections_alert(&mut self, c: &Connections, battery: f32) -> bool {
        let held = self.nav.within(&self.tree, "Equipment") || (battery > 0.0 && battery <= 3.7);
        match c.alert().filter(|_| !held) {
            Some(a) => self.set_alert(Some(a)),
            None if matches!(self.alert, Some(REATTACH | ATTACH)) => self.set_alert(None),
            None => false,
        }
    }

    pub fn set_alert(&mut self, alert: Option<&'static str>) -> bool {
        self.starting = false;
        if self.alert == alert {
            return false;
        }
        match alert {
            Some(a) => info!("alert: {}", one_line(a)),
            None => info!("alert: taken down"),
        }
        self.alert = alert;
        self.nav.close();
        // An alert lights the display for its own idle time, as stock's do.
        if alert.is_some() {
            self.asleep = false;
            self.last_input = Instant::now();
        }
        self.changed_screen = self.now();
        true
    }

    /// The backplate saw someone: a dark display lights from afar, and a
    /// lit one stays lit (stock, on nest-qemu's presence).
    pub fn presence(&mut self) -> bool {
        self.seen();
        let wake = self.settings.text(FARSIGHT).unwrap_or("target_temp");
        if wake == "press" {
            return false;
        }
        self.last_presence = Some(Instant::now());
        if !self.asleep {
            return false;
        }
        debug!("display: someone in the room lights it ({wake})");
        self.asleep = false;
        self.far = wake != "approach";
        self.changed_screen = self.now();
        true
    }

    /// What a change sent from elsewhere (MQTT, Zigbee, Matter, the app,
    /// through nestlocal) would show: the targets, the mode, Eco and the
    /// fan's timer.
    pub fn remote_view(&self) -> (u32, u32, u32, Option<Value>, Option<Value>, Option<Value>) {
        let t = &self.thermostat;
        let get = |key: &str| self.settings.get(key).cloned();
        (t.target.to_bits(), t.low.to_bits(), t.high.to_bits(), get(MODE), get(ECO), get(FAN_TIMER))
    }

    /// A change sent from elsewhere: the display lights, and stays lit for
    /// REMOTE_WAKE, as stock's does for one from its cloud; true when it
    /// was dark.
    pub fn remote_change(&mut self) -> bool {
        self.remote_until = Some(Instant::now() + REMOTE_WAKE);
        if !self.asleep {
            return false;
        }
        debug!("display: lit by a change from elsewhere");
        self.asleep = false;
        self.far = false;
        self.changed_screen = self.now();
        true
    }

    /// Someone came near (the thermopile or the near PIR), as stock's
    /// nlfarsighthandler takes it (0x145640): Farsight's clocks and weather
    /// go to their near view, coming in over `NEAR_TIME` once the far view
    /// has shown for `FAR_HOLD`, and a dark display lights on it whole; its
    /// temperatures give way to the home screen over `LEAVE_TIME`. The near
    /// view stays until an input, as the far view does. Under an alert,
    /// notice or dialog the clocks and weather give way too (0x145408 then
    /// answers 1, not 2), the alert coming in as they go.
    pub fn approach(&mut self) -> bool {
        let wake = self.settings.text(FARSIGHT).unwrap_or("target_temp").to_string();
        let views = matches!(wake.as_str(), "analog_clock" | "digital_clock" | "weather")
            && !matches!(self.screen(), "alert" | "notice" | "dialog");
        let now = self.now();
        if self.asleep {
            let lit = self.presence();
            if lit {
                self.far = views;
                self.near = views.then_some(now - NEAR_TIME);
            }
            return lit;
        }
        self.presence();
        if !self.far || self.near.is_some() {
            return false;
        }
        debug!("display: someone comes near ({wake})");
        if views {
            self.near = Some(now.max(self.changed_screen + FAR_HOLD));
        } else {
            self.far = false;
            self.leaving = Some(now);
            self.left_far(now);
        }
        true
    }

    /// Farsight gives way: a new screen, unless an alert, notice or dialog
    /// was over it, whose own fade-in stays done (stock's alert shows at
    /// 0x227364's opacity alone).
    fn left_far(&mut self, now: f64) {
        if !matches!(self.screen(), "alert" | "notice" | "dialog") {
            self.changed_screen = now;
        }
    }

    pub fn set_display_idle(&mut self, idle: Duration) {
        self.display_idle = idle;
    }

    pub fn asleep(&self) -> bool {
        self.asleep
    }

    /// Another program has the screen (Doom): the display stays awake and
    /// the menus close, so the home screen is what comes back.
    pub fn stand_aside(&mut self) {
        self.last_input = Instant::now();
        self.asleep = false;
        if self.nav.is_open() {
            self.nav.close();
            self.changed_screen = self.now();
        }
    }

    /// Applies one input; any input means a new frame. False when it only
    /// woke the display. The button acts when it is released, as stock's
    /// does; a press that woke the display has its release ignored.
    /// Someone is here: Home/Away Assist's activity, and its day trained.
    fn seen(&mut self) {
        // SAFETY: time with a null pointer only returns the time.
        let now = unsafe { libc::time(std::ptr::null_mut()) } as f64;
        self.activity = now;
        let day = crate::sunblock::Local::at(now).date;
        let key = format!("{}-{}-{}", day.0, day.1, day.2);
        let mut days: Vec<String> = self.settings.text(AWAY_DAYS).and_then(|t| serde_json::from_str(t).ok()).unwrap_or_default();
        if !days.contains(&key) {
            days.push(key);
            let skip = days.len().saturating_sub(AWAY_TRAINING);
            days.drain(..skip);
            self.settings.set(AWAY_DAYS, Value::Text(serde_json::to_string(&days).unwrap_or_default()));
        }
    }

    /// Whether Home/Away Assist has trained ("ready"; else "training").
    pub fn away_ready(&self) -> bool {
        self.settings.text(AWAY_DAYS).and_then(|t| serde_json::from_str::<Vec<String>>(t).ok()).is_some_and(|d| d.len() >= AWAY_TRAINING)
    }

    /// Whether Home/Away Assist finds no one home.
    pub fn away(&self) -> bool {
        // SAFETY: time with a null pointer only returns the time.
        let now = unsafe { libc::time(std::ptr::null_mut()) } as f64;
        self.activity > 0.0 && now - self.activity >= AWAY_AFTER
    }

    /// Home/Away Assist acting as stock's service does with no other
    /// device: Eco when the home is empty, back to the schedule when
    /// someone is seen; Eco set by hand stays (stock's goosemgr, 0x2ef490).
    fn away_tick(&mut self) {
        if self.activity == 0.0 {
            // SAFETY: time with a null pointer only returns the time.
            self.activity = unsafe { libc::time(std::ptr::null_mut()) } as f64;
        }
        let auto = self.settings.get(ECO_AUTO) == Some(&Value::Bool(true));
        if !self.eco() && auto {
            self.settings.set(ECO_AUTO, Value::Bool(false));
        }
        let on = self.settings.text("away.assist") != Some("off") && self.settings.text(MODE) != Some("off");
        if on && self.away_ready() && self.away() && !self.eco() {
            info!("away: no one seen for two hours; Eco");
            self.settings.set(ECO_AUTO, Value::Bool(true));
            self.settings.set(ECO, Value::Bool(true));
            self.changed_target = self.now();
        } else if auto && self.eco() && !self.away() {
            info!("away: someone is home; back to the schedule");
            self.settings.set(ECO_AUTO, Value::Bool(false));
            self.settings.set(ECO, Value::Bool(false));
            self.changed_target = self.now();
        }
    }

    /// Opens the menus at an entry, named level by level (the skin check).
    pub fn open_entry(&mut self, labels: &[&str]) {
        let menus = self.menus();
        self.nav.open_entry(&menus, labels);
        self.changed_screen = self.now();
    }

    /// Opens the menus on the submenu `path` leads to, on its entry `at`
    /// (the desktop's tools, the skins' check); whether it leads to one.
    pub fn open_menu(&mut self, path: &[String], at: Option<usize>) -> bool {
        let menus = self.menus();
        let labels: Vec<&str> = path.iter().map(String::as_str).collect();
        let opened = self.nav.open_at(&menus, &labels, at);
        debug!("menu: opened at {} (entry {at:?}) by a tool", labels.join(" > ").replace('\n', " "));
        self.last_input = Instant::now();
        self.changed_screen = self.now();
        self.changed_menu = self.now();
        opened
    }

    /// One wheel event's counts, the gen 3's (negative clockwise): a
    /// carousel on screen moves by them through stock's physics, where
    /// everything else takes the wheel's detents.
    pub fn rotary(&mut self, counts: i32, at: Instant) -> bool {
        trace!("wheel: {counts} counts");
        if self.alert.is_none() && !self.starting && self.notice.is_none() && !self.far {
            let f = self.fahrenheit();
            if let Some((kb, _)) = self.keyboard.as_mut() {
                kb.counts(counts, at);
                self.keyboard_counted = true;
                self.last_input = Instant::now();
                return true;
            }
            if let Some(editor) = self.editor.as_mut() {
                editor.counts(counts, f, &self.schedule, at);
                self.editor_counted = true;
                self.last_input = Instant::now();
                self.changed_menu = self.now();
                return true;
            }
            // The detail page takes the wheel's detents instead.
            if self.energy.as_mut().is_some_and(|v| v.counts(counts, at)) {
                self.energy_counted = true;
                self.last_input = Instant::now();
                self.changed_menu = self.now();
                return true;
            }
        }
        let menu = self.alert.is_none() && !self.starting && self.notice.is_none()
            && (self.wiring_error.or(self.report).is_none() || self.tech_open)
            && self.eco_prompt.is_none() && self.emergency_alert.is_none() && self.off_alert.is_none() && self.date_alert.is_none() && self.switchover_alert.is_none() && !self.airwave_alert && self.filter_alert.is_none() && self.wifi_alert.is_none()
            && !self.short_cycle_alert && !self.battery_alert && !self.furnace_alert && self.editor.is_none() && self.energy.is_none() && self.keyboard.is_none() && self.postal.is_none() && self.wifi.page.is_none()
            && self.nav.is_open() && !self.far;
        if !menu || self.fan_editing() {
            return false;
        }
        let menus = self.menus();
        let moved = self.nav.rotary(&menus, counts, at);
        if moved {
            self.last_input = Instant::now();
            self.changed_menu = self.now();
        }
        moved
    }

    pub fn input(&mut self, input: Input) -> bool {
        if let Input::Notch(n) = input {
            self.nav.notched = true;
            let changed = self.input(Input::Wheel(n));
            self.nav.notched = false;
            return changed;
        }
        self.last_input = Instant::now();
        let now = self.now();
        if std::mem::take(&mut self.far) {
            self.leaving = Some(now);
            self.left_far(now);
            self.ignore_release = input == Input::Press;
            return false;
        }
        if self.asleep {
            if input == Input::Release {
                return false;
            }
            debug!("display: woken by {input:?}, which does nothing else");
            self.asleep = false;
            self.ignore_release = input == Input::Press;
            self.changed_screen = now;
            return false;
        }
        if input == Input::Release && std::mem::take(&mut self.ignore_release) {
            debug!("engine: release of the press that woke the display ignored");
            return false;
        }
        // A press cancels a reset counting down (stock's 0x13c768).
        if let Some((action, ..)) = self.countdown {
            if input == Input::Release {
                info!("reset: {action} cancelled");
                self.countdown = None;
                self.changed_screen = now;
            }
            return input == Input::Release;
        }
        // A press on a power-off alert opens the wiring report (stock's
        // 0x22dd88: alerts 0xe and 0x31 to
        // equipment.wirereportlayer.resetlayeronlystep).
        if matches!(self.alert, Some(POWER_OFF | POWER_OFF_LOCKOUT)) && !self.starting {
            if input == Input::Release {
                self.open_report(now);
            }
            return input == Input::Release;
        }
        if self.alert.is_some() || self.starting {
            debug!("engine: {input:?} ignored under {}", if self.starting { "the start" } else { "an alert" });
            return false;
        }
        if self.notice.is_some() {
            // CONTINUE: the wiring as it is now is the wiring known, and
            // Equipment shows the system it makes, as stock's wizard does.
            if input == Input::Release {
                info!("notice: continued; the wiring as it is now is kept");
                self.notice = None;
                self.remember_wiring();
                self.pick_saying();
                self.nav.open_entry(&self.menus(), &["Settings", "Equipment"]);
                self.changed_screen = now;
                self.changed_menu = now;
            }
            return input == Input::Release;
        }
        if let Some(error) = self.wiring_error.or(self.report).filter(|_| !(self.tech_open && self.nav.is_open())) {
            let last = error.choices.len() - 1;
            match input {
                // The first turn highlights the first choice.
                Input::Wheel(d) | Input::Notch(d) => {
                    self.error_choice = Some(self.error_choice.map_or(0, |c| (c as i32 + d).clamp(0, last as i32) as usize));
                }
                Input::Release if self.error_choice.is_some() => {
                    debug!("error page: {} chosen", error.choices[self.error_choice.unwrap_or(0)].to_lowercase());
                    match error.choices[self.error_choice.unwrap_or(0)] {
                        "TECH INFO" => {
                            self.error_choice = None;
                            self.tech_open = true;
                            self.pick_saying();
                            self.nav.open_entry(&self.menus(), &["Settings", "Technical\nInfo"]);
                            self.changed_menu = now;
                        }
                        // The wiring is checked as it comes; with no Heat
                        // Link radio there is nothing more to check.
                        "CHECK AGAIN" => {}
                        // A note's page: on to the Equipment menu under it.
                        "CONTINUE" => {
                            self.error_choice = None;
                            self.wiring_error = None;
                            self.report = None;
                        }
                        other => warn!("error page: {} is not available", other.to_lowercase()),
                    }
                    self.changed_screen = now;
                }
                _ => return false,
            }
            return true;
        }
        if let Some((alert, choice)) = self.wifi_alert.as_mut() {
            let last = alert.options().len() - 1;
            match input {
                Input::Wheel(d) | Input::Notch(d) => *choice = (*choice as i32 + d.signum()).clamp(0, last as i32) as usize,
                Input::Release => {
                    info!("wifi: alert answered {}", alert.options()[*choice].to_lowercase());
                    let settings = alert.options()[*choice] == "WIFI SETTINGS";
                    self.wifi_alert = None;
                    // WIFI SETTINGS: the Network list (settingsview "wifi").
                    if settings {
                        self.nav.open_entry(&self.menus(), &["Settings", "Network"]);
                        self.changed_menu = now;
                    }
                    self.changed_screen = now;
                }
                Input::Press => return false,
            }
            return true;
        }
        if self.wifi.page.is_some() && self.keyboard.is_none() {
            match input {
                Input::Wheel(d) | Input::Notch(d) => self.wifi.wheel(d),
                Input::Release => {
                    let (acts, ask, to_tile) = self.wifi.press(self.wall());
                    self.wifi_acts(acts);
                    self.wifi_ask(ask);
                    if to_tile {
                        self.nav.up();
                    }
                    self.changed_screen = now;
                }
                Input::Press => return false,
            }
            self.changed_menu = now;
            return true;
        }
        if let Some((kb, key)) = self.keyboard.as_mut() {
            match input {
                // The same turn's counts have moved it.
                Input::Wheel(d) | Input::Notch(d) => if !std::mem::take(&mut self.keyboard_counted) { kb.wheel(d) },
                Input::Release => {
                    let key = *key;
                    match kb.press() {
                        crate::keyboard::Outcome::Stay => {}
                        crate::keyboard::Outcome::Done(text) => {
                            debug!("keyboard: done for {key}");
                            self.keyboard = None;
                            match key {
                                // A name typed with nothing in it is no network
                                // (stock's Wi-Fi name field).
                                // A hidden network's name, never empty; its
                                // security follows.
                                WIFI_NAME if text.is_empty() => {}
                                WIFI_NAME => self.wifi.hidden(&text),
                                WIFI_PSK => {
                                    if let Some((name, hidden)) = self.joining.take() {
                                        let acts = self.wifi.passphrase(&name, &text, hidden, self.wall());
                                        self.wifi_acts(acts);
                                    }
                                }
                                PLACE_CUSTOM if text.is_empty() => {}
                                PLACE_CUSTOM => self.custom_place(&text),
                                ASKED => self.answer(text),
                                key => self.settings.change(key, Value::Text(text)),
                            }
                            self.changed_screen = now;
                        }
                        crate::keyboard::Outcome::Cancel => {
                            debug!("keyboard: cancelled for {key}");
                            self.keyboard = None;
                            self.joining = None;
                            self.ask = None;
                            self.changed_screen = now;
                        }
                    }
                }
                Input::Press => return false,
            }
            self.changed_menu = now;
            return true;
        }
        if self.systest.is_some() {
            let env = self.systest_env();
            let Some(test) = self.systest.as_mut() else { return false };
            match input {
                Input::Wheel(d) | Input::Notch(d) => test.wheel(d),
                Input::Release => if test.press(&env) == crate::systemtest::Outcome::Close {
                    self.close_systest();
                },
                Input::Press => return false,
            }
            self.hvac.test = self.systest.as_ref().and_then(crate::systemtest::SystemTest::request);
            self.changed_menu = now;
            return true;
        }
        if let Some(pro) = self.pro.as_mut() {
            match input {
                Input::Wheel(d) | Input::Notch(d) => pro.wheel(d),
                Input::Release => match pro.press() {
                    crate::prosetup::Outcome::Stay => {}
                    // Kept; Equipment Settings is under it (TEST's own pages
                    // are to come).
                    crate::prosetup::Outcome::Done(setup) => self.pro_done(setup, false),
                    // TEST opens the system test over it (stock's 0x191c54).
                    crate::prosetup::Outcome::Test(setup) => self.pro_done(setup, true),
                },
                Input::Press => return false,
            }
            self.changed_menu = now;
            return true;
        }
        if let (Some(page), None) = (self.equip.as_mut(), self.off_alert) {
            match input {
                Input::Wheel(d) | Input::Notch(d) => page.wheel(d),
                // Setting the humidity while off asks to turn the system on
                // first (stock's 0x13b83c).
                Input::Release if page.sets_humidity() && self.settings.text(MODE) == Some("off") => {
                    debug!("equipment: the thermostat is off; asking to turn it on");
                    self.off_alert = Some(1);
                    self.changed_screen = now;
                }
                Input::Release => {
                    if let crate::equipment::Outcome::Done(changes) = page.press() {
                        info!("equipment: page done");
                        self.equip = None;
                        // Pro Setup's warning goes too, onto Equipment Settings.
                        if self.nav.view(&self.menus(), &self.settings).is_some_and(|v| v.style.as_deref() == Some("answers")) {
                            self.nav.up();
                        }
                        for (key, value) in changes {
                            self.settings.change(key, value);
                        }
                        self.changed_screen = now;
                    }
                }
                Input::Press => return false,
            }
            self.changed_menu = now;
            return true;
        }
        if let Some(entry) = self.postal.as_mut() {
            match input {
                Input::Wheel(d) | Input::Notch(d) => entry.wheel(d),
                Input::Release => {
                    if let crate::postal::Outcome::Done(code) = entry.press() {
                        let iso = entry.iso;
                        self.postal = None;
                        if self.ask.is_some() {
                            self.answer(code);
                        } else {
                            self.set_location(iso, &code);
                        }
                        self.changed_screen = now;
                    }
                }
                Input::Press => return false,
            }
            self.changed_menu = now;
            return true;
        }
        if let Some(view) = self.energy.as_mut() {
            match input {
                // The same turn's counts have moved the cards.
                Input::Wheel(d) | Input::Notch(d) => if !std::mem::take(&mut self.energy_counted) { view.wheel(d, Instant::now()) },
                Input::Release => {
                    if let crate::history::Pressed::Closed = view.press(Instant::now()) {
                        self.energy = None;
                        self.changed_screen = now;
                    }
                }
                Input::Press => return false,
            }
            self.changed_menu = now;
            return true;
        }
        if self.editor.is_some() {
            let f = self.fahrenheit();
            let unset = crate::schedule::unset(f, self.schedule_kind() == "RANGE");
            let editor = self.editor.as_mut().unwrap();
            match input {
                // The same turn's counts have moved it.
                Input::Wheel(d) | Input::Notch(d) => if !std::mem::take(&mut self.editor_counted) { editor.wheel(d, f, &self.schedule, Instant::now()) },
                Input::Release => {
                    match editor.press(&mut self.schedule, unset) {
                        crate::schedule::Outcome::Closed => {
                            self.editor = None;
                            self.changed_screen = now;
                        }
                        crate::schedule::Outcome::Changed => self.apply_schedule(true),
                        crate::schedule::Outcome::Stay => {}
                    }
                }
                Input::Press => return false,
            }
            self.changed_menu = now;
            return true;
        }
        if let Some((level, choice)) = self.filter_alert {
            let last = if level == 1 { 1 } else { 0 };
            match input {
                Input::Wheel(d) | Input::Notch(d) => self.filter_alert = Some((level, (choice as i32 + d.signum()).clamp(0, last) as usize)),
                Input::Release => {
                    self.filter_alert = None;
                    // REMIND LATER on the first; DONE: the filter is changed.
                    if level == 1 && choice == 0 {
                        info!("filter: remind later");
                        self.history.filter.remind_later();
                    } else {
                        info!("filter: changed");
                        self.filter_changed(0);
                    }
                    self.changed_screen = now;
                }
                Input::Press => return false,
            }
            return true;
        }
        if let Some(choice) = self.emergency_alert {
            match input {
                Input::Wheel(d) | Input::Notch(d) => self.emergency_alert = Some(if d > 0 { 1 } else if d < 0 { 0 } else { choice }),
                Input::Release => {
                    self.emergency_alert = None;
                    debug!("emergency heat: alert answered {}", if choice == 1 { "turn off" } else { "leave on" });
                    // TURN OFF leaves emergency heat and goes on (0x1d94f0).
                    if choice == 1 {
                        self.settings.change(EMERGENCY, Value::Text("off".into()));
                        self.emergency_tick();
                        let menus = self.menus();
                        if let Outcome::Action(name) = self.nav.press(&menus, &mut self.settings) {
                            self.action(&name);
                        }
                    }
                    self.changed_menu = now;
                    self.changed_screen = now;
                }
                Input::Press => return false,
            }
            return true;
        }
        if let Some(choice) = self.off_alert {
            match input {
                Input::Wheel(d) | Input::Notch(d) => self.off_alert = Some(if d > 0 { 1 } else if d < 0 { 0 } else { choice }),
                Input::Release => {
                    self.off_alert = None;
                    debug!("off alert: answered {}", if choice == 1 { "turn on" } else { "leave off" });
                    // TURN ON turns the system on and goes on to the page
                    // asked for (the fan's, emergency heat's, the humidity
                    // target's); LEAVE OFF stays (0x1e89e8).
                    if choice == 1 {
                        self.turn_on();
                        if let Some(page) = self.equip.as_mut() {
                            page.press();
                        } else {
                            let menus = self.menus();
                            if let Outcome::Action(name) = self.nav.press(&menus, &mut self.settings) {
                                self.action(&name);
                            }
                        }
                    }
                    self.changed_menu = now;
                    self.changed_screen = now;
                }
                Input::Press => return false,
            }
            return true;
        }
        if let Some(choice) = self.date_alert {
            match input {
                Input::Wheel(d) | Input::Notch(d) => self.date_alert = Some(if d > 0 { 1 } else if d < 0 { 0 } else { choice }),
                Input::Release => {
                    self.date_alert = None;
                    info!("alert: date and time answered {}", if choice == 1 { "set date/time" } else { "ok" });
                    // SET DATE/TIME: Settings' page setting them.
                    if choice == 1 {
                        let menus = self.menus();
                        let at = menus.items.iter().find(|i| i.label == "Settings").and_then(|i| match &i.kind {
                            crate::menu::Kind::Menu(items) => items.iter().find(|i| i.label == "Date & Time"),
                            _ => None,
                        }).and_then(|i| match &i.kind {
                            crate::menu::Kind::Menu(items) => items.iter().position(|i| i.label == "Set time"),
                            _ => None,
                        });
                        if self.nav.open_at(&menus, &["Settings", "Date & Time"], at) && at.is_some() {
                            if let Outcome::Action(name) = self.nav.press(&menus, &mut self.settings) {
                                self.action(&name);
                            }
                        }
                        self.changed_menu = now;
                    }
                    self.changed_screen = now;
                }
                Input::Press => return false,
            }
            return true;
        }
        if let Some(choice) = self.switchover_alert {
            let options = self.switchover_options();
            match input {
                Input::Wheel(d) | Input::Notch(d) => self.switchover_alert = Some((choice as i32 + d.signum()).clamp(0, options.len() as i32 - 1) as usize),
                Input::Release => {
                    self.switchover_alert = None;
                    info!("alert: switchover answered {}", options[choice].to_lowercase());
                    // A mode other than Off sets it and stops Eco; Off,
                    // checked, and CANCEL leave things as they are
                    // (stock's 0x1b8298).
                    let mode = match options[choice] {
                        "HEAT" => Some("heat"),
                        "COOL" => Some("cool"),
                        "HEAT • COOL" => Some("heat-cool"),
                        _ => None,
                    };
                    if let Some(mode) = mode {
                        if self.eco() {
                            self.settings.change(ECO, Value::Bool(false));
                            self.settings.set(ECO_AUTO, Value::Bool(false));
                        }
                        self.settings.change(MODE, Value::Text(mode.into()));
                    }
                    self.changed_screen = now;
                }
                Input::Press => return false,
            }
            return true;
        }
        if self.airwave_alert {
            if input == Input::Release {
                info!("alert: airwave answered ok");
                self.airwave_alert = false;
                self.changed_screen = now;
            }
            return input != Input::Press;
        }
        if let Some(stop) = self.eco_prompt {
            match input {
                Input::Wheel(d) | Input::Notch(d) => self.eco_prompt = Some(d < 0 || (stop && d == 0)),
                Input::Release => {
                    self.eco_prompt = None;
                    debug!("eco: prompt answered {}", if stop { "stop" } else { "cancel" });
                    if stop {
                        self.settings.change(ECO, Value::Bool(false));
                    }
                    self.changed_screen = now;
                }
                Input::Press => return false,
            }
            return true;
        }
        if self.short_cycle_alert || self.battery_alert || self.furnace_alert {
            match input {
                // A page taller than the face turns down a line a detent;
                // with two answers the turn also picks one.
                Input::Wheel(d) | Input::Notch(d) => {
                    self.alert_scrolled = (self.alert_scrolled as i64 + d as i64).clamp(0, 40) as usize;
                    if self.battery_alert && d != 0 {
                        if let Some(choice) = self.equipment_alert.as_mut() {
                            *choice = usize::from(d > 0);
                        }
                    }
                    self.changed_screen = now;
                }
                Input::Release => {
                    let answer = match self.equipment_alert.take() {
                        Some(1) if self.battery_alert => "back",
                        Some(_) => "continue",
                        None => "ok",
                    };
                    if self.short_cycle_alert {
                        info!("alert: short cycle answered {answer}");
                        self.short_cycle_alert = false;
                    } else if self.battery_alert {
                        info!("alert: battery drained answered {answer}");
                        self.battery_alert = false;
                    } else {
                        info!("alert: furnace shutdown answered {answer}");
                        self.furnace_alert = false;
                    }
                    // BACK stays in Settings; CONTINUE goes on into
                    // Equipment, opened under the alert (0x157388).
                    if answer == "back" {
                        self.nav.up();
                        self.changed_menu = now;
                    } else if answer == "continue" {
                        self.equipment_notes(now);
                    }
                    self.alert_scrolled = 0;
                    self.changed_screen = now;
                }
                Input::Press => return false,
            }
            return true;
        }
        // The ring turned while off asks for a mode (stock's 0x1d9880;
        // its other condition, Safety Temperatures running, +0x94, has no
        // setter in 6.4-8).
        if !self.nav.is_open() && self.settings.text(MODE) == Some("off") && matches!(input, Input::Wheel(_)) {
            let options = self.switchover_options();
            self.switchover_alert = options.iter().position(|o| *o == "OFF");
            info!("alert: {}", one_line(SWITCHOVER_TEXT));
            self.changed_screen = now;
            return true;
        }
        if !self.nav.is_open() && self.eco() && matches!(input, Input::Wheel(_)) {
            self.eco_prompt = Some(false);
            self.changed_screen = now;
            return true;
        }
        if !self.nav.is_open() && self.settings.text(MODE) == Some("heat-cool") && input != Input::Press {
            self.range_input(input, now);
            return true;
        }
        match (self.nav.is_open(), input) {
            (false, Input::Wheel(detents)) | (false, Input::Notch(detents)) => {
                let f = self.fahrenheit();
                self.thermostat.turn(detents, f);
                debug!("home: target {:.1}°C", self.thermostat.target);
                self.changed_target = now;
                self.turned_target = now;
            }
            (false, Input::Release) => {
                self.pick_saying();
                self.software_shown();
                // A heat or cool target at either end on a system that does
                // both: the menus open on Mode (stock's 0x1d8aa0).
                let mode = self.settings.text(MODE).unwrap_or("heat");
                if matches!(mode, "heat" | "cool") && self.target_end() != 0 && self.heats_cools() == (true, true) {
                    debug!("home: target at its end; the menus open on Mode");
                    let menus = self.menus();
                    let at = menus.items.iter().position(|i| i.label == "Mode");
                    self.nav.open_at(&menus, &[], at);
                } else {
                    self.nav.open(&self.menus());
                }
                self.changed_screen = now;
                self.changed_menu = now;
            }
            (true, Input::Wheel(detents)) | (true, Input::Notch(detents)) => {
                // The running fan's entry offers only Stop fan and Back.
                if self.fan_editing() {
                    self.nav.highlight((detents < 0).then(|| Value::Text("off".into())));
                } else {
                    let menus = self.menus();
                    self.nav.turn(&menus, detents);
                    if let Some(name) = self.nav.confirmed(&menus) {
                        self.action(&name);
                        self.changed_screen = now;
                    }
                }
                self.changed_menu = now;
            }
            (true, Input::Release) => {
                let menus = self.menus();
                let guarded = self.nav.pointed(&menus).is_some_and(|p| EMERGENCY_GUARDED.contains(&p));
                if guarded && matches!(self.hvac_mode(), crate::hvac::control::Mode::Emergency(_)) {
                    debug!("menu: emergency heat guards this entry; asking first");
                    self.emergency_alert = Some(0);
                    self.changed_screen = now;
                    return true;
                }
                // Settings' Fan (its Fan Schedule with one speed) while off
                // asks to turn the system on first (stock's nlSettingsView,
                // 0x1b007c); so does Equipment Settings' emergency heat
                // (0x13ba54).
                if matches!(self.nav.pointed(&menus), Some((2, "Fan Schedule" | "Fan")) | Some((4, "Emerg. Heat"))) && self.settings.text(MODE) == Some("off") {
                    debug!("menu: the thermostat is off; asking to turn it on");
                    self.off_alert = Some(1);
                    self.changed_screen = now;
                    return true;
                }
                // Mode on a system that only heats or only cools turns it
                // off and on, stopping Eco, and closes the menus (stock's
                // nlThermostatView, 0x1e0af0).
                if self.nav.pointed(&menus) == Some((1, "Mode")) && self.heats_cools() != (true, true) {
                    let off = self.settings.text(MODE) == Some("off");
                    info!("mode: turned {} by Mode's press", if off { "on" } else { "off" });
                    if self.eco() {
                        self.settings.change(ECO, Value::Bool(false));
                        self.settings.set(ECO_AUTO, Value::Bool(false));
                    }
                    if off {
                        self.turn_on();
                    } else {
                        self.settings.change(MODE, Value::Text("off".into()));
                    }
                    self.nav.close();
                    self.changed_screen = now;
                    self.changed_menu = now;
                    return true;
                }
                let before = self.nav.within(&menus, "Equipment");
                let outcome = self.nav.press(&menus, &mut self.settings);
                if outcome != Outcome::Stay {
                    debug!("menu: press: {outcome:?}");
                }
                match outcome {
                    Outcome::Closed => self.changed_screen = now,
                    Outcome::Action(name) => self.action(&name),
                    Outcome::Ask(ask) => self.open_ask(ask),
                    Outcome::Stay => {}
                }
                // Opening Equipment while the short-cycle flag stands: its
                // HVAC alert first, HvacBatteryDrained's when that stands
                // too (stock's 0x1b007c: globals+0x18a7, then the
                // thermostat's +0x421, taken as HvacBatteryDrained).
                let entered = !before && self.nav.within(&menus, "Equipment");
                if entered && self.short_cycle {
                    if self.battery_drained {
                        info!("alert: {BATTERY_DRAINED_TITLE}, on the way into Equipment");
                        self.battery_alert = true;
                    } else {
                        info!("alert: {SHORT_CYCLE_TITLE}, on the way into Equipment");
                        self.short_cycle_alert = true;
                    }
                    self.equipment_alert = Some(0);
                    self.alert_scrolled = 0;
                    self.changed_screen = now;
                } else if entered {
                    self.equipment_notes(now);
                }
                if self.fan_editing() {
                    self.nav.highlight(Some(Value::Text("off".into())));
                }
                self.changed_menu = now;
            }
            (_, Input::Press) => return false,
        }
        true
    }

    /// The home screen in heat-cool, as stock's: turning picks an end of the
    /// range (clockwise the cooling one), pressing starts changing it,
    /// turning then changes it and pressing again sets it. A press with
    /// nothing picked opens the menus.
    fn range_input(&mut self, input: Input, now: f64) {
        match (self.range, input) {
            (Some((side, true)), Input::Wheel(detents)) | (Some((side, true)), Input::Notch(detents)) => {
                let f = self.fahrenheit();
                self.thermostat.turn_range(side, detents, f);
                debug!("home: range {:.1}-{:.1}°C", self.thermostat.low, self.thermostat.high);
                self.changed_target = now;
                self.turned_target = now;
            }
            (_, Input::Wheel(detents)) | (_, Input::Notch(detents)) => {
                let side = if detents > 0 { Side::High } else { Side::Low };
                debug!("home: range's {} end picked", side.name());
                self.range = Some((side, false));
                self.changed_target = now;
            }
            (Some((side, false)), Input::Release) => {
                debug!("home: range's {} end being changed", side.name());
                self.range = Some((side, true));
                self.changed_target = now;
            }
            (Some((side, true)), Input::Release) => {
                debug!("home: range's {} end set", side.name());
                self.range = None;
                self.changed_target = now;
            }
            (None, Input::Release) => {
                self.pick_saying();
                self.software_shown();
                self.nav.open(&self.menus());
                self.changed_screen = now;
                self.changed_menu = now;
            }
            (_, Input::Press) => {}
        }
    }

    pub fn action(&mut self, name: &str) {
        debug!("menu action: {name}");
        // A plugin's shell command, left to run on its own.
        if let Some(command) = name.strip_prefix("run:") {
            match std::process::Command::new("sh").args(["-c", command]).spawn() {
                Ok(mut child) => {
                    std::thread::spawn(move || child.wait());
                }
                Err(e) => error!("run: {command}: {e}"),
            }
            return;
        }
        match name {
            "clock.set" => {
                self.set_clock();
                self.changed_screen = self.now();
            }
            // The place asked about is where the thermostat is: Location
            // is left, as once one is given.
            "location.keep" => {
                let tree = self.menus();
                while self.nav.within(&tree, "Location") {
                    self.nav.up();
                }
                self.changed_screen = self.now();
            }
            // The unit restarts, as stock's Reset > Restart does; the
            // desktop's host does not.
            "restart" if cfg!(feature = "desktop") => info!("restart: not on the desktop"),
            "restart" => {
                self.settings.flush();
                info!("restart: rebooting");
                match std::process::Command::new("reboot").status() {
                    Ok(status) if status.success() => {}
                    Ok(status) => error!("restart: reboot: {status}"),
                    Err(e) => error!("restart: reboot: {e}"),
                }
            }
            "history" => self.open_history(),
            // PRO SETUP for conventional wiring: its wires.
            "equipment.setup" if !self.model.as_deref().is_some_and(|m| heat_link_unit(self.device, m, self.backplate_version)) => {
                let settings = &self.settings;
                let get = |k: &str| settings.text(k).map(str::to_string);
                let wiring = crate::equipment::Wiring { wired: self.wired_terminals(), heat_pump: self.heat_pump(), get: &get };
                self.equip = Some(crate::equipment::Page::wires(crate::equipment::wires(&wiring)));
                self.changed_screen = self.now();
            }
            "equipment.heatpump" => {
                let number = |k: &str| match self.settings.get(k) { Some(&Value::Number(n)) => Some(n as f32), _ => None };
                let e = &self.hvac.equipment;
                let (compressor, aux) = (number(crate::equipment::COMPRESSOR).or(e.compressor_lockout), number(crate::equipment::AUX).or(e.aux_lockout));
                let ob = self.settings.text(crate::equipment::OB).map(str::to_string);
                self.equip = Some(crate::equipment::Page::heat_pump(ob.as_deref(), compressor, aux, e.savings == 3));
                self.changed_screen = self.now();
            }
            "equipment.dualfuel" => {
                let e = &self.hvac.equipment;
                let over = self.settings.text(crate::equipment::OVERRIDE)
                    .unwrap_or(match e.dual_fuel_override { 1 => "ALWAYS-ALT", 2 => "ALWAYS-PRIMARY", _ => "NONE" }).to_string();
                self.equip = Some(crate::equipment::Page::dual_fuel(e.dual_fuel_breakpoint, &over, self.hvac.outdoor.is_some()));
                self.changed_screen = self.now();
            }
            "equipment.humidity" => {
                let hour = |k: &str, or: u32| match self.settings.get(k) { Some(&Value::Number(n)) => n as u32 % 24, _ => or };
                let e = &self.hvac.equipment;
                let hours = [hour(crate::equipment::QUIET_START, 22), hour(crate::equipment::QUIET_END, 6)];
                let quiet = e.humidity.quiet.map(|(a, b)| [a / 3600, b / 3600]);
                let humidifier = self.settings.get(HAS_HUMIDIFIER) == Some(&Value::Bool(true));
                self.equip = Some(crate::equipment::Page::humidity(humidifier, e.humidity.target, quiet, hours));
                self.changed_screen = self.now();
            }
            "equipment.star" => {
                let e = &self.hvac.equipment;
                let wired = self.wired_terminals();
                let options = crate::equipment::star_options(self.heat_pump(), e.dual_fuel, wired.contains(&"W1"), wired.contains(&"W2"),
                                                            wired.contains(&"G") && e.y2 == 3);
                let star = self.settings.text(crate::equipment::STAR).map(str::to_string);
                self.equip = Some(crate::equipment::Page::star(options, star.as_deref()));
                self.changed_screen = self.now();
            }
            // Equipment's TEST (stock's 0x13a080).
            "equipment.test" => self.open_systest(),
            // PRO SETUP, from what is kept (stock's fresh unit: electric).
            "equipment.setup" => {
                let text = |key: &str, default: &str| self.settings.text(key).unwrap_or(default).to_string();
                let setup = crate::prosetup::Setup {
                    boiler: text(BOILER, "electric"), control: text(HEAT_CONTROL, "onoff"), source: text(HEAT_SOURCE, "electric"),
                    delivery: text(HEAT_DELIVERY, "radiator"), hot_water: text(HOT_WATER_CONTROL, "notconnected"),
                };
                let mut rest = Vec::new();
                if self.heats_cools().1 { rest.push("Cooling".to_string()); }
                if self.hvac.equipment.has_fan { rest.push("Fan".to_string()); }
                self.pro = Some(crate::prosetup::ProSetup::open(setup, rest));
                self.changed_screen = self.now();
            }
            // A country: its postal code next; Ireland's county and the
            // UAE's city come with the name.
            name if name.starts_with("location.") => {
                let mut parts = name["location.".len()..].splitn(2, '.');
                let iso = parts.next().unwrap_or("");
                if let Some(place) = parts.next() {
                    let iso = crate::postal::COUNTRIES.iter().map(|c| c.iso).chain(["IE", "AE"]).find(|c| *c == iso).unwrap_or("");
                    self.set_location(iso, place);
                } else if let Some(country) = crate::postal::country(iso) {
                    let stored = if self.settings.text(COUNTRY) == Some(iso) { self.settings.text(POSTAL).unwrap_or("") } else { "" };
                    self.postal = Some(crate::postal::Entry::open(country, stored));
                }
                self.changed_screen = self.now();
            }
            // A network not in range by name, then its passphrase.
            "net.type" => {
                self.keyboard = Some((crate::keyboard::Keyboard::open("", 256, "Wi-Fi network"), WIFI_NAME));
                self.changed_screen = self.now();
            }
            // DISCONNECT, or DO NOT CONNECT with nothing connected: no
            // question asked (stock's 0x1f31b4).
            "net.disconnect" => {
                let acts = self.wifi.disconnect(&self.networks, self.wall());
                self.wifi_acts(acts);
                self.changed_screen = self.now();
            }
            // Placement's name for a room of one's own (stock's nlNameLayer:
            // 30 characters at most).
            "keyboard.custom" => {
                self.keyboard = Some((crate::keyboard::Keyboard::open("", 30, "Custom"), PLACE_CUSTOM));
                self.changed_screen = self.now();
            }
            "schedule" => {
                self.nav.close();
                let mut editor = crate::schedule::Editor::open(self.week_minute(), &self.schedule);
                editor.scroll_in(Instant::now(), &self.schedule);
                self.editor = Some(editor);
                self.changed_screen = self.now();
            }
            // Reset's pages count down first, a press cancelling them.
            name if self.countdown.is_none_or(|(n, ..)| n != name) && RESET_COUNTDOWNS.iter().any(|(n, _)| *n == name) => {
                let action = RESET_COUNTDOWNS.iter().find(|(n, _)| *n == name).map_or("", |(n, _)| n);
                info!("reset: {action} in {RESET_COUNT} s");
                self.nav.close();
                self.countdown = Some((action, Instant::now(), RESET_COUNT as u32));
                self.changed_screen = self.now();
            }
            // Network and All Settings run off the UI's thread under their
            // page; All Settings ends in a restart.
            "reset.network" | "reset.all" => {
                self.countdown = None;
                let all = name == "reset.all";
                info!("reset: {}", if all { "all settings, then a restart" } else { "network" });
                self.nav.close();
                self.set_alert(Some(if all { RESETTING_ALL } else { RESETTING_NETWORK }));
                let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let flag = done.clone();
                let file = self.settings.file().map(std::path::Path::to_path_buf);
                let writer = if all { self.settings.take_writer() } else { None };
                std::thread::spawn(move || {
                    crate::platform::relax();
                    let done = || flag.store(true, std::sync::atomic::Ordering::SeqCst);
                    if all {
                        drop(writer);
                        crate::reset::all(file.as_deref(), done);
                    } else {
                        crate::reset::network();
                        done();
                    }
                });
                self.resetting = Some(done);
            }
            // The schedule cleared (stock's 0xc01e0 with 0) or started over
            // as the basic one (with 1 or 2); Auto-Schedule erases it and
            // learns afresh.
            "reset.schedule.clear" | "reset.schedule.basic" | "reset.autoschedule" | "reset.away" => {
                self.countdown = None;
                match name {
                    "reset.away" => {
                        info!("reset: home/away assist trains again");
                        // Home/Away Assist trains again (stock's auto_away_reset).
                        self.settings.set(AWAY_DAYS, Value::Text("[]".into()));
                    }
                    "reset.schedule.basic" => self.basic_schedule(),
                    _ => {
                        info!("reset: schedule erased");
                        self.schedule = Default::default();
                    }
                }
                self.changed_screen = self.now();
            }
            // The timer's page of speeds: DONE starts the run it sets.
            "fan.timer.start" => {
                let minutes = self.settings.text(FAN_RUN).unwrap_or("15").to_string();
                info!("fan: run for {minutes} minutes at {}", self.settings.text(FAN_TIMER_SPEED).unwrap_or("stage1"));
                self.settings.change(FAN_TIMER, Value::Text(minutes));
                self.changed_screen = self.now();
            }
            "eco.start" | "eco.stop" => {
                // Started while off, the system comes on in its last mode
                // (stock's ecolayer: Eco on, then the system on).
                if name == "eco.start" && self.settings.text(MODE) == Some("off") {
                    self.turn_on();
                }
                self.settings.change(ECO, Value::Bool(name == "eco.start"));
                self.settings.set(ECO_AUTO, Value::Bool(false));
                self.nav.close();
                self.changed_screen = self.now();
            }
            // The rest reach the system once the client controls it; until
            // then they are only reported.
            _ => warn!("menu action: {name} is not done here"),
        }
    }

    #[cfg(test)]
    pub fn nav_form_open(&self) -> bool {
        self.nav.form_open()
    }

    /// The settings a form is editing, not to be sent until it is done.
    pub fn held_keys(&self) -> Vec<String> {
        if self.nav.form_open() { self.nav.form_keys(&self.menus()) } else { Vec::new() }
    }

    /// Whether the target earns the leaf: off and Eco always do, heating
    /// at or under the heat threshold, cooling at or over the cool one.
    pub fn leaf_on(&self) -> bool {
        let t = &self.thermostat;
        match self.settings.text(MODE).unwrap_or("heat") {
            // Eco earns it when its temperatures do (stock's leaf test on
            // the eco pair; an off side passes).
            _ if self.eco() => {
                let (low, high) = self.eco_temperatures();
                low.is_none_or(|l| l <= t.leaf.0.unwrap_or(18.5) + 0.01) && high.is_none_or(|h| h >= t.leaf.1.unwrap_or(26.0) - 0.01)
            }
            "off" => true,
            "heat" => t.target <= t.leaf.0.unwrap_or(18.5) + 0.01,
            "cool" => t.target >= t.leaf.1.unwrap_or(26.0) - 0.01,
            _ => false,
        }
    }

    /// Whether Eco is on.
    pub fn eco(&self) -> bool {
        matches!(self.settings.get(ECO), Some(Value::Bool(true)))
    }

    /// Whether the system heats and cools: stock's equipment flags 5 and 2
    /// (0x22ecd8: a first heat stage, W1 or O/B, and a first cool stage,
    /// Y1) from the wiring; nestlocal's `can_heat` and `can_cool` until the
    /// wiring is known.
    pub fn heats_cools(&self) -> (bool, bool) {
        use crate::hvac::wiring::Switch;
        match &self.hvac.wiring {
            Some(w) => (w.exists(Switch::Stage1Heat), w.exists(Switch::Stage1Cool)),
            None => (self.thermostat.can_heat, self.thermostat.can_cool),
        }
    }

    /// The Switchover alert's choices: the modes the system runs, OFF and
    /// CANCEL (stock's 0x1b8428: heat with equipment flag 5, cool with 2,
    /// HEAT • COOL with both).
    fn switchover_options(&self) -> &'static [&'static str] {
        match self.heats_cools() {
            (true, true) => &["HEAT", "COOL", "HEAT • COOL", "OFF", "CANCEL"],
            (false, true) => &["COOL", "OFF", "CANCEL"],
            _ => &["HEAT", "OFF", "CANCEL"],
        }
    }

    /// At start: with Wi-Fi not powered, the clock before `BUILD_DATE`
    /// and the time once set by hand, stock's DateTime alert (g2 0x49740:
    /// IsPowered, vtable +0x1b0, `date_time_adj_completed`).
    pub fn date_time_check(&mut self, now: f64, wifi_powered: bool) {
        if !wifi_powered && !time_valid(now) && self.settings.get(CLOCK_ADJUSTED) == Some(&Value::Bool(true)) {
            info!("alert: {DATE_TIME_TEXT}");
            self.date_alert = Some(0);
            self.changed_screen = self.now();
        }
    }

    /// Turns the system on in its last mode, or heat (cool on a system
    /// that only cools) when the system cannot run that.
    fn turn_on(&mut self) {
        let (heats, cools) = self.heats_cools();
        let runs = |m: &str| match m {
            "heat" => heats,
            "cool" => cools,
            "heat-cool" => heats && cools,
            _ => false,
        };
        let fallback = if heats || !cools { "heat" } else { "cool" };
        let mode = self.settings.text(MODE_ON).filter(|m| runs(m)).unwrap_or(fallback).to_string();
        self.settings.change(MODE, Value::Text(mode));
    }

    /// Minutes from Monday 00:00, local time.
    fn week_minute(&self) -> u32 {
        let c = clock_now(false);
        crate::schedule::week_minute(c.weekday, c.hour, c.minute)
    }

    /// Sets the target the schedule asks for now: at a setpoint's minute
    /// as the week goes by, or at once when `changed` (a setpoint edited
    /// here, as stock's takes effect).
    fn apply_schedule(&mut self, changed: bool) {
        let minute = self.week_minute();
        let due = self.week_minute.is_some_and(|last| last != minute) && self.schedule.find(minute).is_some();
        self.week_minute = Some(minute);
        if !(due || changed) || self.eco() {
            return;
        }
        let Some(p) = self.schedule.point_at(minute) else { return };
        debug!("schedule: {} setpoint {}", crate::schedule::when(p.minute), if changed { "edited" } else { "due" });
        if self.settings.text(MODE) == Some("heat-cool") {
            // A range setpoint sets both ends; a single temperature is not
            // one heat-cool follows.
            if let Some(high) = p.high {
                self.set_range(p.temp, high);
            }
        } else if (p.temp - self.thermostat.target).abs() >= 0.05 {
            self.schedule_set = true;
            self.set_target(p.temp);
        }
    }

    /// A range set by the schedule or Early-On, kept within the dial's
    /// range.
    fn set_range(&mut self, low: f32, high: f32) {
        let t = &mut self.thermostat;
        let (low, high) = (low.clamp(Thermostat::MIN, Thermostat::MAX), high.clamp(Thermostat::MIN, Thermostat::MAX));
        if (low - t.low).abs() >= 0.05 || (high - t.high).abs() >= 0.05 {
            (t.low, t.high) = (low, high);
            self.schedule_set = true;
            self.changed_target = self.now();
        }
    }

    /// Early-On (stock's 0x518de4 planner): before the next setpoint, start
    /// early by the time-to-temperature curve's estimate (5 h at most, or
    /// the night's limit for heat), then set the setpoint's temperatures
    /// at once, as stock does (both ends of a range).
    fn early_on(&mut self) {
        use crate::hvac::ttt::{COOL_SLOT, HEAT_SLOT};
        let mode = self.settings.text(MODE).unwrap_or("heat");
        // Heating on a heat pump with aux follows Heat Pump Balance (on
        // unless off) rather than Early-On's own setting (0x2fa488).
        let eq = &self.hvac.equipment;
        let early = self.settings.text("sense.early") == Some("on");
        let on = match mode {
            "heat" if eq.heat_pump && eq.aux_heat => eq.savings != 3,
            "heat" | "cool" | "heat-cool" => early,
            _ => false,
        };
        let enabled = on && !self.eco();
        if !enabled || self.schedule.points.is_empty() {
            self.early = None;
            return;
        }
        // SAFETY: time with a null pointer only returns the time.
        let now = unsafe { libc::time(std::ptr::null_mut()) } as f64;
        let c = clock_now(false);
        let minute = crate::schedule::week_minute(c.weekday, c.hour, c.minute);
        let Some(next) = self.schedule.points.iter().find(|p| p.minute > minute).or(self.schedule.points.first()).copied() else { return };
        let ahead = (next.minute + crate::schedule::WEEK - minute) % crate::schedule::WEEK;
        let due = now - f64::from(c.second) + f64::from(ahead) * 60.0;
        if self.early.as_ref().is_some_and(|e| e.due != due) {
            self.early = None;
        }
        let room = self.thermostat.current;
        // In heat-cool a range setpoint is heated to when its low is above
        // the room, else cooled to when its high is below it.
        let (temp, heat) = match (mode, next.high) {
            ("heat-cool", None) => {
                self.early = None;
                return;
            }
            ("heat-cool", Some(high)) => if next.temp > room || high >= room { (next.temp, true) } else { (high, false) },
            _ => (next.temp, mode == "heat"),
        };
        let e = self.early.get_or_insert(EarlyOn { due, next: temp, heat, start: None, cancelled: false });
        if e.cancelled {
            return;
        }
        if e.start.is_none() {
            let wanted = if e.heat { e.next > room } else { e.next < room };
            if !wanted {
                return;
            }
            let night = |t: f64| {
                // SAFETY: localtime_r fills the tm it is given.
                let hour = unsafe {
                    let secs = t as libc::time_t;
                    let mut tm: libc::tm = std::mem::zeroed();
                    libc::localtime_r(&secs, &mut tm);
                    tm.tm_hour
                };
                !(11..20).contains(&hour)
            };
            let night_cap = match self.settings.get("sense.early.hours") {
                Some(&Value::Number(h)) if h > 0.0 => Some(h * 3600.0),
                _ => None,
            };
            let cap = match night_cap {
                Some(n) if e.heat && (night(now) || night(due)) => n.min(18000.0),
                _ => 18000.0,
            };
            let lead = f64::from(self.hvac.ttt.seconds(if e.heat { HEAT_SLOT } else { COOL_SLOT }, (e.next - room).abs())).min(cap);
            if now >= (due - lead).max(now) && lead > 0.0 {
                info!("early-on: {} {:.0} min early for the {} setpoint", if e.heat { "heating" } else { "cooling" }, (due - now) / 60.0,
                      crate::schedule::when(next.minute));
                e.start = Some(now);
            }
            return;
        }
        // Started: the setpoint's own temperatures, at once.
        if mode == "heat-cool" {
            if let Some(high) = next.high {
                self.set_range(next.temp, high);
            }
        } else if (next.temp - self.thermostat.target).abs() >= 0.05 {
            self.schedule_set = true;
            self.set_target(next.temp);
        }
    }

    /// The energy history's sample, every 30 seconds (stock's 0x34fc0c).
    fn history_tick(&mut self) {
        if self.history_at.is_some_and(|at| at.elapsed() < Duration::from_secs(30)) {
            return;
        }
        self.history_at = Some(Instant::now());
        use crate::hvac::wiring::Switch::*;
        let h = &self.hvac;
        let sample = crate::history::Sample {
            heat: [Stage1Heat, Stage2Heat, Stage3Heat, Stage1AltHeat, Stage2AltHeat, AuxHeat, EmergencyHeat].iter().any(|&s| h.is_on(s)),
            cool: h.is_on(Stage1Cool) || h.is_on(FanCool),
            leaf: self.leaf_on(),
            off: self.settings.text(MODE) == Some("off"),
            manual_eco: self.eco(),
            weather: self.weather.is_some(),
            dry: h.decision.is_some_and(|d| d.controller == crate::hvac::control::Controller::CoolToDry),
            target: match self.hvac_mode() {
                crate::hvac::control::Mode::Heat(t) | crate::hvac::control::Mode::Emergency(t) => Some((true, t)),
                crate::hvac::control::Mode::Cool(t) => Some((false, t)),
                _ => None,
            },
            air: [Fan, Stage2Fan, Stage3Fan, FanCool, Stage1Cool, Stage2Cool, Stage3Cool].iter().any(|&s| h.is_on(s))
                || [Stage1Heat, Stage2Heat, Stage3Heat, Stage1AltHeat, Stage2AltHeat, AuxHeat, EmergencyHeat].iter()
                    .any(|&s| h.is_on(s) && !matches!(h.equipment.delivery[s as usize], 1 | 2)),
            scheduled: self.schedule.at(self.week_minute()).and_then(|t| match self.schedule_kind() {
                "HEAT" => Some((true, t)),
                "COOL" => Some((false, t)),
                _ => None,
            }),
        };
        // SAFETY: time with a null pointer only returns the time.
        let now = unsafe { libc::time(std::ptr::null_mut()) } as f64;
        self.history.sample(now, sample);
        let yesterday = self.history.yesterday();
        if self.settings.text("history.yesterday") != Some(yesterday.as_str()) {
            self.settings.set("history.yesterday", Value::Text(yesterday));
        }
        self.filter_page_tick();
        self.filter_tick();
        let any = !self.history.days.is_empty();
        if self.settings.get("history.any") != Some(&Value::Bool(any)) {
            self.settings.set("history.any", Value::Bool(any));
        }
    }

    /// Whether Early-On is bringing the room to the next setpoint, heating
    /// or cooling.
    pub fn preconditioning(&self) -> Option<bool> {
        self.early.as_ref().filter(|e| e.start.is_some() && !e.cancelled).map(|e| e.heat)
    }

    /// Where a heat or cool target sits by its shown whole degrees: -1 at
    /// or under 10 °C (51 °F), 1 at or over 31 °C (89 °F), else 0 (stock's
    /// 0x1d0f8c case 9, 0x1d8aa0).
    fn target_end(&self) -> i8 {
        let (whole, low, high) = if self.fahrenheit() {
            (to_fahrenheit(self.thermostat.target).round(), 51.0, 89.0)
        } else {
            (self.thermostat.target.floor(), 10.0, 31.0)
        };
        if whole <= low { -1 } else if whole >= high { 1 } else { 0 }
    }

    /// Stock's PRESS TO COOL and PRESS TO HEAT (0x1d0f8c case 9): heat set
    /// at its low end on a system that cools, cool at its high end on one
    /// that heats.
    fn press_to(&self) -> Option<&'static str> {
        match (self.settings.text(MODE).unwrap_or("heat"), self.target_end()) {
            ("heat", -1) if self.heats_cools().1 => Some("cool"),
            ("cool", 1) if self.heats_cools().0 => Some("heat"),
            _ => None,
        }
    }

    /// Stock's SAFETY over the number, as the gen 2 works it out (0x5f9dc;
    /// the gen 3's flag has no writer): the room within 0.39 °C of the low
    /// safety temperature on a system that heats, or within 0.56 °C of the
    /// high one on one that cools, while nothing else would act on it: off,
    /// Eco without a temperature on that side or with one past the safety
    /// temperature, or the other mode.
    fn safety_shown(&self) -> bool {
        if !self.local_sensor {
            return false;
        }
        let room = self.thermostat.current;
        let (low, high) = self.temperatures(SAFETY_LOW, SAFETY_HIGH);
        let (eco_low, eco_high) = self.eco_temperatures();
        let mode = self.settings.text(MODE).unwrap_or("heat");
        let (off, eco) = (mode == "off", self.eco());
        let (heats, cools) = self.heats_cools();
        let heat = low.is_some_and(|l| room < l + 0.39 && heats
            && (off || (eco && eco_low.is_none_or(|e| e < l)) || (!eco && mode == "cool")));
        let cool = high.is_some_and(|h| room > h - 0.56 && cools
            && (off || (eco && eco_high.is_none_or(|e| e > h)) || (!eco && mode == "heat")));
        heat || cool
    }

    /// A schedule nestlocal sent.
    pub fn set_schedule(&mut self, schedule: crate::schedule::Schedule) {
        self.schedule = schedule;
    }

    /// The schedule's type for the mode, as stock keeps one of each:
    /// RANGE in heat-cool, COOL in cool, HEAT otherwise.
    pub fn schedule_kind(&self) -> &'static str {
        match self.settings.text(MODE) {
            Some("heat-cool") => "RANGE",
            Some("cool") => "COOL",
            _ => "HEAT",
        }
    }

    /// Keeps a schedule of another type aside, for when the mode wants it.
    pub fn set_schedule_aside(&mut self, kind: &str, schedule: &crate::schedule::Schedule) {
        self.settings.set(&format!("schedule.{kind}"), Value::Text(schedule.to_days(kind, 0).to_string()));
    }

    /// The mode's schedule type changed: the old schedule goes aside and
    /// the new type's comes back (stock swaps its schedules so).
    fn swap_schedule(&mut self, old: &str, new: &str) {
        let old_schedule = std::mem::take(&mut self.schedule);
        self.set_schedule_aside(old, &old_schedule);
        if let Some(days) = self.settings.text(&format!("schedule.{new}")).and_then(|t| serde_json::from_str(t).ok()) {
            self.schedule = crate::schedule::Schedule::from_days(&days);
        }
    }

    /// Stock's basic schedule for each type, the mode's in use and the
    /// others aside (0x37b140 with 1 or 2, 0x37aea8): its kind by the unit
    /// and, on a Heat Link unit, the country (vt+0x500, 0xb98e8: 1 for
    /// Ireland and the UK).
    fn basic_schedule(&mut self) {
        let kind = if !self.heat_link() { 0 } else if matches!(self.settings.text(COUNTRY), Some("IE" | "GB")) { 1 } else { 2 };
        info!("reset: basic schedule {kind}");
        let current = self.schedule_kind();
        self.schedule = Default::default();
        for (name, schedule) in crate::schedule::basic(kind) {
            if name == current {
                self.schedule = schedule;
            } else {
                self.set_schedule_aside(name, &schedule);
            }
        }
    }

    /// A pair of temperature settings, low and high, each a number in
    /// Celsius or off.
    pub fn temperatures(&self, low: &str, high: &str) -> (Option<f32>, Option<f32>) {
        let side = |k: &str| match self.settings.get(k) {
            Some(&Value::Number(n)) => Some(n as f32),
            _ => None,
        };
        (side(low), side(high))
    }

    /// Eco's temperatures, low and high, each when used.
    pub fn eco_temperatures(&self) -> (Option<f32>, Option<f32>) {
        self.temperatures(ECO_HEAT, ECO_COOL)
    }

    /// How long since the last input.
    pub fn idle(&self) -> Duration {
        self.last_input.elapsed()
    }

    /// A target set from elsewhere, in Celsius, kept within the dial's range.
    pub fn set_target(&mut self, celsius: f32) {
        debug!("thermostat: target {celsius:.1}°C set from elsewhere");
        self.thermostat.target = celsius.clamp(Thermostat::MIN, Thermostat::MAX);
        self.changed_target = self.now();
    }

    /// The room as the backplate measures it; true when that changes the
    /// screen. The backplate's readings replace nestlocal's from then on.
    pub fn set_room(&mut self, celsius: Option<f32>, humidity: Option<f32>) -> bool {
        self.local_sensor = true;
        let before = (self.thermostat.current, self.thermostat.humidity);
        if let Some(c) = celsius {
            self.thermostat.current = c;
            self.hvac.room_read();
        }
        if humidity.is_some() {
            self.thermostat.humidity = humidity;
        }
        let shown = |c: f32| (c * 2.0).round();
        shown(before.0) != shown(self.thermostat.current) || before.1.map(f32::round) != self.thermostat.humidity.map(f32::round)
    }

    /// The thermostat's mode for the HVAC control, in Celsius.
    pub fn hvac_mode(&self) -> crate::hvac::control::Mode {
        use crate::hvac::control::Mode;
        let t = &self.thermostat;
        let mode = self.settings.text(MODE).unwrap_or("heat");
        // Emergency heat (stock's mode 4), with emergency heat equipment.
        let eq = &self.hvac.equipment;
        if self.settings.text(EMERGENCY) == Some("on") && eq.emergency_heat && mode != "off" {
            return Mode::Emergency(if mode == "heat-cool" { t.low } else { t.target });
        }
        if self.eco() {
            // Eco targets both its temperatures whatever the mode (stock's
            // 0x378f88), each clamped to stock's range.
            let (low, high) = self.eco_temperatures();
            let low = low.map(|t| t.clamp(4.44, 21.11));
            let high = high.map(|t| t.clamp(24.44, 32.0));
            return match (mode, low, high) {
                ("off", ..) | (_, None, None) => Mode::Off,
                (_, Some(low), Some(high)) => Mode::Range(low, high),
                (_, Some(low), None) => Mode::Heat(low),
                (_, None, Some(high)) => Mode::Cool(high),
            };
        }
        match mode {
            "heat" => Mode::Heat(t.target),
            "cool" => Mode::Cool(t.target),
            "heat-cool" => Mode::Range(t.low, t.high),
            _ => Mode::Off,
        }
    }

    /// A temperature report: what the HVAC switches.
    pub fn hvac_update(&mut self) {
        // What the control learned, kept across restarts.
        if !self.learned_loaded {
            self.learned_loaded = true;
            let mut l = self.hvac.learned();
            for (v, key) in l.iter_mut().zip(LEARNED) {
                if let Some(&Value::Number(n)) = self.settings.get(key) {
                    *v = n as f32;
                }
            }
            self.hvac.set_learned(l);
        }
        if !self.ttt_loaded {
            self.ttt_loaded = true;
            if let Some(text) = self.settings.text(TTT) {
                self.hvac.ttt.load(text);
            }
            if let Some(text) = self.settings.text(BALANCE) {
                self.hvac.balance.load(text);
            }
            if let Some(text) = self.settings.text(OVERSHOOT) {
                self.hvac.load_overshoot(text);
            }
        }
        // A manual change of target arms stock's time to temperature.
        let t = &self.thermostat;
        let targets = (t.target, t.low, t.high);
        if self.last_targets.is_some_and(|l| l != targets) {
            let manual = !self.schedule_set && !self.eco();
            self.hvac.manual_target = manual;
            if manual {
                self.hvac.ttt.arm();
                // A change by hand cancels Early-On until its setpoint;
                // one to the setpoint's own temperature (as nestlocal gives
                // back what Early-On set) does not.
                let (target, low, high) = targets;
                let heat_cool = self.settings.text(MODE) == Some("heat-cool");
                if let Some(e) = self.early.as_mut().filter(|e| e.start.is_some()) {
                    let own = if heat_cool { if e.heat { low } else { high } } else { target };
                    if (own - e.next).abs() >= 0.05 {
                        e.cancelled = true;
                    }
                }
            }
        }
        self.last_targets = Some(targets);
        self.schedule_set = false;
        let mode = self.hvac_mode();
        let room = self.thermostat.current;
        // The timer at its speed, else the schedule at its own.
        let eq = &self.hvac.equipment;
        let fan = if self.fan_running() { Some(eq.fan_timer_speed) } else if self.fan_scheduled() { Some(eq.fan_schedule_speed) } else { None };
        self.hvac.equipment.safety = self.temperatures(SAFETY_LOW, SAFETY_HIGH);
        self.hvac.equipment.fan_cooling = self.settings.text(AIRWAVE) != Some("off");
        self.hvac.own_target = !self.eco();
        // Stock's outdoor readings go after 7200 s (0x4edb5c).
        let fresh = self.weather_at.is_some_and(|at| at.elapsed() <= WEATHER_STALE);
        self.hvac.outdoor = self.weather.as_ref().filter(|_| fresh).map(|w| w.0);
        self.hvac.equipment.humidity.cool_to_dry = self.settings.text("sense.dry") == Some("on");
        self.hvac.update(mode, room, self.thermostat.humidity, fan);
        // Airwave's first run makes it ready (stock's 0x4db944 bookkeeping).
        let ready = matches!(self.settings.get(AIRWAVE_READY), Some(Value::Bool(true)));
        // Its alert comes with it, once (g2 0x4000006b: `fan_cooling_ready`
        // unset, then set).
        if !ready && self.hvac.fan_cooling() {
            info!("alert: {AIRWAVE_TEXT}");
            self.settings.set(AIRWAVE_READY, Value::Bool(true));
            self.airwave_alert = true;
            self.changed_screen = self.now();
        }
        let status = if matches!(self.settings.get(AIRWAVE_READY), Some(Value::Bool(true))) { "Ready" } else { "Learning" };
        if self.settings.text(AIRWAVE_STATUS) != Some(status) {
            self.settings.set(AIRWAVE_STATUS, Value::Text(status.into()));
        }
        let curves = self.hvac.ttt.save();
        if self.settings.text(TTT) != Some(curves.as_str()) {
            self.settings.set(TTT, Value::Text(curves));
        }
        let overshoot = self.hvac.overshoot();
        if self.settings.text(OVERSHOOT) != Some(overshoot.as_str()) {
            self.settings.set(OVERSHOOT, Value::Text(overshoot));
        }
        let balance = self.hvac.balance.save();
        if !balance.is_empty() && self.settings.text(BALANCE) != Some(balance.as_str()) {
            self.settings.set(BALANCE, Value::Text(balance));
        }
        for (v, key) in self.hvac.learned().iter().zip(LEARNED) {
            if self.settings.get(key) != Some(&Value::Number(f64::from(*v))) {
                self.settings.set(key, Value::Number(f64::from(*v)));
            }
        }
    }

    /// What should wake the unit if it sleeps now.
    /// The filter reminder (stock's 0x34e914): past the replacement
    /// threshold a filter is due and, with reminders on, the first alert
    /// shows; after REMIND LATER the second shows past its own threshold.
    fn filter_tick(&mut self) {
        let number = |k: &str| match self.settings.get(k) { Some(&Value::Number(n)) if n > 0.0 => Some(n as u64), _ => None };
        let threshold = number(FILTER_THRESHOLD).unwrap_or(FILTER_DEFAULT);
        let second = number(FILTER_SECOND);
        let on = self.settings.text("filter.remind") == Some("on");
        let f = self.history.filter;
        let mut level = match self.settings.get(FILTER_LEVEL) { Some(&Value::Number(n)) => n as u8, _ => 0 };
        let mut needed = self.settings.get(FILTER_NEEDED) == Some(&Value::Bool(true));
        if f.total > threshold || f.alert_sent {
            needed = true;
        }
        if on && !f.alert_sent && f.total > threshold && level < 1 {
            info!("filter: reminder up ({} h of {} h)", f.total / 3600, threshold / 3600);
            level = 1;
            self.filter_alert = Some((1, 0));
            self.changed_screen = self.now();
        } else if on && f.alert_sent && second.is_some_and(|t| f.total > t) && level < 2 {
            info!("filter: second reminder up ({} h)", f.total / 3600);
            level = 2;
            self.filter_alert = Some((2, 0));
            self.changed_screen = self.now();
        }
        for (key, v) in [(FILTER_LEVEL, Value::Number(f64::from(level))), (FILTER_NEEDED, Value::Bool(needed)), (FILTER_RUNTIME, Value::Number(f.runtime() as f64))] {
            if self.settings.get(key) != Some(&v) {
                self.settings.set(key, v);
            }
        }
    }

    /// The Reminders pages' words, and the month answered: the filter
    /// changed on this day of that month (stock's choice), reminders on.
    fn filter_page_tick(&mut self) {
        // SAFETY: time with a null pointer only returns the time; localtime_r
        // and mktime fill and read the tm they are given.
        let (now, mut tm) = unsafe {
            let now = libc::time(std::ptr::null_mut());
            let mut tm: libc::tm = std::mem::zeroed();
            libc::localtime_r(&now, &mut tm);
            (now, tm)
        };
        const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        for k in 1..=5 {
            let name = MONTHS[((tm.tm_mon - k).rem_euclid(12)) as usize];
            let key = format!("filter.m{k}");
            if self.settings.text(&key) != Some(name) {
                self.settings.set(&key, Value::Text(name.into()));
            }
        }
        let intro = if self.settings.text("filter.remind") == Some("on") { "Your thermostat will\nremind you to change\nyour air filters." }
                    else { "Changing air filters is a\nsimple way to save energy.\nYour thermostat can remind\nyou, based on usage." };
        if self.settings.text("filter.intro") != Some(intro) {
            self.settings.set("filter.intro", Value::Text(intro.into()));
        }
        let Some(back) = self.settings.text("filter.month").and_then(|m| m.parse::<i32>().ok()) else { return };
        self.settings.set("filter.month", Value::Text(String::new()));
        tm.tm_mon -= back;
        tm.tm_hour = 0;
        tm.tm_min = 0;
        tm.tm_sec = 0;
        tm.tm_isdst = -1;
        // SAFETY: mktime normalises the tm it is given.
        let date = unsafe { libc::mktime(&mut tm) };
        self.settings.set("filter.remind", Value::Text("on".into()));
        self.filter_changed(((now - date).max(0) / 86400) as u64);
        self.settings.set(FILTER_CHANGED, Value::Number(date as f64));
    }

    /// The filter was changed `days` ago: the reminder starts over.
    pub fn filter_changed(&mut self, days: u64) {
        let threshold = match self.settings.get(FILTER_THRESHOLD) { Some(&Value::Number(n)) if n > 0.0 => n as u64, _ => FILTER_DEFAULT };
        self.history.filter.changed(days, threshold);
        self.settings.set(FILTER_LEVEL, Value::Number(0.0));
        self.settings.set(FILTER_NEEDED, Value::Bool(false));
        // SAFETY: time with a null pointer only returns the time.
        let now = unsafe { libc::time(std::ptr::null_mut()) } as f64;
        self.settings.set(FILTER_CHANGED, Value::Number(now - days as f64 * 86400.0));
    }

    /// Sunblock's inputs and its rethink when due (stock's
    /// nlSunlightCorrection); ready and active go out as settings.
    fn sunblock_tick(&mut self) {
        use crate::sunblock::{Cause, Inputs, Sunblock};
        // SAFETY: time with a null pointer only returns the time.
        let now = unsafe { libc::time(std::ptr::null_mut()) } as f64;
        let on = self.settings.text("sense.sunblock") != Some("off") && self.local_sensor;
        let inputs = Inputs { enabled: on, temperature: self.local_sensor.then_some(self.thermostat.current) };
        let heating = {
            use crate::hvac::wiring::Switch::*;
            [Stage1Heat, Stage2Heat, Stage3Heat, Stage1AltHeat, Stage2AltHeat, AuxHeat, EmergencyHeat].iter().any(|&s| self.hvac.is_on(s))
        };
        let sb = self.sunblock.get_or_insert_with(|| {
            let kept = self.settings.text(SUNBLOCK_LEARNED).and_then(|t| serde_json::from_str(t).ok());
            Sunblock::new(now, kept, on)
        });
        sb.heating(now, heating);
        if let Some(t) = inputs.temperature {
            sb.temperature(now, t);
        }
        sb.light(now, self.light);
        if on != self.sunblock_on {
            self.sunblock_on = on;
            sb.rethink(now, Cause::Enabled, inputs);
        } else if sb.timer.is_some_and(|t| now >= t) {
            sb.rethink(now, Cause::Timer, inputs);
        }
        self.sunblock_out();
    }

    /// Sunblock's results: SetTempCompMode on a change, ready and active,
    /// and its learning when stock would write its recovery file.
    fn sunblock_out(&mut self) {
        let Some(sb) = self.sunblock.as_mut() else { return };
        let (comp, ready, active) = (sb.temp_comp, sb.ready, sb.active);
        let learned = std::mem::take(&mut sb.save).then(|| serde_json::to_string(sb.learned()).ok()).flatten();
        if self.sunblock_comp != comp {
            info!("sunblock: temperature compensation {}", if comp { "on" } else { "off" });
            self.sunblock_comp = comp;
            self.temp_comp = Some(comp);
        }
        // Its More info page's status (stock's sunblocklayer.readystate
        // and learningstate).
        let status = if ready { "Ready" } else { "Learning" };
        if self.settings.text("sunblock.status") != Some(status) {
            self.settings.set("sunblock.status", Value::Text(status.into()));
        }
        for (key, v) in [(SUNBLOCK_READY, ready), (SUNBLOCK_ACTIVE, active)] {
            if self.settings.get(key) != Some(&Value::Bool(v)) {
                self.settings.set(key, Value::Bool(v));
            }
        }
        if let Some(text) = learned {
            self.settings.set(SUNBLOCK_LEARNED, Value::Text(text));
        }
    }

    /// The backplate's light buffer, and its vergence wake, for Sunblock.
    pub fn light_buffer(&mut self, samples: &[u16]) {
        // SAFETY: time with a null pointer only returns the time.
        let now = unsafe { libc::time(std::ptr::null_mut()) } as f64;
        let inputs = crate::sunblock::Inputs { enabled: self.sunblock_on, temperature: self.local_sensor.then_some(self.thermostat.current) };
        let window = self.weather.as_ref().and_then(|(_, _, rise, set)| Some((day_bin((*rise)?), day_bin((*set)?))));
        if let Some(sb) = self.sunblock.as_mut() {
            sb.light_buffer(now, samples, inputs, window);
        }
        self.sunblock_out();
    }

    pub fn vergence(&mut self) {
        self.sunblock_rethink(crate::sunblock::Cause::Vergence);
    }

    /// Before a sleep, Sunblock's wake thresholds are made afresh.
    pub fn pre_sleep(&mut self) {
        self.sunblock_rethink(crate::sunblock::Cause::PreSleep);
    }

    /// The backplate reset: Sunblock starts its region afresh.
    pub fn backplate_reset(&mut self) {
        self.sunblock_rethink(crate::sunblock::Cause::Reset);
    }

    fn sunblock_rethink(&mut self, cause: crate::sunblock::Cause) {
        // SAFETY: time with a null pointer only returns the time.
        let now = unsafe { libc::time(std::ptr::null_mut()) } as f64;
        let inputs = crate::sunblock::Inputs { enabled: self.sunblock_on, temperature: self.local_sensor.then_some(self.thermostat.current) };
        if let Some(sb) = self.sunblock.as_mut() {
            sb.rethink(now, cause, inputs);
        }
        self.sunblock_out();
    }

    pub fn wake(&self) -> crate::backplate::link::Wake {
        let timer = self.fan_until.filter(|_| self.fan_running());
        let schedule = self.schedule().map(|s| {
            let now = std::time::SystemTime::now();
            let (minute, runs) = (local_minute(now), s.runs(local_minute(now)));
            let ahead = (1..=24 * 60).find(|m| s.runs((minute + m) % (24 * 60)) != runs).unwrap_or(24 * 60);
            let into = now.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() % 60);
            Instant::now() + Duration::from_secs(u64::from(ahead) * 60 - into)
        });
        let mut wake = self.hvac.wake(self.hvac_mode(), [timer, schedule].into_iter().flatten().min());
        if let Some(sb) = &self.sunblock {
            (wake.als, wake.vergence) = (sb.als_wake, sb.vergence);
        }
        wake
    }

    /// Legal Info's closing saying: one of stock's four, picked afresh
    /// each time the menus open.
    fn pick_saying(&mut self) {
        const SAYINGS: [&str; 4] = ["Brought to you from\nthe garage on Alma.", "My good friends\ncall me Harry.",
                                    "No squirrels.", "It's just a switch."];
        let n = self.now().to_bits() as usize % SAYINGS.len();
        self.settings.default("legal.saying", Value::Text(SAYINGS[n].into()));
    }

    /// The menus as the settings show them.
    fn menus(&self) -> std::rc::Rc<Tree> {
        let mut cache = self.menus_cache.borrow_mut();
        if let Some(cached) = cache.as_mut().filter(|c| c.tree_revision == self.tree_revision) {
            if cached.settings_revision == self.settings.revision() {
                return cached.tree.clone();
            }
            cached.settings_revision = self.settings.revision();
            if cached.values.iter().all(|(key, value)| self.settings.get(key) == value.as_ref()) {
                return cached.tree.clone();
            }
        }
        let keys = if let Some(cached) = cache.as_ref().filter(|c| c.tree_revision == self.tree_revision) {
            cached.values.iter().map(|(key, _)| key.clone()).collect()
        } else {
            let mut keys = std::collections::BTreeSet::new();
            menu_keys(&self.tree.items, &mut keys);
            keys.into_iter().collect::<Vec<_>>()
        };
        let values = keys.into_iter().map(|key| {
            let value = self.settings.get(&key).cloned();
            (key, value)
        }).collect();
        let tree = std::rc::Rc::new(Tree { items: crate::menu::shown(&self.tree.items, &self.settings), ..self.tree.clone() });
        *cache = Some(ShownCache { tree_revision: self.tree_revision, settings_revision: self.settings.revision(), values, tree: tree.clone() });
        tree
    }

    /// Whether the Fan choice is being edited while the fan runs.
    fn fan_editing(&self) -> bool {
        self.fan_running() && self.nav.editing_key(&self.menus()) == Some(FAN_TIMER)
    }

    /// The running fan timer as the skin shows it: minutes left, the
    /// share of its run left, and when it ends (its day too when that is
    /// a day or more away, as stock's); with a fan of more than one speed,
    /// the timer's speed in both (0x13e050, 0x1a973c).
    fn fan_view(&self) -> Option<crate::skin::FanView> {
        let until = self.fan_until.filter(|_| self.fan_running())?;
        let left = until.saturating_duration_since(Instant::now());
        let total = self.settings.text(FAN_TIMER)?.parse::<u64>().ok()? * 60;
        let end = local_time(std::time::SystemTime::now() + left);
        let at = clock_text(end.0, end.1, self.twelve_hour());
        let at = if left < Duration::from_secs(24 * 3600) { at } else { format!("{at} at {}", WEEKDAYS[end.2 as usize]) };
        let speed = self.fan_speed(self.hvac.equipment.fan_timer_speed);
        let left_text = time_left(left);
        Some(crate::skin::FanView {
            left_text: match speed {
                Some(speed) => format!("Running for the next {} at {speed} speed", left_text.trim_end_matches(" left")),
                None => left_text,
            },
            left: left.as_secs_f64() / total.max(1) as f64,
            until: match speed {
                Some(speed) => format!("On {speed} until {at}"),
                None => format!("Until {at}"),
            },
            speed: speed.is_some(),
        })
    }

    /// The name stock's texts give the fan's speed `stage` (1 to 3); None
    /// for a fan of one speed (stock's 0xcb60c: of two speeds the second
    /// is high).
    fn fan_speed(&self, stage: u8) -> Option<&'static str> {
        let stages = self.hvac.equipment.fan_stages;
        (stages > 1).then_some(match stage {
            1 => "low",
            2 if stages == 2 => "high",
            2 => "medium",
            _ => "high",
        })
    }

    /// What the top menu's Fan says under its name while the fan schedule
    /// is on, as stock's ring (0x1a973c): its minutes an hour and its
    /// hours, each left out when always, and its speed with a fan of more
    /// than one.
    fn fan_schedule_line(&self) -> Option<String> {
        let s = self.schedule()?;
        let twelve = self.twelve_hour();
        let (from, to) = (clock_text(s.start as u8, 0, twelve), clock_text(s.end as u8, 0, twelve));
        let speed = self.fan_speed(self.hvac.equipment.fan_schedule_speed);
        let on = speed.map_or("On".to_string(), |speed| format!("On {speed}"));
        Some(match (s.duty >= 60, s.start == s.end) {
            (true, true) => speed.map_or("Always on".to_string(), |speed| format!("Always on {speed}")),
            (true, false) => format!("{on} every day from {from} to {to}"),
            (false, true) => format!("{on} every day for {} mins/hr", s.duty),
            (false, false) => format!("{on} every day for {} mins/hr from {from} to {to}", s.duty),
        })
    }

    /// Whether the clock shows 12-hour time.
    fn twelve_hour(&self) -> bool {
        self.settings.text("display.clock") == Some("12h")
    }

    /// Whether the fan timer is running: the Fan choice set to a duration,
    /// counted from when it was chosen.
    pub fn fan_running(&self) -> bool {
        self.fan_until.is_some_and(|until| Instant::now() < until)
    }

    /// Whether the fan runs, for its timer or its schedule.
    pub fn fan_on(&self) -> bool {
        self.fan_running() || self.fan_scheduled()
    }

    /// Follows the fan schedule: switched on or changed, its runs start
    /// from this minute of the hour, as stock's do. True when the fan
    /// started or stopped.
    fn schedule_tick(&mut self) -> bool {
        let seen = (self.settings.text(FAN_SCHEDULE).map(str::to_string), self.settings.get(FAN_DUTY).cloned(),
                    self.settings.get(FAN_START).cloned(), self.settings.get(FAN_END).cloned());
        // The first look is the settings as kept: a restart keeps the runs' minute.
        if self.schedule_seen.as_ref().is_some_and(|s| *s != seen) && seen.0.as_deref() == Some("on") {
            let minute = local_minute(std::time::SystemTime::now()) % 60;
            self.settings.set(FAN_SINCE, Value::Number(f64::from(minute)));
        }
        self.schedule_seen = Some(seen);
        let on = self.fan_on();
        let changed = std::mem::replace(&mut self.fan_was_on, on) != on;
        if changed {
            info!("fan: {}", if self.fan_running() { "on, timer" } else if on { "on, schedule" } else { "off" });
        }
        changed
    }

    /// Keeps the clock's settings live: the time as shown ("clock.time"),
    /// Date & Time's tile ("clock.tile") and Set time's fields, which the
    /// menus' changes hold only while its form is open. True when the
    /// minute changed.
    fn clock_tick(&mut self) -> bool {
        // SAFETY: time with a null pointer only returns the time.
        let tm = local_tm(unsafe { libc::time(std::ptr::null_mut()) });
        let time = clock_text(tm.tm_hour as u8, tm.tm_min as u8, self.twelve_hour());
        let changed = self.settings.text("clock.time").is_some_and(|shown| shown != time);
        let tile = format!("{} {}\n{time}", MONTHS[tm.tm_mon as usize % 12], tm.tm_mday);
        self.settings.default("clock.tile", Value::Text(tile));
        self.settings.default("clock.time", Value::Text(time));
        let now = [tm.tm_mon + 1, tm.tm_mday, tm.tm_year + 1900, tm.tm_hour, tm.tm_min];
        for (key, value) in CLOCK_FIELDS.iter().zip(now) {
            self.settings.default(key, Value::Number(f64::from(value)));
            if !self.nav.form_open() {
                self.settings.forget(key);
            }
        }
        // Set time's time zones are the country's (stock's 0x1210a0), the
        // one in use chosen, else the first (0x121a88).
        let (group, list) = zones(self.settings.text(COUNTRY));
        self.settings.default(CLOCK_ZONES, Value::Text(group.into()));
        let zone = self.settings.text(TIME_ZONE).filter(|z| list.contains(z)).or(list.first().copied()).unwrap_or("");
        self.settings.default(CLOCK_ZONE, Value::Text(zone.to_string()));
        if !self.nav.form_open() {
            self.settings.forget(CLOCK_ZONE);
        }
        self.learning_days();
        changed
    }

    /// How long Auto-Schedule has been on in the mode in use, as its page
    /// words it: "0 days", "one day", "12 days". Stock keeps a count per
    /// mode (heat, cool, heat-cool: globals+0xc04, 0xc00, 0xc08, vt+0x114,
    /// 0x118, 0x11c) that its preference learner raises by one on each
    /// local day it learns in that mode (0x365f80, through vt+0x110); here
    /// each new local day on in a mode counts so, all dropped when
    /// Auto-Schedule is turned off.
    fn learning_days(&mut self) {
        // SAFETY: time with a null pointer only returns the time.
        let now = unsafe { libc::time(std::ptr::null_mut()) };
        let day = (now + local_tm(now).tm_gmtoff) / 86400;
        self.learning_days_on(day as f64);
    }

    fn learning_days_on(&mut self, today: f64) {
        let on = self.settings.text("sense.learning") == Some("on");
        let mode = self.settings.text(MODE).unwrap_or("heat").to_string();
        let modes = ["heat", "cool", "heat-cool"];
        if !on {
            for m in modes {
                self.settings.forget(&format!("{LEARNING_DAYS}.{m}"));
                self.settings.forget(&format!("{LEARNING_DAYS}.{m}.day"));
            }
        }
        let number = |s: &Settings, k: &str| match s.get(k) { Some(&Value::Number(n)) => Some(n), _ => None };
        let days = if on && modes.contains(&mode.as_str()) {
            let (count, last) = (format!("{LEARNING_DAYS}.{mode}"), format!("{LEARNING_DAYS}.{mode}.day"));
            let mut days = number(&self.settings, &count).unwrap_or(0.0);
            match number(&self.settings, &last) {
                Some(d) if d >= today => {}
                Some(_) => {
                    days += 1.0;
                    self.settings.set(&count, Value::Number(days));
                    self.settings.set(&last, Value::Number(today));
                }
                None => {
                    self.settings.set(&count, Value::Number(days));
                    self.settings.set(&last, Value::Number(today));
                }
            }
            days as u64
        } else {
            0
        };
        let text = match days {
            1 => "one day".to_string(),
            n => format!("{n} days"),
        };
        self.settings.default("sense.days", Value::Text(text));
    }

    /// Keeps Technical Info's facts ("tech.*") fresh while the menus are
    /// open, every two seconds.
    fn tech_tick(&mut self) -> bool {
        let ready = self.tech_results.lock().ok().and_then(|mut result| result.take());
        let changed = ready.is_some();
        if let Some((model, facts, networks, working)) = ready {
            self.tech_pending = false;
            self.model = Some(model);
            self.set_networks(networks, working);
            for (key, value) in facts {
                self.settings.default(key, Value::Text(value));
            }
        }
        let due = self.tech_at.is_none_or(|at| at.elapsed() >= Duration::from_secs(2));
        if !self.nav.is_open() || (!changed && (self.tech_pending || !due)) {
            return changed;
        }
        let model = self.model.as_deref().unwrap_or("");
        let mut facts: Vec<(&str, String)> = Vec::new();
        let t = &self.thermostat;
        let fahrenheit = self.settings.text(UNITS) == Some("F");
        let degrees = |c: f32| if fahrenheit { format!("{:.1}°F", c * 9.0 / 5.0 + 32.0) } else { format!("{c:.1}°C") };
        // POWER as stock shows it (0x1bcd30): Vusb on a Heat Link unit;
        // elsewhere Voc, or one "n/a" line while Voc or Vin is 0.
        let (v1, vin, current, state) = self.power;
        let heat_link = heat_link_unit(self.device, model, self.backplate_version);
        facts.push(("equipment.heat_link", if heat_link { "on" } else { "off" }.into()));
        // Technical Info's HEAT LINK only on a Heat Link unit (stock's
        // 0x1c0ff4: vtable +0x4b8, as EQUIPMENT's absence): a bare heading
        // until a Heat Link is paired, then its entry key.
        let paired = self.settings.text("tech.heatlink.key").is_some_and(|v| !v.is_empty());
        facts.push(("tech.heatlink", if !heat_link { "off" } else if paired { "key" } else { "heading" }.into()));
        let shown = heat_link || (v1 != 0 && vin != 0);
        let label = |l: &str| if shown { l.to_string() } else { String::new() };
        facts.push(("tech.v1.label", if heat_link { "Vusb".into() } else if shown { "Voc".into() } else { "Voc/Vin/Iin: n/a".into() }));
        facts.push(("tech.v1", if !shown { String::new() } else if heat_link { format!("{:.2} V", f64::from(v1) / 1000.0) } else { format!("{:.2} V", f64::from(v1) / 100.0) }));
        facts.push(("tech.vin.label", label("Vin")));
        facts.push(("tech.vin", format!("{:.2} V", f64::from(vin) / 100.0)));
        facts.push(("tech.iin.label", label("Iin")));
        facts.push(("tech.iin", format!("{current} mA ({})", ["o", "c", "i", "i", "a", "a", "a", "d", "d", "d"][usize::from(state.min(9))])));
        facts.push(("tech.current", degrees(t.current)));
        facts.push(("tech.target", degrees(t.target)));
        facts.push(("tech.humidity", t.humidity.map_or_else(|| "unknown".into(), |h| format!("{h:.0}%"))));
        facts.push(("tech.activity", format!("Long {}\nALS vis. {}\nTP: OK", self.motions, self.light)));
        facts.push(("tech.where", self.summary("placement.where").unwrap_or_else(|| "unknown".into())));
        // Equipment's "Your System": what the wires give, by stock's report
        // keys where its words mean other things elsewhere.
        let eq = &self.hvac.equipment;
        let mut parts: Vec<String> = Vec::new();
        // After Pro Setup, a Heat Link unit names its heating as stock's does.
        let setup = self.settings.text(BOILER).filter(|_| self.heat_link())
            .map(|boiler| crate::prosetup::system_lines(boiler, self.settings.text(HEAT_CONTROL).unwrap_or("onoff")));
        if let Some(lines) = setup {
            parts.extend(lines);
        } else if self.heats_cools().0 { parts.push("@wiringreportlayer.report.heating|Heating".into()); }
        if eq.x2_heat { parts.push("Heating stage 2".into()); }
        if eq.x3_heat { parts.push("Heating stage 3".into()); }
        if self.heats_cools().1 { parts.push("@wiringreportlayer.report.cooling|Cooling".into()); }
        if eq.x2_cool { parts.push("Cooling stage 2".into()); }
        if eq.x3_cool { parts.push("Cooling stage 3".into()); }
        if eq.has_fan { parts.push("@wiringreportlayer.report.fan|Fan".into()); }
        facts.push(("tech.system", if parts.is_empty() { "No HVAC Connected".into() } else { parts.join("\n") }));
        // EQUIPMENT, outside a Heat Link unit (stock's 0x1bc930): the
        // circuits sensed on, or No HVAC Connected, then the wires inserted
        // once the pins are known; five to a line.
        let lines = |names: &[&str], on: &dyn Fn(usize) -> bool| {
            let named: Vec<&str> = names.iter().enumerate().filter(|&(i, n)| !n.is_empty() && on(i)).map(|(_, n)| *n).collect();
            named.chunks(5).map(|c| c.join(" - ")).collect::<Vec<_>>().join("\n")
        };
        let wires = self.wires.unwrap_or_default();
        let sensed = lines(&SENSED_NAMES, &|i| wires.get(i).is_some_and(|w| w & 1 != 0));
        facts.push(("tech.sensed", if sensed.is_empty() { crate::i18n::tr("@systeminfolayer.nohvac.aw|No HVAC Connected") } else { sensed }));
        let pins = self.hvac.wiring.as_ref().map(|w| w.pins);
        facts.push(("tech.inserted.label", if pins.is_some() { crate::i18n::tr("@systeminfolayer.wiresinserted.aw|Wires inserted") } else { String::new() }));
        facts.push(("tech.inserted", pins.map(|p| lines(&INSERTED_NAMES, &|i| p.get(i).is_some_and(|&w| w != 0))).unwrap_or_default()));
        // DISPLAY's Time Zone: the zone's abbreviation (stock's 0xc2110,
        // strftime's %Z), when there is one.
        let zone = time_zone();
        facts.push(("tech.zone.label", if zone.is_empty() { String::new() } else { crate::i18n::tr("@systeminfolayer.timezone.aw|Time Zone") }));
        facts.push(("tech.zone", zone));
        if self.settings.get("tech.upload").is_none() {
            facts.push(("tech.upload", crate::i18n::tr("@systeminfolayer.never.aw|Never")));
        }
        // Software's "Updated %s", once an update is known (stock's 0x1f1184).
        let updated = match self.settings.get(SOFTWARE_UPDATED) {
            Some(&Value::Number(at)) => Some(crate::i18n::tr("@updatemenulayer.updatedateformat.aw|Updated %s").replacen("%s", &self.software_date(at), 1)),
            _ => None,
        };
        facts.push(("tech.software.dated", if updated.is_some() { "on" } else { "off" }.into()));
        facts.push(("tech.software.updated", updated.unwrap_or_default()));
        for (key, value) in facts {
            self.settings.default(key, Value::Text(value));
        }
        if due && !self.tech_pending {
            self.tech_at = Some(Instant::now());
            let model = self.model.clone();
            let results = self.tech_results.clone();
            self.tech_pending = true;
            std::thread::spawn(move || {
                crate::platform::relax();
                let model = model.unwrap_or_else(crate::techinfo::model);
                let facts = crate::techinfo::gather(&model);
                let networks = crate::wifi::services();
                let working = crate::wifi::working();
                if let Ok(mut result) = results.lock() {
                    *result = Some((model, facts, networks, working));
                }
            });
        }
        changed
    }

    /// What the menus' tile for `key` says of its value: its summary.
    fn summary(&self, key: &str) -> Option<String> {
        fn find<'a>(items: &'a [crate::menu::Item], key: &str) -> Option<&'a crate::menu::Item> {
            items.iter().find_map(|i| if i.shown.summarize.as_deref() == Some(key) { Some(i) } else if let crate::menu::Kind::Menu(c) = &i.kind { find(c, key) } else { None })
        }
        let value = self.settings.text(key)?;
        find(&self.tree.items, key)?.shown.summaries.iter().find(|(v, _)| v == value).map(|(_, s)| s.clone())
    }

    /// The backplate said who it is: Technical Info's BASE.
    pub fn backplate_identity(&mut self, id: &crate::backplate::link::Identity) {
        self.backplate_version = crate::backplate::link::backplate_version(&id.model);
        let facts = [("tech.base.software", format!("{} / {}", id.firmware_version, id.bsl_version)),
                     ("tech.base.model", id.model.clone()), ("tech.base.serial", id.serial.clone())];
        for (key, value) in facts {
            self.settings.default(key, Value::Text(value));
        }
    }

    /// nestlocal answered: Technical Info's last cloud communication.
    pub fn cloud_heard(&mut self) {
        self.settings.default("tech.cloud", Value::Text(now_stamp()));
    }

    /// The unit's state went to nestlocal: Technical Info's last data
    /// upload (stock's 0x1bf428: "Never" until then).
    pub fn data_uploaded(&mut self) {
        self.settings.default("tech.upload", Value::Text(now_stamp()));
    }

    /// Sets the system clock from Set time's fields.
    fn set_clock(&mut self) {
        // The zone first, the time then given in it (stock's 0x123700:
        // FUN_000d4bd8, then nlSetTime).
        let zone = self.settings.text(CLOCK_ZONE).filter(|z| zones(self.settings.text(COUNTRY)).1.contains(z)).map(str::to_string);
        if let Some(zone) = zone.filter(|z| self.settings.text(TIME_ZONE) != Some(z.as_str())) {
            if cfg!(any(test, feature = "desktop")) {
                info!("clock: time zone {zone} kept, not used on the desktop");
            } else {
                info!("clock: time zone {zone}");
                use_time_zone(&zone);
            }
            self.settings.change(TIME_ZONE, Value::Text(zone));
        }
        let field = |key: &str| match self.settings.get(key) {
            Some(&Value::Number(n)) => n as i32,
            _ => 0,
        };
        // SAFETY: tm is plain data, filled before mktime reads it.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        (tm.tm_mon, tm.tm_mday, tm.tm_year) = (field(CLOCK_FIELDS[0]) - 1, field(CLOCK_FIELDS[1]), field(CLOCK_FIELDS[2]) - 1900);
        (tm.tm_hour, tm.tm_min, tm.tm_isdst) = (field(CLOCK_FIELDS[3]), field(CLOCK_FIELDS[4]), -1);
        // SAFETY: mktime reads and normalises the tm it is given.
        let at = libc::timespec { tv_sec: unsafe { libc::mktime(&mut tm) }, tv_nsec: 0 };
        // SAFETY: at is a valid timespec for the call. The desktop's
        // clock is the host's, left alone.
        if cfg!(feature = "desktop") {
            info!("clock: not set on the desktop");
            self.settings.set(CLOCK_ADJUSTED, Value::Bool(true));
        } else if unsafe { libc::clock_settime(libc::CLOCK_REALTIME, &at) } != 0 {
            error!("clock: not set: {}", std::io::Error::last_os_error());
        } else {
            info!("clock: set to {}-{:02}-{:02} {:02}:{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min);
            self.settings.set(CLOCK_ADJUSTED, Value::Bool(true));
        }
        for key in CLOCK_FIELDS {
            self.settings.forget(key);
        }
    }

    /// Whether the fan schedule runs the fan now.
    pub fn fan_scheduled(&self) -> bool {
        self.schedule().is_some_and(|s| s.runs(local_minute(std::time::SystemTime::now())))
    }

    /// The fan schedule while it is on.
    fn schedule(&self) -> Option<FanSchedule> {
        if self.settings.text(FAN_SCHEDULE) != Some("on") {
            return None;
        }
        let number = |key: &str| match self.settings.get(key) {
            Some(&Value::Number(n)) => n.round() as u32,
            _ => 0,
        };
        Some(FanSchedule { duty: number(FAN_DUTY).clamp(1, 60), start: number(FAN_START) % 24, end: number(FAN_END) % 24,
                           anchor: number(FAN_SINCE) % 60 })
    }

    /// Follows the Fan choice: a duration starts the timer, Off stops it,
    /// and an expired timer sets it back to Off. When it ends is kept, so
    /// a restart carries on with the run. True when that changed.
    fn fan_tick(&mut self) -> bool {
        let chosen = self.settings.text(FAN_TIMER).and_then(|m| m.parse::<u64>().ok());
        let wall = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
        match (chosen, self.fan_until) {
            (Some(minutes), None) => {
                let ends = match self.settings.get(FAN_ENDS) {
                    Some(&Value::Number(ends)) if ends > 0.0 => ends,
                    _ => {
                        let ends = wall + (minutes * 60) as f64;
                        self.settings.set(FAN_ENDS, Value::Number(ends));
                        ends
                    }
                };
                if ends <= wall {
                    self.fan_stopped();
                } else {
                    self.fan_until = Some(Instant::now() + Duration::from_secs_f64(ends - wall));
                }
                true
            }
            (Some(_), Some(until)) if Instant::now() >= until => {
                self.fan_stopped();
                true
            }
            (None, Some(_)) => {
                self.fan_stopped();
                true
            }
            _ => false,
        }
    }

    /// The fan run as the device bucket keeps it: when it ends (seconds
    /// since the epoch, 0 for none) and its length in minutes.
    pub fn fan_run(&self) -> (f64, u64) {
        let ends = match self.settings.get(FAN_ENDS) {
            Some(&Value::Number(e)) if self.fan_running() => e,
            _ => 0.0,
        };
        (ends, self.settings.text(FAN_TIMER).and_then(|m| m.parse().ok()).unwrap_or(0))
    }

    /// A fan run set from elsewhere: until `ends` (0 stops it), `minutes` long.
    pub fn set_fan_run(&mut self, ends: f64, minutes: u64) {
        if ends <= 0.0 || minutes == 0 {
            if self.fan_until.is_some() || self.settings.text(FAN_TIMER) != Some("off") {
                self.fan_stopped();
            }
            return;
        }
        self.settings.set(FAN_TIMER, Value::Text(minutes.to_string()));
        self.settings.set(FAN_ENDS, Value::Number(ends));
        self.fan_until = None;
        self.fan_tick();
    }

    fn fan_stopped(&mut self) {
        self.fan_until = None;
        if self.settings.text(FAN_TIMER) != Some("off") {
            self.settings.set(FAN_TIMER, Value::Text("off".into()));
        }
        self.settings.set(FAN_ENDS, Value::Number(0.0));
    }

    /// Whether the room's readings come from the unit's own backplate.
    pub fn local_sensor(&self) -> bool {
        self.local_sensor
    }

    /// Time passing: the menus close after a while without input, and later
    /// the display sleeps. True when that changed the screen.
    pub fn tick(&mut self) -> bool {
        let mut clock = self.clock_tick();
        if self.nav.is_open() {
            let menus = self.menus();
            let moving = self.nav.tick(&menus, Instant::now());
            clock |= moving;
            // Settings come to rest on in the ring: the update is seen
            // (stock's 0x1a71b8); its line stays until the menus open again.
            if !moving && self.software_unseen && self.nav.pointed(&menus) == Some((1, "Settings")) {
                if let Some(at) = self.settings.get(SOFTWARE_UPDATED).cloned() {
                    self.settings.set(SOFTWARE_SEEN, at);
                }
            }
        }
        let now = Instant::now();
        if let Some(editor) = self.editor.as_mut() {
            clock |= editor.tick(now, &self.schedule);
        }
        if let Some((kb, _)) = self.keyboard.as_mut() {
            clock |= kb.tick(now);
        }
        if let Some(view) = self.energy.as_mut() {
            clock |= view.tick(now);
        }
        if self.leaving.is_some() && !self.leaving() {
            self.leaving = None;
            self.near = None;
            clock = true;
        }
        // Farsight's analog clock moves its second hand every second.
        if self.far && self.settings.text(FARSIGHT) == Some("analog_clock") {
            let second = clock_now(false).second;
            clock |= self.far_second.replace(second) != Some(second);
        }
        clock |= self.tech_tick();
        clock |= self.values_tick();
        clock |= self.systest_tick();
        // A reset's countdown, a second at a time, then the reset.
        if let Some((action, at, shown)) = self.countdown {
            let left = RESET_COUNT - at.elapsed().as_secs_f64().floor();
            if left < 0.0 {
                self.action(action);
                return true;
            }
            if left as u32 != shown {
                self.countdown = Some((action, at, left as u32));
                return true;
            }
        }
        if self.resetting.as_ref().is_some_and(|d| d.load(std::sync::atomic::Ordering::SeqCst)) {
            self.resetting = None;
            let all = self.alert == Some(RESETTING_ALL);
            info!("reset: done{}", if all { "; restarting" } else { "" });
            self.set_alert(if all { Some(GOODBYE) } else { None });
            return true;
        }
        self.network_tick();
        if self.nav.is_open() && self.nav.expire(&self.menus()) {
            self.changed_screen = self.now();
            return true;
        }
        if self.fan_tick() || self.schedule_tick() {
            return true;
        }
        let before = self.thermostat.target;
        self.apply_schedule(false);
        self.early_on();
        self.emergency_tick();
        self.weather_offline();
        self.equipment_tick();
        self.setpoint_tick();
        self.history_tick();
        self.sunblock_tick();
        self.away_tick();
        if self.power_tick() | self.short_cycle_tick() | self.furnace_tick() | self.heat_link_tick() {
            return true;
        }
        if before != self.thermostat.target {
            return true;
        }
        let idle = self.last_input.elapsed();
        if self.range.is_some() && (idle >= MENU_IDLE || self.settings.text(MODE) != Some("heat-cool")) {
            debug!("home: range's end let go");
            self.range = None;
            self.changed_target = self.now();
            return true;
        }
        let quiet = self.last_presence.map_or(idle, |p| idle.min(p.elapsed()));
        // A stage under test keeps its page up; its own limit ends it.
        let testing = self.systest.as_ref().is_some_and(crate::systemtest::SystemTest::testing);
        let held = self.remote_until.is_some_and(|until| Instant::now() < until);
        if !self.asleep && quiet >= self.display_idle && !testing && !held {
            debug!("display: no one for {} s; sleeping", quiet.as_secs());
            self.nav.abandon_form(&mut self.settings);
            self.nav.close();
            self.editor = None;
            self.energy = None;
            self.keyboard = None;
            self.postal = None;
            self.ask = None;
            self.pro = None;
            self.equip = None;
            self.close_systest();
            self.asleep = true;
            self.far = false;
            self.near = None;
            self.leaving = None;
            self.changed_screen = self.now();
            return true;
        }
        // Someone in the room keeps the menus open, as stock does.
        if (self.nav.is_open() || self.editor.is_some() || self.energy.is_some() || self.keyboard.is_some() || self.postal.is_some() || self.pro.is_some() || self.equip.is_some()
            || self.systest.is_some()) && quiet >= MENU_IDLE && !testing {
            debug!("menu: no one for {} s; closing", quiet.as_secs());
            self.nav.abandon_form(&mut self.settings);
            self.nav.close();
            self.editor = None;
            self.energy = None;
            self.keyboard = None;
            self.postal = None;
            self.ask = None;
            self.pro = None;
            self.equip = None;
            self.close_systest();
            self.changed_screen = self.now();
            return true;
        }
        clock
    }

    /// connman's services read again, and what the Network page shows of
    /// them: each network as name|wifiN|flags (c connected, s saved, p
    /// secured), the connected one, and "Searching" until some show, "No
    /// networks found yet." after 15 s (stock's wifilayer).
    fn read_networks(&mut self) {
        if self.network_pending { return }
        self.network_pending = true;
        self.wifi_read = Some(Instant::now());
        let result = self.network_results.clone();
        std::thread::spawn(move || {
            crate::platform::relax();
            let networks = crate::wifi::services();
            let working = crate::wifi::working();
            if let Ok(mut ready) = result.lock() {
                *ready = Some((networks, working));
            }
        });
    }

    fn set_networks(&mut self, networks: Vec<crate::wifi::Service>, working: bool) {
        self.wifi_read = Some(Instant::now());
        self.networks = networks;
        let n = &self.networks;
        let connected = n.iter().find(|s| s.connected());
        let ssid = if !working { "Wi-Fi error".into() } else { connected.map(|s| s.name.chars().take(30).collect::<String>()).unwrap_or_else(|| "Not connected".into()) };
        let list = n.iter().map(|s| format!("{}|wifi{}|{}{}{}", s.name, s.level(), if s.connected() { "c" } else { "" },
                                            if s.saved { "s" } else { "" }, if s.secured() { "p" } else { "" })).collect::<Vec<_>>().join("\n");
        let status = if !n.is_empty() {
            self.searching = None;
            ""
        } else if self.searching.get_or_insert_with(Instant::now).elapsed() >= Duration::from_secs(15) {
            "No networks found yet."
        } else {
            "Searching"
        };
        let level = connected.map_or(0.0, |s| f64::from(s.level()));
        if let Some(s) = connected {
            if self.settings.text(WIFI_HOME) != Some(s.name.as_str()) {
                self.settings.set(WIFI_HOME, Value::Text(s.name.clone()));
            }
        }
        for (key, v) in [("net.ssid", Value::Text(ssid)), ("net.list", Value::Text(list)), ("net.status", Value::Text(status.into())),
                         ("net.connected", Value::Bool(connected.is_some())), ("net.level", Value::Number(level))] {
            if self.settings.get(key) != Some(&v) {
                self.settings.set(key, v);
            }
        }
        self.settings_notice();
        self.test_gate();
    }

    /// What the top menu's Settings says under its name, as stock's
    /// selector (0x1a8a4c): Wi-Fi not working, then no internet, which
    /// stock's nlNetworkConnectivity::IsConnected counts a service connman
    /// has ready as having (no notice on nest-qemu, whose services never
    /// pass connman's online check), with a system issue between them,
    /// then a software update not yet seen there (0x1a8f2c). Stock shows
    /// none of the first three, nor their badge, in its demo mode (globals
    /// vtable +0x1e8, config `initialstate` "demoview"), which the client
    /// has not.
    fn settings_notice(&mut self) {
        let notice = if self.settings.text("net.ssid") == Some("Wi-Fi error") { "Wi-Fi connection error".to_string() }
            else if self.short_cycle { "System issue".to_string() }
            else if !self.networks.iter().any(|s| s.state == "online" || s.state == "ready") { "Internet disconnected".to_string() }
            else if let (true, Some(&Value::Number(at))) = (self.software_unseen, self.settings.get(SOFTWARE_UPDATED)) {
                crate::i18n::tr("@selectorlayer.softwareupdated|Software updated on %s").replacen("%s", &self.software_date(at), 1)
            } else { String::new() };
        if self.settings.text(SETTINGS_NOTICE) != Some(notice.as_str()) {
            self.settings.default(SETTINGS_NOTICE, Value::Text(notice));
            self.changed_menu = self.now();
        }
    }

    /// When the software was updated (`at`, seconds since the epoch), as
    /// stock's selector and Software page write it.
    fn software_date(&self, at: f64) -> String {
        crate::history::short_date(at as i64, self.settings.text("display.language").unwrap_or(crate::i18n::ENGLISH))
    }

    /// The software running at start (`version`, at `now` seconds since
    /// the epoch): another version than the one last run is an update
    /// then; none recorded (a fresh install) is none.
    pub fn software_running(&mut self, version: &str, now: f64) {
        self.settings.default("tech.software", Value::Text(version.split_whitespace().next().unwrap_or_default().into()));
        let last = self.settings.text(SOFTWARE_VERSION).map(str::to_string);
        if last.as_deref() == Some(version) {
            return;
        }
        if let Some(last) = last {
            info!("software: updated, {last} -> {version}");
            self.settings.set(SOFTWARE_UPDATED, Value::Number(now.floor()));
        }
        self.settings.set(SOFTWARE_VERSION, Value::Text(version.into()));
    }

    /// As the menus open: whether Settings says when the software was
    /// updated, while that update is not the one last seen there.
    fn software_shown(&mut self) {
        let updated = self.settings.get(SOFTWARE_UPDATED);
        self.software_unseen = updated.is_some() && updated != self.settings.get(SOFTWARE_SEEN);
        self.settings_notice();
    }

    /// Wi-Fi as time passes: the services read every second while a page
    /// follows connman, every 2 s with the Network list open and a scan
    /// asked for every 10 s there (stock's timers); a network chosen from
    /// the list.
    fn network_tick(&mut self) {
        let ready = self.network_results.lock().ok().and_then(|mut result| result.take());
        if let Some((networks, working)) = ready.as_ref() {
            self.network_pending = false;
            self.set_networks(networks.clone(), *working);
        }
        self.recover_tick();
        let listing = self.nav.is_open() && self.nav.within(&self.menus(), "Network");
        let following = matches!(self.wifi.page, Some(crate::network::Page::Connecting { .. } | crate::network::Page::Disconnecting { .. }));
        let every = if following { 1 } else { 2 };
        if (following || listing) && self.wifi_read.is_none_or(|t| t.elapsed() >= Duration::from_secs(every)) {
            self.read_networks();
        }
        if ready.is_some() && (following || listing) {
            let acts = self.wifi.tick(&self.networks, self.wall());
            self.wifi_acts(acts);
            self.changed_menu = self.now();
            // A refused passphrase on the configured network (0x1a).
            if let Some(crate::network::Page::Failed { name, key: true, .. }) = &self.wifi.page {
                if self.settings.text(WIFI_HOME) == Some(name.as_str()) && self.wifi_alert.is_none() {
                    let alert = WifiAlert::Password(name.clone());
                    info!("wifi: alert: {}", one_line(&alert.text()));
                    self.wifi_alert = Some((alert, 0));
                }
            }
        }
        // Opening Network with Wi-Fi not working: the fault alert (0x30).
        if listing && self.wifi_scan.is_none() && self.settings.text("net.ssid") == Some("Wi-Fi error") && self.wifi_alert.is_none() {
            info!("wifi: alert: {}", one_line(&WifiAlert::Fault.text()));
            self.wifi_alert = Some((WifiAlert::Fault, 0));
            self.changed_screen = self.now();
        }
        if !listing {
            self.wifi_scan = None;
        }
        if listing && self.wifi_scan.is_none_or(|t| t.elapsed() >= Duration::from_secs(10)) {
            self.wifi_scan = Some(Instant::now());
            crate::wifi::scan();
        }
        let Some(name) = self.settings.text("net.join").filter(|s| !s.is_empty()).map(str::to_string) else { return };
        self.settings.set("net.join", Value::Text(String::new()));
        debug!("wifi: {name} chosen from the list");
        let (acts, ask) = self.wifi.choose(&self.networks, &name, self.wall());
        self.wifi_acts(acts);
        self.wifi_ask(ask);
        self.changed_screen = self.now();
    }

    /// Offline, the recoverer tries the configured network again, and
    /// the alerts come: not found after its failures, disconnected for
    /// an hour, a day, and on. Back online, they go.
    fn recover_tick(&mut self) {
        let now = self.wall();
        let Some(home) = self.settings.text(WIFI_HOME).map(str::to_string) else { return };
        let joining = self.wifi.page.is_some() || self.keyboard.is_some();
        if self.offline_since.is_some() && self.wifi_read.is_none_or(|t| t.elapsed() >= Duration::from_secs(RECOVER_EVERY as u64)) && !joining {
            self.read_networks();
        }
        if let Some(on) = self.networks.iter().find(|s| s.connected()) {
            if self.offline_since.take().is_some() {
                info!("wifi: back on {}", on.name);
                self.recover = (0, RECOVER_LIMIT, 0.0);
                self.offline_told = 0;
                if matches!(self.wifi_alert, Some((WifiAlert::NotFound(_) | WifiAlert::Disconnected(_), _))) {
                    self.wifi_alert = None;
                    self.changed_screen = self.now();
                }
            }
            return;
        }
        if self.offline_since.is_none() {
            info!("wifi: lost {home}");
        }
        let since = *self.offline_since.get_or_insert(now);
        if joining {
            return;
        }
        let hours = ((now - since) / 3600.0) as u64;
        let tell = hours >= 1 && (hours < 24 && hours > self.offline_told || hours >= 24 && hours / 24 > self.offline_told / 24);
        if tell && self.wifi_alert.is_none() {
            self.offline_told = hours;
            info!("wifi: alert: {}", one_line(&WifiAlert::Disconnected(hours).text()));
            self.wifi_alert = Some((WifiAlert::Disconnected(hours), 0));
            self.changed_screen = self.now();
        }
        let (fails, limit, at) = self.recover;
        if now < at {
            return;
        }
        if fails >= limit {
            if self.wifi_alert.is_none() {
                info!("wifi: alert: {}", one_line(&WifiAlert::NotFound(home.clone()).text()));
                self.wifi_alert = Some((WifiAlert::NotFound(home.clone()), 0));
                self.changed_screen = self.now();
            }
            self.recover = (0, RECOVER_AFTER_ALERT, now + RECOVER_WAIT);
            return;
        }
        self.recover = (fails + 1, limit, now + RECOVER_EVERY);
        debug!("wifi: reconnecting to {home}, try {} of {limit}", fails + 1);
        if let Some(s) = self.networks.iter().find(|s| s.name == home) {
            crate::wifi::connect(&s.path);
        }
    }

    /// A room of one's own for Placement (stock's nlNameLayer): a known
    /// room's name picks it; another gets a new lowercase GUID, listed with
    /// the rooms from then on.
    /// Plugins' values: their commands run off the UI's thread, at start
    /// and every 2 s while the menus are open; what they print is each
    /// key's value, kept as a default so it is never saved. True when one
    /// changed.
    fn values_tick(&mut self) -> bool {
        let mut changed = false;
        let fresh = self.values.lock().ok().and_then(|mut v| v.take());
        for (key, out) in fresh.into_iter().flatten() {
            if self.settings.get(&key) != Some(&Value::Text(out.clone())) {
                debug!("plugins: {key} = {out:?}");
                self.settings.default(&key, Value::Text(out));
                changed = true;
            }
        }
        let due = self.values_at.is_none_or(|t| self.nav.is_open() && t.elapsed() >= Duration::from_secs(2));
        if due && !self.tree.values.is_empty() {
            self.values_at = Some(Instant::now());
            let commands = self.tree.values.clone();
            let out = self.values.clone();
            std::thread::spawn(move || {
                crate::platform::relax();
                let read = commands.into_iter().map(|(key, command)| {
                    let text = std::process::Command::new("sh").args(["-c", &command]).output()
                        .map(|o| String::from_utf8_lossy(&o.stdout).trim_end().to_string()).unwrap_or_default();
                    (key, text)
                }).collect();
                if let Ok(mut v) = out.lock() {
                    *v = Some(read);
                }
            });
        }
        if changed {
            self.changed_menu = self.now();
        }
        changed
    }

    /// Pro Setup's Your system: what it set kept, onto Equipment
    /// Settings, or with TEST into the system test.
    fn pro_done(&mut self, setup: crate::prosetup::Setup, test: bool) {
        info!("equipment: pro setup done");
        self.pro = None;
        // Off the warning, onto Equipment Settings.
        self.nav.up();
        for (key, value) in [(BOILER, setup.boiler), (HEAT_CONTROL, setup.control), (HEAT_SOURCE, setup.source),
                             (HEAT_DELIVERY, setup.delivery), (HOT_WATER_CONTROL, setup.hot_water)] {
            self.settings.change(key, Value::Text(value));
        }
        if test {
            self.open_systest();
        }
        self.changed_screen = self.now();
    }

    /// The system test opens on its gate or its list.
    fn open_systest(&mut self) {
        self.test_gate();
        let gate = match self.settings.text("test.gate") {
            Some("wifi") => Some("wifi"),
            Some("weather") => Some("weather"),
            _ => None,
        };
        let setup = crate::systemtest::Setup {
            wiring: self.hvac.wiring(),
            equipment: &self.hvac.equipment,
            heads_up: self.settings.get(FURNACE_HEADSUP_TEST) == Some(&Value::Bool(true)),
            heat_link: self.heat_link(),
            gate,
        };
        self.systest = Some(crate::systemtest::SystemTest::open(&setup));
        self.changed_screen = self.now();
        self.changed_menu = self.now();
    }

    /// The system test closes: its request withdrawn.
    fn close_systest(&mut self) {
        if let Some(mut test) = self.systest.take() {
            test.close();
            self.hvac.test = None;
            self.changed_screen = self.now();
        }
    }

    /// What the system test reads: the time, the fresh outside
    /// temperature, the control's hold on its request and the HVAC power.
    fn systest_env(&self) -> crate::systemtest::Env {
        let fresh = self.weather_at.is_some_and(|at| at.elapsed() <= WEATHER_STALE);
        crate::systemtest::Env {
            now: Instant::now(),
            outdoor: self.weather.as_ref().filter(|_| fresh).map(|w| w.0),
            fahrenheit: self.fahrenheit(),
            held: self.hvac.test_held(),
            powered: self.wires.is_none_or(|w| w[9] & 1 != 0 || w[10] & 1 != 0),
        }
    }

    /// The system test as time passes: it stops for whatever takes the
    /// screen from it (an alert, a dialog, an error page); its request
    /// follows it. True when its page changed.
    fn systest_tick(&mut self) -> bool {
        if self.systest.is_none() {
            return false;
        }
        if self.screen() != "systest" {
            info!("systest: {} took the screen; the test stops", self.screen());
            self.close_systest();
            return true;
        }
        let env = self.systest_env();
        let changed = self.systest.as_mut().is_some_and(|t| t.tick(&env));
        self.hvac.test = self.systest.as_ref().and_then(crate::systemtest::SystemTest::request);
        changed
    }

    /// What stock's system test waits for: Wi-Fi, then weather.
    fn test_gate(&mut self) {
        let gate = if !matches!(self.settings.get("net.connected"), Some(Value::Bool(true))) { "wifi" }
            else if self.weather.is_none() { "weather" } else { "" };
        if self.settings.text("test.gate") != Some(gate) {
            self.settings.set("test.gate", Value::Text(gate.into()));
            self.settings.set("test.blocked", Value::Bool(!gate.is_empty()));
        }
    }

    /// Keeps the location given and leaves Location for the Settings
    /// tiles, as stock does.
    fn set_location(&mut self, iso: &str, postal: &str) {
        self.settings.change(COUNTRY, Value::Text(iso.to_string()));
        self.settings.change(POSTAL, Value::Text(postal.to_string()));
        let tree = self.menus();
        while self.nav.within(&tree, "Location") {
            self.nav.up();
        }
    }

    fn custom_place(&mut self, name: &str) {
        let tree = self.menus();
        let known = |items: &[crate::menu::Item]| items.iter().find(|i| i.label.eq_ignore_ascii_case(name)).and_then(|i| match &i.kind {
            crate::menu::Kind::Set { key, value, .. } if key == "placement.where" => Some(value.clone()),
            _ => None,
        });
        let rooms = tree.items.iter().filter_map(|i| match &i.kind { crate::menu::Kind::Menu(c) => Some(c), _ => None })
            .flat_map(|c| c.iter()).find(|i| i.label == "Placement").and_then(|i| match &i.kind { crate::menu::Kind::Menu(c) => Some(c.clone()), _ => None });
        let mut customs: Vec<String> = self.settings.text("placement.customs").unwrap_or("").lines().map(str::to_string).collect();
        let id = rooms.as_deref().and_then(known)
            .or_else(|| customs.iter().find_map(|l| l.split('|').next().filter(|n| n.eq_ignore_ascii_case(name)).and(l.rsplit('|').next()).map(str::to_string)))
            .unwrap_or_else(|| {
                let mut b = [0u8; 16];
                if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
                    let _ = std::io::Read::read_exact(&mut f, &mut b);
                }
                b[6] = b[6] & 0x0f | 0x40;
                b[8] = b[8] & 0x3f | 0x80;
                let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
                let id = format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32]);
                customs.push(format!("{name}|||{id}"));
                id
            });
        self.settings.set("placement.customs", Value::Text(customs.join("\n")));
        self.settings.change("placement.where.name", Value::Text(name.to_string()));
        self.settings.set("placement.where", Value::Text(id));
    }

    /// A menu entry's keyboard or boxes, opened on what is kept.
    fn open_ask(&mut self, ask: crate::menu::Ask) {
        let stored = self.settings.text(&ask.key).unwrap_or("").to_string();
        match &ask.boxes {
            Some(pattern) => self.postal = Some(crate::postal::Entry::pattern(pattern, &ask.prompt, &stored)),
            None => self.keyboard = Some((crate::keyboard::Keyboard::open(&stored, ask.max, &ask.prompt), ASKED)),
        }
        self.ask = Some(ask);
        self.changed_screen = self.now();
    }

    /// What was asked for, kept and given to the entry's command.
    fn answer(&mut self, text: String) {
        let Some(ask) = self.ask.take() else { return };
        if let Some(command) = &ask.run {
            match std::process::Command::new("sh").args(["-c", command]).env("VALUE", &text).spawn() {
                Ok(mut child) => {
                    std::thread::spawn(move || child.wait());
                }
                Err(e) => error!("run: {command}: {e}"),
            }
        }
        self.settings.change(&ask.key, Value::Text(text));
    }

    /// The keyboard for what the Wi-Fi pages ask.
    fn wifi_ask(&mut self, ask: Option<crate::network::Ask>) {
        if let Some(crate::network::Ask::Passphrase { name, saved, hidden }) = ask {
            self.joining = Some((name, hidden));
            let kb = crate::keyboard::Keyboard::open_masked(saved.as_deref().unwrap_or(""), 256, "Password");
            self.keyboard = Some((kb, WIFI_PSK));
            self.changed_screen = self.now();
        }
    }

    /// Does what the Wi-Fi pages ask of connman.
    fn wifi_acts(&mut self, acts: Vec<crate::network::Act>) {
        use crate::network::Act;
        for act in acts {
            match act {
                Act::Provision { name, passphrase, hidden } => {
                    info!("wifi: joining {name}{}", if hidden.is_some() { " (hidden)" } else { "" });
                    if let Err(e) = crate::wifi::provision(&name, passphrase.as_deref(), hidden) {
                        error!("wifi: {e}");
                    }
                    // A hidden network shows once a scan probes for it.
                    if hidden.is_some() {
                        crate::wifi::scan();
                    }
                }
                Act::Connect(path) => {
                    debug!("wifi: connecting {path}");
                    crate::wifi::connect(&path);
                }
                Act::Autoconnect(path, on) => {
                    debug!("wifi: autoconnect {path} {}", if on { "on" } else { "off" });
                    crate::wifi::autoconnect(&path, on);
                }
                Act::Disconnect(path) => {
                    info!("wifi: disconnecting {path}");
                    crate::wifi::disconnect(&path);
                }
            }
        }
        self.wifi_read = None;
    }

    fn wall(&self) -> f64 {
        #[cfg(test)]
        if let Some(at) = self.wall_at {
            return at;
        }
        // SAFETY: time with a null pointer only returns the time.
        unsafe { libc::time(std::ptr::null_mut()) as f64 }
    }

    /// Stock's `is_furnace_shutdown`, which it sends up as it changes.
    pub fn furnace_shutdown(&self) -> bool {
        self.furnace_shutdown
    }

    #[cfg(test)]
    pub fn set_furnace_shutdown(&mut self, on: bool) {
        self.furnace_shutdown = on;
    }

    /// Opens the History screen on yesterday.
    pub fn open_history(&mut self) {
        self.nav.close();
        let days = self.history.days.len().min(crate::history::SHOWN);
        self.energy = Some(crate::history::View::open(days));
        self.changed_screen = self.now();
    }

    /// Lets a flung timeline or row of cards come to rest at once, as the
    /// next seconds would, for a skin check to draw where they stop.
    pub fn settle(&mut self) {
        let now = Instant::now();
        for k in 1..=600 {
            let t = now + Duration::from_millis(17 * k);
            let editor = self.editor.as_mut().is_some_and(|e| e.tick(t, &self.schedule));
            let energy = self.energy.as_mut().is_some_and(|v| v.tick(t));
            let keyboard = self.keyboard.as_mut().is_some_and(|(kb, _)| kb.tick(t));
            if !editor && !energy && !keyboard {
                break;
            }
        }
    }

    /// The clicks the Schedule and History screens have due, which they
    /// sound as their wheels move them rather than a click a wheel event.
    pub fn take_clicks(&mut self) -> u32 {
        let editor = self.editor.as_mut().map_or(0, |e| std::mem::take(&mut e.clicks));
        editor + self.energy.as_mut().map_or(0, |v| std::mem::take(&mut v.clicks))
    }

    /// The Wi-Fi page's view (stock's connectlayer, networktypelayer).
    fn wifi_view(&self, p: &crate::network::Page) -> crate::skin::WifiView {
        use crate::network::{Page, SECURITY};
        const PHASES: [&str; 3] = ["Connecting to", "Getting Addresses", "Connecting to Internet"];
        let help = |phase: u8| format!("See nest.com/w{}", phase.clamp(1, 3) + 1);
        let v = crate::skin::WifiView::default();
        match p {
            Page::Connecting { name, phase, stop, .. } => crate::skin::WifiView { page: "connecting", name: name.clone(),
                text: PHASES[usize::from(phase.clamp(&1, &3) - 1)].into(), stop: *stop, ..v },
            Page::Online { name } => crate::skin::WifiView { page: "online", name: name.clone(), text: "This thermostat is online".into(),
                hint: "Press to continue".into(), ..v },
            Page::Failed { name, key: true, .. } => crate::skin::WifiView { page: "failed", name: name.clone(),
                text: "Password incorrect\nfor this network".into(), hint: "Press to go back".into(), ..v },
            Page::Failed { name, phase, .. } => crate::skin::WifiView { page: "failed", name: name.clone(), text: "Failed to connect".into(),
                help: help(*phase), hint: "Press to continue".into(), ..v },
            Page::Security { at, .. } => crate::skin::WifiView { page: "security", text: "Network security?".into(),
                choices: SECURITY.to_vec(), at: *at, ..v },
            Page::Disconnecting { .. } => crate::skin::WifiView { page: "disconnecting", text: "Disconnecting from Wi-Fi network".into(), ..v },
        }
    }

    /// The History screen's view (stock's nlEnergyView): the days oldest
    /// first, and the detail of the one open.
    fn history_view(&self, v: &crate::history::View) -> crate::skin::HistoryView {
        use crate::history::{duration, exact, message, title, HALF_DAY};
        let shown = &self.history.days[..v.days];
        let text = |s: u32| if s == 0 { "NO USAGE".to_string() } else { duration(s) };
        let days = shown.iter().rev().map(|d| crate::skin::HistoryDay {
            title: title(d.date), leaf: d.leaf_earned, missing: d.missing > HALF_DAY, heat: d.heat, cool: d.cool,
            heat_text: text(d.heat), cool_text: text(d.cool),
        }).collect();
        let (heat, cool) = (shown.iter().any(|d| d.heat > 0), shown.iter().any(|d| d.cool > 0));
        let usage = match (heat, cool) {
            (true, true) => 2,
            (true, false) => 0,
            (false, true) => 1,
            _ => -1,
        };
        let detail = v.detail.zip(v.day()).map(|(scroll, i)| {
            let d = &shown[i];
            let reason = (d.heat + d.cool > 0 && d.above_by.abs() >= 4 && d.reason >= 0).then_some(d.reason);
            crate::skin::HistoryDetail { scroll, heat_text: exact(d.heat), cool_text: exact(d.cool), cycles: d.cycles.clone(), reason, message: message(d) }
        });
        crate::skin::HistoryView { at: v.at, pos: f64::from(v.position()), days, usage, detail }
    }

    /// The Schedule screen's view: the setpoints in the shown unit.
    fn schedule_view(&self, e: &crate::schedule::Editor, shown: impl Fn(f32) -> f32) -> crate::skin::ScheduleView {
        use crate::schedule::Doing;
        let point = |p: &crate::schedule::Setpoint| (p.minute, shown(p.temp), p.high.map(&shown));
        let editing = match &e.doing {
            Doing::Time(_) => Some("time"),
            Doing::Temp(_) => Some("temp"),
            Doing::High(_) => Some("high"),
            _ => None,
        };
        let pending = e.pending(Instant::now()).map(|p| point(&p));
        crate::skin::ScheduleView {
            cursor: e.cursor,
            held: e.held,
            scroll: f64::from(e.scroll),
            points: self.schedule.points.iter().map(point).collect(),
            starts: self.schedule.day_starts().into_iter().map(|t| t.map(&shown)).collect(),
            pending,
            editing,
            menu: match &e.doing {
                Doing::Menu { choices, at } => Some((choices.to_vec(), *at)),
                _ => None,
            },
            now: self.week_minute(),
        }
    }

    /// The view a skin draws, for a face of `face_px` pixels drawn in `design` units.
    /// The screen showing, as the skin is told it.
    pub fn screen(&self) -> &'static str {
        if self.alert.is_some() || self.countdown.is_some() { "alert" } else if self.starting { "starting" } else if self.notice.is_some() { "notice" }
        else if self.wiring_error.or(self.report).is_some() && !(self.tech_open && self.nav.is_open()) { "error" }
        else if self.eco_prompt.is_some() || self.emergency_alert.is_some() || self.off_alert.is_some() || self.date_alert.is_some() || self.switchover_alert.is_some() || self.airwave_alert || self.filter_alert.is_some() || self.wifi_alert.is_some() || self.short_cycle_alert || self.battery_alert || self.furnace_alert { "dialog" }
        else if self.systest.is_some() { "systest" }
        else if self.editor.is_some() { "schedule" } else if self.energy.is_some() { "history" } else if self.keyboard.is_some() { "keyboard" } else if self.postal.is_some() { "postal" } else if self.wifi.page.is_some() { "wifi" } else if self.nav.is_open() { "menu" }
        else if self.far || self.leaving() { "farsight" } else { "home" }
    }

    /// Farsight is still giving way to the home screen.
    fn leaving(&self) -> bool {
        let time = if self.settings.text(FARSIGHT) == Some("analog_clock") { LEAVE_ANALOG } else { LEAVE_TIME };
        self.leaving.is_some_and(|at| self.now() < at + time)
    }

    /// The hourly forecast nestlocal gave, by hour from now (1 to 48): its
    /// time (epoch s) and temperature (°C); true when that changes.
    pub fn set_forecast(&mut self, hours: Vec<Option<(i64, f32)>>) -> bool {
        let changed = hours != self.forecast;
        self.forecast = hours;
        changed
    }

    /// The three hours Farsight's weather forecasts as someone comes near,
    /// as stock picks them (0x142f3c): the first more than 3 hours away,
    /// then 4 and 8 hours on, each from 1 to 7 in the morning moved on to
    /// 8 with those after it. Their temperature as shown and their time.
    fn forecast_view(&self, now: i64, shown: impl Fn(f32) -> f32) -> Vec<(f32, String)> {
        let at = |i: usize| self.forecast.get(i).copied().flatten();
        let Some(first) = (0..48).find(|&i| at(i).is_some_and(|(time, _)| time - now > 3 * 3600)) else { return Vec::new() };
        let mut hours = [first.min(43), 0, 0];
        hours[1] = hours[0] + 4;
        hours[2] = hours[1].min(43) + 4;
        let twelve = self.settings.text("display.clock") == Some("12h");
        let mut view = Vec::new();
        for k in 0..3 {
            let Some((time, _)) = at(hours[k]) else { return Vec::new() };
            let hour = crate::history::hour_of(time);
            if (1..=7).contains(&hour) {
                for h in &mut hours[k..] {
                    *h = (*h + 8 - hour as usize).min(47);
                }
            }
            let Some((time, temp)) = at(hours[k]) else { return Vec::new() };
            view.push((shown(temp), crate::history::forecast_time(time, twelve)));
        }
        view
    }

    /// The local time, with Farsight's date lines while someone is near.
    fn clock(&self) -> crate::skin::Clock {
        let mut clock = clock_now(self.settings.text("display.clock") == Some("12h"));
        if self.near.is_some() {
            // SAFETY: time with a null pointer only returns the time.
            let now = unsafe { libc::time(std::ptr::null_mut()) } as i64;
            clock.weekday_date = Some(crate::history::weekday_date(now, self.settings.text("display.language").unwrap_or(crate::i18n::ENGLISH)));
            clock.weekday_day = Some(crate::history::weekday_day(now));
        }
        clock
    }

    /// Where the menus are, for the log.
    pub fn menu_place(&self) -> Option<String> {
        self.nav.place(&self.menus())
    }

    /// What the thermostat is set to, for the log: the mode and the
    /// temperatures it works to, in the scale shown.
    pub fn set_to(&self) -> String {
        use crate::hvac::control::Mode;
        let f = self.fahrenheit();
        let t = |c: f32| if f { format!("{:.0}°F", to_fahrenheit(c)) } else { format!("{c:.1}°C") };
        let eco = if self.eco() { ", eco" } else { "" };
        match self.hvac_mode() {
            Mode::Off => format!("off{eco}"),
            Mode::Heat(c) => format!("heat to {}{eco}", t(c)),
            Mode::Cool(c) => format!("cool to {}{eco}", t(c)),
            Mode::Emergency(c) => format!("emergency heat to {}", t(c)),
            Mode::Range(low, high) => format!("heat-cool {} to {}{eco}", t(low), t(high)),
        }
    }

    pub fn ui(&self, design: f32, face_px: f32) -> Ui {
        let f = self.fahrenheit();
        let shown = |c: f32| if f { to_fahrenheit(c) } else { c };
        let mode = self.settings.text(MODE).unwrap_or("heat").to_string();
        // In Eco the dial shows the Eco temperature the mode works to.
        let eco = self.eco().then(|| match self.hvac_mode() {
            crate::hvac::control::Mode::Heat(t) | crate::hvac::control::Mode::Range(t, _) => Some(t),
            crate::hvac::control::Mode::Cool(t) => Some(t),
            _ => None,
        });
        // The lockout counted down under the number, as stock's 0x1cd760:
        // the compressor's from at most 150 s, a minimum cycle's from at
        // most cycle_cap's.
        let cycle_cap = cycle_cap(self.hvac.heads_up());
        let lockout = self.hvac.lockout().filter(|_| self.local_sensor).map(|(kind, end)| {
            let left = end.saturating_duration_since(Instant::now()).as_secs_f64().ceil() as u64;
            match kind {
                crate::hvac::control::Lockout::Compressor => ("compressor", left.min(150)),
                crate::hvac::control::Lockout::MinimumCycle => ("cycle", left.min(cycle_cap)),
            }
        });
        Ui {
            now: self.now(),
            screen: self.screen(),
            schedule: self.editor.as_ref().map(|e| self.schedule_view(e, shown)),
            history: self.energy.as_ref().map(|v| self.history_view(v)),
            wifi: self.wifi.page.as_ref().map(|p| self.wifi_view(p)),
            keyboard: self.keyboard.as_ref().map(|(kb, _)| {
                use crate::keyboard::Key;
                crate::skin::KeyboardView {
                    keys: kb.keys().iter().map(|k| match k {
                        Key::Char(c) => c.to_string(),
                        Key::Shift => "shift".into(),
                        Key::Space => "space".into(),
                        Key::Delete => "delete".into(),
                        Key::Page => "page".into(),
                        Key::Done => "done".into(),
                        Key::Cancel => "cancel".into(),
                    }).collect(),
                    at: kb.at,
                    page: kb.page,
                    text: kb.shown(),
                    placeholder: kb.placeholder.clone(),
                    page_label: kb.page_label(),
                }
            }),
            postal: self.postal.as_ref().map(|e| crate::skin::PostalView {
                prompt: e.prompt(),
                chars: e.chars.iter().map(|c| c.map_or("-".into(), |c| if c == ' ' { String::new() } else { c.to_string() })).collect(),
                set: e.chars.iter().map(Option::is_some).collect(),
                at: e.at,
                gap: e.gap,
            }),
            dialog: self.wifi_alert.as_ref().map(|(alert, choice)| crate::skin::Dialog {
                text: alert.text(),
                options: alert.options(),
                selected: *choice,
                icon: Some("wifi"),
                ..Default::default()
            }).or(self.filter_alert.map(|(level, choice)| crate::skin::Dialog {
                text: "It's time to change\nyour air filter.".into(),
                options: if level == 1 { &["REMIND LATER", "DONE"] } else { &["DONE"] },
                selected: choice,
                ..Default::default()
            })).or(self.emergency_alert.map(|choice| crate::skin::Dialog {
                text: "EMERGENCY HEAT IS ON\nTurn it off to continue.".into(),
                options: &["LEAVE ON", "TURN OFF"],
                selected: choice,
                ..Default::default()
            })).or(self.off_alert.map(|choice| crate::skin::Dialog {
                text: "This thermostat is off.\n\nDo you want to turn this thermostat on?".into(),
                options: &["LEAVE OFF", "TURN ON"],
                selected: choice,
                icon: Some("mode"),
                ..Default::default()
            })).or(self.date_alert.map(|choice| crate::skin::Dialog {
                text: DATE_TIME_TEXT.into(),
                options: &["OK", "SET DATE/TIME"],
                selected: choice,
                ..Default::default()
            })).or(self.switchover_alert.map(|choice| crate::skin::Dialog {
                text: SWITCHOVER_TEXT.into(),
                options: self.switchover_options(),
                selected: choice,
                checked: self.switchover_options().iter().position(|o| *o == "OFF"),
                ..Default::default()
            })).or(self.airwave_alert.then(|| crate::skin::Dialog {
                text: AIRWAVE_TEXT.into(),
                options: &["OK"],
                icon: Some("airwave"),
                ..Default::default()
            })).or(self.eco_prompt.map(|stop| crate::skin::Dialog {
                text: "To adjust the\ntemperature, stop using\nEco Temperatures.".into(),
                options: &["STOP", "CANCEL"],
                selected: if stop { 0 } else { 1 },
                ..Default::default()
            })).or(self.short_cycle_alert.then(|| crate::skin::Dialog {
                text: SHORT_CYCLE_TEXT.into(),
                // OK (common string 3); CONTINUE (2) on the way into
                // Equipment (0x156a60).
                options: if self.equipment_alert.is_some() { &["CONTINUE"] } else { &["OK"] },
                icon: Some("issue"),
                title: Some(SHORT_CYCLE_TITLE),
                link: Some("nest.com/m13"),
                scrolled: self.alert_scrolled,
                ..Default::default()
            })).or(self.battery_alert.then(|| crate::skin::Dialog {
                text: BATTERY_DRAINED_TEXT.into(),
                // OK; CONTINUE and BACK (2, 0) on the way into Equipment
                // (0x157440).
                options: if self.equipment_alert.is_some() { &["CONTINUE", "BACK"] } else { &["OK"] },
                selected: self.equipment_alert.unwrap_or(0),
                icon: Some("issue"),
                title: Some(BATTERY_DRAINED_TITLE),
                link: Some("nest.com/m13"),
                scrolled: self.alert_scrolled,
                ..Default::default()
            })).or(self.furnace_alert.then(|| crate::skin::Dialog {
                // No heading, U+E034, OK; CONTACT A PRO before it only with
                // a Nest Pro's contact, which nothing here has (0x147734).
                text: FURNACE_SHUTDOWN_TEXT.into(),
                options: &["OK"],
                icon: Some("message"),
                link: Some("nest.com/furnace"),
                scrolled: self.alert_scrolled,
                ..Default::default()
            })),
            alert: self.countdown.and_then(|(action, _, left)| RESET_COUNTDOWNS.iter().find(|(n, _)| *n == action)
                    .map(|(_, text)| crate::i18n::tr(text).replacen("%d", &left.to_string(), 1)))
                .or_else(|| self.alert.map(str::to_string)),
            far: self.far || self.leaving(),
            notice: self.notice,
            error: self.wiring_error.or(self.report).map(|e| (e, self.error_choice)),
            fan: self.fan_view(),
            fan_on: self.fan_on(),
            fan_schedule: self.fan_schedule_line(),
            design,
            face_px,
            thermostat: ThermostatView {
                target: shown(eco.flatten().unwrap_or(self.thermostat.target)),
                eco: eco.is_some(),
                farsight: match self.settings.text(FARSIGHT) {
                    Some("current_temp") => "current_temp",
                    Some("analog_clock") => "analog_clock",
                    Some("digital_clock") => "digital_clock",
                    Some("weather") => "weather",
                    _ => "target_temp",
                },
                starts_in: self.hvac.starts_in().and(lockout).map(|(_, left)| left),
                delayed_for: self.hvac.delayed_for().and(lockout).map(|(_, left)| left),
                lockout: lockout.map(|(kind, _)| kind),
                current: shown(self.thermostat.current),
                min: shown(Thermostat::MIN),
                max: shown(Thermostat::MAX),
                unit: if f { "F" } else { "C" },
                hvac: if self.local_sensor { self.hvac.state() } else { self.thermostat.hvac(&mode).name() },
                stage: if self.local_sensor { self.hvac.stage() } else { 1 },
                drying: self.local_sensor && self.hvac.decision.is_some_and(|d| d.controller == crate::hvac::control::Controller::CoolToDry),
                emergency: matches!(self.hvac_mode(), crate::hvac::control::Mode::Emergency(_)),
                aux: self.local_sensor && self.hvac.is_on(crate::hvac::wiring::Switch::AuxHeat),
                alt: if !self.local_sensor { 0 } else if self.hvac.is_on(crate::hvac::wiring::Switch::Stage2AltHeat) { 2 }
                     else { u8::from(self.hvac.is_on(crate::hvac::wiring::Switch::Stage1AltHeat)) },
                airwave: self.local_sensor && self.hvac.fan_cooling(),
                preconditioning: self.preconditioning().map(|heat| if heat { "heat" } else { "cool" }),
                safety: self.safety_shown(),
                system_issue: self.battery_drained,
                press_to: self.press_to(),
                // Without the bucket's thresholds, stock's defaults: 65 °F heating, 79 °F cooling.
                leaf: (shown(self.thermostat.leaf.0.unwrap_or(18.5)), shown(self.thermostat.leaf.1.unwrap_or(26.0))),
                leaf_on: self.leaf_on(),
                nudge: self.nudge * if self.fahrenheit() { 1.0 } else { 0.5 },
                sunblock: self.settings.get(SUNBLOCK_ACTIVE) == Some(&Value::Bool(true)),
                outdoor: self.weather.as_ref().map(|w| shown(w.0)),
                time_to_target: self.time_to_target(),
                weather_image: self.weather_image(),
                online: matches!(self.settings.get("net.connected"), Some(Value::Bool(true))),
                // SAFETY: time with a null pointer only returns the time.
                forecast: if self.near.is_some() { self.forecast_view(unsafe { libc::time(std::ptr::null_mut()) } as i64, shown) } else { Vec::new() },
                mode,
                low: shown(self.thermostat.low),
                high: shown(self.thermostat.high),
                humidity: self.thermostat.humidity,
                can_heat: self.heats_cools().0,
                can_cool: self.heats_cools().1,
                range: self.range.map(|(side, _)| side.name()),
                range_editing: self.range.is_some_and(|(_, editing)| editing),
            },
            test: self.systest.as_ref().map(crate::systemtest::SystemTest::view),
            menu: self.pro.as_ref().map(crate::prosetup::ProSetup::view).or_else(|| self.equip.as_ref().map(crate::equipment::Page::view)).or_else(|| self.nav.view(&self.menus(), &self.settings)),
            clock: self.clock(),
            changed_screen: self.changed_screen,
            changed_target: self.changed_target,
            turned_target: self.turned_target,
            changed_menu: self.changed_menu,
            changed_near: self.near,
            changed_leave: self.leaving,
        }
    }
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September",
                            "October", "November", "December"];
/// Set time's fields: the local date and time, live until changed.
const CLOCK_FIELDS: [&str; 5] = ["clock.month", "clock.day", "clock.year", "clock.hour", "clock.minute"];
/// Set time's time zone field, its country's group of zones (the menus'
/// `clock.zones=…`), and the zone in use, kept (stock's zone file).
const CLOCK_ZONE: &str = "clock.zone";
const CLOCK_ZONES: &str = "clock.zones";
const TIME_ZONE: &str = "clock.tz";

/// A country's group of time zones and their zone files, as stock's Set
/// time offers them (0x1210a0 over the table 0x122014): none for a
/// country it does not know.
fn zones(country: Option<&str>) -> (&'static str, &'static [&'static str]) {
    match country {
        Some("US") => ("US", &["Pacific/Honolulu", "America/Anchorage", "America/Los_Angeles", "America/Denver", "America/Phoenix",
                              "America/Chicago", "America/New_York", "America/Puerto_Rico"]),
        Some("CA") => ("CA", &["America/Los_Angeles", "America/Denver", "America/Chicago", "America/Regina", "America/New_York",
                              "America/Puerto_Rico", "America/St_Johns"]),
        Some("MX") => ("MX", &["America/Cancun", "America/Mexico_City", "America/Mazatlan", "America/Tijuana"]),
        Some("RU") => ("RU", &["Europe/Kaliningrad", "Europe/Moscow", "Europe/Samara", "Asia/Yekaterinburg", "Asia/Omsk",
                              "Asia/Krasnoyarsk", "Asia/Irkutsk", "Asia/Yakutsk", "Asia/Vladivostok", "Asia/Magadan", "Asia/Kamchatka"]),
        Some("SA" | "AE") => ("ME", &["Asia/Riyadh", "Asia/Dubai"]),
        Some("AU") => ("AU", &["Australia/Perth", "Australia/Adelaide", "Australia/Darwin", "Australia/Sydney", "Australia/Brisbane"]),
        Some("NZ") => ("NZ", &["Pacific/Auckland"]),
        Some("CN") => ("CN", &["Asia/Shanghai"]),
        Some("AT" | "BE" | "BG" | "HR" | "CZ" | "DK" | "EE" | "FI" | "FR" | "DE" | "GR" | "HU" | "IE" | "IT" | "LV" | "LT" | "LU" | "NL"
             | "NO" | "PL" | "PT" | "RO" | "SK" | "SI" | "ES" | "SE" | "CH" | "GB") => ("EU", &["Europe/London", "Europe/Berlin", "Europe/Tallinn"]),
        _ => ("none", &[]),
    }
}

/// Puts the client's local time in a zone file's zone (stock's
/// FUN_000d4bd8, nlSetTimeZoneFile).
fn use_time_zone(zone: &str) {
    extern "C" {
        fn tzset();
    }
    std::env::set_var("TZ", zone);
    // SAFETY: tzset only rereads TZ.
    unsafe { tzset() };
}
const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/// The most a minimum cycle's countdown starts from (stock's 0x1cd760):
/// 1800 s while the compressor heads-up is in a cool state or resting
/// (`compressor_heads_up_state` 4–9, 0xc82f4), else 300 s.
fn cycle_cap(heads_up: crate::hvac::headsup::State) -> u64 {
    use crate::hvac::headsup::State::*;
    if matches!(heads_up, CoolWatch | CoolRetest | CoolBlock | Rest | LongRest) { 1800 } else { 300 }
}

/// What is left of a run as the fan page words it (stock's 0x13dd1c):
/// minutes rounded up below 3541 s, hours to the nearest below 84601 s,
/// days to the nearest up to 561600 s, then 1 week.
fn time_left(left: Duration) -> String {
    let s = left.as_secs_f64().ceil() as u64;
    let (n, unit) = if s < 3541 {
        (s.div_ceil(60), "minute")
    } else if s < 84601 {
        ((s + 1800) / 3600, "hour")
    } else if s <= 561_600 {
        ((s + 43200) / 86400, "day")
    } else {
        (1, "week")
    };
    format!("{n} {unit}{} left", if n == 1 { "" } else { "s" })
}

/// A time of day as the clock shows it: "8:05 PM", or "20:05".
/// Now as Technical Info words a time: "Oct 19, 2026 14:05:09".
fn now_stamp() -> String {
    // SAFETY: time with a null pointer only returns the time.
    let tm = local_tm(unsafe { libc::time(std::ptr::null_mut()) });
    format!("{} {}, {} {:02}:{:02}:{:02}", &MONTHS[tm.tm_mon as usize % 12][..3], tm.tm_mday, tm.tm_year + 1900, tm.tm_hour, tm.tm_min, tm.tm_sec)
}

/// The local time zone's abbreviation now ("EST"), as strftime's %Z.
fn time_zone() -> String {
    let mut buf = [0u8; 16];
    // SAFETY: time with a null pointer only returns the time; strftime
    // writes at most buf.len() bytes into buf, from the tm local_tm fills.
    let n = unsafe {
        let tm = local_tm(libc::time(std::ptr::null_mut()));
        libc::strftime(buf.as_mut_ptr().cast(), buf.len(), c"%Z".as_ptr(), &tm)
    };
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

fn clock_text(hour: u8, minute: u8, twelve_hour: bool) -> String {
    if twelve_hour {
        format!("{}:{minute:02} {}", (hour + 11) % 12 + 1, if hour < 12 { "AM" } else { "PM" })
    } else {
        format!("{hour:02}:{minute:02}")
    }
}

/// A moment's local hour, minute and weekday (0 is Sunday).
fn local_time(at: std::time::SystemTime) -> (u8, u8, u8) {
    let tm = local_tm(at.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as libc::time_t);
    (tm.tm_hour as u8, tm.tm_min as u8, tm.tm_wday as u8)
}

fn local_tm(at: libc::time_t) -> libc::tm {
    // SAFETY: localtime_r fills the tm it is given.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&at, &mut tm);
        tm
    }
}

/// The local time, as the skin shows it.
fn clock_now(twelve_hour: bool) -> crate::skin::Clock {
    // SAFETY: time with a null pointer only returns the time.
    let tm = local_tm(unsafe { libc::time(std::ptr::null_mut()) });
    crate::skin::Clock {
        weekday: tm.tm_wday as u8,
        hour: tm.tm_hour as u8,
        minute: tm.tm_min as u8,
        second: tm.tm_sec as u8,
        twelve_hour,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fan_schedule_runs_its_minutes_each_hour_from_when_it_was_set_inside_its_window() {
        let s = FanSchedule { duty: 15, start: 8, end: 17, anchor: 13 };
        let at = |h: u32, m: u32| s.runs(h * 60 + m);
        assert!(at(9, 13) && at(9, 27) && !at(9, 28) && !at(10, 12) && at(10, 13), "15 minutes from :13");
        assert!(at(16, 13) && !at(17, 13) && !at(7, 13), "08 to 17");
        assert!(FanSchedule { duty: 60, start: 22, end: 6, anchor: 0 }.runs(23 * 60 + 40), "always, over midnight");
        assert!(!FanSchedule { duty: 60, start: 22, end: 6, anchor: 0 }.runs(12 * 60));
        assert!(FanSchedule { duty: 30, start: 0, end: 0, anchor: 50 }.runs(3 * 60 + 5), "00 to 00 is all day; runs wrap the hour");
    }
    use crate::menu::{Choice, Item, Kind, Shown};

    fn click(e: &mut Engine) {
        e.input(Input::Press);
        e.input(Input::Release);
    }

    #[test]
    fn heat_link_is_in_equipment_settings_only_on_a_heat_link_unit() {
        for (model, shown) in [("Display-3.4", true), ("Display-2.7", false)] {
            let (tree, defaults) = crate::skin::load_menus(std::path::Path::new("config/menus.lua"), &[("classic".into(), "Classic".into())]).unwrap();
            let mut e = Engine::new(tree, defaults, Settings::load(None));
            e.set_model(model);
            // The menus open: Technical Info's facts are read.
            e.open_entry(&["Settings"]);
            e.tick();
            e.open_entry(&["Settings", "Equipment", "Continue"]);
            let rows: Vec<String> = e.nav.view(&e.menus(), &e.settings).unwrap().rows.into_iter().map(|r| r.label).collect();
            assert!(rows.iter().any(|r| r == "Pro Setup"), "{rows:?}");
            assert_eq!(rows.iter().any(|r| r == "Heat Link"), shown, "{model}");
        }
    }

    #[test]
    fn pro_setup_ends_on_equipment_settings() {
        let (tree, defaults) = crate::skin::load_menus(std::path::Path::new("config/menus.lua"), &[("classic".into(), "Classic".into())]).unwrap();
        let mut e = Engine::new(tree, defaults, Settings::load(None));
        e.set_model("Display-3.4");
        let menus = e.menus();
        e.nav.open_entry(&menus, &["Settings", "Equipment", "Continue"]);
        let title = |e: &Engine| e.nav.view(&e.menus(), &e.settings).map(|v| (v.style, v.prompt.unwrap_or_default()));
        // PRO SETUP, then the warning's CONTINUE.
        while e.nav.view(&e.menus(), &e.settings).is_some_and(|v| v.rows[v.selected].label != "Pro Setup") {
            e.input(Input::Wheel(1));
        }
        click(&mut e);
        // A row takes three detents: two steps.
        e.input(Input::Wheel(2));
        click(&mut e);
        assert!(e.pro.is_some(), "{:?}", title(&e));
        // The type, its form's CONTINUE, Your system's CONTINUE.
        click(&mut e);
        e.input(Input::Wheel(9));
        click(&mut e);
        click(&mut e);
        assert!(e.pro.is_none(), "kept");
        assert_eq!(title(&e).map(|t| t.0), Some(Some("choices".into())), "Equipment Settings: {:?}", title(&e));
    }

    #[test]
    fn heat_type_asks_delivery_then_fuel_and_ends_on_equipment_settings() {
        let (tree, defaults) = crate::skin::load_menus(std::path::Path::new("config/menus.lua"), &[("classic".into(), "Classic".into())]).unwrap();
        let mut e = Engine::new(tree, defaults, Settings::load(None));
        e.set_model("Display-1.0");
        e.tick();
        e.open_entry(&["Settings", "Equipment", "Continue", "Heat Type", "Type"]);
        e.input(Input::Wheel(3));
        click(&mut e);
        click(&mut e);
        assert_eq!((e.settings.text(HEAT_DELIVERY), e.settings.text(HEAT_SOURCE)), (Some("in-floor-radiant"), Some("gas")), "a turn of three detents: two rows");
        let view = e.nav.view(&e.menus(), &e.settings).unwrap();
        assert_eq!((view.style.as_deref(), view.rows[view.selected].label.as_str()), (Some("choices"), "Heat Type"));
    }

    #[test]
    fn emergency_heat_while_off_asks_to_turn_on_first() {
        let (tree, defaults) = crate::skin::load_menus(std::path::Path::new("config/menus.lua"), &[("classic".into(), "Classic".into())]).unwrap();
        let mut e = Engine::new(tree, defaults, Settings::load(None));
        e.settings.set(MODE, Value::Text("off".into()));
        e.settings.default(HAS_EMERGENCY, Value::Bool(true));
        e.open_entry(&["Settings", "Equipment", "Continue"]);
        while e.nav.view(&e.menus(), &e.settings).is_some_and(|v| v.rows[v.selected].label != "Emerg. Heat") {
            e.input(Input::Wheel(1));
        }
        click(&mut e);
        assert_eq!(e.off_alert, Some(1));
    }

    #[test]
    fn setting_the_humidity_while_off_asks_to_turn_on_first() {
        let (tree, defaults) = crate::skin::load_menus(std::path::Path::new("config/menus.lua"), &[("classic".into(), "Classic".into())]).unwrap();
        let mut e = Engine::new(tree, defaults, Settings::load(None));
        e.settings.set(MODE, Value::Text("off".into()));
        e.open_entry(&["Settings", "Equipment", "Continue"]);
        e.action("equipment.humidity");
        click(&mut e);
        assert_eq!(e.off_alert, Some(1), "TURN ON highlighted");
        click(&mut e);
        assert_ne!(e.settings.text(MODE), Some("off"));
        assert!(e.ui(480.0, 480.0).menu.and_then(|m| m.form).is_some(), "on to the target");
    }

    #[test]
    fn conventional_pro_setup_keeps_a_wires_fields_for_the_control() {
        let (tree, defaults) = crate::skin::load_menus(std::path::Path::new("config/menus.lua"), &[("classic".into(), "Classic".into())]).unwrap();
        let mut e = Engine::new(tree, defaults, Settings::load(None));
        e.set_model("Display-1.0");
        // W1, Y1, C, Rc, G, O/B: a heat pump.
        e.set_wiring([1, 1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
        e.tick();
        assert_eq!(e.settings.get(HAS_HEAT_PUMP), Some(&Value::Bool(true)));
        let menus = e.menus();
        e.nav.open_entry(&menus, &["Settings", "Equipment", "Continue"]);
        while e.nav.view(&e.menus(), &e.settings).is_some_and(|v| v.rows[v.selected].label != "Pro Setup") {
            e.input(Input::Wheel(1));
        }
        click(&mut e);
        e.input(Input::Wheel(2));
        click(&mut e);
        let rows = |e: &Engine| e.ui(480.0, 480.0).menu.map(|m| m.rows.iter().map(|r| r.label.clone()).collect::<Vec<_>>()).unwrap_or_default();
        assert_eq!(rows(&e)[..2], ["W1: Aux. heat".to_string(), "Y1: Heat / cool".to_string()]);
        // O/B to B.
        e.input(Input::Wheel(3));
        click(&mut e);
        click(&mut e);
        e.input(Input::Wheel(1));
        click(&mut e);
        click(&mut e);
        e.input(Input::Wheel(9));
        click(&mut e);
        assert!(e.equip.is_none(), "kept");
        e.tick();
        assert!(!e.hvac.equipment.ob_cool, "B energises to heat");
        let view = e.nav.view(&e.menus(), &e.settings).unwrap();
        assert_eq!(view.style.as_deref(), Some("choices"), "back on Equipment Settings");
    }

    fn engine() -> Engine {
        let tree = Tree { values: Vec::new(), title: "Settings".into(), back: "Back".into(), items: vec![
            Item { label: "Units".into(), icon: None, style: None, prompt: None, shown: Shown::default(), kind: Kind::Choice { key: UNITS.into(), options: vec![
                Choice { value: "C".into(), label: "Celsius".into() },
                Choice { value: "F".into(), label: "Fahrenheit".into() },
            ] } },
        ] };
        Engine::new(tree, vec![(UNITS.into(), Value::Text("C".into())), (MODE.into(), Value::Text("heat".into()))], Settings::load(None))
    }

    #[test]
    fn menu_cache_tracks_only_settings_used_to_show_items() {
        let mut e = engine();
        e.tree.items[0].shown.when = Some(("feature".into(), true));
        e.tree_revision += 1;
        let hidden = e.menus();
        assert!(hidden.items.is_empty());
        e.settings.default("clock.time", Value::Text("12:00".into()));
        assert!(std::rc::Rc::ptr_eq(&hidden, &e.menus()));
        e.settings.default("feature", Value::Bool(true));
        let visible = e.menus();
        assert_eq!(visible.items.len(), 1);
        assert!(!std::rc::Rc::ptr_eq(&hidden, &visible));
    }

    #[test]
    fn home_turns_the_target_and_the_button_opens_the_menus() {
        let mut e = engine();
        e.input(Input::Wheel(4));
        let ui = e.ui(480.0, 480.0);
        assert_eq!((ui.screen, ui.thermostat.target, ui.thermostat.hvac), ("home", 23.0, "heating"));
        click(&mut e);
        assert_eq!(e.ui(480.0, 480.0).screen, "menu");
    }

    #[test]
    fn a_unit_change_in_the_menus_reaches_the_home_screen() {
        let mut e = engine();
        click(&mut e);
        e.input(Input::Wheel(-1));
        click(&mut e);
        e.input(Input::Wheel(1));
        click(&mut e);
        let ui = e.ui(480.0, 480.0);
        assert_eq!((ui.screen, ui.thermostat.unit), ("home", "F"));
        assert!((ui.thermostat.target - 69.8).abs() < 0.01);
    }

    #[test]
    fn idle_menus_close() {
        let mut e = engine();
        click(&mut e);
        assert!(!e.tick());
        e.last_input -= MENU_IDLE;
        assert!(e.tick());
        assert_eq!(e.ui(480.0, 480.0).screen, "home");
    }

    #[test]
    fn the_button_acts_on_release_and_not_after_waking_the_display() {
        let mut e = engine();
        assert!(!e.input(Input::Press));
        assert_eq!(e.ui(480.0, 480.0).screen, "home", "nothing until the button is let go");
        assert!(e.input(Input::Release));
        assert_eq!(e.ui(480.0, 480.0).screen, "menu");
        e.last_input -= DISPLAY_IDLE;
        e.tick();
        e.input(Input::Press);
        assert!(!e.input(Input::Release));
        assert_eq!(e.ui(480.0, 480.0).screen, "home", "the press only lit the display");
    }

    #[test]
    fn connections_pick_reattach_over_attach_until_put_back() {
        let mut c = Connections::default();
        assert_eq!(c.alert(), None);
        c.set_attached(false);
        assert_eq!(c.alert(), Some(ATTACH));
        c.comm = false;
        assert_eq!(c.alert(), Some(REATTACH), "comm down comes first");
        c.comm = true;
        c.set_attached(true);
        c.set_i2c(false);
        assert_eq!(c.alert(), Some(REATTACH));
        c.set_attached(false);
        c.set_attached(true);
        assert_eq!(c.alert(), None, "put back, I2C down alone no longer holds it");
        c.set_i2c(false);
        assert_eq!(c.alert(), Some(REATTACH));
        let mut e = engine();
        e.set_alert(Some(WIRING_CHANGED));
        assert!(!e.connections_alert(&Connections::default(), 4.0), "another alert stays");
        e.set_alert(None);
        assert!(!e.connections_alert(&c, 3.6), "held off at a low battery");
        assert!(e.connections_alert(&c, 4.0));
        assert!(e.connections_alert(&Connections::default(), 0.0));
        assert_eq!(e.alert(), None);
    }

    #[test]
    fn each_mode_type_keeps_its_own_schedule() {
        use crate::schedule::Setpoint;
        let mut e = engine();
        e.setpoint_tick();
        e.schedule.set(Setpoint { minute: 360, temp: 20.5, high: None });
        let heat = e.schedule.clone();
        e.settings.set(MODE, Value::Text("heat-cool".into()));
        e.setpoint_tick();
        assert_eq!(e.schedule_kind(), "RANGE");
        assert!(e.schedule.points.is_empty(), "no range schedule yet");
        e.schedule.set(Setpoint { minute: 420, temp: 19.0, high: Some(26.0) });
        let range = e.schedule.clone();
        e.settings.set(MODE, Value::Text("heat".into()));
        e.setpoint_tick();
        assert_eq!(e.schedule, heat);
        e.settings.set(MODE, Value::Text("heat-cool".into()));
        e.setpoint_tick();
        assert_eq!(e.schedule, range);
    }

    #[test]
    fn home_away_assist_turns_eco_on_when_empty_and_off_when_someone_returns() {
        let mut e = engine();
        e.away_tick();
        e.activity -= AWAY_AFTER + 1.0;
        e.away_tick();
        assert!(!e.eco(), "still training");
        let days: Vec<String> = (0..AWAY_TRAINING).map(|d| format!("d{d}")).collect();
        e.settings.set(AWAY_DAYS, Value::Text(serde_json::to_string(&days).unwrap()));
        e.away_tick();
        assert!(e.eco() && e.settings.get(ECO_AUTO) == Some(&Value::Bool(true)));
        e.seen();
        e.away_tick();
        assert!(!e.eco(), "someone came home");
        e.settings.set(ECO, Value::Bool(true));
        e.activity -= AWAY_AFTER + 1.0;
        e.seen();
        e.away_tick();
        assert!(e.eco(), "Eco set by hand stays");
    }

    #[test]
    fn the_filter_reminder_comes_past_its_threshold_and_remind_later_waits_for_the_second() {
        let mut e = engine();
        e.settings.set("filter.remind", Value::Text("on".into()));
        e.settings.set(FILTER_SECOND, Value::Number(36000.0));
        e.history.filter.total = FILTER_DEFAULT + 30;
        e.filter_tick();
        let d = e.ui(480.0, 480.0).dialog.unwrap();
        assert_eq!((d.options, e.settings.get(FILTER_LEVEL)), (&["REMIND LATER", "DONE"][..], Some(&Value::Number(1.0))));
        e.input(Input::Press);
        e.input(Input::Release);
        assert!(e.history.filter.alert_sent && e.ui(480.0, 480.0).dialog.is_none());
        e.history.filter.total = 36030;
        e.filter_tick();
        assert_eq!(e.ui(480.0, 480.0).dialog.map(|d| d.options), Some(&["DONE"][..]), "the second reminder");
        e.input(Input::Press);
        e.input(Input::Release);
        assert_eq!((e.history.filter.total, e.settings.get(FILTER_NEEDED)), (0, Some(&Value::Bool(false))), "changed today");
        e.filter_changed(400);
        assert_eq!(e.history.filter.total, FILTER_DEFAULT - 270_000, "400 days at five hours, held 75 hours short");
    }

    #[test]
    fn a_custom_room_gets_a_guid_once_and_keeps_its_name() {
        let mut e = engine();
        e.custom_place("Studio");
        let id = e.settings.text("placement.where").unwrap().to_string();
        assert_eq!((id.len(), id.as_bytes()[14], id.to_lowercase() == id), (36, b'4', true));
        assert_eq!(e.settings.text("placement.where.name"), Some("Studio"));
        e.settings.set("placement.where", Value::Text("other".into()));
        e.custom_place("studio");
        assert_eq!(e.settings.text("placement.where"), Some(id.as_str()), "the same room again");
        assert_eq!(e.settings.text("placement.customs").map(|c| c.lines().count()), Some(1));
    }

    #[test]
    fn an_ask_keeps_what_its_keyboard_or_boxes_give() {
        let ask = |boxes: Option<&str>| crate::menu::Ask {
            key: "plugin.code".into(), boxes: boxes.map(Into::into), prompt: "Code?".into(), max: 30, run: None };
        let mut e = engine();
        e.open_ask(ask(None));
        assert!(e.keyboard.is_some());
        // 'a', then on round the ring to DONE.
        e.input(Input::Release);
        let done = e.keyboard.as_ref().unwrap().0.keys().iter().position(|k| *k == crate::keyboard::Key::Done).unwrap() as i32;
        e.input(Input::Wheel(done));
        e.input(Input::Release);
        assert_eq!((e.keyboard.is_none(), e.settings.text("plugin.code")), (true, Some("a")));
        e.open_ask(ask(Some("9A")));
        assert_eq!(e.postal.as_ref().map(|p| p.chars.clone()), Some(vec![None, None]), "a doesn't fit a digit's box");
        for d in [1, 1] {
            e.input(Input::Wheel(d));
            e.input(Input::Release);
        }
        e.input(Input::Release);
        assert_eq!((e.postal.is_none(), e.settings.text("plugin.code")), (true, Some("0A")));
        assert_eq!(e.settings.text(COUNTRY), None, "no location set");
    }

    #[test]
    fn heat_cool_follows_range_setpoints_only() {
        use crate::schedule::Setpoint;
        let mut e = engine();
        e.settings.set(MODE, Value::Text("heat-cool".into()));
        e.schedule.set(Setpoint { minute: 0, temp: 19.0, high: Some(26.0) });
        e.apply_schedule(true);
        let t = &e.thermostat;
        assert_eq!((t.low, t.high, t.target), (19.0, 26.0, 21.0));
        e.schedule = crate::schedule::Schedule::default();
        e.schedule.set(Setpoint { minute: 0, temp: 18.0, high: None });
        e.apply_schedule(true);
        assert_eq!((e.thermostat.low, e.thermostat.high), (19.0, 26.0), "a single temperature is not a range");
    }

    #[test]
    fn heat_cool_picks_an_end_then_changes_it_on_a_press() {
        let mut e = engine();
        e.settings.set(MODE, Value::Text("heat-cool".into()));
        e.input(Input::Wheel(2));
        let t = e.ui(480.0, 480.0).thermostat;
        assert_eq!((t.range, t.range_editing, t.low, t.high), (Some("high"), false, 20.0, 24.0), "turning only picks");
        e.input(Input::Wheel(-1));
        assert_eq!(e.ui(480.0, 480.0).thermostat.range, Some("low"));
        click(&mut e);
        e.input(Input::Wheel(3));
        let t = e.ui(480.0, 480.0).thermostat;
        assert_eq!((t.range, t.range_editing, t.low), (Some("low"), true, 21.5));
        click(&mut e);
        let ui = e.ui(480.0, 480.0);
        assert_eq!((ui.screen, ui.thermostat.range), ("home", None), "the second press sets it");
        click(&mut e);
        assert_eq!(e.ui(480.0, 480.0).screen, "menu", "with nothing picked, a press opens the menus");
    }

    #[test]
    fn a_manual_change_that_starts_heating_shows_stocks_time_to_temperature() {
        let mut e = engine();
        e.settings.set(MODE, Value::Text("heat".into()));
        e.thermostat.target = 18.0;
        e.set_room(Some(19.0), None);
        e.hvac_update();
        assert_eq!(e.time_to_target(), None);
        e.thermostat.target = 21.0;
        e.hvac_update();
        assert_eq!(e.time_to_target(), Some(45), "2 °C on stock's heat curve: 2704 s");
        e.schedule_set = true;
        e.thermostat.target = 22.0;
        e.hvac_update();
        assert_eq!(e.time_to_target(), Some(63), "a schedule's change keeps it shown, to the new target: 3802 s");
    }

    #[test]
    fn auto_schedule_counts_its_days_on_as_stock_words_them() {
        let mut e = engine();
        let heat = format!("{LEARNING_DAYS}.heat");
        e.settings.set("sense.learning", Value::Text("on".into()));
        e.learning_days_on(100.0);
        assert_eq!(e.settings.text("sense.days"), Some("0 days"));
        e.learning_days_on(100.0);
        assert_eq!(e.settings.text("sense.days"), Some("0 days"), "a day counts once");
        e.learning_days_on(101.0);
        assert_eq!(e.settings.text("sense.days"), Some("one day"));
        e.settings.set(MODE, Value::Text("cool".into()));
        e.learning_days_on(102.0);
        assert_eq!(e.settings.text("sense.days"), Some("0 days"), "cooling counts its own");
        for day in 103..115 {
            e.learning_days_on(f64::from(day));
        }
        assert_eq!(e.settings.text("sense.days"), Some("12 days"));
        e.settings.set(MODE, Value::Text("heat".into()));
        e.learning_days_on(115.0);
        assert_eq!(e.settings.text("sense.days"), Some("2 days"), "heating counted no day spent cooling");
        e.settings.set("sense.learning", Value::Text("off".into()));
        e.learning_days_on(115.0);
        assert!(e.settings.get(&heat).is_none(), "off forgets the counts");
        assert_eq!(e.settings.text("sense.days"), Some("0 days"));
    }

    #[test]
    fn a_change_of_wiring_is_noticed_once_and_continue_keeps_it() {
        let mut e = engine();
        let mut pins = [0; 15];
        pins[crate::hvac::wiring::test_index(8)] = 1;
        pins[2] = 1;
        assert!(!e.set_wiring(pins), "the first wiring seen is only kept");
        assert!(!e.set_wiring(pins));
        pins[2] = 0;
        assert!(e.set_wiring(pins));
        assert_eq!((e.ui(480.0, 480.0).screen, e.ui(480.0, 480.0).notice), ("notice", Some(WIRING_CHANGED)));
        assert!(!e.input(Input::Wheel(3)), "turning does nothing");
        click(&mut e);
        assert_eq!(e.ui(480.0, 480.0).screen, "menu", "CONTINUE opens the menus");
        assert!(!e.set_wiring(pins), "the new wiring is the one known");
    }

    #[test]
    fn the_menus_conditions_follow_the_wiring() {
        use crate::hvac::wiring::test_index;
        let mut e = engine();
        let on = |e: &Engine, key: &str| e.settings.get(key) == Some(&Value::Bool(true));
        let keys = [HAS_FAN, HAS_COOLING, HAS_AIRWAVE, HAS_EARLY_ON];
        e.equipment_tick();
        assert!(keys.iter().all(|k| on(&e, k)), "all of it until the wiring is known");
        let mut pins = [0; 15];
        pins[test_index(5)] = 1;
        e.set_wiring(pins);
        e.equipment_tick();
        assert_eq!(keys.map(|k| on(&e, k)), [false, false, false, true], "heat only");
        e.settings.set(HEAT_CONTROL, Value::Text("opentherm".into()));
        e.equipment_tick();
        assert!(!on(&e, HAS_EARLY_ON), "True Radiant's preheat without cooling");
        pins[test_index(0)] = 1;
        e.set_wiring(pins);
        e.equipment_tick();
        assert_eq!(keys.map(|k| on(&e, k)), [false, true, false, true], "cooling without a fan");
        pins[test_index(2)] = 1;
        e.set_wiring(pins);
        e.equipment_tick();
        assert!(keys.iter().all(|k| on(&e, k)), "cooling and a fan");
    }

    #[test]
    fn cool_to_dry_heat_pump_balance_reminders_and_the_system_follow_stocks_gates() {
        use crate::hvac::wiring::{test_index, Wiring};
        let mut e = engine();
        let on = |e: &Engine, key: &str| e.settings.get(key) == Some(&Value::Bool(true));
        let wire = |e: &mut Engine, functions: &[u8], star: i32| {
            let mut pins = [0; 15];
            for &f in functions {
                pins[test_index(f)] = 1;
            }
            e.hvac.wiring = Some(Wiring { pins, y2: -1, star, dual_fuel: false });
            e.equipment_tick();
        };
        // W1 and Rh: heating alone, no air but the furnace's.
        wire(&mut e, &[5, 8], -1);
        assert_eq!((on(&e, HAS_DRY), on(&e, HAS_BALANCE), e.settings.text(SYSTEM)), (false, false, Some("heat")));
        assert!(on(&e, HAS_FILTER), "a furnace's delivery unknown is forced air");
        e.hvac.equipment.delivery[crate::hvac::wiring::Switch::Stage1Heat as usize] = 2;
        e.equipment_tick();
        assert!(!on(&e, HAS_FILTER), "radiators move no air");
        // Y1, O/B, W2 (aux), G: a heat pump with auxiliary heat.
        wire(&mut e, &[0, 3, 6, 2, 8], -1);
        assert_eq!((on(&e, HAS_DRY), on(&e, HAS_BALANCE), on(&e, HAS_FILTER), e.settings.text(SYSTEM)), (true, true, true, Some("both")));
        // A stand-alone dehumidifier on STAR takes Cool to Dry away.
        wire(&mut e, &[0, 2, 8, 16], 2);
        e.hvac.equipment.humidity.dehumidifier = 1;
        e.equipment_tick();
        assert!(!on(&e, HAS_DRY));
        e.hvac.equipment.humidity.dehumidifier = 2;
        e.equipment_tick();
        assert!(on(&e, HAS_DRY), "an AC-integrated one keeps it");
        e.settings.set(BALANCE_LEVEL, Value::Text("balanced".into()));
        e.equipment_tick();
        assert_eq!(e.hvac.equipment.savings, 1);
    }

    #[test]
    fn heat_pump_balance_confirms_the_level_chosen_then_leaves_its_answers() {
        let mut e = stock_menus();
        e.settings.set(HAS_BALANCE, Value::Bool(true));
        let menus = e.menus();
        e.nav.open_entry(&menus, &["Settings", "Nest Sense", "Heat Pump Balance"]);
        point_at(&mut e, "Balanced");
        click(&mut e);
        let ui = e.ui(480.0, 480.0);
        let view = ui.menu.as_ref().unwrap();
        assert!(view.info.as_ref().is_some_and(|i| i.text.contains("balanced\ncomfort")), "its confirmation");
        assert_eq!(e.settings.text(BALANCE_LEVEL), Some("balanced"));
        click(&mut e);
        assert!(e.ui(480.0, 480.0).menu.as_ref().unwrap().info.is_none());
        assert_eq!(e.nav.pointed(&e.menus()).map(|p| p.1), Some("Heat Pump Balance"), "back on its entry");
    }

    #[test]
    fn reset_schedule_basic_starts_over_on_stocks_basic_schedule_for_the_unit_and_country() {
        let mut e = engine();
        e.action("reset.schedule.basic");
        assert_eq!(e.ui(480.0, 480.0).alert.as_deref(), Some("Resetting schedule in 10 seconds"));
        e.countdown = Some(("reset.schedule.basic", Instant::now() - Duration::from_secs(11), 0));
        e.tick();
        assert_eq!(e.schedule.points.len(), 24);
        let first = e.schedule.points[0];
        assert_eq!((first.minute, (first.temp * 100.0).round()), (360, 2056.0), "Monday 06:00 at 69 °F");
        assert!(e.settings.text("schedule.COOL").is_some_and(|d| d.contains("23.889")), "cooling's aside, 75 °F");
        // A Heat Link unit in the UK: heating only, 20 °C and 16 °C.
        let mut e = engine();
        e.set_model("Display-3.4");
        e.settings.set(COUNTRY, Value::Text("GB".into()));
        e.basic_schedule();
        assert_eq!(e.schedule.points.iter().take(2).map(|p| p.temp).collect::<Vec<_>>(), [20.0, 16.0]);
        assert!(e.settings.text("schedule.COOL").is_none());
        e.settings.set(COUNTRY, Value::Text("FR".into()));
        e.basic_schedule();
        assert_eq!(e.schedule.points[1].temp, 17.5, "elsewhere 17.5 °C while out");
    }

    #[test]
    fn weather_offline_says_wifi_is_off_for_a_low_battery_from_under_3_6_v_to_over_3_8_v() {
        let mut e = engine();
        e.weather_offline();
        assert_eq!(e.settings.text(WEATHER_OFFLINE), Some("Connect to Wi-Fi to\nshow the weather."));
        for (volts, low) in [(3.55, true), (3.7, true), (3.81, false)] {
            e.battery = [volts; 5];
            e.battery_read = None;
            e.battery_tick(|| volts);
            assert_eq!(e.battery_low, low, "{volts} V");
        }
        e.battery_low = true;
        e.weather_offline();
        assert!(e.settings.text(WEATHER_OFFLINE).is_some_and(|t| t.contains("battery")));
    }

    #[test]
    fn set_time_offers_the_countrys_time_zones_as_stock() {
        assert_eq!(zones(Some("GB")), ("EU", &["Europe/London", "Europe/Berlin", "Europe/Tallinn"][..]));
        assert_eq!(zones(Some("US")).1.len(), 8);
        assert_eq!(zones(None).0, "none");
        let mut e = stock_menus();
        e.settings.set(COUNTRY, Value::Text("AU".into()));
        e.clock_tick();
        assert_eq!(e.settings.text(CLOCK_ZONES), Some("AU"));
        e.open_menu(&["Settings".into()], None);
        point_at(&mut e, "Date & Time");
        click(&mut e);
        point_at(&mut e, "Set time");
        click(&mut e);
        let ui = e.ui(480.0, 480.0);
        let form = ui.menu.as_ref().and_then(|m| m.form.as_ref()).expect("Set time's form");
        assert_eq!(form.fields.len(), 6);
        assert_eq!(form.texts[5].as_deref(), Some("Western Australia"), "the first of the country's zones");
    }

    #[test]
    fn a_reset_counts_down_a_press_cancels_it_and_at_the_end_it_is_done() {
        let mut e = engine();
        e.settings.set(AWAY_DAYS, Value::Text("[1]".into()));
        e.action("reset.away");
        assert_eq!(e.screen(), "alert");
        assert_eq!(e.ui(480.0, 480.0).alert.as_deref(), Some("Resetting Away in 10 seconds"));
        e.input(Input::Press);
        e.input(Input::Release);
        assert_eq!((e.screen(), e.settings.text(AWAY_DAYS)), ("home", Some("[1]")), "cancelled");
        e.action("reset.away");
        e.countdown = Some(("reset.away", Instant::now() - Duration::from_secs(11), 0));
        e.tick();
        assert_eq!((e.screen(), e.settings.text(AWAY_DAYS)), ("home", Some("[]")));
    }

    #[test]
    fn each_mode_gives_back_its_own_setpoint() {
        let mut e = engine();
        e.thermostat.target = 21.0;
        e.setpoint_tick();
        e.settings.set(MODE, Value::Text("cool".into()));
        e.setpoint_tick();
        e.thermostat.target = 24.0;
        e.settings.set(MODE, Value::Text("heat".into()));
        e.setpoint_tick();
        assert_eq!(e.thermostat.target, 21.0, "heat's own");
        e.settings.set(MODE, Value::Text("cool".into()));
        e.setpoint_tick();
        assert_eq!(e.thermostat.target, 24.0, "cool's own");
    }

    #[test]
    fn starting_eco_while_off_turns_on_in_the_last_mode() {
        let mut e = engine();
        e.settings.set(MODE, Value::Text("cool".into()));
        e.setpoint_tick();
        e.settings.set(MODE, Value::Text("off".into()));
        e.setpoint_tick();
        e.action("eco.start");
        assert_eq!(e.settings.text(MODE), Some("cool"));
        assert!(e.eco());
    }

    #[test]
    fn emergency_heat_sets_aside_learning_and_guards_the_schedule() {
        let tree = Tree { values: Vec::new(), title: "Menu".into(), back: "Back".into(), items: vec![
            Item { label: "Schedule".into(), icon: None, style: None, prompt: None, shown: Shown::default(), kind: Kind::Action { name: "schedule".into() } },
        ] };
        let mut e = Engine::new(tree, vec![(MODE.into(), Value::Text("heat".into()))], Settings::load(None));
        e.hvac.equipment.emergency_heat = true;
        e.settings.set(MODE, Value::Text("heat".into()));
        e.settings.set("sense.learning", Value::Text("on".into()));
        e.tick();
        e.settings.set(EMERGENCY, Value::Text("on".into()));
        e.tick();
        assert_eq!(e.settings.text("sense.learning"), Some("off"));
        assert!(matches!(e.hvac_mode(), crate::hvac::control::Mode::Emergency(_)));
        // Open the menus and point at Schedule.
        click(&mut e);
        let pointed = |e: &Engine| e.nav.pointed(&e.menus()).map(|p| p.1.to_string());
        for _ in 0..12 {
            if pointed(&e).as_deref() == Some("Schedule") {
                break;
            }
            e.input(Input::Wheel(1));
        }
        assert_eq!(pointed(&e).as_deref(), Some("Schedule"));
        click(&mut e);
        assert_eq!(e.ui(480.0, 480.0).dialog.map(|d| d.selected), Some(0), "EMERGENCY HEAT IS ON, LEAVE ON first");
        e.input(Input::Wheel(1));
        click(&mut e);
        assert_eq!(e.settings.text(EMERGENCY), Some("off"));
        assert_eq!(e.settings.text("sense.learning"), Some("on"), "given back");
        assert_eq!(e.ui(480.0, 480.0).screen, "schedule", "and on to the Schedule");
    }

    fn stock_menus() -> Engine {
        let (tree, defaults) = crate::skin::load_menus(std::path::Path::new("config/menus.lua"), &[("classic".into(), "Classic".into())]).unwrap();
        Engine::new(tree, defaults, Settings::load(None))
    }

    /// Turns until a press would open `label`.
    fn point_at(e: &mut Engine, label: &str) {
        for _ in 0..40 {
            if e.nav.pointed(&e.menus()).is_some_and(|p| p.1 == label) {
                return;
            }
            e.input(Input::Wheel(1));
        }
        panic!("no {label}");
    }

    #[test]
    fn mode_on_a_heat_only_system_turns_it_off_and_on() {
        let mut e = stock_menus();
        // Heating and cooling as the wiring has them, whatever nestlocal says.
        let wired = |functions: &[u8]| {
            let mut pins = [0; 15];
            for &f in functions {
                pins[crate::hvac::wiring::test_index(f)] = 1;
            }
            Some(crate::hvac::wiring::Wiring { pins, y2: -1, star: -1, dual_fuel: false })
        };
        e.hvac.wiring = wired(&[5]);
        e.settings.set(MODE, Value::Text("heat".into()));
        e.settings.set(ECO, Value::Bool(true));
        e.tick();
        click(&mut e);
        point_at(&mut e, "Mode");
        click(&mut e);
        assert_eq!((e.settings.text(MODE), e.eco(), e.nav.is_open()), (Some("off"), false, false), "off, Eco stopped, home");
        click(&mut e);
        point_at(&mut e, "Mode");
        click(&mut e);
        assert_eq!((e.settings.text(MODE), e.nav.is_open()), (Some("heat"), false), "on again");
        // Heating and cooling both: the modes to choose from.
        e.thermostat.can_cool = false;
        e.hvac.wiring = wired(&[5, 0]);
        click(&mut e);
        point_at(&mut e, "Mode");
        click(&mut e);
        assert_eq!((e.settings.text(MODE), e.nav.editing_key(&e.menus())), (Some("heat"), Some(MODE)));
    }

    #[test]
    fn settings_fan_while_off_asks_to_turn_the_thermostat_on() {
        let mut e = stock_menus();
        e.settings.set(MODE, Value::Text("cool".into()));
        e.tick();
        e.settings.set(MODE, Value::Text("off".into()));
        e.tick();
        click(&mut e);
        point_at(&mut e, "Settings");
        click(&mut e);
        point_at(&mut e, "Fan Schedule");
        click(&mut e);
        let dialog = e.ui(480.0, 480.0).dialog.expect("the alert");
        assert_eq!((dialog.options, dialog.selected), (&["LEAVE OFF", "TURN ON"][..], 1), "TURN ON first");
        e.input(Input::Wheel(-1));
        click(&mut e);
        assert_eq!((e.settings.text(MODE), e.nav.pointed(&e.menus()).map(|p| p.1)), (Some("off"), Some("Fan Schedule")), "left off, on Settings");
        click(&mut e);
        click(&mut e);
        assert_eq!(e.settings.text(MODE), Some("cool"), "on in its last mode");
        assert!(e.nav.form_open(), "and on to the fan's page");
    }

    #[test]
    fn the_ring_turned_while_off_asks_for_a_mode() {
        let mut e = engine();
        e.settings.set(MODE, Value::Text("off".into()));
        e.settings.set(ECO, Value::Bool(true));
        e.tick();
        e.input(Input::Wheel(1));
        let dialog = e.ui(480.0, 480.0).dialog.expect("the alert");
        assert_eq!((dialog.options, dialog.selected, dialog.checked), (&["HEAT", "COOL", "HEAT • COOL", "OFF", "CANCEL"][..], 3, Some(3)), "off first, ticked");
        e.input(Input::Wheel(1));
        click(&mut e);
        assert_eq!((e.settings.text(MODE), e.ui(480.0, 480.0).dialog), (Some("off"), None), "CANCEL leaves it off");
        e.input(Input::Wheel(1));
        for _ in 0..3 {
            e.input(Input::Wheel(-1));
        }
        click(&mut e);
        assert_eq!((e.settings.text(MODE), e.eco()), (Some("heat"), false), "HEAT turns it on, Eco stopped");
        e.thermostat.can_cool = false;
        e.settings.set(MODE, Value::Text("off".into()));
        e.input(Input::Wheel(-1));
        assert_eq!(e.ui(480.0, 480.0).dialog.map(|d| d.options), Some(&["HEAT", "OFF", "CANCEL"][..]), "the modes it runs");
    }

    #[test]
    fn a_wrong_clock_at_start_without_wi_fi_asks_to_set_it() {
        let mut e = stock_menus();
        e.date_time_check(0.0, false);
        assert!(e.date_alert.is_none(), "the time never set by hand");
        e.settings.set(CLOCK_ADJUSTED, Value::Bool(true));
        e.date_time_check(0.0, true);
        assert!(e.date_alert.is_none(), "Wi-Fi powered gives the time");
        e.date_time_check(BUILD_DATE, false);
        assert!(e.date_alert.is_none(), "the time can be right");
        e.date_time_check(BUILD_DATE - 1.0, false);
        let dialog = e.ui(480.0, 480.0).dialog.expect("the alert");
        assert_eq!((dialog.text.as_str(), dialog.options, dialog.selected), (DATE_TIME_TEXT, &["OK", "SET DATE/TIME"][..], 0));
        e.input(Input::Wheel(1));
        click(&mut e);
        assert!(e.ui(480.0, 480.0).dialog.is_none() && e.nav.form_open(), "on to the date and time");
    }

    #[test]
    fn airwave_tells_of_its_first_run_once() {
        let mut e = engine();
        e.airwave_alert = true;
        let dialog = e.ui(480.0, 480.0).dialog.expect("the alert");
        assert_eq!((dialog.text.as_str(), dialog.icon), (AIRWAVE_TEXT, Some("airwave")));
        click(&mut e);
        assert!(!e.airwave_alert);
    }

    #[test]
    fn a_long_hvac_alert_turns_down_and_done_takes_it_down() {
        let mut e = engine();
        e.battery_alert = true;
        e.input(Input::Wheel(3));
        assert_eq!(e.ui(480.0, 480.0).dialog.map(|d| d.scrolled), Some(3));
        e.input(Input::Wheel(-5));
        assert_eq!(e.ui(480.0, 480.0).dialog.map(|d| d.scrolled), Some(0));
        click(&mut e);
        assert!(!e.battery_alert && e.ui(480.0, 480.0).dialog.is_none());
    }

    #[test]
    fn a_fan_of_more_speeds_sets_its_run_and_speed_on_one_page() {
        let mut e = stock_menus();
        e.hvac.equipment.fan_stages = 3;
        e.tick();
        click(&mut e);
        point_at(&mut e, "Fan");
        click(&mut e);
        assert!(e.nav.form_open(), "the timer's page of speeds");
        // The run: 15 minutes on to 30; the speed: low on to medium; DONE.
        for input in [Input::Press, Input::Release, Input::Notch(1), Input::Press, Input::Release,
                      Input::Press, Input::Release, Input::Notch(1), Input::Press, Input::Release, Input::Press, Input::Release] {
            e.input(input);
        }
        assert!(!e.nav.is_open(), "DONE closes the menus");
        e.tick();
        e.equipment_tick();
        assert!(e.fan_running());
        assert_eq!((e.settings.text(FAN_TIMER), e.hvac.equipment.fan_timer_speed), (Some("30"), 2));
        assert_eq!(e.settings.text(FAN_ENTRY), Some("dial"), "running: the dial's entry, its sentence page");
        assert!(e.fan_view().expect("running").speed);
        // The run ending on its page closes the menus.
        click(&mut e);
        point_at(&mut e, "Fan");
        click(&mut e);
        assert_eq!(e.nav.editing_key(&e.menus()), Some(FAN_TIMER));
        e.fan_until = Some(Instant::now() - Duration::from_secs(1));
        e.tick();
        e.tick();
        assert_eq!((e.settings.text(FAN_ENTRY), e.nav.is_open()), (Some("3"), false));
    }

    #[test]
    fn settings_fan_with_speeds_asks_heating_and_cooling_speed_by_the_system() {
        let mut e = stock_menus();
        e.hvac.equipment.fan_stages = 2;
        e.thermostat.can_cool = false;
        e.tick();
        assert!(e.open_menu(&["Settings".into(), "Fan".into()], None), "Fan settings");
        point_at(&mut e, "System fan speed");
        click(&mut e);
        let view = e.nav.view(&e.menus(), &e.settings).and_then(|v| v.form).expect("its page");
        assert_eq!(view.prompt.as_deref(), Some("What speed should the fan run when heating?"));
        assert_eq!(view.texts, vec![Some("auto".to_string())]);
        for input in [Input::Press, Input::Release, Input::Notch(2), Input::Press, Input::Release] {
            e.input(input);
        }
        e.tick();
        assert_eq!((e.settings.text(FAN_HEAT_COOL_SPEED), e.hvac.equipment.fan_heat_cool_speed), (Some("stage2"), Some(2)));
    }

    #[test]
    fn a_fan_of_more_than_one_speed_names_it() {
        let mut e = engine();
        e.settings.set(FAN_SCHEDULE, Value::Text("on".into()));
        e.settings.set(FAN_DUTY, Value::Number(15.0));
        e.settings.set(FAN_START, Value::Number(8.0));
        e.settings.set(FAN_END, Value::Number(17.0));
        e.hvac.equipment.fan_schedule_speed = 2;
        assert_eq!(e.fan_schedule_line().as_deref(), Some("On every day for 15 mins/hr from 08:00 to 17:00"), "one speed");
        e.hvac.equipment.fan_stages = 2;
        assert_eq!(e.fan_schedule_line().as_deref(), Some("On high every day for 15 mins/hr from 08:00 to 17:00"), "the second of two");
        e.hvac.equipment.fan_stages = 3;
        assert_eq!(e.fan_schedule_line().as_deref(), Some("On medium every day for 15 mins/hr from 08:00 to 17:00"));
        e.settings.set(FAN_DUTY, Value::Number(60.0));
        e.settings.set(FAN_END, Value::Number(8.0));
        e.hvac.equipment.fan_schedule_speed = 1;
        assert_eq!(e.fan_schedule_line().as_deref(), Some("Always on low"));
        e.settings.set(FAN_SCHEDULE, Value::Text("off".into()));
        e.settings.set(FAN_TIMER, Value::Text("30".into()));
        e.tick();
        let fan = e.fan_view().expect("running");
        assert!(fan.until.starts_with("On low until "), "{}", fan.until);
        assert!(fan.left_text.starts_with("Running for the next ") && fan.left_text.ends_with(" minutes at low speed"), "{}", fan.left_text);
    }

    #[test]
    fn only_units_outside_the_heat_link_gate_give_the_backplate_safety_temperatures() {
        let mut e = engine();
        e.real_backplate = true;
        e.settings.set(SAFETY_LOW, Value::Number(4.444));
        e.model = Some("Display-3.4".into());
        e.set_wires([1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
        e.set_wiring([1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(e.backplate_safety(), None, "a Heat Link unit");
        e.model = Some("Display-2.7".into());
        let p = e.backplate_safety().expect("outside the gate");
        assert_eq!(p[0], 1, "heat at the default 4.44 °C, cool off");
        e.real_backplate = false;
        assert_eq!(e.backplate_safety(), None, "simulated");
    }

    #[test]
    fn a_gen_2_is_a_heat_link_unit_by_its_backplate() {
        let mut e = engine();
        e.device = Device::Gen2;
        e.model = Some("Display-2.6".into());
        e.backplate_version = 2;
        assert!(!e.heat_link(), "Backplate-2.x");
        e.backplate_version = 3;
        assert!(e.heat_link(), "Amber");
        e.backplate_version = 5;
        assert!(e.heat_link(), "Backplate-6.x");
        e.device = Device::Gen3;
        assert!(!e.heat_link(), "the gen 3's gate goes by its model");
    }

    #[test]
    fn outside_the_heat_link_gate_the_wiring_report_gives_the_page() {
        let mut e = engine();
        e.model = Some("Display-2.7".into());
        e.backplate_version = 5;
        let wires = [1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0];
        let pins = [1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        e.set_wires(wires);
        assert!(!e.set_wiring(pins));
        assert_eq!(e.ui(480.0, 480.0).error, None, "W1, Y1, G, C and Rh: nothing to report");
        // C on its pin without power, the pins as before: E79.
        let mut no_c = wires;
        no_c[8] = 0;
        e.set_wires(no_c);
        assert!(e.set_wiring(pins));
        let page = e.ui(480.0, 480.0).error.map(|(p, _)| p);
        assert_eq!(page.map(|p| (p.code, p.title, p.link)), Some(("E79", "WIRING REPORT", "nest.com/e79")));
        // A fault on Y1 with everything powered: E103.
        e.set_wires(wires);
        e.set_wiring(pins);
        let mut faults = [false; 13];
        faults[1] = true;
        assert!(e.set_faults(faults));
        assert_eq!(e.ui(480.0, 480.0).error.map(|(p, _)| p.code), Some("E103"));
        assert!(e.set_faults([false; 13]));
        assert_eq!(e.ui(480.0, 480.0).error, None);
    }

    #[test]
    fn y1_without_power_after_a_cold_spell_is_no_power_and_notes_wait_for_equipment() {
        let mut e = engine();
        e.model = Some("Display-2.7".into());
        e.backplate_version = 5;
        // W1 and G wired, Y1's pin without power, no C.
        let wires = [1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0];
        let y1 = [1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        e.set_wires(wires);
        e.set_wiring(y1);
        assert_eq!(e.ui(480.0, 480.0).error.map(|(p, _)| (p.code, p.title)), Some(("E80", "WIRING REPORT")));
        e.set_weather(Some(-12.0), None, None, None);
        e.set_wiring([1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
        assert_eq!(e.ui(480.0, 480.0).error.map(|(p, _)| (p.code, p.title)), Some(("E116", "NO POWER")), "code 46");
        // G on its pin without a G wire: only a note, no page until
        // Equipment opens.
        let no_g = [1, 1, 0, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0];
        let pins = [1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        e.set_wires(no_g);
        e.set_wiring(pins);
        assert_eq!(e.ui(480.0, 480.0).error, None);
        let note = e.wiring_report(&no_g, &pins).map(|(c, p)| (c, p.code, p.choices));
        assert_eq!(note, Some((60, "N76", &["TECH INFO", "CONTINUE"][..])));
    }

    #[test]
    fn without_rh_or_c_the_error_page_shows_and_a_turn_offers_tech_info() {
        let mut e = engine();
        e.model = Some("Display-3.4".into());
        // nest-qemu's default wiring: W1, Y1, G, C and Rh.
        let wires = [1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0];
        let pins = [1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        e.set_wires(wires);
        assert!(!e.set_wiring(pins));
        let mut no_c = wires;
        no_c[8] = 0;
        e.set_wires(no_c);
        assert!(e.set_wiring([1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]), "a change of wiring: the notice first");
        assert_eq!(e.ui(480.0, 480.0).notice, Some(WIRING_CHANGED));
        click(&mut e);
        assert_eq!(e.ui(480.0, 480.0).screen, "error", "CONTINUE: the error comes before Equipment");
        assert_eq!(e.ui(480.0, 480.0).error, Some((UNPOWERED, None)));
        assert!(!e.input(Input::Press));
        assert!(e.input(Input::Wheel(1)));
        assert_eq!(e.ui(480.0, 480.0).error, Some((UNPOWERED, Some(0))));
        click(&mut e);
        assert_eq!(e.ui(480.0, 480.0).screen, "menu", "TECH INFO opens the one menu shown");
        e.nav.close();
        assert_eq!(e.ui(480.0, 480.0).screen, "error");
        e.set_wires(wires);
        assert!(e.set_wiring(pins), "power back: another change of wiring");
        assert_eq!(e.ui(480.0, 480.0).screen, "notice");
        click(&mut e);
        e.nav.close();
        assert_eq!(e.ui(480.0, 480.0).screen, "home", "and no error");
    }

    #[test]
    fn eco_holds_the_room_between_its_clamped_temperatures_whatever_the_mode() {
        use crate::hvac::control::Mode;
        let mut e = engine();
        e.settings.set(MODE, Value::Text("heat".into()));
        e.settings.set(ECO, Value::Bool(true));
        e.settings.set(ECO_HEAT, Value::Number(3.0));
        e.settings.set(ECO_COOL, Value::Number(29.0));
        assert_eq!(e.hvac_mode(), Mode::Range(4.44, 29.0));
        assert!(e.leaf_on());
        e.settings.set(ECO_HEAT, Value::Number(20.0));
        assert!(!e.leaf_on(), "an Eco heat temperature over the leaf's threshold");
        e.settings.set(ECO_COOL, Value::Text("off".into()));
        assert_eq!(e.hvac_mode(), Mode::Heat(20.0));
    }

    #[test]
    fn a_heat_link_unit_reports_a_wired_heat_link_once_its_backplate_has_wires() {
        let mut e = engine();
        assert_eq!(e.heat_link_connection(), None, "the model unknown");
        e.set_model("Display-3.4");
        assert_eq!(e.heat_link_connection(), Some(0));
        e.set_wires([1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0]);
        assert_eq!(e.heat_link_connection(), Some(0), "simulated wires do not count");
        e.real_backplate = true;
        assert_eq!(e.heat_link_connection(), Some(1));
        e.set_model("Display-4.1");
        assert_eq!(e.heat_link_connection(), None);
    }

    #[test]
    fn without_w1_stock_cannot_control_the_equipment_and_shows_h71() {
        let mut e = engine();
        e.model = Some("Display-3.4".into());
        e.set_wires([1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
        e.set_wiring([1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        e.set_wires([0, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
        assert!(e.set_wiring([0, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        assert_eq!(e.ui(480.0, 480.0).notice, Some(CANNOT_CONTROL));
        click(&mut e);
        assert_eq!(e.ui(480.0, 480.0).error, Some((NO_HEAT, None)));
        e.input(Input::Wheel(1));
        e.input(Input::Wheel(10));
        assert_eq!(e.ui(480.0, 480.0).error, Some((NO_HEAT, Some(3))), "TECH INFO, the last");
        let mut other = engine();
        other.model = Some("Display-4.1".into());
        other.set_wires([0; 13]);
        other.set_wiring([0; 15]);
        assert_eq!(other.ui(480.0, 480.0).error, None, "only Heat Link units check");
    }

    fn heat_link_engine() -> Engine {
        let (tree, defaults) = crate::skin::load_menus(std::path::Path::new("config/menus.lua"), &[("classic".into(), "Classic".into())]).unwrap();
        let mut e = Engine::new(tree, defaults, Settings::load(None));
        e.model = Some("Display-3.4".into());
        e.real_backplate = true;
        e
    }

    #[test]
    fn at_start_a_disconnected_heat_link_waits_stocks_delay_before_h71() {
        let mut e = heat_link_engine();
        let no_w1 = ([0, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0], [0, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        e.set_wires(no_w1.0);
        e.set_wiring(no_w1.1);
        assert_eq!(e.ui(480.0, 480.0).error, None, "held");
        e.set_wiring(no_w1.1);
        assert_eq!(e.ui(480.0, 480.0).error, None, "a later report keeps the hold");
        assert!(!e.heat_link_tick());
        e.heat_link_hold.run_out();
        assert!(e.heat_link_tick());
        assert_eq!(e.ui(480.0, 480.0).error, Some((NO_HEAT, None)));
        // W1 back during the delay: nothing shows, nor after.
        let mut back = heat_link_engine();
        back.set_wires(no_w1.0);
        back.set_wiring(no_w1.1);
        back.set_wires([1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
        back.set_wiring([1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        back.heat_link_hold.run_out();
        assert!(!back.heat_link_tick());
        assert_eq!(back.ui(480.0, 480.0).error, None);
        // No delay on the simulated backplate, nor for H72.
        let mut sim = heat_link_engine();
        sim.real_backplate = false;
        sim.set_wires(no_w1.0);
        sim.set_wiring(no_w1.1);
        assert_eq!(sim.ui(480.0, 480.0).error, Some((NO_HEAT, None)));
        let mut unpowered = heat_link_engine();
        unpowered.set_wires([0; 13]);
        unpowered.set_wiring([0; 15]);
        assert_eq!(unpowered.ui(480.0, 480.0).error, Some((UNPOWERED, None)));
    }

    #[test]
    fn a_change_of_wiring_with_a_wired_heat_link_opens_its_pages_then_the_error() {
        let mut e = heat_link_engine();
        let wires = [1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0];
        e.set_wires(wires);
        assert!(!e.set_wiring([1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        let mut no_c = wires;
        no_c[8] = 0;
        e.set_wires(no_c);
        assert!(e.set_wiring([1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        assert_eq!(e.ui(480.0, 480.0).notice, None, "no notice");
        assert_eq!(e.ui(480.0, 480.0).screen, "menu");
        assert!(e.nav.within(&e.menus(), "Heat Link"), "Heat Link connected, wired only");
        assert_eq!(e.settings.text(WIRING), Some("110011000000000"), "the wiring kept");
        e.nav.close();
        assert_eq!(e.ui(480.0, 480.0).error, Some((UNPOWERED, None)), "the error after");
        assert_eq!(e.ui(480.0, 480.0).screen, "error");
        // Without W1 stock's cannot-control notice stays.
        let mut h71 = heat_link_engine();
        h71.set_wires(wires);
        h71.set_wiring([1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        h71.set_wires([0, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
        assert!(h71.set_wiring([0, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        assert_eq!(h71.ui(480.0, 480.0).notice, Some(CANNOT_CONTROL));
    }

    #[test]
    fn technical_info_has_heat_link_only_on_a_heat_link_unit() {
        let mut e = heat_link_engine();
        e.nav.open_entry(&e.menus(), &["Settings"]);
        e.tech_tick();
        assert_eq!(e.settings.text("tech.heatlink"), Some("heading"), "unpaired here");
        let mut paired = heat_link_engine();
        paired.nav.open_entry(&paired.menus(), &["Settings"]);
        *paired.tech_results.lock().unwrap() = Some(("Display-3.4".into(), vec![("tech.heatlink.key", "L1E3P3".into())], Vec::new(), true));
        paired.tech_pending = true;
        assert!(paired.tech_tick());
        assert_eq!(paired.settings.text("tech.heatlink"), Some("key"), "paired key from the background read");
        let mut other = heat_link_engine();
        other.model = Some("Display-4.1".into());
        other.nav.open_entry(&other.menus(), &["Settings"]);
        other.tech_tick();
        assert_eq!(other.settings.text("tech.heatlink"), Some("off"));
    }

    #[test]
    fn an_alert_covers_everything_and_ignores_input_until_taken_down() {
        let mut e = engine();
        assert!(e.set_alert(Some(REATTACH)));
        assert_eq!(e.ui(480.0, 480.0).screen, "alert");
        e.input(Input::Press);
        e.input(Input::Release);
        e.input(Input::Wheel(3));
        assert_eq!((e.ui(480.0, 480.0).screen, e.thermostat.target), ("alert", 21.0));
        assert!(e.set_alert(None));
        assert_eq!(e.ui(480.0, 480.0).screen, "home");
    }

    #[test]
    fn someone_in_the_room_lights_the_display_from_afar_until_an_input() {
        let mut e = engine();
        e.last_input -= DISPLAY_IDLE;
        assert!(e.tick() && e.asleep());
        assert!(e.presence());
        assert_eq!(e.ui(480.0, 480.0).screen, "farsight");
        // Presence keeps it lit past the display's idle time.
        assert!(!e.tick() && !e.asleep());
        // The first input brings the dial once the far view has given way,
        // and does nothing else.
        assert!(!e.input(Input::Wheel(3)));
        e.leaving = e.leaving.map(|at| at - LEAVE_TIME);
        e.tick();
        assert_eq!((e.ui(480.0, 480.0).screen, e.thermostat.target), ("home", 21.0));
    }

    #[test]
    fn someone_near_brings_farsights_near_view_until_an_input() {
        let mut e = engine();
        e.settings.set(FARSIGHT, Value::Text("digital_clock".into()));
        e.last_input -= DISPLAY_IDLE;
        assert!(e.tick() && e.presence());
        assert_eq!(e.ui(480.0, 480.0).changed_near, None);
        // The far view holds a second before the near view comes in.
        assert!(e.approach());
        let ui = e.ui(480.0, 480.0);
        assert_eq!(ui.screen, "farsight");
        assert!((ui.changed_near.unwrap() - (ui.changed_screen + FAR_HOLD)).abs() < 0.1);
        assert!(ui.clock.weekday_date.is_some() && ui.clock.weekday_day.is_some());
        assert!(!e.approach(), "once");
        // The first input brings the dial once the near view has faded
        // out, and does nothing else.
        assert!(!e.input(Input::Wheel(3)));
        e.leaving = e.leaving.map(|at| at - LEAVE_TIME);
        e.tick();
        let ui = e.ui(480.0, 480.0);
        assert_eq!((ui.screen, ui.changed_near, e.thermostat.target), ("home", None, 21.0));
        assert!(!e.approach(), "nothing on the home screen");
        // A dark display lights on the near view, whole.
        e.last_input -= DISPLAY_IDLE;
        e.last_presence = None;
        assert!(e.tick() && e.asleep());
        assert!(e.approach());
        let ui = e.ui(480.0, 480.0);
        assert_eq!(ui.screen, "farsight");
        assert!(ui.now - ui.changed_near.unwrap() >= NEAR_TIME);
        // The temperatures give way to the home screen over 500 ms.
        e.settings.set(FARSIGHT, Value::Text("target_temp".into()));
        e.input(Input::Wheel(1));
        e.last_input -= DISPLAY_IDLE;
        e.last_presence = None;
        assert!(e.tick() && e.presence());
        assert!(e.approach());
        let ui = e.ui(480.0, 480.0);
        assert_eq!((ui.screen, ui.changed_near), ("farsight", None));
        assert!(ui.changed_leave.is_some());
        e.leaving = e.leaving.map(|at| at - LEAVE_TIME);
        assert!(e.tick());
        assert_eq!(e.ui(480.0, 480.0).screen, "home");
        // Waking on a press, it stays dark.
        e.settings.set(FARSIGHT, Value::Text("press".into()));
        e.last_input -= DISPLAY_IDLE;
        e.last_presence = None;
        assert!(e.tick() && !e.approach() && e.asleep());
    }

    #[test]
    fn an_input_fades_farsights_clocks_and_weather_out_first() {
        let mut e = engine();
        e.settings.set(FARSIGHT, Value::Text("analog_clock".into()));
        e.last_input -= DISPLAY_IDLE;
        assert!(e.tick() && e.presence());
        assert!(e.approach());
        let near = e.near;
        assert!(!e.input(Input::Wheel(1)));
        let ui = e.ui(480.0, 480.0);
        assert_eq!((ui.screen, ui.changed_near, e.thermostat.target), ("farsight", near, 21.0));
        assert!(ui.changed_leave.is_some());
        // Gone after 750 ms, the analog clock's time.
        e.leaving = e.leaving.map(|at| at - LEAVE_TIME);
        e.tick();
        assert_eq!(e.ui(480.0, 480.0).screen, "farsight");
        e.leaving = e.leaving.map(|at| at - (LEAVE_ANALOG - LEAVE_TIME));
        assert!(e.tick());
        let ui = e.ui(480.0, 480.0);
        assert_eq!((ui.screen, ui.changed_near, ui.changed_leave), ("home", None, None));
        // The temperatures give way over 500 ms, and the input does nothing
        // else.
        e.settings.set(FARSIGHT, Value::Text("target_temp".into()));
        e.last_input -= DISPLAY_IDLE;
        e.last_presence = None;
        assert!(e.tick() && e.presence());
        assert!(!e.input(Input::Wheel(1)));
        assert_eq!((e.ui(480.0, 480.0).screen, e.thermostat.target), ("farsight", 21.0));
        e.leaving = e.leaving.map(|at| at - LEAVE_TIME);
        assert!(e.tick());
        assert_eq!(e.ui(480.0, 480.0).screen, "home");
    }

    #[test]
    fn an_alert_over_farsight_keeps_it_under_and_someone_near_leaves_the_clock() {
        let mut e = engine();
        e.settings.set(FARSIGHT, Value::Text("digital_clock".into()));
        assert!(e.set_alert(Some(REATTACH)));
        e.last_input -= DISPLAY_IDLE;
        assert!(e.tick() && e.presence());
        let ui = e.ui(480.0, 480.0);
        assert_eq!((ui.screen, ui.far), ("alert", true));
        let shown = ui.changed_screen;
        // Near, the clock gives way to the alert instead of its near view,
        // the alert's own fade-in not started again.
        assert!(e.approach());
        let ui = e.ui(480.0, 480.0);
        assert_eq!((ui.changed_near, ui.changed_leave.is_some(), ui.far), (None, true, true));
        assert_eq!(ui.changed_screen, shown);
        e.leaving = e.leaving.map(|at| at - LEAVE_TIME);
        e.tick();
        let ui = e.ui(480.0, 480.0);
        assert_eq!((ui.screen, ui.far), ("alert", false));
    }

    #[test]
    fn farsights_weather_forecasts_three_hours_as_stock_picks_them() {
        // SAFETY: time with a null pointer only returns the time.
        let t = unsafe { libc::time(std::ptr::null_mut()) } as i64;
        let mut e = engine();
        let at = |hour: i64| t - t % 3600 - crate::history::hour_of(t) as i64 * 3600 + hour * 3600;
        let hours = |now: i64| (0..48).map(|i| Some((now + (i + 1) * 3600, i as f32))).collect::<Vec<_>>();
        // From noon: 4, 8 and 12 hours on.
        e.set_forecast(hours(at(12)));
        let view = e.forecast_view(at(12), |c| c);
        assert_eq!(view.iter().map(|h| h.0).collect::<Vec<_>>(), [3.0, 7.0, 11.0]);
        assert_eq!(view[0].1, crate::history::forecast_time(at(16), false));
        // From 10 PM the night's 2 AM moves on to 8, and the rest with it.
        e.set_forecast(hours(at(22)));
        assert_eq!(e.forecast_view(at(22), |c| c).iter().map(|h| h.0).collect::<Vec<_>>(), [9.0, 13.0, 17.0]);
        // Nothing more than 3 hours away: no forecast.
        assert!(e.forecast_view(at(22) + 48 * 3600, |c| c).is_empty());
    }

    #[test]
    fn someone_in_the_room_keeps_the_menus_open() {
        let mut e = engine();
        e.input(Input::Press);
        e.input(Input::Release);
        e.last_input -= MENU_IDLE;
        e.presence();
        assert!(!e.tick());
        assert_eq!(e.ui(480.0, 480.0).screen, "menu");
    }

    #[test]
    fn turning_in_eco_asks_to_stop_it_first() {
        let mut e = engine();
        e.settings.set(ECO, Value::Bool(true));
        e.settings.set(ECO_HEAT, Value::Number(9.0));
        assert_eq!((e.ui(480.0, 480.0).thermostat.target, e.ui(480.0, 480.0).thermostat.eco), (9.0, true));
        e.input(Input::Wheel(2));
        let ui = e.ui(480.0, 480.0);
        assert_eq!((ui.screen, ui.dialog.unwrap().selected), ("dialog", 1), "Cancel first");
        e.input(Input::Press);
        e.input(Input::Release);
        assert!(e.eco() && e.ui(480.0, 480.0).screen == "home", "Cancel keeps Eco");
        e.input(Input::Wheel(1));
        e.input(Input::Wheel(-1));
        e.input(Input::Press);
        e.input(Input::Release);
        assert!(!e.eco());
        assert_eq!(e.thermostat.target, 21.0, "the target is untouched");
    }

    #[test]
    fn time_left_is_worded_as_stocks() {
        assert_eq!(time_left(Duration::from_secs(29 * 60 + 40)), "30 minutes left", "minutes rounded up");
        assert_eq!(time_left(Duration::from_secs(60)), "1 minute left");
        assert_eq!(time_left(Duration::from_secs(61)), "2 minutes left");
        assert_eq!(time_left(Duration::from_secs(3540)), "59 minutes left");
        assert_eq!(time_left(Duration::from_secs(3541)), "1 hour left");
        assert_eq!(time_left(Duration::from_secs(84600)), "24 hours left");
        assert_eq!(time_left(Duration::from_secs(84601)), "1 day left");
        assert_eq!(time_left(Duration::from_secs(561_601)), "1 week left");
        assert_eq!(time_left(Duration::from_secs(3 * 86400 - 60)), "3 days left");
        assert_eq!(time_left(Duration::from_secs(2 * 3600 - 30)), "2 hours left");
        assert_eq!(time_left(Duration::from_secs(7 * 86400 - 60)), "1 week left");
        assert_eq!(time_left(Duration::from_secs(6 * 86400)), "6 days left");
    }

    #[test]
    fn the_fan_schedule_is_worded_under_fan_as_stocks() {
        let mut e = engine();
        assert_eq!(e.fan_schedule_line(), None);
        e.settings.set(FAN_SCHEDULE, Value::Text("on".into()));
        for (duty, start, end, line) in [(15.0, 8.0, 17.0, "On every day for 15 mins/hr from 08:00 to 17:00"),
                                         (60.0, 8.0, 17.0, "On every day from 08:00 to 17:00"),
                                         (30.0, 0.0, 0.0, "On every day for 30 mins/hr"), (60.0, 5.0, 5.0, "Always on")] {
            e.settings.set(FAN_DUTY, Value::Number(duty));
            e.settings.set(FAN_START, Value::Number(start));
            e.settings.set(FAN_END, Value::Number(end));
            assert_eq!(e.fan_schedule_line().as_deref(), Some(line));
        }
        e.settings.set("display.clock", Value::Text("12h".into()));
        e.settings.set(FAN_DUTY, Value::Number(60.0));
        e.settings.set(FAN_START, Value::Number(0.0));
        e.settings.set(FAN_END, Value::Number(13.0));
        assert_eq!(e.fan_schedule_line().as_deref(), Some("On every day from 12:00 AM to 1:00 PM"));
    }

    #[test]
    fn a_restart_carries_on_with_the_fan_run() {
        let mut e = engine();
        e.settings.set(FAN_TIMER, Value::Text("30".into()));
        let wall = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
        e.settings.set(FAN_ENDS, Value::Number(wall + 600.0));
        e.tick();
        let left = e.fan_until.unwrap().saturating_duration_since(Instant::now()).as_secs();
        assert!((598..=600).contains(&left), "{left}");
        // One that ended while the client was down is over.
        let mut e = engine();
        e.settings.set(FAN_TIMER, Value::Text("30".into()));
        e.settings.set(FAN_ENDS, Value::Number(wall - 1.0));
        e.tick();
        assert!(!e.fan_running());
        assert_eq!(e.settings.text(FAN_TIMER), Some("off"));
    }

    #[test]
    fn the_running_fans_entry_offers_stop_and_back() {
        let mut e = engine();
        e.settings.set(FAN_TIMER, Value::Text("30".into()));
        e.tick();
        e.tree.items.push(Item { label: "Fan".into(), icon: None, style: Some("dial".into()), prompt: None, shown: Shown::default(),
            kind: Kind::Choice { key: FAN_TIMER.into(), options: vec![
                Choice { value: "off".into(), label: "Fan".into() }, Choice { value: "30".into(), label: "30 minutes".into() }] } });
        e.nav.open(&e.tree);
        let fan = e.tree.items.len() - 1;
        let at = e.nav.view(&e.tree, &e.settings).unwrap().selected as i32;
        e.nav.wheel(&e.tree, fan as i32 - at);
        e.input(Input::Press);
        e.input(Input::Release);
        assert_eq!(e.nav.view(&e.tree, &e.settings).unwrap().pending, Some(0), "Stop fan first");
        e.input(Input::Wheel(1));
        assert_eq!(e.nav.view(&e.tree, &e.settings).unwrap().pending, Some(2), "then Back");
        e.input(Input::Wheel(-1));
        e.input(Input::Press);
        e.input(Input::Release);
        e.tick();
        assert!(!e.fan_running());
    }

    #[test]
    fn the_fan_timer_runs_until_it_ends_then_turns_itself_off() {
        let mut e = engine();
        e.settings.set(FAN_TIMER, Value::Text("30".into()));
        assert!(e.tick() && e.fan_running());
        e.fan_until = Some(Instant::now() - Duration::from_secs(1));
        assert!(e.tick());
        assert!(!e.fan_running());
        assert_eq!(e.settings.text(FAN_TIMER), Some("off"));
    }

    #[test]
    fn an_update_shows_under_settings_until_settings_is_selected_in_the_ring() {
        let mut e = engine();
        e.tree.items.push(Item { label: "Settings".into(), icon: None, style: None, prompt: None, shown: Shown::default(), kind: Kind::Menu(Vec::new()) });
        let online = || vec![crate::wifi::Service { state: "online".into(), ..Default::default() }];
        e.software_running("6.4-8 0.1.0", 1000.0);
        e.software_running("6.4-8 0.1.0", 2000.0);
        assert_eq!(e.settings.get(SOFTWARE_UPDATED), None, "a fresh install, then the same version: no update");
        e.software_running("6.5-1 0.1.0", 1_760_000_000.5);
        assert_eq!(e.settings.get(SOFTWARE_UPDATED), Some(&Value::Number(1_760_000_000.0)));
        e.networks = online();
        e.software_shown();
        let notice = e.settings.text(SETTINGS_NOTICE).unwrap_or_default().to_string();
        assert!(notice.starts_with("Software updated on ") && notice.len() > 20, "{notice:?}");
        e.networks = Vec::new();
        e.settings_notice();
        assert_eq!(e.settings.text(SETTINGS_NOTICE), Some("Internet disconnected"), "no internet comes first");
        e.networks = online();
        e.settings_notice();
        // Selected in the ring: seen, the line kept while the menus are open.
        let menus = e.menus();
        assert!(e.nav.open_at(&menus, &[], Some(1)));
        assert_eq!(e.nav.pointed(&menus), Some((1, "Settings")));
        e.tick();
        assert_eq!(e.settings.get(SOFTWARE_SEEN), Some(&Value::Number(1_760_000_000.0)));
        // The tick read this host's (lack of) Wi-Fi.
        e.settings.set("net.ssid", Value::Text("Home".into()));
        e.networks = online();
        e.settings_notice();
        assert_eq!(e.settings.text(SETTINGS_NOTICE), Some(notice.as_str()));
        e.software_shown();
        assert_eq!(e.settings.text(SETTINGS_NOTICE), Some(""), "gone once the menus open again");
        e.software_running("6.5-2 0.1.0", 1_770_000_000.0);
        e.software_shown();
        assert!(e.settings.text(SETTINGS_NOTICE).unwrap_or_default().starts_with("Software updated on "), "a later update shows");
    }

    #[test]
    fn a_picked_end_lets_go_when_idle() {
        let mut e = engine();
        e.settings.set(MODE, Value::Text("heat-cool".into()));
        e.input(Input::Wheel(1));
        e.last_input -= MENU_IDLE;
        assert!(e.tick());
        assert_eq!(e.ui(480.0, 480.0).thermostat.range, None);
    }

    #[test]
    fn the_display_sleeps_when_idle_and_the_input_that_wakes_it_does_nothing_else() {
        let mut e = engine();
        click(&mut e);
        e.last_input -= DISPLAY_IDLE;
        assert!(e.tick() && e.asleep());
        assert_eq!(e.ui(480.0, 480.0).screen, "home");
        assert!(!e.input(Input::Wheel(3)));
        assert!(!e.asleep());
        assert_eq!(e.thermostat.target, 21.0);
        assert!(e.input(Input::Wheel(1)));
        assert_eq!(e.thermostat.target, 21.5);
    }

    #[test]
    fn a_change_from_elsewhere_lights_the_display_for_thirty_seconds() {
        let mut e = engine();
        e.last_input -= DISPLAY_IDLE;
        assert!(e.tick() && e.asleep());
        let before = e.remote_view();
        e.set_target(23.0);
        assert_ne!(e.remote_view(), before);
        assert!(e.remote_change() && !e.asleep());
        e.tick();
        assert!(!e.asleep(), "held lit though no one has been near");
        e.remote_until = Some(Instant::now() - Duration::from_secs(1));
        assert!(e.tick() && e.asleep(), "dark again after its 30 s");
        assert_eq!(e.thermostat.target, 23.0);
    }

    #[test]
    fn the_clouds_short_cycle_raises_its_alert_and_settings_line_for_48_hours() {
        let mut e = engine();
        e.settings.set(MODE, Value::Text("cool".into()));
        assert!(!e.set_short_cycle(true, true), "ignored in cool");
        e.settings.set(MODE, Value::Text("heat".into()));
        assert!(e.set_short_cycle(true, true));
        assert!(!e.set_short_cycle(true, true), "once");
        let ui = e.ui(480.0, 480.0);
        assert_eq!(ui.screen, "dialog");
        let d = ui.dialog.unwrap();
        assert_eq!((d.title, d.text.as_str(), d.link, d.options), (Some(SHORT_CYCLE_TITLE), SHORT_CYCLE_TEXT, Some("nest.com/m13"), &["OK"][..]));
        // Under Wi-Fi not working (as off the unit), not over it.
        let line = if e.settings.text("net.ssid") == Some("Wi-Fi error") { "Wi-Fi connection error" } else { "System issue" };
        assert_eq!(e.settings.text(SETTINGS_NOTICE), Some(line));
        assert_eq!(e.settings.text(EQUIPMENT_NOTICE).map(crate::i18n::tr).as_deref(), Some("System Issue"), "on the Equipment tile");
        let since = e.short_cycle_since();
        assert!(since > 0.0 && e.short_cycle_until == Some(since + SHORT_CYCLE_HOLD));
        click(&mut e);
        assert_ne!(e.ui(480.0, 480.0).screen, "dialog", "DONE takes the alert down");
        assert_eq!(e.settings.text(SETTINGS_NOTICE), Some(line), "the line stays");
        // At start, the cloud's detection is taken back while it lasts.
        let mut again = engine();
        again.settings.set(SHORT_CYCLE_SINCE, Value::Number(since - 3600.0));
        assert!(again.short_cycle_tick());
        assert!(again.short_cycle && again.short_cycle_alert);
        assert_eq!(again.hvac.equipment.power_steal, 2, "power stealing off while heating");
        assert_eq!(again.short_cycle_until, Some(since - 3600.0 + SHORT_CYCLE_HOLD));
        // Its end clears it, and power stealing.
        again.short_cycle_until = Some(0.0);
        assert!(again.short_cycle_tick());
        assert!(!again.short_cycle && !again.short_cycle_alert);
        assert_eq!(again.hvac.equipment.power_steal, 0);
        assert_eq!(again.short_cycle_since(), 0.0);
        assert!(again.settings.text(SETTINGS_NOTICE).is_some_and(|n| n != "System issue"));
        assert_eq!(again.settings.text(EQUIPMENT_NOTICE), Some(""));
        // Past 48 h nothing is taken back.
        let mut late = engine();
        late.settings.set(SHORT_CYCLE_SINCE, Value::Number(since - SHORT_CYCLE_HOLD - 1.0));
        assert!(!late.short_cycle_tick());
        assert!(!late.short_cycle);
    }

    #[test]
    fn a_target_at_its_far_end_hints_the_other_mode_and_the_button_opens_mode() {
        let (tree, defaults) = crate::skin::load_menus(std::path::Path::new("config/menus.lua"), &[("classic".into(), "Classic".into())]).unwrap();
        let mut e = Engine::new(tree, defaults, Settings::load(None));
        for (mode, target, shown) in [("heat", 10.5, Some("cool")), ("heat", 11.0, None), ("cool", 31.0, Some("heat")), ("cool", 30.5, None),
                                      ("heat", 31.0, None), ("heat-cool", 10.0, None)] {
            e.settings.set(MODE, Value::Text(mode.into()));
            e.thermostat.target = target;
            assert_eq!(e.ui(480.0, 480.0).thermostat.press_to, shown, "{mode} {target}");
        }
        e.settings.set(MODE, Value::Text("heat".into()));
        e.thermostat.target = 10.0;
        e.thermostat.can_cool = false;
        assert_eq!(e.ui(480.0, 480.0).thermostat.press_to, None, "nothing to cool with");
        e.thermostat.can_cool = true;
        e.settings.set(UNITS, Value::Text("F".into()));
        e.thermostat.target = 10.5;
        assert_eq!(e.ui(480.0, 480.0).thermostat.press_to, Some("cool"), "51 °F");
        e.thermostat.target = 11.0;
        assert_eq!(e.ui(480.0, 480.0).thermostat.press_to, None, "52 °F");
        let selected = |e: &Engine| e.nav.view(&e.menus(), &e.settings).map(|v| v.rows[v.selected].label.clone());
        click(&mut e);
        assert_ne!(selected(&e).as_deref(), Some("Mode"));
        e.nav.close();
        e.thermostat.target = 9.0;
        click(&mut e);
        assert_eq!(selected(&e).as_deref(), Some("Mode"), "the menus open on Mode");
    }

    #[test]
    fn the_status_line_learns_of_the_lockout_counted() {
        use crate::hvac::control::{Action, Controller, Decision, Lockout, Operation};
        let decision = |controller, action, operation, lockout| {
            Some(Decision { controller, action, operation, lockout, fan: false, humidity: crate::hvac::humidity::Op::Off })
        };
        let mut e = engine();
        e.hvac.decision = decision(Controller::Lockout, Action::Drift, Operation::Off, Some((Lockout::Compressor, Operation::Stage1Heat, 150.0)));
        assert_eq!(e.ui(480.0, 480.0).thermostat.lockout, None, "nestlocal's room: the control is not ours");
        e.set_room(Some(20.0), None);
        e.hvac.decision = decision(Controller::Lockout, Action::Drift, Operation::Off, Some((Lockout::Compressor, Operation::Stage1Heat, 150.0)));
        assert_eq!(e.ui(480.0, 480.0).thermostat.lockout, Some("compressor"));
        e.hvac.decision = decision(Controller::Lockout, Action::Heat, Operation::Stage1Heat, Some((Lockout::MinimumCycle, Operation::Off, 120.0)));
        assert_eq!(e.ui(480.0, 480.0).thermostat.lockout, Some("cycle"));
    }

    #[test]
    fn a_minimum_cycle_counts_from_half_an_hour_while_the_heads_up_cools_or_rests() {
        use crate::hvac::headsup::State::*;
        for (state, cap) in [(Idle, 300), (HeatWatch, 300), (HeatRetest, 300), (HeatBlock, 300),
                             (CoolWatch, 1800), (CoolRetest, 1800), (CoolBlock, 1800), (Rest, 1800), (LongRest, 1800)] {
            assert_eq!(cycle_cap(state), cap, "{state:?}");
        }
    }

    #[test]
    fn safety_shows_with_the_room_near_a_safety_temperature_nothing_else_acts_on() {
        let mut e = engine();
        e.settings.set(SAFETY_LOW, Value::Number(5.0));
        e.settings.set(SAFETY_HIGH, Value::Number(35.0));
        let safety = |e: &Engine| e.ui(480.0, 480.0).thermostat.safety;
        e.settings.set(MODE, Value::Text("cool".into()));
        e.thermostat.current = 5.2;
        assert!(!safety(&e), "nestlocal's room: the control is not ours");
        e.set_room(Some(5.2), None);
        assert!(safety(&e), "cool does not heat");
        e.set_room(Some(5.4), None);
        assert!(!safety(&e), "0.39 °C over it");
        e.set_room(Some(5.2), None);
        e.settings.set(MODE, Value::Text("heat".into()));
        assert!(!safety(&e), "heat heats");
        e.settings.set(MODE, Value::Text("off".into()));
        assert!(safety(&e), "off");
        e.thermostat.can_heat = false;
        assert!(!safety(&e), "nothing to heat with");
        e.thermostat.can_heat = true;
        e.settings.set(MODE, Value::Text("heat".into()));
        e.settings.set(ECO, Value::Bool(true));
        e.settings.set(ECO_HEAT, Value::Number(10.0));
        assert!(!safety(&e), "Eco heats to 10 °C");
        e.settings.set(ECO_HEAT, Value::Text("off".into()));
        assert!(safety(&e), "Eco without heat");
        e.settings.set(ECO, Value::Bool(false));
        e.set_room(Some(34.5), None);
        assert!(safety(&e), "heat with the room 0.5 °C under the high one");
        e.set_room(Some(34.4), None);
        assert!(!safety(&e), "0.6 °C under it");
        e.settings.set(MODE, Value::Text("cool".into()));
        e.set_room(Some(36.0), None);
        assert!(!safety(&e), "cool cools");
    }

    #[test]
    fn the_furnace_shutdown_alert_is_a_dialog_until_done() {
        let mut e = engine();
        assert!(!e.furnace_tick(), "nothing to say");
        assert!(e.settings.text(FURNACE).is_some(), "its data kept");
        e.furnace_alert = true;
        assert_eq!(e.screen(), "dialog");
        let d = e.ui(480.0, 480.0).dialog.unwrap();
        assert_eq!((d.title, d.icon, d.text.as_str(), d.link, d.options), (None, Some("message"), FURNACE_SHUTDOWN_TEXT, Some("nest.com/furnace"), &["OK"][..]));
        click(&mut e);
        assert_eq!(e.screen(), "home");
    }

    #[test]
    fn power_off_with_y1_cut_by_a_cold_lockout_raises_its_own_alert() {
        let mut e = engine();
        e.real_backplate = true;
        e.backplate_version = 2;
        // W1, Y1 and Rh wired, no C; only W1 has power.
        let mut pins = [0; 15];
        for p in [0, 1, 4] {
            pins[p] = 1;
        }
        e.hvac.wiring = Some(crate::hvac::wiring::Wiring { pins, y2: -1, star: -1, dual_fuel: false });
        let mut wires = [0; 13];
        wires[0] = 1;
        e.wires = Some(wires);
        e.settings.set(COLD_AT, Value::Number(e.wall()));
        assert!(!e.power_tick(), "150 s first");
        e.unpowered_since = Some(Instant::now() - POWER_OFF_DELAY);
        assert!(e.power_tick());
        assert_eq!(e.alert(), Some(POWER_OFF_LOCKOUT));
        // No cold in the last 6 h: the plain power-off alert in its place.
        e.settings.set(COLD_AT, Value::Number(0.0));
        assert!(e.power_tick());
        assert_eq!(e.alert(), Some(POWER_OFF));
        wires[10] = 1;
        e.wires = Some(wires);
        assert!(e.power_tick());
        assert_eq!(e.alert(), None, "gone with power on R");
    }

    #[test]
    fn a_battery_drained_heating_without_power_stealing_is_a_system_issue() {
        let mut e = engine();
        let tick = |e: &mut Engine, volts: f32| {
            e.battery_read = None;
            e.battery_tick(|| volts)
        };
        assert!(!tick(&mut e, 3.5), "no short cycle");
        e.short_cycle = true;
        assert!(!tick(&mut e, 3.5), "power stealing on");
        e.hvac.equipment.power_steal = 2;
        assert!(!tick(&mut e, 3.6));
        for _ in 0..4 {
            assert!(!tick(&mut e, 3.5), "3.6 V among the last five");
        }
        assert!(!tick(&mut e, 0.0), "no reading");
        assert!(tick(&mut e, 3.5));
        assert!(e.hvac.battery_drained, "the HVAC control is told");
        assert!(e.ui(480.0, 480.0).thermostat.system_issue);
        assert_eq!(e.screen(), "dialog");
        let d = e.ui(480.0, 480.0).dialog.unwrap();
        assert_eq!((d.title, d.text.as_str(), d.link, d.options), (Some(BATTERY_DRAINED_TITLE), BATTERY_DRAINED_TEXT, Some("nest.com/m13"), &["OK"][..]));
        assert!(!e.battery_tick(|| 4.0), "once a minute");
        e.battery_due();
        assert!(!e.battery_tick(|| 3.5) && e.battery_read.is_some(), "and at a wake");
        click(&mut e);
        assert_eq!(e.screen(), "home");
        assert!(e.ui(480.0, 480.0).thermostat.system_issue, "answered, it stands");
        assert!(!tick(&mut e, 3.6), "kept up to 3.65 V");
        assert!(tick(&mut e, 3.66));
        assert!(!e.ui(480.0, 480.0).thermostat.system_issue);
        for _ in 0..5 {
            tick(&mut e, 3.5);
        }
        assert!(e.battery_drained);
        e.hvac.equipment.power_steal = 0;
        assert!(tick(&mut e, 3.5), "power stealing back to normal");
        assert!(!e.battery_drained && !e.hvac.battery_drained && e.screen() == "home");
    }
    #[test]
    fn a_press_on_the_power_off_alert_opens_the_wiring_report_until_r_has_power() {
        let mut e = stock_menus();
        e.real_backplate = true;
        e.backplate_version = 2;
        e.model = Some("Display-2".into());
        // W1, Y1 and Rh wired, none with power.
        let mut pins = [0; 15];
        for p in [0, 1, 4] {
            pins[p] = 1;
        }
        e.hvac.wiring = Some(crate::hvac::wiring::Wiring { pins, y2: -1, star: -1, dual_fuel: false });
        let mut wires = [0; 13];
        e.wires = Some(wires);
        assert!(!e.power_tick());
        e.unpowered_since = Some(Instant::now() - POWER_OFF_DELAY);
        assert!(e.power_tick());
        assert_eq!(e.alert(), Some(POWER_OFF));
        let safety = e.backplate_safety();
        assert!(safety.is_some());
        e.input(Input::Wheel(1));
        assert_eq!(e.alert(), Some(POWER_OFF), "a turn does nothing");
        click(&mut e);
        assert_eq!((e.alert(), e.screen()), (None, "error"));
        let page = e.ui(480.0, 480.0).error.map(|(p, _)| p.text);
        assert_eq!(page, Some("No power to Rh wire detected."), "the report's own code for R");
        assert!(e.nav.within(&e.menus(), "Equipment"), "Equipment under it");
        assert_eq!(e.backplate_safety(), safety, "the HVAC control keeps to the wiring's own error");
        assert!(!e.power_tick(), "answered: not raised again while R stays off");
        assert_eq!(e.alert(), None);
        wires[10] = 1;
        e.wires = Some(wires);
        assert!(e.power_tick(), "R powered: the report goes");
        assert_ne!(e.screen(), "error");
        wires[10] = 0;
        e.wires = Some(wires);
        assert!(!e.power_tick());
        e.unpowered_since = Some(Instant::now() - POWER_OFF_DELAY);
        assert!(e.power_tick(), "off again: raised again");
        assert_eq!(e.alert(), Some(POWER_OFF));
    }

    #[test]
    fn opening_equipment_with_the_short_cycle_flag_shows_its_alert_first() {
        let mut e = stock_menus();
        let open_equipment = |e: &mut Engine| {
            e.open_entry(&["Settings"]);
            point_at(e, "Equipment");
            click(e);
        };
        open_equipment(&mut e);
        assert_eq!(e.screen(), "menu", "no flag: straight in");
        e.nav.close();
        e.short_cycle = true;
        open_equipment(&mut e);
        assert_eq!(e.screen(), "dialog");
        let d = e.ui(480.0, 480.0).dialog.unwrap();
        assert_eq!((d.title, d.options), (Some(SHORT_CYCLE_TITLE), &["CONTINUE"][..]));
        click(&mut e);
        assert!(e.screen() != "dialog" && e.nav.within(&e.menus(), "Equipment"), "CONTINUE: on into Equipment");
        e.nav.close();
        e.battery_drained = true;
        open_equipment(&mut e);
        let d = e.ui(480.0, 480.0).dialog.unwrap();
        assert_eq!((d.title, d.options, d.selected), (Some(BATTERY_DRAINED_TITLE), &["CONTINUE", "BACK"][..], 0));
        e.input(Input::Wheel(1));
        assert_eq!(e.ui(480.0, 480.0).dialog.map(|d| d.selected), Some(1));
        click(&mut e);
        assert_eq!(e.screen(), "menu");
        assert!(!e.nav.within(&e.menus(), "Equipment"), "BACK: still in Settings");
        assert!(e.short_cycle && e.battery_drained, "answering leaves the flags");
    }

    #[test]
    fn detection_waits_for_a_clock_that_can_be_right() {
        let mut e = engine();
        e.wall_at = Some(BUILD_DATE - 60.0);
        assert!(!e.furnace_tick());
        assert!(e.settings.text(FURNACE).is_none(), "nothing taken in");
        e.wall_at = Some(BUILD_DATE + 60.0);
        assert!(!e.furnace_tick());
        assert!(e.settings.text(FURNACE).is_some(), "its data kept");
        assert!(!time_valid(BUILD_DATE - 1.0) && time_valid(BUILD_DATE));
    }

    #[test]
    fn the_furnace_shutdown_flag_offers_the_heads_up_test_once_raised() {
        let mut e = engine();
        e.furnace_flag(true);
        assert!(e.furnace_shutdown() && e.furnace_alert);
        assert_eq!(e.settings.get(FURNACE_HEADSUP_TEST), Some(&Value::Bool(true)));
        e.furnace_flag(false);
        assert!(!e.furnace_shutdown() && !e.furnace_alert);
        assert_eq!(e.settings.get(FURNACE_HEADSUP_TEST), Some(&Value::Bool(true)), "kept");
    }

    #[test]
    fn the_system_test_asks_the_control_and_an_alert_withdraws_it() {
        let mut e = engine();
        // W1, Y1, Rc, Rh, G; online with the weather.
        let mut pins = [0; 15];
        for p in [0, 1, 2, 4, 5] {
            pins[p] = 1;
        }
        e.hvac.wiring = Some(crate::hvac::wiring::Wiring { pins, y2: -1, star: -1, dual_fuel: false });
        e.action("equipment.test");
        assert_eq!(e.screen(), "systest");
        assert!(e.ui(480.0, 480.0).test.is_some_and(|v| v.text.starts_with("Your thermostat is not connected to Wi-Fi.")), "gated");
        click(&mut e);
        assert!(e.systest.is_none(), "CONTINUE leaves");
        e.settings.set("net.connected", Value::Bool(true));
        e.weather = Some((20.0, None, None, None));
        e.weather_at = Some(Instant::now());
        e.action("equipment.test");
        assert_eq!(e.ui(480.0, 480.0).test.map(|v| v.options), Some(vec!["Heating".to_string(), "Cooling".into(), "Fan".into(), "DONE".into()]));
        click(&mut e);
        assert_eq!(e.hvac.test.map(|t| t.operation), Some(crate::hvac::control::Operation::Stage1Heat));
        e.set_room(Some(21.0), None);
        e.hvac_update();
        assert!(e.hvac.is_on(crate::hvac::wiring::Switch::Stage1Heat), "W1 through the control");
        // A running stage keeps the page up past the menus' idle time.
        e.last_input -= MENU_IDLE;
        e.tick();
        assert!(e.systest.is_some());
        // An alert takes the screen: the test stops, its request goes.
        e.furnace_alert = true;
        e.tick();
        assert!(e.systest.is_none() && e.hvac.test.is_none());
    }
}
