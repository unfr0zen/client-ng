//! Equipment Settings' pages for conventional wiring, as stock's layers
//! ask them: HEAT PUMP (heatpumplayer: the O/B wire, then the compressor's
//! and auxiliary heat's outdoor lockouts), DUAL FUEL (dualfuellayer: the
//! breakpoint, or primary or alternative heat without the weather),
//! HUMIDIFIER / DEHUMIDIFIER (humidityequipmentlayer, humiditylayer,
//! humidityquiettimelayer), the STAR wire's equipment (proviewselection-
//! layer's star wire question) and PRO SETUP's wires (pinconnectionslayer,
//! pinselectionlayer). Each page is drawn as the menus are; what a page
//! sets is given back as settings once it closes.

use crate::menu::{FormView, InfoPage, Row, View};
use crate::model::Value;

/// The settings the pages keep (the device bucket's fields in net.rs).
pub const OB: &str = "equipment.ob_orientation";
pub const COMPRESSOR: &str = "heatpump.compressor";
pub const AUX: &str = "heatpump.aux";
pub const BREAKPOINT: &str = "dualfuel.breakpoint";
pub const OVERRIDE: &str = "dualfuel.override";
pub const HUMIDITY: &str = "humidity.target";
pub const QUIET: &str = "humidity.quiet";
pub const QUIET_START: &str = "humidity.quiet.start";
pub const QUIET_END: &str = "humidity.quiet.end";
pub const STAR: &str = "equipment.star_type";
pub const Y2: &str = "equipment.y2_type";
pub const DUAL_FUEL: &str = "equipment.dual_fuel";
pub const DEHUMIDIFIER_TYPE: &str = "equipment.dehumidifier_type";

/// What a page's close gives back: the settings it changed.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    Done(Vec<(&'static str, Value)>),
}

/// A slider of temperatures or humidity: `step` from `min` to `max`, with
/// a named end below or above it (stock's "always", "Never use alt.").
#[derive(Clone, Debug, PartialEq)]
struct Slider {
    min: f32,
    max: f32,
    step: f32,
    below: Option<&'static str>,
    above: Option<&'static str>,
    /// The position: 0 is the end below when there is one.
    at: usize,
}

impl Slider {
    fn count(&self) -> usize {
        ((self.max - self.min) / self.step).round() as usize + 1
    }

    fn len(&self) -> usize {
        self.count() + usize::from(self.below.is_some()) + usize::from(self.above.is_some())
    }

    /// Placed on a value, or on an end (None: below when there is one).
    fn on(mut self, v: Option<f32>, above: bool) -> Slider {
        let first = usize::from(self.below.is_some());
        self.at = match v {
            Some(v) => first + ((v.clamp(self.min, self.max) - self.min) / self.step).round() as usize,
            None if above && self.above.is_some() => self.len() - 1,
            None => 0,
        };
        self
    }

    /// The value, None at an end.
    fn value(&self) -> Option<f32> {
        let first = usize::from(self.below.is_some());
        (self.at >= first && self.at < first + self.count()).then(|| self.min + (self.at - first) as f32 * self.step)
    }

    /// At the end below (false) or above (true).
    fn end(&self) -> Option<bool> {
        match self.value() {
            Some(_) => None,
            None => Some(self.at > 0 || self.below.is_none()),
        }
    }

    fn text(&self) -> Option<String> {
        match self.end()? {
            false => self.below.map(str::to_string),
            true => self.above.map(str::to_string),
        }
    }

    fn turn(&mut self, d: i32) {
        self.at = (self.at as i64 + i64::from(d)).clamp(0, self.len() as i64 - 1) as usize;
    }
}

/// A field of options (Pro Setup's): the value kept and what its box says.
#[derive(Clone, Debug, PartialEq)]
struct Pick {
    label: &'static str,
    options: Vec<(&'static str, &'static str)>,
    at: usize,
}

impl Pick {
    fn new(label: &'static str, options: &[(&'static str, &'static str)], value: Option<&str>) -> Pick {
        let at = options.iter().position(|(v, _)| Some(*v) == value).unwrap_or(0);
        Pick { label, options: options.to_vec(), at }
    }

