//! The week's schedule and stock's Schedule screen: a timeline of
//! setpoints from Monday 00:00 under a cursor at its quarter of an hour,
//! the wheel flinging it as stock's momentum scroller (0x1a3558 state 7),
//! with the home dial before Monday and after Sunday to leave by. Pressing
//! on an empty place offers NEW / CANCEL / DONE, on a setpoint CHANGE /
//! REMOVE / CANCEL; a new or changed setpoint takes its time, then its
//! temperature, a press each.

use std::time::Instant;

use serde_json::{json, Map, Value as Json};

use crate::scroll::{Scroll, SCHEDULE};

/// Minutes in a day and in the week.
pub const DAY: u32 = 24 * 60;
pub const WEEK: u32 = 7 * DAY;
/// The cursor's step, in minutes.
const STEP: i32 = 15;
/// A wheel detent's counts (the gen 3's).
const DETENT: f32 = 120.0;
/// Minutes a detent moves a setpoint's time, and degrees of the scale
/// shown it moves a setpoint's temperature (stock's, matched on nest-qemu
/// in Fahrenheit; stock's 0x1a3558 state 2 moves Celsius as far).
const TIME: f32 = 22.5;
const TEMP: f32 = 1.25;
/// Detents to a row of the menu over the timeline.
const MENU_ROW: f32 = 3.0;
/// The home dial sits this long before Monday on the timeline, and as
/// long after Sunday's end.
pub const DIAL: i32 = -8 * 60;
/// The timeline's strip: 24 pixels an hour, Monday 00:00 at 192 and the
/// week's end at 4224, the dials at 0 and 4416 (stock's 0x19fe50,
/// 0x1a4644); its 24 items are 192 pixels apart.
const PX_PER_MIN: f32 = 0.4;
const MONDAY_X: f32 = 192.0;
const ITEMS: usize = 24;
/// A setpoint closer than this to the timeline's time is the one under
/// the cursor, in minutes (stock's draw, 0x1a2c10: under 4501 s).
const HOLD: f32 = 75.0;
/// A schedule's temperatures, °C and °F (stock's 0x1a4644).
const LIMITS_C: (f32, f32) = (9.0, 32.0);
const LIMITS_F: (f32, f32) = (50.0, 90.0);
/// A new setpoint's slide to the temperature in force, in seconds.
const SLIDE: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Setpoint {
    /// Minutes from Monday 00:00.
    pub minute: u32,
    /// The temperature, or a range's low.
    pub temp: f32,
    /// A range's high (a RANGE setpoint, for heat-cool).
    pub high: Option<f32>,
}

/// The week's setpoints, in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Schedule {
    pub points: Vec<Setpoint>,
}

impl Schedule {
    /// The temperature in force at `minute`: the last setpoint before it,
    /// wrapping round the week.
    pub fn at(&self, minute: u32) -> Option<f32> {
        self.points.iter().rev().find(|p| p.minute <= minute).or(self.points.last()).map(|p| p.temp)
    }

    /// The setpoint in force at `minute`, as `at`.
    pub fn point_at(&self, minute: u32) -> Option<Setpoint> {
        self.points.iter().rev().find(|p| p.minute <= minute).or(self.points.last()).copied()
    }

    /// The temperatures at each midnight's triangle, Monday's to the
    /// week's end, as stock's draw (0x1a2624): none on an empty schedule,
    /// nor with a setpoint in the 6 hours before that midnight (not round
    /// the week) or in the hour from it; the week's end has one only with
    /// another drawn, and none with a setpoint from 6 hours before it
    /// (stock looks from its last second).
    pub fn day_starts(&self) -> Vec<Option<f32>> {
        let near = |midnight: u32| self.points.iter().any(|p| {
            (p.minute < midnight && midnight - p.minute < 6 * 60) || (p.minute >= midnight && p.minute - midnight < 60)
        });
        let mut starts: Vec<Option<f32>> = (0..7).map(|d| self.at(d * DAY).filter(|_| !near(d * DAY))).collect();
        let late = self.points.last().is_some_and(|p| WEEK - p.minute <= 6 * 60);
        let end = starts.iter().any(Option::is_some).then(|| self.at(WEEK - 1)).flatten().filter(|_| !late);
        starts.push(end);
        starts
    }

    pub fn find(&self, minute: u32) -> Option<Setpoint> {
        self.points.iter().find(|p| p.minute == minute).copied()
    }

    /// Adds a setpoint, replacing any at the same minute.
    pub fn set(&mut self, point: Setpoint) {
        self.remove(point.minute);
        let i = self.points.iter().position(|p| p.minute > point.minute).unwrap_or(self.points.len());
        self.points.insert(i, point);
    }

    pub fn remove(&mut self, minute: u32) {
        self.points.retain(|p| p.minute != minute);
    }

    /// From a schedule bucket's `days`: day numbers from 0 (Monday) to
    /// entries with a `time` in seconds and a `temp`, or a RANGE's
    /// `temp-min` and `temp-max`; continuation entries are the carry-over
    /// from the day before, not setpoints.
    pub fn from_days(days: &Json) -> Schedule {
        let mut schedule = Schedule::default();
        for (day, entries) in days.as_object().into_iter().flat_map(|d| d.iter()) {
            let Some(day) = day.parse::<u32>().ok().filter(|d| *d < 7) else { continue };
            for e in entries.as_object().into_iter().flat_map(|e| e.values()) {
                if e["entry_type"].as_str() == Some("continuation") {
                    continue;
                }
                let range = e["type"].as_str() == Some("RANGE");
                let temp = if range { e["temp-min"].as_f64() } else { e["temp"].as_f64() };
                let high = if range { e["temp-max"].as_f64().map(|h| h as f32) } else { None };
                let (Some(time), Some(temp)) = (e["time"].as_f64(), temp) else { continue };
                if range && high.is_none() {
                    continue;
                }
                let minute = (time / 60.0).round() as u32;
                if minute < DAY {
                    schedule.set(Setpoint { minute: day * DAY + minute, temp: temp as f32, high });
                }
            }
        }
        schedule
    }

