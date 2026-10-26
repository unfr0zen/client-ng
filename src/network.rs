//! Stock nlclient's Wi-Fi pages (nlWiFiScreen: wifilayer, connectlayer,
//! networktypelayer): what choosing a network does, the connecting page
//! following connman's state, its 45-second limit, the online and failed
//! pages, the hidden network's security question and the disconnecting
//! page. connman is driven through the actions this returns.

use crate::wifi::Service;

/// How long connecting may take before stock decides (0x1fe200).
const TIMEOUT: f64 = 45.0;
/// How long the disconnecting page waits for the network to go.
const DISCONNECTING: f64 = 25.0;
/// How long a network just provisioned may take to appear in connman: a
/// hidden one only shows once a scan has probed for its name.
const APPEAR: f64 = 5.0;
const APPEAR_HIDDEN: f64 = 20.0;

/// The hidden network's security choices, and connman's names for them.
pub const SECURITY: [&str; 4] = ["NONE", "WPA", "WPA2", "WEP"];
const SECURITY_KINDS: [&str; 4] = ["none", "psk", "psk", "wep"];

#[derive(Clone, Debug, PartialEq)]
pub enum Page {
    /// Connecting to `name`: its service once known, the phase reached
    /// (1 association, 2 configuration, 3 ready), and whether STOP
    /// CONNECTING is highlighted.
    Connecting { name: String, path: Option<String>, since: f64, phase: u8, stop: bool, hidden: bool },
    Online { name: String },
    /// A wrong passphrase (`key`), else any other failure, at its phase.
    Failed { name: String, key: bool, phase: u8 },
    /// A hidden network's security, the choice highlighted.
    Security { name: String, at: usize },
    Disconnecting { since: f64 },
}

/// What connman is to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Provision { name: String, passphrase: Option<String>, hidden: Option<&'static str> },
    Connect(String),
    Autoconnect(String, bool),
    Disconnect(String),
}

/// What the keyboard is to ask for.
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    /// A passphrase for `name`, the saved one shown masked, and the hidden
    /// network's security when it is one.
    Passphrase { name: String, saved: Option<String>, hidden: Option<&'static str> },
}

#[derive(Debug, Default)]
pub struct Flow {
    pub page: Option<Page>,
}

impl Flow {
    /// A network chosen from the list (0x1f3544): the connected one is
    /// online already; an open or saved one connects; a secured unsaved
    /// one, or one whose passphrase was refused, asks for its passphrase.
    pub fn choose(&mut self, services: &[Service], name: &str, now: f64) -> (Vec<Act>, Option<Ask>) {
        let Some(s) = services.iter().find(|s| s.name == name) else {
            debug!("wifi: {name} chosen but not in sight");
            return (Vec::new(), None);
        };
        if s.connected() {
            debug!("wifi: {name} chosen, on it already");
            self.page = Some(Page::Online { name: name.into() });
            return (Vec::new(), None);
        }
        if s.secured() && (!s.saved || s.error == "invalid-key") {
            debug!("wifi: {name} chosen; asking its passphrase");
            let saved = s.saved.then(|| crate::wifi::passphrase(&s.path)).flatten();
            return (Vec::new(), Some(Ask::Passphrase { name: name.into(), saved, hidden: None }));
        }
        info!("wifi: joining {name}");
        (self.connect(services, s.path.clone(), name, now), None)
    }

    /// Connects a known service, AutoConnect taken off the one in use.
    fn connect(&mut self, services: &[Service], path: String, name: &str, now: f64) -> Vec<Act> {
        let mut acts: Vec<Act> = services.iter().filter(|s| s.connected() && s.path != path).map(|s| Act::Autoconnect(s.path.clone(), false)).collect();
        acts.push(Act::Connect(path.clone()));
        self.page = Some(Page::Connecting { name: name.into(), path: Some(path), since: now, phase: 1, stop: false, hidden: false });
        acts
    }

    /// A passphrase typed (empty is taken, as stock's): the network is
    /// provisioned with it and connected once connman has its service.
    pub fn passphrase(&mut self, name: &str, passphrase: &str, hidden: Option<&'static str>, now: f64) -> Vec<Act> {
        debug!("wifi: passphrase for {name} typed ({})", if passphrase.is_empty() { "empty" } else { "<redacted>" });
        self.page = Some(Page::Connecting { name: name.into(), path: None, since: now, phase: 1, stop: false, hidden: hidden.is_some() });
        vec![Act::Provision { name: name.into(), passphrase: Some(passphrase.into()), hidden }]
    }

    /// A hidden network's name typed: its security is asked.
    pub fn hidden(&mut self, name: &str) {
        debug!("wifi: hidden network {name}; asking its security");
        self.page = Some(Page::Security { name: name.into(), at: 0 });
    }

