//! Wi-Fi through the unit's connman, as stock's nlWiFiScreen drives it:
//! its services (name, strength, security, saved, state, error) for the
//! list, a scan asked for while the list is open, a network joined by
//! connman's provisioning file and a Connect, and AutoConnect moved to the
//! network that works. The unit has no connmanctl; connman's provisioning
//! files and dbus-send are what it has.

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::Instant;

/// connman's store, where it watches for provisioning files.
const CONNMAN: &str = "/var/lib/connman";
/// The provisioning file the client writes, one network at a time.
const PROVISION: &str = "unfr0zen-wifi.config";

/// A Wi-Fi service as connman knows it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Service {
    pub path: String,
    pub name: String,
    /// connman's strength, 0 to 100.
    pub strength: u8,
    /// psk, wep, ieee8021x or none.
    pub security: String,
    /// Saved (connman's Favorite).
    pub saved: bool,
    /// idle, failure, association, configuration, ready, online,
    /// disconnect.
    pub state: String,
    /// connman's Error (invalid-key, connect-failed, dhcp-failed…).
    pub error: String,
}

impl Service {
    pub fn secured(&self) -> bool {
        !self.security.is_empty() && self.security != "none"
    }

    pub fn connected(&self) -> bool {
        matches!(self.state.as_str(), "ready" | "online")
    }

    /// Stock's signal level, 1 to 5 (strength / 21 + 1).
    pub fn level(&self) -> u8 {
        self.strength / 21 + 1
    }
}

