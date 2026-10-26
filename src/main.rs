//! client-ng: the thermostat's own UI.
//!
//! The engine (Rust) owns the display, the wheel and the button, the
//! settings and the menus' navigation; a skin (Lua) draws what the engine
//! shows. See client-ng/README.md.

// The display path is Linux-only, or the desktop build's; elsewhere only
// --check runs. The desktop build leaves the unit's display unused.
#![cfg_attr(any(not(target_os = "linux"), feature = "desktop"), allow(dead_code))]

// First, so its macros are in every module after it.
#[macro_use]
mod log;

mod backplate;
mod display;
mod engine;
mod equipment;
mod ft;
mod heatlink;
mod history;
mod i18n;
mod keyboard;
mod hvac;
mod menu;
mod model;
mod network;
mod net;
mod paint;
mod postal;
mod prosetup;
mod platform;
mod reset;
mod schedule;
mod scroll;
mod skin;
mod sunblock;
mod systemtest;
mod techinfo;
mod typeset;
mod wifi;

use std::path::{Path, PathBuf};

use engine::{Engine, Input, DIAL_STEP};
use model::Settings;
use skin::Skin;

struct Options {
    /// The folder of skins; the `display.skin` setting picks one.
    skins: PathBuf,
    menus: PathBuf,
    /// Where the system drops plugins of its own.
    plugins: PathBuf,
    settings: Option<PathBuf>,
    /// Dial counts to a step, negative when counts run counterclockwise.
    wheel_step: i32,
    check: bool,
    /// Overrides for the display's geometry: face diameter, origin, quarter turns.
    face: Option<f32>,
    origin: Option<(f32, f32)>,
    turns: Option<u8>,
    /// Logs the frames drawn each second.
    fps: bool,
    cpu: bool,
    /// nestlocal's /entry, the CAs its certificate chains to, and the serial
    /// whose buckets to follow; no entry, no link.
    cloud: Option<String>,
    ca: PathBuf,
    serial: Option<String>,
    /// How long the display stays lit without input.
    display_idle: std::time::Duration,
    /// Run stock's backplate simulator in place of the backplate.
    simulate_backplate: bool,
    /// The desktop build's HTTP server for tools (HOST:PORT), and the
    /// face's pixels each way when not the display's.
    #[cfg(feature = "desktop")]
    control: String,
    #[cfg(feature = "desktop")]
    pixels: Option<usize>,
    #[cfg(feature = "desktop")]
    headless: bool,
    /// The unit's model (nlmodel) the desktop build stands in for.
    #[cfg(feature = "desktop")]
    model: Option<String>,
}

/// Whether the backplate sensors' alert may be raised (stock's setting).
const I2C_ALLOWED: &str = "i2c_alert_allowed_to_trigger";

const USAGE: &str = "usage: client-ng [--skins DIR] [--menus FILE] [--plugins DIR] [--settings FILE|--no-settings]
                       [--wheel-step N] [--face PX] [--origin X,Y] [--turns N] [--fps] [--log LEVEL] [--check]
                       [--cloud URL|--no-cloud] [--ca FILE] [--serial S] [--display-idle SECS]
                       [--simulate-backplate]

  --skins DIR      the skins to choose from in the menus, one folder each
                   (default: skins beside the binary)
  --menus FILE     the settings menus (default: config/menus.lua beside the binary)
  --plugins DIR    plugins the system drops in, with those beside the menus
                   (default: /etc/unfr0zen/plugins)
  --settings FILE  where changed settings are kept (default: settings.json beside the binary)
  --wheel-step N    the wheel's counts to a step, negative if counts fall clockwise
                   (default: -240, two of the gen 3's detents)
  --face, --origin, --turns
                   the face's diameter, its top-left in framebuffer pixels and the
                   panel's clockwise quarter turns, for displays not recognised
  --fps            log the frames drawn each second
  --log LEVEL      how much to log: error, warn, info, debug or trace (default: info, or
                   $CLIENT_NG_LOG)
  --cpu            draw on the CPU into the framebuffer even where EGL is there
  --cloud URL      nestlocal's service entry (default: https://127.0.0.1/entry, as nlclient's
                   cloudregisterurl); --no-cloud keeps the thermostat local
  --ca FILE        the CAs to trust for it (default: /etc/ssl/certs/ca-bundle.pem)
  --serial S       the serial whose buckets to follow (default: the hostname)
  --display-idle SECS
                   how long the display stays lit without input (default: 40); once
                   dark, the unit may suspend
  --simulate-backplate
                   run stock's backplate simulator in place of the backplate, as
                   backplatesimulationrun does
  --check          load every skin and the menus, draw each screen without a display, exit

The desktop build (--features desktop) draws in a window, with the skins, menus and
settings of this checkout, no cloud and nest-qemu's backplate.py by default, and takes:

  --control HOST:PORT
                   serve the screen and the inputs to tools (default: 127.0.0.1:8095)
  --pixels N       draw the face N pixels across (default: the display's, 480 points;
                   480 pixels headless); 480 compares with the gen 3's frames
  --headless       open no window: the screen and input only through --control
  --model NAME     the unit's model (nlmodel) to be (default: Display-3.4, nest-qemu's,
                   a Heat Link unit); Display-2.7 is a gen 3 with conventional wiring";

fn options() -> Result<Options, String> {
    let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)).unwrap_or_default();
    // The desktop build runs from cargo's target folder: its skins and menus
    // are the checkout's.
    #[cfg(feature = "desktop")]
    let (beside, binary) = (platform::checkout(), beside);
    let mut o = Options {
        skins: beside.join("skins"),
        menus: beside.join("config/menus.lua"),
        plugins: skin::DROP_IN.into(),
        #[cfg(not(feature = "desktop"))]
        settings: Some(beside.join("settings.json")),
        #[cfg(feature = "desktop")]
        settings: Some(binary.join("settings.json")),
        wheel_step: DIAL_STEP,
        check: false,
        face: None,
        origin: None,
        turns: None,
        fps: false,
        cpu: false,
        #[cfg(not(feature = "desktop"))]
        cloud: Some("https://127.0.0.1/entry".into()),
        #[cfg(feature = "desktop")]
        cloud: None,
        ca: "/etc/ssl/certs/ca-bundle.pem".into(),
        serial: None,
        display_idle: engine::DISPLAY_IDLE,
        simulate_backplate: false,
        #[cfg(feature = "desktop")]
        control: "127.0.0.1:8095".into(),
        #[cfg(feature = "desktop")]
        pixels: None,
        #[cfg(feature = "desktop")]
        headless: false,
        #[cfg(feature = "desktop")]
        model: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--skins" => o.skins = value("--skins")?.into(),
            "--log" => log::set(log::Level::parse(&value("--log")?).ok_or("--log needs error, warn, info, debug or trace")?),
            "--menus" => o.menus = value("--menus")?.into(),
            "--plugins" => o.plugins = value("--plugins")?.into(),
            "--settings" => o.settings = Some(value("--settings")?.into()),
            "--no-settings" => o.settings = None,
            "--wheel-step" => o.wheel_step = value("--wheel-step")?.parse().ok().filter(|&n: &i32| n != 0).ok_or("--wheel-step needs a non-zero count")?,
            "--check" => o.check = true,
            "--fps" => o.fps = true,
            "--cpu" => o.cpu = true,
            "--simulate-backplate" => o.simulate_backplate = true,
            #[cfg(feature = "desktop")]
            "--control" => o.control = value("--control")?,
            #[cfg(feature = "desktop")]
            "--headless" => o.headless = true,
            #[cfg(feature = "desktop")]
            "--model" => o.model = Some(value("--model")?),
            #[cfg(feature = "desktop")]
            "--pixels" => o.pixels = Some(value("--pixels")?.parse().ok().filter(|&n: &usize| n >= 64).ok_or("--pixels needs a size of 64 or more")?),
            "--cloud" => o.cloud = Some(value("--cloud")?),
            "--no-cloud" => o.cloud = None,
            "--ca" => o.ca = value("--ca")?.into(),
            "--serial" => o.serial = Some(value("--serial")?),
            "--display-idle" => o.display_idle = std::time::Duration::from_secs(
                value("--display-idle")?.parse().ok().filter(|&s: &u64| s > 0).ok_or("--display-idle needs seconds")?),
            "--face" => o.face = Some(value("--face")?.parse().map_err(|_| "--face needs pixels")?),
            "--turns" => o.turns = Some(value("--turns")?.parse().map_err(|_| "--turns needs 0 to 3")?),
            "--origin" => {
                let v = value("--origin")?;
                let (x, y) = v.split_once(',').ok_or("--origin needs X,Y")?;
                o.origin = Some((x.parse().map_err(|_| "--origin needs X,Y")?, y.parse().map_err(|_| "--origin needs X,Y")?));
            }
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown option {other}\n{USAGE}")),
        }
    }
    Ok(o)
}

fn engine(menus: &Path, settings: Option<PathBuf>, skins: &[(String, String)]) -> Result<Engine, String> {
    let (tree, defaults) = skin::load_menus(menus, skins).map_err(|e| format!("menus: {e}"))?;
    let history = settings.as_ref().map(|s| s.with_file_name("history.json"));
    let mut e = Engine::new(tree, defaults, Settings::load(settings));
    e.history = history::History::load(history);
    Ok(e)
}

