//! Stock nlclient's Sunblock (nlSunlightCorrection, 0x3337cc): it learns
//! when sun falls on the unit from the ambient light, a 5-minute bin of
//! the day at a time, and while a light spike lasts it watches for the
//! temperature to run away from where it was. When it does, it tells the
//! backplate to compensate (SetTempCompMode) until the temperature comes
//! back, the spike has long ended or two hours pass. The correction itself
//! is the backplate's; stock adjusts no temperature in software.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Bins in a day, 300 s each.
const BINS: usize = 288;
/// A bin's samples are averaged over at most this many days.
const MAX_SAMPLES: u32 = 5;
/// The day window without weather: 05:00 to 19:00.
const DAY_START: usize = 60;
const DAY_END: usize = 227;
/// The thresholds before anything is learned, and their floors.
const LOW: u32 = 2000;
const HIGH: u32 = 3000;
const LOW_FLOOR: u32 = 1000;
const HIGH_FLOOR: u32 = 1250;
/// Heating within this long raises the spike threshold by HEAT_BOOST.
const HEAT_WINDOW: f64 = 600.0;
const HEAT_BOOST: u32 = 20000;
/// The longest each state lasts, and the spike's grace once it ends.
const CONFIRM: f64 = 300.0;
const LOOKING: f64 = 9000.0;
const CORRECTING: f64 = 7200.0;
const TIMEOUT: f64 = 600.0;
const AFTER_SPIKE: f64 = 2700.0;
/// Error readings for this long disable it, until a good one.
const ALS_ERRORS: f64 = 3600.0;
/// The divergence thresholds over the baseline: 1 °F and 2 °F.
const ONE_F: f32 = 5.0 / 9.0;
const TWO_F: f32 = 10.0 / 9.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Disabled,
    Idle,
    LowProbability,
    HighProbability,
    LookingForTemperature,
    SunlightCorrection,
    Timeout,
    ConfirmAlsSpike,
}

/// What asks for a rethink (stock's causes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    Timer,
    Vergence,
    PreSleep,
    Light,
    Enabled,
    Reset,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Bin {
    count: u32,
    smoothed: u32,
    probability: bool,
}

/// A day's spike: its bins, and how many days ago.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Spike {
    days_ago: u32,
    start: usize,
    end: usize,
}

/// What stock keeps in its recovery file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Learned {
    updated: f64,
    day_start: usize,
    day_end: usize,
    weekday: Vec<Spike>,
    weekend: Vec<Spike>,
    max_count: u32,
    low: u32,
    high: u32,
    bins: Vec<Bin>,
}

impl Learned {
    fn new(now: f64) -> Learned {
        Learned { updated: now, day_start: DAY_START, day_end: DAY_END, weekday: Vec::new(), weekend: Vec::new(),
                  max_count: 0, low: LOW, high: HIGH, bins: vec![Bin::default(); BINS] }
    }

    fn is_day(&self, bin: usize) -> bool {
        if self.day_start <= self.day_end {
            (self.day_start..=self.day_end).contains(&bin)
        } else {
            bin >= self.day_start || bin <= self.day_end
        }
    }

    fn day_bins(&self) -> usize {
        (self.day_end + BINS - self.day_start) % BINS + 1
    }

    /// A light sample into its bin (0x32f364).
    fn sample(&mut self, bin: usize, value: u16) {
        let b = &mut self.bins[bin];
        b.count = (b.count + 1).min(MAX_SAMPLES);
        b.smoothed = ((f64::from(value) + f64::from(b.smoothed) * f64::from(b.count - 1)) / f64::from(b.count)).round() as u32;
        self.max_count = self.max_count.max(b.count);
    }