    /// As a schedule bucket's `days`, with each day's continuation entry
    /// as stock writes them; `kind` is the setpoints' type (HEAT, COOL,
    /// RANGE).
    pub fn to_days(&self, kind: &str, now: i64) -> Json {
        let round = |t: f32| (f64::from(t) * 1000.0).round() / 1000.0;
        let entry = |p: Setpoint, kind: &str, entry_type: &str, by: i64| {
            let mut e = json!({
                "entry_type": entry_type, "time": (p.minute % DAY) * 60,
                "touched_at": now, "touched_by": by, "touched_tzo": 0, "type": kind,
            });
            match p.high.filter(|_| kind == "RANGE") {
                Some(high) => {
                    e["temp-min"] = json!(round(p.temp));
                    e["temp-max"] = json!(round(high));
                }
                None => e["temp"] = json!(round(p.temp)),
            }
            e
        };
        let mut days = Map::new();
        for day in 0..7 {
            let mut entries = Map::new();
            let mut n = 0;
            if let Some(p) = self.point_at(day * DAY) {
                entries.insert("0".into(), entry(Setpoint { minute: 0, ..p }, kind, "continuation", 1));
                n = 1;
            }
            for p in self.points.iter().filter(|p| p.minute / DAY == day) {
                entries.insert(n.to_string(), entry(*p, kind, "setpoint", 2));
                n += 1;
            }
            days.insert(day.to_string(), Json::Object(entries));
        }
        Json::Object(days)
    }
}

/// What the Schedule screen is doing.
#[derive(Clone, Debug, PartialEq)]
pub enum Doing {
    Browse,
    /// A menu over the timeline: its choices and the highlighted one.
    Menu { choices: &'static [&'static str], at: usize },
    /// Setting a new or changed setpoint's time (its temperature
    /// follows), then its temperature.
    Time(Setpoint),
    Temp(Setpoint),
    /// A range setpoint's high end, after its low one.
    High(Setpoint),
}

const EMPTY: &[&str] = &["New", "Cancel", "Done"];
const ON_POINT: &[&str] = &["Change", "Remove", "Cancel"];

/// The screen while it is open.
#[derive(Clone, Debug, PartialEq)]
pub struct Editor {
    /// Minutes from Monday 00:00, the quarter hour the cursor is on; DIAL
    /// on either dial.
    pub cursor: i32,
    /// The setpoint under the cursor, by its minute.
    pub held: Option<u32>,
    pub doing: Doing,
    /// Where the timeline sits under the cursor, in minutes, as the wheel
    /// has flung it.
    pub scroll: f32,
    /// The wheel's place within a time, temperature or menu row, in
    /// minutes, degrees or detents.
    part: f32,
    strip: Scroll,
    /// Clicks due, as stock's 0x1a3558 sounds them: browsing, each hour
    /// crossed (and each item of the strip off the week); a time, each
    /// quarter; a menu, each row; a temperature, each degree past the last
    /// click.
    pub clicks: u32,
    /// The strip's item when last looked at off the week.
    item: usize,
    /// Degrees shown the wheel has moved a temperature since its click.
    drift: f32,
    /// The scroll in from the dial to now as the screen opens (stock's
    /// 0x1a141c state 4): when it began, its length in seconds and where
    /// it ends on the strip.
    opening: Option<(Instant, f32, f32)>,
    /// A new setpoint's temperatures sliding from the scale's ends to
    /// those in force while its time is set (stock's state 0): when it
    /// began, and where from.
    slide: Option<(Instant, f32, Option<f32>)>,
    /// The minutes a setpoint's time keeps to while it is set: an hour
    /// from its neighbours, round the week (stock's 0x1a101c).
    bounds: (i32, i32),
}

/// What a press did.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    Changed,
    Closed,
}

/// The strip's pixel at a week minute, and back.
fn x_of(minute: f32) -> f32 {
    MONDAY_X + minute * PX_PER_MIN
}

fn minute_of(x: f32) -> f32 {
    (x - MONDAY_X) / PX_PER_MIN
}

/// A temperature `detents` steps of the scale shown on, within a
/// schedule's limits.
fn stepped(celsius: f32, detents: i32, fahrenheit: bool) -> f32 {
    if fahrenheit {
        let f = (crate::model::to_fahrenheit(celsius).round() + detents as f32).clamp(LIMITS_F.0, LIMITS_F.1);
        (f - 32.0) * 5.0 / 9.0
    } else {
        ((celsius * 2.0).round() / 2.0 + detents as f32 * 0.5).clamp(LIMITS_C.0, LIMITS_C.1)
    }
}

/// A new setpoint's temperature where none is in force (stock's 0x1a141c
/// state 0): the top of the scale, a range from the bottom to the top,
/// in °C.
pub fn unset(fahrenheit: bool, range: bool) -> (f32, Option<f32>) {
    let c = |f: f32| (f - 32.0) * 5.0 / 9.0;
    let (low, high) = if fahrenheit { (c(LIMITS_F.0), c(LIMITS_F.1)) } else { LIMITS_C };
    if range { (low, Some(high)) } else { (high, None) }
}

