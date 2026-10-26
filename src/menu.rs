//! Settings menus: a tree read from a menus file, and the navigation through
//! it. Skins only draw the view; every skin moves through menus the same way.
//!
//! The wheel moves the selection, or changes the value being edited; the
//! button opens a submenu, starts or commits an edit, flips a toggle, runs
//! an action, or goes back. Editing a choice offers Back after its options,
//! which leaves the value as it was; an edit opened from the top menu
//! closes the menus when it ends. Every menu has a Back entry: where the menus
//! file puts one, or else at its end; Done entries, which go straight
//! home, stand in for it. The top menu opens on Back, so a
//! second press closes it.

use crate::model::{Settings, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Menu(Vec<Item>),
    Choice { key: String, options: Vec<Choice> },
    Toggle { key: String },
    Range { key: String, min: f64, max: f64, step: f64, unit: String },
    Action { name: String },
    /// A line of text over a group of entries, never selected.
    Heading,
    /// Fields edited in place one at a time, then Done and, with `more`,
    /// More info (its page, with `link` under the text), More info first
    /// when `more_first`. With `enable`, a setting turned on and off by the
    /// form instead: Set and Back while it is off (Back puts the fields
    /// back as they were), Done and Disable while it is on.
    /// `action`, when given, is run by Done, which then closes the menus.
    Form { fields: Vec<Field>, more: Option<String>, link: Option<String>, more_first: bool, enable: Option<String>,
           action: Option<String> },
    /// Sets `key` to `value` and stays, ticked while it is the setting;
    /// `confirm` is a page shown after.
    Set { key: String, value: String, confirm: Option<String> },
    /// A page of text, a link under it, and a button (Back unless named)
    /// to the entry's menu; `summary` is what the menu shows of it. With
    /// `wait`, the page shows for that many seconds in place of a button,
    /// then leaves the menus altogether when `home`, or gives way to the
    /// `after` text with Continue. `run` is a shell command started as the
    /// page opens.
    Info { text: String, link: Option<String>, button: Option<String>, summary: Option<String>, wait: Option<f64>, home: bool,
           after: Option<String>, run: Option<String> },
    Back,
    /// Back up this many levels at once.
    Up(usize),
    /// A heading a press opens in place, showing its lines: a label and a
    /// value, which may name settings as `{key}`.
    Section(Vec<(String, String)>),
    /// Leaves the menus for the home screen, from any depth.
    Done,
    /// Asks for a text: the keyboard, or boxes of a pattern.
    Ask(Ask),
}

/// A text asked for: kept as `key` and, with `run`, given to that shell
/// command as $VALUE. `boxes` is a pattern of boxes (`9` a digit, `A` a
/// letter, `*` either, `?` either or nothing, a space a gap), the keyboard
/// when absent; `prompt` is what the boxes ask or the empty field says.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub key: String,
    pub boxes: Option<String>,
    pub prompt: String,
    pub max: usize,
    pub run: Option<String>,
}

/// A form's field: a temperature in Celsius, or a count of `unit`, kept
/// as a number or "off", which lies past one end of the range when `off`
/// says which.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub label: String,
    /// What the entry's tile calls it ("Min").
    pub short: String,
    pub key: String,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub off: Option<Off>,
    pub unit: Option<String>,
    /// A field of options instead of a number (Pro Setup's): its box says
    /// the option's label, the ring goes round them, and the option's value
    /// is kept.
    pub options: Vec<Choice>,
}

/// A form's rows after its fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Done,
    More,
    Set,
    Back,
    Disable,
}

impl Button {
    pub fn name(self) -> &'static str {
        match self {
            Button::Done => "done",
            Button::More => "more",
            Button::Set => "set",
            Button::Back => "back",
            Button::Disable => "disable",
        }
    }
}