    /// The day's end (0x333a84): the window from the weather, then with
    /// three days of samples the thresholds and the sunny bins; the
    /// finished day's spikes age.
    fn midnight(&mut self, weekday: bool, window: Option<(usize, usize)>) {
        if let Some((start, end)) = window {
            self.day_start = start.min(BINS - 1);
            self.day_end = end.min(BINS - 1);
        }
        debug!("sunblock: day ended: window bins {} to {}, {} days of samples", self.day_start, self.day_end, self.max_count);
        if self.max_count > 2 {
            let mut v: Vec<u32> = (0..self.day_bins()).map(|i| self.bins[(self.day_start + i) % BINS].smoothed).collect();
            v.sort_unstable();
            let at = |f: u32| ((u64::from(f) * ((v.len() as u64 - 1) << 16) + 0x8000) >> 32) as usize;
            self.low = v[at(0xe8f5)].max(LOW_FLOOR);
            self.high = v[at(0xfae1)].max(HIGH_FLOOR);
            self.probability();
            debug!("sunblock: learned thresholds low {} high {}, {} bins likely sunny", self.low, self.high, self.bins.iter().filter(|b| b.probability).count());
        }
        let spikes = if weekday { &mut self.weekday } else { &mut self.weekend };
        spikes.retain_mut(|s| {
            s.days_ago += 1;
            s.days_ago <= 3
        });
    }

    /// Runs of 2 to 24 bright bins (gaps of up to 3), widened by 5 bins
    /// each side within the day, are where sun is likely (0x32fa20).
    fn probability(&mut self) {
        let mut flags = [false; BINS];
        let mark = |s: usize, e: usize, flags: &mut [bool; BINS]| {
            if (e + BINS - s) % BINS + 1 > 24 || (e + BINS - s) % BINS < 1 {
                return;
            }
            for i in 0..((e + BINS - s) % BINS + 11) {
                let b = (s + BINS - 5 + i) % BINS;
                if self.is_day(b) {
                    flags[b] = true;
                }
            }
        };
        let mut run: Option<(usize, usize)> = None;
        for i in 0..self.day_bins() {
            let b = (self.day_start + i) % BINS;
            if self.bins[b].smoothed < self.high {
                continue;
            }
            run = match run {
                Some((s, e)) if (b + BINS - e) % BINS <= 3 => Some((s, b)),
                Some((s, e)) => {
                    mark(s, e, &mut flags);
                    Some((b, b))
                }
                None => Some((b, b)),
            };
        }
        if let Some((s, e)) = run {
            mark(s, e, &mut flags);
        }
        for (bin, flag) in self.bins.iter_mut().zip(flags) {
            bin.probability = flag;
        }
    }

    /// A spike within 9 bins of `bin` on one of the last days of the
    /// same kind (0x32e934).
    fn spiked_near(&self, weekday: bool, bin: usize) -> bool {
        let spikes = if weekday { &self.weekday } else { &self.weekend };
        let bin = bin as i64;
        spikes.iter().filter(|s| s.days_ago != 0).any(|s| {
            let (mut lo, mut hi) = (s.start as i64 - 9, s.end as i64 + 9);
            if lo < 0 {
                lo += BINS as i64;
            }
            if hi >= BINS as i64 {
                hi -= BINS as i64;
            }
            if lo <= hi { lo <= bin && bin <= hi } else { bin >= lo || bin <= hi }
        })
    }
}

/// The local time's parts that Sunblock uses: seconds into the day, Monday
/// to Friday, and the date.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Local {
    pub seconds: u32,
    pub weekday: bool,
    pub date: (i32, i32, i32),
}

impl Local {
    pub fn at(t: f64) -> Local {
        // SAFETY: localtime_r fills the tm it is given.
        let tm = unsafe {
            let secs = t as libc::time_t;
            let mut tm: libc::tm = std::mem::zeroed();
            libc::localtime_r(&secs, &mut tm);
            tm
        };
        Local { seconds: (tm.tm_hour * 3600 + tm.tm_min * 60 + tm.tm_sec) as u32, weekday: (1..=5).contains(&tm.tm_wday), date: (tm.tm_year, tm.tm_mon, tm.tm_mday) }
    }

