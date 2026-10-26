//! Stock's System Test (systemtestlayer, 0x1c2754–0x1cc990), which
//! Equipment's TEST and Pro Setup's TEST open on its list (0x13a080,
//! 0x191c54: the layer's Wi-Fi and weather gate, else page 2). The list
//! offers each stage the wiring has (0x1c3910); a stage's page runs it
//! until DONE, its 35-minute limit (10 for an AC-integrated dehumidifier)
//! or the way out, with TROUBLESHOOT's link on the side; heating is not
//! tested above 29 °C (85 °F) outside, cooling and an AC-integrated
//! dehumidifier not below 16 °C (60 °F) (0x1c46bc, 0x1c46fc). The
//! Furnace Heads-Up, offered once the furnace shutdown detection has
//! flagged it, runs the furnace 15 minutes and stops at a loss of power.
//!
//! What a stage runs is a request to the HVAC control (crate::hvac's
//! `test`), which keeps to its safety temperatures, minimum cycles and
//! compressor lockout; while those hold the request back, the page counts
//! down as stock's compressor lockout page. Leaving the test, its time
//! limit, an alert or an error withdraws the request. Stock keeps the
//! answers for its SelfTestResult and FHUSystemTest log events, which go
//! to the cloud only; here they are logged.

use std::time::{Duration, Instant};

use crate::hvac::control::{Equipment, Operation, Test};
use crate::hvac::humidity::Op;
use crate::hvac::wiring::{Switch, Wiring};
use crate::i18n::{fill, tr};

/// A test's time limit (0x834 s), an AC-integrated dehumidifier's (600 s)
/// and the Furnace Heads-Up's run (900 s).
pub const LIMIT: Duration = Duration::from_secs(2100);
pub const DEHUMIDIFIER_LIMIT: Duration = Duration::from_secs(600);
pub const HEADS_UP: Duration = Duration::from_secs(900);

/// Stock's text for `key` (systemtestlayer's), in the language in use.
fn t(key: &str, english: &str) -> String {
    tr(&format!("@systemtestlayer.{key}|{english}"))
}

/// A stage's page (stock's pages 3 to 0x10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Fan,
    FanLow,
    FanMedium,
    FanHigh,
    Cool,
    Cool2,
    ForcedAir,
    ForcedAirGas,
    Radiant,
    Radiator,
    Baseboard,
    Humidifier,
    Dehumidifier,
    HeadsUp,
}

/// The pages a kind's time limit and completion fall under (0x2e to
/// 0x32, 0x11 to 0x1d).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Group {
    Fan,
    Heat,
    Cool,
    Humidifier,
    Dehumidifier,
}

impl Kind {
    fn group(self) -> Group {
        use Kind::*;
        match self {
            Fan | FanLow | FanMedium | FanHigh => Group::Fan,
            Cool | Cool2 => Group::Cool,
            Humidifier => Group::Humidifier,
            Dehumidifier => Group::Dehumidifier,
            ForcedAir | ForcedAirGas | Radiant | Radiator | Baseboard | HeadsUp => Group::Heat,
        }
    }

    fn heading(self) -> String {
        use Kind::*;
        match self {
            Fan => t("testingfan", "Testing Fan"),
            FanLow => t("testingfanlow.aw", "Testing low speed fan"),
            FanMedium => t("testingfanmedium.aw", "Testing medium speed fan"),
            FanHigh => t("testingfanhigh.aw", "Testing high speed fan"),
            Cool | Cool2 => t("testingcool", "Testing Cooling"),
            Humidifier => t("testinghumidifier", "Testing Humidifier"),
            Dehumidifier => t("testingdehumidifier", "Testing Dehumidifier"),
            ForcedAir | ForcedAirGas | Radiant | Radiator | Baseboard | HeadsUp => t("testingheat", "Testing Heat"),
        }
    }

    fn body(self) -> String {
        use Kind::*;
        match self {
            Fan => t("test.fan.aw", "The fan should be on. Make sure air is coming out of the vents."),
            FanLow => t("test.fanlow.aw", "Your fan should be on low speed. Make sure air is coming out of the vents."),
            FanMedium => t("test.fanmedium.aw", "Your fan should be on medium speed. Make sure air is coming out of the vents."),
            FanHigh => t("test.fanhigh.aw", "Your fan should be on high speed. Make sure air is coming out of the vents."),
            Cool => t("test.stageonecool.aw", "Air conditioning should be on. Check the vents. The air might take a few minutes to cool down."),
            Cool2 => t("test.stagetwocool.2.aw", "Cooling should be on. Check the vents. The air might take a few minutes to cool down."),
            ForcedAir => t("test.heatforcedair.aw", "The heat should be on. Check the vents. It might take a few minutes for the air to warm up."),
            ForcedAirGas => t("test.heatforcedairgas.aw", "The heat should be on. Check the vents. It might take a few minutes for the air to begin."),
            Radiant => t("test.heatradiant.aw", "The heat should be on. Radiant heat can take up to a half hour to be noticeable."),
            Radiator => t("test.heatradiator.aw", "Radiators should be warming up. Make sure the valves are open. This might take a few minutes."),
            Baseboard => t("test.heatbaseboard.aw", "The heat should be on. Electric baseboard heat can take a few minutes to be noticeable."),
            Humidifier => t("test.humidifier.aw", "The humidifier should be on."),
            Dehumidifier => t("test.dehumidifier.aw", "The dehumidifier should be on."),
            HeadsUp => t("test.furnaceheadsup.aw", "Running Furnace Heads-Up test."),
        }
    }

