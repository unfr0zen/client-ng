//! Stock nlclient's backplate firmware update policy (0x306348, 0x3060e0):
//! after each description, the firmware plist (`backplateupdatefile`) is
//! looked up for the backplate's model; a bootloader whose version differs
//! is flashed first, then the firmware when any of its description,
//! version, user, host or date differs. The next description checks each
//! pass; a pass that did not take is started again twice at most.

use super::link::Identity;

/// One image in the plist: its kind ("mono", "bsl"), format ("s-record",
/// "hex"), the models it applies to and what the backplate will report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Image {
    pub kind: String,
    pub format: String,
    pub min: Option<String>,
    pub max: Option<String>,
    pub description: String,
    pub version: String,
    pub user: String,
    pub host: String,
    pub date: String,
    pub data: Vec<u8>,
}

impl Image {
    /// The firmware info string the backplate reports for this image.
    fn info(&self) -> String {
        format!("{} {} {}@{} {}", self.description, self.version, self.user, self.host, self.date)
    }

    /// The S-records, a line each; a last line without its end is not one
    /// (0x32dfb0).
    pub fn records(&self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let mut line = Vec::new();
        for &b in &self.data {
            if b == b'\r' || b == b'\n' {
                if !line.is_empty() {
                    out.push(std::mem::take(&mut line));
                }
            } else {
                line.push(b);
            }
        }
        out
    }
}

/// The plist's production images (Installations > Production).
pub fn images(xml: &str) -> Vec<Image> {
    let Some(start) = xml.find("<key>Production</key>") else { return Vec::new() };
    let rest = &xml[start..];
    let end = rest.find("</array>").unwrap_or(rest.len());
    let mut images = Vec::new();
    for dict in rest[..end].split("<dict>").skip(1) {
        let mut image = Image::default();
        let mut parts = dict.split("<key>").skip(1);
        for part in parts.by_ref() {
            let Some((key, value)) = part.split_once("</key>") else { continue };
            let value = value.trim();
            let text = |tag: &str| value.strip_prefix(&format!("<{tag}>")).and_then(|v| v.split(&format!("</{tag}>")).next()).map(str::to_string);
            match key {
                "Data" => image.data = text("data").map(|b| base64(&b)).unwrap_or_default(),
                "Kind" => image.kind = text("string").unwrap_or_default(),
                "Format" => image.format = text("string").unwrap_or_default(),
                "MinApplicability" => image.min = text("string"),
                "MaxApplicability" => image.max = text("string"),
                "Description" => image.description = text("string").unwrap_or_default(),
                "Version" => image.version = text("string").unwrap_or_default(),
                "User" => image.user = text("string").unwrap_or_default(),
                "Host" => image.host = text("string").unwrap_or_default(),
                "Date" => image.date = text("string").unwrap_or_default(),
                _ => {}
            }
        }
        images.push(image);
    }
    images
}

fn base64(text: &str) -> Vec<u8> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for v in text.bytes().filter_map(value) {
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// Models compared as stock does: case aside, digit runs as numbers.
fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    let split = |s: &str| -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = Vec::new();
        for c in s.to_lowercase().chars() {
            let digit = c.is_ascii_digit();
            match out.last_mut() {
                Some((d, run)) if *d == digit => run.push(c),
                _ => out.push((digit, c.to_string())),
            }
        }
        out
    };
    let (a, b) = (split(a), split(b));
    for (x, y) in a.iter().zip(&b) {
        let o = match (x.0, y.0) {
            (true, true) => x.1.parse::<u64>().unwrap_or(0).cmp(&y.1.parse::<u64>().unwrap_or(0)),
            _ => x.1.cmp(&y.1),
        };
        if o.is_ne() {
            return o;
        }
    }
    a.len().cmp(&b.len())
}

/// The first image of `kind` in `format` for the model (Min <= model < Max).
pub fn find<'a>(images: &'a [Image], kind: &str, format: &str, model: &str) -> Option<&'a Image> {
    images.iter().find(|i| {
        i.kind == kind && i.format.eq_ignore_ascii_case(format)
            && i.min.as_deref().is_none_or(|m| compare(model, m).is_ge())
            && i.max.as_deref().is_none_or(|m| compare(model, m).is_lt())
    })
}

/// A bootloader update when its version differs; a firmware update when
/// anything in its info does.
fn needed(kind: u8, image: &Image, id: &Identity) -> bool {
    match kind {
        b'B' => image.version != id.bsl_version,
        _ => image.info() != id.firmware_info.trim(),
    }
}

/// What the driver wants of the link after a description.
#[derive(Debug, PartialEq)]
pub enum Step {
    Nothing,
    /// Send this kind's S-records.
    Send { kind: u8, records: Vec<Vec<u8>> },
    /// The update finished, well or not.
    Done(bool),
}

#[derive(Default)]
pub struct Updater {
    images: Vec<Image>,
    /// The pass under way ('B' or 'M'), and how often it was started again.
    kind: Option<u8>,
    restarts: u8,
}

impl Updater {
    pub fn new(images: Vec<Image>) -> Updater {
        Updater { images, kind: None, restarts: 0 }
    }

    fn image(&self, kind: u8, model: &str) -> Option<&Image> {
        find(&self.images, if kind == b'B' { "bsl" } else { "mono" }, "s-record", model)
    }

    fn send(&mut self, kind: u8, model: &str) -> Step {
        self.kind = Some(kind);
        let records = self.image(kind, model).map(Image::records).unwrap_or_default();
        info!("backplate: starting {} update", if kind == b'B' { "bsl" } else { "mono" });
        Step::Send { kind, records }
    }

