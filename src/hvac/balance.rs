//! Heat Pump Balance's learning, as stock nlclient's heating-rate
//! estimator does it (0x4e00d4 config, 0x4e041c runs, 0x4df84c samples,
//! 0x4dfae8 lockout): each compressor-only heat run from idle gives a
//! heating rate, fitted against the outdoor temperature; the outdoor
//! temperature where that rate falls to the mode's target, plus a margin,
//! is the auxiliary heat's lockout.

use super::control::{Action, Decision, Operation};

/// The outdoor fit's weight for a new run.
const ALPHA: f32 = 0.05;
/// A run shorter than this teaches nothing.
const MIN_RUN: f64 = 300.0;
/// Below this a variance counts as none.
const TINY: f32 = 1.19e-7;

/// Each mode's (`heatpump_savings`: 0 max savings, 1 balanced, 2 max
/// comfort) default lockout, target heating rate (°C/h), margin, and the
/// lockout's bounds (°C).
const MODES: [(f32, f32, f32, f32, f32); 3] = [
    (1.7, 0.6, 0.0, -12.0, 4.5),
    (4.5, 1.1, 1.7, -1.1, 7.2),
    (7.2, 1.7, 2.8, 4.5, 10.0),
];

/// A run's samples (stock's CumulativeMeanAndCovariance): hours since the
/// run began against the room.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Run {
    n: f64,
    mx: f64,
    my: f64,
    sxx: f64,
    sxy: f64,
}

impl Run {
    fn add(&mut self, x: f64, y: f64) {
        self.n += 1.0;
        let dx = x - self.mx;
        self.mx += dx / self.n;
        self.my += (y - self.my) / self.n;
        self.sxx += dx * (x - self.mx);
        self.sxy += dx * (y - self.my);
    }

    /// The least-squares slope, °C an hour, when it can be had.
    fn slope(&self) -> Option<f32> {
        if self.n < 2.0 {
            return None;
        }
        let var = (self.sxx / self.n) as f32;
        (var.abs() > TINY && var >= 0.0).then(|| (self.sxy / self.sxx) as f32)
    }
}

/// The outdoor fit (stock's ExponentiallyWeightedMovingMeanAndCovariance):
/// outdoor temperature against heating rate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Outdoor {
    pub mx: Option<f32>,
    pub vx: f32,
    pub my: f32,
    pub vy: f32,
    pub cov: f32,
}