    /// TROUBLESHOOT's page (0x1f to 0x29): a second speed's is t11, a
    /// third's t12, of `speeds`.
    fn link(self, speeds: u8) -> String {
        use Kind::*;
        match self {
            Fan | FanLow => t("troubleshoot.fan.aw", "nest.com/t01"),
            FanMedium => t("troubleshoot.stagetwofan.aw", "nest.com/t11"),
            FanHigh if speeds < 3 => t("troubleshoot.stagetwofan.aw", "nest.com/t11"),
            FanHigh => t("troubleshoot.stagethreefan.aw", "nest.com/t12"),
            Cool => t("troubleshoot.stageonecool.aw", "nest.com/t02"),
            Cool2 => t("troubleshoot.stagetwocool.aw", "nest.com/t03"),
            ForcedAir | ForcedAirGas | HeadsUp => t("troubleshoot.heatforcedair.aw", "nest.com/t04"),
            Radiant => t("troubleshoot.heatradiant.aw", "nest.com/t05"),
            Radiator => t("troubleshoot.heatradiator.aw", "nest.com/t06"),
            Baseboard => t("troubleshoot.heatbaseboard.aw", "nest.com/t07"),
            Humidifier => t("troubleshoot.humidifier.aw", "nest.com/t08"),
            Dehumidifier => t("troubleshoot.dehumidifier.aw", "nest.com/t09"),
        }
    }
}

impl Group {
    /// The page once DONE turns it off, as stock's from Settings (its
    /// set-up's own say "… has been turned off.").
    fn completed(self) -> String {
        match self {
            Group::Fan => t("test.testingfancomplete.aw", "Testing fan completed."),
            Group::Heat => t("test.testingheatcomplete.aw", "Testing heat completed."),
            Group::Cool => t("test.testingcoolcomplete.aw", "Testing cooling completed."),
            Group::Humidifier => t("test.testinghumidifiercomplete.aw", "Testing humidifier completed."),
            Group::Dehumidifier => t("test.testingdehumidifiercomplete.aw", "Testing dehumidifier completed."),
        }
    }

    /// The time limit's page: its heading and text, after `minutes`.
    fn timeout(self, minutes: u64) -> (String, String) {
        let (heading, text) = match self {
            Group::Fan => (t("testing", "Testing"), t("timeout.fan.aw", "After %lld minutes, this test was stopped and the fan has been turned off.")),
            Group::Heat => (t("testingheat", "Testing Heat"), t("timeout.heat.aw", "After %lld minutes, this test was stopped and the heat has been turned off.")),
            Group::Cool => (t("testingcool", "Testing Cooling"), t("timeout.cool.aw", "After %lld minutes, this test was stopped and cooling has been turned off.")),
            Group::Humidifier => (t("testinghumidifier", "Testing Humidifier"),
                                  t("timeout.humidifier.aw", "After %lld minutes, this test was stopped and the humidifier has been turned off.")),
            Group::Dehumidifier => (t("testingdehumidifier", "Testing Dehumidifier"),
                                    t("timeout.dehumidifier.aw", "After %lld minutes, this test was stopped and the dehumidifier has been turned off.")),
        };
        (heading, fill(&text, &[minutes.to_string()]))
    }
}

/// What the outside must not be for a test (0x1c46bc, 0x1c46fc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outside {
    /// Heating: not above 29 °C (85 °F).
    Heat,
    /// Cooling, and an AC-integrated dehumidifier: not below 16 °C (60 °F).
    Cool,
    Dehumidifier,
}

/// One of the list's stages.
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    label: String,
    kind: Kind,
    request: Test,
    /// The page's second heading line: the stage and its wires.
    wires: String,
    limit: Duration,
    outside: Option<Outside>,
}

/// The test's pages (stock's page numbers).
#[derive(Clone, Debug, PartialEq)]
enum Page {
    /// 0x33, 0x35: no Wi-Fi, or no weather.
    Gate(String),
    /// 2: what to test.
    List,
    /// 3 to 0x10, an entry's test, or 0x37 while a lockout holds it back.
    Run(usize),
    /// 0x11 to 0x1e: an entry's test turned off.
    Done(usize),
    /// 0x1f to 0x29: where to troubleshoot, the test running on.
    Troubleshoot(usize),
    /// 0x2a: the Furnace Heads-Up's offer.
    Info(usize),
    /// 0x2b to 0x2d: too hot or too cold outside.
    Unsafe(String),
    /// 0x2e to 0x32: the test's time limit passed.
    Timeout(usize),
    /// 0x36: the furnace shut down after these seconds.
    Shutdown(u64),
}

/// A test under way: its entry, and since when the control runs it (none
/// while a lockout holds it back).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Running {
    entry: usize,
    since: Option<Instant>,
}

/// What the test reads from the thermostat each time.
#[derive(Clone, Copy, Debug)]
pub struct Env {
    pub now: Instant,
    /// The outside temperature (°C), while the weather is fresh.
    pub outdoor: Option<f32>,
    pub fahrenheit: bool,
    /// How long the HVAC control holds the request back, while it does.
    pub held: Option<Duration>,
    /// The HVAC power (Rh or Rc).
    pub powered: bool,
}

/// What the thermostat's setup gives the list.
pub struct Setup<'a> {
    pub wiring: Option<Wiring>,
    pub equipment: &'a Equipment,
    /// `show_furnace_headsup_test`.
    pub heads_up: bool,
    /// A Heat Link's heating has no wires to name (vt+0x4b4).
    pub heat_link: bool,
    /// The gate's reason, "wifi" or "weather", when it holds the test back.
    pub gate: Option<&'static str>,
}

/// A page as the skin draws it: the heading's lines, the text, a link
/// under it, the choices with the selected one and those ticked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub heading: Vec<String>,
    pub text: String,
    pub link: Option<String>,
    pub options: Vec<String>,
    pub ticked: Vec<bool>,
    pub selected: usize,
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    /// Out of the test, back where it was opened.
    Close,
}

#[derive(Clone)]
pub struct SystemTest {
    entries: Vec<Entry>,
    /// The entries tested to DONE (stock's list flag 8).
    tested: Vec<bool>,
    page: Page,
    at: usize,
    /// The entry last chosen on the list, selected again on the way back.
    chosen: usize,
    running: Option<Running>,
    held: Option<Duration>,
    /// The countdown's second last shown.
    second: Option<u64>,
    /// When the test opened, and what it went through, for the log.
    opened: Instant,
    timed_out: bool,
    troubleshot: bool,
    /// The fan's speeds the wiring has.
    speeds: u8,
}

