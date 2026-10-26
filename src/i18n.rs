//! The client's texts in the language chosen. The client is written in
//! English; each text is found in stock's en_US strings, by its words
//! (case and line breaks aside), and that key's text in the language is
//! shown. Stock's strings stay on the unit
//! (/nestlabs/share/client/locales/<language>.lproj/Localizable.strings);
//! files of the same form beside the menus (locales/<language>.lproj/…)
//! add languages, or replace texts, without a build.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

/// Where stock keeps its strings on the unit.
fn unit_locales() -> PathBuf {
    crate::platform::share().join("client/locales")
}
/// The language the client is written in.
pub const ENGLISH: &str = "en_US";

/// The texts in use: English text (as `normal` makes it) to the language's,
/// stock's texts with values in them as templates, and what was shown.
struct Texts {
    /// The language's texts by key, for texts that name theirs.
    keys: HashMap<String, String>,
    exact: HashMap<String, String>,
    templates: Vec<Template>,
    shown: Mutex<HashMap<String, String>>,
}

static TEXTS: RwLock<Option<Texts>> = RwLock::new(None);

/// A text with values in it ("IN %d MIN"): the English around the values,
/// lowercase, and the language's text with its placeholders.
struct Template {
    around: Vec<String>,
    text: String,
}

/// printf's placeholders as stock's strings use them, as the text's pieces
/// around them; None for a text without any.
fn pieces(text: &str) -> Option<Vec<String>> {
    let mut out = vec![String::new()];
    let mut chars = text.chars().peekable();
    let mut any = false;
    while let Some(c) = chars.next() {
        if c != '%' {
            out.last_mut().unwrap().push(c);
            continue;
        }
        if chars.peek() == Some(&'%') {
            chars.next();
            out.last_mut().unwrap().push('%');
            continue;
        }
        // %1$s, %.1f, %lld, %@ …
        while chars.peek().is_some_and(|c| c.is_ascii_digit() || matches!(c, '$' | '.' | '-' | 'l')) {
            chars.next();
        }
        chars.next();
        any = true;
        out.push(String::new());
    }
    any.then_some(out)
}

/// The language's text with `values` in its placeholders: in order, or
/// where a placeholder names one (%2$s).
pub fn fill(text: &str, values: &[String]) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    let mut next = 0;
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'%') {
            chars.next();
            out.push('%');
            continue;
        }
        let mut spec = String::new();
        while chars.peek().is_some_and(|c| c.is_ascii_digit() || matches!(c, '$' | '.' | '-' | 'l')) {
            spec.push(chars.next().unwrap());
        }
        chars.next();
        let at = spec.split_once('$').and_then(|(n, _)| n.parse::<usize>().ok()).map(|n| n - 1).unwrap_or_else(|| {
            next += 1;
            next - 1
        });
        out.push_str(values.get(at).map_or("", String::as_str));
    }
    out
}

impl Template {
    /// The values `text` (words one space apart) has where the template
    /// has placeholders, when it fits.
    fn values(&self, text: &str) -> Option<Vec<String>> {
        let lower = text.to_lowercase();
        if lower.len() != text.len() {
            return None;
        }
        let first = &self.around[0];
        if !lower.starts_with(first.as_str()) {
            return None;
        }
        let mut at = first.len();
        let mut values = Vec::new();
        for (i, piece) in self.around.iter().enumerate().skip(1) {
            let last = i == self.around.len() - 1;
            let end = if last && piece.is_empty() {
                lower.len()
            } else if last {
                if !lower.ends_with(piece.as_str()) || lower.len() < at + piece.len() {
                    return None;
                }
                lower.len() - piece.len()
            } else {
                at + lower[at..].find(piece.as_str())?
            };
            // A value is a name, a time, a number or a version (6.4-8): a
            // few words, not a clause ("closer to the Heat Link, then" is no
            // "%s to %s").
            let value = &text[at..end];
            if end <= at || value.split_whitespace().count() > 3 || value.contains([',', '.']) && !value.trim().chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-')) {
                return None;
            }
            values.push(value.to_string());
            at = end + piece.len();
        }
        Some(values)
    }
}

