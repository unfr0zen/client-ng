//! The overshoot gate's learning, as stock nlclient's overshoot learner
//! does it (0x4e170c start, 0x4e1988 tracking, 0x4e2064 result, 0x4e12a8
//! writeback): after each heating or cooling cycle the room's coast past
//! where it stopped, and how long it took to peak, are averaged per stage
//! group; the gate then stops a cycle that much earlier.

use super::control::{Action, Controller, Decision, Operation};

/// Heat stage 1, heat stages 2 and 3, cool stage 1, cool stages 2 and 3.
pub const GROUPS: usize = 4;
/// Stock's starting values: overshoot (°C), its variance, time to peak
/// (hours), its variance; none where stock has none.
const DEFAULTS: [Group; GROUPS] = [
    Group { ov: Some(0.2), ov_var: None, time: None, time_var: Some(0.027) },
    Group { ov: Some(0.1), ov_var: None, time: None, time_var: Some(0.027) },
    Group { ov: Some(0.05), ov_var: None, time: None, time_var: Some(0.027) },
    Group { ov: Some(0.15), ov_var: None, time: None, time_var: Some(0.027) },
];
/// The most a group's overshoot may be (heat, cool), and one sample.
pub const CAP: (f32, f32) = (0.5, 0.39);
const SAMPLE_CAP: (f32, f32) = (1.5, 1.17);
/// The room must pass where the cycle stopped by this to count, and come
/// back from its peak by this to end the track (°C).
const START: f32 = 0.1;
const FINISH: f32 = 0.25;
/// A track with no peak gives up after this once the room is back past
/// where it stopped; any track gives up after the longest (hours).
const NO_PEAK: f64 = 0.067;
const LONGEST: f64 = 6.0;
const ALPHA: f32 = 0.05;
const DECAY_SHORT: f32 = 0.1;
const DECAY_UNFINISHED: f32 = 0.01;
const SIGMAS: f32 = 2.0;