/// A temperature's whole degrees as the unit shows them.
fn degrees(c: f32, fahrenheit: bool) -> i32 {
    (if fahrenheit { c * 1.8 + 32.0 } else { c }).round() as i32
}

/// "m:ss", stock's countdowns' format ("%.1d:%.2d").
fn countdown(d: Duration) -> String {
    let s = d.as_secs_f64().ceil() as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// The wires a request switches, as stock names them (0x1c985c): each
/// stage's terminal in its order, "%s wire" or "%s wires".
fn wires(w: &Wiring, switches: &[Switch]) -> String {
    const ORDER: [Switch; 14] = [Switch::Fan, Switch::Stage2Fan, Switch::Stage3Fan, Switch::Stage1Cool, Switch::Stage2Cool, Switch::Stage1Heat,
                                 Switch::Stage2Heat, Switch::Stage3Heat, Switch::Stage1AltHeat, Switch::Stage2AltHeat, Switch::AuxHeat,
                                 Switch::EmergencyHeat, Switch::Humidifier, Switch::Dehumidifier];
    // The circuits' terminals; the rest are named nothing.
    let name = |c: u8| match c {
        0 => Some("W1"),
        1 => Some("Y1"),
        2 => Some("G"),
        3 => Some("OB"),
        4 => Some("W2/AUX"),
        7 => Some("Y2"),
        11 => Some("*"),
        _ => None,
    };
    let names: Vec<&str> = ORDER.iter().filter(|s| switches.contains(s)).filter_map(|&s| w.circuit(s).and_then(name)).collect();
    match names.len() {
        0 => String::new(),
        1 => fill(&t("wire", "%s wire"), &[names[0].to_string()]),
        _ => fill(&t("wires", "%s wires"), &[names.join(",")]),
    }
}

impl SystemTest {
    /// The test as Equipment's or Pro Setup's TEST opens it: the gate's
    /// page, else the list of what the wiring has (0x1c3910).
    pub fn open(setup: &Setup) -> SystemTest {
        let entries = setup.wiring.as_ref().map_or_else(Vec::new, |w| entries(w, setup));
        let speeds = setup.wiring.as_ref().map_or(1, |w| if w.exists(Switch::Stage3Fan) { 3 } else if w.exists(Switch::Stage2Fan) { 2 } else { 1 });
        let page = match setup.gate {
            Some("wifi") => Page::Gate(t("noconnectivity.aw", "Your thermostat is not connected to Wi-Fi. Once connected, you'll be able to run a test of your system from settings.")),
            Some(_) => Page::Gate(t("noweather.aw", "Your thermostat currently does not have access to weather information. Once available, you will be able to run a test of your system from within settings.")),
            None => Page::List,
        };
        info!("systest: opened; {}", if let Page::Gate(_) = page { format!("held back without {}", setup.gate.unwrap_or("")) }
              else { format!("testing {}", entries.iter().map(|e| e.label.as_str()).collect::<Vec<_>>().join(", ")) });
        SystemTest { tested: vec![false; entries.len()], entries, page, at: 0, chosen: 0, running: None, held: None, second: None, opened: Instant::now(),
                     timed_out: false, troubleshot: false, speeds }
    }

    /// What the HVAC control is asked to run.
    pub fn request(&self) -> Option<Test> {
        self.running.map(|r| self.entries[r.entry].request)
    }

    /// Whether a stage is being tested (the screen then stays open).
    pub fn testing(&self) -> bool {
        self.running.is_some()
    }

    /// The test's entries' labels, for the log.
    #[cfg(test)]
    fn labels(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.label.as_str()).collect()
    }

    fn options(&self) -> usize {
        match &self.page {
            Page::List => self.entries.len() + 1,
            Page::Run(i) if self.entries[*i].kind == Kind::HeadsUp && !self.waiting() => 1,
            Page::Run(_) if self.waiting() => 1,
            Page::Run(_) | Page::Troubleshoot(_) | Page::Info(_) | Page::Timeout(_) | Page::Shutdown(_) => 2,
            Page::Gate(_) | Page::Done(_) | Page::Unsafe(_) => 1,
        }
    }

    /// A lockout holds the test back before it has started (stock's 0x37).
    fn waiting(&self) -> bool {
        self.running.is_some_and(|r| r.since.is_none()) && self.held.is_some()
    }

    fn go(&mut self, page: Page) {
        if page == Page::List {
            self.at = self.chosen;
        } else {
            self.at = 0;
        }
        self.page = page;
        self.second = None;
    }

    pub fn wheel(&mut self, d: i32) {
        self.at = (self.at as i64 + i64::from(d)).clamp(0, self.options() as i64 - 1) as usize;
    }

    /// Starts entry `i`'s test, unless the outside rules it out.
    fn start(&mut self, i: usize, env: &Env) {
        let e = &self.entries[i];
        if let Some(rule) = e.outside {
            let out = env.outdoor.map(|c| degrees(c, env.fahrenheit));
            let (limit, safe) = match rule {
                Outside::Heat => {
                    let limit = if env.fahrenheit { 85 } else { 29 };
                    (limit, out.is_some_and(|o| o <= limit))
                }
                Outside::Cool | Outside::Dehumidifier => {
                    let limit = if env.fahrenheit { 60 } else { 16 };
                    (limit, out.is_some_and(|o| o >= limit))
                }
            };
            if !safe {
                let now = out.map_or_else(|| "--°".to_string(), |o| format!("{o}°"));
                let text = match rule {
                    Outside::Heat => t("unsafeheat.aw", "It is currently %ls outside. To protect your system, your thermostat does not test heating when the outside temperature is above %ls."),
                    Outside::Cool => t("unsafecool.aw", "It is currently %ls outside. To protect your system, your thermostat does not test cooling when the outside temperature is below %ls."),
                    Outside::Dehumidifier => t("unsafedehum.aw", "It is currently %ls outside. To protect your system, your thermostat does not test AC-integrated dehumidifiers when the outside temperature is below %ls."),
                };
                info!("systest: {} not tested: {now} outside, the limit {limit}°", e.label);
                self.go(Page::Unsafe(fill(&text, &[now, format!("{limit}°")])));
                return;
            }
        }
        info!("systest: {} test started{}", e.label, if e.wires.is_empty() { String::new() } else { format!(" {}", e.wires) });
        self.running = Some(Running { entry: i, since: None });
        self.held = env.held;
        self.go(Page::Run(i));
    }

    /// Withdraws the request: the control decides as before the test.
    fn stop(&mut self, why: &str) {
        if let Some(r) = self.running.take() {
            let ran = r.since.map_or(0, |at| at.elapsed().as_secs());
            info!("systest: {} test {why} after {ran} s", self.entries[r.entry].label);
        }
        self.held = None;
    }

    /// The test closes: whatever runs stops (on the way out, an alert or
    /// an error), and the results are logged as stock sends them.
    pub fn close(&mut self) {
        self.stop("stopped");
        let tested: Vec<&str> = self.entries.iter().zip(&self.tested).filter(|(_, &t)| t).map(|(e, _)| e.label.as_str()).collect();
        info!("systest: result: {} s, tested {}, timed out {}, troubleshooting seen {}", self.opened.elapsed().as_secs(),
              if tested.is_empty() { "nothing".to_string() } else { tested.join(", ") }, self.timed_out, self.troubleshot);
    }

    pub fn press(&mut self, env: &Env) -> Outcome {
        let at = self.at;
        match self.page.clone() {
            Page::Gate(_) => return Outcome::Close,
            Page::List if at == self.entries.len() => return Outcome::Close,
            Page::List => {
                self.chosen = at;
                if self.entries[at].kind == Kind::HeadsUp {
                    self.go(Page::Info(at));
                } else {
                    self.start(at, env);
                }
            }
            Page::Run(_) if self.waiting() => {
                self.stop("given up waiting");
                self.go(Page::List);
            }
            Page::Run(i) if self.entries[i].kind == Kind::HeadsUp => {
                self.stop("cancelled");
                self.go(Page::List);
            }
            Page::Run(i) if at == 0 => {
                self.stop("done");
                self.tested[i] = true;
                self.go(Page::Done(i));
            }
            Page::Run(i) => {
                self.troubleshot = true;
                self.go(Page::Troubleshoot(i));
            }
            // KEEP TESTING starts it again; DONE stops it.
            Page::Troubleshoot(i) if at == 0 => self.start(i, env),
            Page::Troubleshoot(_) => {
                self.stop("done");
                self.go(Page::List);
            }
            // RUN TEST; TEST AGAIN; RUN TEST AGAIN.
            Page::Info(i) if at == 0 => self.start(i, env),
            Page::Timeout(i) if at == 1 => self.start(i, env),
            Page::Shutdown(_) if at == 0 => {
                let i = self.chosen;
                self.start(i, env);
            }
            Page::Done(_) | Page::Unsafe(_) | Page::Info(_) | Page::Timeout(_) | Page::Shutdown(_) => self.go(Page::List),
        }
        Outcome::Stay
    }

    /// Time passing: a lockout's hold, the test's limit, and the furnace's
    /// power during the Heads-Up. True when the page changed.
    pub fn tick(&mut self, env: &Env) -> bool {
        let Some(r) = self.running else { return false };
        let e = &self.entries[r.entry];
        let mut changed = false;
        if r.since.is_none() {
            if self.held.is_some() != env.held.is_some() {
                changed = true;
            }
            self.held = env.held;
            if env.held.is_none() {
                debug!("systest: {} running", e.label);
                self.running = Some(Running { since: Some(env.now), ..r });
            }
        }
        let ran = self.running.and_then(|r| r.since).map_or(Duration::ZERO, |at| env.now.saturating_duration_since(at));
        if e.kind == Kind::HeadsUp && self.running.is_some_and(|r| r.since.is_some()) && !env.powered {
            info!("systest: Furnace Heads-Up: the furnace shut down after {} s", ran.as_secs());
            self.stop("stopped by the furnace");
            self.go(Page::Shutdown(ran.as_secs()));
            return true;
        }
        if ran >= e.limit {
            let i = r.entry;
            if e.kind == Kind::HeadsUp {
                self.stop("done");
                self.tested[i] = true;
                self.go(Page::Done(i));
            } else {
                info!("systest: {} test timed out", e.label);
                self.stop("timed out");
                self.timed_out = true;
                self.go(Page::Timeout(i));
            }
            return true;
        }
        // The countdowns' seconds.
        let shown = if self.waiting() { self.held.map(|h| h.as_secs()) } else if e.kind == Kind::HeadsUp { Some((e.limit - ran).as_secs()) } else { None };
        if matches!(self.page, Page::Run(_)) && shown != self.second {
            self.second = shown;
            changed = true;
        }
        changed
    }

    pub fn view(&self) -> View {
        let mut v = View { selected: self.at, ..View::default() };
        let done = || tr("DONE");
        match &self.page {
            Page::Gate(text) => {
                v.text = text.clone();
                v.options = vec![tr("CONTINUE")];
            }
            Page::List => {
                v.text = t("whattotest.aw", "What do you want to test?");
                v.options = self.entries.iter().map(|e| e.label.clone()).chain(std::iter::once(done())).collect();
                v.ticked = self.tested.iter().copied().chain(std::iter::once(false)).collect();
            }
            Page::Run(_) if self.waiting() => {
                v.text = format!("{}{}", t("compressorlockout.aw", "To protect your system, your thermostat will start the test in:\n\n"),
                                 countdown(self.held.unwrap_or_default()));
                v.options = vec![tr("BACK")];
            }
            Page::Run(i) if self.entries[*i].kind == Kind::HeadsUp => {
                let ran = self.running.and_then(|r| r.since).map_or(Duration::ZERO, |at| at.elapsed());
                v.text = format!("{}\n\n{}", Kind::HeadsUp.body(), countdown(HEADS_UP.saturating_sub(ran)));
                v.options = vec![tr("CANCEL")];
            }
            Page::Run(i) => {
                let e = &self.entries[*i];
                v.heading = std::iter::once(e.kind.heading()).chain((!e.wires.is_empty()).then(|| e.wires.clone())).collect();
                v.text = e.kind.body();
                v.options = vec![done(), t("troubleshoot.choice", "TROUBLESHOOT")];
            }
            Page::Done(i) => {
                let e = &self.entries[*i];
                v.text = if e.kind == Kind::HeadsUp { t("test.furnaceheadsupdone.aw", "The test finished and no issues were found.") } else { e.kind.group().completed() };
                v.options = vec![done()];
            }
            Page::Troubleshoot(i) => {
                v.text = t("troubleshoot.goto.aw", "Find details about troubleshooting this test at");
                v.link = Some(self.entries[*i].kind.link(self.speeds));
                v.options = vec![t("keeptesting.choice", "KEEP TESTING"), done()];
            }
            Page::Info(_) => {
                v.text = t("furnaceheadsup.info.aw", "Run a 15 minute test of your furnace to check for the issue identified by the Furnace Heads-Up alert.");
                v.options = vec![t("runtest.choice", "RUN TEST"), tr("BACK")];
            }
            Page::Unsafe(text) => {
                v.text = text.clone();
                v.options = vec![tr("BACK")];
            }
            Page::Timeout(i) => {
                let e = &self.entries[*i];
                let (heading, text) = e.kind.group().timeout(e.limit.as_secs() / 60);
                v.heading = vec![heading, t("teststopped", "- Stopped -")];
                v.text = text;
                v.options = vec![t("skiptest.choice", "SKIP TEST"), t("testagain.choice", "TEST AGAIN")];
            }
            Page::Shutdown(secs) => {
                v.text = if *secs < 120 {
                    t("furnaceheadsup.shutdown.single.aw", "Your furnace ran for one minute before shutting down. It may not be working properly. You might want to contact a Nest Pro.")
                } else {
                    fill(&t("furnaceheadsup.shutdown.plural.aw", "Your furnace ran for %d minutes before shutting down. It may not be working properly. You might want to contact a Nest Pro."),
                         &[(secs / 60).to_string()])
                };
                v.options = vec![t("runagain", "RUN TEST AGAIN"), done()];
            }
        }
        v
    }
}