impl Editor {
    /// Opens on the current quarter of an hour.
    pub fn open(now: u32, schedule: &Schedule) -> Editor {
        let mut strip = Scroll::new(SCHEDULE, ITEMS, 0);
        strip.place(x_of(now as f32));
        strip.set_snap(false);
        let item = strip.index();
        let mut e = Editor { cursor: DIAL, held: None, doing: Doing::Browse, scroll: now as f32, part: 0.0, strip, clicks: 0, item, drift: 0.0,
                             opening: None, slide: None, bounds: (0, WEEK as i32 - STEP) };
        e.follow(schedule);
        e
    }

    /// Starts the timeline at the dial and scrolls it in to where it
    /// opened, eased sin², as stock's opening (0x1a141c state 4: 150 ms
    /// each 100 pixels, and 500/7 ms each 100 short of 1000).
    pub fn scroll_in(&mut self, at: Instant, schedule: &Schedule) {
        let to = self.strip.x();
        let d = to / 100.0;
        let seconds = (d * 150.0 + ((10.0 - d) * 500.0 / 7.0).max(0.0)) / 1000.0;
        self.opening = Some((at, seconds, to));
        let clicks = self.clicks;
        self.place_x(0.0, schedule);
        self.clicks = clicks;
    }

    /// The setpoint being made or changed as it shows at `now`: a new
    /// one's temperatures slide in over 500 ms, easing out (stock's curve
    /// 2), while its time is set.
    pub fn pending(&self, now: Instant) -> Option<Setpoint> {
        match &self.doing {
            Doing::Time(p) => {
                let Some((start, from, from_high)) = self.slide else { return Some(*p) };
                let e = (std::f32::consts::FRAC_PI_2 * (now.duration_since(start).as_secs_f32() / SLIDE).min(1.0)).sin();
                let mix = |a: f32, b: f32| a + (b - a) * e;
                Some(Setpoint { temp: mix(from, p.temp), high: p.high.map(|h| mix(from_high.unwrap_or(h), h)), ..*p })
            }
            Doing::Temp(p) | Doing::High(p) => Some(*p),
            _ => None,
        }
    }

    /// Where a setpoint at `minute` may be set to, the schedule without it.
    fn bounds_of(minute: u32, schedule: &Schedule) -> (i32, i32) {
        let prev = schedule.points.iter().rev().find(|p| p.minute < minute);
        let next = schedule.points.iter().find(|p| p.minute > minute);
        let low = match (prev, schedule.points.last()) {
            (Some(p), _) => p.minute as i32 + 60,
            (None, Some(last)) => last.minute as i32 - WEEK as i32 + 60,
            (None, None) => 0,
        };
        let high = match (next, schedule.points.first()) {
            (Some(n), _) => n.minute as i32 - 60,
            (None, Some(first)) => first.minute as i32 + WEEK as i32 - 60,
            (None, None) => WEEK as i32,
        };
        // Crowded, it keeps to the later bound, as stock clamps it last.
        let high = high.clamp(0, WEEK as i32 - STEP);
        (low.clamp(0, high), high)
    }

    /// Starts setting `point`'s time, kept to its bounds.
    fn set_time(&mut self, mut point: Setpoint, schedule: &Schedule) {
        self.bounds = Self::bounds_of(point.minute, schedule);
        point.minute = (point.minute as i32).max(self.bounds.0).min(self.bounds.1) as u32;
        self.doing = Doing::Time(point);
        self.cursor = point.minute as i32;
        self.part = point.minute as f32;
    }

    /// The cursor, the setpoint under it and the strip's spring, from
    /// where the strip is.
    fn follow(&mut self, schedule: &Schedule) {
        let x = self.strip.x();
        self.scroll = minute_of(x);
        if (MONDAY_X..=x_of(WEEK as f32)).contains(&x) {
            let was = self.cursor;
            self.cursor = quarter(self.scroll).min(WEEK as i32 - STEP);
            if was >= 0 && was as u32 % DAY / 60 != self.cursor as u32 % DAY / 60 {
                self.clicks += 1;
            }
            self.strip.set_snap(false);
        } else {
            self.cursor = DIAL;
            self.strip.set_snap(true);
            let item = self.strip.index();
            if item != self.item {
                self.item = item;
                self.clicks += 1;
            }
        }
        self.held = None;
        if self.cursor >= 0 {
            self.held = schedule.points.iter().map(|p| p.minute).filter(|&m| (m as f32 - self.scroll).abs() < HOLD)
                .min_by(|a, b| (*a as f32 - self.scroll).abs().total_cmp(&(*b as f32 - self.scroll).abs()));
        }
    }

    /// Puts the timeline at `minute`, at rest.
    fn place(&mut self, minute: u32, schedule: &Schedule) {
        self.place_x(x_of(minute as f32), schedule);
    }

    fn place_x(&mut self, x: f32, schedule: &Schedule) {
        self.strip.place(x);
        self.item = self.strip.index();
        self.follow(schedule);
    }