impl Outdoor {
    fn add(&mut self, x: f32, y: f32) {
        let Some(mx) = self.mx else {
            *self = Outdoor { mx: Some(x), vx: 0.0, my: y, vy: 0.0, cov: 0.0 };
            return;
        };
        let (dx, dy) = (x - mx, y - self.my);
        self.mx = Some(mx + ALPHA * dx);
        self.my += ALPHA * dy;
        self.vx = (1.0 - ALPHA) * (self.vx + ALPHA * dx * dx);
        self.vy = (1.0 - ALPHA) * (self.vy + ALPHA * dy * dy);
        self.cov = (1.0 - ALPHA) * (self.cov + ALPHA * dx * dy);
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Balance {
    pub outdoor: Outdoor,
    run: Option<(f64, Run)>,
}

impl Balance {
    /// The aux lockout for `savings` (3, off: none), at least the
    /// compressor's lockout.
    pub fn lockout(&self, savings: u8, compressor_lockout: Option<f32>) -> Option<f32> {
        let &(default, rate, margin, lo, hi) = MODES.get(usize::from(savings))?;
        let o = &self.outdoor;
        let fit = o.mx.filter(|_| o.vx.abs() > TINY && o.vx >= 0.0).map(|mx| {
            let b = o.cov / o.vx;
            (b, o.my - b * mx)
        });
        let l = match fit {
            Some((b, a)) if b > 0.0 => ((rate - a) / b + margin).clamp(lo, hi),
            Some((0.0, _)) => hi,
            _ => default,
        };
        Some(compressor_lockout.map_or(l, |c| c.max(l)))
    }

    /// Follows a decision with the room at `now` (seconds): a compressor
    /// heat run from idle starts a fit; its end, if 5 minutes long and the
    /// outdoor temperature known, feeds its rate to the outdoor fit.
    pub fn update(&mut self, d: &Decision, previous: Action, room: f32, outdoor: Option<f32>, now: f64) {
        let compressor = matches!(d.operation, Operation::Stage1Heat | Operation::Stage2Heat | Operation::Stage3Heat);
        if let Some((start, run)) = self.run.as_mut() {
            if compressor {
                run.add((now - *start) / 3600.0, f64::from(room));
                return;
            }
            let (start, run) = (*start, *run);
            self.run = None;
            if let (Some(rate), Some(o), true) = (run.slope(), outdoor, now - start >= MIN_RUN) {
                if rate >= 0.0 {
                    self.outdoor.add(o, rate);
                }
                debug!("hvac: heat pump balance: run of {:.0} s heated {rate:.2} °C/h at outdoor {o:.1} °C{}", now - start, if rate < 0.0 { ", not learned" } else { "" });
            } else {
                debug!("hvac: heat pump balance: run of {:.0} s not learned (outdoor {})", now - start, super::control::opt(outdoor));
            }
        }
        if previous == Action::Drift && d.action == Action::Heat && compressor {
            let mut run = Run::default();
            run.add(0.0, f64::from(room));
            self.run = Some((now, run));
        }
    }

    /// The outdoor fit as stored (stock's hre_* values): mean x, variance
    /// x, mean y, variance y, covariance.
    pub fn save(&self) -> String {
        let o = &self.outdoor;
        match o.mx {
            Some(mx) => [mx, o.vx, o.my, o.vy, o.cov].map(|v| v.to_string()).join(","),
            None => String::new(),
        }
    }

    /// Takes back what `save` gave, within stock's bounds (0x4df388).
    pub fn load(&mut self, text: &str) {
        let v: Vec<f32> = text.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        let [mx, vx, my, vy, cov] = v[..] else { return };
        let ok = (-1000.0..=1000.0).contains(&mx) && (-1000.0..=1000.0).contains(&my)
            && (0.0..=1e6).contains(&vx) && (0.0..=1e6).contains(&vy) && (-1e6..=1e6).contains(&cov);
        if ok {
            self.outdoor = Outdoor { mx: Some(mx), vx, my, vy, cov };
            self.run = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::control::Controller;

    fn heat(op: Operation) -> Decision {
        Decision { controller: Controller::LearningBand, action: Action::Heat, operation: op, lockout: None, fan: false,
                   humidity: super::super::humidity::Op::Off }
    }

    fn idle() -> Decision {
        Decision { action: Action::Drift, operation: Operation::Off, ..heat(Operation::Off) }
    }

    /// A 10-minute compressor run heating at `rate` °C an hour.
    fn run(b: &mut Balance, rate: f32, outdoor: f32, at: f64) {
        b.update(&heat(Operation::Stage1Heat), Action::Drift, 20.0, Some(outdoor), at);
        for i in 1..=20 {
            let t = f64::from(i) * 30.0;
            b.update(&heat(Operation::Stage1Heat), Action::Heat, 20.0 + rate * (t / 3600.0) as f32, Some(outdoor), at + t);
        }
        b.update(&idle(), Action::Heat, 21.0, Some(outdoor), at + 630.0);
    }

    #[test]
    fn the_lockout_is_the_default_until_two_outdoor_temperatures_are_seen() {
        let mut b = Balance::default();
        assert_eq!(b.lockout(1, None), Some(4.5));
        run(&mut b, 2.0, 5.0, 0.0);
        assert_eq!(b.lockout(1, None), Some(4.5), "one outdoor temperature: no fit");
        run(&mut b, 3.0, 10.0, 10_000.0);
        // Rate 2 at 5 °C, then 3 at 10 °C weighted 0.05: slope 0.2 and 1.0
        // °C/h at 0 °C outside, so 1.1 °C/h at 0.5 °C, plus balanced's 1.7.
        let l = b.lockout(1, None).unwrap();
        assert!((l - 2.2).abs() < 0.01, "{l}");
        assert_eq!(b.lockout(1, Some(4.0)), Some(4.0), "at least the compressor's");
        assert_eq!(b.lockout(3, None), None, "off");
        let mut back = Balance::default();
        back.load(&b.save());
        assert_eq!(back.outdoor, b.outdoor);
    }

    #[test]
    fn a_short_run_or_aux_teaches_nothing() {
        let mut b = Balance::default();
        b.update(&heat(Operation::Stage1Heat), Action::Drift, 20.0, Some(5.0), 0.0);
        b.update(&heat(Operation::Stage1Heat), Action::Heat, 20.1, Some(5.0), 120.0);
        b.update(&idle(), Action::Heat, 20.2, Some(5.0), 200.0);
        assert_eq!(b.outdoor.mx, None, "under 5 minutes");
        b.update(&heat(Operation::Stage1HeatAux), Action::Drift, 20.0, Some(5.0), 1000.0);
        b.update(&idle(), Action::Heat, 20.5, Some(5.0), 2000.0);
        assert_eq!(b.outdoor.mx, None, "aux from the start is no compressor run");
    }
}