    fn bin(&self) -> usize {
        (self.seconds / 300).min(BINS as u32 - 1) as usize
    }
}

/// What a rethink reads besides its own state.
#[derive(Clone, Copy, Debug)]
pub struct Inputs {
    pub enabled: bool,
    /// The room as measured, when valid.
    pub temperature: Option<f32>,
}

pub struct Sunblock {
    pub state: State,
    entered: f64,
    /// Light samples arrived since the last pre-sleep rethink.
    als_fresh: bool,
    verged: bool,
    base_min: f32,
    base_median: f32,
    diverged: Option<f32>,
    diverged_at: f64,
    deadline: f64,
    spiking: bool,
    als_error: bool,
    als_error_since: Option<f64>,
    learned: Learned,
    tracked: Option<(i32, i32, i32)>,
    /// The latest light and when; the room every 30 s; the last heating.
    light: Option<(f64, u16)>,
    temperatures: VecDeque<(f64, f32)>,
    heated_at: Option<f64>,
    pub ready: bool,
    pub active: bool,
    /// SetTempCompMode: the backplate corrects while on.
    pub temp_comp: bool,
    /// ALS_WAKE (low, high) and VERGENCE_WAKE (mode, centi-°C, second) for
    /// the next sleep.
    pub als_wake: (u16, u16),
    pub vergence: (u16, i16, i16),
    /// When the next rethink is due.
    pub timer: Option<f64>,
    /// The learning changed in a way stock writes its recovery file for.
    pub save: bool,
}

impl Sunblock {
    /// From the learning kept, or afresh (0x332024).
    pub fn new(now: f64, kept: Option<Learned>, enabled: bool) -> Sunblock {
        let learned = kept.filter(|l| l.bins.len() == BINS).unwrap_or_else(|| Learned::new(now));
        let mut s = Sunblock { state: if enabled { State::Idle } else { State::Disabled }, entered: now, als_fresh: false, verged: false,
                               base_min: 0.0, base_median: 0.0, diverged: None, diverged_at: 0.0, deadline: 0.0, spiking: false,
                               als_error: false, als_error_since: None, learned, tracked: None, light: None, temperatures: VecDeque::new(),
                               heated_at: None, ready: false, active: false, temp_comp: false, als_wake: (0, 0xffff), vergence: (0, 0, 0),
                               timer: None, save: false };
        s.ready = s.is_ready(enabled);
        s
    }

    pub fn learned(&self) -> &Learned {
        &self.learned
    }

    fn is_ready(&self, enabled: bool) -> bool {
        enabled && !self.als_error && self.learned.max_count > 2
    }

    /// The light now (ALS), the room every 30 s and heating, as they come.
    pub fn light(&mut self, now: f64, visible: u16) {
        self.light = Some((now, visible));
    }

    pub fn temperature(&mut self, now: f64, celsius: f32) {
        if self.temperatures.back().is_none_or(|&(t, _)| now - t >= 30.0) {
            self.temperatures.push_back((now, celsius));
            while self.temperatures.len() > 12 {
                self.temperatures.pop_front();
            }
        }
    }

    pub fn heating(&mut self, now: f64, on: bool) {
        if on {
            self.heated_at = Some(now);
        }
    }

    /// The backplate's light buffer, a sample a 5 minutes with the newest
    /// now (0x333c20); a day ending among them learns first.
    pub fn light_buffer(&mut self, now: f64, samples: &[u16], inputs: Inputs, window: Option<(usize, usize)>) {
        if !inputs.enabled {
            return;
        }
        let n = samples.len();
        for (i, &v) in samples.iter().enumerate() {
            let t = now - (n - 1 - i) as f64 * 300.0;
            let local = Local::at(t);
            let tracked = *self.tracked.get_or_insert(Local::at(self.learned.updated).date);
            if local.date != tracked {
                let ended = Local::at(t - 86400.0).weekday;
                self.learned.midnight(ended, window);
                self.ready = self.is_ready(inputs.enabled);
                self.save = true;
                self.tracked = Some(local.date);
            }
            if v != 0xffff {
                self.learned.sample(local.bin(), v);
            }
        }
        self.learned.updated = now;
        self.rethink(now, Cause::Light, inputs);
    }

