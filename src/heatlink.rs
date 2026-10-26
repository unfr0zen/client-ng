//! Stock's Heat Link behaviour the client can follow locally, on a unit
//! inside the Heat Link gate: the wired Heat Link's setup pages opened on
//! a change of wiring, and the delay before its disconnected page at
//! start. The Heat Link's own state (manual mode, hot water, OpenTherm,
//! battery, its serial and versions) reaches stock over Weave only.

use std::time::{Duration, Instant};

/// Stock's HL disconnected alert delay (0x232aa8, timer +0xb8): 30 s, or
/// 45 s on an Agate (globals vtable +0x4b0), which the client does not
/// tell apart.
pub const DISCONNECTED_DELAY: Duration = Duration::from_secs(30);

/// The menus' path to stock's "Heat Link connected / wired only" page.
pub const SETUP: [&str; 4] = ["Settings", "Equipment", "Continue", "Heat Link"];

/// The disconnected page's delay (0x232aa8): the first wiring report
/// since start finding the Heat Link disconnected (H71) holds its page
/// for `DISCONNECTED_DELAY`; a report without H71 ends the hold.
#[derive(Debug, Default)]
pub struct Hold {
    seen: bool,
    until: Option<Instant>,
}

impl Hold {
    /// A wiring report: whether it finds H71. True while its page is held.
    pub fn report(&mut self, disconnected: bool, now: Instant) -> bool {
        if !self.seen {
            self.seen = true;
            if disconnected {
                self.until = Some(now + DISCONNECTED_DELAY);
            }
        }
        if !disconnected {
            self.until = None;
        }
        self.held(now)
    }

    pub fn held(&self, now: Instant) -> bool {
        self.until.is_some_and(|until| now < until)
    }

    /// True once, when the delay has run out: the page is due.
    pub fn expired(&mut self, now: Instant) -> bool {
        let due = self.until.is_some_and(|until| now >= until);
        if due {
            self.until = None;
        }
        due
    }

    /// The delay as though it had run out.
    #[cfg(test)]
    pub fn run_out(&mut self) {
        if self.until.is_some() {
            self.until = Some(Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_report_finding_h71_holds_its_page_for_30_s() {
        let t = Instant::now();
        let mut h = Hold::default();
        assert!(h.report(true, t));
        assert!(h.report(true, t + Duration::from_secs(29)), "a later report keeps the hold");
        assert!(!h.expired(t + Duration::from_secs(29)));
        assert!(h.expired(t + DISCONNECTED_DELAY));
        assert!(!h.expired(t + DISCONNECTED_DELAY), "once");
        assert!(!h.report(true, t + DISCONNECTED_DELAY));
    }

    #[test]
    fn only_the_first_report_since_start_holds() {
        let t = Instant::now();
        let mut h = Hold::default();
        assert!(!h.report(false, t));
        assert!(!h.report(true, t + Duration::from_secs(1)), "H71 found later shows at once");
    }

    #[test]
    fn a_report_without_h71_ends_the_hold() {
        let t = Instant::now();
        let mut h = Hold::default();
        assert!(h.report(true, t));
        assert!(!h.report(false, t + Duration::from_secs(5)));
        assert!(!h.expired(t + DISCONNECTED_DELAY), "nothing due");
        assert!(!h.report(true, t + Duration::from_secs(6)), "nor held again");
    }
}
