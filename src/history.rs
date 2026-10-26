//! Energy history, as stock nlclient records it (its energy summary,
//! 0x34fc0c sampling, 0x34eed0 day rollover, 0x34d4e4 reason): every 30
//! seconds the day's heating, cooling, leaf, off and Eco seconds; at local
//! midnight the day is closed, a reason is found against the week's
//! average, and 29 days are kept.

use std::path::PathBuf;
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// Days kept.
const KEPT: usize = 29;
/// A gap between samples longer than this counts as missing.
const GAP: f64 = 120.0;
/// Half a day: past it a day's information is missing.
pub const HALF_DAY: u32 = 43200;

/// Why a day used more or less than the week's average (stock's
/// `energy_winner`).
pub const MANUAL: i8 = 0;
pub const WEATHER: i8 = 1;
pub const ECO: i8 = 2;
pub const AWAY: i8 = 3;
pub const DRY: i8 = 5;

/// One day (stock's ES record), seconds unless said.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Day {
    /// Local midnight, epoch seconds.
    pub date: i64,
    pub missing: u32,
    pub valid: u32,
    pub heat: u32,
    pub cool: u32,
    pub leaf: u32,
    pub off: u32,
    pub manual_eco: u32,
    pub auto_away: u32,
    pub weather: u32,
    pub dry: u32,
    pub start_off: bool,
    pub end_off: bool,
    /// The day's runs: start (seconds of the day), length, heat or cool.
    pub cycles: Vec<(u32, u32, bool)>,
    /// The reason, −1 for none, and how far from the week's average.
    pub reason: i8,
    pub above_by: i64,
    /// The leaf shown for an hour or more.
    pub leaf_earned: bool,
    /// Time with a heat or cool target, and those targets' mean (°C;
    /// summed through the day, the mean once closed): as set, and as the
    /// schedule had them (stock's TmActH/C, TTActH/C, TTSchH/C).
    pub act: Targets,
    pub sch: Targets,
}

/// A kind of target's time (s) and mean, heat then cool.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Targets {
    pub heat_time: u32,
    pub cool_time: u32,
    pub heat: f64,
    pub cool: f64,
}

impl Targets {
    /// A sample's target, heat or cool, for `dt` seconds.
    fn add(&mut self, target: Option<(bool, f32)>, dt: u32) {
        match target {
            Some((true, t)) => {
                self.heat_time += dt;
                self.heat += f64::from(t) * f64::from(dt);
            }
            Some((false, t)) => {
                self.cool_time += dt;
                self.cool += f64::from(t) * f64::from(dt);
            }
            None => {}
        }
    }

    /// The sums made means.
    fn close(&mut self) {
        if self.heat_time > 0 {
            self.heat /= f64::from(self.heat_time);
        }
        if self.cool_time > 0 {
            self.cool /= f64::from(self.cool_time);
        }
    }
}

/// What a sample sees.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    pub heat: bool,
    pub cool: bool,
    pub leaf: bool,
    pub off: bool,
    pub manual_eco: bool,
    pub weather: bool,
    pub dry: bool,
    /// The target, heat (true) or cool, as set and as scheduled; neither in
    /// heat-cool or off.
    pub target: Option<(bool, f32)>,
    pub scheduled: Option<(bool, f32)>,
    /// Air moves through the filter (stock's 0x2ef8c8: the fan, or a stage
    /// that is not radiant or a radiator).
    pub air: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct History {
    /// Closed days, newest first.
    pub days: Vec<Day>,
    pub today: Day,
    /// The last sample, and the run open since (start, heat).
    last: Option<f64>,
    open: Option<(u32, bool)>,
    /// Stock drops the first day it sees, partial as it is.
    crossed: bool,
    /// The air filter's run time (stock's TotalFilterTime, FilterAlertSent,
    /// FilterRuntimeAtDismissal).
    #[serde(default)]
    pub filter: Filter,
    #[serde(skip)]
    file: Option<PathBuf>,
}

