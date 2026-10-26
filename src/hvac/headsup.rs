//! COMPRESSOR_HEADS_UP, as stock nlclient runs it (0x4de490 before the
//! decision, 0x4df1cc as a controller, 0x4dddc0 after it, 0x4dd944 on each
//! temperature): a compressor run that moves the room too slowly while it
//! is far from target is stopped for a 30-minute retest; if the room does
//! no better with it off, the next runs are watched more strictly, up to a
//! block for as long as that action is wanted.

use super::control::{Action, Controller, Decision, Lockout, Operation};

/// The watched run's samples (hours since it began, °C): all of them
/// (stock's CumulativeMeanAndCovariance) and weighted towards the latest.
const RECENT_ALPHA: f32 = 0.04878;
/// The learned rates' weight.
const LEARN_ALPHA: f32 = 0.0004;
const TINY: f32 = f32::EPSILON;
/// A run is judged after half an hour; the retest lasts as long.
const JUDGE: f64 = 1800.0;
const RETEST: f64 = 1800.0;
/// Past these the recent rate counts too, and the fixed thresholds apply.
const LONG_HEAT: f64 = 18000.0;
const LONG_COOL: f64 = 14400.0;
/// How far from target the room must still be (°C).
const HEAT_SHORT: f32 = 3.33;
const COOL_SHORT: f32 = 2.78;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Axis {
    mean: f32,
    n: f32,
    m2: f32,
}

/// Stock's running mean and covariance (0x4e0994), frozen at a million
/// samples.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Welford {
    x: Option<Axis>,
    my: f32,
    cov: f32,
}

impl Welford {
    fn add(&mut self, x: f32, y: f32) {
        let Some(ax) = self.x.as_mut() else {
            self.x = Some(Axis { mean: x, n: 1.0, m2: 0.0 });
            (self.my, self.cov) = (y, 0.0);
            return;
        };
        if ax.n >= 1e6 {
            return;
        }
        let (old_x, old_y) = (ax.mean, self.my);
        ax.n += 1.0;
        ax.mean = old_x + (x - old_x) / ax.n;
        ax.m2 += (x - old_x) * (x - ax.mean);
        self.my = old_y + (y - old_y) / ax.n;
        self.cov += (x - old_x) * (y - self.my);
    }

    /// The slope, °C an hour: covariance over the variance of x.
    fn slope(&self) -> Option<f32> {
        let ax = self.x?;
        let (vx, cov) = (ax.m2 / ax.n, self.cov / ax.n);
        (vx.abs() > TINY && vx >= 0.0).then_some(cov / vx)
    }
}

/// Stock's exponentially weighted mean and covariance (0x4e0d30).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Ewma {
    mx: Option<f32>,
    vx: f32,
    my: f32,
    cov: f32,
}

impl Ewma {
    fn add(&mut self, x: f32, y: f32, a: f32) {
        let Some(mx) = self.mx else {
            *self = Ewma { mx: Some(x), ..Ewma::default() };
            self.my = y;
            return;
        };
        let (dx, dy) = (x - mx, y - self.my);
        self.mx = Some(mx + a * dx);
        self.my += a * dy;
        self.vx = (1.0 - a) * (self.vx + a * dx * dx);
        self.cov = (1.0 - a) * (self.cov + a * dx * dy);
    }

    fn slope(&self) -> Option<f32> {
        self.mx?;
        (self.vx.abs() > TINY && self.vx >= 0.0).then_some(self.cov / self.vx)
    }
}

/// A weighted mean and variance (0x4e0c1c).
fn learn(mean: &mut f32, var: &mut f32, x: f32) {
    let d = x - *mean;
    *mean += LEARN_ALPHA * d;
    *var = (1.0 - LEARN_ALPHA) * (*var + LEARN_ALPHA * d * d);
}