    /// Moves the opening scroll, the flung timeline or a new setpoint's
    /// slide on to `now`; whether anything still moves.
    pub fn tick(&mut self, now: Instant, schedule: &Schedule) -> bool {
        if let Some((start, seconds, to)) = self.opening {
            let t = now.duration_since(start).as_secs_f32() / seconds;
            // The scroll in is no browsing: it does not click.
            let clicks = self.clicks;
            if t >= 1.0 {
                self.opening = None;
                self.place_x(to, schedule);
            } else {
                self.place_x(to * (std::f32::consts::FRAC_PI_2 * t).sin().powi(2), schedule);
            }
            self.clicks = clicks;
            return self.opening.is_some();
        }
        if let Some((start, ..)) = self.slide {
            if now.duration_since(start).as_secs_f32() >= SLIDE || !matches!(self.doing, Doing::Time(_)) {
                self.slide = None;
            }
            return self.slide.is_some();
        }
        if self.doing != Doing::Browse {
            return false;
        }
        let moving = self.strip.step(now);
        self.follow(schedule);
        moving
    }

    /// Ends the opening scroll where it is, as the wheel comes.
    fn take_over(&mut self) {
        self.opening = None;
    }

    /// A wheel event's counts (the gen 3's, negative clockwise), flinging
    /// the timeline or moving a setpoint or the menu as stock's do.
    pub fn counts(&mut self, counts: i32, fahrenheit: bool, schedule: &Schedule, now: Instant) {
        self.take_over();
        let detents = -(counts as f32) / DETENT;
        match &mut self.doing {
            Doing::Browse => {
                self.strip.wheel(counts, now);
                self.follow(schedule);
            }
            Doing::Menu { choices, at } => {
                let was = *at;
                let pos = (*at as f32 * MENU_ROW + self.part + detents).clamp(-1.0, (choices.len() - 1) as f32 * MENU_ROW + 1.0);
                *at = ((pos / MENU_ROW).round().max(0.0) as usize).min(choices.len() - 1);
                self.part = pos - *at as f32 * MENU_ROW;
                self.clicks += u32::from(*at != was);
            }
            Doing::Time(p) => {
                let was = p.minute;
                let (low, high) = self.bounds;
                self.part = (self.part + detents * TIME).clamp(low as f32, high as f32);
                p.minute = quarter(self.part).clamp(low, high) as u32;
                self.cursor = p.minute as i32;
                self.clicks += u32::from(p.minute != was);
            }
            Doing::Temp(_) | Doing::High(_) => {
                // In the scale's steps: a degree Fahrenheit, half a Celsius.
                let step = if fahrenheit { 1.0 } else { 0.5 };
                self.part += detents * TEMP / step;
                let steps = self.part.trunc() as i32;
                self.part -= steps as f32;
                let before = self.doing.clone();
                self.temp_steps(steps, fahrenheit);
                if self.doing != before || steps == 0 {
                    self.drift += detents * TEMP;
                }
                if self.drift.abs() > 1.0 {
                    self.drift = 0.0;
                    self.clicks += 1;
                }
            }
        }
    }

    /// Whole detents, from a wheel that gives no counts.
    pub fn wheel(&mut self, detents: i32, fahrenheit: bool, schedule: &Schedule, now: Instant) {
        self.take_over();
        match &mut self.doing {
            Doing::Browse => {
                for _ in 0..detents.unsigned_abs() {
                    self.strip.wheel(-(DETENT as i32) * detents.signum(), now);
                }
                self.follow(schedule);
            }
            Doing::Menu { choices, at } => {
                let was = *at;
                *at = (*at as i32 + detents).clamp(0, choices.len() as i32 - 1) as usize;
                self.clicks += u32::from(*at != was);
            }
            Doing::Time(p) => {
                let was = p.minute;
                p.minute = step(p.minute as i32, detents).clamp(self.bounds.0, self.bounds.1) as u32;
                self.cursor = p.minute as i32;
                self.clicks += u32::from(p.minute != was);
            }
            Doing::Temp(_) | Doing::High(_) => {
                let before = self.doing.clone();
                self.temp_steps(detents, fahrenheit);
                self.clicks += u32::from(self.doing != before);
            }
        }
    }

    fn temp_steps(&mut self, detents: i32, fahrenheit: bool) {
        if detents == 0 {
            return;
        }
        let gap = crate::model::RANGE_GAP;
        match &mut self.doing {
            Doing::Temp(p) => {
                p.temp = stepped(p.temp, detents, fahrenheit);
                // The high end keeps the range's gap, pushed along if need be.
                if let Some(h) = p.high.as_mut() {
                    *h = h.max(p.temp + gap);
                }
            }
            Doing::High(p) => {
                if let Some(h) = p.high.as_mut() {
                    *h = stepped(*h, detents, fahrenheit).max(p.temp + gap);
                }
            }
            _ => {}
        }
    }

