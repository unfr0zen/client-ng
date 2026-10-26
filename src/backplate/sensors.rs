//! The backplate's second temperature report (TEMP2_BUF: the head unit's,
//! the grille's and the backplate's own sensors, centi-degrees C), as
//! stock nlclient watches it: its I2C tracker (0x334cc0, 0x334ab4), which
//! resets the backplate and then raises the reattach alert when the
//! sensors read nothing, and its sanity check (0x307d5c), which resets it
//! on a sensor jumping or disagreeing, at most every 6 hours.

/// A day of good readings re-arms the alert after it was shown.
const REARM: f64 = 24.0 * 3600.0;
/// Sanity: samples further apart than this start over; a jump past 5 °C
/// or 40 % of the last reading, or the grille 20 °C from the head unit,
/// is an issue.
const STALE: f64 = 180.0;
const JUMP: f32 = 5.0;
const JUMP_SHARE: f32 = 0.4;
const SPREAD: f32 = 20.0;
/// The sanity reset waits 3 hours after any reset and 6 after the last
/// sensor reset (or the start).
const AFTER_ANY_RESET: f64 = 3.0 * 3600.0;
const AFTER_SENSOR_RESET: f64 = 6.0 * 3600.0;

/// The tracker's states, named as stock logs them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum I2cState {
    Valid,
    SentReset,
    AlertPosted,
    AlertDismissedWaitingForTempSample,
    ReattachedButInvalid,
}

impl I2cState {
    fn name(self) -> &'static str {
        match self {
            I2cState::Valid => "valid",
            I2cState::SentReset => "sentReset",
            I2cState::AlertPosted => "alertPosted",
            I2cState::AlertDismissedWaitingForTempSample => "alertDismissed_waitingForTempSample",
            I2cState::ReattachedButInvalid => "reattachedButInvalid",
        }
    }
}

/// What the tracker asks of the link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum I2cAction {
    /// Reset the backplate (I2CDown, not forced).
    Reset,
    /// The sensors are up (true) or down: the alert follows.
    Up(bool),
    /// `i2c_alert_allowed_to_trigger` changed, to be kept.
    Allowed(bool),
}

/// Whether a reading is nothing: 0 or -0.
fn zero(v: u16) -> bool {
    v & 0x7fff == 0
}

#[derive(Debug)]
pub struct I2c {
    /// The last two samples' head unit and grille readings, newest first.
    ring: [Option<(u16, u16)>; 2],
    pub state: I2cState,
    entered: f64,
    allowed: bool,
    /// Since when good readings have come while the alert is disarmed.
    good_since: Option<f64>,
}

impl I2c {
    pub fn new(now: f64, allowed: bool) -> I2c {
        I2c { ring: [None; 2], state: I2cState::Valid, entered: now, allowed, good_since: None }
    }

    fn goto(&mut self, to: I2cState, now: f64) {
        info!("backplate: I2CCommunication \"{}\" \"{}\" {}", self.state.name(), to.name(), (now - self.entered).max(0.0) as u64);
        self.state = to;
        self.entered = now;
    }

    /// A TEMP2_BUF frame's samples (head unit, grille), at `now`.
    pub fn samples(&mut self, samples: &[(u16, u16)], now: f64) -> Vec<I2cAction> {
        for &s in samples {
            self.ring = [Some(s), self.ring[0]];
        }
        let nothing = |s: Option<(u16, u16)>| s.is_some_and(|(hu, grille)| zero(hu) && zero(grille));
        let something = |s: Option<(u16, u16)>| s.is_some_and(|(hu, grille)| !zero(hu) || !zero(grille));
        let bad = nothing(self.ring[0]) && nothing(self.ring[1]);
        let good = something(self.ring[0]) || (nothing(self.ring[0]) && something(self.ring[1]));
        let mut out = Vec::new();
        match self.state {
            I2cState::Valid if self.allowed => {
                if bad {
                    out.push(I2cAction::Reset);
                    self.ring = [None; 2];
                    self.goto(I2cState::SentReset, now);
                }
            }
            I2cState::Valid => {
                if good {
                    let since = *self.good_since.get_or_insert(now);
                    if now - since > REARM {
                        self.allowed = true;
                        out.push(I2cAction::Allowed(true));
                    }
                } else if bad {
                    self.good_since = None;
                }
            }
            I2cState::SentReset if bad => {
                out.push(I2cAction::Up(false));
                if self.allowed {
                    self.allowed = false;
                    self.good_since = None;
                    out.push(I2cAction::Allowed(false));
                }
                self.goto(I2cState::AlertPosted, now);
            }
            I2cState::SentReset if good => self.goto(I2cState::Valid, now),
            I2cState::AlertDismissedWaitingForTempSample if bad => self.goto(I2cState::ReattachedButInvalid, now),
            I2cState::AlertPosted | I2cState::AlertDismissedWaitingForTempSample | I2cState::ReattachedButInvalid if good => {
                out.push(I2cAction::Up(true));
                self.goto(I2cState::Valid, now);
            }
            _ => {}
        }
        out
    }