/// A form's buttons, in order.
fn buttons(kind: &Kind, settings: &Settings) -> Vec<Button> {
    let Kind::Form { more, more_first, enable, .. } = kind else { return Vec::new() };
    match enable {
        Some(key) if settings.text(key) == Some("on") => vec![Button::Done, Button::Disable],
        Some(_) => vec![Button::Set, Button::Back],
        None if more.is_none() => vec![Button::Done],
        None if *more_first => vec![Button::More, Button::Done],
        None => vec![Button::Done, Button::More],
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Off {
    Below,
    Above,
}

impl Field {
    /// The values the field steps through, in order.
    fn values(&self) -> Vec<Option<f64>> {
        let n = ((self.max - self.min) / self.step).round() as i64;
        let numbers = (0..=n).map(|i| Some(self.min + i as f64 * self.step));
        match self.off {
            Some(Off::Below) => std::iter::once(None).chain(numbers).collect(),
            Some(Off::Above) => numbers.chain(std::iter::once(None)).collect(),
            None => numbers.collect(),
        }
    }

    /// The field's value in the settings: a number, or None for "off".
    pub fn value(&self, settings: &Settings) -> Option<f64> {
        if !self.options.is_empty() {
            let text = settings.text(&self.key);
            return Some(self.options.iter().position(|o| Some(o.value.as_str()) == text).unwrap_or(0) as f64);
        }
        match settings.get(&self.key) {
            Some(&Value::Number(n)) => Some(n),
            _ => None,
        }
    }

    fn step(&self, current: Option<f64>, detents: i32) -> Option<f64> {
        if !self.options.is_empty() {
            let n = self.options.len() as i64;
            return Some((current.unwrap_or(0.0) as i64 + detents as i64).rem_euclid(n) as f64);
        }
        let values = self.values();
        let at = values.iter().position(|v| match (v, current) {
            (Some(a), Some(b)) => (a - b).abs() < self.step / 2.0,
            (None, None) => true,
            _ => false,
        }).unwrap_or_else(|| values.iter().position(|v| v.is_some_and(|a| current.is_some_and(|b| a >= b))).unwrap_or(0));
        values[(at as i64 + detents as i64).clamp(0, values.len() as i64 - 1) as usize]
    }

    /// What is kept for a value: the option's, a number, or "off".
    fn stored(&self, v: Option<f64>) -> Value {
        match self.options.get(v.unwrap_or(0.0) as usize) {
            Some(option) => Value::Text(option.value.clone()),
            None => v.map_or(Value::Text("off".into()), Value::Number),
        }
    }

    /// What the box says of a field of options.
    fn text(&self, v: Option<f64>) -> Option<String> {
        self.options.get(v.unwrap_or(0.0) as usize).map(|o| crate::i18n::tr(&o.label))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub label: String,
    pub icon: Option<String>,
    /// How the skin is asked to present the entry, in its own terms.
    pub style: Option<String>,
    /// A question or explanation the skin shows with the entry's screen.
    pub prompt: Option<String>,
    /// When the entry is in the menus, and prompts that replace `prompt`.
    pub shown: Shown,
    pub kind: Kind,
}

/// How an entry shows: only while a toggle-like setting is on (or off),
/// prompts used while one is on (or, as "key=value", has that value), a page of text before it, and what its
/// tile says for each value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shown {
    pub when: Option<(String, bool)>,
    /// The setting that runs the activity arc on the entry's page while on.
    pub busy: Option<String>,
    pub prompts: Vec<(String, String)>,
    pub intro: Option<String>,
    pub summaries: Vec<(String, String)>,
    /// The setting whose value a submenu's tile summarises.
    pub summarize: Option<String>,
    /// What the submenu's page is titled, when not the entry's label.
    pub title: Option<String>,
    /// Lines under its prompt, set lighter (a place being confirmed).
    pub detail: Option<String>,
    /// A setting whose lines ("label", or "label|icon|flag") stand in for
    /// this entry, one each, as Set entries of the entry's key.
    pub fill: Option<String>,
    /// Picks that keep their order instead of putting the one given first.
    pub ordered: bool,
    /// A symbol for the entry's tile alone, when its page shows none, or a
    /// picture (`unit:NAME`, `run:NAME`).
    pub tile: Option<String>,
    /// A picture (`unit:NAME`, `run:NAME`, `plugin:NAME`) on the entry's
    /// page, and its width and height when not square.
    pub image: Option<String>,
    pub image_size: Option<(f32, f32)>,
    /// A condition that ticks an entry that sets nothing (an answer that
    /// only explains, as stock's Weather offline).
    pub tick: Option<String>,
}

/// A condition's setting is on: true, "on", or for "key=value" that value.
fn is_on(settings: &Settings, key: &str) -> bool {
    match key.split_once('=') {
        Some((k, v)) => settings.text(k) == Some(v),
        None => matches!(settings.get(key), Some(Value::Bool(true))) || settings.text(key) == Some("on"),
    }
}

/// The tree as the settings show it: entries hidden by their condition
/// left out, prompts chosen.
pub fn shown(items: &[Item], settings: &Settings) -> Vec<Item> {
    let on = |key: &str| is_on(settings, key);
    items.iter().filter(|i| i.shown.when.as_ref().is_none_or(|(k, v)| on(k) == *v)).flat_map(|i| {
        // An entry filled from a setting: a row per line, each a Set of
        // the entry's key to its label, ticked by its flag.
        if let (Some(fill), Kind::Set { key, .. }) = (&i.shown.fill, &i.kind) {
            return settings.text(fill).unwrap_or_default().lines().filter(|l| !l.is_empty()).map(|line| {
                let mut parts = line.split('|');
                let label = parts.next().unwrap_or_default().to_string();
                let icon = parts.next().filter(|s| !s.is_empty()).map(str::to_string);
                let flag = parts.next().unwrap_or_default().to_string();
                // A fourth part is the value set, when not the label.
                let value = parts.next().filter(|v| !v.is_empty()).map_or_else(|| label.clone(), str::to_string);
                Item { label: label.clone(), icon, style: Some(flag), prompt: None, shown: Shown::default(),
                       kind: Kind::Set { key: key.clone(), value, confirm: None } }
            }).collect::<Vec<_>>();
        }
        // The same for a pick that asks first: its page's answer takes each
        // line's label and value (the Language list).
        if let (Some(fill), Kind::Menu(page), Some("ask")) = (&i.shown.fill, &i.kind, i.style.as_deref()) {
            return settings.text(fill).unwrap_or_default().lines().filter(|l| !l.is_empty()).filter_map(|line| {
                let mut parts = line.split('|');
                let label = parts.next().unwrap_or_default().to_string();
                let value = parts.nth(2).filter(|v| !v.is_empty()).map_or_else(|| label.clone(), str::to_string);
                let mut page = page.clone();
                let Some(Item { kind: Kind::Set { key, confirm, .. }, .. }) = page.first().cloned() else { return None };
                page[0] = Item { label: label.clone(), kind: Kind::Set { key, value, confirm }, ..page[0].clone() };
                Some(Item { label, kind: Kind::Menu(page), shown: Shown { fill: None, ..i.shown.clone() }, ..i.clone() })
            }).collect::<Vec<_>>();
        }
        vec![i.clone()]
    }).map(|mut i| {
        if let Some((_, p)) = i.shown.prompts.iter().find(|(k, _)| on(k)) {
            i.prompt = Some(p.clone());
        }
        if let Kind::Menu(children) = &i.kind {
            let mut children = shown(children, settings);
            // Picks put the one given first, as stock's Placement does
            // (its Language keeps the alphabet).
            if i.style.as_deref() == Some("picks") && !i.shown.ordered {
                // (questions keep their order and tick the answer instead)
                // Custom rooms take their place in the alphabet, as stock's.
                let at: Vec<usize> = (0..children.len()).filter(|&k| matches!(children[k].kind, Kind::Set { .. })).collect();
                let mut sets: Vec<Item> = at.iter().map(|&k| children[k].clone()).collect();
                sets.sort_by_key(|c| c.label.to_lowercase());
                for (k, item) in at.into_iter().zip(sets) {
                    children[k] = item;
                }
                let given = |c: &Item| matches!(&c.kind, Kind::Set { key, value, .. } if settings.text(key) == Some(value.as_str()));
                if let Some(at) = children.iter().position(given) {
                    let pick = children.remove(at);
                    children.insert(0, pick);
                }
            }
            i.kind = Kind::Menu(children);
        }
        i
    }).collect()
}

/// The menus file's tree, and what it calls Back.
#[derive(Clone, Debug, PartialEq)]
pub struct Tree {
    pub title: String,
    pub back: String,
    pub items: Vec<Item>,
    /// Settings read from shell commands while the menus are open: key
    /// and command (plugins' `values`).
    pub values: Vec<(String, String)>,
}

/// What pressing the button led to, beyond moving through the menus.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    /// Back from the top menu.
    Closed,
    Action(String),
    Ask(Ask),
}

/// Where the user is in the menus.
#[derive(Default)]
pub struct Nav {
    /// The selected index at each level, the top menu first.
    levels: Vec<usize>,
    /// The value being edited, not yet committed.
    editing: Option<Value>,
    /// Editing a choice, Back is highlighted instead of an option.
    cancel: bool,
    /// An info page is open.
    reading: bool,
    /// An entry's intro page is open, before the entry itself.
    introduced: bool,
    /// How far a long info page is scrolled, in lines.
    scrolled: usize,
    /// A form is open: the selected row (its fields, then its buttons)
    /// and the value being changed in it.
    form: Option<(usize, Option<Option<f64>>)>,
    /// The form's fields as it opened, for Back.
    opened: Vec<(String, Option<Value>)>,
    /// The section open, at its level (depth, index).
    expanded: Option<(usize, usize)>,
    /// When a timed page closes itself.
    page_until: Option<std::time::Instant>,
    /// A timed page gave way to its `after` text.
    after: bool,
    /// On a confirm page, how far the ring has been turned towards its
    /// action, in steps of CONFIRM_TURNS.
    turns: i32,
    /// Detents turned in the menus, net.
    travel: i64,
    /// Counts presses and openings: a new value is a new page.
    page: u64,
    /// Detents turned toward the next tile or row, when, and at which depth.
    carousel_turn: (i32, Option<std::time::Instant>, usize),
    row_turn: (i32, Option<std::time::Instant>, usize),
    /// A list's or form's place on the wheel, in detents from its first
    /// row, and the level (and whether a form) it belongs to.
    list: Option<(Vec<usize>, bool, f32)>,
    /// The wheel's counts moved a list since its last steps.
    counted: bool,
    /// The steps being taken are a mouse's notches: an entry each.
    pub notched: bool,
    /// The open carousel's wheel physics, and the levels it belongs to.
    scroll: Option<(Vec<usize>, crate::scroll::Scroll)>,
}

/// Half steps of the wheel (a detent each) to a carousel's tile and to a
/// row of a list or form: stock's Settings moves a tile for about four
/// detents, a part lost after a pause, and its lists and forms a row for
/// three, however slowly turned (matched on nest-qemu).
const CAROUSEL_HALVES: i32 = 4;
const ROW_HALVES: i32 = 3;

/// A wheel detent's counts (the gen 3's).
const DETENT_COUNTS: f32 = 120.0;

/// How far a list's place goes past its first and last rows, in detents.
const ROW_SLACK: f32 = 1.0;

/// Technical Info's sections (stock's systeminfolayer): its first row
/// holds SECTION_LEAD detents more, its last (DONE) sits SECTION_DONE
/// past the row before, and its place ends there (matched on nest-qemu).
const SECTION_LEAD: f32 = 3.0;
const SECTION_DONE: f32 = 7.0;
const CAROUSEL_PAUSE: std::time::Duration = std::time::Duration::from_millis(600);

/// Steps of the wheel that fill a confirm page's ring: stock's fills
/// 15 degrees of its 235 for two detents (matched on nest-qemu).
pub const CONFIRM_TURNS: i32 = 15;

impl Nav {
    pub fn open(&mut self, tree: &Tree) {
        self.page += 1;
        self.levels = vec![back_index(&tree.items)];
        self.editing = None;
        self.cancel = false;
        self.reading = false;
        self.introduced = false;
        self.form = None;
        self.expanded = None;
        self.turns = 0;
    }

    /// Opens the menus at an entry, named level by level from the top.
    pub fn open_entry(&mut self, tree: &Tree, labels: &[&str]) {
        self.open(tree);
        let mut items = tree.items.as_slice();
        let mut levels = Vec::new();
        for label in labels {
            let Some(at) = items.iter().position(|i| i.label == *label) else { return };
            levels.push(at);
            match &items[at].kind {
                Kind::Menu(children) => items = children,
                _ => break,
            }
        }
        self.levels = levels;
        self.levels.push(first_entry(items));
    }

    /// Opens the submenu `labels` lead to, on its entry `at` (as shown)
    /// when given; whether they lead to one.
    pub fn open_at(&mut self, tree: &Tree, labels: &[&str], at: Option<usize>) -> bool {
        self.open_entry(tree, labels);
        let mut items = tree.items.as_slice();
        for label in labels {
            match items.iter().find(|i| i.label == *label).map(|i| &i.kind) {
                Some(Kind::Menu(children)) => items = children,
                _ => {
                    self.close();
                    return false;
                }
            }
        }
        if let (Some(at), Some(selected)) = (at, self.levels.last_mut()) {
            *selected = at.min(entries(items).saturating_sub(1));
        }
        self.scroll = None;
        self.list = None;
        true
    }

    pub fn is_open(&self) -> bool {
        !self.levels.is_empty()
    }

    /// Where the menus are, for the log: the menus leading here, the entry
    /// selected and what is being done on it.
    pub fn place(&self, tree: &Tree) -> Option<String> {
        if !self.is_open() {
            return None;
        }
        let (items, titles) = self.level(tree);
        let at = self.levels.last().copied().unwrap_or_default();
        let entry = match items.get(at) {
            Some(item) if !item.label.is_empty() => item.label.clone(),
            Some(item) => format!("#{at} {}", kind_name(&item.kind)),
            None => format!("#{at} {}", tree.back),
        };
        let doing = match (&self.form, &self.editing) {
            (Some((row, Some(_))), _) => format!(", changing row {row}"),
            (Some((row, None)), _) => format!(", row {row}"),
            (_, Some(_)) if self.cancel => ", editing, on back".into(),
            (_, Some(v)) => format!(", editing {v:?}"),
            _ if self.reading => ", its page".into(),
            _ if self.introduced => ", its intro".into(),
            _ if self.turns > 0 => format!(", confirm {}/{CONFIRM_TURNS}", self.turns),
            _ => String::new(),
        };
        Some(format!("{} > [{entry}]{doing}", titles.join(" > ")).replace('\n', " "))
    }

    /// Whether an open menu is the entry `label` or lies under it.
    pub fn within(&self, tree: &Tree, label: &str) -> bool {
        let mut items = tree.items.as_slice();
        for &at in self.levels.iter().take(self.levels.len().saturating_sub(1)) {
            let Some(item) = items.get(at) else { return false };
            if item.label == label {
                return true;
            }
            match &item.kind {
                Kind::Menu(children) => items = children,
                _ => return false,
            }
        }
        false
    }

    /// Whether a form is open.
    pub fn form_open(&self) -> bool {
        self.form.is_some()
    }

    /// Puts an open form's fields back as they were when it opened: Back,
    /// or the menus closing on their own (stock keeps a change only once
    /// DONE or SET follows).
    pub fn abandon_form(&mut self, settings: &mut Settings) {
        if self.form.is_none() {
            return;
        }
        debug!("menu: form left; its fields put back");
        for (key, value) in std::mem::take(&mut self.opened) {
            if let Some(v) = value {
                settings.set(&key, v);
            }
        }
    }

    /// The keys an open form is editing, held back from the cloud until
    /// it is done.
    pub fn form_keys(&self, tree: &Tree) -> Vec<String> {
        if self.form.is_none() {
            return Vec::new();
        }
        let (items, _) = self.level(tree);
        match self.levels.last().and_then(|&s| items.get(s)).map(|i| &i.kind) {
            Some(Kind::Form { fields, enable, .. }) => fields.iter().map(|f| f.key.clone()).chain(enable.clone()).collect(),
            _ => Vec::new(),
        }
    }

    /// A timed page that has run its time closes, and its level with it
    /// (stock's Software goes back to Settings). True when one did.
    pub fn expire(&mut self, tree: &Tree) -> bool {
        if !self.reading || self.page_until.is_none_or(|at| std::time::Instant::now() < at) {
            return false;
        }
        let (items, _) = self.level(tree);
        let item = self.levels.last().and_then(|&s| items.get(s));
        let home = matches!(item, Some(Item { kind: Kind::Info { home: true, .. }, .. }));
        self.page_until = None;
        if matches!(item, Some(Item { kind: Kind::Info { after: Some(_), .. }, .. })) {
            self.after = true;
            return true;
        }
        self.reading = false;
        if home {
            self.close();
        } else if self.levels.len() > 1 {
            self.levels.pop();
        }
        true
    }

    /// Back up a level, onto the entry the open menu came from.
    pub fn up(&mut self) {
        if self.levels.len() > 1 {
            self.levels.pop();
            self.editing = None;
            self.reading = false;
            self.introduced = false;
            self.form = None;
        }
    }

    /// Back on a list of picks or choices from a page of its own, its
    /// first entry is selected (stock's Reset and Equipment Settings); from
    /// a confirmation, the pick stays (stock's Language).
    fn back_to_first(&mut self, tree: &Tree, leaving: Option<&str>) {
        if self.levels.len() < 2 || leaving == Some("ask") {
            return;
        }
        let style = self.submenu(tree).2.and_then(|p| p.style.clone());
        if matches!(style.as_deref(), Some("picks" | "choices")) {
            if let Some(s) = self.levels.last_mut() {
                *s = 0;
            }
        }
    }

    pub fn close(&mut self) {
        self.levels.clear();
        self.editing = None;
        self.cancel = false;
        self.reading = false;
        self.introduced = false;
        self.form = None;
        self.expanded = None;
    }

    /// The items at the current level and the titles leading to it.
    fn level<'a>(&self, tree: &'a Tree) -> (&'a [Item], Vec<&'a str>) {
        let (items, titles, _) = self.submenu(tree);
        (items, titles)
    }

    /// The current level's items, the titles leading to it, and the entry
    /// it opened from (none at the top).
    fn submenu<'a>(&self, tree: &'a Tree) -> (&'a [Item], Vec<&'a str>, Option<&'a Item>) {
        let mut items = tree.items.as_slice();
        let mut titles = vec![tree.title.as_str()];
        let mut parent = None;
        for &index in &self.levels[..self.levels.len().saturating_sub(1)] {
            match items.get(index) {
                Some(item @ Item { kind: Kind::Menu(children), .. }) => {
                    items = children;
                    titles.push(item.shown.title.as_deref().unwrap_or(&item.label));
                    parent = Some(item);
                }
                _ => break,
            }
        }
        (items, titles, parent)
    }

    /// Whether the current level is a submenu of answers.
    fn answers(&self, tree: &Tree) -> bool {
        is_answers(self.submenu(tree).2)
    }

    /// Whether the open level is a carousel of tiles, the wheel moving it
    /// through stock's physics (`rotary`) rather than by detents.
    fn carousel(&self, tree: &Tree) -> bool {
        if self.levels.len() < 2 || self.reading || self.introduced || self.editing.is_some() || self.form.is_some() {
            return false;
        }
        let parent_style = self.submenu(tree).2.and_then(|p| p.style.clone());
        !matches!(parent_style.as_deref(), Some("list" | "answers" | "ask" | "titled" | "picks" | "question"
            | "system" | "choices" | "network" | "heatlink" | "confirm"))
    }

    /// Whether the top menu's ring is open and turning picks its entries.
    fn ring(&self) -> bool {
        self.levels.len() == 1 && !self.reading && !self.introduced && self.editing.is_none() && self.form.is_none()
    }

    /// The open carousel's or ring's physics, made at its selection when
    /// it opens.
    fn carousel_scroll(&mut self, tree: &Tree) -> Option<&mut crate::scroll::Scroll> {
        let tuning = if self.carousel(tree) { crate::scroll::SETTINGS } else if self.ring() { crate::scroll::MENU } else {
            self.scroll = None;
            return None;
        };
        let parents = self.levels[..self.levels.len() - 1].to_vec();
        if self.scroll.as_ref().is_none_or(|(p, _)| *p != parents) {
            let count = entries(self.level(tree).0);
            let at = *self.levels.last()?;
            self.scroll = Some((parents, crate::scroll::Scroll::new(tuning, count, at)));
        }
        self.scroll.as_mut().map(|(_, s)| s)
    }

    /// A wheel event's counts (the gen 3's, negative clockwise) on a
    /// carousel; false where the level moves by detents instead.
    pub fn rotary(&mut self, tree: &Tree, counts: i32, now: std::time::Instant) -> bool {
        if let Some((rows, rank)) = self.list_rows(tree) {
            // A list's place moves with every count, its selection the row
            // nearest: stock's lists and forms take ROW_HALVES detents a
            // row, and go a detent past either end.
            let key = (self.levels[..self.levels.len() - 1].to_vec(), self.form.is_some());
            let (centres, end) = self.list_places(tree, rows);
            // The nearest row's, the later one halfway.
            let row = |pos: f32| (0..rows).rev().min_by(|&a, &b| (pos - centres[a]).abs().total_cmp(&(pos - centres[b]).abs())).unwrap_or(0) as i64;
            // Kept while its row is the one picked, else from that row.
            let pos = match &self.list {
                Some((p, f, pos)) if (p, f) == (&key.0, &key.1) && row(*pos) == rank as i64 => *pos,
                _ if rank == 0 => 0.0,
                _ => centres[rank],
            };
            let pos = (pos - counts as f32 / DETENT_COUNTS).clamp(-ROW_SLACK, end);
            self.list = Some((key.0, key.1, pos));
            let rows_moved = (row(pos) - rank as i64) as i32;
            if rows_moved != 0 {
                self.travel -= i64::from(rows_moved);
                self.wheel(tree, rows_moved);
            }
            self.counted = true;
            return true;
        }
        let Some(scroll) = self.carousel_scroll(tree) else { return false };
        scroll.wheel(counts, now);
        let at = scroll.index();
        if let Some(selected) = self.levels.last_mut() {
            *selected = at;
        }
        true
    }

    /// Moves the open carousel on to `now`; whether it still moves.
    pub fn tick(&mut self, tree: &Tree, now: std::time::Instant) -> bool {
        let Some(scroll) = self.carousel_scroll(tree) else { return false };
        let moving = scroll.step(now);
        let at = scroll.index();
        if let Some(selected) = self.levels.last_mut() {
            *selected = at;
        }
        moving
    }

    /// The open list's or form's rows the wheel picks from and the one
    /// picked, while one is open and nothing in it is being changed.
    fn list_rows(&self, tree: &Tree) -> Option<(usize, usize)> {
        let rows = self.levels.len() > 1 && !self.reading && !self.introduced && self.editing.is_none()
            && !self.carousel(tree) && self.submenu(tree).2.is_none_or(|p| p.style.as_deref() != Some("confirm"))
            && self.form.is_none_or(|(_, changing)| changing.is_none());
        if !rows {
            return None;
        }
        let (items, _) = self.level(tree);
        let &selected = self.levels.last()?;
        if let (Some((at, _)), Some(Item { kind: Kind::Form { fields, more, enable, .. }, .. })) = (self.form, items.get(selected)) {
            return Some((fields.len() + 1 + usize::from(enable.is_some() || more.is_some()), at));
        }
        let count = if self.answers(tree) || is_system(self.submenu(tree).2) { items.len() } else { entries(items) };
        let heading = |i: &&Item| i.kind == Kind::Heading;
        let headings = items[..count.min(items.len())].iter().filter(heading).count();
        let before = items[..selected.min(items.len())].iter().filter(heading).count();
        Some((count - headings, selected - before))
    }

    /// The open list's place on the wheel, 0 at its first row to 1 a detent
    /// past its last, for its scrolling.
    fn list_at(&self, tree: &Tree) -> Option<f64> {
        let (rows, _) = self.list_rows(tree)?;
        let (p, f, pos) = self.list.as_ref()?;
        if *p != self.levels[..self.levels.len() - 1] || *f != self.form.is_some() {
            return None;
        }
        let (_, end) = self.list_places(tree, rows);
        Some(f64::from((pos / end).clamp(0.0, 1.0)))
    }

    /// Where the open list's rows sit on the wheel, in detents, and where
    /// its place ends: ROW_HALVES apart from 0 and ROW_SLACK past the last,
    /// but for a list of sections.
    fn list_places(&self, tree: &Tree, rows: usize) -> (Vec<f32>, f32) {
        let pitch = ROW_HALVES as f32;
        let sections = self.form.is_none() && self.level(tree).0.iter().any(|i| matches!(i.kind, Kind::Section(_)));
        if !sections {
            return ((0..rows).map(|r| r as f32 * pitch).collect(), (rows - 1) as f32 * pitch + ROW_SLACK);
        }
        let mut centres: Vec<f32> = (0..rows).map(|r| SECTION_LEAD + r as f32 * pitch).collect();
        if rows > 1 {
            centres[rows - 1] = centres[rows - 2] + SECTION_DONE;
        }
        let end = centres[rows - 1];
        (centres, end)
    }

    /// The wheel turned `steps` (two detents each): a list's or a form's
    /// selection moves a row for every ROW_HALVES detents, anything else
    /// as `wheel` has it.
    pub fn turn(&mut self, tree: &Tree, steps: i32) {
        if self.list_rows(tree).is_none() || self.notched {
            return self.wheel(tree, steps);
        }
        self.travel += i64::from(steps);
        // The same turn's counts have moved it.
        if std::mem::take(&mut self.counted) {
            return;
        }
        let rows = Self::halves(&mut self.row_turn, self.levels.len(), steps, ROW_HALVES, false);
        if rows != 0 {
            self.travel -= i64::from(rows);
            self.wheel(tree, rows);
        }
    }

    /// The wheel turned `detents` steps, a row or an option each.
    pub fn wheel(&mut self, tree: &Tree, detents: i32) {
        self.travel += i64::from(detents);
        // A carousel or ring whose physics the wheel's counts move leaves
        // detents be; a notch glides it an entry.
        if self.notched && (self.carousel(tree) || self.ring()) {
            if let Some(scroll) = self.carousel_scroll(tree) {
                scroll.glide_by(detents, std::time::Instant::now());
            }
            return;
        }
        let parents = &self.levels[..self.levels.len().saturating_sub(1)];
        if (self.carousel(tree) || self.ring()) && self.scroll.as_ref().is_some_and(|(p, _)| p == parents) {
            return;
        }
        if self.reading || self.introduced {
            // A long page scrolls; the skin stops it at its end.
            self.scrolled = (self.scrolled as i64 + detents as i64).clamp(0, 40) as usize;
            return;
        }
        let (items, _) = self.level(tree);
        if let (Some((at, changing)), Some(Item { kind: Kind::Form { fields, more, enable, .. }, .. })) =
            (&mut self.form, self.levels.last().and_then(|&s| items.get(s))) {
            match changing {
                Some(v) => *v = fields[*at].step(*v, detents),
                None => {
                    // Two buttons with `enable` or More info, else Done alone.
                    let last = fields.len() + usize::from(enable.is_some() || more.is_some());
                    *at = (*at as i64 + detents as i64).clamp(0, last as i64) as usize;
                }
            }
            return;
        }
        let top = self.levels.len() == 1;
        let answers = self.answers(tree);
        // A confirm page keeps Cancel highlighted; turning fills its ring.
        if self.submenu(tree).2.is_some_and(|p| p.style.as_deref() == Some("confirm")) {
            self.turns = (self.turns + detents).clamp(0, CONFIRM_TURNS);
            return;
        }
        let Some(selected) = self.levels.last_mut() else { return };
        if let Some(pending) = &mut self.editing {
            match items.get(*selected).map(|i| &i.kind) {
                Some(Kind::Choice { options, .. }) => {
                    let at = if self.cancel { options.len() } else { option_index(options, pending).unwrap_or(0) };
                    // Answers alone have no Back after them.
                    let last = if items[*selected].style.as_deref() == Some("answers") { options.len() - 1 } else { options.len() };
                    let to = (at as i64 + detents as i64).clamp(0, last as i64) as usize;
                    self.cancel = to == options.len();
                    if !self.cancel {
                        *pending = Value::Text(options[to].value.clone());
                    }
                }
                Some(_) => *pending = step(&items[*selected], pending, detents),
                None => {}
            }
            return;
        }
        // A carousel takes CAROUSEL_STEPS to a tile.
        let parent_style = self.submenu(tree).2.and_then(|p| p.style.clone());
        let carousel = !top && !matches!(parent_style.as_deref(), Some("list" | "answers" | "ask" | "titled" | "picks" | "question"
            | "system" | "choices" | "network" | "heatlink" | "confirm"));
        let detents = if carousel { Self::halves(&mut self.carousel_turn, self.levels.len(), detents, CAROUSEL_HALVES, true) } else { detents };
        let system = is_system(self.submenu(tree).2);
        let Some(selected) = self.levels.last_mut() else { return };
        // Answers have no Back of their own.
        let count = if answers || system { items.len() } else { entries(items) } as i64;
        // The top menu is a ring and goes round; the levels below stop at their ends.
        let mut to = if top {
            (*selected as i64 + detents as i64).rem_euclid(count.max(1)) as usize
        } else {
            (*selected as i64 + detents as i64).clamp(0, count - 1) as usize
        };
        // Headings are passed over, in the direction of the turn.
        while items.get(to).is_some_and(|i| i.kind == Kind::Heading) {
            let next = to as i64 + detents.signum() as i64;
            if next < 0 || next >= count {
                break;
            }
            to = next as usize;
        }
        *selected = to;
    }

    /// Wheel steps (two detents each) to tiles or rows of `halves` detents,
    /// the part toward the next kept at the same depth and direction, and,
    /// with `pause`, only for a moment.
    fn halves(turn: &mut (i32, Option<std::time::Instant>, usize), depth: usize, steps: i32, halves: i32, pause: bool) -> i32 {
        let now = std::time::Instant::now();
        let (part, at, was) = *turn;
        let kept = was == depth && part.signum() != -steps.signum()
            && (!pause || at.is_some_and(|t| now.duration_since(t) < CAROUSEL_PAUSE));
        let total = if kept { part } else { 0 } + 2 * steps;
        *turn = (total % halves, Some(now), depth);
        total / halves
    }

    /// A confirm page's action once its ring is full; the menus close.
    pub fn confirmed(&mut self, tree: &Tree) -> Option<String> {
        if self.turns < CONFIRM_TURNS {
            return None;
        }
        let (items, _, _) = self.submenu(tree);
        let name = items.iter().find_map(|i| match &i.kind { Kind::Action { name } => Some(name.clone()), _ => None })?;
        self.close();
        Some(name)
    }

    /// The setting the choice being edited sets, if one is.
    pub fn editing_key<'a>(&self, tree: &'a Tree) -> Option<&'a str> {
        self.editing.as_ref()?;
        let (items, _) = self.level(tree);
        key(&items.get(*self.levels.last()?)?.kind)
    }

    /// Editing a choice: highlights `value`, or Back when None.
    pub fn highlight(&mut self, value: Option<Value>) {
        if self.editing.is_none() {
            return;
        }
        self.cancel = value.is_none();
        if let Some(v) = value {
            self.editing = Some(v);
        }
    }

    /// The entry a press would open, with its depth (1 the top menu),
    /// while nothing is being edited.
    pub fn pointed<'a>(&self, tree: &'a Tree) -> Option<(usize, &'a str)> {
        if self.editing.is_some() || self.form.is_some() {
            return None;
        }
        let (items, _, _) = self.submenu(tree);
        items.get(*self.levels.last()?).map(|i| (self.levels.len(), i.label.as_str()))
    }

    pub fn press(&mut self, tree: &Tree, settings: &mut Settings) -> Outcome {
        self.page += 1;
        // A carousel still moving glides to the tile at its middle, which
        // opens, as stock's.
        let parents = self.levels[..self.levels.len().saturating_sub(1)].to_vec();
        if let (Some((p, scroll)), true) = (self.scroll.as_mut(), self.levels.len() > 1) {
            if *p == parents {
                scroll.glide(std::time::Instant::now());
            }
        }
        // The ring's physics starts afresh at whatever the press opens.
        if self.levels.len() == 1 {
            self.scroll = None;
        }
        let (items, _, parent) = self.submenu(tree);
        let Some(&selected) = self.levels.last() else { return Outcome::Stay };
        // A level of answers is left once one is given, or its form done.
        let answers = is_answers(parent);
        // Back from an info page returns to its menu's first answer (the
        // answer given, among answers) under a prompt, to the entry itself
        // in other menus, or home from the top menu's.
        let item = items.get(selected);
        if let (Some((at, changing)), Some(form @ Item { kind: Kind::Form { fields, more_first, enable, action, .. }, .. })) = (self.form, item) {
            let buttons = buttons(&form.kind, settings);
            if std::mem::take(&mut self.reading) {
                // Back to the first field, or to More info when it leads.
                let more = buttons.iter().position(|b| *b == Button::More).map(|i| fields.len() + i);
                self.form = Some((if *more_first { more.unwrap_or(0) } else { 0 }, None));
            } else if let Some(v) = changing {
                // Set, and on to the next row.
                settings.change(&fields[at].key, fields[at].stored(v));
                self.form = Some((at + 1, None));
            } else if at < fields.len() {
                self.form = Some((at, Some(fields[at].value(settings))));
            } else {
                match buttons.get(at - fields.len()) {
                    Some(Button::More) => {
                        self.reading = true;
                        self.scrolled = 0;
                        return Outcome::Stay;
                    }
                    Some(Button::Set) => settings.change(enable.as_deref().unwrap_or_default(), Value::Text("on".into())),
                    Some(Button::Disable) => settings.change(enable.as_deref().unwrap_or_default(), Value::Text("off".into())),
                    Some(Button::Back) => self.abandon_form(settings),
                    Some(Button::Done) if action.is_some() => {
                        let name = action.clone().unwrap_or_default();
                        self.close();
                        return Outcome::Action(name);
                    }
                    Some(Button::Done) | None => {}
                }
                self.form = None;
                if answers {
                    self.levels.pop();
                }
            }
            return Outcome::Stay;
        }
        let past_intro = std::mem::take(&mut self.introduced);
        if past_intro && !matches!(item, Some(Item { kind: Kind::Menu(_), .. })) {
            // Past the intro, on to the entry itself.
            if let Some(Item { kind: Kind::Choice { key, .. }, .. }) = item {
                self.editing = settings.get(key).cloned().or_else(|| item.and_then(|i| first(&i.kind)));
            }
            return Outcome::Stay;
        }
        // A submenu past its intro opens as a press on it would.
        if !past_intro && item.is_some_and(|i| i.shown.intro.is_some() && i.kind != Kind::Back) && self.editing.is_none() {
            self.introduced = true;
            return Outcome::Stay;
        }
        if self.reading && self.page_until.is_some() {
            // A timed page takes no input.
            return Outcome::Stay;
        }
        if std::mem::take(&mut self.reading) {
            // A Set's confirmation goes back to the entry itself, or out of
            // its answers.
            if item.is_some_and(|i| matches!(i.kind, Kind::Set { .. })) {
                if answers {
                    self.levels.pop();
                }
                return Outcome::Stay;
            }
            if self.levels.len() == 1 {
                self.close();
                return Outcome::Closed;
            }
            let prompted = parent.is_some_and(|p| p.prompt.is_some());
            if let Some(s) = self.levels.last_mut() {
                if answers {
                    *s = answer_index(items, settings);
                } else if prompted {
                    *s = first_entry(items);
                }
            }
            return Outcome::Stay;
        }
        if items.get(selected).is_some_and(|i| i.kind == Kind::Done) {
            self.close();
            return Outcome::Closed;
        }
        let leaving = self.submenu(tree).2.and_then(|p| p.style.clone());
        if let Some(&Item { kind: Kind::Up(n), .. }) = items.get(selected) {
            let keep = self.levels.len().saturating_sub(n).max(1);
            self.levels.truncate(keep);
            self.editing = None;
            self.back_to_first(tree, leaving.as_deref());
            return Outcome::Stay;
        }
        let item = items.get(selected).filter(|i| i.kind != Kind::Back);
        let Some(item) = item else {
            self.levels.pop();
            self.editing = None;
            // Back from an entry of the top menu goes home, as stock's does.
            if self.levels.len() == 1 {
                self.levels.clear();
            }
            self.back_to_first(tree, leaving.as_deref());
            return if self.levels.is_empty() { Outcome::Closed } else { Outcome::Stay };
        };
        if let Some(pending) = self.editing.take() {
            match (key(&item.kind), std::mem::take(&mut self.cancel)) {
                (Some(key), false) => settings.change(key, pending),
                (Some(key), true) => debug!("menu: {key} left as it was"),
                _ => {}
            }
            // An edit opened from the top menu ends back home, as stock's do.
            if self.levels.len() == 1 {
                self.levels.clear();
                return Outcome::Closed;
            }
            return Outcome::Stay;
        }
        match &item.kind {
            Kind::Menu(children) if item.style.as_deref() == Some("answers") => self.levels.push(answer_index(children, settings)),
            // Choices open on their first entry, Done included (stock's Equipment Settings).
            Kind::Menu(_) if matches!(item.style.as_deref(), Some("choices" | "confirm")) => {
                self.turns = 0;
                self.levels.push(0);
            }
            // A chain of questions opens on its first one.
            Kind::Menu(children) if matches!(item.style.as_deref(), Some("steps" | "chain")) => {
                self.levels.push(0);
                if let Some(Kind::Menu(first)) = children.first().map(|c| &c.kind) {
                    self.levels.push(answer_index(first, settings));
                }
            }
            // Picks open on the one given (Placement puts it first anyway).
            Kind::Menu(children) if item.style.as_deref() == Some("picks") => self.levels.push(answer_index(children, settings)),
            Kind::Menu(children) => {
                self.turns = 0;
                self.levels.push(first_entry(children));
            }
            Kind::Toggle { key } => {
                let on = matches!(settings.get(key), Some(Value::Bool(true)));
                settings.change(key, Value::Bool(!on));
            }
            Kind::Choice { key, options } if matches!(item.style.as_deref(), Some("cycle" | "units")) => {
                let at = settings.get(key).and_then(|v| option_index(options, v)).unwrap_or(0);
                settings.change(key, Value::Text(options[(at + 1) % options.len()].value.clone()));
            }
            Kind::Choice { key, .. } | Kind::Range { key, .. } => {
                self.editing = settings.get(key).cloned().or_else(|| first(&item.kind));
            }
            Kind::Action { name } => return Outcome::Action(name.clone()),
            Kind::Ask(ask) => return Outcome::Ask(ask.clone()),
            Kind::Info { wait, run, .. } => {
                self.reading = true;
                self.after = false;
                self.scrolled = 0;
                self.page_until = wait.map(|s| std::time::Instant::now() + std::time::Duration::from_secs_f64(s));
                if let Some(command) = run {
                    return Outcome::Action(format!("run:{command}"));
                }
            }
            Kind::Form { fields, .. } => {
                self.form = Some((0, None));
                self.opened = fields.iter().map(|f| (f.key.clone(), settings.get(&f.key).cloned())).collect();
            }
            Kind::Set { key, value, confirm } => {
                settings.change(key, Value::Text(value.clone()));
                if answers && confirm.is_some() {
                    // Answers with a confirmation leave once it is read.
                    self.reading = true;
                } else if answers {
                    // A confirmed pick (stock's Language) leaves its list too.
                    let asked = self.submenu(tree).2.is_some_and(|p| p.style.as_deref() == Some("ask"));
                    self.levels.pop();
                    if asked && self.levels.len() > 1 {
                        self.levels.pop();
                    }
                    // A step of a chain of questions: the next opens, or
                    // the last answer leaves the chain.
                    let depth = self.levels.len();
                    let (items, _, grandparent) = self.submenu(tree);
                    let chain = grandparent.and_then(|g| g.style.as_deref()).filter(|s| matches!(*s, "steps" | "chain"));
                    if depth >= 2 && chain.is_some() {
                        let at = self.levels[depth - 1];
                        if at + 1 < items.len() && matches!(items[at + 1].kind, Kind::Menu(_)) {
                            self.levels[depth - 1] = at + 1;
                            if let Kind::Menu(children) = &items[at + 1].kind {
                                self.levels.push(answer_index(children, settings));
                            }
                        } else {
                            // The last answer leaves the chain, and a chain
                            // of "steps" its menu too.
                            self.levels.pop();
                            if chain == Some("steps") {
                                self.levels.pop();
                            }
                        }
                    }
                } else {
                    self.reading = confirm.is_some();
                }
            }
            Kind::Heading => {}
            Kind::Section(_) => {
                let at = (self.levels.len(), selected);
                self.expanded = if self.expanded == Some(at) { None } else { Some(at) };
            }
            Kind::Back | Kind::Up(_) | Kind::Done => {}
        }
        Outcome::Stay
    }

    /// The current menu as the skin draws it.
    pub fn view(&self, tree: &Tree, settings: &Settings) -> Option<View> {
        let &selected = self.levels.last()?;
        let (items, titles, parent) = self.submenu(tree);
        let mut rows: Vec<Row> = items.iter().enumerate().map(|(i, item)| {
            let editing = i == selected && self.editing.is_some();
            let value = if editing { self.editing.clone() } else { key(&item.kind).and_then(|k| settings.get(k).cloned()) };
            let summary = match &item.kind {
                Kind::Info { summary, .. } => summary.as_ref().map(|s| fill(s, settings)),
                // Without summaries, its ".name" setting (the language's
                // name), or the setting's text as it is.
                _ if item.shown.summaries.is_empty() => item.shown.summarize.as_deref()
                    .and_then(|k| settings.text(&format!("{k}.name")).or_else(|| settings.text(k))).map(str::to_string),
                // A value the summaries do not name is named by its ".name"
                // setting (Placement's custom rooms).
                _ => item.shown.summarize.as_deref().and_then(|k| settings.text(k).and_then(|v| item.shown.summaries.iter().find(|(s, _)| s == v))
                    .map(|(_, s)| s.clone()).or_else(|| settings.text(&format!("{k}.name")).map(str::to_string))),
            };
            let back = matches!(item.kind, Kind::Back | Kind::Up(_));
            let done = item.kind == Kind::Done;
            let (options, saved) = match &item.kind {
                Kind::Choice { options, key } => (options.clone(), settings.get(key).and_then(|v| option_index(options, v))),
                _ => (Vec::new(), None),
            };
            let fraction = match (&item.kind, &value) {
                (Kind::Range { min, max, .. }, Some(Value::Number(n))) => Some((n - min) / (max - min)),
                _ => None,
            };
            Row {
                label: if back && item.label.is_empty() { crate::i18n::tr(&tree.back) }
                       else if done && item.label.is_empty() { crate::i18n::tr("Done") } else { fill(&item.label, settings) },
                icon: item.icon.clone().or_else(|| back.then(|| "back".into())),
                tile: item.shown.tile.clone(),
                style: item.style.clone(),
                prompt: item.prompt.as_deref().map(|p| fill(p, settings)),
                fraction,
                kind: kind_name(&item.kind),
                value: value.as_ref().map(|v| describe(item, v)).or(summary).map(|v| crate::i18n::tr(&v)),
                editing,
                options: options.into_iter().map(|o| Choice { label: crate::i18n::tr(&o.label), ..o }).collect(),
                saved,
                ticked: matches!(&item.kind, Kind::Set { key, value, .. } if settings.text(key) == Some(value.as_str()))
                    || item.shown.tick.as_deref().is_some_and(|k| is_on(settings, k)),
                summary: match &item.kind {
                    Kind::Form { fields, .. } => fields.iter().filter(|f| !f.short.is_empty())
                        .map(|f| (crate::i18n::tr(&f.short), f.value(settings))).collect(),
                    _ => Vec::new(),
                },
                detail: match &item.kind {
                    Kind::Section(lines) if self.expanded == Some((self.levels.len(), i)) =>
                        lines.iter().map(|(label, value)| (fill(label, settings), fill(value, settings))).filter(|(label, _)| !label.is_empty()).collect(),
                    _ => Vec::new(),
                },
            }
        }).collect();
        if !has_exit(items) && !is_answers(parent) && !is_system(parent) {
            rows.push(Row { label: crate::i18n::tr(&tree.back), icon: Some("back".into()), tile: None, style: None, prompt: None, fraction: None, kind: "back", value: None, editing: false,
                            options: Vec::new(), saved: None, ticked: false, summary: Vec::new(), detail: Vec::new() });
        }
        let pending = match (items.get(selected).map(|i| &i.kind), &self.editing) {
            (Some(Kind::Choice { options, .. }), Some(v)) => Some(if self.cancel { options.len() } else { option_index(options, v).unwrap_or(0) }),
            _ => None,
        };
        Some(View {
            title: titles.last().map(|t| crate::i18n::tr(t)).unwrap_or_default(),
            path: titles.iter().map(|t| t.to_string()).collect(),
            depth: self.levels.len() - 1,
            selected,
            editing: self.editing.is_some(),
            pending,
            icon: parent.and_then(|p| p.icon.clone()),
            style: parent.and_then(|p| p.style.clone()),
            prompt: parent.and_then(|p| p.prompt.as_deref()).map(|p| fill(p, settings)),
            detail: parent.and_then(|p| p.shown.detail.as_deref()).map(|d| fill(d, settings)),
            intro: parent.is_some_and(|p| p.shown.intro.is_some()),
            list_at: self.list_at(tree),
            image: parent.and_then(|p| p.shown.image.clone()),
            image_size: parent.and_then(|p| p.shown.image_size),
            rows,
            turned: f64::from(self.turns) / f64::from(CONFIRM_TURNS),
            scroll: self.scroll.as_ref().filter(|(p, _)| self.carousel(tree) && *p == self.levels[..self.levels.len() - 1])
                .map(|(_, s)| f64::from(s.position())),
            travel: self.travel,
            page: self.page,
            form: match (self.form, items.get(selected)) {
                (Some((at, changing)), Some(form @ Item { kind: Kind::Form { fields, .. }, prompt, icon, style, .. })) if !self.reading => Some(FormView {
                    prompt: prompt.as_deref().map(|p| fill(p, settings)),
                    icon: icon.clone(),
                    style: style.clone(),
                    fields: fields.iter().enumerate().map(|(i, f)| (f.label.clone(),
                        if i == at { changing.unwrap_or_else(|| f.value(settings)) } else { f.value(settings) }, f.unit.clone())).collect(),
                    texts: fields.iter().enumerate().map(|(i, f)| f.text(if i == at { changing.unwrap_or_else(|| f.value(settings)) } else { f.value(settings) }))
                        .collect(),
                    buttons: buttons(&form.kind, settings).into_iter().map(Button::name).collect(),
                    at,
                    changing: changing.is_some(),
                }),
                _ => None,
            },
            info: if let (true, Some(Kind::Form { more: Some(text), link, .. })) = (self.reading && self.form.is_some(), items.get(selected).map(|i| &i.kind)) {
                Some(InfoPage { text: text.clone(), link: link.clone(), button: crate::i18n::tr(&tree.back), icon: None, scrolled: self.scrolled, style: None, wait: None, image: None, busy: false })
            } else if let (true, Some(Kind::Set { confirm: Some(text), .. })) = (self.reading, items.get(selected).map(|i| &i.kind)) {
                Some(InfoPage { text: text.clone(), link: None, button: crate::i18n::tr("OK"), icon: None, scrolled: self.scrolled, style: None, wait: None, image: None, busy: false })
            } else if self.introduced {
                items.get(selected).and_then(|i| i.shown.intro.as_deref()).map(|text| InfoPage { text: fill(text, settings), link: None, button: crate::i18n::tr("Continue"), icon: None, scrolled: self.scrolled, style: Some("intro".into()), wait: None, image: None, busy: false })
            } else { match (self.reading, items.get(selected).map(|i| &i.kind)) {
                (true, Some(Kind::Info { after: Some(text), .. })) if self.after => Some(InfoPage {
                    text: fill(text, settings), link: None, button: crate::i18n::tr("Continue"), icon: None, scrolled: self.scrolled, style: None, wait: None, image: None, busy: false }),
                (true, Some(Kind::Info { text, link, button, wait, .. })) => Some(InfoPage {
                    text: fill(text, settings), link: link.clone(), button: button.as_deref().map_or_else(|| crate::i18n::tr(&tree.back), crate::i18n::tr),
                    icon: items[selected].icon.clone().or_else(|| wait.and(parent.and_then(|p| p.icon.clone()))),
                    scrolled: self.scrolled, style: items[selected].style.clone(), wait: *wait, image: items[selected].shown.image.clone(),
                    busy: items[selected].shown.busy.as_deref().is_some_and(|k| is_on(settings, k)) }),
                _ => None,
            } },
        })
    }
}

