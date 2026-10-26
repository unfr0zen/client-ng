//! Furnace shutdown detection, as stock nlclient's nlFurnaceShutdown runs
//! it (0x3b0728 each update, 0x3afa8c on a cycle's end or a power loss,
//! 0x3afd7c deciding, 0x3b01ac alerting) over three nlFurnaceCycleTracker
//! rings (0x4103b0–0x410548): the last 50 heat cycles, the last 20 of them
//! that cost the power, and the last 10 power losses no cycle explains. A
//! furnace that keeps shutting down within 15 minutes of heating, its
//! safety cutting the thermostat's power, raises the FurnaceShutdown alert
//! (0x1f) and `is_furnace_shutdown` (globals vtable +0x458, 0xbdd60).

use std::collections::VecDeque;

/// A power loss this soon after a heat cycle ended is that cycle's doing
/// (s); one later than `UNEXPLAINED` is a normal power loss.
const PROVOKED: f64 = 300.0;
const UNEXPLAINED: f64 = 1200.0;
/// The data a verdict needs: four days of it (s), full rings of heat and
/// provoking cycles, no cycle over 900 s, fewer than ten normal power
/// losses within four days, more than ten provoking cycles longer than
/// the provoking ones' mean, and those at least 4/5 of all the cycles
/// longer than that mean.
const DAYS: f64 = 345_600.0;
const HEAT: usize = 50;
const PROVOKING: usize = 20;
const LOSSES: usize = 10;
const LONGEST: f64 = 900.0;
const LONGER: usize = 10;
/// An alert stands a week (s), and comes at most twice a year (s).
const STANDS: f64 = 604_800.0;
const AGAIN: f64 = 15_768_000.0;

/// Stock's verdicts (0x3afd7c).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    TooSoon = 1,
    FewCycles = 2,
    FewProvoking = 3,
    NormalLosses = 4,
    FewLonger = 5,
    RatioLow = 6,
    LongCycle = 7,
    Shutdown = 8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Event {
    /// When it ended, how long it ran (s), and whether it cost the power.
    at: f64,
    duration: f64,
    provoking: bool,
}

/// A tracker: what it follows on or off and since when, its last events,
/// newest first, and the count and summed duration of those that cost
/// the power.
#[derive(Debug, PartialEq)]
struct Ring {
    capacity: usize,
    on: bool,
    on_at: f64,
    off_at: f64,
    events: VecDeque<Event>,
    provoking: usize,
    provoking_sum: f64,
}

impl Ring {
    fn new(capacity: usize) -> Ring {
        Ring { capacity, on: false, on_at: 0.0, off_at: 0.0, events: VecDeque::new(), provoking: 0, provoking_sum: 0.0 }
    }

    /// The state now; true when it went off (0x4103d0).
    fn update(&mut self, on: bool, now: f64) -> bool {
        let ended = self.on && !on;
        if on && !self.on {
            self.on_at = now;
        }
        if ended {
            self.off_at = now;
        }
        self.on = on;
        ended
    }

    /// The run that just ended (0x4104e8).
    fn ended(&self) -> Event {
        Event { at: self.off_at, duration: self.off_at - self.on_at, provoking: false }
    }

    /// An event in, the oldest out when full (0x410424).
    fn push(&mut self, e: Event) {
        if self.events.len() == self.capacity {
            if let Some(old) = self.events.pop_back().filter(|o| o.provoking) {
                self.provoking -= 1;
                self.provoking_sum -= old.duration;
            }
        }
        if e.provoking {
            self.provoking += 1;
            self.provoking_sum += e.duration;
        }
        self.events.push_front(e);
    }

    fn save(&self, out: &mut Vec<String>) {
        out.extend([u8::from(self.on).to_string(), self.on_at.to_string(), self.off_at.to_string(), self.events.len().to_string()]);
        for e in &self.events {
            out.extend([e.at.to_string(), e.duration.to_string(), u8::from(e.provoking).to_string()]);
        }
    }