    /// The security chosen: NONE connects, the rest ask a passphrase.
    fn secured(&mut self, name: String, at: usize, now: f64) -> (Vec<Act>, Option<Ask>) {
        let kind = SECURITY_KINDS[at];
        debug!("wifi: {name} secured with {}", SECURITY[at]);
        if kind == "none" {
            self.page = Some(Page::Connecting { name: name.clone(), path: None, since: now, phase: 1, stop: false, hidden: true });
            return (vec![Act::Provision { name, passphrase: None, hidden: Some(kind) }], None);
        }
        self.page = None;
        (Vec::new(), Some(Ask::Passphrase { name, saved: None, hidden: Some(kind) }))
    }

    /// DISCONNECT or DO NOT CONNECT: the network in use is left.
    pub fn disconnect(&mut self, services: &[Service], now: f64) -> Vec<Act> {
        let Some(s) = services.iter().find(|s| s.connected()) else { return Vec::new() };
        info!("wifi: leaving {}", s.name);
        self.page = Some(Page::Disconnecting { since: now });
        vec![Act::Disconnect(s.path.clone())]
    }

    /// Follows connman: the phase, online, a failure, the time limit.
    pub fn tick(&mut self, services: &[Service], now: f64) -> Vec<Act> {
        match self.page.clone() {
            Some(Page::Connecting { name, path, since, phase, stop, hidden }) => {
                let found = match &path {
                    Some(p) => services.iter().find(|s| s.path == *p),
                    None => services.iter().find(|s| s.name == name),
                };
                let Some(s) = found else {
                    // A provisioned network connman never shows fails at once.
                    if path.is_none() && now - since >= if hidden { APPEAR_HIDDEN } else { APPEAR } {
                        error!("wifi: {name} never appeared in connman after {:.0} s", now - since);
                        self.page = Some(Page::Failed { name, key: false, phase });
                    }
                    return Vec::new();
                };
                if path.is_none() {
                    debug!("wifi: {name} provisioned as {}", s.path);
                    return self.connect(services, s.path.clone(), &name, since);
                }
                let reached = match s.state.as_str() {
                    "association" => 1,
                    "configuration" => 2,
                    "ready" => 3,
                    _ => phase,
                }.max(phase);
                if reached != phase {
                    debug!("wifi: joining {name}: phase {reached} ({})", s.state);
                }
                if s.state == "online" {
                    info!("wifi: joined {name}");
                    self.page = Some(Page::Online { name });
                    return vec![Act::Autoconnect(s.path.clone(), true)];
                }
                if s.state == "failure" {
                    error!("wifi: joining {name} failed at phase {reached}: {}", if s.error.is_empty() { "no reason given" } else { &s.error });
                    self.page = Some(Page::Failed { name, key: s.error == "invalid-key", phase: reached });
                    return Vec::new();
                }
                if now - since >= TIMEOUT {
                    // Ready but no internet counts as online (0x2026d4).
                    if s.connected() {
                        info!("wifi: joined {name}, {} after {TIMEOUT:.0} s, taken as online", s.state);
                        self.page = Some(Page::Online { name });
                        return vec![Act::Autoconnect(s.path.clone(), true)];
                    }
                    error!("wifi: joining {name} timed out after {TIMEOUT:.0} s at phase {reached} ({})", s.state);
                    self.page = Some(Page::Failed { name, key: false, phase: reached });
                    return vec![Act::Disconnect(s.path.clone())];
                }
                self.page = Some(Page::Connecting { name, path, since, phase: reached, stop, hidden });
                Vec::new()
            }
            Some(Page::Disconnecting { since }) => {
                if !services.iter().any(Service::connected) || now - since >= DISCONNECTING {
                    match services.iter().find(|s| s.connected()) {
                        Some(s) => warn!("wifi: {} still {} after {DISCONNECTING:.0} s", s.name, s.state),
                        None => info!("wifi: disconnected"),
                    }
                    self.page = None;
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    pub fn wheel(&mut self, detents: i32) {
        match &mut self.page {
            Some(Page::Connecting { stop, .. }) => *stop = detents > 0 || (*stop && detents == 0),
            Some(Page::Security { at, .. }) => *at = (*at as i32 + detents).clamp(0, SECURITY.len() as i32 - 1) as usize,
            _ => {}
        }
    }

    /// A press: STOP CONNECTING when highlighted, the security chosen, or
    /// a result page dismissed. `true` when the online page goes, which
    /// stock follows back to the settings tile.
    pub fn press(&mut self, now: f64) -> (Vec<Act>, Option<Ask>, bool) {
        match self.page.take() {
            Some(Page::Connecting { name, path, stop: true, .. }) => {
                info!("wifi: joining {name} stopped");
                (path.map(Act::Disconnect).into_iter().collect(), None, false)
            }
            Some(p @ Page::Connecting { .. }) | Some(p @ Page::Disconnecting { .. }) => {
                self.page = Some(p);
                (Vec::new(), None, false)
            }
            Some(Page::Security { name, at }) => {
                let (acts, ask) = self.secured(name, at, now);
                (acts, ask, false)
            }
            Some(Page::Online { .. }) => (Vec::new(), None, true),
            Some(Page::Failed { .. }) | None => (Vec::new(), None, false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc(name: &str, state: &str, security: &str, saved: bool, error: &str) -> Service {
        Service { path: format!("/s/{name}"), name: name.into(), strength: 60, security: security.into(), saved, state: state.into(), error: error.into() }
    }

    #[test]
    fn choosing_follows_stocks_rules() {
        let list = [svc("Home", "ready", "none", true, ""), svc("Open", "idle", "none", false, ""), svc("Lock", "idle", "psk", false, ""),
                    svc("Saved", "idle", "psk", true, ""), svc("Refused", "failure", "psk", true, "invalid-key")];
        let mut f = Flow::default();
        assert_eq!(f.choose(&list, "Home", 0.0), (vec![], None));
        assert_eq!(f.page, Some(Page::Online { name: "Home".into() }), "the connected one is online already");
        let (acts, ask) = f.choose(&list, "Open", 0.0);
        assert_eq!((acts, ask), (vec![Act::Autoconnect("/s/Home".into(), false), Act::Connect("/s/Open".into())], None));
        assert!(matches!(f.choose(&list, "Lock", 0.0).1, Some(Ask::Passphrase { .. })));
        assert_eq!(f.choose(&list, "Saved", 0.0).0.last(), Some(&Act::Connect("/s/Saved".into())), "saved: no passphrase asked");
        assert!(matches!(f.choose(&list, "Refused", 0.0).1, Some(Ask::Passphrase { .. })), "a refused passphrase is asked again");
    }

    #[test]
    fn connecting_follows_connman_to_online_or_failure() {
        let mut f = Flow::default();
        assert_eq!(f.passphrase("Nest", "pw", None, 0.0), vec![Act::Provision { name: "Nest".into(), passphrase: Some("pw".into()), hidden: None }]);
        assert_eq!(f.tick(&[svc("Nest", "idle", "psk", false, "")], 1.0), vec![Act::Connect("/s/Nest".into())]);
        f.tick(&[svc("Nest", "configuration", "psk", false, "")], 2.0);
        assert!(matches!(f.page, Some(Page::Connecting { phase: 2, .. })));
        assert_eq!(f.tick(&[svc("Nest", "online", "psk", true, "")], 3.0), vec![Act::Autoconnect("/s/Nest".into(), true)]);
        assert_eq!(f.page, Some(Page::Online { name: "Nest".into() }));
        assert!(f.press(4.0).2, "to the tile");
        f.passphrase("Nest", "bad", None, 10.0);
        f.tick(&[svc("Nest", "association", "psk", false, "")], 11.0);
        f.tick(&[svc("Nest", "failure", "psk", false, "invalid-key")], 12.0);
        assert_eq!(f.page, Some(Page::Failed { name: "Nest".into(), key: true, phase: 1 }));
    }

    #[test]
    fn the_time_limit_takes_ready_as_online_and_otherwise_fails() {
        let mut f = Flow::default();
        f.choose(&[svc("Open", "idle", "none", false, "")], "Open", 0.0);
        f.tick(&[svc("Open", "ready", "none", false, "")], 10.0);
        assert!(matches!(f.page, Some(Page::Connecting { phase: 3, .. })), "connecting to the internet");
        f.tick(&[svc("Open", "ready", "none", false, "")], 46.0);
        assert_eq!(f.page, Some(Page::Online { name: "Open".into() }));
        f.choose(&[svc("Far", "idle", "none", false, "")], "Far", 100.0);
        assert_eq!(f.tick(&[svc("Far", "association", "none", false, "")], 146.0), vec![Act::Disconnect("/s/Far".into())]);
        assert_eq!(f.page, Some(Page::Failed { name: "Far".into(), key: false, phase: 1 }));
    }

    #[test]
    fn a_hidden_network_asks_its_security_and_fails_when_never_seen() {
        let mut f = Flow::default();
        f.hidden("Attic");
        f.wheel(2);
        let (acts, ask, _) = f.press(0.0);
        assert_eq!((acts, ask), (vec![], Some(Ask::Passphrase { name: "Attic".into(), saved: None, hidden: Some("psk") })), "WPA2");
        f.hidden("Shed");
        let (acts, _, _) = f.press(0.0);
        assert_eq!(acts, vec![Act::Provision { name: "Shed".into(), passphrase: None, hidden: Some("none") }]);
        f.tick(&[], 6.0);
        assert!(matches!(f.page, Some(Page::Connecting { .. })), "a hidden one has longer to show");
        f.tick(&[], 21.0);
        assert_eq!(f.page, Some(Page::Failed { name: "Shed".into(), key: false, phase: 1 }));
    }

    #[test]
    fn stop_connecting_and_disconnecting() {
        let mut f = Flow::default();
        f.choose(&[svc("Open", "idle", "none", false, "")], "Open", 0.0);
        f.wheel(1);
        assert_eq!(f.press(1.0).0, vec![Act::Disconnect("/s/Open".into())]);
        assert_eq!(f.page, None);
        assert_eq!(f.disconnect(&[svc("Home", "online", "none", true, "")], 0.0), vec![Act::Disconnect("/s/Home".into())]);
        f.tick(&[svc("Home", "idle", "none", true, "")], 1.0);
        assert_eq!(f.page, None, "gone once the network is");
    }
}
