//! Which backplate circuit each of stock's logical switches drives, as
//! stock nlclient decides it from the wired terminals and the functions
//! the setup gave the Y2 and STAR terminals (its 0x2e47d0, 0x2e336c and
//! 0x2e3280; client-ng/BACKPLATE.md).

/// Stock's logical switches, in its order (all of them, whether or not
/// the client drives them yet).
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Switch {
    Fan,
    FanCool,
    Stage1Cool,
    Stage2Cool,
    Stage3Cool,
    Stage1Heat,
    Stage2Heat,
    Stage3Heat,
    Stage1AltHeat,
    Stage2AltHeat,
    AuxHeat,
    EmergencyHeat,
    Humidifier,
    Dehumidifier,
    AcDehumidifier,
    Ob,
    WaterHeater,
    Stage2Fan,
    Stage3Fan,
}

impl Switch {
    pub const ALL: [Switch; 19] = [
        Switch::Fan, Switch::FanCool, Switch::Stage1Cool, Switch::Stage2Cool, Switch::Stage3Cool, Switch::Stage1Heat,
        Switch::Stage2Heat, Switch::Stage3Heat, Switch::Stage1AltHeat, Switch::Stage2AltHeat, Switch::AuxHeat,
        Switch::EmergencyHeat, Switch::Humidifier, Switch::Dehumidifier, Switch::AcDehumidifier, Switch::Ob,
        Switch::WaterHeater, Switch::Stage2Fan, Switch::Stage3Fan,
    ];

    /// Stock's name for it, as its logs print it.
    pub fn name(self) -> &'static str {
        ["fan", "fan cool", "stage one cool", "stage two cool", "stage three cool", "stage one heat",
         "stage two heat", "stage three heat", "stage one alternate heat", "stage two alternate heat",
         "auxiliary heat", "emergency heat", "humidifier", "dehumidifier", "ac dehumidifier", "OB",
         "water heater", "stage two fan", "stage three fan"][self as usize]
    }
}

/// Stock's terminal functions: the nine terminals, the functions the Y2
/// terminal can take (9 to 12) and those of the STAR terminal (13 to 19).
const Y1: u8 = 0;
const Y2: u8 = 1;
const G: u8 = 2;
const OB: u8 = 3;
const W1: u8 = 5;
const W2: u8 = 6;

/// A terminal function's pin, for tests.
#[cfg(test)]
pub fn test_index(function: u8) -> usize {
    index(function, false).unwrap()
}

/// A terminal function's byte in MSG_WIRES (the circuit) or MSG_PINS.
fn index(function: u8, wires: bool) -> Option<usize> {
    Some(match (function, wires) {
        (0, _) => 1,
        (1 | 9..=12, true) => 7,
        (1 | 9..=12, false) => 9,
        (2, true) => 2,
        (2, false) => 5,
        (3, true) => 3,
        (3, false) => 6,
        (4, true) => 9,
        (4, false) => 3,
        (5, _) => 0,
        (6, true) => 4,
        (6, false) => 7,
        (7, true) => 8,
        (7, false) => 2,
        (8, true) => 10,
        (8, false) => 4,
        (13..=19, _) => 11,
        _ => return None,
    })
}

/// What the wiring offers: the pins with a wire, the functions the setup
/// gave the Y2 and STAR terminals (stock's enumerations), and whether a
/// heat pump's W terminals are a fossil-fuel furnace (`dual_fuel_selected`)
/// rather than auxiliary heat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wiring {
    pub pins: [u8; 15],
    pub y2: i32,
    pub star: i32,
    pub dual_fuel: bool,
}

impl Wiring {
    /// Whether a terminal function is wired (stock's 0x2e336c).
    pub(super) fn has(&self, function: u8) -> bool {
        let wired = index(function, false).is_some_and(|i| self.pins[i] == 1);
        wired && match function {
            1 => self.y2 != 3,
            9 => self.y2 == 3,
            10 => self.y2 == 0,
            11 => self.y2 == 1,
            12 => self.y2 == 2,
            13 => self.star == 5,
            14 => self.star == 3,
            15 => self.star == 1,
            16 => self.star == 2,
            17 => self.star == 0,
            18 => self.star == 4,
            19 => self.star == -1,
            _ => true,
        }
    }

