//! The ambient light sensor's count, as stock smooths it for Auto
//! brightness.

/// The ambient light Auto follows, as stock smooths it (0xc3b00): a mean
/// of up to five samples; a sample more than 150 counts away is dropped
/// once, and a second in a row starts the mean over from it.
#[derive(Default)]
pub struct LightMean {
    mean: u32,
    samples: u32,
    skipped: bool,
}

impl LightMean {
    pub fn add(&mut self, visible: u16) -> u16 {
        let v = u32::from(visible);
        if self.samples != 0 && v.abs_diff(self.mean) > 150 {
            if self.skipped {
                self.mean = v;
                self.samples = 1;
            }
            self.skipped = !self.skipped;
            return self.mean as u16;
        }
        if self.samples < 5 {
            self.samples += 1;
        }
        let n = f64::from(self.samples);
        self.mean = ((f64::from(v) + (n - 1.0) * f64::from(self.mean)) / n).round() as u32;
        self.skipped = false;
        self.mean as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_is_a_mean_of_five_that_drops_one_outlier() {
        let mut m = LightMean::default();
        assert_eq!(m.add(100), 100);
        assert_eq!(m.add(110), 105);
        for _ in 0..10 {
            m.add(110);
        }
        assert_eq!(m.mean, 108, "rounding holds the mean short of the samples");
        assert_eq!(m.add(400), 108, "an outlier is dropped once");
        assert_eq!(m.add(400), 400, "a second starts over");
        assert_eq!(m.add(410), 405);
    }
}