/// The Wi-Fi services in a GetServices reply, nameless (hidden) ones left
/// out, in stock's order: connected first, then saved, then by name.
pub fn parse_services(reply: &str) -> Vec<Service> {
    let mut out: Vec<Service> = Vec::new();
    let mut cur: Option<Service> = None;
    let mut key: Option<String> = None;
    let mut security = false;
    for line in reply.lines() {
        let t = line.trim();
        if let Some(path) = t.strip_prefix("object path \"").and_then(|p| p.strip_suffix('"')) {
            if let Some(s) = cur.take() {
                out.push(s);
            }
            cur = path.contains("/wifi_").then(|| Service { path: path.to_string(), ..Service::default() });
            continue;
        }
        let Some(s) = cur.as_mut() else { continue };
        if security {
            if let Some(v) = t.strip_prefix("string \"").and_then(|v| v.strip_suffix('"')) {
                if s.security.is_empty() || s.security == "none" {
                    s.security = v.to_string();
                }
            } else if t.starts_with(']') {
                security = false;
            }
            continue;
        }
        if let Some(k) = t.strip_prefix("string \"").and_then(|v| v.strip_suffix('"')) {
            key = Some(k.to_string());
            continue;
        }
        if let (Some(k), Some(v)) = (key.take(), t.strip_prefix("variant")) {
            let v = v.trim();
            let text = || v.strip_prefix("string \"").and_then(|v| v.strip_suffix('"')).unwrap_or("").to_string();
            match k.as_str() {
                "Name" => s.name = text(),
                "State" => s.state = text(),
                "Error" => s.error = text(),
                "Strength" => s.strength = v.strip_prefix("byte ").and_then(|n| n.trim().parse().ok()).unwrap_or(0),
                "Favorite" => s.saved = v.ends_with("true"),
                "Security" => security = true,
                _ => {}
            }
        }
    }
    if let Some(s) = cur {
        out.push(s);
    }
    out.retain(|s| !s.name.is_empty());
    out.sort_by(|a, b| b.connected().cmp(&a.connected()).then(b.saved.cmp(&a.saved))
        .then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

/// A dbus-send to connman, logged at trace: it runs every second or two
/// while the Wi-Fi pages are open.
fn dbus(args: &[&str]) -> Result<String, String> {
    let started = Instant::now();
    let result = send(args);
    let ms = started.elapsed().as_millis();
    match &result {
        Ok(reply) => trace!("wifi: dbus {}: {} bytes in {ms} ms", args.join(" "), reply.len()),
        Err(e) => trace!("wifi: dbus {}: {e}, after {ms} ms", args.join(" ")),
    }
    result
}

fn send(args: &[&str]) -> Result<String, String> {
    // The desktop's host has no connman to ask, and asking would block the
    // client's loop until dbus-send gave up: nest-qemu's answers stand in.
    if cfg!(feature = "desktop") {
        #[cfg(feature = "desktop")]
        return crate::platform::desktop::connman::reply(args);
    }
    let out = Command::new("dbus-send").args(["--system", "--print-reply", "--dest=net.connman"]).args(args)
        .output().map_err(|e| format!("dbus-send: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// The services as last read (path, name, state, error), to log what
/// changes between reads.
static SEEN: Mutex<Vec<(String, String, String, String)>> = Mutex::new(Vec::new());

/// connman's Wi-Fi services now.
pub fn services() -> Vec<Service> {
    let list = match dbus(&["/", "net.connman.Manager.GetServices"]) {
        Ok(reply) => parse_services(&reply),
        Err(e) => {
            debug!("wifi: services: {e}");
            Vec::new()
        }
    };
    note(&list);
    list
}

/// Logs the services' count and each one's state as they change.
fn note(list: &[Service]) {
    let now: Vec<_> = list.iter().map(|s| (s.path.clone(), s.name.clone(), s.state.clone(), s.error.clone())).collect();
    let mut seen = SEEN.lock().unwrap();
    if now.len() != seen.len() {
        debug!("wifi: {} networks in sight", now.len());
    }
    for (path, name, state, error) in &now {
        let before = seen.iter().find(|s| s.0 == *path);
        if before.is_none_or(|b| b.2 != *state || b.3 != *error) {
            let was = before.map_or("unseen", |b| b.2.as_str());
            match error.as_str() {
                "" => debug!("wifi: {name}: {was} -> {state}"),
                e => debug!("wifi: {name}: {was} -> {state} ({e})"),
            }
        }
    }
    *seen = now;
}

/// Whether the radio was last seen working: 0 unknown, 1 off, 2 on.
static POWERED: AtomicU8 = AtomicU8::new(0);

/// Whether connman has Wi-Fi powered: the radio works.
pub fn working() -> bool {
    let on = powered();
    let now = if on { 2 } else { 1 };
    if POWERED.swap(now, Ordering::Relaxed) != now {
        info!("wifi: radio {}", if on { "working" } else { "not working" });
    }
    on
}

fn powered() -> bool {
    let reply = match dbus(&["/", "net.connman.Manager.GetTechnologies"]) {
        Ok(reply) => reply,
        Err(e) => {
            debug!("wifi: technologies: {e}");
            return false;
        }
    };
    let mut wifi = false;
    let mut powered = false;
    for line in reply.lines() {
        let t = line.trim();
        if t.starts_with("object path") {
            wifi = t.contains("/technology/wifi");
        } else if wifi && t.starts_with("string \"Powered\"") {
            powered = true;
        } else if wifi && powered && t.starts_with("variant") {
            return t.ends_with("true");
        }
    }
    false
}

/// Asks connman for a scan; the services follow.
pub fn scan() {
    debug!("wifi: scanning");
    std::thread::spawn(|| {
        crate::platform::relax();
        let started = Instant::now();
        match dbus(&["--reply-timeout=30000", "/net/connman/technology/wifi", "net.connman.Technology.Scan"]) {
            Ok(_) => debug!("wifi: scan done in {} ms", started.elapsed().as_millis()),
            Err(e) => debug!("wifi: scan: {e}"),
        }
    });
}

/// connman's provisioning file for a network: its name in hex, whether it
/// hides its name and its security, and its passphrase when it has one.
pub fn provisioning(ssid: &str, passphrase: Option<&str>, hidden: Option<&str>) -> String {
    let hex: String = ssid.bytes().map(|b| format!("{b:02x}")).collect();
    let mut out = format!("[service_{hex}]\nType=wifi\nSSID={hex}\n");
    if let Some(security) = hidden {
        out.push_str(&format!("Hidden=true\nSecurity={security}\n"));
    }
    if let Some(p) = passphrase.filter(|p| !p.is_empty()) {
        out.push_str(&format!("Passphrase={p}\n"));
    }
    out
}

/// Hands a network to connman: its provisioning file, which connman takes
/// up at once.
pub fn provision(ssid: &str, passphrase: Option<&str>, hidden: Option<&str>) -> Result<(), String> {
    let path = Path::new(CONNMAN).join(PROVISION);
    // The passphrase stays out of the log.
    debug!("wifi: provisioning {ssid}{}{}", hidden.map_or(String::new(), |s| format!(", hidden, {s}")),
           if passphrase.is_some_and(|p| !p.is_empty()) { ", passphrase <redacted>" } else { "" });
    std::fs::write(&path, provisioning(ssid, passphrase, hidden)).map_err(|e| format!("{}: {e}", path.display()))
}

/// Connects a service, off the UI's thread (connman answers when done).
pub fn connect(path: &str) {
    let path = path.to_string();
    debug!("wifi: connecting {path}");
    std::thread::spawn(move || {
        crate::platform::relax();
        let started = Instant::now();
        match dbus(&["--reply-timeout=60000", &path, "net.connman.Service.Connect"]) {
            Ok(_) => debug!("wifi: connect {path}: done in {} ms", started.elapsed().as_millis()),
            Err(e) => warn!("wifi: connect {path}: {e}"),
        }
    });
}

/// A service's AutoConnect (stock moves it to the network that works).
pub fn autoconnect(path: &str, on: bool) {
    let value = format!("variant:boolean:{on}");
    debug!("wifi: autoconnect {path} {}", if on { "on" } else { "off" });
    if let Err(e) = dbus(&[path, "net.connman.Service.SetProperty", "string:AutoConnect", &value]) {
        error!("wifi: autoconnect {path}: {e}");
    }
}

/// Leaves a network: AutoConnect off, then Disconnect.
pub fn disconnect(path: &str) {
    autoconnect(path, false);
    debug!("wifi: disconnecting {path}");
    if let Err(e) = dbus(&[path, "net.connman.Service.Disconnect"]) {
        error!("wifi: disconnect {path}: {e}");
    }
}

/// The passphrase connman keeps for a saved network.
pub fn passphrase(service: &str) -> Option<String> {
    let id = service.rsplit('/').next()?;
    std::fs::read_to_string(Path::new(CONNMAN).join(id).join("settings")).ok()?
        .lines().find_map(|l| l.strip_prefix("Passphrase=").map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPLY: &str = r#"method return
   array [
      struct {
         object path "/net/connman/service/wifi_aa_4e6574_managed_psk"
         array [
            dict entry(
               string "Security"
               variant                   array [
                     string "psk"
                  ]
            )
            dict entry(
               string "State"
               variant                   string "failure"
            )
            dict entry(
               string "Error"
               variant                   string "invalid-key"
            )
            dict entry(
               string "Strength"
               variant                   byte 65
            )
            dict entry(
               string "Name"
               variant                   string "nest"
            )
         ]
      }
      struct {
         object path "/net/connman/service/wifi_aa_51_managed_none"
         array [
            dict entry(
               string "State"
               variant                   string "ready"
            )
            dict entry(
               string "Favorite"
               variant                   boolean true
            )
            dict entry(
               string "Name"
               variant                   string "QEMU"
            )
            dict entry(
               string "IPv4"
               variant                   array [
                     dict entry(
                        string "Address"
                        variant                            string "10.0.2.15"
                     )
                  ]
            )
         ]
      }
      struct {
         object path "/net/connman/service/wifi_aa_hidden_managed_psk"
         array [
            dict entry(
               string "Name"
               variant                   string ""
            )
         ]
      }
      struct {
         object path "/net/connman/service/lowpan_wpan0_x"
         array [
         ]
      }
      struct {
         object path "/net/connman/service/wifi_aa_41_managed_none"
         array [
            dict entry(
               string "Strength"
               variant                   byte 20
            )
            dict entry(
               string "Name"
               variant                   string "Attic"
            )
         ]
      }
   ]"#;

    #[test]
    fn services_in_stocks_order_with_their_security_level_and_error() {
        let s = parse_services(REPLY);
        let row = |s: &Service| (s.name.clone(), s.connected(), s.saved, s.secured(), s.level(), s.error.clone());
        assert_eq!(s.iter().map(row).collect::<Vec<_>>(), [
            ("QEMU".into(), true, true, false, 1, String::new()),
            ("Attic".into(), false, false, false, 1, String::new()),
            ("nest".into(), false, false, true, 4, "invalid-key".into()),
        ], "connected, saved, then by name; the hidden one left out");
    }

    #[test]
    fn provisioning_names_the_network_in_hex_with_its_passphrase() {
        assert_eq!(provisioning("Nest-WPA2", Some("nestpassword"), None),
                   "[service_4e6573742d57504132]\nType=wifi\nSSID=4e6573742d57504132\nPassphrase=nestpassword\n");
        assert_eq!(provisioning("a", None, Some("none")), "[service_61]\nType=wifi\nSSID=61\nHidden=true\nSecurity=none\n");
    }
}
