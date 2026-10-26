//! Short-cycle detection, as stock nlclient's nlHvacShortCycle runs it
//! (0x3b0cd8 each update, 0x3b0af8 deciding) over a heat and a cool
//! nlShortCycleTracker (0x3fa4dc–0x3fa7d0): ten heating or cooling cycles
//! in a row, each under 90 s and each followed within 300 s by the HVAC
//! power going, are short cycling; so are twelve power losses within
//! 1799 s. Stock only logs the decision (its HvacShortCyclingDecision
//! event): what shows the alert is the cloud's `hvac_cycle_status`.

use std::collections::VecDeque;

/// A tracker's cycles: kept, and needed for a verdict.
const EVENTS: usize = 10;
/// The longest gap between one cycle's end and the next one's start for
/// both to count in a row (s).
const GAP: f64 = 270.0;
/// Cycles shorter than this are short (s).
const SHORT: f64 = 90.0;
/// Cycles that must have cost the power for short cycling.
const PROVOKING: usize = 10;
/// A power loss this soon after a cycle ended is that cycle's doing (s).
const WINDOW: f64 = 300.0;
/// Power losses kept; this many within `UNSTABLE` s are an unstable power
/// source (0x3b0834).
const LOSSES: usize = 12;
const UNSTABLE: f64 = 1799.0;
/// How long an alert stands without a decision taking it down: for the
/// power source, and for a heating or cooling system (s).
const POWER_TIMEOUT: f64 = 1810.0;
const CYCLE_TIMEOUT: f64 = 14400.0;

/// What a tracker makes of its cycles (0x3fa660), in stock's numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Fewer than ten cycles in a row.
    Insufficient = 0,
    /// Ten short cycles, not all of them costing the power.
    ShortCycles = 1,
    /// None ended within the last 270 s.
    Idle = 2,
    /// One of them ran 90 s or more.
    Normal = 3,
    ShortCycling = 4,
}

/// What a decision was about: the power source, heating or cooling (the
/// decision's aHvacMode).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Power = 0,
    Heat = 1,
    Cool = 2,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Event {
    /// When it ended, how long it ran (s), and whether a power loss
    /// followed it.
    at: f64,
    duration: f64,
    provoking: bool,
}

/// Runs of one thing being on (stock's cycle tracker, 0x4103d0 and
/// 0x410424): its state, the last start and end, the last runs, newest
/// first, and how many of them cost the power.
#[derive(Debug)]
struct Runs {
    capacity: usize,
    on: Option<bool>,
    on_at: f64,
    off_at: f64,
    events: VecDeque<Event>,
    provoking: usize,
}

impl Runs {
    fn new(capacity: usize) -> Runs {
        Runs { capacity, on: None, on_at: 0.0, off_at: 0.0, events: VecDeque::new(), provoking: 0 }
    }

    /// The state now; true when a run ended.
    fn update(&mut self, on: bool, now: f64) -> bool {
        let was = self.on.replace(on);
        if was == Some(true) {
            if !on {
                self.off_at = now;
            }
            !on
        } else {
            if on {
                self.on_at = now;
            }
            false
        }
    }

    /// The run that just ended, the oldest making way when full.
    fn push_ended(&mut self) {
        if self.events.len() == self.capacity && self.events.pop_back().is_some_and(|e| e.provoking) {
            self.provoking -= 1;
        }
        self.events.push_front(Event { at: self.off_at, duration: self.off_at - self.on_at, provoking: false });
    }

    fn clear(&mut self) {
        *self = Runs::new(self.capacity);
    }

    fn longest(&self) -> f64 {
        self.events.iter().map(|e| e.duration).fold(0.0, f64::max)
    }
}

/// One side's cycles in a row (nlShortCycleTracker).
#[derive(Debug)]
struct Tracker {
    runs: Runs,
}

impl Tracker {
    fn new() -> Tracker {
        Tracker { runs: Runs::new(EVENTS) }
    }

    /// The side on or not; true when a cycle ended (0x3fa350, 0x3fa4dc).
    /// A cycle starting more than 270 s after the last one ended begins a
    /// new run of cycles. (Stock's clear also zeroes the start and end it
    /// then records, so every later cycle starts a new run as well; here
    /// the cycle that ended is the new run's first.)
    fn update(&mut self, on: bool, now: f64) -> bool {
        if !self.runs.update(on, now) {
            return false;
        }
        if self.runs.events.front().is_some_and(|e| self.runs.on_at - e.at > GAP) {
            debug!("hvac: short cycle: {:.0} s since the last cycle, counting anew", self.runs.on_at - self.runs.events[0].at);
            self.runs.events.clear();
            self.runs.provoking = 0;
        }
        self.runs.push_ended();
        true
    }