    /// Seconds since heating, in stock's 30 s steps, when within 600 s.
    fn heat(&self, now: f64) -> Option<f64> {
        let ago = now - self.heated_at?;
        (ago <= HEAT_WINDOW).then(|| ((ago / 30.0).floor() * 30.0 - 30.0).max(-1.0))
    }

    /// The baseline when a spike starts (0x32f1ec): the lowest of now and
    /// the last 10 readings, and their sixth lowest.
    fn baseline(&mut self, current: f32) {
        let mut h: Vec<f32> = self.temperatures.iter().rev().take(10).map(|&(_, c)| c).collect();
        if h.len() < 10 {
            self.base_min = h.iter().copied().fold(current, f32::min);
            self.base_median = current;
            return;
        }
        self.base_min = h.iter().copied().fold(current, f32::min);
        h.sort_by(f32::total_cmp);
        self.base_median = h[5];
    }

    fn spike_ended(&mut self, now: f64) {
        self.deadline = now + AFTER_SPIKE;
        self.spiking = false;
    }

    /// A rethink (0x3337cc): the state the inputs call for, then the wake
    /// thresholds, `active` and the next rethink.
    pub fn rethink(&mut self, now: f64, cause: Cause, inputs: Inputs) {
        let local = Local::at(now);
        let bin = local.bin();
        match cause {
            Cause::Light => self.als_fresh = true,
            Cause::PreSleep => self.als_fresh = false,
            Cause::Vergence => {
                if !matches!(self.state, State::LookingForTemperature | State::SunlightCorrection) {
                    return;
                }
                self.verged = true;
            }
            Cause::Enabled | Cause::Reset => self.ready = self.is_ready(inputs.enabled),
            Cause::Timer => {}
        }
        let is_day = self.learned.is_day(bin);
        let eligible = is_day && self.ready;
        let heat = self.heat(now);
        let prob = self.learned.bins[bin].probability;
        let base = if prob { self.learned.low } else { self.learned.high };
        let thr = if heat.is_some() { (base + HEAT_BOOST).min(0xffff) } else { base };
        let elapsed = now - self.entered;
        let region = if prob { State::HighProbability } else { State::LowProbability };
        let mut new = self.state;
        let evaluate = inputs.enabled && self.als_fresh && matches!(cause, Cause::Timer | Cause::Vergence | Cause::Light);
        match cause {
            Cause::Enabled if inputs.enabled => new = if eligible { region } else { State::Idle },
            Cause::Enabled => new = State::Disabled,
            Cause::Reset if self.state != State::Disabled => new = if eligible { region } else { State::Idle },
            _ => {}
        }
        if evaluate {
            let als = self.light.filter(|(t, _)| now - t <= 600.0).map(|(_, v)| v);
            debug!("sunblock: {cause:?} in {:?} for {:.0} s: light {}, threshold {thr}{}, bin {bin} {} probability, {}, room {}", self.state, elapsed,
                   als.map_or("stale".into(), |a| a.to_string()), if heat.is_some() { " (heating boost)" } else { "" },
                   if prob { "high" } else { "low" }, if eligible { "eligible" } else if is_day { "not ready" } else { "night" },
                   inputs.temperature.map_or("none".into(), |t| format!("{t:.2} °C")));
            if let Some(als) = als {
                if als == 0xffff {
                    let since = *self.als_error_since.get_or_insert(now);
                    if now - since > ALS_ERRORS && !self.als_error {
                        warn!("sunblock: light unread for {ALS_ERRORS} s: off until it reads again");
                        self.als_error = true;
                        self.ready = self.is_ready(inputs.enabled);
                    }
                } else {
                    self.als_error_since = None;
                    if self.als_error {
                        info!("sunblock: light reads again");
                        self.als_error = false;
                        self.ready = self.is_ready(inputs.enabled);
                    }
                    let eligible = is_day && self.ready;
                    new = self.transition(now, als, thr, prob, elapsed, eligible, inputs.temperature);
                }
            }
            self.verged = false;
        }
        let eligible_now = is_day && self.ready;
        if !eligible_now && !matches!(new, State::Disabled | State::Idle) {
            new = State::Idle;
        }
        if !inputs.enabled {
            new = State::Disabled;
        }
        if new != self.state {
            info!("sunblock: {:?} -> {:?}", self.state, new);
            if new == State::SunlightCorrection {
                self.temp_comp = true;
            }
            if self.state == State::SunlightCorrection {
                self.temp_comp = false;
                let spike = Spike { days_ago: 0, start: Local::at(self.diverged_at).bin(), end: bin };
                if local.weekday { self.learned.weekday.push(spike) } else { self.learned.weekend.push(spike) }
                self.save = true;
                if new != State::LookingForTemperature {
                    self.spike_ended(now);
                }
            } else if self.state == State::LookingForTemperature && new == State::Disabled {
                self.spike_ended(now);
            }
            self.state = new;
            self.entered = now;
        }
        let prev_spike = eligible && self.learned.spiked_near(local.weekday, bin);
        self.outputs(now, local, thr, prev_spike, heat);
    }