/// The air filter's run time, seconds; after a REMIND LATER `total`
/// counts again from there and `at_dismissal` keeps what it had.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Filter {
    pub total: u64,
    pub alert_sent: bool,
    pub at_dismissal: u64,
}

impl Filter {
    /// The run time as published (stock's filter_runtime_sec).
    pub fn runtime(&self) -> u64 {
        self.total + self.at_dismissal
    }

    /// REMIND LATER (0x34d01c).
    pub fn remind_later(&mut self) {
        self.alert_sent = true;
        self.at_dismissal = self.total;
        self.total = 0;
    }

    /// The filter was changed `days` ago (0x34cf34): five hours a day
    /// counted since, 75 hours short of `threshold` at most.
    pub fn changed(&mut self, days: u64, threshold: u64) {
        self.alert_sent = false;
        self.at_dismissal = 0;
        self.total = days * 18000;
        if self.total > threshold {
            self.total = threshold.saturating_sub(270_000);
        }
    }
}

/// Local midnight of an epoch time, and the seconds since it.
fn midnight(t: f64) -> (i64, u32) {
    // SAFETY: localtime_r fills the tm it is given; mktime reads it.
    unsafe {
        let secs = t as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&secs, &mut tm);
        let into = (tm.tm_hour * 3600 + tm.tm_min * 60 + tm.tm_sec) as u32;
        tm.tm_hour = 0;
        tm.tm_min = 0;
        tm.tm_sec = 0;
        tm.tm_isdst = -1;
        (libc::mktime(&mut tm) as i64, into)
    }
}

impl History {
    pub fn load(file: Option<PathBuf>) -> History {
        let mut h = file.as_ref().and_then(|f| std::fs::read(f).ok().map(|d| (f, d)))
            .and_then(|(f, d)| serde_json::from_slice::<History>(&d).map_err(|e| warn!("history: {}: {e}; starting afresh", crate::log::path(f))).ok())
            .unwrap_or_default();
        if let Some(f) = &file {
            debug!("history: {} ({} days)", crate::log::path(f), h.days.len());
        }
        h.file = file;
        h
    }

    fn save(&self) {
        let Some(file) = &self.file else { return };
        if let Ok(data) = serde_json::to_vec(self) {
            let tmp = file.with_extension("tmp");
            if std::fs::write(&tmp, data).and_then(|_| std::fs::rename(&tmp, file)).is_err() {
                error!("history: could not write {}", crate::log::path(file));
            }
        }
    }

    /// A sample at `now` (epoch seconds).
    pub fn sample(&mut self, now: f64, s: Sample) {
        let (date, into) = midnight(now);
        if self.today.date == 0 {
            self.today.date = date;
            self.today.start_off = s.off;
        }
        if date != self.today.date {
            self.close(date, s.off);
        }
        let Some(last) = self.last.replace(now) else { return };
        let dt = (now - last).clamp(0.0, f64::from(into.max(1))) as u32;
        let d = &mut self.today;
        if now - last > GAP {
            d.missing += dt;
        } else {
            d.valid += dt;
            let add = |on: bool, slot: &mut u32| if on { *slot += dt };
            add(s.heat, &mut d.heat);
            add(s.cool, &mut d.cool);
            add(s.leaf, &mut d.leaf);
            add(s.off, &mut d.off);
            add(s.manual_eco, &mut d.manual_eco);
            add(s.weather, &mut d.weather);
            add(s.dry, &mut d.dry);
            d.act.add(s.target, dt);
            if s.air {
                self.filter.total += u64::from(dt);
            }
            d.sch.add(s.scheduled, dt);
        }
        d.end_off = s.off;
        // The day's runs, for its timeline.
        let running = if s.heat { Some(true) } else if s.cool { Some(false) } else { None };
        match (self.open, running) {
            (Some((_, h)), Some(r)) if h == r => {}
            (open, running) => {
                if let Some((start, heat)) = open {
                    d.cycles.push((start, into.saturating_sub(start), heat));
                }
                self.open = running.map(|heat| (into, heat));
            }
        }
    }