    /// The power went: the last cycle's doing if it ended within 300 s
    /// (0x3fa5b0).
    fn power_lost(&mut self, now: f64) -> bool {
        let Some(e) = self.runs.events.front_mut() else { return false };
        if now - e.at >= WINDOW || e.provoking {
            return false;
        }
        e.provoking = true;
        self.runs.provoking += 1;
        debug!("hvac: short cycle: power lost {:.0} s after a {:.0} s cycle", now - e.at, e.duration);
        true
    }

    fn verdict(&self, now: f64) -> Verdict {
        let r = &self.runs;
        let since = if r.events.is_empty() { 0.0 } else { now - r.off_at };
        if r.events.len() < EVENTS {
            Verdict::Insufficient
        } else if since > GAP {
            Verdict::Idle
        } else if r.longest() >= SHORT {
            Verdict::Normal
        } else if r.provoking < PROVOKING {
            Verdict::ShortCycles
        } else {
            Verdict::ShortCycling
        }
    }

    fn reset(&mut self) {
        self.runs.clear();
    }
}

/// Short-cycle detection over the heating, the cooling and the HVAC
/// power: the decision (none at first), when it was last made and the
/// side the standing alert is about.
#[derive(Debug)]
pub struct ShortCycle {
    heat: Tracker,
    cool: Tracker,
    power: Runs,
    decision: Option<Verdict>,
    since: f64,
    alert_side: Option<Side>,
}

impl Default for ShortCycle {
    fn default() -> ShortCycle {
        ShortCycle { heat: Tracker::new(), cool: Tracker::new(), power: Runs::new(LOSSES), decision: None, since: 0.0, alert_side: None }
    }
}

impl ShortCycle {
    /// Whether the system is short cycling now.
    pub fn short_cycling(&self) -> bool {
        self.decision == Some(Verdict::ShortCycling)
    }

    /// One update (0x3b0cd8): heating or cooling on and the HVAC power
    /// there, at `now` seconds. Heating is a first stage, alternate or
    /// auxiliary heat switch on; cooling the first cool stage.
    pub fn update(&mut self, now: f64, heating: bool, cooling: bool, powered: bool) {
        let lost = self.power.update(powered, now);
        let heat_ended = self.heat.update(heating, now);
        let cool_ended = self.cool.update(cooling, now);
        if lost {
            self.power.push_ended();
            self.heat.power_lost(now);
            self.cool.power_lost(now);
        }
        self.decide(now, heat_ended, cool_ended, lost);
    }

    /// Twelve power losses within 1799 s are short cycling; otherwise the
    /// decision stands (0x3b0834).
    fn unstable(&self, now: f64) -> Option<Verdict> {
        if self.short_cycling() || self.power.events.len() < LOSSES {
            return self.decision;
        }
        let period = now - self.power.events[LOSSES - 1].at;
        if period > UNSTABLE {
            return self.decision;
        }
        debug!("hvac: short cycle: {LOSSES} power losses in {period:.0} s: unstable power source");
        Some(Verdict::ShortCycling)
    }