    /// A press; `unset` is a new setpoint's temperature where none is in
    /// force.
    pub fn press(&mut self, schedule: &mut Schedule, unset: (f32, Option<f32>)) -> Outcome {
        // A press while the screen scrolls in lands it where it goes.
        if let Some((.., to)) = self.opening.take() {
            let clicks = self.clicks;
            self.place_x(to, schedule);
            self.clicks = clicks;
        }
        self.slide = None;
        match self.doing.clone() {
            Doing::Browse => {
                self.strip.stop();
                if self.cursor < 0 {
                    return Outcome::Closed;
                }
                let choices = if self.held.is_some() { ON_POINT } else { EMPTY };
                self.doing = Doing::Menu { choices, at: 0 };
                self.part = 0.0;
                Outcome::Stay
            }
            Doing::Menu { choices, at } => {
                let minute = if choices == ON_POINT { self.held.unwrap_or(self.cursor as u32) } else { self.cursor as u32 };
                debug!("schedule: {} at {}", choices[at].to_lowercase(), when(minute));
                match choices[at] {
                    "New" => {
                        let now = schedule.point_at(minute);
                        let (temp, high) = now.map_or(unset, |p| (p.temp, p.high));
                        // From the scale's ends to those in force.
                        if now.is_some() {
                            self.slide = Some((Instant::now(), unset.0, unset.1));
                        }
                        self.set_time(Setpoint { minute, temp, high }, schedule);
                        Outcome::Stay
                    }
                    "Change" => {
                        let point = schedule.find(minute).unwrap_or(Setpoint { minute, temp: unset.0, high: unset.1 });
                        schedule.remove(minute);
                        self.set_time(point, schedule);
                        Outcome::Stay
                    }
                    "Remove" => {
                        info!("schedule: setpoint {} removed", when(minute));
                        schedule.remove(minute);
                        self.doing = Doing::Browse;
                        self.follow(schedule);
                        Outcome::Changed
                    }
                    "Done" => Outcome::Closed,
                    _ => {
                        self.doing = Doing::Browse;
                        Outcome::Stay
                    }
                }
            }
            Doing::Time(p) => {
                self.doing = Doing::Temp(p);
                self.part = 0.0;
                self.drift = 0.0;
                Outcome::Stay
            }
            Doing::Temp(p) if p.high.is_some() => {
                self.doing = Doing::High(p);
                self.part = 0.0;
                self.drift = 0.0;
                Outcome::Stay
            }
            Doing::Temp(p) | Doing::High(p) => {
                info!("schedule: setpoint {} set", point(&p));
                schedule.set(p);
                self.doing = Doing::Browse;
                self.place(p.minute, schedule);
                Outcome::Changed
            }
        }
    }
}

/// The quarter hour nearest `minute`.
fn quarter(minute: f32) -> i32 {
    (minute / STEP as f32).round() as i32 * STEP
}

/// A quarter of an hour a detent, from the dial to the week's end.
fn step(minute: i32, detents: i32) -> i32 {
    if minute < 0 {
        return if detents > 0 { 0 } else { DIAL };
    }
    let next = minute + detents * STEP;
    if next < 0 { DIAL } else { next.min(WEEK as i32 - STEP) }
}

/// A week minute as the log shows it: "Mon 07:30".
pub fn when(minute: u32) -> String {
    const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    let m = minute % WEEK;
    format!("{} {:02}:{:02}", DAYS[(m / DAY) as usize], m % DAY / 60, m % 60)
}

/// A setpoint as the log shows it.
fn point(p: &Setpoint) -> String {
    match p.high {
        Some(high) => format!("{} {:.1}-{high:.1}°C", when(p.minute), p.temp),
        None => format!("{} {:.1}°C", when(p.minute), p.temp),
    }
}

/// Stock's basic schedule's times (its quick schedules, 0x3a25e4, tables
/// 0x70a978, 0x70abb8, 0x70adf8): four a weekday, Friday's last later, two
/// at the weekend, in minutes from Monday 00:00.
const BASIC_TIMES: [u32; 24] = [
    360, 510, 990, 1350, DAY + 360, DAY + 510, DAY + 990, DAY + 1350, 2 * DAY + 360, 2 * DAY + 510, 2 * DAY + 990, 2 * DAY + 1350,
    3 * DAY + 360, 3 * DAY + 510, 3 * DAY + 990, 3 * DAY + 1350, 4 * DAY + 360, 4 * DAY + 510, 4 * DAY + 990, 4 * DAY + 1380,
    5 * DAY + 450, 5 * DAY + 1380, 6 * DAY + 450, 6 * DAY + 1350,
];

/// The basic schedule's temperatures in °C by its type (stock's vt+0x500,
/// 0xb98e8; set at 0x9b2c8): 0, heating and cooling (69/65/69/64 °F and
/// 75/77/75/74 °F) on a unit without Heat Link; 1, heating only for
/// Ireland and the UK; 2, heating only elsewhere. Each day's points wake,
/// leave, come home and sleep in turn, the weekend's wake and sleep.
fn basic_temps(kind: u8, i: usize) -> (f32, Option<f32>) {
    let f = |f: f32| (f - 32.0) * 5.0 / 9.0;
    // The setpoint's part of its day: wake, leave, home, sleep.
    let part = if i >= 20 { [0, 3][i % 2] } else { i % 4 };
    match kind {
        0 => ([f(69.0), f(65.0), f(69.0), f(64.0)][part], Some([f(75.0), f(77.0), f(75.0), f(74.0)][part])),
        1 => ([20.0, 16.0, 20.0, 15.0][part], None),
        _ => ([20.0, 17.5, 20.0, 16.5][part], None),
    }
}

/// Stock's basic schedule of `kind` (`basic_temps`) for each schedule type
/// it has: HEAT, and with cooling temperatures COOL and RANGE.
pub fn basic(kind: u8) -> Vec<(&'static str, Schedule)> {
    let mut out = vec![("HEAT", Schedule::default())];
    if kind == 0 {
        out.push(("COOL", Schedule::default()));
        out.push(("RANGE", Schedule::default()));
    }
    for (i, &minute) in BASIC_TIMES.iter().enumerate() {
        let (heat, cool) = basic_temps(kind, i);
        for (name, s) in &mut out {
            let point = match (*name, cool) {
                ("COOL", Some(c)) => Setpoint { minute, temp: c, high: None },
                ("RANGE", Some(c)) => Setpoint { minute, temp: heat, high: Some(c) },
                _ => Setpoint { minute, temp: heat, high: None },
            };
            s.set(point);
        }
    }
    out
}