    /// A description arrived: start, check or go on with an update.
    pub fn described(&mut self, id: &Identity) -> Step {
        let need = |k: u8| self.image(k, &id.model).is_some_and(|i| needed(k, i, id));
        let (bsl, mono) = (need(b'B'), need(b'M'));
        match self.kind {
            None if bsl => {
                info!("backplate: bsl update required: version {} for {}", self.image(b'B', &id.model).map_or("", |i| i.version.as_str()), id.bsl_version);
                self.send(b'B', &id.model)
            }
            None if mono => {
                info!("backplate: mono update required: {} for {}", self.image(b'M', &id.model).map_or(String::new(), Image::info), id.firmware_info.trim());
                self.send(b'M', &id.model)
            }
            None => {
                debug!("backplate: firmware matches the source ({})", id.model);
                Step::Nothing
            }
            Some(b'B') if !bsl => {
                info!("backplate: bsl complete, running again to do mono");
                self.restarts = 0;
                if mono { self.send(b'M', &id.model) } else { self.finish(true) }
            }
            Some(b'M') if !bsl && !mono => {
                info!("backplate: mono update complete");
                self.finish(true)
            }
            Some(kind) => {
                warn!("backplate: update failed, {} version still doesn't match source", if bsl { "bsl" } else { "mono" });
                if self.restarts < 2 {
                    self.restarts += 1;
                    info!("backplate: restarting update from beginning ({} of 2)", self.restarts);
                    self.send(if bsl { b'B' } else { kind }, &id.model)
                } else {
                    error!("backplate: failed to update firmware on backplate");
                    self.finish(false)
                }
            }
        }
    }

    fn finish(&mut self, ok: bool) -> Step {
        if ok {
            debug!("backplate: backplate firmware update complete");
        }
        self.kind = None;
        self.restarts = 0;
        Step::Done(ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(kind: &str, version: &str, min: &str, max: &str) -> Image {
        Image { kind: kind.into(), format: "s-record".into(), min: Some(min.into()), max: Some(max.into()), description: if kind == "bsl" { "BSL" } else { "TFE (BP_A2)" }.into(),
                version: version.into(), user: "u".into(), host: "h".into(), date: "2023-09-08 01:43:03".into(), data: b"S0030000FC\r\nS9030000FC\r\n".to_vec() }
    }

    fn identity(mono: &str, bsl: &str) -> Identity {
        Identity { model: "Backplate-6.0".into(), firmware_info: format!("TFE (BP_A2) {mono} u@h 2023-09-08 01:43:03"), bsl_version: bsl.into(), ..Default::default() }
    }

    #[test]
    fn the_plist_gives_each_images_fields_and_bytes() {
        let xml = "<plist><dict><key>Installations</key><dict><key>Production</key><array><dict>\
                   <key>Data</key><data>UzAwMw0K\nUzkwMw0K</data><key>Format</key><string>s-record</string>\
                   <key>Kind</key><string>mono</string><key>MinApplicability</key><string>Backplate-6.0</string>\
                   <key>Version</key><string>2.3.27</string></dict></array><key>Test</key><array></array></dict></dict></plist>";
        let i = images(xml);
        assert_eq!(i.len(), 1);
        assert_eq!((i[0].kind.as_str(), i[0].version.as_str(), i[0].min.as_deref()), ("mono", "2.3.27", Some("Backplate-6.0")));
        assert_eq!(i[0].records(), vec![b"S003".to_vec(), b"S903".to_vec()]);
    }

    #[test]
    fn models_compare_numerically_within_min_and_below_max() {
        let images = [image("mono", "2.3.52", "Backplate-5.20", "Backplate-5.30"), image("mono", "2.3.27", "Backplate-6.0", "Backplate-6.20")];
        assert_eq!(find(&images, "mono", "s-record", "Backplate-5.9").map(|i| &i.version[..]), None, "5.9 is below 5.20");
        assert_eq!(find(&images, "mono", "s-record", "Backplate-5.21").map(|i| &i.version[..]), Some("2.3.52"));
        assert_eq!(find(&images, "mono", "s-record", "Backplate-6.0").map(|i| &i.version[..]), Some("2.3.27"));
        assert_eq!(find(&images, "mono", "s-record", "Backplate-6.20"), None, "max is excluded");
    }

    #[test]
    fn the_bootloader_goes_first_then_the_firmware_then_done() {
        let mut u = Updater::new(vec![image("bsl", "3.1", "Backplate-6.0", "Backplate-6.20"), image("mono", "2.3.27", "Backplate-6.0", "Backplate-6.20")]);
        assert_eq!(u.described(&identity("2.3.27", "3.1")), Step::Nothing);
        assert!(matches!(u.described(&identity("2.3.11", "3.0")), Step::Send { kind: b'B', .. }));
        assert!(matches!(u.described(&identity("2.3.11", "3.1")), Step::Send { kind: b'M', .. }));
        assert_eq!(u.described(&identity("2.3.27", "3.1")), Step::Done(true));
        assert_eq!(u.kind, None);
    }

    #[test]
    fn a_pass_that_does_not_take_is_tried_twice_more() {
        let mut u = Updater::new(vec![image("mono", "2.3.27", "Backplate-6.0", "Backplate-6.20")]);
        let old = identity("2.3.11", "3.1");
        assert!(matches!(u.described(&old), Step::Send { kind: b'M', .. }));
        assert!(matches!(u.described(&old), Step::Send { kind: b'M', .. }));
        assert!(matches!(u.described(&old), Step::Send { kind: b'M', .. }));
        assert_eq!(u.described(&old), Step::Done(false));
    }
}