    /// Local midnight passed: the day closes (the first one seen is
    /// dropped), empty days fill any gap, and the newest gets its reason.
    fn close(&mut self, date: i64, off: bool) {
        let mut done = std::mem::take(&mut self.today);
        if let Some((start, heat)) = self.open.take() {
            done.cycles.push((start, 86400u32.saturating_sub(start), heat));
        }
        done.leaf_earned = done.leaf >= 3600;
        done.act.close();
        done.sch.close();
        if std::mem::replace(&mut self.crossed, true) {
            let mut day = done.date + 86400;
            self.days.insert(0, done);
            while day < date - 3600 {
                self.days.insert(0, Day { date: day, missing: 86400, reason: -1, ..Day::default() });
                day += 86400;
            }
            self.days.dedup_by_key(|d| d.date);
            self.days.retain(|d| d.date < date);
            self.days.sort_by_key(|d| std::cmp::Reverse(d.date));
            self.days.truncate(KEPT);
            self.reason();
            if let Some(d) = self.days.first() {
                info!("history: day closed: heat {} s, cool {} s, leaf {}, reason {}", d.heat, d.cool, d.leaf_earned, d.reason);
            }
        }
        self.today = Day { date, start_off: off, ..Day::default() };
        self.save();
    }

    /// The newest day's reason (0x34d4e4): against the average of the six
    /// days before it with information, an unusual day below it is put
    /// down to being turned off, Eco, Home/Away Assist, then a target held
    /// further from the schedule than the same day last week, then the
    /// weather; a usual one only to those but the weather; a day above it,
    /// when unusual, to Cool to Dry, the target test, then the weather.
    /// Seasonal Savings comes from the service and is not given here.
    fn reason(&mut self) {
        let Some(today) = self.days.first() else { return };
        let total = |d: &Day| i64::from(d.heat) + i64::from(d.cool);
        let mut reason = -1;
        let n = self.days.len().min(7);
        let previous: Vec<i64> = self.days[1..n].iter().filter(|d| d.missing <= HALF_DAY).map(total).collect();
        let mut above_by = 0;
        // Stock reads the eighth slot as the same day last week, empty
        // with only seven days.
        let week = self.days.len() >= 7;
        let last = self.days.get(7).cloned().unwrap_or_default();
        if self.days.len() >= 2 && today.missing <= HALF_DAY && !previous.is_empty() {
            let avg = previous.iter().sum::<i64>() / previous.len() as i64;
            let diff = total(today) - avg;
            above_by = diff;
            let unusual = previous.iter().filter(|p| (*p - avg).abs() + 899 >= diff.abs()).count() < 3;
            let leading_zero = self.days.iter().take_while(|d| total(d) == 0).count() > 2;
            let (a, w) = (&today.act, &last.act);
            let weather = || if today.weather > 64799 { WEATHER } else { -1 };
            if !leading_zero && avg >= 900 && diff != 0 {
                reason = if diff < 0 && unusual {
                    if !today.start_off && today.end_off {
                        MANUAL
                    } else if today.start_off && today.off > 72000 {
                        -1
                    } else if today.manual_eco > 3599 {
                        ECO
                    } else if today.auto_away > 3599 {
                        AWAY
                    } else if !week {
                        -1
                    } else {
                        let heat = a.heat_time >= a.cool_time && w.heat_time >= w.cool_time;
                        let cool = (a.heat_time < a.cool_time && w.cool_time >= w.heat_time) || (a.heat_time == a.cool_time && w.heat_time < w.cool_time);
                        let x = if heat && a.heat < today.sch.heat {
                            Some(w.heat - a.heat)
                        } else if !heat && cool && a.cool > today.sch.cool {
                            Some(a.cool - w.cool)
                        } else {
                            None
                        };
                        match x {
                            Some(x) if x >= 3604.0 / 65536.0 => MANUAL,
                            _ if heat || cool => weather(),
                            _ => -1,
                        }
                    }
                } else if diff < 0 {
                    if !today.start_off && today.end_off {
                        MANUAL
                    } else if today.auto_away > 17999 {
                        AWAY
                    } else if today.manual_eco > 17999 {
                        ECO
                    } else if !week {
                        -1
                    } else {
                        let x = if a.cool_time < a.heat_time {
                            (w.heat_time > w.cool_time && today.sch.heat > a.heat).then_some(w.heat - a.heat)
                        } else {
                            (a.cool_time > a.heat_time && w.cool_time > w.heat_time && a.cool > today.sch.cool).then_some(a.cool - w.cool)
                        };
                        if x.is_some_and(|x| x >= 18153.0 / 65536.0) { MANUAL } else { -1 }
                    }
                } else if !unusual {
                    -1
                } else if today.dry > 0 {
                    DRY
                } else if !week {
                    -1
                } else {
                    let cool = (a.heat_time < a.cool_time && w.heat_time <= w.cool_time) || (a.heat_time == a.cool_time && w.heat_time < w.cool_time);
                    let heat = a.heat_time >= a.cool_time && w.heat_time >= w.cool_time;
                    let manual = if cool {
                        a.cool < today.sch.cool && w.cool - a.cool > 3603.0 / 65536.0
                    } else if heat {
                        a.heat > today.sch.heat && a.heat - w.heat > 3603.0 / 65536.0
                    } else {
                        false
                    };
                    if manual { MANUAL } else { weather() }
                };
            }
        }
        let today = &mut self.days[0];
        today.reason = reason;
        today.above_by = above_by;
    }