    /// Whether the equipment has a switch (stock's 0x2e34d0): what its
    /// `has_*` keys and its HVAC control's stages come from. Stock's flag
    /// that lets Y2 as G2 or STAR as g3 be heat stages (its globals+0x1078)
    /// is not identified and taken as off.
    pub fn exists(&self, s: Switch) -> bool {
        let h = |f: u8| self.has(f);
        let ob = h(OB);
        let stage_flag = false;
        match s {
            Switch::Fan => h(G),
            Switch::FanCool => h(G) && h(Y1),
            Switch::Stage1Cool | Switch::AcDehumidifier => h(Y1),
            Switch::Stage2Cool => {
                if ob { (h(Y1) && h(9)) || h(12) || h(10) } else { h(Y2) || (h(Y1) && h(9)) }
            }
            Switch::Stage3Cool => h(Y1) && h(13),
            Switch::Stage1Heat => h(W1) || ob,
            Switch::Stage2Heat => {
                let g2 = h(W1) && h(9) && stage_flag;
                if ob { g2 || h(11) || h(10) } else { h(W2) || g2 }
            }
            Switch::Stage3Heat => (!ob && h(14)) || (h(W1) && h(13) && stage_flag),
            Switch::Stage1AltHeat => ob && self.dual_fuel && (h(W1) || h(W2) || h(14)),
            Switch::Stage2AltHeat => ob && self.dual_fuel && ((h(W1) && (h(W2) || h(14))) || (!h(W1) && h(W2) && h(14))),
            Switch::AuxHeat => ob && !self.dual_fuel && (h(W1) || h(W2) || h(14)),
            Switch::EmergencyHeat => h(17),
            Switch::Humidifier => h(15),
            Switch::Dehumidifier => h(16),
            Switch::Ob => ob,
            Switch::WaterHeater => h(18),
            Switch::Stage2Fan => h(9),
            Switch::Stage3Fan => h(13),
        }
    }

    /// The backplate's own safety temperatures, 0x89's payload (stock's
    /// 0x316c84): heat around `low` on the first wired of emergency,
    /// alternate, aux and the heat stages (none when that is a heat pump's
    /// compressor), cool around `high` on the highest cool stage, each
    /// with the fan, cool with O/B when O energises on cool, and the
    /// backplate's compressor lockout. `heat_fan`: heat needs the
    /// thermostat's fan.
    pub fn safety_payload(&self, low: Option<f32>, high: Option<f32>, heat_fan: bool, ob_cool: bool, lockout: u16) -> [u8; 16] {
        // Centi-degrees around a 16.16 temperature, as stock computes them.
        let centi = |c: f32, d: i32, sign: i32| {
            let t = (c * 65536.0) as i32 + sign * d;
            ((t as f32 * (1.0 / 65536.0)) * 100.0) as i32 as i16
        };
        let bit = |s: Switch| self.circuit(s).map_or(0u16, |c| 1 << c);
        let mut enable = 0u16;
        let (mut heat, mut cool) = ([0i16; 2], [0i16; 2]);
        let (mut heat_mask, mut cool_mask) = (0u16, 0u16);
        let heat_chain = [Switch::EmergencyHeat, Switch::Stage2AltHeat, Switch::Stage1AltHeat, Switch::AuxHeat,
                          Switch::Stage3Heat, Switch::Stage2Heat, Switch::Stage1Heat];
        if let (Some(low), Some(&s)) = (low, heat_chain.iter().find(|&&s| self.exists(s))) {
            let compressor = self.exists(Switch::Ob) && matches!(s, Switch::Stage1Heat | Switch::Stage2Heat);
            if !compressor {
                heat_mask = bit(s) | if self.exists(Switch::Fan) && heat_fan { bit(Switch::Fan) } else { 0 };
                if heat_mask != 0 {
                    enable |= 1;
                    heat = [centi(low, 0x63d7, -1), centi(low, 0x63d7, 1)];
                }
            }
        }
        let cool_chain = [Switch::Stage3Cool, Switch::Stage2Cool, Switch::Stage1Cool];
        if let (Some(high), Some(&s)) = (high, cool_chain.iter().find(|&&s| self.exists(s))) {
            cool_mask = bit(s) | if self.exists(Switch::Fan) { bit(Switch::Fan) } else { 0 };
            if cool_mask != 0 {
                if self.exists(Switch::Ob) && ob_cool {
                    cool_mask |= bit(Switch::Ob);
                }
                enable |= 2;
                cool = [centi(high, 0x8f5c, -1), centi(high, 0x8f5c, 1)];
            }
        }
        let mut p = [0u8; 16];
        for (i, v) in [enable, heat[0] as u16, heat[1] as u16, heat_mask, cool[0] as u16, cool[1] as u16, cool_mask, lockout].iter().enumerate() {
            p[2 * i..2 * i + 2].copy_from_slice(&v.to_le_bytes());
        }
        p
    }

    /// The circuit each switch drives, by `Switch as usize`.
    pub fn circuits(&self) -> [Option<u8>; 19] {
        Switch::ALL.map(|s| self.circuit(s))
    }