    fn value(&self) -> &'static str {
        self.options[self.at].0
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Shape {
    /// Answers under a prompt, the one given ticked.
    Ask(Vec<&'static str>, Option<usize>),
    /// Entries of a list (row label and kind), its exit last.
    List(Vec<(String, &'static str)>),
    /// One slider, then the button.
    Slider(Slider, Option<&'static str>, &'static str),
    /// Fields of options, then CONTINUE.
    Picks(Vec<Pick>),
    /// Quiet time's hours, then QUIET TIME ON and QUIET TIME OFF.
    Hours([u32; 2], &'static str),
    /// A page of text and its button.
    Info(&'static str),
}

/// The page shown: its shape, title and prompt, the selected row, and
/// whether its field is being changed.
#[derive(Clone, Debug, PartialEq)]
struct Screen {
    shape: Shape,
    title: String,
    prompt: Option<String>,
    at: usize,
    changing: bool,
}

/// A press: on a row of a list or an answer, or on a form's button.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Press {
    Row(usize),
    Button(usize),
    None,
}

impl Screen {
    fn new(shape: Shape, title: &str, prompt: Option<String>) -> Screen {
        let at = match &shape {
            Shape::Ask(_, Some(t)) => *t,
            _ => 0,
        };
        Screen { shape, title: title.into(), prompt, at, changing: false }
    }

    /// Rows of the form's fields, and of everything selectable.
    fn fields(&self) -> usize {
        match &self.shape {
            Shape::Slider(..) => 1,
            Shape::Picks(p) => p.len(),
            Shape::Hours(..) => 2,
            _ => 0,
        }
    }

    fn rows(&self) -> usize {
        match &self.shape {
            Shape::Ask(a, _) => a.len(),
            Shape::List(l) => l.len(),
            Shape::Slider(..) | Shape::Picks(_) => self.fields() + 1,
            Shape::Hours(..) => 4,
            Shape::Info(_) => 1,
        }
    }

    fn wheel(&mut self, d: i32) {
        if self.changing {
            match &mut self.shape {
                Shape::Slider(s, ..) => s.turn(d),
                Shape::Picks(p) => {
                    let f = &mut p[self.at];
                    f.at = (f.at as i64 + i64::from(d)).rem_euclid(f.options.len() as i64) as usize;
                }
                Shape::Hours(h, _) => h[self.at] = (h[self.at] as i64 + i64::from(d)).rem_euclid(24) as u32,
                _ => {}
            }
        } else {
            self.at = (self.at as i64 + i64::from(d)).clamp(0, self.rows() as i64 - 1) as usize;
        }
    }

    fn press(&mut self) -> Press {
        let fields = self.fields();
        match self.shape {
            Shape::Ask(..) | Shape::List(_) => Press::Row(self.at),
            Shape::Info(_) => Press::Button(0),
            _ if self.at < fields => {
                // A press changes the field; the next one sets it and moves on.
                if self.changing {
                    self.at += 1;
                }
                self.changing = !self.changing;
                Press::None
            }
            _ => Press::Button(self.at - fields),
        }
    }

    /// The page in the language in use (stock's star glyph, U+E035, as
    /// ours, *).
    fn view(&self) -> View {
        let tr = |text: &str| crate::i18n::tr(text).replace('\u{e035}', "*");
        let prompt = self.prompt.as_deref().map(tr);
        let mut view = View {
            title: tr(&self.title), path: Vec::new(), depth: 2, selected: self.at, editing: false, pending: None, icon: None,
            style: None, prompt: prompt.clone(), detail: None, intro: false, list_at: None, image: None, image_size: None,
            rows: Vec::new(), info: None, form: None, turned: 0.0, scroll: None, travel: 0, page: 0,
        };
        let row = |label: &str, kind: &'static str, ticked: bool| Row {
            label: tr(label), icon: None, tile: None, style: None, prompt: None, fraction: None, kind, value: None, editing: false,
            options: Vec::new(), saved: None, ticked, summary: Vec::new(), detail: Vec::new(),
        };
        let form = |fields, texts, buttons, style: Option<&str>| FormView {
            prompt: prompt.clone(), icon: None, style: style.map(str::to_string), fields, texts, buttons, at: self.at,
            changing: self.changing,
        };
        match &self.shape {
            Shape::Ask(answers, ticked) => {
                view.style = Some("question".into());
                view.rows = answers.iter().enumerate().map(|(i, a)| row(a, "set", *ticked == Some(i))).collect();
            }
            Shape::List(rows) => {
                view.style = Some(if self.prompt.is_some() { "answers" } else { "titled" }.into());
                view.rows = rows.iter().map(|(label, kind)| row(label, kind, false)).collect();
            }
            Shape::Slider(s, label, button) => {
                let unit = (s.max > 40.0).then(|| "% RH".to_string());
                view.form = Some(form(vec![(tr(label.unwrap_or("")), s.value().map(f64::from), unit)], vec![s.text().as_deref().map(tr)],
                                      vec![*button], None));
            }
            Shape::Picks(picks) => {
                view.form = Some(form(picks.iter().map(|p| (tr(p.label), None, None)).collect(),
                                      picks.iter().map(|p| Some(tr(p.options[p.at].1))).collect(), vec!["continue"],
                                      Some("equipment")));
                view.form.as_mut().unwrap().prompt = Some(tr(&self.title));
            }
            Shape::Hours(h, what) => {
                let field = |label: &str, hour: u32| (label.to_string(), Some(f64::from(hour)), Some("hour".to_string()));
                // The equipment's name in the language, in the English
                // sentence, which is then found in the language.
                let what = tr(what);
                view.form = Some(form(vec![field(&tr(&format!("Do not run the\n{what} from")), h[0]),
                                           field(&tr("@humidityquiettimelayer.to|to"), h[1])], vec![None, None],
                                      vec!["quiet on", "quiet off"], Some("quiettime")));
            }
            Shape::Info(button) => {
                view.info = Some(InfoPage { text: prompt.unwrap_or_default(), link: None, button: tr(button),
                                            icon: None, scrolled: 0, style: None, wait: None, image: None, busy: false });
            }
        }
        view
    }
}

/// HEAT PUMP's pages (stock's states selectconfig, compressortemp,
/// confirmcompressortemp, auxtemp, balalancenotoff).
#[derive(Clone, Debug, PartialEq)]
enum HeatPumpStep {
    Ob,
    Compressor,
    Confirm,
    Aux,
    Balance,
}

const COMPRESSOR_ASKED: &str = "Use the heat pump\ncompressor when the\noutdoor temperature\nis above:";
const AUX_ASKED: &str = "Use the heat pump\nauxiliary heat when the\noutdoor temperature\nis below:";

/// The compressor's lockout from -31.5 °C to 0 (stock's -25 °F to 32 °F),
/// "always" below; aux heat's from -18 °C to 32 °C (0 °F to 90 °F),
/// "always" above (g2 0xc8710).
fn compressor_slider(v: Option<f32>) -> Slider {
    Slider { min: -31.5, max: 0.0, step: 0.5, below: Some("always"), above: None, at: 0 }.on(v, false)
}

fn aux_slider(v: Option<f32>) -> Slider {
    Slider { min: -18.0, max: 32.0, step: 0.5, below: None, above: Some("always"), at: 0 }.on(v, true)
}

/// DUAL FUEL's breakpoint: -12 °C to 10 °C (10 °F to 50 °F), "Never use
/// alt." below and "Always use alt." above (g2 0xae73c).
fn breakpoint_slider(v: f32, over: &str) -> Slider {
    let s = Slider { min: -12.0, max: 10.0, step: 0.5, below: Some("Never use alt."), above: Some("Always use alt."), at: 0 };
    match over {
        "ALWAYS-PRIMARY" => s.on(None, false),
        "ALWAYS-ALT" => s.on(None, true),
        _ => s.on(Some(v), false),
    }
}

/// A humidity target: a humidifier's 10 % to 60 %, off below; a
/// dehumidifier's 30 % to 80 %, off above (g2 0xceee0).
fn humidity_slider(humidifier: bool, v: Option<f32>) -> Slider {
    if humidifier {
        Slider { min: 10.0, max: 60.0, step: 5.0, below: Some("off"), above: None, at: 0 }.on(v, false)
    } else {
        Slider { min: 30.0, max: 80.0, step: 5.0, below: None, above: Some("off"), at: 0 }.on(v, true)
    }
}

fn compressor_screen(v: Option<f32>) -> Screen {
    Screen::new(Shape::Slider(compressor_slider(v), None, "done"), "", Some(COMPRESSOR_ASKED.into()))
}

fn aux_screen(v: Option<f32>) -> Screen {
    Screen::new(Shape::Slider(aux_slider(v), None, "done"), "", Some(AUX_ASKED.into()))
}

fn number(v: Option<f32>) -> Value {
    v.map_or(Value::Text("off".into()), |v| Value::Number(f64::from(v)))
}

/// What the wiring offers the STAR wire, as stock's star wire question
/// lists it (g2 0xfe8c8): the value kept (`star_type`), the answer, and
/// whether stock warns that Pro Setup is needed for it.
pub fn star_options(heat_pump: bool, dual_fuel: bool, w1: bool, w2: bool, g2: bool) -> Vec<(&'static str, &'static str, Option<&'static str>)> {
    let mut out = vec![("hum", "HUMIDIFIER", Some(HUM_WARNING)), ("dehum", "DEHUMIDIFIER", Some(DEHUM_WARNING))];
    if g2 {
        out.push(("g3", "G3: HIGH SPEED FAN", Some(FAN_WARNING)));
    }
    if !heat_pump {
        if w1 && w2 {
            out.push(("w3", "W3: STAGE-3 HEAT", None));
        }
        return out;
    }
    match (dual_fuel, w1, w2) {
        (false, false, false) => out.push(("w3", "AUXILIARY HEAT", None)),
        (true, false, false) => out.push(("w3", "ALTERNATE HEAT", Some(DUAL_FUEL_WARNING))),
        (true, true, false) | (true, false, true) => out.push(("w3", "ALT. HEAT STAGE 2", Some(DUAL_FUEL_WARNING))),
        _ => {}
    }
    out.push(("emer-heat", "E: EMERGENCY", None));
    out
}

const HUM_WARNING: &str = "You'll need to use Pro Setup to configure a humidifier. Incorrect settings will cause unexpected HVAC activity or equipment damage.";
const DEHUM_WARNING: &str = "You'll need to use Pro Setup to configure a dehumidifier. Incorrect settings will cause unexpected HVAC activity or equipment damage.";
const FAN_WARNING: &str = "You'll need to use Pro Setup to configure a multiple speed fan. Incorrect settings will cause unexpected HVAC activity or equipment damage.";
const DUAL_FUEL_WARNING: &str = "You'll need to use Pro Setup to configure a dual fuel system. Incorrect settings will cause unexpected HVAC activity or equipment damage.";

/// A wired terminal Pro Setup lists: its name and its fields (none for
/// power, common, fan and cool).
#[derive(Clone, Debug, PartialEq)]
pub struct Wire {
    pub name: &'static str,
    fields: Vec<Pick>,
}

/// Pro Setup's fields' options (pinselectionlayer's names).
const SOURCES: [(&str, &str); 5] = [("gas", "Gas"), ("electric", "Electric"), ("oil", "Oil"), ("lp", "Propane (LP)"), ("geothermal", "Geothermal")];
const DELIVERIES: [(&str, &str); 4] =
    [("forced-air", "Forced air"), ("radiator", "Radiators"), ("in-floor-radiant", "In-floor radiant"), ("electric-strip", "Electric baseboard")];
const ORIENTATIONS: [(&str, &str); 2] = [("O", "O"), ("B", "B")];
const AUX_TYPES: [(&str, &str); 2] = [("off", "Heat pump aux."), ("on", "Alt. heat (dual fuel)")];
const Y2_CONVENTIONAL: [(&str, &str); 2] = [("cooling", "Conv. cooling stage 2"), ("g2", "G2: fan")];
const Y2_HEAT_PUMP: [(&str, &str); 4] = [("heatingandcooling", "Heat pump st. 2 heat / cool"), ("heating", "Heat pump stage 2 heat"),
    ("cooling", "Heat pump stage 2 cool"), ("g2", "G2: fan")];
const STAR_TYPES: [(&str, &str); 6] = [("hum", "@pinselectionlayer.hum|Humidifier"), ("dehum", "Dehum with A/C"), ("dehum-standalone", "Dehum Standalone"),
    ("w3", "W3: heat stage 3"), ("g3", "G3: fan"), ("emer-heat", "E: emerg. heat")];

/// What the wiring and the settings give Pro Setup's list.
pub struct Wiring<'a> {
    /// The wired terminals by stock's names, in its order.
    pub wired: Vec<&'static str>,
    pub heat_pump: bool,
    pub get: &'a dyn Fn(&str) -> Option<String>,
}

/// Each wired terminal with the fields stock gives it.
pub fn wires(w: &Wiring) -> Vec<Wire> {
    let get = |k: &str| (w.get)(k);
    let star = get(STAR).unwrap_or_else(|| "w3".into());
    let star = if star == "dehum" && get(DEHUMIDIFIER_TYPE).as_deref() == Some("STAND-ALONE") { "dehum-standalone".to_string() } else { star };
    w.wired.iter().map(|&name| {
        let fields = match name {
            "W1" if w.heat_pump => vec![Pick::new("Type", &AUX_TYPES, get(DUAL_FUEL).as_deref())],
            "W1" => vec![Pick::new("Source", &SOURCES, get(crate::engine::HEAT_SOURCE).as_deref()),
                         Pick::new("Delivery", &DELIVERIES, get(crate::engine::HEAT_DELIVERY).as_deref())],
            "Y2" if w.heat_pump => vec![Pick::new("Type", &Y2_HEAT_PUMP, get(Y2).as_deref())],
            "Y2" => vec![Pick::new("Type", &Y2_CONVENTIONAL, get(Y2).as_deref())],
            "O/B" => vec![Pick::new("Orientation", &ORIENTATIONS, get(OB).as_deref())],
            "STAR" => vec![Pick::new("Type", &STAR_TYPES, Some(&star))],
            _ => Vec::new(),
        };
        Wire { name, fields }
    }).collect()
}

/// What each wire does as its fields have it (pinconnectionslayer's
/// names): a heat pump's with O/B wired, W1 and W2 alternate heat with
/// dual fuel.
fn functions(wires: &[Wire]) -> Vec<&'static str> {
    let pick = |name: &str| wires.iter().find(|w| w.name == name).and_then(|w| w.fields.first()).map(Pick::value);
    let heat_pump = wires.iter().any(|w| w.name == "O/B");
    let dual = pick("W1") == Some("on");
    wires.iter().map(|w| match (w.name, heat_pump) {
        ("W1", true) => if dual { "Alt. heat" } else { "Aux. heat" },
        ("W1", false) => "Heat",
        ("W2", true) => if dual { "Alt. stage 2" } else { "Aux. heat" },
        ("W2", false) => "Heat 2",
        ("Y1", true) => "Heat / cool",
        ("Y1", false) => "Cool",
        ("Y2", true) => "Heat / cool 2",
        ("Y2", false) => "Cool 2",
        ("G", _) => "Fan",
        ("O/B", _) => "Heat pump",
        ("STAR", _) => match pick("STAR") {
            Some("hum") => "@pinconnectionslayer.types.12|Humidifier",
            Some("dehum" | "dehum-standalone") => "@pinconnectionslayer.types.13|Dehumidifier",
            Some("g3") => "G3",
            Some("emer-heat") => "Emergency heat",
            _ if heat_pump => "Aux. heat",
            _ => "Heat 3",
        },
        ("C", _) => "24V common",
        _ => "Power",
    }).collect()
}

/// The settings a wire's fields keep.
fn wire_changes(wire: &Wire) -> Vec<(&'static str, Value)> {
    let text = |v: &str| Value::Text(v.to_string());
    wire.fields.iter().flat_map(|f| match (wire.name, f.label) {
        (_, "Source") => vec![(crate::engine::HEAT_SOURCE, text(f.value()))],
        (_, "Delivery") => vec![(crate::engine::HEAT_DELIVERY, text(f.value()))],
        (_, "Orientation") => vec![(OB, text(f.value()))],
        ("W1", _) => vec![(DUAL_FUEL, text(f.value()))],
        ("Y2", _) => vec![(Y2, text(f.value()))],
        (_, _) => match f.value() {
            "dehum" => vec![(STAR, text("dehum")), (DEHUMIDIFIER_TYPE, text("AC-INTEGRATED"))],
            "dehum-standalone" => vec![(STAR, text("dehum")), (DEHUMIDIFIER_TYPE, text("STAND-ALONE"))],
            v => vec![(STAR, text(v))],
        },
    }).collect()
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    HeatPump { step: HeatPumpStep, balance_off: bool, compressor: Option<f32>, aux: Option<f32> },
    DualFuel,
    Humidity { humidifier: bool, target: Option<f32>, quiet: Option<[u32; 2]>, hours: [u32; 2] },
    Star { options: Vec<(&'static str, &'static str, Option<&'static str>)> },
    Wires { wires: Vec<Wire>, open: Option<usize> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    kind: Kind,
    screen: Screen,
    changes: Vec<(&'static str, Value)>,
}

impl Page {
    /// HEAT PUMP, on the O/B wire's question; the lockouts follow with
    /// Heat Pump Balance off, its note with it on.
    pub fn heat_pump(ob: Option<&str>, compressor: Option<f32>, aux: Option<f32>, balance_off: bool) -> Page {
        let ticked = match ob { Some("B") => Some(1), Some(_) => Some(0), None => None };
        let screen = Screen::new(Shape::Ask(vec!["O", "B", "I DON'T KNOW"], ticked), "",
                                 Some("Does your heat pump use\nan O wire (common) or\nB wire (less common)?".into()));
        Page { kind: Kind::HeatPump { step: HeatPumpStep::Ob, balance_off, compressor, aux }, screen, changes: Vec::new() }
    }

    /// DUAL FUEL: the breakpoint with the weather, else the choice of
    /// primary or alternative heat.
    pub fn dual_fuel(breakpoint: f32, over: &str, online: bool) -> Page {
        let screen = if online {
            Screen::new(Shape::Slider(breakpoint_slider(breakpoint, over), None, "done"), "",
                        Some("Use alternate heat\nwhen the outside\ntemperature is below:".into()))
        } else {
            let ticked = match over { "ALWAYS-ALT" => Some(1), "ALWAYS-PRIMARY" => Some(0), _ => None };
            Screen::new(Shape::Ask(vec!["PRIMARY", "ALTERNATIVE"], ticked), "", Some("Do you want to switch\nto alternative heat?".into()))
        };
        Page { kind: Kind::DualFuel, screen, changes: Vec::new() }
    }

    /// HUMIDIFIER or DEHUMIDIFIER: the target and its choices.
    pub fn humidity(humidifier: bool, target: Option<f32>, quiet: Option<[u32; 2]>, hours: [u32; 2]) -> Page {
        let mut page = Page { kind: Kind::Humidity { humidifier, target, quiet, hours }, screen: Screen::new(Shape::Info(""), "", None),
                              changes: Vec::new() };
        page.humidity_list(0);
        page
    }

    /// The STAR wire's equipment.
    pub fn star(options: Vec<(&'static str, &'static str, Option<&'static str>)>, star: Option<&str>) -> Page {
        let ticked = options.iter().position(|o| Some(o.0) == star);
        let answers = options.iter().map(|o| o.1).collect();
        let screen = Screen::new(Shape::Ask(answers, ticked), "", Some("@proviewselectionlayer.starwireselectmsg.aw|Choose the equipment\nwired to the *\nconnector.".into()));
        Page { kind: Kind::Star { options }, screen, changes: Vec::new() }
    }

    /// PRO SETUP's wires, each opening its fields.
    pub fn wires(wires: Vec<Wire>) -> Page {
        let mut page = Page { kind: Kind::Wires { wires, open: None }, screen: Screen::new(Shape::Info(""), "", None), changes: Vec::new() };
        page.wire_list(0);
        page
    }

    fn wire_list(&mut self, at: usize) {
        let Kind::Wires { wires, .. } = &self.kind else { return };
        let mut rows: Vec<(String, &'static str)> = wires.iter().zip(functions(wires)).map(|(w, f)| (format!("{}: {}", w.name, crate::i18n::tr(f)), "menu")).collect();
        rows.push(("Done".into(), "back"));
        self.screen = Screen::new(Shape::List(rows), "Pro Setup", None);
        self.screen.at = at;
    }

    fn humidity_list(&mut self, at: usize) {
        let Kind::Humidity { humidifier, target, .. } = &self.kind else { return };
        let prompt = match (humidifier, target) {
            (true, Some(t)) => format!("Humidify\n{t:.0}% RH"),
            (false, Some(t)) => format!("Dehumidify\n{t:.0}% RH"),
            (true, None) => "@humidityequipmentlayer.humisoff.aw|Humidifier\noff".into(),
            (false, None) => "Dehumidifier\noff".into(),
        };
        let mut rows = vec![("SET HUMIDITY".to_string(), "action")];
        if target.is_some() {
            rows.push((if *humidifier { "HUMIDIFIER OFF" } else { "DEHUM. OFF" }.into(), "action"));
            rows.push(("QUIET TIME".into(), "action"));
        }
        rows.push(("Back".into(), "back"));
        let n = rows.len();
        self.screen = Screen::new(Shape::List(rows), "", Some(prompt));
        self.screen.at = at.min(n - 1);
    }

    pub fn view(&self) -> View {
        self.screen.view()
    }

    /// A press now opens the humidity target (SET HUMIDITY selected).
    pub fn sets_humidity(&self) -> bool {
        matches!(&self.screen.shape, Shape::List(rows) if rows.get(self.screen.at).is_some_and(|r| r.0 == "SET HUMIDITY"))
    }

    pub fn wheel(&mut self, d: i32) {
        self.screen.wheel(d);
    }

    pub fn press(&mut self) -> Outcome {
        let press = self.screen.press();
        if press == Press::None {
            return Outcome::Stay;
        }
        match &mut self.kind {
            Kind::HeatPump { step, balance_off, compressor, aux } => {
                match (step.clone(), press, &self.screen.shape) {
                    (HeatPumpStep::Ob, Press::Row(i), _) => {
                        // I DON'T KNOW keeps stock's default, O.
                        self.changes.push((OB, Value::Text(if i == 1 { "B" } else { "O" }.into())));
                        if *balance_off {
                            *step = HeatPumpStep::Compressor;
                            self.screen = compressor_screen(*compressor);
                        } else {
                            *step = HeatPumpStep::Balance;
                            self.screen = Screen::new(Shape::Info("Done"), "", Some("Nest Sense Heat Pump\nBalance is on. Your\nthermostat will use\nauxiliary heat as needed.".into()));
                        }
                    }
                    (HeatPumpStep::Compressor, Press::Button(_), Shape::Slider(s, ..)) => {
                        *compressor = s.value();
                        // A minimum temperature is confirmed first.
                        if compressor.is_some() {
                            *step = HeatPumpStep::Confirm;
                            self.screen = Screen::new(Shape::Ask(vec!["CONFIRM", "BACK"], None), "",
                                                      Some("Compressor min.\ntemperature should only\nbe used if AUX controls\nan auxiliary heater.".into()));
                        } else {
                            *step = HeatPumpStep::Aux;
                            self.screen = aux_screen(*aux);
                        }
                    }
                    (HeatPumpStep::Confirm, Press::Row(0), _) => {
                        *step = HeatPumpStep::Aux;
                        self.screen = aux_screen(*aux);
                    }
                    (HeatPumpStep::Confirm, Press::Row(_), _) => {
                        *step = HeatPumpStep::Compressor;
                        self.screen = compressor_screen(*compressor);
                    }
                    (HeatPumpStep::Aux, Press::Button(_), Shape::Slider(s, ..)) => {
                        let v = s.value();
                        self.changes.push((COMPRESSOR, number(*compressor)));
                        self.changes.push((AUX, number(v)));
                        return self.done();
                    }
                    // With Balance on only the orientation is kept.
                    (HeatPumpStep::Balance, _, _) => return self.done(),
                    _ => {}
                }
                Outcome::Stay
            }
            Kind::DualFuel => {
                match (&self.screen.shape, press) {
                    (Shape::Slider(s, ..), Press::Button(_)) => {
                        let over = match s.end() { Some(false) => "ALWAYS-PRIMARY", Some(true) => "ALWAYS-ALT", None => "NONE" };
                        if let Some(v) = s.value() {
                            self.changes.push((BREAKPOINT, Value::Number(f64::from(v))));
                        }
                        self.changes.push((OVERRIDE, Value::Text(over.into())));
                    }
                    (Shape::Ask(..), Press::Row(i)) => {
                        self.changes.push((OVERRIDE, Value::Text(if i == 0 { "ALWAYS-PRIMARY" } else { "ALWAYS-ALT" }.into())));
                    }
                    _ => return Outcome::Stay,
                }
                self.done()
            }
            Kind::Humidity { humidifier, target, quiet, hours } => {
                let what = if *humidifier { "@humidityquiettimelayer.humififier|humidifier" } else { "@humidityquiettimelayer.dehumififier|dehumidifier" };
                match (&self.screen.shape, press) {
                    (Shape::List(rows), Press::Row(i)) => match rows[i].0.as_str() {
                        "SET HUMIDITY" => {
                            let prompt = if *humidifier { "Turn to set the minimum\nhumidity your thermostat\nshould maintain." }
                                         else { "Turn to set the maximum\nhumidity your thermostat\nshould maintain." };
                            // A first target is stock's 35 % to humidify, 55 % to dehumidify.
                            let start = target.or(Some(if *humidifier { 35.0 } else { 55.0 }));
                            self.screen = Screen::new(Shape::Slider(humidity_slider(*humidifier, start), None, "done"), "", Some(prompt.into()));
                        }
                        "QUIET TIME" => {
                            self.screen = Screen::new(Shape::Hours(quiet.unwrap_or(*hours), what), "", None);
                        }
                        "Back" => return self.done(),
                        _ => {
                            *target = None;
                            self.changes.push((HUMIDITY, number(None)));
                            self.humidity_list(0);
                        }
                    },
                    (Shape::Slider(s, ..), Press::Button(_)) => {
                        *target = s.value();
                        self.changes.push((HUMIDITY, number(*target)));
                        self.humidity_list(0);
                    }
                    (Shape::Hours(h, _), Press::Button(b)) => {
                        let h = *h;
                        *hours = h;
                        *quiet = (b == 0).then_some(h);
                        self.changes.push((QUIET, Value::Text(if b == 0 { "on" } else { "off" }.into())));
                        if b == 0 {
                            self.changes.push((QUIET_START, Value::Number(f64::from(h[0]))));
                            self.changes.push((QUIET_END, Value::Number(f64::from(h[1]))));
                        }
                        let at = if target.is_some() { 2 } else { 0 };
                        self.humidity_list(at);
                    }
                    _ => {}
                }
                Outcome::Stay
            }
            Kind::Star { options } => match (&self.screen.shape, press) {
                (Shape::Ask(..), Press::Row(i)) => {
                    let (value, _, warning) = options[i];
                    self.changes.push((STAR, Value::Text(value.into())));
                    match warning {
                        // Stock warns that its equipment needs Pro Setup.
                        Some(text) => {
                            self.screen = Screen::new(Shape::Info("OK"), "", Some(text.into()));
                            Outcome::Stay
                        }
                        None => self.done(),
                    }
                }
                (Shape::Info(_), _) => self.done(),
                _ => Outcome::Stay,
            },
            Kind::Wires { wires, open } => match (open.take(), press, &self.screen.shape) {
                (None, Press::Row(i), _) if i == wires.len() => self.done(),
                (None, Press::Row(i), _) if wires[i].fields.is_empty() => Outcome::Stay,
                (None, Press::Row(i), _) => {
                    *open = Some(i);
                    let wire = &wires[i];
                    self.screen = Screen::new(Shape::Picks(wire.fields.clone()), wire.name, None);
                    Outcome::Stay
                }
                (Some(i), Press::Button(_), Shape::Picks(picks)) => {
                    wires[i].fields = picks.clone();
                    self.changes.extend(wire_changes(&wires[i]));
                    self.wire_list(i);
                    Outcome::Stay
                }
                (o, _, _) => {
                    *open = o;
                    Outcome::Stay
                }
            },
        }
    }

    fn done(&mut self) -> Outcome {
        Outcome::Done(std::mem::take(&mut self.changes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn last(changes: &[(&'static str, Value)], key: &str) -> Option<Value> {
        changes.iter().rev().find(|c| c.0 == key).map(|c| c.1.clone())
    }

    #[test]
    fn heat_pump_asks_o_or_b_then_both_lockouts_with_balance_off() {
        let mut p = Page::heat_pump(Some("O"), None, None, true);
        p.wheel(1);
        assert_eq!(p.press(), Outcome::Stay, "B");
        let f = p.view().form.unwrap();
        assert_eq!((f.fields[0].1, f.texts[0].as_deref()), (None, Some("always")), "no lockout: always");
        p.press();
        p.wheel(10);
        p.press();
        p.press();
        assert_eq!(p.view().rows[0].label, "CONFIRM", "a minimum temperature is confirmed");
        p.press();
        let f = p.view().form.unwrap();
        assert_eq!(f.texts[0].as_deref(), Some("always"));
        p.press();
        p.wheel(-4);
        p.press();
        let Outcome::Done(c) = p.press() else { panic!("done") };
        assert_eq!(last(&c, OB), Some(Value::Text("B".into())));
        assert_eq!(last(&c, COMPRESSOR), Some(Value::Number(-27.0)));
        assert_eq!(last(&c, AUX), Some(Value::Number(30.5)));
    }

    #[test]
    fn heat_pump_with_balance_on_keeps_only_the_orientation() {
        let mut p = Page::heat_pump(None, Some(-5.0), None, false);
        p.wheel(2);
        p.press();
        assert!(p.view().info.unwrap().text.starts_with("Nest Sense Heat Pump"));
        assert_eq!(p.press(), Outcome::Done(vec![(OB, Value::Text("O".into()))]), "I DON'T KNOW is O");
    }

    #[test]
    fn dual_fuel_ends_are_overrides() {
        let mut p = Page::dual_fuel(-1.0, "NONE", true);
        p.press();
        p.wheel(-100);
        p.press();
        assert_eq!(p.view().form.unwrap().texts[0].as_deref(), Some("Never use alt."));
        assert_eq!(p.press(), Outcome::Done(vec![(OVERRIDE, Value::Text("ALWAYS-PRIMARY".into()))]));
        let mut p = Page::dual_fuel(-1.0, "NONE", false);
        p.wheel(1);
        assert_eq!(p.press(), Outcome::Done(vec![(OVERRIDE, Value::Text("ALWAYS-ALT".into()))]));
    }

    #[test]
    fn humidity_sets_a_target_and_quiet_time() {
        let mut p = Page::humidity(true, None, None, [22, 6]);
        assert_eq!(p.view().prompt.as_deref(), Some("Humidifier\noff"));
        assert_eq!(p.view().rows.len(), 2, "SET HUMIDITY, Back");
        p.press();
        assert_eq!(p.view().form.unwrap().fields[0].1, Some(35.0));
        p.press();
        p.wheel(1);
        p.press();
        p.press();
        assert_eq!(p.view().prompt.as_deref(), Some("Humidify\n40% RH"));
        p.wheel(2);
        p.press();
        let f = p.view().form.unwrap();
        assert_eq!((f.fields[0].1, f.fields[1].1), (Some(22.0), Some(6.0)));
        p.wheel(2);
        p.press();
        p.wheel(1);
        let Outcome::Done(c) = p.press() else { panic!("Back leaves") };
        assert_eq!(last(&c, HUMIDITY), Some(Value::Number(40.0)));
        assert_eq!(last(&c, QUIET), Some(Value::Text("on".into())));
        assert_eq!(last(&c, QUIET_START), Some(Value::Number(22.0)));
    }

    #[test]
    fn star_options_follow_the_wiring() {
        let names = |o: Vec<(&'static str, &'static str, Option<&'static str>)>| o.into_iter().map(|o| o.1).collect::<Vec<_>>();
        assert_eq!(names(star_options(false, false, true, true, false)), ["HUMIDIFIER", "DEHUMIDIFIER", "W3: STAGE-3 HEAT"]);
        assert_eq!(names(star_options(true, false, false, false, false)), ["HUMIDIFIER", "DEHUMIDIFIER", "AUXILIARY HEAT", "E: EMERGENCY"]);
        let mut p = Page::star(star_options(true, false, false, false, false), Some("emer-heat"));
        assert_eq!(p.view().selected, 3);
        p.wheel(-3);
        p.press();
        assert!(p.view().info.unwrap().text.contains("humidifier"));
        assert_eq!(p.press(), Outcome::Done(vec![(STAR, Value::Text("hum".into()))]));
    }

    #[test]
    fn pro_setup_lists_the_wires_and_keeps_their_fields() {
        let get = |k: &str| (k == crate::engine::HEAT_SOURCE).then(|| "oil".to_string());
        let w = Wiring { wired: vec!["W1", "Y1", "G", "O/B", "Rc"], heat_pump: true, get: &get };
        let mut p = Page::wires(wires(&w));
        assert_eq!(p.view().rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(),
                   ["W1: Aux. heat", "Y1: Heat / cool", "G: Fan", "O/B: Heat pump", "Rc: Power", "Done"]);
        p.wheel(3);
        p.press();
        assert_eq!(p.view().form.unwrap().texts, vec![Some("O".to_string())]);
        p.press();
        p.wheel(1);
        p.press();
        p.press();
        assert_eq!(p.view().selected, 3, "back on its wire");
        p.wheel(9);
        assert_eq!(p.press(), Outcome::Done(vec![(OB, Value::Text("B".into()))]));
    }
}