/// The list's entries (0x1c3910): the Furnace Heads-Up first when it is
/// offered, then each stage the wiring has in stock's order, heat, cool,
/// fan, humidity, fan speeds. With a fan of more than one speed the second
/// and third stages are not offered on their own (0x2e4b68).
fn entries(w: &Wiring, s: &Setup) -> Vec<Entry> {
    let e = s.equipment;
    let heat_pump = w.exists(Switch::Ob);
    let multi_speed = w.exists(Switch::Stage2Fan) || w.exists(Switch::Stage3Fan);
    let three_speeds = w.exists(Switch::Stage3Fan);
    let mut out = Vec::new();
    let delivery = |s: Switch| e.delivery[s as usize];
    // A heat stage's page by its delivery (0x1c380c): forced air from gas
    // whose furnace runs its own fan says so.
    let heat_kind = |s: Switch| match delivery(s) {
        1 => Kind::Radiant,
        2 => Kind::Radiator,
        3 => Kind::Baseboard,
        _ if e.source[s as usize] == 0 && e.fan_type[s as usize] > 0 => Kind::ForcedAirGas,
        _ => Kind::ForcedAir,
    };
    let entry = |label: String, kind: Kind, request: Test, line: Option<String>, outside: Option<Outside>| {
        let switches = crate::hvac::control::switches(request.operation, request.humidity, false, e);
        let mut on = switches;
        if let Some(speed) = request.fan {
            on.push([Switch::Fan, Switch::Stage2Fan, Switch::Stage3Fan][usize::from(speed.clamp(1, 3) - 1)]);
        }
        let named = wires(w, &on);
        let wires = match line {
            Some(stage) if !named.is_empty() => format!("{stage} ({named})"),
            Some(stage) => stage,
            None if named.is_empty() => String::new(),
            None => format!("({named})"),
        };
        let limit = if request.humidity == Op::AcIntegratedDehumidify { DEHUMIDIFIER_LIMIT } else if kind == Kind::HeadsUp { HEADS_UP } else { LIMIT };
        Entry { label, kind, request, wires: wires.trim().to_string(), limit, outside }
    };
    let op = |operation: Operation| Test { operation, humidity: Op::Off, fan: None };
    // The Furnace Heads-Up's stage (0xcc370): the furnace, forced air.
    if s.heads_up {
        let furnace = if heat_pump { [Switch::Stage1AltHeat, Switch::AuxHeat].into_iter().find(|&s| w.exists(s) && delivery(s) == 0) }
                      else { Some(Switch::Stage1Heat).filter(|&s| w.exists(s) && delivery(s) == 0) };
        if let Some(f) = furnace {
            let operation = match f { Switch::Stage1AltHeat => Operation::Stage1AltHeat, Switch::AuxHeat => Operation::AuxOnly, _ => Operation::Stage1Heat };
            out.push(entry(t("furnaceheadsup.choice", "Furnace Heads-Up"), Kind::HeadsUp, op(operation), None, None));
        }
    }
    let heat_line = |key: &str, english: &str| if s.heat_link { None } else { Some(t(key, english)) };
    let heats: [(Switch, Operation, String, Option<String>); 7] = [
        (Switch::Stage1Heat, Operation::Stage1Heat,
         if heat_pump { t("heatpumpheating.choice", "Heat pump heating") } else { t("heating.choice", "Heating") }, None),
        (Switch::Stage2Heat, Operation::Stage2Heat,
         if heat_pump { t("heatpumpheatingx2.choice", "Heating stage 2") } else { t("twostageheating.choice", "Heating stage 2") }, heat_line("stagetwoheat", "Stage 2")),
        (Switch::Stage3Heat, Operation::Stage3Heat, t("threestageheating.choice", "Heating stage 3"), heat_line("stagethreeheat", "Stage 3")),
        (Switch::Stage1AltHeat, Operation::Stage1AltHeat, t("altheating.choice", "Alternate heating"), heat_line("stageonealternateheat", "Stage 1 Alt")),
        (Switch::Stage2AltHeat, Operation::Stage2AltHeat, t("stagetwoaltheating.choice", "Alt. heat stage 2"), heat_line("stagetwoalternateheat", "Stage 2 Alt")),
        (Switch::AuxHeat, Operation::AuxOnly, t("auxheat.choice", "Auxiliary heating"), heat_line("auxiliaryheat", "Auxiliary")),
        (Switch::EmergencyHeat, Operation::EmergencyHeat, t("emergencyheating.choice", "Emergency heating"), heat_line("emergencyheat", "Emergency")),
    ];
    let skipped = |sw: Switch| multi_speed && matches!(sw, Switch::Stage2Cool | Switch::Stage3Cool | Switch::Stage2Heat | Switch::Stage3Heat | Switch::Stage2AltHeat);
    for (sw, operation, label, line) in heats {
        if !w.exists(sw) || skipped(sw) {
            continue;
        }
        let mut en = entry(label, heat_kind(sw), op(operation), line, Some(Outside::Heat));
        // A Heat Link's heating names no wires.
        if s.heat_link {
            en.wires.clear();
        }
        out.push(en);
    }
    if w.exists(Switch::Stage1Cool) {
        let label = if heat_pump { t("heatpumpcooling.choice", "Heat pump cooling") } else { t("cooling.choice", "Cooling") };
        out.push(entry(label, Kind::Cool, op(Operation::Stage1Cool), None, Some(Outside::Cool)));
    }
    if w.exists(Switch::Stage2Cool) && !skipped(Switch::Stage2Cool) {
        let label = if heat_pump { t("heatpumpcoolingx2.choice", "Cooling stage 2") } else { t("stagetwocooling.choice", "Cooling stage 2") };
        out.push(entry(label, Kind::Cool2, op(Operation::Stage2Cool), Some(t("test.stagetwocool.1", "Stage 2")), Some(Outside::Cool)));
    }
    let fan = |speed: u8| Test { operation: Operation::Off, humidity: Op::Off, fan: Some(speed) };
    if w.exists(Switch::Fan) {
        let (label, kind) = if multi_speed { (t("fanlow.choice", "Low fan"), Kind::FanLow) } else { (t("fan.choice", "Fan"), Kind::Fan) };
        out.push(entry(label, kind, fan(1), None, None));
    }
    if w.exists(Switch::Humidifier) {
        // A bypass humidifier runs with the furnace (0x1c43fc).
        let operation = if e.humidity.humidifier == 2 { Operation::Stage1Heat } else { Operation::Off };
        out.push(entry(t("humidifier.choice", "Humidifier"), Kind::Humidifier, Test { operation, humidity: Op::Humidify, fan: None }, None, None));
    }
    if w.exists(Switch::Dehumidifier) {
        // An AC-integrated one runs with the first cool stage.
        let request = if e.humidity.dehumidifier == 1 { Test { operation: Operation::Off, humidity: Op::Dehumidify, fan: None } }
                      else { Test { operation: Operation::Stage1Cool, humidity: Op::AcIntegratedDehumidify, fan: None } };
        let outside = (request.operation == Operation::Stage1Cool).then_some(Outside::Dehumidifier);
        out.push(entry(t("dehumidifier.choice", "Dehumidifier"), Kind::Dehumidifier, request, None, outside));
    }
    if w.exists(Switch::Stage2Fan) {
        let (label, kind) = if three_speeds { (t("fanmedium.choice", "Medium fan"), Kind::FanMedium) } else { (t("fanhigh.choice", "High fan"), Kind::FanHigh) };
        out.push(entry(label, kind, fan(2), None, None));
    }
    if w.exists(Switch::Stage3Fan) {
        out.push(entry(t("fanhigh.choice", "High fan"), Kind::FanHigh, fan(3), None, None));
    }
    out
}

