//! Stock's wheel-driven strip (its physics object at nlSettingsView+0x90,
//! dec 0x41b214 to 0x41bbc4): each wheel event pushes it, it slows by
//! damping and a spring draws it to the nearest item once the wheel has
//! rested, stepped on the wall clock in 17 ms steps. Positions are in
//! pixels of a 480-pixel face; item `i` sits at `i * size`.

use std::time::Instant;

/// One strip's constants: settings' (`ST_`) or the top menu's (`MN_`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    /// Pixels from one item to the next.
    pub size: f32,
    /// Wheel push to velocity.
    pub scaler: f32,
    /// Spring constant, per step, per pixel.
    pub spring: f32,
    /// Damping ratio.
    pub damping: f32,
    /// What a push past either end keeps.
    pub edge: f32,
    /// The push's power curve (0: none).
    pub power: f32,
    /// Whether the strip goes round (the top menu) or stops at its ends.
    pub wrap: bool,
    /// A wheel event's counts, scaled by the gain, to a push: settings'
    /// view scales them to its 480 pixels (bin 0x1a0b7c); the top menu
    /// takes them as they are, its entries counting the other way round
    /// from stock's (which negates them, dec 0x1a7c94).
    pub factor: f32,
    /// How many times the wall clock's pace it settles at once the wheel
    /// has rested (1: stock's).
    pub settle: f32,
}

/// Settings' carousel (FUN_001afdf8 with the ST_ defaults, power curve 3),
/// settling on a tile twice as fast as stock's.
pub const SETTINGS: Tuning = Tuning { size: 320.0, scaler: 14.5, spring: 0.02, damping: 1.2, edge: 0.526, power: 1.1, wrap: false, factor: 1.5, settle: 2.3 };

/// The top menu's ring (FUN_001ac704 with the MN_ defaults: no power
/// curve), 60 to an entry, going round.
pub const MENU: Tuning = Tuning { size: 60.0, scaler: 2.0, spring: 0.065, damping: 0.91, edge: 0.526, power: 0.0, wrap: true, factor: 1.0, settle: 1.0 };

/// The Schedule screen's timeline (0x1a311c with the SH_ defaults, power
/// curve 6): 192 pixels, eight hours, to an item, from the home dial
/// before Monday to the one after Sunday. Within the week it does not
/// snap (`set_snap`).
pub const SCHEDULE: Tuning = Tuning { size: 192.0, scaler: 40.0, spring: 0.054, damping: 0.91, edge: 0.526, power: 1.4, wrap: false, factor: 1.5, settle: 1.0 };

/// History's day cards (0x135ebc, power curve 1): two thirds of the face
/// to a card.
pub const HISTORY: Tuning = Tuning { size: 320.0, scaler: 12.25, spring: 0.054, damping: 0.91, edge: 0.526, power: 0.95, wrap: false, factor: 1.5, settle: 1.0 };

/// The rotary thread's gain and the settings view's scaling of a wheel
/// event's counts to a push (bin 0x20fe50, 0x1a0b7c).
const GAIN: f32 = -368.0;

/// Steps are 17 ms.
const STEP_MS: f32 = 17.0;

/// The most steps one call takes, however long since the last.
const MAX_STEPS: f32 = 10.0;

/// From when after the last push a strip settles at its tuning's pace:
/// the spring is coming on, the push spent.
const SETTLE_FROM_MS: f32 = 250.0;

/// A wheel-driven strip's state.
#[derive(Clone, Debug, PartialEq)]
pub struct Scroll {
    tuning: Tuning,
    /// Items, from 0.
    count: usize,
    /// Position and velocity (pixels per step).
    x: f32,
    v: f32,
    /// When the wheel last pushed it, and when it last stepped.
    pushed: Option<Instant>,
    stepped: Option<Instant>,
    /// Shared past-the-end slack (stock's one global, here the strip's).
    slack: f32,
    settled: bool,
    /// Whether a spring draws it to the nearest item (stock's +0x1c).
    snap: bool,
    /// A press's glide to the nearest item: from, to, start.
    glide: Option<(f32, f32, Instant)>,
}

impl Scroll {
    /// At rest on item `at`.
    pub fn new(tuning: Tuning, count: usize, at: usize) -> Scroll {
        Scroll { tuning, count, x: at as f32 * tuning.size, v: 0.0, pushed: None, stepped: None, slack: 0.0, settled: true, snap: true, glide: None }
    }

    fn max(&self) -> f32 {
        self.tuning.size * self.count as f32
    }