    fn load(&mut self, words: &mut impl Iterator<Item = f64>) -> Option<()> {
        *self = Ring::new(self.capacity);
        self.on = words.next()? != 0.0;
        self.on_at = words.next()?;
        self.off_at = words.next()?;
        let n = words.next()? as usize;
        let mut events = Vec::new();
        for _ in 0..n.min(self.capacity) {
            events.push(Event { at: words.next()?, duration: words.next()?, provoking: words.next()? != 0.0 });
        }
        for e in events.into_iter().rev() {
            self.push(e);
        }
        Some(())
    }
}

/// What the last event was (stock's +0xb0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Last {
    None = 0,
    CycleEnded = 1,
    PowerLost = 2,
}

/// Furnace shutdown detection: the heat cycles, those that cost the power,
/// the normal power losses (its tracker the HVAC power's), the last
/// event, when it started, when it last alerted and whether that alert
/// stands (stock's +0xf8: 0 never, 1 standing, 2 cleared).
#[derive(Debug, PartialEq)]
pub struct Furnace {
    heat: Ring,
    provoking: Ring,
    power: Ring,
    last: Last,
    since: f64,
    alerted_at: f64,
    alert: u8,
}

impl Default for Furnace {
    fn default() -> Furnace {
        Furnace { heat: Ring::new(HEAT), provoking: Ring::new(PROVOKING), power: Ring::new(LOSSES), last: Last::None, since: 0.0, alerted_at: 0.0, alert: 0 }
    }
}

impl Furnace {
    /// One update at `now` (wall seconds): the furnace heating, the HVAC
    /// power there. Some(true) when the alert rises, Some(false) when it
    /// goes.
    pub fn update(&mut self, now: f64, heating: bool, powered: bool) -> Option<bool> {
        if self.since == 0.0 {
            self.since = now;
        }
        let ended = self.heat.update(heating, now);
        if ended {
            self.event(Last::CycleEnded);
        }
        let lost = self.power.update(powered, now);
        if lost {
            self.event(Last::PowerLost);
        }
        self.alerting(now, ended || lost)
    }

    /// A heat cycle ended or the power was lost (0x3afa8c). A loss within
    /// 300 s of the last cycle's end after that cycle marks it provoking;
    /// one more than 1200 s after it, or with none before it, is a normal
    /// power loss.
    fn event(&mut self, kind: Last) {
        let lost_at = self.power.off_at;
        match (self.last, kind) {
            (Last::CycleEnded, Last::PowerLost) => {
                let Some(c) = self.heat.events.front_mut() else {
                    warn!("hvac: furnace shutdown: no heat cycle before the power loss");
                    self.last = kind;
                    return;
                };
                let after = lost_at - c.at;
                if after < PROVOKED {
                    c.provoking = true;
                    let c = *c;
                    self.heat.provoking += 1;
                    self.heat.provoking_sum += c.duration;
                    self.provoking.push(c);
                    debug!("hvac: furnace shutdown: power lost {after:.0} s after a {:.0} s heat cycle", c.duration);
                } else if after > UNEXPLAINED {
                    let e = self.power.ended();
                    self.power.push(e);
                }
            }
            (_, Last::PowerLost) => {
                if self.heat.events.front().is_none_or(|c| lost_at - c.at > UNEXPLAINED) {
                    let e = self.power.ended();
                    self.power.push(e);
                }
            }
            _ => {
                let e = self.heat.ended();
                self.heat.push(e);
            }
        }
        self.last = kind;
    }

    /// The heat cycles that cost the power, those longer than the
    /// provoking cycles' mean, and those of them that cost it (0x3afc7c).
    fn counts(&self) -> (usize, usize, usize) {
        let (n, sum) = (self.provoking.events.len() as f64, self.provoking.provoking_sum);
        let longer = |e: &&Event| n * e.duration > sum;
        let cycles = &self.heat.events;
        (cycles.iter().filter(|e| e.provoking).count(), cycles.iter().filter(longer).count(),
         cycles.iter().filter(longer).filter(|e| e.provoking).count())
    }