/// Stock's states: idle; heat watch, retest, block; cool watch, retest,
/// block; a rest of an hour, and of four.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    HeatWatch,
    HeatRetest,
    HeatBlock,
    CoolWatch,
    CoolRetest,
    CoolBlock,
    Rest,
    LongRest,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HeadsUp {
    /// Armed: `compressor_heads_up_enabled` (on unless set off) with a
    /// cool stage wired.
    pub enabled: bool,
    pub state: State,
    entered: f64,
    level: u8,
    rests: u8,
    block: bool,
    worse: bool,
    retest_ok: bool,
    start: Option<f64>,
    all: Welford,
    recent: Ewma,
    run_rate: Option<f32>,
    /// The rates learned (°C/h) and their variances, heat then cool; stock
    /// keeps them in memory only.
    learned: [(f32, f32); 2],
    /// The last judgement, for the control's log.
    pub note: Option<String>,
}

impl Default for HeadsUp {
    fn default() -> HeadsUp {
        HeadsUp { enabled: false, state: State::Idle, entered: 0.0, level: 0, rests: 0, block: false, worse: false, retest_ok: false,
                  start: None, all: Welford::default(), recent: Ewma::default(), run_rate: None, learned: [(0.0, 4.0); 2], note: None }
    }
}

/// The room and targets the checks need.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Room {
    pub room: f32,
    pub heat: Option<f32>,
    pub cool: Option<f32>,
    pub safety: (Option<f32>, Option<f32>),
    pub outdoor: Option<f32>,
}

impl HeadsUp {
    fn timeout(&self) -> f64 {
        match self.state {
            State::HeatWatch | State::CoolWatch => if self.level == 1 { 7200.0 } else { 1800.0 },
            State::HeatRetest | State::CoolRetest => RETEST,
            State::Rest => 3600.0,
            State::LongRest => 14400.0,
            _ => f64::INFINITY,
        }
    }

    fn enter(&mut self, state: State, now: f64) {
        if state != self.state {
            debug!("hvac: compressor heads-up {:?} -> {state:?} (level {}, rests {})", self.state, self.level, self.rests);
            self.state = state;
            self.entered = now;
        }
    }

    /// A new watch or retest: the fits start over (0x4dd698).
    fn restart(&mut self, now: f64) {
        self.start = Some(now);
        self.all = Welford::default();
        self.recent = Ewma::default();
        self.block = false;
    }

    fn elapsed(&self, now: f64) -> Option<f64> {
        self.start.map(|s| now - s)
    }

    /// A room temperature (0x4dd944), feeding the fits while watching or
    /// retesting, and the learned rate past half an hour of a watch.
    pub fn sample(&mut self, room: f32, now: f64) {
        let watching = matches!(self.state, State::HeatWatch | State::HeatRetest | State::CoolWatch | State::CoolRetest);
        let Some(el) = self.elapsed(now).filter(|e| *e >= 0.0 && watching) else { return };
        let x = (el / 3600.0) as f32;
        self.all.add(x, room);
        self.recent.add(x, room, RECENT_ALPHA);
        if el < JUDGE {
            return;
        }
        if let Some(rate) = self.recent.slope() {
            match self.state {
                State::HeatWatch => learn(&mut self.learned[0].0, &mut self.learned[0].1, rate),
                State::CoolWatch => learn(&mut self.learned[1].0, &mut self.learned[1].1, rate),
                _ => {}
            }
        }
    }

    /// The rate below (heat) or above (cool) which a run is not keeping up
    /// (0x4de264).
    fn threshold(&self, heat: bool, el: f64, outdoor: Option<f32>) -> f32 {
        if self.level >= 2 {
            return 0.0;
        }
        if heat && el >= LONG_HEAT {
            return if outdoor.is_none_or(|o| o > 4.5) { -0.9 } else { -1.1 };
        }
        if !heat && el >= LONG_COOL {
            return if outdoor.is_none_or(|o| o < 30.0) { 1.1 } else { 1.3 };
        }
        if heat {
            let (m, v) = self.learned[0];
            (m - 2.0 * v.max(0.0).sqrt()).clamp(-4.0, -2.0)
        } else {
            let (m, v) = self.learned[1];
            (m + 2.0 * v.max(0.0).sqrt()).clamp(2.0, 6.0)
        }
    }

