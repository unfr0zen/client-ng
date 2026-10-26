//! Technical Info's facts about the unit itself, as stock's systeminfolayer
//! shows them: its battery, its Wi-Fi and addresses, its software and
//! model. Each is a ("tech.<name>", text) pair for the menus' `{key}`s.

use std::process::Command;

/// Stock's settings, kept on the unit where stock left them.
const STOCK_SETTINGS: &str = "/media/user-config/settings.config";

/// The facts, read now (`model` is passed in, read once: it needs a tool).
pub fn gather(model: &str) -> Vec<(&'static str, String)> {
    let mut out = vec![("tech.battery", battery()), ("tech.model", model.to_string())];
    // The network in use: its name, security, connman's strength and the
    // last ping of the router (stock's systeminfolayer).
    let wifi = match crate::wifi::services().into_iter().find(|s| s.connected()) {
        Some(s) => {
            let security = match s.security.as_str() {
                "psk" => "wpa/wpa2",
                "" | "none" => "none",
                other => other,
            };
            let mut text = format!("{}\n{security}\nsignal strength {}", s.name, s.strength);
            if let Some(ms) = ping() {
                text += &format!("\n{ms} ms ping");
            }
            text
        }
        None => "Not connected.".to_string(),
    };
    out.push(("tech.wifi", wifi));
    out.push(("tech.mac", std::fs::read_to_string("/sys/class/net/wlan0/address").unwrap_or_default().trim().replace(':', "")));
    let v4 = run("ip", &["-4", "addr", "show", "wlan0"]);
    let (address, prefix) = inet(&v4, "inet ").unwrap_or_default();
    out.push(("tech.ipv4", address));
    out.push(("tech.mask", prefix.parse::<u32>().map_or_else(|_| "none".into(), mask)));
    let routes = run("ip", &["-4", "route"]);
    out.push(("tech.router", routes.lines().find_map(|l| l.strip_prefix("default via ")?.split_whitespace().next().map(str::to_string))
        .unwrap_or_else(|| "none".into())));
    let v6 = run("ip", &["-6", "addr", "show", "wlan0", "scope", "global"]);
    let (address6, prefix6) = inet(&v6, "inet6 ").or_else(|| inet(&run("ip", &["-6", "addr", "show", "wlan0"]), "inet6 ")).unwrap_or_default();
    out.push(("tech.ipv6", if address6.is_empty() { "none".into() } else { address6 }));
    out.push(("tech.prefix6", if prefix6.is_empty() { "none".into() } else { prefix6 }));
    out.push(("tech.router6", run("ip", &["-6", "route"]).lines().find_map(|l| l.strip_prefix("default via ")?.split_whitespace().next().map(str::to_string))
        .unwrap_or_else(|| "none".into())));
    let resolv = std::fs::read_to_string("/etc/resolv.conf").unwrap_or_default();
    let words = |key: &str| {
        let list: Vec<&str> = resolv.lines().filter_map(|l| l.strip_prefix(key)).flat_map(str::split_whitespace).collect();
        if list.is_empty() { "none".to_string() } else { list.join("\n") }
    };
    out.push(("tech.dns", words("nameserver ")));
    out.push(("tech.search", words("search ")));
    out.push(("tech.software", software()));
    out.push(("tech.serial", crate::net::hostname()));
    // A Heat Link paired under stock: the entry key it was paired with.
    let key = std::fs::read_to_string(STOCK_SETTINGS).ok().and_then(|s| stock_setting(&s, "heatlink_entry_code")).unwrap_or_default();
    out.push(("tech.heatlink.key", key));
    out
}

/// The firmware's version: the kernel's release carries it (4.1.15-6.4-8).
pub fn software() -> String {
    run("uname", &["-r"]).trim().split_once('-').map_or("unknown", |x| x.1).to_string()
}

/// A setting's value in stock's settings.config (`<s key="..." value="..."/>`).
fn stock_setting(text: &str, key: &str) -> Option<String> {
    let at = text.find(&format!("key=\"{key}\""))?;
    let rest = &text[at..text[at..].find('>').map_or(text.len(), |e| at + e)];
    let value = rest.split_once("value=\"")?.1;
    Some(value[..value.find('"')?].to_string())
}

/// The router's round trip, measured off the UI's thread: the last
/// result, and a new measurement started when none is under way.
fn ping() -> Option<u32> {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    static LAST: AtomicU32 = AtomicU32::new(u32::MAX);
    static BUSY: AtomicBool = AtomicBool::new(false);
    if !BUSY.swap(true, Ordering::SeqCst) {
        std::thread::spawn(|| {
            crate::platform::relax();
            let router = run("ip", &["-4", "route"]).lines().find_map(|l| l.strip_prefix("default via ")?.split_whitespace().next().map(str::to_string));
            let ms = router.and_then(|r| {
                let out = run("ping", &["-c", "1", "-W", "2", &r]);
                out.split("time=").nth(1)?.split_whitespace().next()?.parse::<f32>().ok()
            });
            LAST.store(ms.map_or(u32::MAX, |m| m.round() as u32), Ordering::SeqCst);
            BUSY.store(false, Ordering::SeqCst);
        });
    }
    Some(LAST.load(Ordering::SeqCst)).filter(|&m| m != u32::MAX)
}

/// The unit's model, as its bootloader names it (nlmodel: "Display-3.4").
/// The desktop build stands in for nest-qemu's unit, a Display-3.4.
pub fn model() -> String {
    if cfg!(feature = "desktop") {
        return "Display-3.4".into();
    }
    run("fw_printenv", &["-n", "nlmodel"]).trim().to_string()
}

fn battery() -> String {
    match crate::platform::Device::current().battery() {
        v if v > 0.0 => format!("{v:.3} V"),
        _ => "unknown".into(),
    }
}

fn run(program: &str, args: &[&str]) -> String {
    // The unit's tools, which the desktop's host lacks or answers for
    // itself.
    if cfg!(feature = "desktop") {
        return String::new();
    }
    Command::new(program).args(args).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .map_err(|e| debug!("techinfo: {program}: {e}")).unwrap_or_default()
}

/// The first address and prefix length after `kind` ("inet " or "inet6 ").
fn inet(text: &str, kind: &str) -> Option<(String, String)> {
    let cidr = text.lines().find_map(|l| l.trim().strip_prefix(kind))?.split_whitespace().next()?;
    let (address, prefix) = cidr.split_once('/')?;
    Some((address.to_string(), prefix.to_string()))
}

/// A prefix length as a dotted mask: 24 → 255.255.255.0.
fn mask(prefix: u32) -> String {
    let bits = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix.min(32)) };
    bits.to_be_bytes().iter().map(u8::to_string).collect::<Vec<_>>().join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_and_addresses_read_as_stock_shows_them() {
        assert_eq!(mask(24), "255.255.255.0");
        assert_eq!(mask(0), "0.0.0.0");
        let v4 = "3: wlan0: <UP>\n    inet 10.0.2.15/24 brd 10.0.2.255 scope global wlan0\n";
        assert_eq!(inet(v4, "inet "), Some(("10.0.2.15".into(), "24".into())));
    }

    #[test]
    fn stocks_settings_give_the_heat_links_entry_key() {
        let text = "<settings>\n  <s key=\"location_set\" value=\"1\"/>\n  <s key=\"heatlink_entry_code\" value=\"L1E3P3\"/>\n</settings>\n";
        assert_eq!(stock_setting(text, "heatlink_entry_code").as_deref(), Some("L1E3P3"));
        assert_eq!(stock_setting(text, "heatlink_node_id"), None);
    }
}