/// A `.strings` file's keys and texts: `"key" = "text";` lines, with C
/// escapes and comments.
pub fn parse(source: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut chars = source.chars().peekable();
    let mut quoted = || -> Option<String> {
        loop {
            match chars.next()? {
                '"' => break,
                '/' if chars.peek() == Some(&'*') => {
                    let mut last = ' ';
                    for c in chars.by_ref() {
                        if last == '*' && c == '/' {
                            break;
                        }
                        last = c;
                    }
                }
                '/' if chars.peek() == Some(&'/') => {
                    for c in chars.by_ref() {
                        if c == '\n' {
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
        let mut s = String::new();
        loop {
            match chars.next()? {
                '"' => return Some(s),
                '\\' => match chars.next()? {
                    'n' => s.push('\n'),
                    't' => s.push('\t'),
                    c => s.push(c),
                },
                c => s.push(c),
            }
        }
    };
    while let (Some(key), Some(text)) = (quoted(), quoted()) {
        out.insert(key, text);
    }
    out
}

/// How a text is matched: its words, lowercase, one space apart.
fn normal(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Stock's file for a language the settings name (its Español (MX) is es_US).
fn file_name(language: &str) -> String {
    let language = if language == "es_MX" { "es_US" } else { language };
    format!("{language}.lproj/Localizable.strings")
}

/// A language's strings: stock's on the unit, then the client's own over
/// them.
fn strings(language: &str, own: &Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for dir in [unit_locales().as_path(), own] {
        if let Ok(source) = std::fs::read_to_string(dir.join(file_name(language))) {
            out.extend(parse(&source));
        }
    }
    out
}

/// Uses `language`, from stock's strings and those in `own`; English
/// shows the client's texts as they are.
pub fn select(language: &str, own: &Path) {
    let texts = (language != ENGLISH).then(|| {
        let english = strings(ENGLISH, own);
        let other = strings(language, own);
        let mut exact = HashMap::new();
        let mut templates = Vec::new();
        // Keys in order, so the same English text always takes the same key.
        let mut keys: Vec<&String> = english.keys().collect();
        keys.sort();
        for key in keys {
            if let Some(text) = other.get(key) {
                let words = normal(&english[key]);
                match pieces(&words) {
                    // Words around the values, or it would fit any text.
                    Some(around) if around.iter().any(|p| p.chars().filter(|c| c.is_alphabetic()).count() >= 2) => {
                        templates.push(Template { around, text: text.clone() });
                    }
                    Some(_) => {}
                    None => {
                        exact.entry(words).or_insert_with(|| text.clone());
                    }
                }
            }
        }
        // The most English first: "IN 1 HR %d MIN" before "IN %d MIN".
        templates.sort_by_key(|t| std::cmp::Reverse(t.around.iter().map(String::len).sum::<usize>()));
        debug!("i18n: {language}: {} texts, {} matched exactly, {} templates", other.len(), exact.len(), templates.len());
        if other.is_empty() {
            warn!("i18n: no {} for {language}; texts stay in English", file_name(language));
        }
        Texts { keys: other, exact, templates, shown: Mutex::new(HashMap::new()) }
    });
    info!("i18n: language {language}");
    if let Ok(mut t) = TEXTS.write() {
        *t = texts;
    }
}

/// A text that names its key, "@key|English": where the same English means
/// other things elsewhere (stock's "Heating" as equipment, not as status).
/// Its English may take lines; a line naming a key of its own starts
/// another text.
fn keyed(line: &str) -> Option<(&str, &str)> {
    line.strip_prefix('@').filter(|l| !l.contains("\n@"))?.split_once('|')
}

/// English: texts that name their key show the English after it.
fn english(text: &str) -> String {
    if !text.contains('@') {
        return text.to_string();
    }
    text.split('\n').map(|line| keyed(line).map_or(line, |(_, e)| e)).collect::<Vec<_>>().join("\n")
}

/// `text` in the language in use, or as it is when it has none there.
pub fn tr(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let Ok(texts) = TEXTS.read() else { return english(text) };
    let Some(texts) = texts.as_ref() else { return english(text) };
    // Texts drawn every frame are looked up once.
    if let Some(t) = texts.shown.lock().ok().and_then(|s| s.get(text).cloned()) {
        return t;
    }
    let one = |line: &str| -> Option<String> {
        if let Some((key, english)) = keyed(line) {
            return Some(texts.keys.get(key).cloned().unwrap_or_else(|| english.to_string()));
        }
        let words = normal(line);
        texts.exact.get(&words).cloned().or_else(|| {
            let words = line.split_whitespace().collect::<Vec<_>>().join(" ");
            texts.templates.iter().find_map(|t| t.values(&words).map(|v| fill(&t.text, &v)))
        })
    };
    // Lines the whole does not match, each on its own ("Farsight:" over
    // "Digital Clock").
    // A label's long and short forms, "Run the fan for/Run the fan", each
    // on its own.
    let out = one(text).or_else(|| (text.contains('/') && !text.contains('\n')).then(|| {
        text.split('/').map(|part| one(part).unwrap_or_else(|| part.to_string())).collect::<Vec<_>>().join("/")
    }).filter(|t| t != text)).unwrap_or_else(|| if text.contains('\n') {
        // Each paragraph whole ("…\n\nStatus: On"), else each of its lines.
        text.split("\n\n").map(|para| one(para).unwrap_or_else(|| {
            para.split('\n').map(|line| one(line).unwrap_or_else(|| line.to_string())).collect::<Vec<_>>().join("\n")
        })).collect::<Vec<_>>().join("\n\n")
    } else {
        text.to_string()
    });
    // Once a text: what is shown is kept below.
    if out == english(text) && text.chars().any(char::is_alphabetic) {
        debug!("i18n: no translation for {:?}; English", text.replace('\n', " "));
    }
    if let Ok(mut shown) = texts.shown.lock() {
        if shown.len() > 4096 {
            shown.clear();
        }
        shown.insert(text.to_string(), out.clone());
    }
    out
}

/// Each language's name in itself, as stock's list names them; a language
/// added beside the menus names itself with its "unfr0zen.language" text.
const NAMES: [(&str, &str); 10] = [("de_DE", "DEUTSCH"), ("en_GB", "ENGLISH (UK)"), ("en_US", "ENGLISH (US)"),
    ("es_ES", "ESPAÑOL (ES)"), ("es_MX", "ESPAÑOL (MX)"), ("fr_CA", "FRANÇAIS (CA)"), ("fr_FR", "FRANÇAIS (FR)"),
    ("it_IT", "ITALIANO"), ("nl_NL", "NEDERLANDS"), ("zh_CN", "中文")];

/// Each language's name as stock's Language tile gives it: without the
/// region, in title case.
const TILE_NAMES: [(&str, &str); 10] = [("de_DE", "Deutsch"), ("en_GB", "English"), ("en_US", "English"),
    ("es_ES", "Español"), ("es_MX", "Español"), ("fr_CA", "Français"), ("fr_FR", "Français"),
    ("it_IT", "Italiano"), ("nl_NL", "Nederlands"), ("zh_CN", "中文")];

/// Language `code`'s name on the Language tile, given its name in the list.
pub fn tile_name(code: &str, name: &str) -> String {
    TILE_NAMES.iter().find(|(c, _)| *c == code).map_or(name, |(_, n)| n).to_string()
}

/// The languages installed, stock's and those beside the menus, as the
/// settings name them (es_MX for stock's es_US) with their names, by name.
pub fn installed(own: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for dir in [unit_locales().as_path(), own] {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            let Some(code) = file.strip_suffix(".lproj") else { continue };
            let code = if code == "es_US" { "es_MX" } else { code };
            if out.iter().any(|(c, _)| c == code) || !entry.path().join("Localizable.strings").is_file() {
                continue;
            }
            let name = NAMES.iter().find(|(c, _)| *c == code).map(|(_, n)| n.to_string()).or_else(|| {
                std::fs::read_to_string(entry.path().join("Localizable.strings")).ok().and_then(|s| parse(&s).remove("unfr0zen.language"))
            }).unwrap_or_else(|| code.to_string());
            out.push((code.to_string(), name));
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// The client's own strings beside the menus file.
pub fn own_dir(menus: &Path) -> PathBuf {
    menus.with_file_name("locales")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_installed_are_named_in_themselves_or_by_their_file() {
        let dir = std::env::temp_dir().join(format!("client-langs-{}", std::process::id()));
        for (lang, body) in [("zh_CN", ""), ("pt_PT", "\"unfr0zen.language\" = \"PORTUGUÊS\";\n"), ("xx_XX", "")] {
            std::fs::create_dir_all(dir.join(format!("{lang}.lproj"))).unwrap();
            std::fs::write(dir.join(format!("{lang}.lproj/Localizable.strings")), body).unwrap();
        }
        std::fs::create_dir_all(dir.join("empty.lproj")).unwrap();
        let names: Vec<(String, String)> = installed(&dir).into_iter().filter(|(c, _)| ["zh_CN", "pt_PT", "xx_XX", "empty"].contains(&c.as_str())).collect();
        assert_eq!(names, [("pt_PT".into(), "PORTUGUÊS".into()), ("xx_XX".into(), "xx_XX".into()), ("zh_CN".into(), "中文".into())]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn strings_files_read_with_their_escapes_and_comments() {
        let s = parse("/* a\n * comment */\n\"a.b\" = \"Say \\\"hi\\\"\\nthere\";\n// line\n\"c\" = \"D\";\n");
        assert_eq!(s["a.b"], "Say \"hi\"\nthere");
        assert_eq!(s["c"], "D");
    }

    #[test]
    // Only this test selects a language, and only with texts no other test
    // shows, as the texts in use are the whole client's.
    fn texts_are_found_by_their_words_in_english_and_shown_in_the_language() {
        let dir = std::env::temp_dir().join(format!("client-i18n-{}", std::process::id()));
        for (lang, text) in [("en_US", "Knowing about your home helps your thermostat work better."),
                             ("de_DE", "Wissen über Ihr Zuhause hilft.")] {
            std::fs::create_dir_all(dir.join(format!("{lang}.lproj"))).unwrap();
            std::fs::write(dir.join(format!("{lang}.lproj/Localizable.strings")),
                           format!("\"sel.aw\" = \"{text}\";\n\"settings\" = \"TEST SETTINGS\";\n\"in.aw\" = \"TEST IN 1 HR %d MIN\";\n\"of.aw\" = \"Test %1$d of %2$d left\";\n\"ver.aw\" = \"Test version %s\";\n")).unwrap();
        }
        std::fs::write(dir.join("de_DE.lproj/Localizable.strings"),
                       "\"sel.aw\" = \"Wissen über Ihr Zuhause hilft.\";\n\"settings\" = \"TEST EINSTELLUNGEN\";\n\"in.aw\" = \"TEST IN 1 STD %d MIN\";\n\"of.aw\" = \"Test noch %2$d: %1$d\";\n\"ver.aw\" = \"Testversion %s\";\n").unwrap();
        // Equipment Settings' pages with some of the client's own German
        // (config/locales), texts found by their words and by their keys.
        let words = ["dualfuellayer.choosetypemsg.aw", "dualfuellayer.primary.choice", "humidityequipmentlayer.selectionmsg.dehumto.aw",
                     "humidityquiettimelayer.donotrunhumdehum"];
        let keyed = ["proviewselectionlayer.starwireselectmsg.aw", "humidityquiettimelayer.dehumififier", "pinconnectionslayer.types.12"];
        for (lang, keys) in [("en_US", &words[..]), ("de_DE", &[&words[..], &keyed[..]].concat()[..])] {
            let own = parse(&std::fs::read_to_string(format!("config/locales/{lang}.lproj/Localizable.strings")).unwrap());
            let lines: String = keys.iter().map(|k| format!("\"{k}\" = \"{}\";\n", own[*k].replace('\n', "\\n"))).collect();
            let file = dir.join(format!("{lang}.lproj/Localizable.strings"));
            std::fs::write(&file, std::fs::read_to_string(&file).unwrap() + &lines).unwrap();
        }
        select("de_DE", &dir);
        assert_eq!(tr("Knowing about\nyour home helps your\nthermostat work better."), "Wissen über Ihr Zuhause hilft.");
        assert_eq!(tr("Test settings"), "TEST EINSTELLUNGEN");
        assert_eq!(tr("Knowing about\nyour home helps your\nthermostat work better.\n\nTest settings"),
                   "Wissen über Ihr Zuhause hilft.\n\nTEST EINSTELLUNGEN", "each paragraph whole");
        assert_eq!(tr("Test settings/Knowing about your home helps your thermostat work better."),
                   "TEST EINSTELLUNGEN/Wissen über Ihr Zuhause hilft.", "long and short forms each");
        assert_eq!(tr("Not a stock text"), "Not a stock text");
        assert_eq!(tr("Test 1 of moving your thermostat closer left"), "Test 1 of moving your thermostat closer left", "a clause is no value");
        assert_eq!(tr("Not stock:\nTest settings"), "Not stock:\nTEST EINSTELLUNGEN", "line by line");
        assert_eq!(tr("TEST IN 1 HR 30 MIN"), "TEST IN 1 STD 30 MIN");
        assert_eq!(tr("Test 7 of 9 left"), "Test noch 9: 7");
        assert_eq!(tr("Test version 6.4-8"), "Testversion 6.4-8", "a version is a value");
        assert_eq!(tr("@settings|Its own\n@none|English"), "TEST EINSTELLUNGEN\nEnglish", "texts naming their key");
        assert_eq!(tr("@settings|Its\nown"), "TEST EINSTELLUNGEN", "a key's English in lines");
        use crate::equipment::{star_options, wires, Page, Wiring, STAR};
        let offline = Page::dual_fuel(0.0, "NONE", false).view();
        assert_eq!((offline.prompt.as_deref(), offline.rows[0].label.as_str()), (Some("Auf alternative Heizung umstellen?"), "PRIMÄRE"));
        assert_eq!(Page::star(star_options(false, false, true, true, false), None).view().prompt.as_deref(),
                   Some("Wähle das Gerät für die *-Verbindung aus."), "stock's star glyph as ours");
        let mut dehum = Page::humidity(false, Some(60.0), None, [22, 6]);
        assert_eq!(dehum.view().prompt.as_deref(), Some("Entfeuchten\n60\u{a0}%\u{a0}RH"));
        dehum.wheel(2);
        dehum.press();
        assert_eq!(dehum.view().form.unwrap().fields[0].0, "Entfeuchter nicht benutzen von:", "QUIET TIME, its equipment named in German");
        let get = |k: &str| (k == STAR).then(|| "hum".to_string());
        let pro = Page::wires(wires(&Wiring { wired: vec!["STAR"], heat_pump: false, get: &get })).view();
        assert_eq!(pro.rows[0].label, "STAR: Befeuchter");
        select(ENGLISH, &dir);
        assert_eq!(tr("@settings|Its own"), "Its own");
        assert_eq!(tr("@settings|Its\nown"), "Its\nown");
        assert_eq!(tr("Test settings"), "Test settings");
        let _ = std::fs::remove_dir_all(dir);
    }
}
