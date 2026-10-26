//! Stock nlclient's wiring report for units outside the Heat Link gate
//! (its US checks, 0x2e3eec): from the backplate's pins and wires, the
//! circuits it reported faults on and the setup, the codes its wiring
//! report and error pages show, with their texts and links.

use super::wiring::Wiring;

/// A code's page: the label in its badge and link, and its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub code: u8,
    pub label: &'static str,
    pub link: &'static str,
    pub text: &'static str,
}

/// Codes up to this are errors (they raise the alert); notes come after.
pub const LAST_ERROR: u8 = 54;

/// Terminal functions, as `Wiring::has` numbers them.
const Y1: u8 = 0;
const Y2: u8 = 1;
const G: u8 = 2;
const OB: u8 = 3;
const RC: u8 = 4;
const W1: u8 = 5;
const W2: u8 = 6;
const C: u8 = 7;
const RH: u8 = 8;

/// A wire's bit in MSG_WIRES (W1, Y1, G, O/B, AUX/W2, -, -, Y2, C, Rc, Rh,
/// STAR, -).
fn wired(wires: &[u8; 13], i: usize) -> bool {
    wires[i] & 1 != 0
}

/// The codes for the wiring (0x2e3eec): power, faults, equipment, G2 and
/// G3, then the notes when nothing else applies. `faults` are the circuits
/// with an over-current fault.
pub fn codes(w: &Wiring, wires: &[u8; 13], faults: &[bool; 13]) -> Vec<u8> {
    let p = |f: u8| w.has(f);
    let (w_w1, w_y1, w_c, w_rc, w_rh) = (wired(wires, 0), wired(wires, 1), wired(wires, 8), wired(wires, 9), wired(wires, 10));
    let r = if w_rh || w_rc {
        0
    } else if p(RC) && !p(RH) {
        3
    } else if p(RH) && !p(RC) {
        4
    } else {
        2
    };
    let c = if p(C) {
        if w_c { 0 } else { 9 }
    } else if p(Y1) && !w_y1 {
        10
    } else {
        0
    };
    let mut out = Vec::new();
    // A: power, one code.
    let power = if w_w1 { if c != 0 { c } else { r } } else if r != 0 { r } else { c };
    if power != 0 {
        out.push(power);
    }
    // B: faults, every one.
    if out.is_empty() {
        for (circuit, code) in [(0, 32), (1, 33), (2, 34), (3, 35), (4, 36), (7, 37), (11, 38)] {
            if faults[circuit] {
                out.push(code);
            }
        }
    }
    // C: the equipment.
    if out.is_empty() {
        let (w1, w2) = (p(W1), p(W2));
        let code = if p(OB) {
            if !p(Y1) {
                Some(7)
            } else if w.dual_fuel {
                if !w1 && !w2 && !p(14) && !p(19) { Some(14) } else if w1 && w2 && p(14) { Some(15) } else { None }
            } else {
                match (w1, w2, p(14)) {
                    (true, true, true) => Some(16),
                    (true, true, false) => Some(13),
                    (true, false, true) => Some(17),
                    (false, true, true) => Some(18),
                    _ => None,
                }
            }
        } else {
            let t = || if p(14) { Some(20) } else if p(17) { Some(21) } else { None };
            let u = || p(17).then_some(21);
            if !w1 {
                if !p(Y1) { Some(5) } else if w2 { Some(8) } else { t() }
            } else if !p(Y2) {
                if w2 { u() } else { t() }
            } else if !p(Y1) {
                Some(19)
            } else if !w2 && p(14) {
                Some(20)
            } else {
                u()
            }
        };
        out.extend(code);
    }
    // D: Y2 as G2, STAR as G3; more than one may apply.
    if out.is_empty() {
        if p(9) {
            out.extend(if !p(G) { Some(39) } else if p(W2) { Some(40) } else if p(17) { Some(41) } else if p(OB) { Some(43) } else { None });
        } else if p(13) {
            out.push(42);
        }
    }
    // E: notes, pins with no power on their wire.
    if out.is_empty() {
        let pins = &w.pins;
        for (pin, wire, code) in [(3, 9, 55), (4, 10, 56), (0, 0, 57), (1, 1, 58), (7, 4, 59), (5, 2, 60), (6, 3, 61), (9, 7, 62), (11, 11, 63)] {
            if pins[pin] == 1 && !wired(wires, wire) {
                out.push(code);
            }
        }
    }
    out
}

/// Code 10 (no power to Y1, W1 wired, O/B not) as 46 instead when the
/// outdoor temperature was below -10 °C in the last 6 h: a compressor
/// lockout cut the power (stock's 0x230ea8, Backplate-2 on).
pub fn lockout_power(codes: &mut [u8], pins: &[u8; 15], version: i32, cold_recently: bool) {
    if version >= 2 && cold_recently && pins[0] == 1 && pins[6] != 1 {
        for c in codes.iter_mut().filter(|c| **c == 10) {
            *c = 46;
        }
    }
}

/// What the label's number adds to the code, by the backplate's version
/// (stock's 0x5ba29c).
pub fn offset(version: i32) -> u16 {
    match version {
        2 => 20,
        4 | 5 => 70,
        6 | 7 => 150,
        _ => 0,
    }
}