    /// The decision after an update (0x3b0af8).
    fn decide(&mut self, now: f64, heat_ended: bool, cool_ended: bool, lost: bool) {
        let (new, side) = if heat_ended {
            (Some(self.heat.verdict(now)), Side::Heat)
        } else if cool_ended {
            (Some(self.cool.verdict(now)), Side::Cool)
        } else if lost {
            let (h, c) = (self.heat.verdict(now), self.cool.verdict(now));
            if h == Verdict::ShortCycling || (c != Verdict::ShortCycling && h == Verdict::Normal) {
                (Some(h), Side::Heat)
            } else if matches!(c, Verdict::Normal | Verdict::ShortCycling) {
                (Some(c), Side::Cool)
            } else {
                (self.unstable(now), Side::Power)
            }
        } else {
            // Nothing new: a standing alert goes after its timeout.
            let timeout = if self.alert_side == Some(Side::Power) { POWER_TIMEOUT } else { CYCLE_TIMEOUT };
            if self.short_cycling() && now - self.since > timeout {
                info!("hvac: short cycling timed out after {timeout:.0} s ({:?})", self.alert_side.unwrap_or(Side::Power));
                self.since = now;
                self.decision = None;
                self.alert_side = None;
                self.reset();
            }
            return;
        };
        if new == self.decision {
            return;
        }
        let old = self.decision;
        if !self.short_cycling() {
            self.decision = new;
            if self.short_cycling() {
                self.since = now;
                self.alert_side = Some(side);
                info!("hvac: the system is short cycling ({side:?})");
            }
            debug!("hvac: short cycle decision {side:?}: {old:?} -> {new:?}");
            return;
        }
        // The alert stands: a decision that the system recovered takes it
        // down. An idle tracker keeps it; too few cycles do once the last
        // one ran long; short cycles that no longer cost the power once
        // 390 s have passed since the alert.
        let tracker = match side {
            Side::Heat => Some(&self.heat),
            Side::Cool => Some(&self.cool),
            Side::Power => None,
        };
        let recovered = match new {
            Some(Verdict::Idle) => false,
            Some(Verdict::Insufficient) => tracker.and_then(|t| t.runs.events.front()).is_some_and(|e| e.duration > SHORT),
            Some(Verdict::ShortCycles) => now - self.since >= tracker.map_or(0.0, |_| SHORT + WINDOW),
            _ => true,
        };
        if !recovered {
            debug!("hvac: short cycle decision {side:?}: {new:?}, alert stands");
            return;
        }
        self.decision = new;
        self.since = now;
        self.alert_side = None;
        if new == Some(Verdict::Normal) {
            self.reset();
        }
        info!("hvac: the system recovered from short cycling ({side:?}, {new:?})");
    }