    /// The verdict at `now` (0x3afd7c).
    pub fn verdict(&self, now: f64) -> Verdict {
        let (_, longer, provoking_longer) = self.counts();
        let longest = self.heat.events.iter().map(|e| e.duration).fold(0.0, f64::max);
        let normal_losses = self.power.events.len() == LOSSES && self.power.events.back().is_some_and(|e| now - e.at <= DAYS);
        if now - self.since < DAYS {
            Verdict::TooSoon
        } else if self.heat.events.len() < HEAT {
            Verdict::FewCycles
        } else if self.provoking.events.len() < PROVOKING {
            Verdict::FewProvoking
        } else if longest > LONGEST {
            Verdict::LongCycle
        } else if normal_losses {
            Verdict::NormalLosses
        } else if provoking_longer <= LONGER {
            Verdict::FewLonger
        } else if provoking_longer * 5 < longer * 4 {
            Verdict::RatioLow
        } else {
            Verdict::Shutdown
        }
    }

    /// The alert after an update (0x3b01ac): a standing one goes after a
    /// week; on an event, a shutdown verdict raises it, half a year after
    /// the last.
    fn alerting(&mut self, now: f64, event: bool) -> Option<bool> {
        let since = now - self.alerted_at;
        if self.alert == 1 {
            if since > STANDS {
                info!("hvac: furnace shutdown alert stood a week");
                self.alert = 2;
                return Some(false);
            }
            return None;
        }
        if !event {
            return None;
        }
        let v = self.verdict(now);
        let (provoking, longer, provoking_longer) = self.counts();
        debug!("hvac: furnace shutdown: {v:?}: {} heat cycles, {provoking} provoking ({:.0} s), {} normal power losses, {provoking_longer} of {longer} longer than the mean provoking",
               self.heat.events.len(), self.provoking.provoking_sum, self.power.events.len());
        if v == Verdict::Shutdown && since > AGAIN {
            info!("hvac: furnace shutdown: {provoking_longer}/{longer} cycles longer than the mean cost the power");
            self.alerted_at = now;
            self.alert = 1;
            return Some(true);
        }
        None
    }

    /// The flag at start, from the data kept (0x3b044c): a shutdown
    /// verdict, without the alert.
    pub fn start(&self, now: f64) -> bool {
        self.verdict(now) == Verdict::Shutdown
    }

    /// The detection's data, as kept across restarts (stock's
    /// `furnaceshutdownrecoveryfile`).
    pub fn save(&self) -> String {
        let mut out = vec!["1".to_string(), (self.last as u8).to_string(), self.since.to_string(), self.alerted_at.to_string(), self.alert.to_string()];
        for r in [&self.heat, &self.provoking, &self.power] {
            r.save(&mut out);
        }
        out.join(" ")
    }

