//! Time to temperature, as stock nlclient's model gives it (0x4e2d80
//! estimate, 0x4e24c4 / 0x4e26f0 learning, 0x4e3570 / 0x4e3980 slots,
//! 0x4e33f0 display gate): per stage, the seconds a run takes to move the
//! room by 0, 0.05, 0.25 … 4 °C, then a rate past 4 °C; learned from
//! every run, shown only for a run a manual change started at once.

use super::control::{Action, Decision, Operation};

/// The temperature changes the curves are kept at (°C).
const STEPS: [f32; 11] = [0.0, 0.05, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 2.5, 3.0, 4.0];
/// Stock's curves before learning: seconds at each step, and °C an hour
/// past 4 °C.
const HEAT: ([f32; 11], f32) = ([0.0, 468.0, 697.0, 808.0, 892.0, 1552.0, 2012.0, 2704.0, 3143.0, 3802.0, 4517.0], 0.9);
const COOL: ([f32; 11], f32) = ([0.0, 315.0, 580.0, 835.0, 958.0, 1453.0, 2507.0, 4089.0, 4929.0, 6191.0, 8202.0], 1.1);
/// The longest estimate (s).
const CAP: f64 = 10800.0;
/// Slots: cool stages 1 to 3, heat stages 1 and 2, then the rest of heat.
const SLOTS: usize = 6;
pub const COOL_SLOT: usize = 0;
pub const HEAT_SLOT: usize = 3;

fn slot(op: Operation) -> Option<usize> {
    Some(match op {
        Operation::Stage1Cool => 0,
        Operation::Stage2Cool => 1,
        Operation::Stage3Cool => 2,
        Operation::Stage1Heat => 3,
        Operation::Stage2Heat => 4,
        Operation::Stage3Heat | Operation::Stage1HeatAux | Operation::Stage2HeatAux | Operation::AuxOnly | Operation::EmergencyHeat => 5,
        _ => return None,
    })
}

fn defaults(slot: usize) -> ([f32; 11], f32) {
    if slot < 3 { COOL } else { HEAT }
}

/// A run under way: its slot, when and where it began, and when it first
/// moved the room by each step.
#[derive(Clone, Debug, PartialEq)]
struct Run {
    slot: usize,
    at: f64,
    from: f32,
    crossed: [Option<f64>; 11],
}

/// Whether the estimate shows: armed by a manual change of target, shown
/// once that change starts a run at once, gone at any other decision.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Gate {
    #[default]
    Off,
    Armed,
    Shown,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimeToTemp {
    pub curves: [[f32; 11]; SLOTS],
    pub slopes: [f32; SLOTS],
    /// Runs learned per slot.
    pub counts: [u32; SLOTS],
    run: Option<Run>,
    gate: Gate,
}

impl Default for TimeToTemp {
    fn default() -> TimeToTemp {
        TimeToTemp {
            curves: std::array::from_fn(|i| defaults(i).0),
            slopes: std::array::from_fn(|i| defaults(i).1),
            counts: [0; SLOTS],
            run: None,
            gate: Gate::Off,
        }
    }
}

impl TimeToTemp {
    /// Seconds a run of `slot` takes to move the room by `d` °C.
    pub fn seconds(&self, slot: usize, d: f32) -> f32 {
        let t = &self.curves[slot];
        if d >= STEPS[10] {
            return t[10] + (d - STEPS[10]) / self.slopes[slot] * 3600.0;
        }
        let i = STEPS.iter().rposition(|&s| s <= d).unwrap_or(0);
        t[i] + (d - STEPS[i]) / (STEPS[i + 1] - STEPS[i]) * (t[i + 1] - t[i])
    }

    /// A manual change of target: the next decision may show the estimate.
    pub fn arm(&mut self) {
        self.gate = Gate::Armed;
    }

    /// Follows a decision: the run it starts or continues, learned when it
    /// ends; and the gate.
    pub fn update(&mut self, d: &Decision, room: f32, now: f64, tracker_state: u8) {
        let running = matches!(d.action, Action::Heat | Action::Cool) && d.lockout.is_none();
        let op_slot = running.then(|| slot(d.operation)).flatten();
        self.gate = match (self.gate, op_slot) {
            (Gate::Armed, Some(_)) if tracker_state == 2 => Gate::Shown,
            (Gate::Shown, Some(_)) => Gate::Shown,
            _ => Gate::Off,
        };
        match (&mut self.run, op_slot) {
            (Some(r), Some(s)) if r.slot == s => {
                let moved = (room - r.from).abs();
                for (i, step) in STEPS.iter().enumerate() {
                    if r.crossed[i].is_none() && moved >= *step {
                        r.crossed[i] = Some(now - r.at);
                    }
                }
            }
            (_, s) => {
                if let Some(done) = self.run.take() {
                    self.learn(&done);
                }
                self.run = s.map(|slot| Run { slot, at: now, from: room, crossed: [Some(0.0), None, None, None, None, None, None, None, None, None, None] });
            }
        }
    }