    /// Everything forgotten (0x3b07d8).
    fn reset(&mut self) {
        self.heat.reset();
        self.cool.reset();
        self.power.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A heating cycle of `run` s from `at`, then the power lost `after`
    /// s after it ended for `out` s (none when `after` is None); the time
    /// after.
    fn cycle(s: &mut ShortCycle, at: f64, run: f64, after: Option<f64>) -> f64 {
        s.update(at, false, false, true);
        s.update(at + 1.0, true, false, true);
        s.update(at + 1.0 + run, false, false, true);
        let end = at + 1.0 + run;
        match after {
            Some(a) => {
                s.update(end + a, false, false, false);
                s.update(end + a + 5.0, false, false, true);
                end + a + 5.0
            }
            None => end,
        }
    }

    #[test]
    fn ten_short_cycles_each_losing_the_power_are_short_cycling() {
        let mut s = ShortCycle::default();
        let mut t = 0.0;
        for i in 0..10 {
            assert!(!s.short_cycling(), "not before the tenth (cycle {i})");
            t = cycle(&mut s, t + 60.0, 60.0, Some(20.0));
        }
        assert!(s.short_cycling());
        assert_eq!(s.alert_side, Some(Side::Heat));
    }

    #[test]
    fn short_cycles_without_power_losses_are_not() {
        let mut s = ShortCycle::default();
        let mut t = 0.0;
        for _ in 0..12 {
            t = cycle(&mut s, t + 60.0, 60.0, None);
        }
        assert!(!s.short_cycling());
        assert_eq!(s.decision, Some(Verdict::ShortCycles));
    }

    #[test]
    fn a_loss_past_300_s_or_a_long_cycle_does_not_count() {
        let mut s = ShortCycle::default();
        let mut t = 0.0;
        for _ in 0..10 {
            t = cycle(&mut s, t + 10.0, 60.0, Some(250.0));
        }
        assert!(s.short_cycling(), "within 300 s");
        // The last loss 300 s after the end: not that cycle's.
        let mut s = ShortCycle::default();
        let mut t = 0.0;
        for i in 0..10 {
            t = cycle(&mut s, t + 10.0, 60.0, Some(if i == 9 { 300.0 } else { 20.0 }));
        }
        assert!(!s.short_cycling(), "300 s is past the window");
        assert_eq!(s.decision, Some(Verdict::ShortCycles));
        // One cycle of 90 s: the cycles are normal.
        let mut s = ShortCycle::default();
        let mut t = 0.0;
        for i in 0..10 {
            t = cycle(&mut s, t + 10.0, if i == 4 { 90.0 } else { 60.0 }, Some(20.0));
        }
        assert!(!s.short_cycling());
        assert_eq!(s.decision, Some(Verdict::Normal));
    }

    #[test]
    fn a_gap_over_270_s_starts_counting_anew() {
        let mut s = ShortCycle::default();
        let mut t = 0.0;
        for _ in 0..9 {
            t = cycle(&mut s, t + 60.0, 60.0, Some(20.0));
        }
        t = cycle(&mut s, t + 300.0, 60.0, Some(20.0));
        assert!(!s.short_cycling(), "the tenth came after a gap");
        for _ in 0..8 {
            t = cycle(&mut s, t + 60.0, 60.0, Some(20.0));
        }
        assert!(!s.short_cycling(), "nine in the new run");
        cycle(&mut s, t + 60.0, 60.0, Some(20.0));
        assert!(s.short_cycling(), "ten in the new run");
    }

    #[test]
    fn twelve_power_losses_within_1799_s_are_an_unstable_source() {
        let mut s = ShortCycle::default();
        s.update(0.0, false, false, true);
        for i in 0..12 {
            let t = 1.0 + f64::from(i) * 150.0;
            s.update(t, false, false, false);
            s.update(t + 10.0, false, false, true);
        }
        assert!(s.short_cycling());
        assert_eq!(s.alert_side, Some(Side::Power));
        // Nothing more: it times out after 1810 s, not before.
        s.update(1651.0 + 1810.0, false, false, true);
        assert!(s.short_cycling());
        s.update(1651.0 + 1811.0, false, false, true);
        assert!(!s.short_cycling());
        assert_eq!(s.decision, None);
        // Spread over more than 1799 s: no.
        let mut s = ShortCycle::default();
        s.update(0.0, false, false, true);
        for i in 0..12 {
            let t = 1.0 + f64::from(i) * 170.0;
            s.update(t, false, false, false);
            s.update(t + 10.0, false, false, true);
        }
        assert!(!s.short_cycling());
    }

    #[test]
    fn a_cycle_alert_times_out_after_four_hours() {
        let mut s = ShortCycle::default();
        let mut t = 0.0;
        for _ in 0..10 {
            t = cycle(&mut s, t + 60.0, 60.0, Some(20.0));
        }
        let at = s.since;
        s.update(at + 14400.0, false, false, true);
        assert!(s.short_cycling());
        s.update(at + 14401.0, false, false, true);
        assert!(!s.short_cycling());
    }

    #[test]
    fn a_long_cycle_after_the_alert_clears_it() {
        let mut s = ShortCycle::default();
        let mut t = 0.0;
        for _ in 0..10 {
            t = cycle(&mut s, t + 60.0, 60.0, Some(20.0));
        }
        // Idle keeps it up.
        cycle(&mut s, t + 400.0, 60.0, Some(20.0));
        assert!(s.short_cycling(), "a new run after a gap: too few cycles, the last short");
        cycle(&mut s, t + 1000.0, 120.0, None);
        assert!(!s.short_cycling(), "too few cycles, the last one long");
        assert_eq!(s.decision, Some(Verdict::Insufficient));
    }

    #[test]
    fn short_cycles_without_losses_clear_it_only_390_s_after_the_alert() {
        let mut s = ShortCycle::default();
        let mut t = 0.0;
        for _ in 0..10 {
            t = cycle(&mut s, t + 60.0, 60.0, Some(20.0));
        }
        // Cycles without losses push provoking ones out of the ring.
        t = cycle(&mut s, t + 60.0, 60.0, None);
        assert!(s.short_cycling(), "within 390 s of the alert");
        for _ in 0..3 {
            t = cycle(&mut s, t + 60.0, 60.0, None);
        }
        assert!(!s.short_cycling());
        assert_eq!(s.decision, Some(Verdict::ShortCycles));
    }

    #[test]
    fn cooling_is_tracked_on_its_own() {
        let mut s = ShortCycle::default();
        let mut t = 0.0;
        for _ in 0..10 {
            s.update(t + 60.0, false, true, true);
            s.update(t + 120.0, false, false, true);
            s.update(t + 130.0, false, false, false);
            s.update(t + 135.0, false, false, true);
            t += 135.0;
        }
        assert!(s.short_cycling());
        assert_eq!(s.alert_side, Some(Side::Cool));
    }
}