    /// Yesterday as stock's History ring item shows it.
    pub fn yesterday(&self) -> String {
        let Some(d) = self.days.first() else { return "Yesterday:\nNo usage".into() };
        let mut lines = vec!["Yesterday:".to_string()];
        if d.heat > 0 {
            lines.push(format!("{} of heat", duration(d.heat).to_lowercase()));
        }
        if d.cool > 0 {
            lines.push(format!("{} of cool", duration(d.cool).to_lowercase()));
        }
        if lines.len() == 1 {
            lines.push(if d.missing <= HALF_DAY { "No usage" } else { "Info missing" }.into());
        }
        lines.join("\n")
    }
}

/// Seasonal Savings, the reason no client feature gives yet.
pub const SEASONAL: i8 = 4;

/// The detail page's message for a day (stock's energyhistorylayer
/// setState, 0x120ea0).
pub fn message(d: &Day) -> String {
    if d.missing > HALF_DAY {
        return format!("{} hours of information are missing for this day.", d.missing / 3600);
    }
    if d.heat + d.cool == 0 {
        return "No energy usage.".into();
    }
    let above = d.above_by >= 0;
    let pick = |a: &str, b: &str| if above { a.to_string() } else { b.to_string() };
    if d.above_by.abs() <= 3 || d.reason < 0 {
        return "Energy usage was around the weekly average.".into();
    }
    match d.reason {
        MANUAL => pick("Manual adjustments caused energy usage above the weekly average.", "Manual adjustments helped you save energy."),
        WEATHER => pick("Weather caused energy usage above the weekly average.", "Energy usage was below the weekly average due to weather."),
        ECO => pick("Eco Temperatures caused energy usage above the weekly average.", "Eco Temperatures helped you save energy."),
        AWAY => pick("Home/Away Assist caused energy usage above the weekly average.", "Home/Away Assist helped you save energy."),
        SEASONAL => "Seasonal Savings helped you save energy.".into(),
        DRY => "Cool to Dry caused energy usage above the weekly average.".into(),
        _ => "Energy usage was around the weekly average.".into(),
    }
}

/// The detail page's run total (0x132ea8, its exact form): "5 HR 45 MIN".
pub fn exact(seconds: u32) -> String {
    let (h, m) = (seconds / 3600, seconds % 3600 / 60);
    match (h, m) {
        (0, m) => format!("{m} MIN"),
        (h, 0) => format!("{h} HR"),
        (h, m) => format!("{h} HR {m} MIN"),
    }
}