/// The group a finished operation teaches (stock's table at 0x664150);
/// emergency heat teaches none.
pub fn group(op: Operation) -> Option<usize> {
    use Operation::*;
    Some(match op {
        Stage1Heat | Stage1HeatAux | AuxOnly | Stage1AltHeat => 0,
        Stage2Heat | Stage3Heat | Stage2HeatAux | Stage2AltHeat => 1,
        Stage1Cool => 2,
        Stage2Cool | Stage3Cool => 3,
        _ => return Option::None,
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Group {
    pub ov: Option<f32>,
    pub ov_var: Option<f32>,
    pub time: Option<f32>,
    pub time_var: Option<f32>,
}

/// An exponentially weighted mean and variance (0x4e0c1c).
fn ewma(mean: &mut Option<f32>, var: &mut Option<f32>, x: f32, alpha: f32) {
    match *mean {
        None => {
            *mean = Some(x);
            *var = Some(0.0);
        }
        Some(m) => {
            let d = x - m;
            *mean = Some(m + alpha * d);
            *var = Some((1.0 - alpha) * (var.unwrap_or(0.0) + alpha * d * d));
        }
    }
}

impl Group {
    fn decay(&mut self, alpha: f32) {
        ewma(&mut self.ov, &mut self.ov_var, 0.0, alpha);
        ewma(&mut self.time, &mut self.time_var, 0.0, alpha);
    }
}

/// A cycle's coast being followed.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Track {
    group: usize,
    heat: bool,
    stop: f32,
    at: f64,
    peak: Option<(f32, f64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Going,
    Done,
    Aborted,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Learner {
    pub groups: [Group; GROUPS],
    track: Option<Track>,
    /// The last cycle ended at the gate (stock's +0xbc).
    gated: bool,
}

impl Default for Learner {
    fn default() -> Learner {
        Learner { groups: DEFAULTS, track: None, gated: false }
    }
}

impl Learner {
    /// The overshoot the gate allows for a group.
    pub fn overshoot(&self, group: usize) -> f32 {
        let cap = if group < 2 { CAP.0 } else { CAP.1 };
        self.groups[group].ov.unwrap_or(0.0).clamp(0.0, cap)
    }

    /// Follows a decision (stock's post-decision bookkeeping): a cycle's
    /// end starts a track, the next one's start ends it; the room is
    /// tracked on every decision in between. `targets` is the targets'
    /// tracker state.
    pub fn update(&mut self, running: Decision, d: &Decision, room: f32, now: f64, targets: u8) {
        let cycling = |a: Action| matches!(a, Action::Heat | Action::Cool);
        if self.track.is_some() {
            let step = self.step(room, now);
            if step != Step::Going {
                let sample = if step == Step::Done { self.done() } else { None };
                if let Some(t) = self.track {
                    debug!("hvac: overshoot track for group {} {}: {}, the group's overshoot now {:.2} °C", t.group,
                           if step == Step::Done { "done" } else { "given up" }, sample.map_or("no sample".into(), |s| format!("sample {s:.2} °C")),
                           self.groups[t.group].ov.unwrap_or(0.0));
                }
                self.writeback(sample, step);
            }
        }
        if cycling(running.action) && d.action == Action::Drift {
            self.writeback(None, Step::Going);
            if let Some(group) = group(running.operation) {
                debug!("hvac: overshoot tracking group {group} from {room:.2} °C{}", if d.controller == Controller::Overshoot { ", ended at the gate" } else { "" });
                self.track = Some(Track { group, heat: running.action == Action::Heat, stop: room, at: now, peak: None });
            }
            self.gated = d.controller == Controller::Overshoot;
        } else if running.action == Action::Drift && cycling(d.action) {
            if !matches!(targets, 3 | 4) {
                self.gated = false;
            }
            self.writeback(None, Step::Going);
        }
    }

    /// One room reading on the track (0x4e1988), cooling mirroring heating.
    fn step(&mut self, room: f32, now: f64) -> Step {
        let Some(t) = self.track.as_mut() else { return Step::Aborted };
        let hours = (now - t.at) / 3600.0;
        if hours > LONGEST {
            return Step::Aborted;
        }
        // Past is further in the cycle's direction.
        let past = |a: f32, b: f32| if t.heat { a > b } else { a < b };
        let back = |by: f32| if t.heat { -by } else { by };
        let extreme = match t.peak {
            Some((p, _)) => past(room, p),
            None => past(room, t.stop + if t.heat { START } else { -START }),
        };
        if extreme {
            t.peak = Some((room, now));
            return Step::Going;
        }
        match t.peak {
            Some((p, _)) if past(p + back(FINISH), room) => Step::Done,
            None if hours > NO_PEAK && past(t.stop, room) => Step::Aborted,
            _ => Step::Going,
        }
    }

    /// A finished track (0x4e2064): the time to peak always teaches; the
    /// overshoot only when that time is within two deviations.
    fn done(&mut self) -> Option<f32> {
        let t = self.track?;
        let (peak, at) = t.peak?;
        let g = &mut self.groups[t.group];
        let dt = ((at - t.at) / 3600.0) as f32;
        ewma(&mut g.time, &mut g.time_var, dt, ALPHA);
        let sigma = g.time_var.map(|v| v.max(0.0).sqrt() * SIGMAS);
        let usual = match (g.time, sigma) {
            (Some(m), Some(s)) => m - s < dt && dt < m + s,
            _ => true,
        };
        if !usual {
            return None;
        }
        let (cap, sample_cap) = if t.heat { (CAP.0, SAMPLE_CAP.0) } else { (CAP.1, SAMPLE_CAP.1) };
        let sample = (peak - t.stop).abs().min(sample_cap);
        ewma(&mut g.ov, &mut g.ov_var, sample, ALPHA);
        g.ov = g.ov.map(|o| o.min(cap));
        Some(sample)
    }

    /// Ends the group's track (0x4e12a8): a cycle the gate ended with
    /// little or no overshoot shrinks it; a track left unfinished decays
    /// it slightly.
    fn writeback(&mut self, sample: Option<f32>, step: Step) {
        let Some(t) = self.track.take() else { return };
        let g = &mut self.groups[t.group];
        if self.gated && sample.is_none_or(|p| p < 0.5 * g.ov.unwrap_or(0.0)) {
            g.decay(DECAY_SHORT);
        }
        if step != Step::Done {
            g.decay(DECAY_UNFINISHED);
        }
    }

    /// The groups as stored (`overshoot_parameters`): 16 values, "none"
    /// where stock has none.
    pub fn save(&self) -> String {
        let v = |x: Option<f32>| x.map_or_else(|| "none".to_string(), |x| x.to_string());
        self.groups.iter().flat_map(|g| [v(g.ov), v(g.ov_var), v(g.time), v(g.time_var)]).collect::<Vec<_>>().join(",")
    }

    /// Takes back what `save` gave, when every group is valid (0x4e1d38).
    pub fn load(&mut self, text: &str) {
        let v: Vec<Option<f32>> = text.split(',').map(|s| s.trim().parse().ok()).collect();
        if v.len() != GROUPS * 4 {
            return;
        }
        let groups: [Group; GROUPS] = std::array::from_fn(|i| Group { ov: v[4 * i], ov_var: v[4 * i + 1], time: v[4 * i + 2], time_var: v[4 * i + 3] });
        let valid = groups.iter().all(|g| {
            g.ov.is_none_or(|o| o >= 0.0) && g.ov_var.is_none_or(|x| x >= 0.0)
                && g.time.is_none_or(|t| (0.0..=24.0).contains(&t)) && g.time_var.is_none_or(|x| x >= 0.0)
        });
        if valid {
            self.groups = groups;
            self.track = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(controller: Controller, action: Action, operation: Operation) -> Decision {
        Decision { controller, action, operation, lockout: None, fan: false, humidity: super::super::humidity::Op::Off }
    }

    #[test]
    fn a_heat_cycles_coast_is_learned_and_a_gated_stop_without_one_shrinks_it() {
        let mut l = Learner::default();
        // A first cycle only learns its time to peak (its variance starts
        // at 0, so no overshoot is within it): start from one learned.
        l.groups[0].time = Some(0.1);
        let heating = d(Controller::LearningBand, Action::Heat, Operation::Stage1Heat);
        let gate = d(Controller::Overshoot, Action::Drift, Operation::Off);
        let idle = d(Controller::None, Action::Drift, Operation::Off);
        // Stopped at 21.19, coasts to 21.6 in 6 minutes, then falls back.
        l.update(heating, &gate, 21.19, 0.0, 0);
        for (i, room) in [21.3, 21.45, 21.6, 21.5, 21.3].iter().enumerate() {
            l.update(idle, &idle, *room, 120.0 * (i + 1) as f64, 0);
        }
        let ov = l.groups[0].ov.unwrap();
        assert!((ov - (0.2 + 0.05 * (0.41 - 0.2))).abs() < 0.001, "{ov}");
        assert!(l.groups[0].time.is_some_and(|t| (t - 0.1).abs() < 0.001));
        // Gated again with no coast at all: shrunk by a tenth, and a little more.
        l.update(heating, &gate, 21.19, 10_000.0, 0);
        l.update(idle, &idle, 21.0, 10_000.0 + 600.0, 0);
        assert!(l.groups[0].ov.unwrap() < ov * 0.9);
        let mut back = Learner::default();
        back.load(&l.save());
        assert_eq!(back.groups, l.groups);
    }
}
