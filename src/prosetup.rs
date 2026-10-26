//! Stock's PRO SETUP on a Heat Link unit (proview's boiler type and pin
//! selection layers): what heats the home, then its Heating form (control,
//! source, delivery as the type takes them), a Hot water form for system
//! and other boilers, and Your system. Each page is a menu view, drawn as
//! the menus are. Matched on nest-qemu; what it keeps are the device
//! bucket's boiler_type, heat_link_heat_type, heater_source,
//! heater_delivery and heat_link_hot_water_type.

use crate::menu::{FormView, Row, View};

/// The heating types: the value kept, the list's name, Your system's name,
/// and whether the type has a control and a source, and hot water.
const TYPES: [(&str, &str, &str, bool, bool); 6] = [
    ("combi", "Combi boiler", "Combi boiler", true, false),
    ("system", "System boiler", "System boiler", true, true),
    ("heatpump", "Heat pump", "Heat pump", false, false),
    ("district", "District heating", "District heating", false, false),
    ("electric", "Electric heating", "Electric heating", false, false),
    ("other", "Other", "Other boiler", true, true),
];

/// A field's options: the value kept and what its box says, in the ring's
/// order (stock's).
pub const CONTROLS: [(&str, &str); 2] = [("onoff", "On/Off"), ("opentherm", "OpenTherm")];
pub const SOURCES: [(&str, &str); 6] =
    [("electric", "Electric"), ("oil", "Oil"), ("propane", "Propane (LP)"), ("geothermal", "Geothermal"), ("pellets", "Pellets"), ("gas", "Gas")];
pub const DELIVERIES: [(&str, &str); 2] = [("radiator", "Radiators"), ("in-floor-radiant", "In-floor radiant")];
pub const HOT_WATER: [(&str, &str); 2] = [("notconnected", "Not connected"), ("onoff", "On/Off")];

/// What Pro Setup sets.
#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    pub boiler: String,
    pub control: String,
    pub source: String,
    pub delivery: String,
    pub hot_water: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Page {
    Type,
    Heating,
    HotWater,
    System,
}

/// A form's field: its label, options and which is set.
struct Field {
    label: &'static str,
    options: &'static [(&'static str, &'static str)],
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    /// Your system's CONTINUE: kept, back to Equipment Settings.
    Done(Setup),
    /// Your system's TEST.
    Test(Setup),
}

#[derive(Clone, Debug)]
pub struct ProSetup {
    page: Page,
    setup: Setup,
    /// The selected row: a field, then CONTINUE (or the type, on its list).
    at: usize,
    changing: bool,
    /// What Your system says of the rest (cooling, fan).
    rest: Vec<String>,
}

fn index(options: &[(&str, &str)], value: &str) -> usize {
    options.iter().position(|(v, _)| *v == value).unwrap_or(0)
}

/// Your system's first lines for what Pro Setup kept: the type's name and
/// how it heats (stock's after Pro Setup).
pub fn system_lines(boiler: &str, control: &str) -> [String; 2] {
    let name = TYPES.iter().find(|t| t.0 == boiler).map_or("Other boiler", |t| t.2);
    [name.to_string(), format!("Heat with {}", if control == "opentherm" { "OpenTherm" } else { "On/Off" })]
}

impl ProSetup {
    pub fn open(setup: Setup, rest: Vec<String>) -> ProSetup {
        let at = TYPES.iter().position(|t| t.0 == setup.boiler).unwrap_or(0);
        ProSetup { page: Page::Type, setup, at, changing: false, rest }
    }