    /// Feeds a finished run into its slot (0x4e24c4): each step it reached
    /// pulls the curve its way, less as more runs are learned.
    fn learn(&mut self, r: &Run) {
        let n = self.counts[r.slot];
        let w = if n < 20 { (0.75 + 0.01 * n as f32).clamp(0.7, 0.95) } else { 0.95 };
        let t = &mut self.curves[r.slot];
        let mut any = false;
        for (i, crossed) in r.crossed.iter().enumerate().skip(1) {
            if let Some(e) = crossed {
                t[i] = w * t[i] + (1.0 - w) * *e as f32;
                any = true;
            }
        }
        if !any {
            return;
        }
        self.counts[r.slot] = n + 1;
        debug!("hvac: time to temperature learned slot {} (run {}): {}", r.slot, n + 1,
               r.crossed.iter().zip(STEPS).filter_map(|(c, s)| c.map(|c| format!("{s} °C {c:.0} s"))).skip(1).collect::<Vec<_>>().join(", "));
        // A curve that stops rising goes back to stock's.
        if t.windows(2).any(|p| p[1] < p[0]) {
            *t = defaults(r.slot).0;
        }
    }

    /// Seconds until the room reaches `target`, while the gate shows it
    /// (0x4e2d80): the curve from where the run began to the target, less
    /// the part already run; at most 3 hours.
    pub fn estimate(&self, room: f32, target: f32, heating: bool) -> Option<f64> {
        if self.gate != Gate::Shown {
            return None;
        }
        let r = self.run.as_ref()?;
        if (heating && room >= target) || (!heating && room <= target) {
            return None;
        }
        let left = self.seconds(r.slot, (r.from - target).abs()) - self.seconds(r.slot, (room - r.from).abs());
        Some(f64::from(left.max(0.0)).min(CAP))
    }

    /// The curves as stored: six slots of eleven times, their rates, then
    /// their counts.
    pub fn save(&self) -> String {
        let mut v: Vec<String> = self.curves.iter().flatten().map(|t| t.to_string()).collect();
        v.extend(self.slopes.iter().map(|s| s.to_string()));
        v.extend(self.counts.iter().map(|c| c.to_string()));
        v.join(",")
    }

    /// Takes back what `save` gave; a slot that does not rise stays at
    /// stock's curve.
    pub fn load(&mut self, text: &str) {
        let v: Vec<f32> = text.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        if v.len() != SLOTS * 11 + SLOTS * 2 {
            return;
        }
        for s in 0..SLOTS {
            let t: [f32; 11] = std::array::from_fn(|i| v[s * 11 + i]);
            if t.windows(2).all(|p| p[1] >= p[0]) {
                self.curves[s] = t;
                self.slopes[s] = v[SLOTS * 11 + s].clamp(1.0 / 24.0, 5.0);
                self.counts[s] = v[SLOTS * 12 + s] as u32;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::control::Controller;

    fn heating() -> Decision {
        Decision { controller: Controller::LearningBand, action: Action::Heat, operation: Operation::Stage1Heat, lockout: None, fan: false,
                   humidity: super::super::humidity::Op::Off }
    }

    #[test]
    fn stocks_curves_give_the_time_left_after_a_manual_change() {
        let mut m = TimeToTemp::default();
        m.update(&heating(), 19.0, 0.0, 2);
        assert_eq!(m.estimate(19.0, 21.0, true), None, "not armed: a schedule's change shows nothing");
        let mut m = TimeToTemp::default();
        m.arm();
        m.update(&heating(), 19.0, 0.0, 2);
        assert_eq!(m.estimate(19.0, 21.0, true), Some(2704.0), "2 °C on stock's heat curve");
        m.update(&heating(), 20.0, 1000.0, 2);
        assert_eq!(m.estimate(20.0, 21.0, true), Some(2704.0 - 1552.0));
        assert_eq!(m.estimate(21.0, 21.0, true), None, "reached");
        assert_eq!(m.seconds(3, 5.0), 4517.0 + 3600.0 / 0.9);
    }

    #[test]
    fn a_finished_run_pulls_its_curve_and_survives_a_save() {
        let mut m = TimeToTemp::default();
        m.update(&heating(), 19.0, 0.0, 0);
        m.update(&heating(), 20.0, 1000.0, 0);
        m.update(&Decision { action: Action::Drift, operation: Operation::Off, ..heating() }, 20.0, 1100.0, 0);
        assert_eq!(m.counts[3], 1);
        assert!((m.curves[3][5] - (0.75 * 1552.0 + 0.25 * 1000.0)).abs() < 0.01);
        let mut back = TimeToTemp::default();
        back.load(&m.save());
        assert_eq!(back.curves, m.curves);
        assert_eq!(back.counts, m.counts);
    }
}