    /// The circuit a switch drives, when the wiring has one for it (stock's
    /// 0x2e47d0).
    pub fn circuit(&self, s: Switch) -> Option<u8> {
        let heat_pump = self.has(OB);
        let alternate = || -> Option<u8> {
            if self.has(W1) { Some(W1) } else if self.has(W2) { Some(W2) } else if self.has(14) { Some(14) } else { None }
        };
        let function = match s {
            Switch::Fan | Switch::FanCool => G,
            Switch::Stage1Cool | Switch::AcDehumidifier => Y1,
            Switch::Stage2Cool => if self.has(Y1) && self.has(9) { 9 } else { Y2 },
            Switch::Stage3Cool | Switch::Stage3Fan => 13,
            Switch::Stage1Heat => if heat_pump { Y1 } else { W1 },
            Switch::Stage2Heat => {
                if heat_pump { Y2 } else if self.has(W1) { if self.has(9) { 9 } else { W2 } } else { W2 }
            }
            Switch::Stage3Heat => if self.has(W1) { if self.has(13) { 13 } else { 14 } } else { 14 },
            Switch::Stage1AltHeat => alternate()?,
            Switch::Stage2AltHeat => {
                if self.has(W1) && self.has(W2) { W2 } else if self.has(14) { 14 } else { return None }
            }
            Switch::AuxHeat => if heat_pump { alternate()? } else { return None },
            Switch::EmergencyHeat => 17,
            Switch::Humidifier => 15,
            Switch::Dehumidifier => 16,
            Switch::Ob => OB,
            Switch::WaterHeater => 18,
            Switch::Stage2Fan => 9,
        };
        index(function, true).map(|i| i as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pins(terminals: &[u8]) -> [u8; 15] {
        let mut p = [0; 15];
        for &t in terminals {
            p[index(t, false).unwrap()] = 1;
        }
        p
    }

    #[test]
    fn a_conventional_system_heats_on_w1_cools_on_y1_and_fans_on_g() {
        // nest-qemu's wiring: Rh, C, W1, Y1, G.
        let w = Wiring { pins: pins(&[8, 7, W1, Y1, G]), y2: -1, star: -1, dual_fuel: false };
        assert_eq!(w.circuit(Switch::Stage1Heat), Some(0));
        assert_eq!(w.circuit(Switch::Stage1Cool), Some(1));
        assert_eq!(w.circuit(Switch::Fan), Some(2));
        assert_eq!(w.circuit(Switch::AuxHeat), None, "no aux without a heat pump");
    }

    #[test]
    fn a_heat_pump_heats_and_cools_on_its_compressor() {
        let w = Wiring { pins: pins(&[8, Y1, G, OB, W2]), y2: -1, star: -1, dual_fuel: false };
        assert_eq!(w.circuit(Switch::Stage1Heat), Some(1));
        assert_eq!(w.circuit(Switch::Ob), Some(3));
        assert_eq!(w.circuit(Switch::AuxHeat), Some(4));
    }

    #[test]
    fn the_safety_payload_is_stocks() {
        let w = Wiring { pins: pins(&[8, 7, W1, Y1, G]), y2: -1, star: -1, dual_fuel: false };
        let p = w.safety_payload(Some(4.4444), Some(35.0), true, true, 300);
        let v = |i: usize| i16::from_le_bytes([p[i], p[i + 1]]);
        assert_eq!(p[0], 3);
        assert_eq!((v(2), v(4)), (405, 483), "4.4444 °C ∓ 0.39");
        assert_eq!(u16::from_le_bytes([p[6], p[7]]), 1 << 0 | 1 << 2, "W1 and G");
        assert_eq!((v(8), v(10)), (3444, 3555));
        assert_eq!(u16::from_le_bytes([p[12], p[13]]), 1 << 1 | 1 << 2, "Y1 and G");
        assert_eq!(u16::from_le_bytes([p[14], p[15]]), 300, "the lockout");
        let hp = Wiring { pins: pins(&[8, 7, Y1, G, OB]), y2: -1, star: -1, dual_fuel: false };
        assert_eq!(hp.safety_payload(Some(4.4444), None, true, true, 0)[0], 0, "a heat pump's compressor gets no heat side");
    }

    #[test]
    fn the_setup_gives_y2_and_star_their_equipment() {
        let mut w = Wiring { pins: pins(&[8, 7, Y1, G, OB, W1, Y2, 17]), y2: 1, star: 0, dual_fuel: false };
        assert!(w.exists(Switch::Stage2Heat), "Y2 heating: heat pump stage 2");
        assert!(!w.exists(Switch::Stage2Cool));
        assert!(w.exists(Switch::EmergencyHeat), "STAR as emergency heat");
        assert!(w.circuit(Switch::EmergencyHeat).is_some());
        assert!(w.exists(Switch::AuxHeat) && !w.exists(Switch::Stage1AltHeat));
        w.dual_fuel = true;
        assert!(!w.exists(Switch::AuxHeat) && w.exists(Switch::Stage1AltHeat), "dual fuel: W1 is the furnace");
        w.y2 = 2;
        assert!(w.exists(Switch::Stage2Cool) && !w.exists(Switch::Stage2Heat));
        w.star = 1;
        assert!(w.exists(Switch::Humidifier) && !w.exists(Switch::EmergencyHeat));
    }
}