    /// Before the decision (0x4de490), with the previous decision's action.
    pub fn before(&mut self, previous: Action, r: Room, now: f64) {
        if !self.enabled {
            (self.block, self.worse, self.retest_ok) = (false, false, false);
            return;
        }
        if matches!(self.state, State::Rest | State::LongRest) && now - self.entered >= self.timeout() {
            self.enter(State::Idle, now);
        }
        let Some(el) = self.elapsed(now) else { return };
        let timed_out = el >= self.timeout();
        let both = |a: Option<f32>, b: Option<f32>, f: fn(f32, f32) -> f32| match (a, b) { (Some(a), Some(b)) => Some(f(a, b)), (a, b) => a.or(b) };
        match self.state {
            State::HeatWatch if previous == Action::Heat && el >= JUDGE => {
                let mut run = self.all.slope();
                if el >= LONG_HEAT {
                    run = both(run, self.recent.slope(), f32::min);
                }
                self.run_rate = run;
                let thr = self.threshold(true, el, r.outdoor);
                let far = both(r.heat, r.safety.0, f32::max).is_none_or(|t| r.room < t - HEAT_SHORT);
                self.block = timed_out && run.is_some_and(|run| run < thr) && far;
                self.note = Some(format!("compressor heads-up heat watch: {} °C/h against {thr:.2} after {:.0} s, far from target {far}: {}",
                                         run.map_or("no rate".into(), |r| format!("{r:.2}")), el, if self.block { "block" } else { "keep going" }));
            }
            State::CoolWatch if previous == Action::Cool && el >= JUDGE => {
                let mut run = self.all.slope();
                if el >= LONG_COOL {
                    run = both(run, self.recent.slope(), f32::max);
                }
                self.run_rate = run;
                let thr = self.threshold(false, el, r.outdoor);
                let far = both(r.cool, r.safety.1, f32::min).is_some_and(|t| r.room > t + COOL_SHORT);
                self.block = timed_out && run.is_some_and(|run| run > thr) && far;
                self.note = Some(format!("compressor heads-up cool watch: {} °C/h against {thr:.2} after {:.0} s, far from target {far}: {}",
                                         run.map_or("no rate".into(), |r| format!("{r:.2}")), el, if self.block { "block" } else { "keep going" }));
            }
            State::HeatRetest | State::CoolRetest if previous == Action::Drift && timed_out => {
                match self.all.slope() {
                    None => (self.worse, self.retest_ok) = (false, true),
                    Some(r) => {
                        self.worse = match (self.state, self.run_rate) {
                            (State::HeatRetest, Some(run)) => r > run,
                            (State::HeatRetest, None) => true,
                            (_, Some(run)) => r < run,
                            (_, None) => false,
                        };
                        self.retest_ok = !self.worse;
                    }
                }
                debug!("hvac: compressor heads-up retest: {}", if self.worse { "the run did worse than the compressor off" } else { "the run did better than the compressor off" });
            }
            _ => {}
        }
    }

    /// As a controller after the band (0x4df1cc): the compressor stopped
    /// while the checks say so.
    pub fn control(&self, d: Decision, heat_pump: bool, now: f64) -> Decision {
        use State::*;
        let heat = d.action == Action::Heat && d.operation.compressor(heat_pump) && matches!(self.state, HeatWatch | HeatRetest | HeatBlock | CoolBlock);
        let cool = d.action == Action::Cool && matches!(self.state, HeatBlock | CoolWatch | CoolRetest | CoolBlock);
        if !self.enabled || !(heat || cool) {
            return d;
        }
        let blocks = match self.state {
            HeatWatch | CoolWatch => self.block,
            HeatRetest | CoolRetest => if self.worse { self.level == 3 } else { !self.retest_ok },
            _ => true,
        };
        if !blocks {
            return d;
        }
        let lockout = match self.state {
            HeatWatch | CoolWatch => Some((Lockout::MinimumCycle, d.operation, RETEST)),
            HeatRetest | CoolRetest => Some((Lockout::MinimumCycle, d.operation, (RETEST - self.elapsed(now).unwrap_or(0.0)).max(0.0))),
            _ => None,
        };
        Decision { controller: Controller::CompressorHeadsUp, action: Action::Drift, operation: Operation::Off, lockout, ..d }
    }