    /// The transition table, with fresh light `als` (§2.4).
    #[allow(clippy::too_many_arguments)]
    fn transition(&mut self, now: f64, als: u16, thr: u32, prob: bool, elapsed: f64, eligible: bool, temperature: Option<f32>) -> State {
        use State::*;
        let region = if prob { HighProbability } else { LowProbability };
        let als = u32::from(als);
        let below = als + 100 < thr;
        if !eligible {
            return if matches!(self.state, Disabled) { Disabled } else { Idle };
        }
        match self.state {
            Disabled => Disabled,
            Idle => region,
            LowProbability | HighProbability if als < thr => region,
            LowProbability | HighProbability => match temperature {
                Some(t) => {
                    self.baseline(t);
                    debug!("sunblock: light spike {als} over {thr}: baseline lowest {:.2} °C, median {:.2} °C", self.base_min, self.base_median);
                    self.diverged = None;
                    self.diverged_at = 0.0;
                    self.deadline = 0.0;
                    self.spiking = true;
                    ConfirmAlsSpike
                }
                None => self.state,
            },
            ConfirmAlsSpike if below => region,
            ConfirmAlsSpike if elapsed >= CONFIRM => LookingForTemperature,
            ConfirmAlsSpike => ConfirmAlsSpike,
            LookingForTemperature if below => region,
            LookingForTemperature if elapsed >= LOOKING => Timeout,
            LookingForTemperature if self.verged => match temperature {
                Some(t) => {
                    debug!("sunblock: temperature ran away to {t:.2} °C from a baseline of {:.2} °C", self.base_min.max(self.base_median));
                    self.diverged = Some(t);
                    self.diverged_at = now;
                    SunlightCorrection
                }
                None => LookingForTemperature,
            },
            LookingForTemperature => LookingForTemperature,
            SunlightCorrection if self.verged => if below { region } else { LookingForTemperature },
            SunlightCorrection if elapsed >= CORRECTING => Timeout,
            SunlightCorrection if !self.spiking && now >= self.deadline => region,
            SunlightCorrection => {
                if !self.spiking && als >= thr {
                    self.deadline = 0.0;
                    self.spiking = true;
                } else if self.spiking && below {
                    self.spike_ended(now);
                }
                SunlightCorrection
            }
            Timeout if elapsed >= TIMEOUT => region,
            Timeout => Timeout,
        }
    }