    /// The position, in items.
    pub fn position(&self) -> f32 {
        self.x / self.tuning.size
    }

    /// The item at the middle: the nearest one.
    pub fn index(&self) -> usize {
        let i = ((self.x + self.tuning.size / 2.0) / self.tuning.size).floor() as i64;
        if self.tuning.wrap {
            i.rem_euclid(self.count.max(1) as i64) as usize
        } else {
            i.clamp(0, self.count.saturating_sub(1) as i64) as usize
        }
    }

    /// The position, in pixels.
    pub fn x(&self) -> f32 {
        self.x
    }

    /// The velocity, in pixels a step.
    pub fn velocity(&self) -> f32 {
        self.v
    }

    /// Puts it at `x` pixels, at rest (stock's 0x41ba80).
    pub fn place(&mut self, x: f32) {
        self.x = x;
        self.v = 0.0;
        self.glide = None;
        self.settled = true;
    }

    /// Stops it where it is (stock's 0x41b53c); a spring may still draw
    /// it on.
    pub fn stop(&mut self) {
        self.v = 0.0;
    }

    /// Turns the spring to the nearest item on or off; without it the
    /// strip comes to rest wherever damping leaves it.
    pub fn set_snap(&mut self, snap: bool) {
        if snap && !self.snap {
            self.settled = false;
        }
        self.snap = snap;
    }

    pub fn moving(&self) -> bool {
        !self.settled || self.glide.is_some()
    }

    /// A wheel event's counts (the gen 3's: negative clockwise).
    pub fn wheel(&mut self, counts: i32, now: Instant) {
        let d = counts as f32 * GAIN / 65536.0 * self.tuning.factor;
        self.push(d, now);
    }

    /// Stock's impulse (FUN_0041b320).
    fn push(&mut self, d: f32, now: Instant) {
        self.step(now);
        self.glide = None;
        let t = &self.tuning;
        let inside = t.wrap || (self.x >= 0.0 && self.x + t.size <= self.max());
        let mut d = d;
        if inside && t.power > 0.0 {
            d = d.signum() * 50.0 * (d.abs() / 50.0).powf(t.power);
        } else if !inside {
            d *= 0.25;
        }
        d *= t.scaler;
        if d != 0.0 {
            self.v += d;
            self.pushed = Some(now);
            self.settled = false;
        }
    }

    /// Glides `items` on from where it rests, or from where a glide was
    /// going, over 0.5 s, easing out: a notch's move.
    pub fn glide_by(&mut self, items: i32, now: Instant) {
        self.step(now);
        let t = self.tuning;
        let from_item = self.glide.map_or((self.x / t.size).round(), |(_, to, _)| to / t.size);
        let mut to = from_item + items as f32;
        if !t.wrap {
            to = to.clamp(0.0, self.count.saturating_sub(1) as f32);
        }
        self.glide = Some((self.x, to * t.size, now));
        self.v = 0.0;
        self.settled = true;
    }

    /// Glides to the nearest item over 0.5 s, easing out, as a press while
    /// it moves does.
    pub fn glide(&mut self, now: Instant) {
        if !self.moving() {
            return;
        }
        let to = self.index() as f32 * self.tuning.size;
        self.glide = Some((self.x, to, now));
        self.v = 0.0;
        self.settled = true;
    }

    /// Steps to `now`; whether it still moves.
    pub fn step(&mut self, now: Instant) -> bool {
        if let Some((from, to, start)) = self.glide {
            let t = (now.duration_since(start).as_secs_f32() / 0.5).min(1.0);
            self.x = from + (to - from) * (std::f32::consts::FRAC_PI_2 * t).sin();
            if t >= 1.0 {
                self.glide = None;
                self.stepped = Some(now);
            }
            return self.moving();
        }
        let Some(last) = self.stepped else {
            self.stepped = Some(now);
            return self.moving();
        };
        let mut dt = now.duration_since(last).as_secs_f32() * 1000.0;
        if dt <= 8.0 || self.settled {
            if self.settled {
                self.stepped = Some(now);
            }
            return self.moving();
        }
        let t = self.tuning;
        let since = self.pushed.map_or(f32::MAX, |p| now.duration_since(p).as_secs_f32() * 1000.0);
        let c = 2.0 * t.damping * t.spring.sqrt();
        let damping = if since < 200.0 { c * (0.2 + 0.8 * since / 200.0) } else { c };
        // Frames drawn slower than the steps still move it at the wall
        // clock's pace, as stock's own timer does; a stall of more than
        // MAX_STEPS steps is taken as that many.
        if since >= SETTLE_FROM_MS {
            dt *= t.settle;
        }
        dt = dt.min(STEP_MS * MAX_STEPS * t.settle.max(1.0));
        self.stepped = Some(now);
        while dt > 0.0 {
            let h = if dt > 25.0 { dt -= STEP_MS; STEP_MS } else { let h = dt; dt = 0.0; h };
            let mut force = -damping * self.v;
            let to = (self.x / t.size).round() * t.size - self.x;
            if self.snap {
                force += if since < 200.0 { 0.0 } else if since < 400.0 { t.spring * to * (since - 200.0) / 200.0 } else { t.spring * to };
            }
            self.v += force * h / STEP_MS;
            let dx = self.v * h / STEP_MS;
            self.x = self.edged(dx);
            if !self.snap {
                if self.v.abs() <= 0.0001 * t.size {
                    self.v = 0.0;
                    self.settled = true;
                    break;
                }
                continue;
            }
            let near = (self.x / t.size).round() * t.size - self.x;
            if self.v.abs() < 0.01 * t.size && near.abs() < 0.001 * t.size {
                self.v = 0.0;
                self.x = self.index() as f32 * t.size;
                self.settled = true;
                break;
            }
        }
        self.moving()
    }