    /// The backplate was put back while the alert was up: stock waits for
    /// a sample before telling anything again.
    pub fn reattached(&mut self, now: f64) {
        if self.state == I2cState::AlertPosted {
            self.goto(I2cState::AlertDismissedWaitingForTempSample, now);
        }
    }
}

/// One TEMP2 sample in °C: head unit, grille, backplate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub hu: f32,
    pub grille: f32,
    pub bp: f32,
}

#[derive(Debug)]
pub struct Sanity {
    previous: Option<Sample>,
    at: f64,
    pending: bool,
    sensor_reset: f64,
}

impl Sanity {
    /// Its 6 hours count from `now`, the start.
    pub fn new(now: f64) -> Sanity {
        Sanity { previous: None, at: 0.0, pending: false, sensor_reset: now }
    }

    /// A sample at `now`, `last_reset` being when the backplate was last
    /// reset for any reason: the reset to make, issue (TempSensorIssue)
    /// or one held back (TempSensorIssueDelayed).
    pub fn sample(&mut self, s: Sample, now: f64, last_reset: f64) -> Option<bool> {
        if self.at != 0.0 && now - self.at > STALE {
            self.previous = None;
        }
        let jumped = |x: f32, prev: f32| (x - prev).abs() > JUMP || (x - prev).abs() > JUMP_SHARE * prev.abs();
        let spread = (s.grille - s.hu).abs() > SPREAD;
        let issue = spread || self.previous.is_some_and(|p| jumped(s.hu, p.hu) || jumped(s.grille, p.grille) || jumped(s.bp, p.bp));
        self.previous = Some(s);
        self.at = now;
        if issue {
            warn!("backplate: sensor not ok - hu: {:.2}, grille: {:.2} bp: {:.2} grille-hu: {}", s.hu, s.grille, s.bp, spread);
            self.pending = true;
        } else if !self.pending {
            return None;
        }
        let gate = now - last_reset > AFTER_ANY_RESET && self.sensor_reset.max(last_reset) < now && now - self.sensor_reset > AFTER_SENSOR_RESET;
        if !gate {
            debug!("backplate: sensor reset held back: {:.0} s since the last reset, {:.0} s since the last sensor reset", now - last_reset, now - self.sensor_reset);
            return None;
        }
        self.previous = None;
        self.at = 0.0;
        self.pending = false;
        self.sensor_reset = now;
        Some(issue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensors_reading_nothing_reset_then_alert_until_they_read_again() {
        let mut t = I2c::new(0.0, true);
        assert!(t.samples(&[(2100, 2050)], 1.0).is_empty());
        assert!(t.samples(&[(0, 0x8000)], 2.0).is_empty(), "one empty sample is not enough");
        assert_eq!(t.samples(&[(0, 0)], 3.0), [I2cAction::Reset]);
        assert_eq!(t.samples(&[(0, 0), (0, 0)], 4.0), [I2cAction::Up(false), I2cAction::Allowed(false)]);
        t.reattached(5.0);
        assert_eq!(t.state, I2cState::AlertDismissedWaitingForTempSample);
        assert!(t.samples(&[(0, 0), (0, 0)], 6.0).is_empty());
        assert_eq!(t.state, I2cState::ReattachedButInvalid);
        assert_eq!(t.samples(&[(2100, 2050)], 7.0), [I2cAction::Up(true)]);
        // Disarmed: no reset until a day of good readings.
        assert!(t.samples(&[(0, 0), (0, 0)], 8.0).is_empty());
        assert!(t.samples(&[(2100, 2050)], 10.0).is_empty());
        assert_eq!(t.samples(&[(2100, 2050)], 10.0 + REARM + 1.0), [I2cAction::Allowed(true)]);
    }

    #[test]
    fn a_jumping_sensor_resets_the_backplate_only_after_six_hours() {
        let s = |hu: f32| Sample { hu, grille: 21.0, bp: 22.0 };
        let mut c = Sanity::new(0.0);
        assert_eq!(c.sample(s(21.0), 10.0, 0.0), None);
        assert_eq!(c.sample(s(27.0), 40.0, 0.0), None, "within 6 h of the start: held");
        let late = AFTER_SENSOR_RESET + 100.0;
        assert_eq!(c.sample(s(27.0), late, 0.0), Some(false), "the held issue goes as delayed");
        assert_eq!(c.sample(s(27.0), late + 30.0, 0.0), None);
        let later = late + AFTER_SENSOR_RESET + 100.0;
        assert_eq!(c.sample(s(27.0), later - 30.0, 0.0), None);
        assert_eq!(c.sample(s(50.0), later, later - 3600.0), None, "an hour after another reset: held");
        assert_eq!(c.sample(s(50.0), later + 3.0 * 3600.0, later - 3600.0), Some(true), "the head unit 29 °C off the grille");
        let mut fresh = Sanity::new(0.0);
        assert_eq!(fresh.sample(s(21.0), late, 0.0), None);
        assert_eq!(fresh.sample(s(21.0), late + 30.0, 0.0), None);
        assert_eq!(fresh.sample(Sample { hu: 21.0, grille: 45.0, bp: 22.0 }, late + 60.0, 0.0), Some(true), "grille 24 °C off the head unit");
    }
}