/// Stock's months, abbreviated, in the language in use.
pub const MONTHS: [&str; 12] = ["@string.january_abbr|Jan", "@string.february_abbr|Feb", "@string.march_abbr|Mar",
                                "@string.april_abbr|Apr", "@string.may_abbr|May", "@string.june_abbr|Jun", "@string.july_abbr|Jul",
                                "@string.august_abbr|Aug", "@string.september_abbr|Sep", "@string.october_abbr|Oct",
                                "@string.november_abbr|Nov", "@string.december_abbr|Dec"];

/// `date` (seconds since the epoch) in local time.
fn local(date: i64) -> libc::tm {
    // SAFETY: localtime_r fills the tm it is given.
    unsafe {
        let secs = date as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&secs, &mut tm);
        tm
    }
}

/// A day as stock's nlString::GetFormattedTime (libnlappcore) writes it
/// with flags 0x408: the month abbreviated (8), no year, in the order
/// nlString::GetDateFormatFlagsFromLocale gives `language` (0x400).
pub fn short_date(date: i64, language: &str) -> String {
    let tm = local(date);
    day_month(tm.tm_mday, tm.tm_mon, language)
}

/// "Oct 17" in en_US, "17. Okt." in de_DE, "17 oct." in the others.
fn day_month(day: i32, month: i32, language: &str) -> String {
    in_order(day, &crate::i18n::tr(MONTHS[month as usize % 12]), language)
}

/// The day and `month` in `language`'s order.
fn in_order(day: i32, month: &str, language: &str) -> String {
    match language {
        "en_US" => format!("{month} {day}"),
        "de_DE" => format!("{day}. {month}"),
        _ => format!("{day} {month}"),
    }
}

/// Farsight's digital clock's date line as someone comes near (0x1437bc):
/// fardisplaylayer.dateformat.weekday_date with the weekday whole
/// (libnlappcore's GetDayString 0x200: string.time.saturday, case as
/// translated) and the date as GetFormattedTime 0x400 writes it (the
/// locale's order, the month whole as without 8, no year as without 0x20):
/// "Saturday, October 17".
pub fn weekday_date(date: i64, language: &str) -> String {
    const DAYS: [&str; 7] = ["@string.time.sunday|Sunday", "@string.time.monday|Monday", "@string.time.tuesday|Tuesday",
                             "@string.time.wednesday|Wednesday", "@string.time.thursday|Thursday", "@string.time.friday|Friday",
                             "@string.time.saturday|Saturday"];
    const MONTHS: [&str; 12] = ["@string.january|January", "@string.february|February", "@string.march|March", "@string.april|April",
                                "@string.may|May", "@string.june|June", "@string.july|July", "@string.august|August",
                                "@string.september|September", "@string.october|October", "@string.november|November",
                                "@string.december|December"];
    let tm = local(date);
    let day = crate::i18n::tr(DAYS[tm.tm_wday as usize % 7]);
    let on = in_order(tm.tm_mday, &crate::i18n::tr(MONTHS[tm.tm_mon as usize % 12]), language);
    crate::i18n::fill(&crate::i18n::tr("@fardisplaylayer.dateformat.weekday_date|%1$s, %2$s"), &[day, on])
}

/// Farsight's analog clock's date line as someone comes near (0x144268):
/// fardisplaylayer.dateformat.weekday_dayofmonth with the weekday short
/// (GetDayString without 0x200: string.time.sat) in capitals (0x100) and
/// the day of the month: "SAT 17".
pub fn weekday_day(date: i64) -> String {
    let tm = local(date);
    let day = crate::i18n::tr(DAYS[tm.tm_wday as usize % 7]).to_uppercase();
    crate::i18n::fill(&crate::i18n::tr("@fardisplaylayer.dateformat.weekday_dayofmonth|%1$ls %2$d"), &[day, tm.tm_mday.to_string()])
}