    /// A step `dx` from `x`, held back past either end (bounded strips).
    fn edged(&mut self, dx: f32) -> f32 {
        if self.tuning.wrap {
            return self.x + dx;
        }
        let over = if self.x < 0.0 {
            -self.x
        } else if self.x + self.tuning.size > self.max() {
            self.max() - (self.x + self.tuning.size)
        } else {
            return self.x + dx;
        };
        let mut dx = dx + over * self.tuning.edge;
        let g = dx + self.slack;
        dx = if dx >= 0.0 { dx.min(0.3 * g) } else { dx.max(0.3 * g) };
        self.slack = 0.5 * g;
        self.x + dx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn run(s: &mut Scroll, start: Instant, ms: u64) -> Instant {
        let mut t = start;
        for _ in 0..ms / 17 {
            t += Duration::from_millis(17);
            s.step(t);
        }
        t
    }

    #[test]
    fn a_detent_pushes_about_ten_pixels_a_step() {
        let now = Instant::now();
        let mut s = Scroll::new(SETTINGS, 10, 2);
        s.wheel(-120, now);
        assert!((s.v - 9.92).abs() < 0.05, "{}", s.v);
    }

    #[test]
    fn a_few_detents_glide_back_and_more_reach_the_next_tile() {
        let now = Instant::now();
        let mut s = Scroll::new(SETTINGS, 10, 2);
        s.wheel(-120, now);
        let t = run(&mut s, now, 3000);
        assert!(!s.moving() && s.index() == 2 && s.x == 640.0, "{} {}", s.x, s.index());
        // Seven pushes 30 ms apart (stock's carousel step on nest-qemu).
        let mut t2 = t;
        for _ in 0..7 {
            t2 += Duration::from_millis(30);
            s.wheel(-120, t2);
        }
        run(&mut s, t2, 3000);
        assert_eq!(s.index(), 3);
        assert_eq!(s.x, 960.0);
    }

    #[test]
    fn eight_detents_with_pauses_move_the_top_menu_one_entry() {
        let now = Instant::now();
        let mut s = Scroll::new(MENU, 7, 0);
        let mut t = now;
        for _ in 0..8 {
            t += Duration::from_millis(100);
            s.wheel(-120, t);
            for _ in 0..5 {
                s.step(t + Duration::from_millis(17));
            }
        }
        run(&mut s, t, 3000);
        assert_eq!(s.index(), 1);
    }

    #[test]
    fn the_carousel_stops_at_its_ends() {
        let now = Instant::now();
        let mut s = Scroll::new(SETTINGS, 3, 0);
        let mut t = now;
        for _ in 0..20 {
            t += Duration::from_millis(30);
            s.wheel(120, t);
        }
        run(&mut s, t, 4000);
        assert_eq!(s.index(), 0);
        assert!(!s.moving());
    }

    #[test]
    fn a_press_glides_to_the_nearest_tile_in_half_a_second() {
        let now = Instant::now();
        let mut s = Scroll::new(SETTINGS, 10, 2);
        for i in 0..4 {
            s.wheel(-120, now + Duration::from_millis(30 * i));
        }
        let t = run(&mut s, now, 150);
        s.glide(t);
        let to = s.index() as f32 * 320.0;
        run(&mut s, t, 510);
        assert_eq!(s.x, to);
        assert!(!s.moving());
    }
}