/// Each page once, for the skins' check: the gate, the list with a stage
/// tested, a stage held back, running, its TROUBLESHOOT, DONE, the outside
/// too warm, the time limit, and the Furnace Heads-Up's offer, run,
/// shutdown and finish.
pub fn samples(equipment: &Equipment) -> Vec<(&'static str, SystemTest)> {
    // W1, Y1, Rc, Rh, G and a humidifier on STAR.
    let mut pins = [0; 15];
    for p in [0, 1, 2, 4, 5, 11] {
        pins[p] = 1;
    }
    let wiring = Wiring { pins, y2: -1, star: 1, dual_fuel: false };
    let mut e = equipment.clone();
    e.delivery[Switch::Stage1Heat as usize] = 0;
    let setup = |gate| Setup { wiring: Some(wiring.clone()), equipment: &e, heads_up: true, heat_link: false, gate };
    let now = Instant::now();
    let env = |held: Option<Duration>, powered: bool, at: Duration| Env { now: now + at, outdoor: Some(35.0), fahrenheit: false, held, powered };
    let calm = env(None, true, Duration::ZERO);
    let at = |entry: usize, held: Option<Duration>| {
        let mut s = SystemTest::open(&setup(None));
        s.wheel(entry as i32);
        s.press(&env(held, true, Duration::ZERO));
        s
    };
    let mut out = vec![("system test, no weather", SystemTest::open(&setup(Some("weather"))))];
    let mut list = at(3, None);
    list.press(&calm);
    list.tick(&calm);
    list.press(&calm);
    out.push(("system test, list", list));
    out.push(("system test, held back", at(3, Some(Duration::from_secs(150)))));
    let mut run = at(3, None);
    run.tick(&calm);
    out.push(("system test, running", run));
    let mut trouble = at(3, None);
    trouble.wheel(1);
    trouble.press(&calm);
    out.push(("system test, troubleshoot", trouble));
    let mut done = at(3, None);
    done.press(&calm);
    out.push(("system test, done", done));
    out.push(("system test, too warm", at(1, None)));
    let mut timeout = at(3, None);
    timeout.tick(&calm);
    timeout.tick(&env(None, true, LIMIT));
    out.push(("system test, timed out", timeout));
    out.push(("system test, heads-up", at(0, None)));
    let mut heads_up = at(0, None);
    heads_up.press(&calm);
    heads_up.tick(&calm);
    out.push(("system test, heads-up running", heads_up.clone()));
    heads_up.tick(&env(None, false, Duration::from_secs(400)));
    out.push(("system test, heads-up shutdown", heads_up));
    let mut finished = at(0, None);
    finished.press(&calm);
    finished.tick(&calm);
    finished.tick(&env(None, true, HEADS_UP));
    out.push(("system test, heads-up done", finished));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// nest-qemu's wiring: W1, Y1, Rc, Rh, G; and with STAR as `star`.
    fn wiring(star: i32) -> Wiring {
        let mut pins = [0; 15];
        for p in [0, 1, 2, 4, 5] {
            pins[p] = 1;
        }
        if star >= 0 {
            pins[11] = 1;
        }
        Wiring { pins, y2: -1, star, dual_fuel: false }
    }

    fn open(w: Wiring, e: &Equipment, heads_up: bool) -> SystemTest {
        SystemTest::open(&Setup { wiring: Some(w), equipment: e, heads_up, heat_link: false, gate: None })
    }

    fn env(now: Instant) -> Env {
        Env { now, outdoor: Some(20.0), fahrenheit: false, held: None, powered: true }
    }

    #[test]
    fn the_list_offers_what_the_wiring_has_in_stocks_order() {
        let e = Equipment::default();
        let s = open(wiring(-1), &e, false);
        assert_eq!(s.labels(), ["Heating", "Cooling", "Fan"]);
        assert_eq!(s.view().options.last().map(String::as_str), Some("DONE"));
        assert_eq!(s.entries[1].wires, "(G,Y1 wires)");
        assert_eq!(s.entries[2].wires, "(G wire)");
        // A humidifier on STAR, and the Heads-Up once flagged with a
        // forced-air furnace.
        let mut e = Equipment::default();
        e.delivery[Switch::Stage1Heat as usize] = 0;
        let s = open(wiring(1), &e, true);
        assert_eq!(s.labels(), ["Furnace Heads-Up", "Heating", "Cooling", "Fan", "Humidifier"]);
        assert_eq!(s.entries[4].wires, "(* wire)");
        // Radiators: their own page, and no Heads-Up without forced air.
        e.delivery[Switch::Stage1Heat as usize] = 2;
        let s = open(wiring(-1), &e, true);
        assert_eq!(s.labels()[0], "Heating");
        assert_eq!(s.entries[0].kind, Kind::Radiator);
    }

    #[test]
    fn a_stage_runs_until_done_and_is_ticked() {
        let e = Equipment::default();
        let mut s = open(wiring(-1), &e, false);
        let now = Instant::now();
        assert_eq!(s.request(), None);
        s.press(&env(now));
        assert_eq!(s.request().map(|t| t.operation), Some(Operation::Stage1Heat));
        s.tick(&env(now));
        let v = s.view();
        assert_eq!(v.heading, ["Testing Heat", "(W1 wire)"]);
        assert_eq!(v.options, ["DONE", "TROUBLESHOOT"]);
        s.press(&env(now));
        assert_eq!(s.request(), None, "DONE withdraws the request");
        assert_eq!(s.view().text, "Testing heat completed.");
        s.press(&env(now));
        let v = s.view();
        assert_eq!(v.ticked, [true, false, false, false]);
        assert_eq!(v.selected, 0, "back on the entry chosen");
        // The fan, at its speed.
        s.wheel(2);
        s.press(&env(now));
        assert_eq!(s.request(), Some(Test { operation: Operation::Off, humidity: Op::Off, fan: Some(1) }));
        s.close();
        assert_eq!(s.request(), None, "closing withdraws it");
    }

    #[test]
    fn a_lockout_holds_the_test_back_with_stocks_countdown() {
        let e = Equipment::default();
        let mut s = open(wiring(-1), &e, false);
        let now = Instant::now();
        s.wheel(1);
        let held = Env { held: Some(Duration::from_secs(130)), ..env(now) };
        s.press(&held);
        assert!(s.request().is_some(), "asked for while held");
        let v = s.view();
        assert_eq!(v.text, "To protect your system, your thermostat will start the test in:\n\n2:10");
        assert_eq!(v.options, ["BACK"]);
        s.tick(&held);
        assert!(!s.tick(&held), "the same second");
        assert!(s.tick(&Env { held: Some(Duration::from_secs(129)), ..held }), "a second down");
        // Released: the test runs, its limit from then.
        assert!(s.tick(&env(now + Duration::from_secs(130))));
        assert_eq!(s.view().heading[0], "Testing Cooling");
        assert!(!s.tick(&env(now + Duration::from_secs(130) + LIMIT - Duration::from_secs(1))));
        assert!(matches!(s.page, Page::Run(_)));
        // BACK while held gives up.
        let mut s = open(wiring(-1), &e, false);
        s.wheel(1);
        s.press(&held);
        s.press(&held);
        assert_eq!((s.request(), &s.page), (None, &Page::List));
    }

    #[test]
    fn a_test_stops_at_its_limit_and_can_be_run_again() {
        let e = Equipment::default();
        let mut s = open(wiring(-1), &e, false);
        let now = Instant::now();
        s.wheel(2);
        s.press(&env(now));
        s.tick(&env(now));
        assert!(s.tick(&env(now + LIMIT)));
        assert_eq!(s.request(), None);
        let v = s.view();
        assert_eq!(v.heading, ["Testing", "- Stopped -"]);
        assert_eq!(v.text, "After 35 minutes, this test was stopped and the fan has been turned off.");
        assert_eq!(v.options, ["SKIP TEST", "TEST AGAIN"]);
        s.wheel(1);
        s.press(&env(now + LIMIT));
        assert_eq!(s.request().and_then(|t| t.fan), Some(1), "TEST AGAIN");
    }

    #[test]
    fn an_ac_integrated_dehumidifier_runs_with_cooling_for_ten_minutes() {
        let mut e = Equipment::default();
        e.humidity.dehumidifier = 2;
        let mut s = open(wiring(2), &e, false);
        assert_eq!(s.labels(), ["Heating", "Cooling", "Fan", "Dehumidifier"]);
        let now = Instant::now();
        s.wheel(3);
        s.press(&env(now));
        assert_eq!(s.request(), Some(Test { operation: Operation::Stage1Cool, humidity: Op::AcIntegratedDehumidify, fan: None }));
        s.tick(&env(now));
        s.tick(&env(now + DEHUMIDIFIER_LIMIT));
        assert_eq!(s.view().text, "After 10 minutes, this test was stopped and the dehumidifier has been turned off.");
        // A stand-alone one runs alone, 35 minutes, whatever the outside.
        e.humidity.dehumidifier = 1;
        let mut s = open(wiring(2), &e, false);
        s.wheel(3);
        s.press(&Env { outdoor: Some(-10.0), ..env(now) });
        assert_eq!(s.request(), Some(Test { operation: Operation::Off, humidity: Op::Dehumidify, fan: None }));
        assert_eq!(s.entries[3].limit, LIMIT);
    }

    #[test]
    fn the_outside_rules_out_heating_above_29_and_cooling_below_16() {
        let e = Equipment::default();
        let now = Instant::now();
        let mut s = open(wiring(-1), &e, false);
        s.press(&Env { outdoor: Some(29.6), ..env(now) });
        assert_eq!(s.request(), None);
        assert_eq!(s.view().text, "It is currently 30° outside. To protect your system, your thermostat does not test heating when the outside temperature is above 29°.");
        s.press(&env(now));
        s.press(&Env { outdoor: Some(29.4), ..env(now) });
        assert!(s.request().is_some(), "29 is not above 29");
        let mut s = open(wiring(-1), &e, false);
        s.wheel(1);
        s.press(&Env { outdoor: Some(15.6), fahrenheit: true, ..env(now) });
        assert!(s.request().is_some(), "60 °F is not below 60");
        let mut s = open(wiring(-1), &e, false);
        s.wheel(1);
        s.press(&Env { outdoor: Some(15.0), fahrenheit: true, ..env(now) });
        assert_eq!(s.view().text, "It is currently 59° outside. To protect your system, your thermostat does not test cooling when the outside temperature is below 60°.");
        // No weather: not tested.
        let mut s = open(wiring(-1), &e, false);
        s.press(&Env { outdoor: None, ..env(now) });
        assert_eq!(s.request(), None);
    }

    #[test]
    fn troubleshooting_keeps_the_test_running() {
        let e = Equipment::default();
        let mut s = open(wiring(-1), &e, false);
        let now = Instant::now();
        s.wheel(1);
        s.press(&env(now));
        s.tick(&env(now));
        s.wheel(1);
        s.press(&env(now));
        let v = s.view();
        assert_eq!((v.text.as_str(), v.link.as_deref()), ("Find details about troubleshooting this test at", Some("nest.com/t02")));
        assert!(s.request().is_some(), "still cooling");
        // KEEP TESTING starts it again, its limit anew.
        let later = now + Duration::from_secs(600);
        s.press(&env(later));
        s.tick(&env(later));
        assert!(!s.tick(&env(now + LIMIT)));
        assert!(s.request().is_some());
        s.wheel(1);
        s.press(&env(later));
        s.wheel(1);
        s.press(&env(later));
        assert_eq!((s.request(), &s.page), (None, &Page::List), "DONE stops it");
    }

    #[test]
    fn the_furnace_heads_up_runs_fifteen_minutes_and_stops_at_a_power_loss() {
        let mut e = Equipment::default();
        e.delivery[Switch::Stage1Heat as usize] = 0;
        let now = Instant::now();
        let mut s = open(wiring(-1), &e, true);
        s.press(&env(now));
        assert_eq!(s.view().options, ["RUN TEST", "BACK"]);
        assert_eq!(s.request(), None);
        s.press(&env(now));
        assert_eq!(s.request().map(|t| t.operation), Some(Operation::Stage1Heat));
        s.tick(&env(now));
        assert_eq!(s.view().text, "Running Furnace Heads-Up test.\n\n15:00");
        s.tick(&Env { powered: false, ..env(now + Duration::from_secs(400)) });
        assert_eq!(s.request(), None);
        assert_eq!(s.view().text, "Your furnace ran for 6 minutes before shutting down. It may not be working properly. You might want to contact a Nest Pro.");
        assert_eq!(s.view().options, ["RUN TEST AGAIN", "DONE"]);
        s.press(&env(now));
        s.tick(&env(now));
        s.tick(&Env { powered: false, ..env(now + Duration::from_secs(70)) });
        assert!(s.view().text.starts_with("Your furnace ran for one minute"));
        // The whole run: no issue.
        s.press(&env(now));
        s.tick(&env(now));
        s.tick(&env(now + HEADS_UP));
        assert_eq!(s.view().text, "The test finished and no issues were found.");
        assert_eq!(s.request(), None);
    }

    #[test]
    fn the_gate_and_the_lists_done_close_the_test() {
        let e = Equipment::default();
        let mut s = SystemTest::open(&Setup { wiring: Some(wiring(-1)), equipment: &e, heads_up: false, heat_link: false, gate: Some("wifi") });
        assert!(s.view().text.starts_with("Your thermostat is not connected to Wi-Fi."));
        assert_eq!(s.press(&env(Instant::now())), Outcome::Close);
        let mut s = open(wiring(-1), &e, false);
        s.wheel(10);
        assert_eq!(s.press(&env(Instant::now())), Outcome::Close);
    }
}
