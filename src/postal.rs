//! Stock nlclient's postal-code entry (nlLocationLayer): a row of boxes,
//! one per character of the country's format, each `-` until the ring
//! turns it through what that position takes. A press moves to the next
//! box once this one is set; after the last, DONE. Menus and plugins ask
//! for other codes with boxes of their own pattern.

/// What a position takes (stock's format codes, G 7073a0…).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// 2: a digit.
    Digit,
    /// 1: a letter.
    Letter,
    /// 3: a letter or a digit.
    Either,
    /// 11: a letter, a digit or nothing (the UK's last two).
    Optional,
}

impl Slot {
    /// The characters in ring order; a blank (' ') last where allowed.
    fn chars(self) -> Vec<char> {
        let letters = 'A'..='Z';
        let digits = '0'..='9';
        match self {
            Slot::Digit => digits.collect(),
            Slot::Letter => letters.collect(),
            Slot::Either => letters.chain(digits).collect(),
            Slot::Optional => letters.chain(digits).chain([' ']).collect(),
        }
    }
}

/// A country of stock's table (G 0x710d40): its ISO code, its postal
/// format, and where a gap splits the boxes.
pub struct Country {
    pub iso: &'static str,
    pub format: &'static [Slot],
    pub gap: Option<usize>,
}

use Slot::*;
const D4: &[Slot] = &[Digit; 4];
const D5: &[Slot] = &[Digit; 5];
const D6: &[Slot] = &[Digit; 6];

pub const COUNTRIES: [Country; 35] = {
    const fn c(iso: &'static str, format: &'static [Slot]) -> Country {
        Country { iso, format, gap: None }
    }
    [c("CN", D6), c("RU", D6), c("SA", D5), c("AU", D4), c("NZ", D4),
     Country { iso: "CA", format: &[Letter, Digit, Letter, Digit, Letter, Digit], gap: Some(3) },
     c("MX", D5), c("US", D5), c("AT", D4), c("BE", D4), c("BG", D4), c("HR", D5), c("CZ", D5),
     c("DK", D4), c("EE", D5), c("FI", D5), c("FR", D5), c("DE", D5), c("GR", D5), c("HU", D4),
     c("IT", D5), c("LV", D4), c("LT", D5), c("LU", D4), c("NL", &[Digit, Digit, Digit, Digit, Letter, Letter]),
     c("PL", D5), c("PT", D4), c("RO", D6), c("SK", D5), c("SI", D4), c("ES", D5), c("SE", D5),
     c("GB", &[Letter, Either, Either, Either, Either, Optional, Optional]), c("NO", D4), c("CH", D4)]
};

pub fn country(iso: &str) -> Option<&'static Country> {
    COUNTRIES.iter().find(|c| c.iso == iso)
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    Done(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub iso: &'static str,
    pub format: Vec<Slot>,
    pub gap: Option<usize>,
    /// What the boxes ask, when not a postal code.
    pub question: Option<String>,
    /// Each box's character, None while it shows `-`.
    pub chars: Vec<Option<char>>,
    /// The box being set; `chars.len()` is DONE.
    pub at: usize,
}

impl Entry {
    /// Opens on the first box, the stored code filled in where it fits
    /// the format.
    pub fn open(country: &'static Country, stored: &str) -> Entry {
        let mut given = stored.chars();
        let chars = country.format.iter().map(|s| given.next().filter(|c| s.chars().contains(c))).collect();
        Entry { iso: country.iso, format: country.format.to_vec(), gap: country.gap, question: None, chars, at: 0 }
    }

    /// Boxes of a pattern: `9` a digit, `A` a letter, `*` either, `?`
    /// either or nothing, and a space where the first gap falls.
    pub fn pattern(pattern: &str, question: &str, stored: &str) -> Entry {
        let format: Vec<Slot> = pattern.chars().filter_map(|c| match c {
            '9' => Some(Digit),
            'A' => Some(Letter),
            '*' => Some(Either),
            '?' => Some(Optional),
            _ => None,
        }).collect();
        let gap = pattern.find(' ').map(|i| pattern[..i].chars().filter(|&c| c != ' ').count());
        let mut given = stored.chars();
        let chars = format.iter().map(|s| given.next().filter(|c| s.chars().contains(c))).collect();
        Entry { iso: "", format, gap, question: Some(question.to_string()), chars, at: 0 }
    }

    /// USA asks for its ZIP code.
    pub fn prompt(&self) -> String {
        match &self.question {
            Some(q) => q.clone(),
            None if self.iso == "US" => "What is your ZIP code?".into(),
            None => "What is your postal code?".into(),
        }
    }

    /// Turns the box's character, from `-` through its set, stopping at
    /// either end; DONE stays put.
    pub fn wheel(&mut self, detents: i32) {
        let Some(slot) = self.format.get(self.at) else { return };
        let set = slot.chars();
        let now = self.chars[self.at].and_then(|c| set.iter().position(|&s| s == c)).map_or(-1, |i| i as i32);
        let to = (now + detents).clamp(-1, set.len() as i32 - 1);
        self.chars[self.at] = (to >= 0).then(|| set[to as usize]);
    }

    pub fn press(&mut self) -> Outcome {
        if self.at == self.chars.len() {
            return Outcome::Done(self.chars.iter().flatten().filter(|&&c| c != ' ').collect());
        }
        if self.chars[self.at].is_some() {
            self.at += 1;
        }
        Outcome::Stay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_box_is_set_in_turn_then_done_gives_the_code() {
        let mut e = Entry::open(country("NO").unwrap(), "");
        e.press();
        assert_eq!(e.at, 0, "a box stays until it is set");
        e.wheel(-1);
        assert_eq!(e.chars[0], None, "no further back than -");
        e.wheel(3);
        e.press();
        for _ in 0..3 {
            e.wheel(1);
            e.press();
        }
        e.wheel(5);
        assert_eq!(e.press(), Outcome::Done("2000".into()));
    }

    #[test]
    fn the_stored_code_fills_the_boxes_it_fits() {
        let e = Entry::open(country("NL").unwrap(), "1234A");
        assert_eq!(e.chars, [Some('1'), Some('2'), Some('3'), Some('4'), Some('A'), None]);
        assert_eq!(Entry::open(country("US").unwrap(), "").prompt(), "What is your ZIP code?");
    }

    #[test]
    fn the_uks_last_two_may_be_left_blank() {
        let mut e = Entry::open(country("GB").unwrap(), "SW1A1");
        e.at = 5;
        e.wheel(100);
        e.press();
        e.wheel(100);
        e.press();
        assert_eq!(e.press(), Outcome::Done("SW1A1".into()));
    }

    #[test]
    fn a_pattern_makes_its_own_boxes() {
        let e = Entry::pattern("AA 99?", "Code?", "XY12");
        assert_eq!(e.format, [Letter, Letter, Digit, Digit, Optional]);
        assert_eq!((e.gap, e.prompt().as_str()), (Some(2), "Code?"));
        assert_eq!(e.chars, [Some('X'), Some('Y'), Some('1'), Some('2'), None]);
    }
}