    /// After the decision (0x4dddc0): watches start with a compressor run,
    /// a block starts the retest, a worse retest watches the next run more
    /// strictly (or blocks at the third level); otherwise a rest, every
    /// fourth one longer.
    pub fn after(&mut self, d: &Decision, heat_pump: bool, now: f64) {
        use State::*;
        if !self.enabled {
            return;
        }
        let blocked = d.controller == Controller::CompressorHeadsUp && d.action == Action::Drift && d.operation == Operation::Off;
        let heat_run = d.action == Action::Heat && d.operation.compressor(heat_pump);
        let cool_run = d.action == Action::Cool && d.operation.compressor(heat_pump);
        let heat_side = matches!(self.state, HeatWatch | HeatRetest | HeatBlock);
        match self.state {
            Idle => {
                if heat_run || cool_run {
                    self.level = 1;
                    self.enter(if heat_run { HeatWatch } else { CoolWatch }, now);
                    self.restart(now);
                }
                return;
            }
            Rest | LongRest => return,
            HeatWatch | CoolWatch if blocked => {
                self.enter(if heat_side { HeatRetest } else { CoolRetest }, now);
                self.restart(now);
                return;
            }
            _ => {}
        }
        if self.worse {
            if self.level > 2 && blocked {
                self.enter(if heat_side { HeatBlock } else { CoolBlock }, now);
                return;
            }
            if self.level <= 2 && (heat_run || cool_run) {
                self.level += 1;
                self.worse = false;
                self.enter(if heat_run { HeatWatch } else { CoolWatch }, now);
                self.restart(now);
                return;
            }
        } else if !self.retest_ok {
            let stays = match self.state {
                HeatWatch => d.action == Action::Heat,
                CoolWatch => d.action == Action::Cool,
                HeatRetest | CoolRetest => blocked,
                _ => false,
            };
            if stays {
                return;
            }
        }
        self.retest_ok = false;
        if self.rests < 3 {
            self.rests += 1;
            self.enter(Rest, now);
        } else {
            self.rests = 0;
            self.enter(LongRest, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cooling() -> Decision {
        Decision { controller: Controller::LearningBand, action: Action::Cool, operation: Operation::Stage1Cool, lockout: None, fan: false,
                   humidity: super::super::humidity::Op::Off }
    }

    #[test]
    fn an_air_conditioner_that_cannot_cool_is_stopped_after_two_hours_for_a_retest() {
        let mut h = HeadsUp { enabled: true, ..HeadsUp::default() };
        let r = |room: f32| Room { room, heat: None, cool: Some(22.0), safety: (None, None), outdoor: None };
        let mut now = 0.0;
        let mut d = cooling();
        h.after(&d, false, now);
        assert_eq!(h.state, State::CoolWatch);
        // The room warms 5 °C an hour while cooling: past the 4 °C/h stock
        // allows before it has learned anything.
        let mut room = 27.0;
        while now < 7300.0 {
            now += 60.0;
            room += 5.0 / 60.0;
            h.sample(room, now);
            h.before(Action::Cool, r(room), now);
            d = h.control(cooling(), false, now);
            h.after(&d, false, now);
            if d.controller == Controller::CompressorHeadsUp {
                break;
            }
        }
        assert_eq!(d.controller, Controller::CompressorHeadsUp, "blocked at the watch's end");
        assert!(now >= 7200.0);
        assert_eq!(d.lockout, Some((Lockout::MinimumCycle, Operation::Stage1Cool, RETEST)));
        assert_eq!(h.state, State::CoolRetest);
    }

    #[test]
    fn the_fits_slope_is_the_rate_an_hour() {
        let mut w = Welford::default();
        let mut e = Ewma::default();
        for i in 0..10 {
            let x = i as f32 / 60.0;
            w.add(x, 20.0 + 2.0 * x);
            e.add(x, 20.0 + 2.0 * x, RECENT_ALPHA);
        }
        assert!((w.slope().unwrap() - 2.0).abs() < 1e-3);
        assert!((e.slope().unwrap() - 2.0).abs() < 1e-2);
    }
}