/// Minutes from Monday 00:00 of a local time with `weekday` 0 as Sunday.
pub fn week_minute(weekday: u8, hour: u8, minute: u8) -> u32 {
    (u32::from(weekday) + 6) % 7 * DAY + u32::from(hour) * 60 + u32::from(minute)
}

#[cfg(test)]
mod tests {

    use super::*;
    use std::time::Duration;

    const UNSET: (f32, Option<f32>) = (20.0, None);

    /// Runs the timeline on in 17 ms steps for `ms`.
    fn run(e: &mut Editor, s: &Schedule, start: Instant, ms: u64) -> Instant {
        let mut t = start;
        for _ in 0..ms / 17 {
            t += Duration::from_millis(17);
            e.tick(t, s);
        }
        t
    }

    #[test]
    fn the_wheel_flings_the_timeline_and_clicks_each_hour_crossed() {
        let now = Instant::now();
        let s = Schedule::default();
        let mut e = Editor::open(7 * 60 + 7, &s);
        assert_eq!(e.cursor, 7 * 60, "the quarter hour");
        assert!((e.scroll - 427.0).abs() < 0.01, "the timeline where it is");
        // Detents 30 ms apart fling it on; it slows to rest unsnapped.
        let mut t = now;
        for _ in 0..6 {
            t += Duration::from_millis(30);
            e.counts(-120, false, &s, t);
        }
        run(&mut e, &s, t, 3000);
        assert!(!e.strip.moving());
        let moved = e.scroll - (7.0 * 60.0 + 7.0);
        assert!((300.0..1000.0).contains(&moved), "{moved}");
        assert_ne!(e.scroll % 120.0, 0.0, "no spring within the week");
        assert_eq!(e.cursor, quarter(e.scroll));
        let hours = (e.cursor / 60 - 7) as u32;
        assert!(e.clicks > 0 && e.clicks <= hours, "a click a step an hour or more is crossed in: {} {hours}", e.clicks);
        // Off Monday's start it springs to the dial.
        e.place(10, &s);
        e.counts(1200, false, &s, t);
        run(&mut e, &s, t, 4000);
        assert_eq!((e.cursor, e.scroll), (DIAL, DIAL as f32));
        assert_eq!(e.press(&mut Schedule::default(), UNSET), Outcome::Closed);
        // And past Sunday's end, to the dial after it.
        e.place(WEEK - 20, &s);
        assert_eq!(e.cursor, WEEK as i32 - STEP, "Sunday 23:45 at the week's end");
        e.counts(-1200, false, &s, t);
        run(&mut e, &s, t, 4000);
        assert_eq!((e.cursor, e.scroll), (DIAL, WEEK as f32 - DIAL as f32));
    }

    #[test]
    fn a_setpoints_time_and_temperature_move_and_click_as_stocks() {
        let now = Instant::now();
        let turn = |e: &mut Editor, s: &Schedule, f: bool, detents: i32| for _ in 0..detents.abs() { e.counts(-120 * detents.signum(), f, s, now) };
        let mut s = Schedule::default();
        let mut e = Editor::open(9 * 60 + 45, &s);
        // A new setpoint: its time 22.5 minutes a detent, by quarters, a
        // click each.
        e.press(&mut s, UNSET);
        e.press(&mut s, UNSET);
        e.clicks = 0;
        turn(&mut e, &s, true, 1);
        assert_eq!((e.cursor, e.clicks), (10 * 60 + 15, 1));
        turn(&mut e, &s, true, 1);
        assert_eq!((e.cursor, e.clicks), (10 * 60 + 30, 2));
        e.press(&mut s, UNSET);
        // Its temperature a degree and a quarter a detent, in whole
        // degrees, a click each degree past the last.
        e.clicks = 0;
        turn(&mut e, &s, true, 4);
        assert_eq!(e.clicks, 4);
        e.press(&mut s, UNSET);
        let p = s.points[0];
        assert_eq!((p.minute, crate::model::to_fahrenheit(p.temp).round()), (10 * 60 + 30, 73.0));
        // Celsius moves as far a detent, in halves.
        let mut c = Editor::open(0, &s);
        c.doing = Doing::Temp(Setpoint { minute: 0, temp: 20.0, high: None });
        turn(&mut c, &s, false, 2);
        assert_eq!(c.doing, Doing::Temp(Setpoint { minute: 0, temp: 22.5, high: None }));
        // The setpoint is under the cursor within 75 minutes of it.
        assert_eq!((e.cursor, e.held), (p.minute as i32, Some(p.minute)));
        e.place(p.minute + 70, &s);
        assert_eq!(e.held, Some(p.minute));
        e.place(p.minute - 80, &s);
        assert_eq!(e.held, None);
    }

    #[test]
    fn a_schedules_temperatures_keep_to_stocks_limits_and_start_at_the_top() {
        let f = |c: f32| crate::model::to_fahrenheit(c).round();
        assert_eq!(f(stepped(20.0, -100, true)), 50.0);
        assert_eq!(f(stepped(20.0, 100, true)), 90.0);
        assert_eq!((stepped(20.0, -100, false), stepped(20.0, 100, false)), (9.0, 32.0));
        assert_eq!(unset(false, false), (32.0, None));
        assert_eq!(unset(false, true), (9.0, Some(32.0)));
        let (low, high) = unset(true, true);
        assert_eq!((f(low), high.map(f)), (50.0, Some(90.0)));
        // A new setpoint on an empty schedule takes it; else the one in force.
        let mut s = Schedule::default();
        let mut e = Editor::open(600, &s);
        e.press(&mut s, unset(false, false));
        e.press(&mut s, unset(false, false));
        assert_eq!(e.doing, Doing::Time(Setpoint { minute: 600, temp: 32.0, high: None }));
    }

