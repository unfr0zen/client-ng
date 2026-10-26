//! Stock nlclient's keyboard (nlEncoderWheelLayer, 0x12f680): characters
//! round the ring, a page at a time, then SHIFT, SPACE and DELETE, and below
//! the page key, DONE and CANCEL. The wheel turns stock's strip through its
//! 33 places round the ring (0x130234, the WH_ defaults), the key at its
//! place selected as it moves; a press types the key or does what it says.
//! The text always grows and shrinks at its end.

use std::time::Instant;

use crate::scroll::{Scroll, Tuning};

/// The keyboard's strip (0x130234): 33 places in the ring's 360 degrees,
/// impulse 6, spring 0.065, damping 1.1, edge 0.526, power curve 5 (1.3),
/// going round; it takes the wheel's counts as they are.
const WHEEL: Tuning = Tuning { size: 360.0 / 33.0, scaler: 6.0, spring: 0.065, damping: 1.1, edge: 0.526, power: 1.3, wrap: true, factor: 1.0, settle: 1.0 };
/// The places on each page (stock's table 0x705ff8, 33 a page).
const PLACES: usize = 33;

/// A key: a character, or what a function key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Shift,
    Space,
    Delete,
    Page,
    Done,
    Cancel,
}

/// The pages, in stock's table order (0x705ff8): lower, numeric, symbol,
/// accent, upper.
const PAGES: [&str; 5] = [
    "abcdefghijklmnopqrstuvwxyz",
    "0123456789-!@#$%^&*()_+={}",
    "~[]|\\\":;'<>?,./`ÄÖÜÇÑÉÀÅÆ",
    "àáâåäæçèéêëìíîïñòóôöøßúùûü",
    "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
];
/// The page key's caption on each page: the pages it leads to.
const PAGE_LABELS: [&str; 5] = ["123 <?;", "<?; àøü", "àøü abc", "abc 123", "123 <?;"];

/// What a press did.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    Done(String),
    Cancel,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Keyboard {
    pub page: usize,
    /// The selected key's index in `keys()`.
    pub at: usize,
    pub text: String,
    pub max: usize,
    /// What the empty field says ("Custom", "Label").
    pub placeholder: String,
    /// The text is a saved passphrase, shown as dots; the first key
    /// clears it (stock's prefilled field).
    pub masked: bool,
    /// The wheel's strip, its place the selected key's.
    strip: Scroll,
}

/// A page's 33 places as stock's table lays them: its characters, SHIFT
/// (passed over off the letter pages), SPACE, DELETE, the page key, DONE,
/// CANCEL, and empty places (passed over).
fn places(page: usize) -> Vec<Option<Key>> {
    let mut out: Vec<Option<Key>> = PAGES[page].chars().map(|c| Some(Key::Char(c))).collect();
    out.push((page == 0 || page == 4).then_some(Key::Shift));
    out.extend([Some(Key::Space), Some(Key::Delete)]);
    // The symbol page, a character short, has its gap before the page key.
    if out.len() < 29 {
        out.push(None);
    }
    out.extend([Some(Key::Page), Some(Key::Done), Some(Key::Cancel)]);
    out.resize(PLACES, None);
    out
}

impl Keyboard {
    /// Opens on the first page with 'a' selected (0x12f1c8).
    pub fn open(text: &str, max: usize, placeholder: &str) -> Keyboard {
        Keyboard { page: 0, at: 0, text: text.chars().take(max).collect(), max, placeholder: placeholder.to_string(), masked: false,
                   strip: Scroll::new(WHEEL, PLACES, 0) }
    }

    /// Opens on a saved passphrase, shown masked.
    pub fn open_masked(text: &str, max: usize, placeholder: &str) -> Keyboard {
        Keyboard { masked: !text.is_empty(), ..Keyboard::open(text, max, placeholder) }
    }

    /// The field's text as shown.
    pub fn shown(&self) -> String {
        if self.masked { "•".repeat(self.text.chars().count()) } else { self.text.clone() }
    }

    /// The page's keys in ring order; SHIFT only on the letter pages.
    pub fn keys(&self) -> Vec<Key> {
        let mut keys: Vec<Key> = PAGES[self.page].chars().map(Key::Char).collect();
        if self.page == 0 || self.page == 4 {
            keys.push(Key::Shift);
        }
        keys.extend([Key::Space, Key::Delete, Key::Page, Key::Done, Key::Cancel]);
        keys
    }

    pub fn selected(&self) -> Key {
        self.keys()[self.at]
    }