/// The hour of `date` (epoch s) in local time, 0 to 23.
pub fn hour_of(date: i64) -> i32 {
    local(date).tm_hour
}

/// A forecast hour's time as Farsight's weather writes it (0x1423fc):
/// GetFormattedTime with the hour alone in 12-hour time ("3 PM"; a plain
/// space for libnlappcore's narrow no-break one, which the faces lack),
/// else hours and minutes ("15:00").
pub fn forecast_time(date: i64, twelve_hour: bool) -> String {
    let tm = local(date);
    if !twelve_hour {
        return format!("{:02}:{:02}", tm.tm_hour, tm.tm_min);
    }
    let hour = if tm.tm_hour % 12 == 0 { 12 } else { tm.tm_hour % 12 };
    let half = crate::i18n::tr(if tm.tm_hour < 12 { "@string.time.AM|AM" } else { "@string.time.PM|PM" });
    format!("{hour} {half}")
}

/// Stock's weekday abbreviations, in the language in use.
const DAYS: [&str; 7] = ["@string.time.sun|Sun", "@string.time.mon|Mon", "@string.time.tue|Tue", "@string.time.wed|Wed",
                         "@string.time.thu|Thu", "@string.time.fri|Fri", "@string.time.sat|Sat"];

/// A card's title (stock's "WED OCT 14").
pub fn title(date: i64) -> String {
    let tm = local(date);
    let name = |key: &str| crate::i18n::tr(key).to_uppercase();
    format!("{} {} {}", name(DAYS[tm.tm_wday as usize % 7]), name(MONTHS[tm.tm_mon as usize % 12]), tm.tm_mday)
}

/// Days shown, newest first (stock's carousel takes ten).
pub const SHOWN: usize = 10;

/// The History screen while it is open (stock's nlEnergyView): a row of
/// slots, the live thermostat at each end and the days between, oldest
/// on the left, flung by the wheel as stock's strip (0x135aa4); it opens
/// on yesterday. A press on a day opens its detail, which scrolls; a
/// press there goes back.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    /// 0 is the left thermostat, 1..=days the days oldest first, days + 1
    /// the right thermostat.
    pub at: usize,
    pub days: usize,
    /// The detail page's scroll, in steps, while it is open.
    pub detail: Option<u8>,
    strip: crate::scroll::Scroll,
    /// Clicks due: one each slot the cards come to (stock's 0x135aa4).
    pub clicks: u32,
}

/// How far the detail page scrolls, in wheel steps.
pub const DETAIL_STEPS: u8 = 2;

pub enum Pressed {
    Stay,
    Closed,
}

impl View {
    pub fn open(days: usize) -> View {
        let strip = crate::scroll::Scroll::new(crate::scroll::HISTORY, days + 2, days);
        View { at: days, days, detail: None, strip, clicks: 0 }
    }

    /// Where the cards are, in slots.
    pub fn position(&self) -> f32 {
        self.strip.position()
    }

    /// A wheel event's counts (the gen 3's): they fling the cards, not
    /// the detail page.
    pub fn counts(&mut self, counts: i32, now: Instant) -> bool {
        if self.detail.is_some() {
            return false;
        }
        self.strip.wheel(counts, now);
        self.follow();
        true
    }

    /// Whole detents, from a wheel that gives no counts.
    pub fn wheel(&mut self, detents: i32, now: Instant) {
        match &mut self.detail {
            Some(s) => *s = (i32::from(*s) + detents).clamp(0, i32::from(DETAIL_STEPS)) as u8,
            None => {
                for _ in 0..detents.unsigned_abs() {
                    self.strip.wheel(-120 * detents.signum(), now);
                }
                self.follow();
            }
        }
    }

    /// Moves the flung cards on to `now`; whether they still move.
    pub fn tick(&mut self, now: Instant) -> bool {
        let moving = self.strip.step(now);
        self.follow();
        moving
    }