    /// The wake thresholds, `active` and the next rethink (0x330c48).
    fn outputs(&mut self, now: f64, local: Local, thr: u32, prev_spike: bool, heat: Option<f64>) {
        use State::*;
        let elapsed = now - self.entered;
        let left = |span: f64| if span - elapsed <= 0.0 { 300.0 } else { span - elapsed };
        let wake_low = thr.saturating_sub(100) as u16;
        let centi = |c: f32| (c * 100.0 + 0.5) as i32;
        let mut due: Vec<f64> = Vec::new();
        let (mut active, high) = (false, (thr.min(0xffff)) as u16);
        (self.als_wake, self.vergence) = match self.state {
            Disabled | Idle | Timeout => ((0, 0xffff), (0, 0, 0)),
            LowProbability | HighProbability => ((0, high), (0, 0, 0)),
            ConfirmAlsSpike => {
                due.push(left(CONFIRM));
                ((wake_low, 0xffff), (0, 0, 0))
            }
            LookingForTemperature => {
                due.push(left(LOOKING));
                let over = if self.base_min + ONE_F < self.base_median { self.base_median + ONE_F } else { self.base_min + TWO_F };
                let div = self.diverged.map_or(over, |d| over.max(d));
                ((wake_low, 0xffff), (2, centi(div).clamp(-32768, 32767) as i16, centi(TWO_F) as i16))
            }
            SunlightCorrection => {
                due.push(left(CORRECTING));
                if !self.spiking {
                    due.push(if self.deadline - now <= 0.0 { 300.0 } else { self.deadline - now });
                }
                if prev_spike && elapsed < 300.0 {
                    due.push(300.0 - elapsed);
                }
                active = prev_spike && elapsed >= 300.0;
                let mut conv = self.base_min.max(self.base_median);
                if let Some(d) = self.diverged.filter(|d| d - 2.5 > conv) {
                    conv = d - 2.5;
                }
                let wake = if self.spiking { (wake_low, 0xffff) } else { (0, high) };
                (wake, (1, centi(conv).clamp(-32768, 32767) as i16, 0))
            }
        };
        if self.state == Timeout {
            due.push(left(TIMEOUT));
        }
        if active != self.active {
            info!("sunblock: sunlight correction {}", if active { "active" } else { "inactive" });
            self.active = active;
        }
        self.timer = None;
        if self.state == Disabled || !self.ready {
            return;
        }
        // The day's edge, and the next change of the bin's probability.
        let start = self.learned.day_start as f64 * 300.0;
        let end = self.learned.day_end as f64 * 300.0 + 300.0;
        let s = f64::from(local.seconds);
        let edge = [start, end].iter().map(|e| (e - s).rem_euclid(86400.0)).filter(|d| *d > 0.0).fold(86400.0, f64::min);
        due.push(edge);
        let bin = local.bin();
        let prob = self.learned.bins[bin].probability;
        if let Some(n) = (1..BINS).find(|i| self.learned.bins[(bin + i) % BINS].probability != prob) {
            due.push(((bin + n) as f64 * 300.0 - s).rem_euclid(86400.0));
        }
        if let Some(h) = heat.filter(|h| *h >= 0.0 && !matches!(self.state, Idle | Timeout)) {
            due.push(HEAT_WINDOW - h);
        }
        self.timer = due.into_iter().filter(|d| *d > 0.0).reduce(f64::min).map(|d| now + d);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled(t: Option<f32>) -> Inputs {
        Inputs { enabled: true, temperature: t }
    }

    /// Three days of the same bright hour, 11:00 to 12:00 at 9000, and
    /// 1500 the rest of the day.
    fn learned() -> Learned {
        let mut l = Learned::new(0.0);
        for _ in 0..3 {
            for b in 0..BINS {
                l.sample(b, if (132..144).contains(&b) { 9000 } else { 1500 });
            }
        }
        l.midnight(true, None);
        l
    }

    #[test]
    fn three_days_learn_thresholds_and_a_widened_sunny_hour() {
        let l = learned();
        assert_eq!((l.max_count, l.low, l.high), (3, 1500, 9000), "the 91st and 98th percentiles of the day");
        assert!(l.bins[127].probability && l.bins[148].probability, "five bins either side");
        assert!(!l.bins[126].probability && !l.bins[149].probability);
    }

    #[test]
    fn a_long_bright_run_is_not_a_spike() {
        let mut l = Learned::new(0.0);
        for _ in 0..3 {
            for b in 0..BINS {
                l.sample(b, if (100..130).contains(&b) { 9000 } else { 1500 });
            }
        }
        l.midnight(true, None);
        assert!(l.bins.iter().all(|b| !b.probability), "30 bins is more than 24");
    }

    #[test]
    fn spikes_age_out_after_three_days_and_match_nine_bins_around() {
        let mut l = Learned::new(0.0);
        l.weekday.push(Spike { days_ago: 0, start: 140, end: 142 });
        assert!(!l.spiked_near(true, 140), "not today's");
        l.midnight(true, None);
        assert!(l.spiked_near(true, 131) && l.spiked_near(true, 151) && !l.spiked_near(true, 152));
        assert!(!l.spiked_near(false, 140));
        for _ in 0..3 {
            l.midnight(true, None);
        }
        assert!(l.weekday.is_empty());
    }

    #[test]
    fn a_spike_then_a_divergence_turns_temperature_compensation_on() {
        // Local midday today: the sunny bins.
        let local = Local::at(0.0);
        let noon = 11.5 * 3600.0 - f64::from(local.seconds) + 86400.0;
        let mut s = Sunblock::new(noon, Some(learned()), true);
        assert!(s.ready);
        for i in 0..12 {
            s.temperature(noon - 330.0 + f64::from(i) * 30.0, 21.0);
        }
        s.light(noon, 1000);
        s.rethink(noon, Cause::Light, enabled(Some(21.0)));
        assert_eq!(s.state, State::HighProbability);
        assert_eq!(s.als_wake, (0, 1500), "a sunny bin wakes on the low threshold");
        s.light(noon + 60.0, 9500);
        s.rethink(noon + 60.0, Cause::Light, enabled(Some(21.0)));
        assert_eq!(s.state, State::ConfirmAlsSpike);
        s.rethink(noon + 400.0, Cause::Timer, enabled(Some(21.2)));
        assert_eq!(s.state, State::LookingForTemperature);
        assert_eq!(s.vergence, (2, 2211, 111), "wake 2 °F over the baseline");
        s.light(noon + 880.0, 9500);
        s.rethink(noon + 900.0, Cause::Vergence, enabled(Some(22.2)));
        assert_eq!(s.state, State::SunlightCorrection);
        assert!(s.temp_comp);
        assert_eq!(s.vergence.0, 1);
        s.light(noon + 1000.0, 500);
        s.rethink(noon + 1000.0, Cause::Light, enabled(Some(22.0)));
        assert!(s.temp_comp, "the spike ended, its grace runs");
        s.light(noon + 1000.0 + AFTER_SPIKE, 500);
        s.rethink(noon + 1000.0 + AFTER_SPIKE, Cause::Timer, enabled(Some(21.5)));
        assert_eq!(s.state, State::LowProbability);
        assert!(!s.temp_comp && s.save);
        assert_eq!(s.learned.weekday.len() + s.learned.weekend.len(), 1, "the spike is kept");
    }

    #[test]
    fn disabled_or_unlearned_it_stays_idle() {
        let mut s = Sunblock::new(0.0, None, true);
        assert!(!s.ready);
        s.light(0.0, 9000);
        s.rethink(0.0, Cause::Light, enabled(Some(21.0)));
        assert_eq!((s.state, s.timer), (State::Idle, None));
        s.rethink(0.0, Cause::Enabled, Inputs { enabled: false, temperature: None });
        assert_eq!(s.state, State::Disabled);
    }
}