/// A code's text (the wiring report's strings; stock names the STAR
/// terminal's function where this says STAR).
pub fn text(code: u8) -> &'static str {
    match code {
        2 => "No power wires detected. Rh or Rc wires required.",
        3 => "No power to Rc wire detected.",
        4 => "No power to Rh wire detected.",
        5 => "No heating or cooling wires detected. W1 or Y1 wire is required.",
        7 => "Y1 wire not detected. Heat pump requires a Y1 wire.",
        8 => "Additional wire(s) required. AUX/W2 detected by itself.",
        9 => "No power to common (C) wire detected.",
        10 | 58 => "No power to Y1 wire detected.",
        13 | 16 | 17 | 18 => "Heat pump can have at most one auxiliary heat wire.",
        14 => "Dual fuel requires an alternate heat wire.",
        15 => "Dual fuel can have at most two alternate heat wires.",
        19 => "Y2 wire without Y1 wire.",
        20 => "W3 wire without W2/AUX wire.",
        21 => "E wire without O/B wire.",
        32 => "Your thermostat has detected a problem with the W1 wire.",
        33 => "Your thermostat has detected a problem with the Y1 wire.",
        34 => "Your thermostat has detected a problem with the G wire.",
        35 => "Your thermostat has detected a problem with the O/B wire.",
        36 => "Your thermostat has detected a problem with the W2/Aux wire.",
        37 => "Your thermostat has detected a problem with the Y2 wire.",
        38 => "Your thermostat has detected a problem with the STAR wire.",
        39 => "G2 wire detected. G1 wire also required.",
        40 => "G2 wire detected. Remove W2/AUX wire.",
        41 => "G2 wire detected. Remove E wire.",
        42 => "G3 wire detected. G1 and G2 wires also required.",
        43 => "There's a problem with your setup. Both G2 and O/B wires are connected.",
        46 => "Your thermostat isn't getting power from your home's wiring. It will shut down soon and your home will lose heating. Select More Info to fix your wiring now with a short-term solution. Learn more about a long-term solution at",
        55 => "Power wire Rc detected. No connection to equipment.",
        56 => "Power wire Rh detected. No connection to equipment.",
        57 => "No power to W1 wire detected.",
        59 => "No power to AUX/W2 wire detected.",
        60 => "No power to G wire detected.",
        61 => "No power to O/B wire detected.",
        62 => "No power to Y2 wire detected.",
        63 => "No power to STAR wire detected.",
        _ => "",
    }
}

/// A code's entry for a backplate of `version`: "E74" and
/// "nest.com/e74" for code 4 on a Backplate-6; notes are N, Heat Link
/// codes H.
pub fn entry(code: u8, version: i32) -> Entry {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    // Few codes and versions: each label and link is made once and kept.
    type Labels = HashMap<(u8, u16), (&'static str, &'static str)>;
    static LABELS: OnceLock<Mutex<Labels>> = OnceLock::new();
    let off = offset(version);
    let mut labels = LABELS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    let (label, link) = *labels.entry((code, off)).or_insert_with(|| {
        let (letter, n) = match code {
            55..=64 => ('n', u16::from(code) - 54),
            47..=54 => ('h', u16::from(code) - 46),
            _ => ('e', u16::from(code)),
        };
        let n = n + off;
        (Box::leak(format!("{}{n}", letter.to_ascii_uppercase()).into_boxed_str()), Box::leak(format!("nest.com/{letter}{n}").into_boxed_str()))
    });
    Entry { code, label, link, text: text(code) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MSG_PINS and MSG_WIRES for terminals, all with power.
    fn wiring(terminals: &[u8]) -> (Wiring, [u8; 13]) {
        let mut pins = [0; 15];
        let mut wires = [0; 13];
        for &t in terminals {
            let (pin, wire) = match t {
                W1 => (0, 0), Y1 => (1, 1), C => (2, 8), RC => (3, 9), RH => (4, 10),
                G => (5, 2), OB => (6, 3), W2 => (7, 4), Y2 => (9, 7), _ => (11, 11),
            };
            pins[pin] = 1;
            wires[wire] = 1;
        }
        (Wiring { pins, y2: -1, star: -1, dual_fuel: false }, wires)
    }

    #[test]
    fn a_conventional_system_with_power_has_nothing_to_report() {
        let (w, wires) = wiring(&[RH, C, W1, Y1, G]);
        assert!(codes(&w, &wires, &[false; 13]).is_empty());
    }

    #[test]
    fn power_and_wire_problems_give_stocks_codes() {
        let (w, mut wires) = wiring(&[RH, C, W1, Y1, G]);
        wires[10] = 0;
        assert_eq!(codes(&w, &wires, &[false; 13]), [4], "Rh pin without power");
        let (w, mut wires) = wiring(&[RH, C, W1, Y1, G]);
        wires[8] = 0;
        assert_eq!(codes(&w, &wires, &[false; 13]), [9], "C without power, W1 wired: C first");
        let (w, wires) = wiring(&[RH, G]);
        assert_eq!(codes(&w, &wires, &[false; 13]), [5]);
        let (w, wires) = wiring(&[RH, Y1, OB, W1, W2]);
        assert_eq!(codes(&w, &wires, &[false; 13]), [13], "heat pump with W1 and W2");
        let (w, wires) = wiring(&[RH, W1, Y2]);
        assert_eq!(codes(&w, &wires, &[false; 13]), [19]);
        let mut faults = [false; 13];
        faults[1] = true;
        faults[2] = true;
        let (w, wires) = wiring(&[RH, C, W1, Y1, G]);
        assert_eq!(codes(&w, &wires, &faults), [33, 34]);
        let (w, mut wires) = wiring(&[RH, C, W1, Y1, G]);
        wires[2] = 0;
        assert_eq!(codes(&w, &wires, &[false; 13]), [60], "a note");
    }

    #[test]
    fn labels_follow_the_backplate_version() {
        assert_eq!(entry(4, 5).label, "E74");
        assert_eq!(entry(4, 5).link, "nest.com/e74");
        assert_eq!(entry(55, 5).label, "N71");
        assert_eq!(entry(48, 5).label, "H72");
        assert_eq!(entry(9, 3).label, "E9");
    }
}