    /// The page key's caption.
    pub fn page_label(&self) -> &'static str {
        PAGE_LABELS[self.page]
    }

    /// A wheel event's counts (the gen 3's, negative clockwise) push the
    /// strip (stock's 0x12f680, event 0x40000004).
    pub fn counts(&mut self, counts: i32, now: Instant) {
        self.strip.wheel(counts, now);
        self.follow();
    }

    /// Steps the strip to `now`, the selection with it (stock's 0x12f4e0 on
    /// each frame); whether it still moves.
    pub fn tick(&mut self, now: Instant) -> bool {
        let moving = self.strip.step(now);
        self.follow();
        moving
    }

    /// Selects the key at the strip's place; an empty place, or SHIFT off
    /// the letter pages, gives the one before it while the strip turns on,
    /// else the one after (0x12f4e0).
    fn follow(&mut self) {
        let places = places(self.page);
        let mut place = self.strip.index() % PLACES;
        if places[place].is_none() {
            let back = self.strip.velocity() > 0.0;
            place = if back { (place + PLACES - 1) % PLACES } else { (place + 1) % PLACES };
        }
        if let Some(key) = places[place] {
            if let Some(at) = self.keys().iter().position(|&k| k == key) {
                self.at = at;
            }
        }
    }

    /// A key a detent, wrapping round, the strip put at its place.
    pub fn wheel(&mut self, detents: i32) {
        let n = self.keys().len() as i32;
        self.at = (self.at as i32 + detents).rem_euclid(n) as usize;
        self.place();
    }

    /// Puts the strip at rest on the selected key's place.
    fn place(&mut self) {
        let key = self.selected();
        let place = places(self.page).iter().position(|&k| k == Some(key)).unwrap_or(0);
        self.strip.place(place as f32 * WHEEL.size);
    }

    pub fn press(&mut self) -> Outcome {
        // Never the character: the text may be a passphrase.
        debug!("keyboard: {} pressed", match self.selected() {
            Key::Char(_) => "a character",
            Key::Shift => "shift",
            Key::Space => "space",
            Key::Delete => "delete",
            Key::Page => "page",
            Key::Done => "done",
            Key::Cancel => "cancel",
        });
        if self.masked && matches!(self.selected(), Key::Char(_) | Key::Space | Key::Delete) {
            self.masked = false;
            self.text.clear();
            if self.selected() == Key::Delete {
                return Outcome::Stay;
            }
        }
        match self.selected() {
            Key::Char(c) => self.push(c),
            Key::Space => self.push(' '),
            Key::Delete => {
                self.text.pop();
            }
            // SHIFT locks: lower and upper swap until pressed again.
            Key::Shift => self.page = if self.page == 0 { 4 } else { 0 },
            Key::Page => {
                let key = self.at;
                self.page = match self.page {
                    0 | 4 => 1,
                    p => (p + 1) % 4,
                };
                // The selection stays on the page key wherever it moved.
                self.at = self.keys().iter().position(|&k| k == Key::Page).unwrap_or(key);
            }
            Key::Done => return Outcome::Done(self.text.clone()),
            Key::Cancel => return Outcome::Cancel,
        }
        Outcome::Stay
    }

    fn push(&mut self, c: char) {
        if self.text.chars().count() < self.max {
            self.text.push(c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_key_takes_three_detents_from_half_way() {
        // Detents `gap` ms apart, then the strip left to come to rest.
        let turn = |k: &mut Keyboard, counts: i32, n: u64, gap: u64| {
            let start = Instant::now();
            for i in 0..n {
                k.counts(counts, start + Duration::from_millis(i * gap));
                for s in 1..=gap / 17 {
                    k.tick(start + Duration::from_millis(i * gap + s * 17));
                }
            }
            for s in 1..300 {
                k.tick(start + Duration::from_millis(n * gap + s * 17));
            }
        };
        for (n, at) in [(1, 0), (2, 1)] {
            let mut k = Keyboard::open("", 32, "Wi-Fi network");
            turn(&mut k, -120, n, 30);
            assert_eq!(k.at, at, "{n} detents");
        }
        // 26 back: flung, the strip runs on past where slow turns end.
        let mut flung = Keyboard::open("", 32, "Wi-Fi network");
        turn(&mut flung, 120, 26, 30);
        let mut slow = Keyboard::open("", 32, "Wi-Fi network");
        turn(&mut slow, 120, 26, 200);
        let n = slow.keys().len();
        assert_eq!((n - flung.at, n - slow.at), (12, 7));
    }

    #[test]
    fn an_empty_place_and_shift_off_the_letters_are_passed_over() {
        let mut k = Keyboard::open("", 32, "Label");
        k.wheel(-1);
        assert_eq!(k.selected(), Key::Cancel);
        k.strip.place(32.0 * WHEEL.size);
        k.follow();
        assert_eq!(k.selected(), Key::Char('a'), "the empty place after CANCEL, at rest, gives the key after it");
        k.page = 2;
        k.place();
        k.strip.place(25.0 * WHEEL.size);
        k.follow();
        assert_eq!(k.selected(), Key::Space, "the symbol page's SHIFT place, at rest, gives the key after it");
    }

    #[test]
    fn typing_shifting_paging_and_deleting_as_stock() {
        let mut k = Keyboard::open("", 30, "Custom");
        assert_eq!(k.selected(), Key::Char('a'));
        k.wheel(5);
        k.press();
        k.wheel(21);
        assert_eq!(k.selected(), Key::Shift);
        k.press();
        k.wheel(-21);
        k.press();
        assert_eq!(k.text, "fF", "SHIFT locks");
        k.wheel(24);
        assert_eq!(k.selected(), Key::Page);
        k.press();
        assert_eq!((k.page, k.selected(), k.page_label()), (1, Key::Page, "<?; àøü"));
        k.wheel(-1);
        assert_eq!(k.selected(), Key::Delete);
        k.wheel(-1);
        assert_eq!(k.selected(), Key::Space, "no SHIFT on the number page");
        k.wheel(1);
        k.press();
        assert_eq!(k.text, "f");
        k.wheel(2);
        assert_eq!(k.press(), Outcome::Done("f".into()));
        k.wheel(1);
        assert_eq!(k.press(), Outcome::Cancel);
        k.wheel(1);
        assert_eq!(k.selected(), Key::Char('0'), "round the ring");
    }

    #[test]
    fn a_saved_passphrase_shows_as_dots_and_the_first_key_clears_it() {
        let mut k = Keyboard::open_masked("secret", 256, "Password");
        assert_eq!(k.shown(), "••••••");
        k.press();
        assert_eq!((k.shown(), k.masked), ("a".to_string(), false));
    }

    #[test]
    fn the_text_stops_at_its_length() {
        let mut k = Keyboard::open("abc", 3, "Label");
        k.press();
        assert_eq!(k.text, "abc");
    }
}