    /// Takes the data back; anything unreadable starts afresh.
    pub fn load(&mut self, text: &str) {
        let mut words = text.split_whitespace().map(|w| w.parse::<f64>().unwrap_or(f64::NAN));
        let mut f = Furnace::default();
        let read = (|| {
            if words.next()? != 1.0 {
                return None;
            }
            f.last = match words.next()? as u8 { 1 => Last::CycleEnded, 2 => Last::PowerLost, _ => Last::None };
            f.since = words.next()?;
            f.alerted_at = words.next()?;
            f.alert = words.next()? as u8;
            for r in [&mut f.heat, &mut f.provoking, &mut f.power] {
                r.load(&mut words)?;
            }
            Some(())
        })();
        if read.is_some() && text.split_whitespace().all(|w| w.parse::<f64>().is_ok_and(f64::is_finite)) {
            *self = f;
        } else {
            warn!("hvac: furnace shutdown data unreadable; starting afresh");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A heat cycle of `run` s from `at`, then, when `after` is given, the
    /// power lost that long after it for 10 s; the time after.
    fn cycle(f: &mut Furnace, at: f64, run: f64, after: Option<f64>) -> (f64, Option<bool>) {
        let mut alert = f.update(at, true, true);
        let end = at + run;
        alert = alert.or(f.update(end, false, true));
        match after {
            Some(a) => {
                alert = alert.or(f.update(end + a, false, false));
                alert = alert.or(f.update(end + a + 10.0, false, true));
                (end + a + 10.0, alert)
            }
            None => (end, alert),
        }
    }

    /// Four days and more of short heat cycles, every one costing the
    /// power within a minute, and the time after.
    fn shutting_down(f: &mut Furnace) -> (f64, Option<bool>) {
        let mut t = 1_700_000_000.0;
        f.update(t, false, true);
        let mut alert = None;
        for i in 0..60 {
            // Runs of 400 s and 500 s: the longer ones beat the mean.
            let (next, a) = cycle(f, t + 3600.0, if i % 2 == 0 { 400.0 } else { 500.0 }, Some(60.0));
            alert = alert.or(a);
            t = next + 2.0 * 3600.0;
        }
        (t, alert)
    }

    #[test]
    fn a_furnace_cut_off_by_its_safety_cycle_after_cycle_is_shutting_down() {
        let mut f = Furnace::default();
        let (t, alert) = shutting_down(&mut f);
        assert_eq!(alert, Some(true), "{:?}", f.verdict(t));
        assert_eq!(f.verdict(t), Verdict::Shutdown);
        // Once: a week later it goes, and the next comes half a year on.
        let (t, again) = cycle(&mut f, t, 450.0, Some(60.0));
        assert_eq!(again, None);
        assert_eq!(f.update(t + STANDS + 1.0, false, true), Some(false));
        assert_eq!(cycle(&mut f, t + STANDS + 100.0, 450.0, Some(60.0)).1, None, "not again within half a year");
    }

    #[test]
    fn too_little_data_or_a_long_cycle_is_no_shutdown() {
        let mut f = Furnace::default();
        let t = 1_700_000_000.0;
        f.update(t, false, true);
        let (t, alert) = cycle(&mut f, t + 10.0, 300.0, Some(60.0));
        assert_eq!(alert, None);
        assert_eq!(f.verdict(t), Verdict::TooSoon);
        assert_eq!(f.verdict(t + DAYS), Verdict::FewCycles);
        let mut f = Furnace::default();
        let (t, _) = shutting_down(&mut f);
        let (t, _) = cycle(&mut f, t, 1000.0, Some(60.0));
        assert_eq!(f.verdict(t), Verdict::LongCycle);
    }

    #[test]
    fn cycles_that_keep_the_power_are_not_provoking() {
        let mut f = Furnace::default();
        let mut t = 1_700_000_000.0;
        f.update(t, false, true);
        for _ in 0..60 {
            t = cycle(&mut f, t + 7200.0, 450.0, None).0;
        }
        assert_eq!(f.verdict(t), Verdict::FewProvoking);
        // A power loss long after a cycle is a normal one.
        let (t, _) = cycle(&mut f, t + 100.0, 450.0, Some(1500.0));
        assert_eq!((f.power.events.len(), f.provoking.events.len()), (1, 0), "{t}");
    }

    #[test]
    fn the_data_is_kept_across_restarts() {
        let mut f = Furnace::default();
        let (t, _) = shutting_down(&mut f);
        let mut back = Furnace::default();
        back.load(&f.save());
        assert_eq!(back, f);
        assert!(back.start(t));
        let mut bad = Furnace::default();
        bad.load("1 2 x");
        assert_eq!(bad, Furnace::default());
    }
}