    #[test]
    fn range_setpoints_read_and_write_temp_min_and_temp_max() {
        let days = json!({"1": {"0": {"entry_type": "setpoint", "temp-min": 19.5, "temp-max": 25.0, "time": 3600, "type": "RANGE"},
                                "1": {"entry_type": "setpoint", "temp-min": 19.5, "time": 7200, "type": "RANGE"}}});
        let s = Schedule::from_days(&days);
        assert_eq!(s.points, vec![Setpoint { minute: DAY + 60, temp: 19.5, high: Some(25.0) }], "a RANGE needs both ends");
        let out = s.to_days("RANGE", 0);
        assert_eq!((out["1"]["1"]["temp-min"].clone(), out["1"]["1"]["temp-max"].clone()), (json!(19.5), json!(25.0)));
        assert_eq!((out["1"]["1"]["type"].clone(), out["1"]["1"].get("temp")), (json!("RANGE"), None));
        assert_eq!(out["2"]["0"]["entry_type"], json!("continuation"));
        assert_eq!(Schedule::from_days(&out), s);
    }

    #[test]
    fn a_range_setpoint_changes_its_low_then_its_high_a_gap_apart() {
        let now = Instant::now();
        let mut s = Schedule::default();
        s.set(Setpoint { minute: 600, temp: 20.0, high: Some(24.0) });
        let mut e = Editor::open(600, &s);
        e.press(&mut s, UNSET);
        e.press(&mut s, UNSET);
        assert!(matches!(e.doing, Doing::Time(_)), "Change");
        e.press(&mut s, UNSET);
        e.wheel(7, false, &s, now);
        assert_eq!(e.doing, Doing::Temp(Setpoint { minute: 600, temp: 23.5, high: Some(25.0) }), "the high pushed along");
        e.press(&mut s, UNSET);
        e.wheel(-4, false, &s, now);
        assert_eq!(e.doing, Doing::High(Setpoint { minute: 600, temp: 23.5, high: Some(25.0) }), "never closer than the gap");
        e.wheel(2, false, &s, now);
        assert_eq!(e.press(&mut s, UNSET), Outcome::Changed);
        assert_eq!(s.points, vec![Setpoint { minute: 600, temp: 23.5, high: Some(26.0) }]);
    }


    #[test]
    fn the_temperature_in_force_is_the_last_setpoint_wrapping_the_week() {
        let mut s = Schedule::default();
        assert_eq!(s.at(100), None);
        s.set(Setpoint { minute: 2 * DAY + 13 * 60 + 30, temp: 20.0, high: None });
        s.set(Setpoint { minute: 4 * 60 + 45, temp: 22.0, high: None });
        assert_eq!(s.points.iter().map(|p| p.minute).collect::<Vec<_>>(), vec![285, 3690]);
        assert_eq!(s.at(0), Some(20.0), "Monday starts where Wednesday left off");
        assert_eq!(s.at(300), Some(22.0));
        assert_eq!(s.at(6 * DAY), Some(20.0));
        s.set(Setpoint { minute: 285, temp: 18.0, high: None });
        assert_eq!(s.points.len(), 2, "a setpoint at the same minute replaces");
    }

    #[test]
    fn the_bucket_days_round_trip_with_continuations() {
        let days = json!({"2": {"0": {"entry_type": "continuation", "temp": 19.957, "time": 0, "type": "COOL"},
                                "1": {"entry_type": "setpoint", "temp": 19.957, "time": 48600, "type": "COOL"}},
                          "5": {}});
        let s = Schedule::from_days(&days);
        assert_eq!(s.points, vec![Setpoint { minute: 2 * DAY + 810, temp: 19.957, high: None }]);
        let out = s.to_days("COOL", 7);
        assert_eq!(out["2"]["1"]["time"], json!(48600));
        assert_eq!(out["2"]["1"]["entry_type"], json!("setpoint"));
        assert_eq!(out["0"]["0"]["entry_type"], json!("continuation"));
        assert_eq!(out["0"]["0"]["temp"], json!(19.957));
        assert_eq!(out["0"].as_object().unwrap().len(), 1);
        assert_eq!(Schedule::from_days(&out), s);
        assert_eq!(Schedule::default().to_days("HEAT", 0)["3"], json!({}));
    }

    #[test]
    fn the_screen_scrolls_in_from_the_dial_as_stocks_opening() {
        let s = Schedule::default();
        let start = Instant::now();
        // Monday 06:00: 336 pixels, 504 + 476 ms.
        let mut e = Editor::open(6 * 60, &s);
        e.scroll_in(start, &s);
        assert_eq!((e.cursor, e.scroll), (DIAL, DIAL as f32), "from the dial");
        assert!(e.tick(start + Duration::from_millis(490), &s));
        assert!((e.scroll - (DIAL as f32 + 840.0 * 0.5)).abs() < 2.0, "halfway at half its time: {}", e.scroll);
        assert!(!e.tick(start + Duration::from_millis(981), &s));
        assert_eq!((e.cursor, e.scroll, e.clicks), (6 * 60, 6.0 * 60.0, 0), "there, without clicks");
        // The wheel takes over where it is.
        let mut w = Editor::open(6 * 60, &s);
        w.scroll_in(start, &s);
        w.tick(start + Duration::from_millis(100), &s);
        w.counts(-120, false, &s, start + Duration::from_millis(100));
        assert!(w.opening.is_none());
    }