    fn follow(&mut self) {
        let at = self.strip.index();
        if at != self.at {
            self.at = at;
            self.clicks += 1;
        }
    }

    /// A press stops the cards and glides them to the nearest slot (stock's
    /// 500 ms), then opens its day or, on a thermostat, closes.
    pub fn press(&mut self, now: Instant) -> Pressed {
        if self.detail.take().is_some() {
            return Pressed::Stay;
        }
        self.strip.glide(now);
        if self.at == 0 || self.at > self.days {
            return Pressed::Closed;
        }
        self.detail = Some(0);
        Pressed::Stay
    }

    /// The day at the cursor, as an index into newest-first days.
    pub fn day(&self) -> Option<usize> {
        (1..=self.days).contains(&self.at).then(|| self.days - self.at)
    }
}

/// A run's length as stock writes it (0x132ea8): up to 45 minutes in
/// quarters ("45 MIN"), then hours and quarters ("2½ HR"), rounded up.
pub fn duration(seconds: u32) -> String {
    let quarters = seconds.div_ceil(900);
    if seconds < 2701 {
        return format!("{} MIN", quarters * 15);
    }
    let (hours, q) = (quarters / 4, quarters % 4);
    format!("{hours}{} HR", ["", "¼", "½", "¾"][q as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_opens_on_yesterday_and_presses_open_a_day_or_close_at_the_ends() {
        let now = Instant::now();
        let settle = |v: &mut View, from: Instant| {
            let mut t = from;
            for _ in 0..300 {
                t += std::time::Duration::from_millis(17);
                v.tick(t);
            }
            t
        };
        let mut v = View::open(3);
        assert_eq!((v.day(), v.position()), (Some(0), 3.0));
        // Detents 30 ms apart fling the cards on a slot, a click.
        let mut t = now;
        for _ in 0..6 {
            t += std::time::Duration::from_millis(30);
            v.counts(120, t);
        }
        let t = settle(&mut v, t);
        assert_eq!((v.day(), v.position(), v.clicks), (Some(1), 2.0, 1));
        assert!(matches!(v.press(t), Pressed::Stay) && v.detail == Some(0));
        assert!(!v.counts(120, t), "the detail takes detents");
        v.wheel(9, t);
        assert_eq!(v.detail, Some(DETAIL_STEPS));
        assert!(matches!(v.press(t), Pressed::Stay) && v.detail.is_none());
        v.wheel(-30, t);
        let t = settle(&mut v, t);
        assert_eq!((v.at, v.day()), (0, None), "held at the end");
        assert!(matches!(v.press(t), Pressed::Closed));
    }

    #[test]
    fn short_dates_put_the_day_where_the_language_does() {
        assert_eq!(day_month(17, 9, "en_US"), "Oct 17");
        assert_eq!(day_month(24, 8, "en_GB"), "24 Sep");
        assert_eq!(day_month(3, 0, "fr_FR"), "3 Jan");
        assert_eq!(day_month(5, 11, "de_DE"), "5. Dec");
        assert_eq!(day_month(9, 4, "es_US"), "9 May");
        // 2019-09-24 22:52:16 UTC, on the day local time gives it.
        assert_eq!(short_date(1_569_365_536, "en_US"), format!("Sep {}", local(1_569_365_536).tm_mday));
    }

    #[test]
    fn farsights_date_lines_name_the_weekday_and_the_day() {
        let at = 1_569_365_536;
        let tm = local(at);
        let (day, date) = (["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"][tm.tm_wday as usize], tm.tm_mday);
        assert_eq!(weekday_date(at, "en_US"), format!("{day}, September {date}"));
        assert_eq!(weekday_date(at, "en_GB"), format!("{day}, {date} September"));
        assert_eq!(weekday_day(at), format!("{} {date}", day[..3].to_uppercase()));
        let hour = tm.tm_hour;
        let half = if hour < 12 { "AM" } else { "PM" };
        assert_eq!(forecast_time(at, true), format!("{} {half}", if hour % 12 == 0 { 12 } else { hour % 12 }));
        assert_eq!(forecast_time(at, false), format!("{hour:02}:{:02}", tm.tm_min));
    }

    #[test]
    fn messages_follow_the_reason_and_the_weekly_average() {
        let d = Day { heat: 3600, above_by: 3600, reason: ECO, ..Day::default() };
        assert_eq!(message(&d), "Eco Temperatures caused energy usage above the weekly average.");
        assert_eq!(message(&Day { above_by: -1800, reason: WEATHER, ..d.clone() }), "Energy usage was below the weekly average due to weather.");
        assert_eq!(message(&Day { above_by: 2, ..d.clone() }), "Energy usage was around the weekly average.");
        assert_eq!(message(&Day { heat: 0, ..d.clone() }), "No energy usage.");
        assert_eq!(message(&Day { missing: 50400, ..d }), "14 hours of information are missing for this day.");
        assert_eq!((exact(20700), exact(2700), exact(7200)), ("5 HR 45 MIN".into(), "45 MIN".into(), "2 HR".into()));
    }

    /// Eight days of 4 h of heat at 20 °C, scheduled 21 °C, newest first,
    /// with `today` in front.
    fn week(today: Day) -> History {
        let usual = Day { heat: 4 * 3600, act: Targets { heat_time: 86400, heat: 20.0, ..Targets::default() },
                          sch: Targets { heat_time: 86400, heat: 21.0, ..Targets::default() }, reason: -1, ..Day::default() };
        let mut h = History::default();
        h.days.push(today);
        h.days.extend(std::iter::repeat_n(usual, 8));
        h.reason();
        h
    }

    #[test]
    fn a_day_held_lower_than_last_week_below_its_schedule_is_put_down_to_manual() {
        let base = Day { act: Targets { heat_time: 86400, heat: 19.0, ..Targets::default() },
                         sch: Targets { heat_time: 86400, heat: 21.0, ..Targets::default() }, ..Day::default() };
        let h = week(Day { heat: 3600, ..base.clone() });
        assert_eq!((h.days[0].reason, h.days[0].above_by), (MANUAL, -3 * 3600), "unusual, 1 °C under last week");
        let h = week(Day { heat: 4 * 3600 - 600, weather: 70000, ..base.clone() });
        assert_eq!(h.days[0].reason, MANUAL, "usual too, by more than 0.28 °C");
        let h = week(Day { heat: 4 * 3600 - 600, weather: 70000, act: Targets { heat_time: 86400, heat: 19.9, ..Targets::default() }, ..base });
        assert_eq!(h.days[0].reason, -1, "a usual day is never put down to the weather");
    }

    #[test]
    fn durations_round_up_to_stocks_quarters() {
        assert_eq!(duration(60), "15 MIN");
        assert_eq!(duration(2700), "45 MIN");
        assert_eq!(duration(2701), "1 HR");
        assert_eq!(duration(4600), "1½ HR");
        assert_eq!(duration(8000), "2¼ HR");
    }

    #[test]
    fn a_day_closes_at_midnight_and_yesterday_shows_its_heat() {
        let mut h = History::default();
        let (day, _) = midnight(1_790_000_000.0);
        let at = |d: i64, s: f64| d as f64 + s;
        // The first day seen is dropped.
        h.sample(at(day, 80000.0), Sample::default());
        h.sample(at(day + 86400, 10.0), Sample::default());
        assert!(h.days.is_empty());
        let mut t = at(day + 86400, 3600.0);
        h.sample(t, Sample::default());
        for _ in 0..150 {
            t += 30.0;
            h.sample(t, Sample { heat: true, ..Sample::default() });
        }
        h.sample(at(day + 2 * 86400, 5.0), Sample::default());
        assert_eq!(h.days.len(), 1);
        assert_eq!(h.days[0].heat, 4500);
        assert_eq!(h.days[0].cycles.len(), 1);
        assert_eq!(h.yesterday(), "Yesterday:\n1¼ hr of heat");
    }
}