/// The skin the settings pick, from the folder of skins.
fn load_skin(skins: &Path, engine: &Engine) -> (String, Result<Skin, String>) {
    let name = engine.settings.text(model::SKIN).unwrap_or_default().to_string();
    let skin = Skin::load(&skins.join(&name)).map_err(|e| format!("{name}: {e}"));
    (name, skin)
}

/// Draws every screen of every skin into a recorder: the skins' Lua runs
/// as it would on the unit, without a display.
fn check(o: &Options) -> Result<(), String> {
    let skins = skin::catalog(&o.skins);
    if skins.is_empty() {
        return Err(format!("no skins in {}", o.skins.display()));
    }
    for (folder, _) in &skins {
        check_skin(o, &skins, &o.skins.join(folder))?;
    }
    println!("menus {} check out", o.menus.display());
    Ok(())
}

fn check_skin(o: &Options, skins: &[(String, String)], dir: &Path) -> Result<(), String> {
    // Settings stay in memory: a check changes nothing on disk.
    let mut e = engine(&o.menus, None, skins)?;
    let steps: &[(&str, &[Input])] = &[
        ("home, heating", &[Input::Wheel(4)]),
        ("home, idle", &[Input::Wheel(-6)]),
        ("menu", &[Input::Press, Input::Release]),
        ("menu, moved", &[Input::Wheel(1)]),
        ("submenu or edit", &[Input::Press, Input::Release]),
        ("moved again", &[Input::Wheel(1)]),
        ("ring, history", &[Input::Press, Input::Release, Input::Press, Input::Release, Input::Wheel(-2)]),
        ("history page", &[Input::Press, Input::Release]),
    ];
    // Heat-cool's own home screen: at rest, an end picked, and changing it.
    let mut range = engine(&o.menus, None, skins)?;
    range.settings.set(model::MODE, model::Value::Text("heat-cool".into()));
    let range_steps: &[(&str, &[Input])] = &[
        ("heat-cool", &[]),
        ("heat-cool, picked", &[Input::Wheel(1)]),
        ("heat-cool, changing", &[Input::Press, Input::Release, Input::Wheel(2)]),
    ];
    // An alert over everything.
    let mut alerted = engine(&o.menus, None, skins)?;
    alerted.set_alert(Some(engine::REATTACH));
    let alert_steps: &[(&str, &[Input])] = &[("alert", &[]), ("alert, turned", &[Input::Wheel(2)])];
    // The wiring changed since last seen: the notice, then Equipment.
    let mut rewired = engine(&o.menus, None, skins)?;
    rewired.set_wiring([1; 15]);
    rewired.set_wiring([0; 15]);
    let rewired_steps: &[(&str, &[Input])] = &[("wiring changed", &[]), ("wiring changed, continued", &[Input::Press, Input::Release])];
    // No Rh or C wire: the error page, its TECH INFO highlighted by a turn.
    let mut unpowered = engine(&o.menus, None, skins)?;
    unpowered.set_model("Display-3.4");
    unpowered.set_wires([0; 13]);
    unpowered.set_wiring([0; 15]);
    let unpowered_steps: &[(&str, &[Input])] = &[("unpowered", &[]), ("unpowered, turned", &[Input::Wheel(1)])];
    // No W1: H71, its four choices scrolled to the last.
    let mut no_heat = engine(&o.menus, None, skins)?;
    no_heat.set_model("Display-3.4");
    no_heat.set_wires([0, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
    no_heat.set_wiring([0, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let no_heat_steps: &[(&str, &[Input])] = &[("no heat", &[]), ("no heat, last choice", &[Input::Wheel(1), Input::Wheel(3)])];
    // A wired Heat Link's wiring changed (no C): its pages, then H72.
    let mut wired = engine(&o.menus, None, skins)?;
    wired.set_model("Display-3.4");
    wired.real_backplate = true;
    wired.set_wires([1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
    wired.set_wiring([1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    wired.set_wires([1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0]);
    wired.set_wiring([1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let wired_steps: &[(&str, &[Input])] = &[("heat link wired only", &[]), ("heat link wired only, done", &[Input::Wheel(1), Input::Press, Input::Release])];
    // Lit from afar by someone in the room.
    let mut far = engine(&o.menus, None, skins)?;
    far.set_display_idle(std::time::Duration::ZERO);
    far.tick();
    far.presence();
    let far_steps: &[(&str, &[Input])] = &[("farsight", &[])];
    // The fan timer running.
    let mut fan = engine(&o.menus, None, skins)?;
    fan.settings.set(engine::FAN_TIMER, model::Value::Text("30".into()));
    fan.tick();
    let fan_steps: &[(&str, &[Input])] = &[("fan running", &[]), ("fan running, menu", &[Input::Press, Input::Release])];
    // Eco, and turning in it.
    let mut eco = engine(&o.menus, None, skins)?;
    eco.settings.set(engine::ECO, model::Value::Bool(true));
    let eco_steps: &[(&str, &[Input])] = &[("eco", &[]), ("eco, stop it?", &[Input::Wheel(1)]), ("eco, stop", &[Input::Wheel(-1)])];
    // Mode in Eco: only its press.
    let mut eco_mode = engine(&o.menus, None, skins)?;
    eco_mode.settings.set(engine::ECO, model::Value::Bool(true));
    let eco_mode_steps: &[(&str, &[Input])] = &[("mode, eco", &[Input::Press, Input::Release, Input::Wheel(1)])];
    // Mode on a system wired only to heat (W1 and G): its hint, the press
    // turning it off, and the hint then.
    let mut power = engine(&o.menus, None, skins)?;
    let mut pins = [0; 15];
    (pins[0], pins[5]) = (1, 1);
    power.hvac.wiring = Some(hvac::wiring::Wiring { pins, y2: -1, star: -1, dual_fuel: false });
    let power_steps: &[(&str, &[Input])] = &[
        ("mode, heat only", &[Input::Press, Input::Release, Input::Wheel(1)]),
        ("mode, heat only, turned off", &[Input::Press, Input::Release]),
        ("mode, heat only, off", &[Input::Press, Input::Release, Input::Wheel(1)]),
    ];
    // Stock's alerts: the date and time wrong at start, SET DATE/TIME
    // highlighted; the ring turned while off, a mode highlighted;
    // Airwave's first run; the HVAC alerts, the long one turned down.
    let mut dated = engine(&o.menus, None, skins)?;
    dated.settings.set(engine::CLOCK_ADJUSTED, model::Value::Bool(true));
    dated.date_time_check(0.0, false);
    let dated_steps: &[(&str, &[Input])] = &[("date alert", &[]), ("date alert, set", &[Input::Wheel(1)])];
    let mut switched = engine(&o.menus, None, skins)?;
    switched.settings.set(model::MODE, model::Value::Text("off".into()));
    let switched_steps: &[(&str, &[Input])] = &[("switchover alert", &[Input::Wheel(1)]), ("switchover alert, heat", &[Input::Wheel(-3)])];
    let mut airwave = engine(&o.menus, None, skins)?;
    airwave.airwave_alert = true;
    let airwave_steps: &[(&str, &[Input])] = &[("airwave alert", &[])];
    let mut cycling = engine(&o.menus, None, skins)?;
    cycling.short_cycle_alert = true;
    let cycling_steps: &[(&str, &[Input])] = &[("short cycle alert", &[])];
    let mut drained = engine(&o.menus, None, skins)?;
    drained.battery_alert = true;
    let drained_steps: &[(&str, &[Input])] = &[("battery drained alert", &[]), ("battery drained alert, turned", &[Input::Wheel(20)])];
    // The furnace shutdown alert; the battery's on the way into Equipment,
    // BACK picked; a power-off alert's press, the wiring report.
    let mut furnace = engine(&o.menus, None, skins)?;
    furnace.furnace_alert = true;
    let furnace_steps: &[(&str, &[Input])] = &[("furnace shutdown alert", &[]), ("furnace shutdown alert, turned", &[Input::Wheel(5)])];
    let mut into_equipment = engine(&o.menus, None, skins)?;
    into_equipment.battery_alert = true;
    into_equipment.equipment_alert = Some(0);
    let into_equipment_steps: &[(&str, &[Input])] = &[("battery drained alert, into equipment", &[]), ("battery drained alert, back", &[Input::Wheel(1)])];
    let mut power_off = engine(&o.menus, None, skins)?;
    power_off.set_wires([0; 13]);
    power_off.set_wiring([1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    power_off.set_alert(Some(engine::POWER_OFF));
    let power_off_steps: &[(&str, &[Input])] = &[("power off alert", &[]), ("power off, wiring report", &[Input::Press, Input::Release])];
    // Settings' Fan while off: the alert, LEAVE OFF highlighted.
    let mut off = engine(&o.menus, None, skins)?;
    off.settings.set(model::MODE, model::Value::Text("off".into()));
    off.off_alert = Some(1);
    let off_steps: &[(&str, &[Input])] = &[("off alert", &[]), ("off alert, leave off", &[Input::Wheel(-1)])];
    // A fan of three speeds: its timer at medium on the ring and its page,
    // then its schedule on the ring.
    let mut speeds = engine(&o.menus, None, skins)?;
    speeds.hvac.equipment.fan_stages = 3;
    speeds.settings.set(engine::FAN_TIMER_SPEED, model::Value::Text("stage2".into()));
    speeds.settings.set(engine::FAN_TIMER, model::Value::Text("30".into()));
    speeds.tick();
    speeds.tick();
    let mut scheduled = engine(&o.menus, None, skins)?;
    scheduled.hvac.equipment.fan_stages = 2;
    scheduled.settings.set(engine::FAN_SCHEDULE_SPEED, model::Value::Text("stage2".into()));
    scheduled.settings.set(engine::FAN_SCHEDULE, model::Value::Text("on".into()));
    scheduled.settings.set(engine::FAN_DUTY, model::Value::Number(15.0));
    scheduled.tick();
    // The running fan's sentence, STOP FAN then BACK highlighted.
    let speeds_steps: &[(&str, &[Input])] = &[
        ("fan speeds, ring", &[Input::Press, Input::Release, Input::Wheel(-4)]),
        ("fan speeds, running", &[Input::Press, Input::Release]),
        ("fan speeds, running, back", &[Input::Wheel(1)]),
    ];
    let scheduled_steps: &[(&str, &[Input])] = &[("fan schedule speed, ring", &[Input::Press, Input::Release, Input::Wheel(-4)])];
    // A fan of three speeds not running: the timer's page, a run and a
    // speed chosen, DONE starting it.
    let mut timer = engine(&o.menus, None, skins)?;
    timer.hvac.equipment.fan_stages = 3;
    timer.tick();
    let timer_steps: &[(&str, &[Input])] = &[
        ("fan timer, speeds", &[Input::Press, Input::Release, Input::Wheel(-4), Input::Press, Input::Release]),
        ("fan timer, changing the run", &[Input::Press, Input::Release, Input::Notch(2)]),
        ("fan timer, changing the speed", &[Input::Press, Input::Release, Input::Press, Input::Release, Input::Notch(1)]),
        ("fan timer, done", &[Input::Press, Input::Release, Input::Notch(1)]),
        ("fan timer, started", &[Input::Press, Input::Release]),
    ];
    // Settings' Fan with two speeds: Fan settings, the schedule with its
    // speed and its buttons, then System fan speed.
    let mut fan_menu = engine(&o.menus, None, skins)?;
    fan_menu.hvac.equipment.fan_stages = 2;
    fan_menu.tick();
    fan_menu.open_menu(&["Settings".into(), "Fan".into()], None);
    let fan_menu_steps: &[(&str, &[Input])] = &[
        ("fan settings", &[]),
        ("fan settings, schedule", &[Input::Press, Input::Release]),
        ("fan settings, schedule's speed", &[Input::Notch(3)]),
        ("fan settings, schedule's buttons", &[Input::Notch(1)]),
        ("fan settings, schedule set", &[Input::Press, Input::Release]),
        ("system fan speed", &[Input::Notch(1), Input::Press, Input::Release]),
        ("system fan speed, changing", &[Input::Press, Input::Release, Input::Notch(1)]),
        ("system fan speed, done", &[Input::Press, Input::Release, Input::Press, Input::Release]),
    ];
    // The schedule: browsing, its menu, a setpoint's time then temperature.
    let mut sch = engine(&o.menus, None, skins)?;
    sch.schedule.set(schedule::Setpoint { minute: 3 * schedule::DAY + 8 * 60, temp: 21.5, high: None });
    sch.input(Input::Press);
    sch.input(Input::Release);
    sch.input(Input::Wheel(3));
    let sch_steps: &[(&str, &[Input])] = &[
        ("schedule", &[Input::Press, Input::Release]),
        ("schedule, menu", &[Input::Press, Input::Release]),
        ("schedule, new time", &[Input::Press, Input::Release, Input::Wheel(2)]),
        ("schedule, new temperature", &[Input::Press, Input::Release, Input::Wheel(-3)]),
        ("schedule, set", &[Input::Press, Input::Release]),
        ("schedule, flung", &[Input::Wheel(6)]),
        ("schedule, dial", &[Input::Wheel(-300)]),
        ("schedule, dial after Sunday", &[Input::Wheel(300)]),
    ];
    // An empty schedule in 12-hour time: no day-start triangles.
    let mut sch12 = engine(&o.menus, None, skins)?;
    sch12.settings.set("display.clock", model::Value::Text("12h".into()));
    sch12.input(Input::Press);
    sch12.input(Input::Release);
    sch12.input(Input::Wheel(3));
    let sch12_steps: &[(&str, &[Input])] = &[
        ("schedule, empty, 12-hour", &[Input::Press, Input::Release]),
        ("schedule, empty, new", &[Input::Press, Input::Release, Input::Press, Input::Release]),
    ];
    // History: ten days, a card, an older one, a day's detail scrolled,
    // its runs reaching into both rounded ends; days of each reason with a
    // symbol of the unit font.
    let history_engine = |twelve: bool, reason: i8| -> Result<engine::Engine, String> {
        let mut hist = engine(&o.menus, None, skins)?;
        if twelve {
            hist.settings.set("display.clock", model::Value::Text("12h".into()));
        }
        for i in 0..10u32 {
            hist.history.days.push(history::Day { date: 1_792_000_000 - i64::from(i) * 86400, heat: 3600 * (i % 4), cool: 1800 * (i % 3),
                                                   leaf_earned: i % 3 == 0, missing: if i == 4 { 50400 } else { 0 }, reason, above_by: 3600,
                                                   cycles: vec![(600, 1800, true), (50000, 3600, false), (84000, 2400, true)], ..Default::default() });
        }
        hist.open_history();
        Ok(hist)
    };
    let mut hist = history_engine(false, 3)?;
    let mut hist12 = history_engine(true, 3)?;
    let hist12_steps: &[(&str, &[Input])] = &[("history, detail, 12-hour", &[Input::Press, Input::Release])];
    let mut hist_manual = history_engine(false, 0)?;
    let mut hist_weather = history_engine(false, 1)?;
    let mut hist_seasonal = history_engine(false, 4)?;
    let mut hist_dry = history_engine(false, 5)?;
    let manual_steps: &[(&str, &[Input])] = &[("history, detail, manual", &[Input::Press, Input::Release])];
    let weather_steps: &[(&str, &[Input])] = &[("history, detail, weather", &[Input::Press, Input::Release])];
    let seasonal_steps: &[(&str, &[Input])] = &[("history, detail, seasonal", &[Input::Press, Input::Release])];
    let dry_steps: &[(&str, &[Input])] = &[("history, detail, dry", &[Input::Press, Input::Release])];
    let hist_steps: &[(&str, &[Input])] = &[
        ("history", &[]),
        ("history, older", &[Input::Wheel(-6)]),
        ("history, detail", &[Input::Press, Input::Release]),
        ("history, detail scrolled", &[Input::Wheel(2)]),
        ("history, thermostat", &[Input::Press, Input::Release, Input::Wheel(30)]),
    ];
    // The keyboard: open, typed into, a function key, the next page.
    let mut kb = engine(&o.menus, None, skins)?;
    kb.action("keyboard.custom");
    let kb_steps: &[(&str, &[Input])] = &[
        ("keyboard", &[]),
        ("keyboard, typed", &[Input::Wheel(4), Input::Press, Input::Release]),
        ("keyboard, delete", &[Input::Wheel(24)]),
        ("keyboard, page", &[Input::Wheel(1), Input::Press, Input::Release]),
        ("keyboard, done", &[Input::Wheel(1)]),
    ];
    // Location's postal code: the UK's boxes, one set, DONE.
    let mut zip = engine(&o.menus, None, skins)?;
    zip.action("location.GB");
    let zip_steps: &[(&str, &[Input])] = &[
        ("postal code", &[]),
        ("postal code, set", &[Input::Wheel(3), Input::Press, Input::Release]),
        ("postal code, done", &[Input::Wheel(3), Input::Press, Input::Release, Input::Wheel(3), Input::Press, Input::Release,
                                Input::Wheel(3), Input::Press, Input::Release, Input::Wheel(3), Input::Press, Input::Release,
                                Input::Wheel(3), Input::Press, Input::Release, Input::Wheel(3), Input::Press, Input::Release]),
    ];
    // Equipment's pages open over Equipment Settings.
    const EQUIPMENT_SETTINGS: [&str; 3] = ["Settings", "Equipment", "Continue"];
    // PRO SETUP: its type list, Heating form, Hot water form, Your system.
    let mut pro = engine(&o.menus, None, skins)?;
    pro.set_model("Display-3.4");
    pro.open_entry(&EQUIPMENT_SETTINGS);
    pro.action("equipment.setup");
    // Equipment Settings' conventional pages. HEAT TYPE's two questions.
    let mut heat_type = engine(&o.menus, None, skins)?;
    heat_type.tick();
    heat_type.open_entry(&["Settings", "Equipment", "Continue", "Heat Type", "Type"]);
    let heat_type_steps: &[(&str, &[Input])] = &[("heat type", &[]), ("heat type, fuel", &[Input::Press, Input::Release])];
    // HEAT PUMP with Heat Pump Balance off: O/B, the compressor's lockout
    // set, its confirmation, aux heat's lockout; with it on, its note.
    let mut heat_pump = engine(&o.menus, None, skins)?;
    heat_pump.hvac.equipment.savings = 3;
    heat_pump.open_entry(&EQUIPMENT_SETTINGS);
    heat_pump.action("equipment.heatpump");
    let heat_pump_steps: &[(&str, &[Input])] = &[
        ("heat pump, o/b", &[]),
        ("heat pump, compressor", &[Input::Wheel(1), Input::Press, Input::Release]),
        ("heat pump, compressor changing", &[Input::Press, Input::Release, Input::Wheel(8)]),
        ("heat pump, confirm", &[Input::Press, Input::Release, Input::Press, Input::Release]),
        ("heat pump, aux", &[Input::Press, Input::Release]),
    ];
    let mut balance = engine(&o.menus, None, skins)?;
    balance.open_entry(&EQUIPMENT_SETTINGS);
    balance.action("equipment.heatpump");
    let balance_steps: &[(&str, &[Input])] = &[("heat pump, balance on", &[Input::Press, Input::Release])];
    // DUAL FUEL: the breakpoint at its lower end; without the weather, the choice.
    let mut dual = engine(&o.menus, None, skins)?;
    dual.hvac.outdoor = Some(4.0);
    dual.open_entry(&EQUIPMENT_SETTINGS);
    dual.action("equipment.dualfuel");
    let dual_steps: &[(&str, &[Input])] = &[("dual fuel", &[]), ("dual fuel, never", &[Input::Press, Input::Release, Input::Wheel(-100)])];
    let mut primary = engine(&o.menus, None, skins)?;
    primary.open_entry(&EQUIPMENT_SETTINGS);
    primary.action("equipment.dualfuel");
    let primary_steps: &[(&str, &[Input])] = &[("dual fuel, offline", &[Input::Wheel(1)])];
    // HUMIDIFIER: off, its target set, then quiet time; DEHUMIDIFIER's target.
    let mut hum = engine(&o.menus, None, skins)?;
    hum.settings.default("equipment.humidifier", model::Value::Bool(true));
    hum.open_entry(&EQUIPMENT_SETTINGS);
    hum.action("equipment.humidity");
    let hum_steps: &[(&str, &[Input])] = &[
        ("humidifier, off", &[]),
        ("humidifier, target", &[Input::Press, Input::Release, Input::Press, Input::Release, Input::Wheel(2)]),
        ("humidifier, set", &[Input::Press, Input::Release, Input::Press, Input::Release]),
        ("humidifier, quiet time", &[Input::Wheel(2), Input::Press, Input::Release]),
        ("humidifier, quiet time changing", &[Input::Press, Input::Release, Input::Wheel(1)]),
    ];
    let mut hum_off = engine(&o.menus, None, skins)?;
    hum_off.settings.set(model::MODE, model::Value::Text("off".into()));
    hum_off.open_entry(&EQUIPMENT_SETTINGS);
    hum_off.action("equipment.humidity");
    let hum_off_steps: &[(&str, &[Input])] = &[("humidifier while off", &[Input::Press, Input::Release])];
    let mut dehum = engine(&o.menus, None, skins)?;
    dehum.settings.default("equipment.humidifier", model::Value::Bool(false));
    dehum.open_entry(&EQUIPMENT_SETTINGS);
    dehum.action("equipment.humidity");
    let dehum_steps: &[(&str, &[Input])] = &[("dehumidifier, off", &[]), ("dehumidifier, target", &[Input::Press, Input::Release])];
    // A heat pump's STAR wire, and its humidifier's Pro Setup warning.
    let mut star = engine(&o.menus, None, skins)?;
    star.set_wiring([0, 1, 1, 1, 0, 1, 1, 0, 0, 0, 0, 1, 0, 0, 0]);
    star.hvac.equipment.heat_pump = true;
    star.open_entry(&EQUIPMENT_SETTINGS);
    star.action("equipment.star");
    let star_steps: &[(&str, &[Input])] = &[("star wire", &[]), ("star wire, warning", &[Input::Press, Input::Release])];
    // PRO SETUP of conventional wiring: its wires, W1's fields, one changing.
    let mut wires = engine(&o.menus, None, skins)?;
    wires.set_wiring([1, 1, 1, 1, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0, 0]);
    wires.open_entry(&EQUIPMENT_SETTINGS);
    wires.action("equipment.setup");
    let wires_steps: &[(&str, &[Input])] = &[
        ("pro setup wires", &[]),
        ("pro setup wire", &[Input::Press, Input::Release]),
        ("pro setup wire, changing", &[Input::Press, Input::Release, Input::Wheel(1)]),
    ];
    let pro_steps: &[(&str, &[Input])] = &[
        ("pro setup", &[]),
        ("pro setup, heating", &[Input::Wheel(1), Input::Press, Input::Release]),
        ("pro setup, changing", &[Input::Press, Input::Release, Input::Wheel(1)]),
        ("pro setup, hot water", &[Input::Press, Input::Release, Input::Wheel(4), Input::Press, Input::Release]),
        ("pro setup, your system", &[Input::Wheel(4), Input::Press, Input::Release]),
    ];
    // The Wi-Fi pages: connecting, stopping, online, the two failures,
    // the hidden network's security question, disconnecting.
    use network::Page;
    let wifi_pages = [
        ("wifi, connecting", Page::Connecting { name: "Nest-WPA2".into(), path: None, since: 0.0, phase: 2, stop: false, hidden: false }),
        ("wifi, stop", Page::Connecting { name: "Nest-WPA2".into(), path: None, since: 0.0, phase: 3, stop: true, hidden: false }),
        ("wifi, online", Page::Online { name: "QEMU".into() }),
        ("wifi, wrong passphrase", Page::Failed { name: "Nest-WPA2".into(), key: true, phase: 1 }),
        ("wifi, failed", Page::Failed { name: "Attic".into(), key: false, phase: 2 }),
        ("wifi, security", Page::Security { name: "Attic".into(), at: 2 }),
        ("wifi, disconnecting", Page::Disconnecting { since: 0.0 }),
    ];
    {
        let skin = Skin::load(dir).map_err(|e| format!("skin {}: {e}", dir.display()))?;
        for (name, page) in wifi_pages {
            let mut e = engine(&o.menus, None, skins)?;
            e.wifi.page = Some(page);
            draw_over_time(&skin, &e, name)?;
        }
        // The system test's pages.
        for (name, test) in systemtest::samples(&hvac::control::Equipment::default()) {
            let mut e = engine(&o.menus, None, skins)?;
            e.systest = Some(test);
            draw_over_time(&skin, &e, name)?;
        }
        // Farsight's near views, lit by someone near, then left on a press.
        for view in ["analog_clock", "digital_clock", "weather"] {
            let mut e = engine(&o.menus, None, skins)?;
            e.settings.set(engine::FARSIGHT, model::Value::Text(view.into()));
            e.set_display_idle(std::time::Duration::ZERO);
            e.tick();
            e.approach();
            draw_over_time(&skin, &e, &format!("farsight near, {view}"))?;
            e.input(Input::Press);
            e.input(Input::Release);
            draw_over_time(&skin, &e, &format!("farsight leaving, {view}"))?;
        }
        // Farsight's temperatures, lit from afar, then giving way to the
        // home screen as someone comes near or on a press.
        for view in ["target_temp", "current_temp"] {
            for near in [true, false] {
                let mut e = engine(&o.menus, None, skins)?;
                e.settings.set(engine::FARSIGHT, model::Value::Text(view.into()));
                e.set_display_idle(std::time::Duration::ZERO);
                e.tick();
                e.presence();
                draw_over_time(&skin, &e, &format!("farsight far, {view}"))?;
                if near {
                    e.approach();
                } else {
                    e.input(Input::Press);
                    e.input(Input::Release);
                }
                draw_over_time(&skin, &e, &format!("farsight to home, {view}, {}", if near { "near" } else { "press" }))?;
            }
        }
        // Off, heat-cool and Eco's far views, then giving way on a press.
        for (mode, eco) in [("off", false), ("heat-cool", false), ("heat", true)] {
            let mut e = engine(&o.menus, None, skins)?;
            e.settings.set(model::MODE, model::Value::Text(mode.into()));
            e.settings.set(engine::ECO, model::Value::Bool(eco));
            e.set_display_idle(std::time::Duration::ZERO);
            e.tick();
            e.presence();
            let name = if eco { "eco" } else { mode };
            draw_over_time(&skin, &e, &format!("farsight far, {name}"))?;
            e.input(Input::Press);
            e.input(Input::Release);
            draw_over_time(&skin, &e, &format!("farsight to home, {name}"))?;
        }
        // Farsight's target with a drained battery: the system-issue
        // symbol shows far too, then whole as it gives way on a press.
        {
            let mut e = engine(&o.menus, None, skins)?;
            e.settings.set(engine::FARSIGHT, model::Value::Text("target_temp".into()));
            e.set_short_cycle(true, false);
            e.hvac.equipment.power_steal = 2;
            e.battery_tick(|| 3.5);
            while e.screen() == "dialog" {
                e.input(Input::Press);
                e.input(Input::Release);
            }
            e.set_display_idle(std::time::Duration::ZERO);
            e.tick();
            e.presence();
            if !e.ui(480.0, 480.0).thermostat.system_issue {
                return Err("farsight far, system issue: no system issue".into());
            }
            draw_over_time(&skin, &e, "farsight far, system issue")?;
            e.input(Input::Press);
            e.input(Input::Release);
            draw_over_time(&skin, &e, "farsight to home, system issue")?;
        }
        // An alert over Farsight: hidden while far, coming in as the
        // temperatures give way on a press or the clock as someone comes near.
        for view in ["target_temp", "digital_clock"] {
            let mut e = engine(&o.menus, None, skins)?;
            e.settings.set(engine::FARSIGHT, model::Value::Text(view.into()));
            e.set_alert(Some(engine::REATTACH));
            e.set_display_idle(std::time::Duration::ZERO);
            e.tick();
            e.presence();
            draw_over_time(&skin, &e, &format!("alert over farsight, {view}"))?;
            if view == "target_temp" {
                e.input(Input::Press);
                e.input(Input::Release);
            } else {
                e.approach();
            }
            draw_over_time(&skin, &e, &format!("alert over farsight leaving, {view}"))?;
        }
        for (name, alert) in [("wifi alert, not found", engine::WifiAlert::NotFound("QEMU".into())), ("wifi alert, fault", engine::WifiAlert::Fault)] {
            let mut e = engine(&o.menus, None, skins)?;
            e.wifi_alert = Some((alert, 0));
            draw_over_time(&skin, &e, name)?;
        }
    }
    // A reset counting down; Heat Pump Balance's page, a level confirmed;
    // Eco on a heat-only system; Technical Info's EQUIPMENT open.
    let mut countdown = engine(&o.menus, None, skins)?;
    countdown.action("reset.away");
    let countdown_steps: &[(&str, &[Input])] = &[("reset countdown", &[])];
    let mut sense_balance = engine(&o.menus, None, skins)?;
    sense_balance.settings.set("equipment.balance", model::Value::Bool(true));
    sense_balance.open_menu(&["Settings".into(), "Nest Sense".into(), "Heat Pump Balance".into()], None);
    let sense_balance_steps: &[(&str, &[Input])] = &[("heat pump balance", &[]), ("heat pump balance, confirmed", &[Input::Press, Input::Release])];
    let mut eco_one = engine(&o.menus, None, skins)?;
    eco_one.settings.set("equipment.system", model::Value::Text("heat".into()));
    eco_one.open_menu(&["Settings".into()], Some(3));
    let eco_one_steps: &[(&str, &[Input])] = &[("eco, heat only", &[Input::Press, Input::Release])];
    let mut tech = engine(&o.menus, None, skins)?;
    tech.set_wires([1, 1, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 0]);
    tech.open_menu(&["Settings".into(), "Technical\nInfo".into()], None);
    let tech_steps: &[(&str, &[Input])] = &[("technical info", &[]), ("technical info, equipment", &[Input::Wheel(5), Input::Press, Input::Release])];
    // The keyboard flung round by the wheel; Reset › Schedule's BASIC
    // counting down; Display › Weather offline on a low battery; Set time
    // with the US's time zones; Lock's and Nest Pro's tiles.
    let mut kb_flung = engine(&o.menus, None, skins)?;
    kb_flung.action("keyboard.custom");
    for _ in 0..6 {
        kb_flung.rotary(-120, std::time::Instant::now());
    }
    let kb_flung_steps: &[(&str, &[Input])] = &[("keyboard, flung", &[])];
    let mut basic = engine(&o.menus, None, skins)?;
    basic.settings.set("sense.learning", model::Value::Text("off".into()));
    open_on(&mut basic, &["Settings", "Reset", "Schedule"], "Reset");
    let basic_steps: &[(&str, &[Input])] = &[("reset schedule", &[]), ("reset schedule, basic or clear", &[Input::Press, Input::Release]),
                                             ("reset schedule, basic", &[Input::Press, Input::Release])];
    let mut weather_low = engine(&o.menus, None, skins)?;
    weather_low.battery_tick(|| 3.5);
    weather_low.tick();
    open_on(&mut weather_low, &["Settings", "Display"], "Weather");
    let weather_low_steps: &[(&str, &[Input])] = &[("display, weather on a low battery", &[Input::Press, Input::Release])];
    let mut zoned = engine(&o.menus, None, skins)?;
    zoned.settings.set(engine::COUNTRY, model::Value::Text("US".into()));
    zoned.tick();
    open_on(&mut zoned, &["Settings"], "Date & Time");
    let zoned_steps: &[(&str, &[Input])] = &[("date & time offline", &[Input::Press, Input::Release]),
                                             ("set time, time zone", &[Input::Wheel(1), Input::Press, Input::Release]),
                                             ("set time, time zone chosen", &[Input::Wheel(5), Input::Press, Input::Release, Input::Wheel(1)])];
    let mut lock_tile = engine(&o.menus, None, skins)?;
    open_on(&mut lock_tile, &["Settings"], "Lock");
    let lock_tile_steps: &[(&str, &[Input])] = &[("lock's tile", &[])];
    // Each run starts the skin afresh, as the client does: a skin keeps
    // what it last drew, for its transitions.
    let mut checked = String::new();
    for (e, steps) in [(&mut e, steps), (&mut range, range_steps), (&mut alerted, alert_steps), (&mut far, far_steps), (&mut fan, fan_steps), (&mut eco, eco_steps), (&mut eco_mode, eco_mode_steps), (&mut power, power_steps), (&mut off, off_steps),
                          (&mut dated, dated_steps), (&mut switched, switched_steps), (&mut airwave, airwave_steps), (&mut cycling, cycling_steps), (&mut drained, drained_steps),
                          (&mut furnace, furnace_steps), (&mut into_equipment, into_equipment_steps), (&mut power_off, power_off_steps),
                          (&mut speeds, speeds_steps), (&mut scheduled, scheduled_steps), (&mut timer, timer_steps), (&mut fan_menu, fan_menu_steps), (&mut sch, sch_steps), (&mut sch12, sch12_steps), (&mut rewired, rewired_steps), (&mut unpowered, unpowered_steps), (&mut no_heat, no_heat_steps), (&mut wired, wired_steps), (&mut hist, hist_steps), (&mut hist12, hist12_steps), (&mut hist_manual, manual_steps), (&mut hist_weather, weather_steps), (&mut hist_seasonal, seasonal_steps), (&mut hist_dry, dry_steps), (&mut kb, kb_steps), (&mut zip, zip_steps), (&mut pro, pro_steps),
                          (&mut heat_type, heat_type_steps), (&mut heat_pump, heat_pump_steps), (&mut balance, balance_steps), (&mut dual, dual_steps),
                          (&mut primary, primary_steps), (&mut hum, hum_steps), (&mut hum_off, hum_off_steps), (&mut dehum, dehum_steps), (&mut star, star_steps), (&mut wires, wires_steps),
                          (&mut countdown, countdown_steps), (&mut sense_balance, sense_balance_steps), (&mut eco_one, eco_one_steps), (&mut tech, tech_steps),
                          (&mut kb_flung, kb_flung_steps), (&mut basic, basic_steps), (&mut weather_low, weather_low_steps), (&mut zoned, zoned_steps),
                          (&mut lock_tile, lock_tile_steps)] {
        let skin = Skin::load(dir).map_err(|e| format!("skin {}: {e}", dir.display()))?;
        checked.clone_from(&skin.name);
        for (name, inputs) in steps {
            for &input in *inputs {
                e.input(input);
            }
            e.settle();
            draw_over_time(&skin, e, name)?;
        }
    }
    println!("skin {checked:?} checks out");
    Ok(())
}

/// Opens the menu `path` on its entry `label` as shown (where several
/// entries share a label, the one showing).
fn open_on(e: &mut engine::Engine, path: &[&str], label: &str) {
    let path: Vec<String> = path.iter().map(|s| s.to_string()).collect();
    e.open_menu(&path, None);
    let at = e.ui(480.0, 480.0).menu.and_then(|m| m.rows.iter().position(|r| r.label == label));
    e.open_menu(&path, at);
}

/// Draws the screen as it is and through the next 3 s, so transitions
/// keyed on `ui.changed` run every stage of their animation.
fn draw_over_time(skin: &Skin, e: &engine::Engine, name: &str) -> Result<(), String> {
    let mut ui = e.ui(skin.design, skin.design);
    let start = ui.now;
    let mut calls = 0;
    for dt in [0.0, 0.05, 0.1, 0.2, 0.3, 0.45, 0.6, 0.8, 1.0, 1.5, 2.5, 2.7, 3.0] {
        ui.now = start + dt;
        let mut recorder = paint::Recorder::default();
        skin.draw(&mut recorder, &ui).map_err(|err| format!("{}: {name} at +{dt} s: {err}", skin.name))?;
        calls = recorder.ops.len();
    }
    println!("{}: {name}: {calls} drawing calls", skin.name);
    Ok(())
}

fn main() {
    if let Some(level) = std::env::var("CLIENT_NG_LOG").ok().and_then(|v| log::Level::parse(&v)) {
        log::set(level);
    }
    let o = match options() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    skin::set_drop_in(o.plugins.clone());
    // The log's lines are queued: a panic's message follows them.
    let panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::flush();
        panic(info);
    }));
    let result = if o.check { check(&o) } else { run(&o) };
    if let Err(e) = result {
        error!("client-ng: {e}");
        log::flush();
        std::process::exit(1);
    }
    log::flush();
}

#[cfg(not(any(target_os = "linux", feature = "desktop")))]
fn run(_: &Options) -> Result<(), String> {
    Err("the display needs the unit's Linux; on this system only --check runs".into())
}

/// Lets the HVAC control decide and the backplate switch what it decided;
/// whether what the screen shows changed.
#[cfg(any(target_os = "linux", feature = "desktop"))]
fn decide(engine: &mut Engine, backplate: &platform::backplate::Backplate) -> bool {
    let was = engine.hvac.state();
    engine.hvac_update();
    backplate.switches(engine.hvac.switches_on(), engine.hvac.circuits(), engine.hvac.equipment.dehumidifier_reversed, engine.hvac.equipment.y2 == 3);
    engine.hvac.state() != was
}

#[cfg(any(target_os = "linux", feature = "desktop"))]
fn run(o: &Options) -> Result<(), String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    static STOP: AtomicBool = AtomicBool::new(false);
    extern "C" fn stop(_: libc::c_int) {
        STOP.store(true, Ordering::SeqCst);
    }
    // SAFETY: the handler only stores to an atomic.
    unsafe {
        libc::signal(libc::SIGTERM, stop as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, stop as *const () as libc::sighandler_t);
    }
    // Named as the process it stands in for: stock's start-up waits for
    // `pidof nlclient` once monit's nlclient check has started it.
    // SAFETY: PR_SET_NAME reads a NUL-terminated name of at most 16 bytes.
    #[cfg(all(target_os = "linux", not(feature = "desktop")))]
    unsafe { libc::prctl(libc::PR_SET_NAME, c"nlclient".as_ptr()) };
    info!("client-ng: starting; skins {}, menus {}, settings {}, cloud {}, display idle {} s{}",
          crate::log::path(&o.skins), crate::log::path(&o.menus), o.settings.as_deref().map_or("not kept".into(), crate::log::path),
          o.cloud.as_deref().unwrap_or("none"), o.display_idle.as_secs(), if o.simulate_backplate { ", simulated backplate" } else { "" });
    debug!("client-ng: wheel step {} counts, cpu drawing {}, fps {}", o.wheel_step, o.cpu, o.fps);
    #[cfg(feature = "desktop")]
    {
        platform::desktop::control::start(&o.control)?;
        if let Some(n) = o.pixels {
            let _ = platform::desktop::screen::PIXELS.set(n);
        }
        platform::desktop::HEADLESS.store(o.headless, std::sync::atomic::Ordering::Relaxed);
    }
    if cfg!(all(feature = "desktop", debug_assertions)) {
        warn!("display: a debug build draws a frame in about 27 ms, under 40 a second; --release draws 60");
    }

    let device = platform::Device::current();
    info!("device: {device:?}");
    let mut screen = platform::screen::Screen::open(device, o.cpu)?;

    let (fb_width, fb_height) = screen.size();
    let mut geometry = display::Geometry::for_framebuffer(fb_width, fb_height);
    if let Some(face) = o.face { geometry.face = face; }
    if let Some(origin) = o.origin { geometry.origin = origin; }
    if let Some(turns) = o.turns { geometry.quarter_turns = turns; }
    debug!("display: {fb_width}x{fb_height}, face {} px at {:?}, {} quarter turns", geometry.face, geometry.origin, geometry.quarter_turns);

    let mut engine = engine(&o.menus, o.settings.clone(), &skin::catalog(&o.skins))?;
    #[cfg(feature = "desktop")]
    if let Some(model) = &o.model {
        info!("desktop: model {model}");
        engine.set_model(model);
    }
    let (mut skin_name, mut skin) = load_skin(&o.skins, &engine);
    match &skin {
        Ok(s) => {
            screen.use_fonts(&s.fonts)?;
            info!("skin: {} ({})", s.name, crate::log::path(&s.dir));
        }
        Err(e) => error!("skin: {e}; drawing the fallback face"),
    }

    let mut devices = platform::input::Devices::open(o.wheel_step, device);
    // Unanswered, nlheartbeatd has the hardware watchdog reset the unit.
    let mut bus = match platform::bus::Bus::open(device.heartbeat()) {
        Ok(b) => Some(b),
        Err(e) => {
            if cfg!(feature = "desktop") { debug!("bus: {e}") } else { error!("bus: {e}; nlheartbeatd will have the unit reset") }
            None
        }
    };
    let mut backlight = platform::backlight::Backlight::open().map_err(|e| if cfg!(feature = "desktop") { debug!("backlight: {e}") } else { warn!("backlight: {e}") }).ok();
    // The ambient light sensor's visible count, smoothed, for Auto brightness.
    let mut light: Option<u16> = None;
    let mut light_mean = backplate::light::LightMean::default();
    let mut connections = engine::Connections::default();
    let mut dark = false;
    // Black frames still to draw after the display went dark: stock leaves
    // the panel black, not on its last frame (a few, as a frame reaches the
    // panel only a swap or two after it is drawn).
    let mut blanks = 0u8;
    // After a suspend: when to let the unit sleep again if nothing lit the display.
    let mut woke_at: Option<Instant> = None;
    // Whether the backplate reported since the unit last woke: until it
    // has, the unit stays up, as stock's does, so a lost backplate is seen.
    let mut reported = true;
    engine.set_display_idle(o.display_idle);
    // The firmware's version and the client's: a change of either is an update.
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
    engine.software_running(&format!("{} {}", techinfo::software(), env!("CARGO_PKG_VERSION")), now);
    engine.date_time_check(now, wifi::working());
    let mut beeper = platform::beeper::Beeper::open().map_err(|e| if cfg!(feature = "desktop") { debug!("click: {e}") } else { warn!("click: {e}; staying silent") }).ok();
    let mut backplate = platform::backplate::Backplate::open(device.backplate(), o.simulate_backplate)?;
    backplate.i2c_allowed(!matches!(engine.settings.get(I2C_ALLOWED), Some(model::Value::Bool(false))));
    // The mode the HVAC control last decided for.
    let mut hvac_mode = None;
    // The power-steal mode the backplate was last given (stock starts on 0).
    let mut power_steal = 0u8;
    // Whether the backplate was last told to wake on approach.
    let mut wake_on_approach = None;
    // The safety temperatures the backplate was last given.
    let mut safety_sent = None;
    engine.real_backplate = !backplate.simulated();
    engine.set_starting(true);
    let serial = o.serial.clone().unwrap_or_else(net::hostname);
    #[cfg(feature = "desktop")]
    platform::desktop::control::set_serial(&serial);
    let mut sync = net::Sync::new(&serial);
    let mut link = match o.cloud.as_deref().map(|entry| net::Link::start(entry, &o.ca, &serial)) {
        Some(Ok(l)) => {
            info!("cloud: following buckets of {serial} via {}", o.cloud.as_deref().unwrap_or_default());
            Some(l)
        }
        Some(Err(e)) => {
            warn!("cloud: {e}; the thermostat stays local");
            None
        }
        None => None,
    };
    let plugins = o.menus.with_file_name(skin::PLUGINS);
    let mut stamp = skin::stamp(&[&o.skins, &o.menus, &plugins, &o.plugins]);
    // A changed stamp is acted on once it holds for a check: files being
    // copied in are not read half-written.
    let mut seen = stamp;
    let mut checked = Instant::now();
    let (mut dirty, mut animating) = (true, false);
    // When the skin, not animating, asked for its next frame.
    let mut wake_at: Option<Instant> = None;
    let mut settle_until = Instant::now();
    // Frames keep to a 60 Hz grid, as the panel shows them: one waking
    // late does not delay the next.
    const FRAME: Duration = Duration::from_micros(16_667);
    let pace = screen.paced().then_some(FRAME);
    let mut frame_due = Instant::now();
    let mut last_error = String::new();
    let (mut frames, mut counted) = (0u32, Instant::now());
    // Time spent drawing, flushing and swapping over the counted frames,
    // and when the last input arrived, for its latency to the screen.
    let mut spent = [Duration::ZERO; 4];
    // Passes of the loop over the counted frames: each runs the engine.
    let mut passes = 0u32;
    let mut worst = [Duration::ZERO; 6];
    // The stages before a frame is drawn, and the worst of each over the
    // counted frames: where a slow "pre" goes.
    const STAGES: [&str; 6] = ["inputs", "tick", "hvac", "backplate", "cloud", "bus"];
    let mut stage_worst = [Duration::ZERO; 6];
    let mut missed = 0u32;
    let mut slow = 0u32;
    let mut last_frame = None;
    let mut last_animating = false;
    let mut input_at: Option<Instant> = None;
    // Doom has the screen and the ring while it runs; the client keeps
    // the HVAC going but neither draws nor reads input.
    // The language the texts are shown in, chosen again when it changes.
    let locales = i18n::own_dir(&o.menus);
    let mut language = String::new();
    let mut aside = false;
    static DOOM: AtomicBool = AtomicBool::new(false);
    if !cfg!(feature = "desktop") {
        std::thread::Builder::new().name("doom-watch".into()).spawn(|| {
            while !STOP.load(Ordering::Relaxed) {
                DOOM.store(doom_running(), Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(500));
            }
        }).map_err(|e| format!("Doom process watcher: {e}"))?;
    }
    let mut deaf_until = Instant::now();
    // Every long-lived thread is running now, at the usual priority.
    platform::prioritize();
    // What the log last said of the screen, the menus and the thermostat's targets.
    let mut screen_logged = "";
    let mut place_logged: Option<String> = None;
    let mut set_to_logged = String::new();

    while !STOP.load(Ordering::SeqCst) {
        let mut fds: Vec<libc::pollfd> = devices.fds().into_iter().chain(link.as_ref().map(|l| l.fd())).chain(Some(backplate.fd()))
            .map(|fd| libc::pollfd { fd, events: libc::POLLIN, revents: 0 }).collect();
        // While drawing, the swap paces frames to the display, or the frame
        // time does; only idle waits.
        let woken = wake_at.is_some_and(|at| Instant::now() >= at);
        let drawing = !aside && !dark && (dirty || animating || woken || Instant::now() < settle_until);
        let mut timeout = match pace {
            _ if !drawing => wake_at.map_or(500, |at| at.saturating_duration_since(Instant::now()).as_micros().div_ceil(1000).min(500) as i32),
            Some(_) => frame_due.saturating_duration_since(Instant::now()).as_micros().div_ceil(1000) as i32,
            None => 0,
        };
        if let Some(at) = engine.hvac.deadline() {
            timeout = timeout.min(at.saturating_duration_since(Instant::now()).as_millis() as i32);
        }
        #[cfg(feature = "desktop")]
        {
            timeout = timeout.min(platform::input::PUMP_MS);
        }
        // SAFETY: fds is a live array of pollfd for the call's duration.
        unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) };
        let tick_started = Instant::now();
        passes += 1;
        let (mut lap, mut laps) = (tick_started, [Duration::ZERO; 6]);
        let inputs = devices.read();
        // A turn's counts first, event by event, for a carousel's physics,
        // which then leaves the same turn's detents be.
        for (counts, at) in devices.take_counts() {
            if !aside && Instant::now() >= deaf_until {
                dirty |= engine.rotary(counts, at);
            }
        }
        for input in inputs {
            // Back from Doom, the long press that ended it is still being
            // let go: nothing counts until its release (or 3 s).
            if aside || Instant::now() < deaf_until {
                if !aside && input == Input::Release {
                    deaf_until = Instant::now();
                }
                debug!("engine: {} ignored{}", said(input), if aside { " while Doom runs" } else { " until the press back from Doom is let go" });
                continue;
            }
            input_at = input_at.or(Some(Instant::now()));
            backplate.user_input();
            let on = engine.screen();
            let acted = engine.input(input);
            debug!("engine: {} on {on}{}", said(input), if acted { "" } else { ": nothing done" });
            // Schedule and History click as their wheels move them.
            let wheel = matches!(input, Input::Wheel(_)) && !matches!(on, "schedule" | "history");
            let clicks = acted && (wheel || input == Input::Release)
                && engine.settings.text(model::CLICK) != Some("off");
            if let (true, Some(b)) = (clicks, &mut beeper) {
                b.click();
            }
            dirty = true;
        }
        dirty |= engine.set_nudge(devices.pending());
        (laps[0], lap) = (lap.elapsed(), Instant::now());
        let doom = DOOM.load(Ordering::Relaxed);
        if doom != aside {
            aside = doom;
            info!("display: {}", if aside { "Doom has it" } else { "back from Doom" });
            if !aside {
                deaf_until = Instant::now() + Duration::from_secs(3);
            }
            settle_until = Instant::now() + Duration::from_millis(500);
            dirty = true;
        }
        if aside {
            engine.stand_aside();
        }
        dirty |= engine.tick();
        if let (true, Some(b)) = (engine.take_clicks() > 0 && engine.settings.text(model::CLICK) != Some("off"), &mut beeper) {
            b.click();
        }
        dirty |= engine.battery_tick(|| device.battery());
        let chosen = engine.settings.text("display.language").unwrap_or(i18n::ENGLISH);
        if chosen != language {
            language = chosen.to_string();
            i18n::select(&language, &locales);
            // The Language list, and what its tile says, from the languages installed.
            let installed = i18n::installed(&locales);
            let list: String = installed.iter().map(|(code, name)| format!("{name}|||{code}\n")).collect();
            engine.settings.default("i18n.list", model::Value::Text(list));
            if let Some((code, name)) = installed.iter().find(|(code, _)| *code == language) {
                engine.settings.default("display.language.name", model::Value::Text(i18n::tile_name(code, name)));
            }
            dirty = true;
        }
        (laps[1], lap) = (lap.elapsed(), Instant::now());
        if let Some(on) = engine.temp_comp.take() {
            backplate.temp_comp(on);
        }
        // A new mode or target is decided on at once, as stock does, and
        // a system test's request as it comes and goes.
        let mode = (engine.hvac_mode(), engine.fan_on(), engine.hvac.test);
        // So is a lockout's end.
        let lockout_over = engine.hvac.deadline().is_some_and(|at| Instant::now() >= at);
        if engine.local_sensor() && (Some(mode) != hvac_mode || lockout_over) {
            hvac_mode = Some(mode);
            dirty |= decide(&mut engine, &backplate);
        }
        // The backplate's own safety temperatures, as they change.
        let safety = engine.backplate_safety();
        if safety != safety_sent {
            safety_sent = safety;
            backplate.safety(safety);
        }
        // The thermopile wakes the unit only with Farsight's approach on.
        let approach = engine.settings.text(engine::FARSIGHT) != Some("press");
        if wake_on_approach != Some(approach) {
            wake_on_approach = Some(approach);
            backplate.wake_on_approach(approach);
        }
        if engine.hvac.equipment.power_steal != power_steal {
            power_steal = engine.hvac.equipment.power_steal;
            backplate.power_steal(power_steal);
        }
        (laps[2], lap) = (lap.elapsed(), Instant::now());
        for report in backplate.reports() {
            match report {
                backplate::link::Report::Climate { celsius, humidity } => {
                    reported = true;
                    dirty |= engine.set_room(Some(celsius), humidity);
                    dirty |= decide(&mut engine, &backplate);
                }
                backplate::link::Report::Motion { near } => {
                    engine.motions += 1;
                    dirty |= if near { engine.approach() } else { engine.presence() };
                }
                backplate::link::Report::Light { visible, .. } => {
                    light = Some(light_mean.add(visible));
                    engine.light = visible;
                }
                backplate::link::Report::Wires(wires) => engine.set_wires(wires),
                backplate::link::Report::Power { vusb, vin, current, state } => engine.power = (vusb, vin, current, state),
                backplate::link::Report::Pins(pins) => dirty |= engine.set_wiring(pins),
                backplate::link::Report::Ready(id) => {
                    dirty |= engine.set_starting(false);
                    info!("backplate: {} {} (bsl {}), serial {}", id.model, id.firmware_version, id.bsl_version, id.serial);
                    engine.backplate_identity(&id);
                    connections.comm = true;
                    dirty |= engine.connections_alert(&connections, device.battery());
                }
                backplate::link::Report::Reset { reason, hard } => {
                    engine.backplate_reset();
                    warn!("backplate: reset ({reason:?}); {}", if hard { "pulsing the reset line" } else { "sending a reset" });
                }
                backplate::link::Report::SwitchFault(on) => warn!("backplate: over-current errors {}", if on { "persist; switches cleared" } else { "cleared" }),
                backplate::link::Report::I2cDown(down) => {
                    warn!("backplate connections manager: backplate I2C {}", if down { "down" } else { "up" });
                    connections.set_i2c(!down);
                    dirty |= engine.connections_alert(&connections, device.battery());
                }
                backplate::link::Report::Faults(faults) => dirty |= engine.set_faults(faults),
                backplate::link::Report::I2cAllowed(allowed) => engine.settings.set(I2C_ALLOWED, model::Value::Bool(allowed)),
                backplate::link::Report::Down => {
                    warn!("backplate: comm is down");
                    connections.comm = false;
                    dirty |= engine.connections_alert(&connections, device.battery());
                }
                backplate::link::Report::LightBuffer(samples) => engine.light_buffer(&samples),
                backplate::link::Report::Vergence => engine.vergence(),
                backplate::link::Report::Updating => dirty |= engine.set_alert(Some(engine::UPDATING)),
                backplate::link::Report::UpdateDone(ok) => {
                    if ok { info!("backplate: firmware update complete") } else { error!("backplate: firmware update failed") }
                    if engine.alert() == Some(engine::UPDATING) {
                        dirty |= engine.set_alert(None);
                    }
                }
                backplate::link::Report::Attached(attached) => {
                    connections.set_attached(attached);
                    dirty |= engine.connections_alert(&connections, device.battery());
                }
                _ => {}
            }
        }
        (laps[3], lap) = (lap.elapsed(), Instant::now());
        if let Some(b) = &mut backlight {
            let choice = engine.settings.text("display.brightness").unwrap_or("auto");
            b.level(platform::backlight::level(choice, light), !dark);
        }
        if engine.asleep() != dark {
            dark = engine.asleep();
            blanks = if dark { 3 } else { 0 };
            info!("display: {}", if dark { "asleep" } else { "awake" });
            if let Some(b) = &mut backlight {
                b.set(!dark);
            }
            woke_at = None;
            if let Some(b) = &mut bus {
                b.hold(!dark);
            }
        }
        // Tools stand in for nestlocal on the desktop: its buckets and the
        // weather, as its link brings them.
        #[cfg(feature = "desktop")]
        for (key, value) in platform::desktop::control::take_cloud() {
            dirty |= sync.pull(&mut engine, &key, &value);
        }
        #[cfg(feature = "desktop")]
        if platform::desktop::control::take_approach() {
            dirty |= engine.approach();
        }
        #[cfg(feature = "desktop")]
        for (path, at) in platform::desktop::control::take_menu() {
            if !engine.open_menu(&path, at) {
                warn!("menu: no submenu at {}", path.join(" > "));
            }
            dirty = true;
        }
        if let Some(l) = &mut link {
            l.locate(engine.weather_place());
            for (key, value) in l.updates() {
                engine.cloud_heard();
                let before = engine.remote_view();
                dirty |= sync.pull(&mut engine, &key, &value);
                // A setting changed from elsewhere lights the display.
                if engine.remote_view() != before {
                    dirty |= engine.remote_change();
                }
            }
            let pushed = sync.push(&engine);
            if !pushed.is_empty() {
                engine.data_uploaded();
            }
            for (key, fields) in pushed {
                l.put(key, fields);
            }
        }
        (laps[4], lap) = (lap.elapsed(), Instant::now());
        if let Some(b) = &mut bus {
            let sleep = b.tick(dark && reported);
            if sleep.sleeping && engine.local_sensor() {
                engine.pre_sleep();
                if let Some(on) = engine.temp_comp.take() {
                    backplate.temp_comp(on);
                }
                backplate.quiet(engine.wake());
            }
            if sleep.woke {
                backplate.woke();
                engine.battery_due();
                reported = !engine.local_sensor();
                info!("sleep: the unit woke");
                woke_at = Some(Instant::now());
            }
            // Time for the input that woke the unit to arrive and light the display.
            if woke_at.is_some_and(|t| t.elapsed() >= Duration::from_secs(2)) {
                woke_at = None;
            }
            if woke_at.is_none() {
                b.hold(!(dark && reported));
            }
        }

        laps[5] = lap.elapsed();
        for (high, lap) in stage_worst.iter_mut().zip(laps) {
            *high = (*high).max(lap);
        }
        // Device updates restart the service; scanning the bundle pauses the UI.
        if cfg!(feature = "desktop") && checked.elapsed() >= Duration::from_millis(500) {
            checked = Instant::now();
            let now = skin::stamp(&[&o.skins, &o.menus, &plugins, &o.plugins]);
            let settled = now == seen;
            seen = now;
            if settled && now != stamp {
                stamp = now;
                // Forces the skin to load again below.
                skin_name.clear();
                info!("skins, menus or plugins changed on disk; loading them again");
                match skin::load_menus(&o.menus, &skin::catalog(&o.skins)) {
                    Ok((tree, defaults)) => {
                        info!("menus: reloaded");
                        engine.set_menus(tree, defaults);
                    }
                    Err(e) => warn!("menus: not reloaded: {e}"),
                }
            }
        }
        // A skin picked in the menus, or changed on disk, replaces the one
        // drawing; one that will not load leaves it.
        if engine.settings.text(model::SKIN).unwrap_or_default() != skin_name {
            debug!("skin: loading {}", engine.settings.text(model::SKIN).unwrap_or_default());
            let (name, loaded) = load_skin(&o.skins, &engine);
            skin_name = name;
            match loaded.and_then(|s| screen.use_fonts(&s.fonts).map(|()| s)) {
                Ok(s) => {
                    info!("skin: {} ({})", s.name, crate::log::path(&s.dir));
                    skin = Ok(s);
                    last_error.clear();
                }
                Err(e) => error!("skin: not loaded: {e}"),
            }
            // The panel can keep showing the old skin's last frame when only
            // one or two follow the swap; a few more bring the new one up.
            settle_until = Instant::now() + Duration::from_millis(500);
            dirty = true;
        }

        let screen_now = engine.screen();
        if screen_now != screen_logged {
            info!("screen: {screen_now}{}", if screen_logged.is_empty() { String::new() } else { format!(" (was {screen_logged})") });
            screen_logged = screen_now;
        }
        if log::enabled(log::Level::Debug) {
            let place = engine.menu_place();
            if place != place_logged {
                match &place {
                    Some(p) => debug!("menu: {p}"),
                    None if place_logged.is_some() => debug!("menu: closed"),
                    None => {}
                }
                place_logged = place;
            }
        }
        // Logged once the wheel has settled, not each detent.
        if engine.idle() >= Duration::from_secs(1) || set_to_logged.is_empty() {
            let set_to = engine.set_to();
            if set_to != set_to_logged {
                info!("thermostat: {set_to}");
                set_to_logged = set_to;
            }
        }
        // An image decoded since the last frame is drawn into the next.
        dirty |= screen.take_decoded();
        // A frame reaches the panel only a swap or two after it is drawn, so
        // a change keeps frames coming a little longer.
        if dirty {
            settle_until = settle_until.max(Instant::now() + Duration::from_millis(150));
        }
        let settling = Instant::now() < settle_until;
        if aside {
            continue;
        }
        if dark && blanks > 0 {
            blanks -= 1;
            let design = skin.as_ref().map(|s| s.design).unwrap_or(480.0);
            screen.draw(&geometry, design, Path::new("."), |p| p.clear(paint::Color::BLACK));
            screen.finish();
            screen.swap().map_err(|e| { error!("display: swap failed: {e}"); e })?;
            continue;
        }
        let woken = wake_at.is_some_and(|at| Instant::now() >= at);
        if dark || !(dirty || animating || woken || settling) || (pace.is_some() && Instant::now() < frame_due) {
            continue;
        }
        if let Some(frame) = pace {
            let now = Instant::now();
            frame_due = if now > frame_due + frame { now + frame } else { frame_due + frame };
        }
        let design = skin.as_ref().map(|s| s.design).unwrap_or(480.0);
        let started = Instant::now();
        let ui = engine.ui(design, geometry.face);
        let drawn = match &skin {
            Ok(s) => screen.draw(&geometry, design, &s.dir, |p| s.draw(p, &ui)),
            Err(e) => Err(e.clone()),
        };
        wake_at = skin.as_ref().ok().and_then(Skin::wake).map(|d| Instant::now() + d);
        animating = match drawn {
            Ok(a) => a,
            Err(e) => {
                if e != last_error {
                    error!("skin: {e}");
                    last_error = e.clone();
                }
                screen.draw(&geometry, design, Path::new("."), |p| skin::fallback(p, &ui, &e));
                false
            }
        };
        let drew = Instant::now();
        screen.finish();
        let flushed = Instant::now();
        screen.swap().map_err(|e| { error!("display: swap failed: {e}"); e })?;
        let presented = Instant::now();
        spent[0] += drew - started;
        spent[1] += flushed - drew;
        spent[2] += flushed.elapsed();
        spent[3] += started - tick_started;
        if let (true, Some(at)) = (o.fps, input_at.take()) {
            info!("latency: input to screen {:.1} ms", at.elapsed().as_secs_f64() * 1000.0);
        }
        dirty = false;
        if o.fps {
            frames += 1;
            let interval = if last_animating { last_frame.map_or(Duration::ZERO, |at| presented - at) } else { Duration::ZERO };
            let durations = [started - tick_started, drew - started, flushed - drew, presented - flushed,
                             presented - tick_started, interval];
            for (high, duration) in worst.iter_mut().zip(durations) {
                *high = (*high).max(duration);
            }
            if durations[4] > FRAME { slow += 1; }
            if interval > FRAME { missed += 1; }
            last_frame = Some(presented);
            last_animating = animating;
            if counted.elapsed() >= Duration::from_secs(1) {
                let ms = |d: Duration| d.as_secs_f64() * 1000.0 / f64::from(frames.max(1));
                let counts = screen.take_counts();
                let (shapes, painting, flushes) = (counts.shapes / frames.max(1), ms(counts.painting), counts.flushes / frames.max(1));
                let converting = ms(skin.as_ref().map(|s| s.converting.take()).unwrap_or_default());
                info!("fps: {frames} (ms a frame: pre {:.1}, draw {:.1} of which view {converting:.1} and painting {painting:.1} with {:.1} in {} layers, flush {:.1}, swap {:.1} of which gpu {:.1} and sync {:.1}; {shapes} shapes, {flushes} flushes, {passes} passes)",
                          ms(spent[3]), ms(spent[0]), ms(counts.layer_time), counts.layers, ms(spent[1]), ms(spent[2]), ms(counts.gpu), ms(counts.sync));
                let (stage, held) = stage_worst.iter().enumerate().max_by_key(|(_, d)| **d).map_or(("", 0.0), |(i, d)| (STAGES[i], d.as_secs_f64() * 1000.0));
                info!("frame worst ms: pre {:.1} ({stage} {held:.1}), draw {:.1}, flush {:.1}, swap {:.1}, work {:.1}, interval {:.1}; {slow} slow, {missed} missed",
                      worst[0].as_secs_f64() * 1000.0, worst[1].as_secs_f64() * 1000.0,
                      worst[2].as_secs_f64() * 1000.0, worst[3].as_secs_f64() * 1000.0,
                      worst[4].as_secs_f64() * 1000.0, worst[5].as_secs_f64() * 1000.0);
                frames = 0;
                spent = [Duration::ZERO; 4];
                passes = 0;
                worst = [Duration::ZERO; 6];
                stage_worst = [Duration::ZERO; 6];
                missed = 0;
                slow = 0;
                counted = Instant::now();
            }
        }
    }
    info!("client-ng: stopped");
    Ok(())
}

/// An input as the engine's log says it.
fn said(input: Input) -> String {
    match input {
        Input::Wheel(n) => format!("wheel {n:+} step{}", if n.abs() == 1 { "" } else { "s" }),
        Input::Notch(n) => format!("mouse wheel {n:+} notch{}", if n.abs() == 1 { "" } else { "es" }),
        Input::Press => "press".into(),
        Input::Release => "release".into(),
    }
}

/// Whether Doom is running: a process named doom (on the unit: the
/// desktop's processes are the host's).
fn doom_running() -> bool {
    if cfg!(feature = "desktop") {
        return false;
    }
    std::fs::read_dir("/proc").into_iter().flatten().flatten()
        .any(|e| std::fs::read_to_string(e.path().join("comm")).is_ok_and(|c| c.trim_end() == "doom"))
}