    #[test]
    fn a_new_setpoint_slides_from_the_top_to_the_temperature_in_force() {
        let mut s = Schedule::default();
        s.set(Setpoint { minute: 60, temp: 20.0, high: None });
        let mut e = Editor::open(600, &s);
        e.press(&mut s, unset(false, false));
        e.press(&mut s, unset(false, false));
        let (start, ..) = e.slide.expect("sliding");
        let at = |ms: u64| e.pending(start + Duration::from_millis(ms)).unwrap().temp;
        assert_eq!(at(0), 32.0);
        assert!((at(250) - (32.0 - 12.0 * std::f32::consts::FRAC_1_SQRT_2)).abs() < 0.01, "easing out: {}", at(250));
        assert_eq!(at(500), 20.0);
        assert!(!e.tick(start + Duration::from_millis(500), &s), "done");
        // A change does not slide.
        e.press(&mut s, UNSET);
        e.press(&mut s, UNSET);
        let mut c = Editor::open(60, &s);
        c.press(&mut s, UNSET);
        c.press(&mut s, UNSET);
        assert!(c.slide.is_none());
    }

    #[test]
    fn a_setpoints_time_keeps_an_hour_from_its_neighbours_round_the_week() {
        let now = Instant::now();
        let mut s = Schedule::default();
        s.set(Setpoint { minute: 600, temp: 20.0, high: None });
        s.set(Setpoint { minute: 900, temp: 20.0, high: None });
        assert_eq!(Editor::bounds_of(700, &s), (660, 840));
        assert_eq!(Editor::bounds_of(300, &s), (0, 540), "Monday's start: the week's last is 23 h back");
        assert_eq!(Editor::bounds_of(1000, &s), (960, WEEK as i32 - STEP), "Sunday's end: round to Monday 10:00");
        s.set(Setpoint { minute: WEEK - 30, temp: 20.0, high: None });
        assert_eq!(Editor::bounds_of(100, &s), (30, 540), "an hour on from Sunday 23:30");
        // Turning stops there.
        let mut s = Schedule::default();
        s.set(Setpoint { minute: 600, temp: 20.0, high: None });
        let mut e = Editor::open(800, &s);
        e.press(&mut s, UNSET);
        e.press(&mut s, UNSET);
        let mut w = e.clone();
        w.wheel(-20, false, &s, now);
        assert_eq!(w.cursor, 660);
        for _ in 0..20 {
            e.counts(120, false, &s, now);
        }
        assert_eq!(e.cursor, 660);
        e.counts(-120, false, &s, now);
        assert_eq!(e.cursor, 660 + 30, "and comes straight back, 22.5 minutes on");
    }

    #[test]
    fn midnights_triangles_give_way_to_setpoints_near_them() {
        let mut s = Schedule::default();
        assert_eq!(s.day_starts(), vec![None; 8]);
        s.set(Setpoint { minute: DAY + 30, temp: 18.0, high: None });
        s.set(Setpoint { minute: 3 * DAY - 6 * 60 + 15, temp: 21.0, high: None });
        let starts = s.day_starts();
        assert_eq!(starts[0], Some(21.0), "Monday: none before it in the week");
        assert_eq!(starts[1], None, "Tuesday: a setpoint in its first hour");
        assert_eq!(starts[2], Some(18.0));
        assert_eq!(starts[3], None, "Thursday: one under 6 h before");
        assert_eq!((starts[4], starts[7]), (Some(21.0), Some(21.0)), "and the week's end");
        s.set(Setpoint { minute: WEEK - 6 * 60, temp: 19.0, high: None });
        assert_eq!(s.day_starts()[7], None, "6 h before the week's last second");
    }

    #[test]
    fn a_new_setpoint_takes_a_time_then_a_temperature_and_the_dial_closes() {
        let now = Instant::now();
        let mut s = Schedule::default();
        let mut e = Editor::open(week_minute(3, 1, 6), &s);
        assert_eq!(e.cursor, 2 * DAY as i32 + 60, "Wednesday 01:00");
        assert_eq!(e.press(&mut s, UNSET), Outcome::Stay);
        assert_eq!(e.doing, Doing::Menu { choices: EMPTY, at: 0 });
        e.press(&mut s, UNSET);
        e.wheel(5, false, &s, now);
        e.press(&mut s, UNSET);
        e.wheel(-8, false, &s, now);
        assert_eq!(e.press(&mut s, UNSET), Outcome::Changed);
        assert_eq!(s.points, vec![Setpoint { minute: 2 * DAY + 60 + 75, temp: 16.0, high: None }]);
        assert_eq!((e.cursor, e.held), (s.points[0].minute as i32, Some(s.points[0].minute)));
        e.press(&mut s, UNSET);
        assert_eq!(e.doing, Doing::Menu { choices: ON_POINT, at: 0 });
        e.wheel(1, false, &s, now);
        assert_eq!(e.press(&mut s, UNSET), Outcome::Changed);
        assert!(s.points.is_empty());
        e.wheel(-200, false, &s, now);
        run(&mut e, &s, now, 5000);
        assert_eq!(e.cursor, DIAL);
        assert_eq!(e.press(&mut s, UNSET), Outcome::Closed);
        assert_eq!(step(DIAL, 1), 0, "the dial is one step before Monday");
        assert_eq!(step(0, -1), DIAL);
    }
}