    fn kind(&self) -> &'static (&'static str, &'static str, &'static str, bool, bool) {
        TYPES.iter().find(|t| t.0 == self.setup.boiler).unwrap_or(&TYPES[0])
    }

    fn fields(&self) -> Vec<Field> {
        match self.page {
            Page::Heating if self.kind().3 => vec![
                Field { label: "Control", options: &CONTROLS },
                Field { label: "Source", options: &SOURCES },
                Field { label: "Delivery", options: &DELIVERIES },
            ],
            Page::Heating => vec![Field { label: "Delivery", options: &DELIVERIES }],
            Page::HotWater => vec![Field { label: "Control", options: &HOT_WATER }],
            _ => Vec::new(),
        }
    }

    fn value_mut(&mut self, label: &str) -> &mut String {
        match (self.page, label) {
            (Page::HotWater, _) => &mut self.setup.hot_water,
            (_, "Control") => &mut self.setup.control,
            (_, "Source") => &mut self.setup.source,
            _ => &mut self.setup.delivery,
        }
    }

    pub fn wheel(&mut self, steps: i32) {
        match self.page {
            Page::Type => self.at = (self.at as i64 + steps as i64).clamp(0, TYPES.len() as i64 - 1) as usize,
            Page::System => self.at = (self.at as i64 + steps as i64).clamp(0, 1) as usize,
            _ => {
                let fields = self.fields();
                if self.changing {
                    // Round the field's options, as stock's boxes go.
                    let field = &fields[self.at];
                    let n = field.options.len() as i64;
                    let options = field.options;
                    let label = field.label;
                    let value = self.value_mut(label);
                    let to = (index(options, value) as i64 + steps as i64).rem_euclid(n) as usize;
                    *value = options[to].0.to_string();
                } else {
                    self.at = (self.at as i64 + steps as i64).clamp(0, fields.len() as i64) as usize;
                }
            }
        }
    }

    pub fn press(&mut self) -> Outcome {
        match self.page {
            Page::Type => {
                self.setup.boiler = TYPES[self.at].0.to_string();
                self.page = Page::Heating;
                self.at = 0;
            }
            Page::System => {
                return if self.at == 0 { Outcome::Done(self.setup.clone()) } else { Outcome::Test(self.setup.clone()) };
            }
            _ => {
                let fields = self.fields().len();
                if self.at < fields {
                    // A press changes the field; the next one moves on.
                    if self.changing {
                        self.at += 1;
                    }
                    self.changing = !self.changing;
                } else if self.page == Page::Heating && self.kind().4 {
                    self.page = Page::HotWater;
                    self.at = 0;
                } else {
                    self.page = Page::System;
                    self.at = 0;
                }
            }
        }
        Outcome::Stay
    }

    /// The page as the menus draw it.
    pub fn view(&self) -> View {
        let mut view = View {
            title: String::new(), path: Vec::new(), depth: 2, selected: self.at, editing: false, pending: None, icon: None,
            style: None, prompt: None, detail: None, intro: false, list_at: None, image: None, image_size: None, rows: Vec::new(), info: None, form: None, turned: 0.0, scroll: None, travel: 0, page: 0,
        };
        let row = |label: &str, kind: &'static str, ticked: bool| Row {
            label: label.into(), icon: None, tile: None, style: None, prompt: None, fraction: None, kind, value: None, editing: false,
            options: Vec::new(), saved: None, ticked, summary: Vec::new(), detail: Vec::new(),
        };
        match self.page {
            Page::Type => {
                // Stock's list ticks none.
                view.style = Some("boiler".into());
                view.prompt = Some("What type of heating\nsystem do you have?".into());
                view.rows = TYPES.iter().map(|t| row(t.1, "set", false)).collect();
            }
            Page::System => {
                view.style = Some("system".into());
                view.title = "Your system".into();
                view.prompt = Some(system_lines(&self.setup.boiler, &self.setup.control).into_iter().chain(self.rest.iter().cloned())
                    .collect::<Vec<_>>().join("\n"));
                view.rows = vec![row("Continue", "action", false), row("Test", "action", false)];
            }
            _ => {
                let fields = self.fields();
                view.title = if self.page == Page::HotWater { "Hot water" } else { "Heating" }.into();
                let texts = fields.iter().map(|f| {
                    let value = match (self.page, f.label) {
                        (Page::HotWater, _) => &self.setup.hot_water,
                        (_, "Control") => &self.setup.control,
                        (_, "Source") => &self.setup.source,
                        _ => &self.setup.delivery,
                    };
                    Some(f.options[index(f.options, value)].1.to_string())
                }).collect();
                view.form = Some(FormView {
                    prompt: Some(view.title.clone()), icon: None, style: Some("equipment".into()),
                    fields: fields.iter().map(|f| (f.label.to_string(), None, None)).collect(),
                    texts, buttons: vec!["continue"], at: self.at, changing: self.changing,
                });
            }
        }
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Setup {
        Setup { boiler: "other".into(), control: "onoff".into(), source: "gas".into(), delivery: "radiator".into(), hot_water: "notconnected".into() }
    }

    #[test]
    fn a_system_boiler_asks_heating_then_hot_water_then_shows_the_system() {
        let mut p = ProSetup::open(setup(), vec!["Cooling".into(), "Fan".into()]);
        assert_eq!(p.view().selected, 5, "opens on the type given");
        p.wheel(-4);
        p.press();
        assert_eq!(p.view().form.map(|f| f.texts), Some(vec![Some("On/Off".into()), Some("Gas".into()), Some("Radiators".into())]));
        // Control to OpenTherm, Source on to Electric (round the ring).
        p.press();
        p.wheel(1);
        p.press();
        p.press();
        p.wheel(1);
        p.press();
        p.wheel(1);
        p.press();
        assert_eq!(p.view().title, "Hot water");
        p.press();
        p.wheel(-1);
        p.press();
        p.wheel(1);
        p.press();
        let view = p.view();
        assert_eq!((view.title.as_str(), view.prompt.as_deref()), ("Your system", Some("System boiler\nHeat with OpenTherm\nCooling\nFan")));
        assert_eq!(p.press(), Outcome::Done(Setup { boiler: "system".into(), control: "opentherm".into(), source: "electric".into(),
                                                    delivery: "radiator".into(), hot_water: "onoff".into() }));
    }

    #[test]
    fn a_heat_pump_asks_only_its_delivery() {
        let mut p = ProSetup::open(Setup { boiler: "heatpump".into(), ..setup() }, Vec::new());
        p.press();
        assert_eq!(p.view().form.map(|f| f.fields.len()), Some(1));
        p.wheel(1);
        p.press();
        assert_eq!(p.view().title, "Your system", "no hot water");
    }
}