/// One menu entry as drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub label: String,
    pub icon: Option<String>,
    /// The tile's own symbol, when it has one.
    pub tile: Option<String>,
    pub style: Option<String>,
    pub prompt: Option<String>,
    /// A range's value from its minimum (0) to its maximum (1).
    pub fraction: Option<f64>,
    /// "menu", "choice", "toggle", "range", "action" or "back".
    pub kind: &'static str,
    /// The value as text, for entries that have one.
    pub value: Option<String>,
    pub editing: bool,
    /// A choice's options, and which one is set.
    pub options: Vec<Choice>,
    pub saved: Option<usize>,
    /// A Set entry whose value is the setting.
    pub ticked: bool,
    /// A form's fields as its tile sums them up: short name and value.
    pub summary: Vec<(String, Option<f64>)>,
    /// An open section's lines: label and value.
    pub detail: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub title: String,
    pub path: Vec<String>,
    pub depth: usize,
    pub selected: usize,
    pub editing: bool,
    /// Editing a choice: the highlighted option, or its options' count for Back.
    pub pending: Option<usize>,
    /// The symbol, style and prompt of the entry this submenu opened from.
    pub icon: Option<String>,
    pub style: Option<String>,
    pub prompt: Option<String>,
    /// Lines under that prompt, set lighter.
    pub detail: Option<String>,
    /// Whether that entry opened on an intro page.
    pub intro: bool,
    /// A list's place on the wheel, 0 to 1, once turned (`Nav::list_at`).
    pub list_at: Option<f64>,
    /// The entry's picture, above its prompt, and its size when given.
    pub image: Option<String>,
    pub image_size: Option<(f32, f32)>,
    pub rows: Vec<Row>,
    /// The info page open.
    pub info: Option<InfoPage>,
    /// The form open.
    pub form: Option<FormView>,
    /// A confirm page: how far its ring is turned, 0 to 1.
    pub turned: f64,
    /// A carousel's position, in tiles from its first (0), as its physics
    /// moves it.
    pub scroll: Option<f64>,
    /// Detents turned in the menus, net, for skins that move a page with
    /// the ring itself, and the page they are counted on (a new value
    /// after each press).
    pub travel: i64,
    pub page: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FormView {
    pub prompt: Option<String>,
    pub icon: Option<String>,
    pub style: Option<String>,
    /// Each field's label, value (None for off) and unit (none for degrees).
    pub fields: Vec<(String, Option<f64>, Option<String>)>,
    /// Each field's value as its box says it, for fields of options (Pro
    /// Setup's); None for numbers.
    pub texts: Vec<Option<String>>,
    /// The buttons after the fields, by Button::name, in order.
    pub buttons: Vec<&'static str>,
    /// The selected row: a field, then the buttons.
    pub at: usize,
    /// The selected field is being changed.
    pub changing: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InfoPage {
    pub text: String,
    pub link: Option<String>,
    pub button: String,
    /// The entry's symbol, shown above the text on a top-menu page.
    pub icon: Option<String>,
    /// Lines scrolled, on a page too long for the face.
    pub scrolled: usize,
    /// The page's style, from its entry (such as "legal", "wait"), or
    /// "intro" for an entry's intro page.
    pub style: Option<String>,
    /// Seconds the page stays, without a button.
    pub wait: Option<f64>,
    /// A picture above the text.
    pub image: Option<String>,
    /// The activity arc runs round the page.
    pub busy: bool,
}

/// The entries a level shows: its items, plus Back when it names none.
fn entries(items: &[Item]) -> usize {
    items.len() + usize::from(!has_exit(items))
}

/// Whether a level has its own way out, so needs no Back added.
fn has_exit(items: &[Item]) -> bool {
    items.iter().any(|i| matches!(i.kind, Kind::Back | Kind::Up(_) | Kind::Done))
}

/// Where a submenu opens: its first entry that is not Back or Done.
fn first_entry(items: &[Item]) -> usize {
    items.iter().position(|i| !matches!(i.kind, Kind::Back | Kind::Up(_) | Kind::Done | Kind::Heading)).unwrap_or(0)
}

/// Answers, or picks (a list of answers, the one given first).
/// Stock's Your System page: its own CONTINUE and TEST, no Back.
fn is_system(parent: Option<&Item>) -> bool {
    parent.is_some_and(|p| p.style.as_deref() == Some("system"))
}

fn is_answers(parent: Option<&Item>) -> bool {
    parent.is_some_and(|p| matches!(p.style.as_deref(), Some("answers" | "picks" | "question" | "ask")))
}

/// A text with each `{key}` replaced by that setting, as text or a number.
pub fn fill(text: &str, settings: &Settings) -> String {
    let text = crate::i18n::tr(text);
    let mut out = String::new();
    let mut rest = text.as_str();
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else { break };
        out.push_str(&rest[..open]);
        let key = &rest[open + 1..open + close];
        match settings.get(key) {
            Some(Value::Text(t)) => out.push_str(t),
            Some(Value::Number(n)) => out.push_str(&n.to_string()),
            _ => {}
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

/// Where a level of answers opens: on the answer given, else its first
/// entry.
/// Where answers open: the one set, else the first, which may be Cancel
/// (stock's Reset pages offer CANCEL before RESET).
fn answer_index(items: &[Item], settings: &Settings) -> usize {
    let given = |i: &Item| match &i.kind {
        Kind::Set { key, value, .. } => settings.text(key) == Some(value.as_str()),
        // A pick that asks first: its page's answer.
        Kind::Menu(page) if i.style.as_deref() == Some("ask") => page.first().is_some_and(given_set(settings)),
        _ => false,
    };
    items.iter().position(given)
        .unwrap_or_else(|| if items.first().is_some_and(|i| i.kind == Kind::Back) { 0 } else { first_entry(items) })
}

fn given_set(settings: &Settings) -> impl Fn(&Item) -> bool + '_ {
    |i| matches!(&i.kind, Kind::Set { key, value, .. } if settings.text(key) == Some(value.as_str()))
}

/// Where a level's Back entry is.
fn back_index(items: &[Item]) -> usize {
    items.iter().position(|i| i.kind == Kind::Back).unwrap_or(items.len())
}

fn key(kind: &Kind) -> Option<&str> {
    match kind {
        Kind::Choice { key, .. } | Kind::Toggle { key } | Kind::Range { key, .. } => Some(key),
        _ => None,
    }
}

fn kind_name(kind: &Kind) -> &'static str {
    match kind {
        Kind::Menu(_) => "menu",
        Kind::Choice { .. } => "choice",
        Kind::Toggle { .. } => "toggle",
        Kind::Range { .. } => "range",
        Kind::Action { .. } | Kind::Ask(_) => "action",
        Kind::Info { .. } => "info",
        Kind::Heading => "heading",
        Kind::Form { .. } => "form",
        Kind::Set { .. } => "set",
        Kind::Back | Kind::Up(_) => "back",
        Kind::Section(_) => "section",
        Kind::Done => "done",
    }
}

fn option_index(options: &[Choice], value: &Value) -> Option<usize> {
    match value {
        Value::Text(v) => options.iter().position(|o| &o.value == v),
        _ => None,
    }
}

fn first(kind: &Kind) -> Option<Value> {
    match kind {
        Kind::Choice { options, .. } => options.first().map(|o| Value::Text(o.value.clone())),
        Kind::Range { min, .. } => Some(Value::Number(*min)),
        _ => None,
    }
}

/// The value `detents` wheel steps away, within the item's options or range.
fn step(item: &Item, value: &Value, detents: i32) -> Value {
    match (&item.kind, value) {
        (Kind::Choice { options, .. }, Value::Text(current)) => {
            let at = options.iter().position(|o| &o.value == current).unwrap_or(0) as i64;
            let to = (at + detents as i64).clamp(0, options.len() as i64 - 1) as usize;
            Value::Text(options[to].value.clone())
        }
        (Kind::Range { min, max, step, .. }, Value::Number(n)) => {
            let stepped = ((n - min) / step).round() + detents as f64;
            Value::Number((min + stepped * step).clamp(*min, *max))
        }
        _ => value.clone(),
    }
}

/// A value as the user reads it.
fn describe(item: &Item, value: &Value) -> String {
    if let Value::Text(v) = value {
        if let Some((_, s)) = item.shown.summaries.iter().find(|(k, _)| k == v) {
            return s.clone();
        }
    }
    match (&item.kind, value) {
        (Kind::Choice { options, .. }, Value::Text(v)) => {
            options.iter().find(|o| &o.value == v).map(|o| o.label.clone()).unwrap_or_else(|| v.clone())
        }
        (Kind::Range { unit, .. }, Value::Number(n)) => {
            let n = if n.fract() == 0.0 { format!("{n:.0}") } else { format!("{n}") };
            if unit.is_empty() { n } else { format!("{n}{unit}") }
        }
        (_, Value::Bool(true)) => "On".into(),
        (_, Value::Bool(false)) => "Off".into(),
        (_, Value::Number(n)) => n.to_string(),
        (_, Value::Text(t)) => t.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Tree {
        Tree {
            title: "Settings".into(),
            back: "Back".into(),
            values: Vec::new(),
            items: vec![
                Item { label: "Mode".into(), icon: None, style: None, prompt: None, shown: Shown::default(), kind: Kind::Choice { key: "mode".into(), options: vec![
                    Choice { value: "heat".into(), label: "Heat".into() },
                    Choice { value: "cool".into(), label: "Cool".into() },
                    Choice { value: "off".into(), label: "Off".into() },
                ] } },
                Item { label: "Display".into(), icon: None, style: None, prompt: None, shown: Shown::default(), kind: Kind::Menu(vec![
                    Item { label: "Brightness".into(), icon: None, style: None, prompt: None, shown: Shown::default(), kind: Kind::Range { key: "bright".into(), min: 10.0, max: 100.0, step: 10.0, unit: "%".into() } },
                    Item { label: "Clock".into(), icon: None, style: None, prompt: None, shown: Shown::default(), kind: Kind::Toggle { key: "clock".into() } },
                ]) },
                Item { label: "Restart".into(), icon: None, style: None, prompt: None, shown: Shown::default(), kind: Kind::Action { name: "restart".into() } },
            ],
        }
    }

    fn settings() -> Settings {
        let mut s = Settings::load(None);
        s.default("mode", Value::Text("heat".into()));
        s.default("bright", Value::Number(70.0));
        s.default("clock", Value::Bool(true));
        s
    }

    #[test]
    fn within_names_the_open_menus_not_the_highlighted_entry() {
        let (t, mut nav) = (tree(), Nav::default());
        nav.open_entry(&t, &["Display"]);
        assert!(nav.within(&t, "Display"));
        nav.open(&t);
        nav.wheel(&t, -2);
        assert!(!nav.within(&t, "Display"), "only highlighted");
        assert!(!Nav::default().within(&t, "Display"));
    }

    #[test]
    fn back_after_a_choices_options_leaves_it_unchanged() {
        let (t, mut s, mut nav) = (tree(), settings(), Nav::default());
        nav.open(&t);
        nav.wheel(&t, -3);
        nav.press(&t, &mut s);
        let view = nav.view(&t, &s).unwrap();
        assert_eq!((view.pending, view.rows[0].saved, view.rows[0].options.len()), (Some(0), Some(0), 3));
        nav.wheel(&t, 1);
        nav.wheel(&t, 5);
        assert_eq!(nav.view(&t, &s).unwrap().pending, Some(3), "past the options is Back");
        assert_eq!(nav.press(&t, &mut s), Outcome::Closed, "an edit from the top menu ends at home");
        assert_eq!(s.text("mode"), Some("heat"));
        assert!(nav.view(&t, &s).is_none());
    }

    #[test]
    fn done_goes_home_from_a_submenu_and_replaces_back() {
        let mut t = tree();
        let Kind::Menu(display) = &mut t.items[1].kind else { panic!("Display is a submenu") };
        display.insert(0, Item { label: String::new(), icon: None, style: None, prompt: None, shown: Shown::default(), kind: Kind::Done });
        let (mut s, mut nav) = (settings(), Nav::default());
        nav.open(&t);
        nav.wheel(&t, -2);
        nav.press(&t, &mut s);
        let view = nav.view(&t, &s).unwrap();
        assert_eq!((view.selected, view.rows.len()), (1, 3), "opens past Done, and gains no Back");
        assert_eq!(view.rows[0].label, "Done");
        // A carousel takes two steps to a tile.
        nav.wheel(&t, -1);
        assert_eq!(nav.view(&t, &s).unwrap().selected, 1, "half a tile");
        nav.wheel(&t, -1);
        assert_eq!(nav.press(&t, &mut s), Outcome::Closed);
        assert!(!nav.is_open());
    }

    #[test]
    fn the_top_menu_goes_round_and_lists_stop_at_their_ends() {
        let (t, s, mut nav) = (tree(), settings(), Nav::default());
        nav.open(&t);
        nav.wheel(&t, 1);
        assert_eq!(nav.view(&t, &s).unwrap().selected, 0, "past Back, round to the first entry");
        nav.wheel(&t, -2);
        assert_eq!(nav.view(&t, &s).unwrap().selected, 2);
        nav.wheel(&t, -1);
        nav.press(&t, &mut settings());
        nav.wheel(&t, -5);
        assert_eq!(nav.view(&t, &s).unwrap().selected, 0, "a submenu stops at its first entry");
    }

    #[test]
    fn choices_are_edited_with_the_wheel_and_committed_by_the_button() {
        let (t, mut s, mut nav) = (tree(), settings(), Nav::default());
        nav.open(&t);
        assert_eq!(nav.view(&t, &s).unwrap().selected, 3, "the top menu opens on Back");
        nav.wheel(&t, -3);
        assert_eq!(nav.press(&t, &mut s), Outcome::Stay);
        nav.wheel(&t, 1);
        let view = nav.view(&t, &s).unwrap();
        assert!(view.editing && view.rows[0].value.as_deref() == Some("Cool"));
        assert_eq!(s.text("mode"), Some("heat"), "not committed while editing");
        assert_eq!(nav.press(&t, &mut s), Outcome::Closed);
        assert_eq!(s.text("mode"), Some("cool"));
        assert!(nav.view(&t, &s).is_none());
    }

    #[test]
    fn submenus_open_edit_ranges_and_go_back() {
        let (t, mut s, mut nav) = (tree(), settings(), Nav::default());
        nav.open(&t);
        nav.wheel(&t, -2);
        nav.press(&t, &mut s);
        let view = nav.view(&t, &s).unwrap();
        assert_eq!((view.title.as_str(), view.depth, view.rows.len()), ("Display", 1, 3));
        assert_eq!(view.rows[0].value.as_deref(), Some("70%"));
        nav.press(&t, &mut s);
        nav.wheel(&t, 5);
        nav.press(&t, &mut s);
        assert_eq!(s.get("bright"), Some(&Value::Number(100.0)), "clamped to the range");
        nav.wheel(&t, 2);
        nav.press(&t, &mut s);
        assert_eq!(s.get("clock"), Some(&Value::Bool(false)));
        nav.wheel(&t, 10);
        assert_eq!(nav.view(&t, &s).unwrap().selected, 2, "stops at Back");
        assert_eq!(nav.press(&t, &mut s), Outcome::Closed, "Back from an entry of the top menu goes home");
        assert!(nav.view(&t, &s).is_none());
    }

    #[test]
    fn an_info_page_opens_and_back_returns_to_the_first_answer() {
        let mut t = Tree { values: Vec::new(), title: "Menu".into(), back: "Back".into(), items: vec![Item { label: "Eco".into(), icon: None, style: None, prompt: Some("Use Eco?".into()),
            shown: Shown::default(), kind: Kind::Menu(vec![
                Item { label: "Start".into(), icon: None, style: None, prompt: None, shown: Shown::default(), kind: Kind::Action { name: "start".into() } },
                Item { label: "More info".into(), icon: None, style: None, prompt: None, shown: Shown { busy: Some("saving".into()), ..Shown::default() },
                       kind: Kind::Info { text: "Save energy.".into(), link: Some("nest.com/eco".into()), button: None, summary: None, wait: None, run: None, home: false, after: None } }]) }] };
        let (mut s, mut nav) = (settings(), Nav::default());
        nav.open(&t);
        nav.wheel(&t, -1);
        nav.press(&t, &mut s);
        nav.wheel(&t, 2);
        nav.press(&t, &mut s);
        assert_eq!(nav.view(&t, &s).unwrap().info.map(|i| (i.text, i.link, i.button)),
                   Some(("Save energy.".into(), Some("nest.com/eco".into()), "Back".into())));
        assert!(!nav.view(&t, &s).unwrap().info.unwrap().busy);
        s.set("saving", Value::Text("on".into()));
        assert!(nav.view(&t, &s).unwrap().info.unwrap().busy, "the arc runs while its setting is on");
        nav.wheel(&t, 1);
        nav.press(&t, &mut s);
        let view = nav.view(&t, &s).unwrap();
        assert_eq!((view.info, view.selected), (None, 0));
        // Without a prompt, back returns to the entry itself.
        t.items[0].prompt = None;
        nav.wheel(&t, 2);
        nav.press(&t, &mut s);
        nav.press(&t, &mut s);
        assert_eq!(nav.view(&t, &s).unwrap().selected, 1);
    }

    #[test]
    fn an_intro_page_comes_before_the_answers_which_have_no_back() {
        let item = Item { label: "Reminders".into(), icon: None, style: Some("answers".into()), prompt: None,
            shown: Shown { intro: Some("Filters.".into()), summaries: vec![("off".into(), "None".into())], ..Shown::default() },
            kind: Kind::Choice { key: "remind".into(), options: vec![
                Choice { value: "on".into(), label: "Remind me".into() }, Choice { value: "off".into(), label: "Don't remind me".into() }] } };
        let t = Tree { values: Vec::new(), title: "Menu".into(), back: "Back".into(), items: vec![Item { label: "Settings".into(), icon: None, style: None,
            prompt: None, shown: Shown::default(), kind: Kind::Menu(vec![item]) }] };
        let (mut s, mut nav) = (settings(), Nav::default());
        s.set("remind", Value::Text("off".into()));
        nav.open(&t);
        nav.wheel(&t, -1);
        nav.press(&t, &mut s);
        assert_eq!(nav.view(&t, &s).unwrap().rows[0].value.as_deref(), Some("None"));
        nav.press(&t, &mut s);
        assert_eq!(nav.view(&t, &s).unwrap().info.map(|i| i.button), Some("Continue".into()));
        nav.press(&t, &mut s);
        nav.wheel(&t, 5);
        assert_eq!(nav.view(&t, &s).unwrap().pending, Some(1), "no Back past the answers");
        nav.wheel(&t, -1);
        nav.press(&t, &mut s);
        assert_eq!(s.text("remind"), Some("on"));
    }

    #[test]
    fn a_form_edits_its_fields_in_place_then_done_returns() {
        let field = |label: &str, key: &str, min, max, off| Field { label: label.into(), short: label[..3].into(), key: key.into(), min, max, step: 0.5, off, unit: None, options: Vec::new() };
        let item = Item { label: "Safety Temp".into(), icon: None, style: None, prompt: None, shown: Shown::default(),
            kind: Kind::Form { fields: vec![field("Heat to", "low", 4.5, 7.0, Some(Off::Below)), field("Cool to", "high", 24.0, 40.0, Some(Off::Above))],
                               more: Some("Info.".into()), link: None, more_first: false, enable: None, action: None } };
        let t = Tree { values: Vec::new(), title: "Menu".into(), back: "Back".into(), items: vec![Item { label: "Settings".into(), icon: None, style: None,
            prompt: None, shown: Shown::default(), kind: Kind::Menu(vec![item]) }] };
        let (mut s, mut nav) = (settings(), Nav::default());
        s.set("low", Value::Number(5.0));
        s.set("high", Value::Text("off".into()));
        nav.open(&t);
        nav.wheel(&t, -1);
        nav.press(&t, &mut s);
        assert_eq!(nav.view(&t, &s).unwrap().rows[0].summary, vec![("Hea".to_string(), Some(5.0)), ("Coo".to_string(), None)]);
        nav.press(&t, &mut s);
        nav.wheel(&t, 1);
        nav.press(&t, &mut s);
        let form = nav.view(&t, &s).unwrap().form.unwrap();
        assert_eq!((form.at, form.changing), (1, true), "changing Cool to");
        nav.wheel(&t, -2);
        assert_eq!(nav.view(&t, &s).unwrap().form.unwrap().fields[1].1, Some(39.5), "off lies past 40");
        nav.press(&t, &mut s);
        assert_eq!((s.get("high"), nav.view(&t, &s).unwrap().form.unwrap().at), (Some(&Value::Number(39.5)), 2), "set, on to Done");
        nav.press(&t, &mut s);
        assert!(nav.view(&t, &s).unwrap().form.is_none(), "Done goes back");
    }

    #[test]
    fn a_field_of_options_goes_round_them_and_keeps_the_value() {
        let options = vec![Choice { value: "a".into(), label: "Alpha".into() }, Choice { value: "b".into(), label: "Beta".into() }];
        let field = Field { label: "Kind".into(), short: String::new(), key: "kind".into(), min: 0.0, max: 1.0, step: 1.0, off: None, unit: None, options };
        let item = Item { label: "Setup".into(), icon: None, style: None, prompt: None, shown: Shown::default(),
            kind: Kind::Form { fields: vec![field], more: None, link: None, more_first: false, enable: None, action: None } };
        let t = Tree { values: Vec::new(), title: "Menu".into(), back: "Back".into(), items: vec![Item { label: "Settings".into(), icon: None, style: None,
            prompt: None, shown: Shown::default(), kind: Kind::Menu(vec![item]) }] };
        let (mut s, mut nav) = (settings(), Nav::default());
        s.set("kind", Value::Text("b".into()));
        nav.open(&t);
        nav.wheel(&t, -1);
        nav.press(&t, &mut s);
        nav.press(&t, &mut s);
        nav.press(&t, &mut s);
        assert_eq!(nav.view(&t, &s).and_then(|v| v.form).map(|f| f.texts), Some(vec![Some("Beta".into())]));
        nav.wheel(&t, 1);
        assert_eq!(nav.view(&t, &s).and_then(|v| v.form).map(|f| f.texts), Some(vec![Some("Alpha".into())]), "round to the first");
        nav.press(&t, &mut s);
        assert_eq!(s.text("kind"), Some("a"));
    }

    #[test]
    fn a_form_with_a_setting_to_enable_sets_it_and_backs_out_unchanged() {
        let field = |key: &str| Field { label: key.into(), short: String::new(), key: key.into(), min: 0.0, max: 23.0, step: 1.0,
                                        off: None, unit: Some("hour".into()), options: Vec::new() };
        let item = Item { label: "Fan Schedule".into(), icon: None, style: Some("schedule".into()), prompt: None, shown: Shown::default(),
            kind: Kind::Form { fields: vec![field("start")], more: None, link: None, more_first: false, enable: Some("on".into()), action: None } };
        let t = Tree { values: Vec::new(), title: "Menu".into(), back: "Back".into(), items: vec![Item { label: "Settings".into(), icon: None, style: None,
            prompt: None, shown: Shown::default(), kind: Kind::Menu(vec![item]) }] };
        let (mut s, mut nav) = (settings(), Nav::default());
        s.set("start", Value::Number(8.0));
        s.set("on", Value::Text("off".into()));
        let open = |nav: &mut Nav, s: &mut Settings| {
            nav.open(&t);
            nav.wheel(&t, -1);
            nav.press(&t, s);
            nav.press(&t, s);
        };
        open(&mut nav, &mut s);
        assert_eq!(nav.view(&t, &s).unwrap().form.unwrap().buttons, vec!["set", "back"]);
        nav.press(&t, &mut s);
        nav.wheel(&t, 2);
        nav.press(&t, &mut s);
        assert_eq!(s.get("start"), Some(&Value::Number(10.0)), "a field is set as it is changed");
        nav.wheel(&t, 1);
        nav.press(&t, &mut s);
        assert_eq!((s.get("start"), s.text("on")), (Some(&Value::Number(8.0)), Some("off")), "Back puts it back");
        open(&mut nav, &mut s);
        nav.wheel(&t, 1);
        nav.press(&t, &mut s);
        assert_eq!(s.text("on"), Some("on"), "Set");
        open(&mut nav, &mut s);
        assert_eq!(nav.view(&t, &s).unwrap().form.unwrap().buttons, vec!["done", "disable"]);
        nav.wheel(&t, 2);
        nav.press(&t, &mut s);
        assert_eq!(s.text("on"), Some("off"), "Disable");
    }

    #[test]
    fn a_chain_of_questions_opens_on_the_first_and_each_answer_leads_on() {
        let item = |label: &str, style: Option<&str>, kind| Item { label: label.into(), icon: None, style: style.map(String::from), prompt: None, shown: Shown::default(), kind };
        let set = |label: &str, key: &str, value: &str| item(label, None, Kind::Set { key: key.into(), value: value.into(), confirm: None });
        let q1 = item("Type", Some("picks"), Kind::Menu(vec![set("Single", "type", "family"), set("Multi", "type", "multi")]));
        let q2 = item("Count", Some("picks"), Kind::Menu(vec![set("One", "count", "1"), set("Two", "count", "2")]));
        let chain = item("Construction", Some("steps"), Kind::Menu(vec![q1, q2]));
        let t = Tree { values: Vec::new(), title: "Menu".into(), back: "Back".into(), items: vec![item("Settings", None, Kind::Menu(vec![
            item("Location", Some("titled"), Kind::Menu(vec![chain, Item { label: "Done".into(), icon: None, style: None, prompt: None, shown: Shown::default(), kind: Kind::Done }]))]))] };
        let (mut s, mut nav) = (settings(), Nav::default());
        s.set("type", Value::Text("multi".into()));
        s.set("count", Value::Text("1".into()));
        nav.open(&t);
        nav.wheel(&t, -1);
        for _ in 0..3 {
            nav.press(&t, &mut s);
        }
        let v = nav.view(&t, &s).unwrap();
        assert_eq!((v.title.as_str(), v.selected, v.rows.len()), ("Type", 1, 2), "the first question, on its answer, without Back");
        nav.press(&t, &mut s);
        let v = nav.view(&t, &s).unwrap();
        assert_eq!((v.title.as_str(), v.selected, v.rows.len()), ("Count", 0, 2), "the next question");
        nav.wheel(&t, 1);
        nav.press(&t, &mut s);
        assert_eq!((s.text("count"), nav.view(&t, &s).unwrap().title.as_str()), (Some("2"), "Settings"), "the last answer leaves the chain");
    }

    #[test]
    fn a_confirm_page_keeps_cancel_and_acts_once_the_ring_is_turned_full() {
        let item = |label: &str, kind| Item { label: label.into(), icon: None, style: None, prompt: None, shown: Shown::default(), kind };
        let page = Item { style: Some("confirm".into()), ..item("Restart", Kind::Menu(vec![
            Item { label: "Cancel".into(), ..item("", Kind::Back) }, item("Restart", Kind::Action { name: "restart".into() })])) };
        let t = Tree { values: Vec::new(), title: "Menu".into(), back: "Back".into(), items: vec![item("Reset", Kind::Menu(vec![page]))] };
        let (mut s, mut nav) = (settings(), Nav::default());
        let open_page = |nav: &mut Nav, s: &mut Settings| {
            nav.open(&t);
            nav.wheel(&t, 1);
            nav.press(&t, s);
            nav.press(&t, s);
        };
        open_page(&mut nav, &mut s);
        assert_eq!(nav.view(&t, &s).unwrap().selected, 0, "Cancel first");
        nav.wheel(&t, 4);
        assert_eq!(nav.confirmed(&t), None);
        let view = nav.view(&t, &s).unwrap();
        assert!((view.turned - 4.0 / f64::from(CONFIRM_TURNS)).abs() < 1e-9 && view.selected == 0, "turning fills, never moves");
        nav.wheel(&t, -9);
        assert_eq!(nav.view(&t, &s).unwrap().turned, 0.0);
        nav.wheel(&t, CONFIRM_TURNS + 3);
        assert_eq!(nav.confirmed(&t).as_deref(), Some("restart"));
        assert!(!nav.is_open());
        open_page(&mut nav, &mut s);
        assert_eq!(nav.press(&t, &mut s), Outcome::Stay, "Cancel goes back");
        assert_eq!(nav.view(&t, &s).unwrap().title, "Reset");
    }

    #[test]
    fn back_on_picks_from_a_page_selects_the_first_entry() {
        let item = |label: &str, style: Option<&str>, kind| Item { label: label.into(), icon: None, style: style.map(str::to_string), prompt: None, shown: Shown::default(), kind };
        let page = |label: &str| item(label, Some("answers"), Kind::Menu(vec![item("Cancel", None, Kind::Back), item("Reset", None, Kind::Action { name: "x".into() })]));
        let reset = item("Reset", Some("picks"), Kind::Menu(vec![page("Restart"), page("Away"), page("All"), item("Cancel", None, Kind::Back)]));
        let t = Tree { values: Vec::new(), title: "Menu".into(), back: "Back".into(), items: vec![item("Settings", None, Kind::Menu(vec![reset]))] };
        let (mut s, mut nav) = (settings(), Nav::default());
        assert!(nav.open_at(&t, &["Settings", "Reset"], Some(2)));
        nav.press(&t, &mut s);
        assert_eq!(nav.view(&t, &s).unwrap().title, "All");
        nav.press(&t, &mut s);
        let v = nav.view(&t, &s).unwrap();
        assert_eq!((v.title.as_str(), v.selected), ("Reset", 0), "back on Restart");
    }

    #[test]
    fn a_pick_that_asks_first_opens_on_the_given_and_leaves_its_list_once_confirmed() {
        let item = |label: &str, style: Option<&str>, kind| Item { label: label.into(), icon: None, style: style.map(str::to_string), prompt: None, shown: Shown::default(), kind };
        let set = |label: &str, value: &str| item(label, None, Kind::Set { key: "lang".into(), value: value.into(), confirm: None });
        let ask = |label: &str, value: &str| item(label, Some("ask"), Kind::Menu(vec![set(label, value), item("Cancel", None, Kind::Back)]));
        let list = Item { shown: Shown { ordered: true, ..Shown::default() }, ..item("Language", Some("picks"), Kind::Menu(vec![ask("DE", "de"), ask("EN", "en"), ask("FR", "fr")])) };
        let t = Tree { values: Vec::new(), title: "Menu".into(), back: "Back".into(), items: vec![item("Settings", None, Kind::Menu(vec![list]))] };
        let mut s = settings();
        s.set("lang", Value::Text("en".into()));
        let mut nav = Nav::default();
        nav.open(&t);
        nav.wheel(&t, 1);
        nav.press(&t, &mut s);
        nav.press(&t, &mut s);
        assert_eq!((nav.view(&t, &s).unwrap().title.as_str(), nav.view(&t, &s).unwrap().selected), ("Language", 1), "opens on EN");
        nav.wheel(&t, 1);
        nav.press(&t, &mut s);
        let view = nav.view(&t, &s).unwrap();
        assert_eq!((view.style.as_deref(), view.selected, view.rows[1].icon.as_deref()), (Some("ask"), 0, Some("back")));
        nav.wheel(&t, 1);
        assert_eq!(nav.press(&t, &mut s), Outcome::Stay, "Cancel");
        assert_eq!(nav.view(&t, &s).unwrap().title, "Language");
        nav.press(&t, &mut s);
        nav.press(&t, &mut s);
        assert_eq!(s.text("lang"), Some("fr"));
        assert_eq!(nav.view(&t, &s).unwrap().title, "Settings", "back at the tile");
    }

    #[test]
    fn the_menus_open_at_a_submenu_and_entry_by_labels() {
        let item = |label: &str, kind| Item { label: label.into(), icon: None, style: None, prompt: None, shown: Shown::default(), kind };
        let info = || Kind::Info { run: None, text: "Info.".into(), link: None, button: None, summary: None, wait: None, home: false, after: None };
        let t = Tree { values: Vec::new(), title: "Menu".into(), back: "Back".into(), items: vec![item("Settings", Kind::Menu(vec![
            item("A", info()), item("B", info()), item("Sense", Kind::Menu(vec![item("C", info()), item("D", info())]))]))] };
        let (s, mut nav) = (settings(), Nav::default());
        assert!(nav.open_at(&t, &["Settings"], Some(1)));
        assert_eq!(nav.view(&t, &s).unwrap().rows[1].label, "B");
        assert_eq!(nav.view(&t, &s).unwrap().selected, 1);
        assert!(nav.open_at(&t, &["Settings", "Sense"], None));
        assert_eq!(nav.view(&t, &s).unwrap().rows[0].label, "C");
        assert!(!nav.open_at(&t, &["Settings", "A"], None), "an entry, not a submenu");
        assert!(!nav.is_open());
    }

    #[test]
    fn a_lists_rows_take_three_detents_and_it_goes_a_detent_past_its_ends() {
        let item = |label: &str, kind| Item { label: label.into(), icon: None, style: None, prompt: None, shown: Shown::default(), kind };
        let set = |label: &str, value: &str| item(label, Kind::Set { key: "dry".into(), value: value.into(), confirm: None });
        let page = Item { style: Some("answers".into()), ..item("Cool to Dry", Kind::Menu(vec![set("Yes", "on"), set("No", "off"), set("Maybe", "maybe")])) };
        let t = Tree { values: Vec::new(), title: "Menu".into(), back: "Back".into(), items: vec![item("Settings", Kind::Menu(vec![page]))] };
        let (mut s, mut nav) = (settings(), Nav::default());
        s.set("dry", Value::Text("on".into()));
        nav.open(&t);
        nav.wheel(&t, -1);
        nav.press(&t, &mut s);
        nav.press(&t, &mut s);
        let now = std::time::Instant::now();
        // Clockwise detents, one at a time: the gen 3's counts.
        let turn = |nav: &mut Nav, detents: i32| {
            for _ in 0..detents.abs() {
                nav.rotary(&t, -120 * detents.signum(), now);
            }
            nav.view(&t, &s).unwrap().selected
        };
        assert_eq!(turn(&mut nav, 1), 0, "a detent stays");
        assert_eq!(turn(&mut nav, 1), 1, "two reach the next row's half");
        assert_eq!(turn(&mut nav, 3), 2, "three more the next");
        assert_eq!(turn(&mut nav, 10), 2, "the last row");
        assert_eq!(turn(&mut nav, -2), 2, "held a detent past the end");
        assert_eq!(turn(&mut nav, -1), 1, "then back past half a row");
        assert_eq!(nav.view(&t, &s).unwrap().list_at.map(|a| (a * 7.0).round()), Some(4.0), "at 4 of 7 detents");
    }

    #[test]
    fn a_list_of_sections_holds_its_first_row_longer_and_ends_on_done() {
        let item = |label: &str, kind| Item { label: label.into(), icon: None, style: None, prompt: None, shown: Shown::default(), kind };
        let section = |label: &str| item(label, Kind::Section(vec![("Key".into(), "value".into())]));
        let info = Item { style: Some("titled".into()), ..item("Info", Kind::Menu(vec![section("A"), section("B"), section("C"), item("Done", Kind::Done)])) };
        let t = Tree { values: Vec::new(), title: "Menu".into(), back: "Back".into(), items: vec![item("Settings", Kind::Menu(vec![info]))] };
        let (s, mut nav) = (settings(), Nav::default());
        assert!(nav.open_at(&t, &["Settings", "Info"], None));
        let now = std::time::Instant::now();
        let turn = |nav: &mut Nav, detents: i32| {
            for _ in 0..detents.abs() {
                nav.rotary(&t, -120 * detents.signum(), now);
            }
            nav.view(&t, &s).unwrap().selected
        };
        assert_eq!(turn(&mut nav, 4), 0, "the first row for four detents");
        assert_eq!(turn(&mut nav, 1), 1, "the next on the fifth");
        assert_eq!(turn(&mut nav, 5), 2, "three apart after");
        assert_eq!(turn(&mut nav, 2), 2, "Done seven past the row before");
        assert_eq!(turn(&mut nav, 1), 3, "and taken halfway there");
        assert_eq!(turn(&mut nav, 5), 3);
        assert_eq!(nav.view(&t, &s).unwrap().list_at, Some(1.0), "ending on Done");
        assert_eq!(turn(&mut nav, -1), 3, "no detent past it");
        assert_eq!(turn(&mut nav, -3), 2);
    }

    #[test]
    fn answers_open_on_the_one_given_and_close_once_one_is() {
        let item = |label: &str, kind| Item { label: label.into(), icon: None, style: None, prompt: None, shown: Shown::default(), kind };
        let set = |label: &str, value: &str| item(label, Kind::Set { key: "dry".into(), value: value.into(), confirm: None });
        let form = Kind::Form { fields: vec![Field { label: "Up to".into(), short: String::new(), key: "hours".into(), min: 0.0, max: 5.0,
                                                     step: 1.0, off: None, unit: Some("hours".into()), options: Vec::new() }],
                                more: Some("Info.".into()), link: None, more_first: true, enable: None, action: None };
        let page = Item { style: Some("answers".into()), ..item("Cool to Dry", Kind::Menu(vec![set("Yes", "on"), set("No", "off"),
            item("Max duration", form), item("More info", Kind::Info { run: None, text: "Info.".into(), link: None, button: None, summary: None, wait: None, home: false, after: None })])) };
        let t = Tree { values: Vec::new(), title: "Menu".into(), back: "Back".into(), items: vec![item("Settings", Kind::Menu(vec![item("Nest Sense", Kind::Menu(vec![page]))]))] };
        let (mut s, mut nav) = (settings(), Nav::default());
        s.set("dry", Value::Text("off".into()));
        nav.open(&t);
        nav.wheel(&t, -1);
        for _ in 0..3 {
            nav.press(&t, &mut s);
        }
        let v = nav.view(&t, &s).unwrap();
        assert_eq!((v.selected, v.rows.len()), (1, 4), "on the answer given, no Back added");
        nav.wheel(&t, 5);
        assert_eq!(nav.view(&t, &s).unwrap().selected, 3, "More info ends the answers");
        nav.press(&t, &mut s);
        nav.press(&t, &mut s);
        assert_eq!(nav.view(&t, &s).unwrap().selected, 1, "back from More info to the answer given");
        nav.wheel(&t, 1);
        nav.press(&t, &mut s);
        assert_eq!(nav.view(&t, &s).unwrap().form.unwrap().at, 0);
        nav.wheel(&t, 1);
        nav.press(&t, &mut s);
        nav.press(&t, &mut s);
        assert_eq!(nav.view(&t, &s).unwrap().form.unwrap().at, 1, "back from the form's More info to More info");
        nav.wheel(&t, 1);
        nav.press(&t, &mut s);
        assert_eq!(nav.view(&t, &s).unwrap().title, "Nest Sense", "the form's Done leaves the answers");
        nav.press(&t, &mut s);
        nav.wheel(&t, -1);
        nav.press(&t, &mut s);
        let v = nav.view(&t, &s).unwrap();
        assert_eq!((s.text("dry"), v.title.as_str()), (Some("on"), "Nest Sense"), "an answer is given and leaves");
    }

    #[test]
    fn back_from_the_top_closes_and_actions_are_reported() {
        let (t, mut s, mut nav) = (tree(), settings(), Nav::default());
        nav.open(&t);
        nav.wheel(&t, -1);
        assert_eq!(nav.press(&t, &mut s), Outcome::Action("restart".into()));
        nav.wheel(&t, 1);
        assert_eq!(nav.press(&t, &mut s), Outcome::Closed);
        assert!(!nav.is_open());
    }

    #[test]
    fn a_back_entry_placed_in_the_menus_is_used_instead_of_one_at_the_end() {
        let mut t = tree();
        t.items.insert(1, Item { label: String::new(), icon: None, style: None, prompt: None, shown: Shown::default(), kind: Kind::Back });
        let (mut s, mut nav) = (settings(), Nav::default());
        nav.open(&t);
        let view = nav.view(&t, &s).unwrap();
        assert_eq!((view.rows.len(), view.selected, view.rows[1].label.as_str()), (4, 1, "Back"));
        nav.wheel(&t, 10);
        assert_eq!(nav.view(&t, &s).unwrap().selected, 3, "no extra Back after the last item");
        nav.wheel(&t, -2);
        assert_eq!(nav.press(&t, &mut s), Outcome::Closed);
    }
}
