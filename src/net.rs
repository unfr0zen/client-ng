//! The link to nestlocal, the unit's stand-in for Nest's service, over the
//! cloud protocol nlclient spoke (crates/nestlocal/src/cloud.rs):
//!   POST /entry                   where the transport is
//!   POST <transport>/v7/subscribe  held until nestlocal changes a bucket
//!   POST <transport>/v7/put        the UI's changes
//! Only shared.<serial> and device.<serial> are followed, for the target,
//! the mode, the scale and the room temperature. nestlocal keeps the truth:
//! the client starts from its copy and sends only what the UI changed.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::os::unix::io::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use serde_json::{json, Map, Value as Json};

use crate::engine::{Engine, ECO_COOL, ECO_HEAT, SAFETY_HIGH, SAFETY_LOW};
use crate::model::{Value, MODE, UNITS};

pub type Fields = Map<String, Json>;

/// Longer than nestlocal holds an idle subscribe (290 s by default).
const HOLD: Duration = Duration::from_secs(330);
const REQUEST: Duration = Duration::from_secs(10);
/// How long the UI has to be still before a change is sent: a turn of the
/// wheel goes out once, at its end.
const SETTLE: Duration = Duration::from_millis(800);

#[derive(Clone, Debug, PartialEq)]
struct Url {
    host: String,
    port: u16,
    path: String,
}

fn parse_url(url: &str) -> Result<Url, String> {
    let rest = url.strip_prefix("https://").ok_or_else(|| format!("{url}: not an https URL"))?;
    let (authority, path) = rest.find('/').map_or((rest, "/"), |i| (&rest[..i], &rest[i..]));
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().map_err(|_| format!("{url}: bad port"))?),
        None => (authority, 443),
    };
    Ok(Url { host: host.to_string(), port, path: path.to_string() })
}

fn text(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// POSTs JSON over TLS and returns the response's body.
fn post(tls: &Arc<rustls::ClientConfig>, url: &Url, path: &str, body: &[u8], timeout: Duration) -> Result<Vec<u8>, String> {
    request(tls, url, "POST", path, Some(body), timeout)
}

/// A request and its reply's body, each logged: the status, the sizes and
/// the time taken, and the bodies themselves at trace.
fn request(tls: &Arc<rustls::ClientConfig>, url: &Url, method: &str, path: &str, body: Option<&[u8]>, timeout: Duration) -> Result<Vec<u8>, String> {
    let (started, host) = (Instant::now(), format!("{}:{}", url.host, url.port));
    if let Some(b) = body {
        trace!("cloud: {method} {host}{path} sends {}", clip(b));
    }
    let result = exchange(tls, url, method, path, body, timeout);
    let ms = started.elapsed().as_millis();
    match &result {
        Ok((status, reply)) => {
            debug!("cloud: {method} {host}{path}: {status}, {} bytes sent, {} back, in {ms} ms", body.map_or(0, <[u8]>::len), reply.len());
            trace!("cloud: {method} {host}{path} answers {}", clip(reply));
        }
        Err(e) => debug!("cloud: {method} {host}{path}: {e}, after {ms} ms"),
    }
    result.map(|(_, reply)| reply)
}

/// At most 512 characters of a body, for the log.
fn clip(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    match text.char_indices().nth(512) {
        Some((at, _)) => format!("{}... ({} bytes)", &text[..at], body.len()),
        None => text.into_owned(),
    }
}

fn exchange(tls: &Arc<rustls::ClientConfig>, url: &Url, method: &str, path: &str, body: Option<&[u8]>, timeout: Duration) -> Result<(u16, Vec<u8>), String> {
    let addr = (url.host.as_str(), url.port).to_socket_addrs().map_err(text)?.next()
        .ok_or_else(|| format!("{}: no address", url.host))?;
    let tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(|e| format!("{addr}: {e}"))?;
    tcp.set_read_timeout(Some(timeout)).map_err(text)?;
    tcp.set_write_timeout(Some(REQUEST)).map_err(text)?;
    let name = ServerName::try_from(url.host.clone()).map_err(text)?;
    let conn = rustls::ClientConnection::new(tls.clone(), name).map_err(text)?;
    let mut stream = rustls::StreamOwned::new(conn, tcp);
    match body {
        Some(body) => {
            write!(stream, "{method} {path} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                   url.host, body.len()).map_err(text)?;
            stream.write_all(body).map_err(text)?;
        }
        None => write!(stream, "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", url.host).map_err(text)?,
    }
    stream.flush().map_err(text)?;
    read_response(BufReader::new(stream))
}

/// An HTTP/1.1 response's status and body, sized or chunked.
fn read_response(mut r: impl BufRead) -> Result<(u16, Vec<u8>), String> {
    let mut line = String::new();
    r.read_line(&mut line).map_err(text)?;
    let status: u16 = line.split_whitespace().nth(1).and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("bad status line {:?}", line.trim_end()))?;
    let (mut length, mut chunked) = (None, false);
    loop {
        line.clear();
        if r.read_line(&mut line).map_err(text)? == 0 {
            return Err("headers cut short".into());
        }
        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
            if name == "content-length" {
                length = value.parse::<usize>().ok();
            } else if name == "transfer-encoding" && value.eq_ignore_ascii_case("chunked") {
                chunked = true;
            }
        }
    }
    let mut body = Vec::new();
    if chunked {
        loop {
            line.clear();
            r.read_line(&mut line).map_err(text)?;
            let size = usize::from_str_radix(line.trim().split(';').next().unwrap_or(""), 16)
                .map_err(|_| format!("bad chunk size {:?}", line.trim()))?;
            if size == 0 {
                break;
            }
            let start = body.len();
            body.resize(start + size, 0);
            r.read_exact(&mut body[start..]).map_err(text)?;
            line.clear();
            r.read_line(&mut line).map_err(text)?;
        }
    } else if let Some(n) = length {
        body.resize(n, 0);
        r.read_exact(&mut body).map_err(text)?;
    } else {
        // Closed without close_notify is how a server without a length ends.
        let _ = r.read_to_end(&mut body);
    }
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}: {}", String::from_utf8_lossy(&body).trim()));
    }
    Ok((status, body))
}

/// A bucket as the client last saw it.
#[derive(Clone, Debug, Default)]
struct Bucket {
    revision: i64,
    timestamp: i64,
    value: Option<Fields>,
}

#[derive(Default)]
struct Buckets {
    by_key: BTreeMap<String, Bucket>,
    /// Buckets nestlocal asked for (its copy is older): sent whole next time.
    asked: BTreeSet<String>,
}

impl Buckets {
    fn subscribe_body(&self) -> Vec<u8> {
        let objects: Vec<Json> = self.by_key.iter().map(|(key, b)| {
            let mut o = json!({"object_key": key, "object_revision": b.revision, "object_timestamp": b.timestamp});
            if let (true, Some(v)) = (self.asked.contains(key), &b.value) {
                o["value"] = Json::Object(v.clone());
            }
            o
        }).collect();
        json!({"chunked": true, "objects": objects}).to_string().into_bytes()
    }

    /// Takes an answer's objects; returns the buckets whose value changed.
    fn take(&mut self, body: &[u8]) -> Result<Vec<(String, Fields)>, String> {
        let body: Json = serde_json::from_slice(body).map_err(|e| format!("bad JSON: {e}"))?;
        let mut changed = Vec::new();
        for o in body["objects"].as_array().into_iter().flatten() {
            let Some(key) = o["object_key"].as_str() else { continue };
            let Some(b) = self.by_key.get_mut(key) else { continue };
            let (rev, ts) = (o["object_revision"].as_i64().unwrap_or(0), o["object_timestamp"].as_i64().unwrap_or(0));
            match o.get("value").and_then(Json::as_object) {
                Some(v) => {
                    *b = Bucket { revision: rev, timestamp: ts, value: Some(v.clone()) };
                    self.asked.remove(key);
                    changed.push((key.to_string(), v.clone()));
                }
                // Without a value: nestlocal's copy is older, and it wants ours.
                None if b.value.is_some() && ts < b.timestamp => {
                    self.asked.insert(key.to_string());
                }
                None => {}
            }
        }
        Ok(changed)
    }

    /// A put's answer: the new revision and timestamp of what was sent.
    /// A bucket not yet received whole stays unknown, so the subscribe
    /// still asks for it.
    fn sent(&mut self, key: &str, fields: &Fields, body: &[u8]) {
        let Some(b) = self.by_key.get_mut(key) else { return };
        let Some(value) = &mut b.value else { return };
        value.extend(fields.clone());
        let answer: Json = serde_json::from_slice(body).unwrap_or_default();
        if let Some(o) = answer["objects"].as_array().into_iter().flatten().find(|o| o["object_key"] == key) {
            b.revision = o["object_revision"].as_i64().unwrap_or(b.revision);
            b.timestamp = o["object_timestamp"].as_i64().unwrap_or(b.timestamp);
        }
    }
}

/// Logs a link's state only when it changes.
struct Status(String);

impl Status {
    fn report(&mut self, level: crate::log::Level, now: String) {
        if now != self.0 {
            crate::log::write(level, format_args!("cloud: {now}"));
            self.0 = now;
        }
    }
}

/// The running link: a thread holding a subscribe, a thread sending puts.
pub struct Link {
    updates: mpsc::Receiver<(String, Fields)>,
    puts: mpsc::Sender<(String, Fields)>,
    /// Readable when updates are waiting, so the UI's poll wakes.
    wake: UnixStream,
    /// Where to ask the weather for: "<postal code>,<country>".
    place: Arc<Mutex<Option<String>>>,
}

impl Link {
    /// Starts following serial's buckets at the service `entry` names,
    /// trusting the CAs in `ca`.
    pub fn start(entry: &str, ca: &Path, serial: &str) -> Result<Link, String> {
        let entry = parse_url(entry)?;
        let mut roots = rustls::RootCertStore::empty();
        for cert in CertificateDer::pem_file_iter(ca).map_err(|e| format!("{}: {e}", ca.display()))? {
            roots.add(cert.map_err(|e| format!("{}: {e}", ca.display()))?).map_err(|e| format!("{}: {e}", ca.display()))?;
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let tls = Arc::new(rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions().map_err(text)?
            .with_root_certificates(roots)
            .with_no_client_auth());

        let mut buckets = Buckets::default();
        // The link bucket names the structure's, which is followed once known.
        for key in [format!("shared.{serial}"), format!("device.{serial}"), format!("link.{serial}"), format!("schedule.{serial}")] {
            buckets.by_key.insert(key, Bucket::default());
        }
        let buckets = Arc::new(Mutex::new(buckets));
        let (wake_tx, wake) = UnixStream::pair().map_err(text)?;
        wake.set_nonblocking(true).map_err(text)?;
        let (updates_tx, updates) = mpsc::channel();
        let (puts, puts_rx) = mpsc::channel::<(String, Fields)>();
        let transport = Arc::new(Mutex::new(None::<Url>));
        let weather_url = Arc::new(Mutex::new(None::<String>));
        let place = Arc::new(Mutex::new(None::<String>));

        let (t, b, w, tls_sub, updates_sub, wake_sub) =
            (transport.clone(), buckets.clone(), weather_url.clone(), tls.clone(), updates_tx.clone(), wake_tx.try_clone().map_err(text)?);
        std::thread::Builder::new().name("cloud-subscribe".into()).spawn(move || {
            subscribe(&tls_sub, &entry, &t, &w, &b, &updates_sub, wake_sub)
        }).map_err(text)?;
        let (tls_weather, place_weather) = (tls.clone(), place.clone());
        std::thread::Builder::new().name("cloud-weather".into()).spawn(move || {
            weather(&tls_weather, &weather_url, &place_weather, &updates_tx, wake_tx)
        }).map_err(text)?;
        std::thread::Builder::new().name("cloud-put".into()).spawn(move || {
            for (key, fields) in puts_rx {
                put(&tls, &transport, &buckets, &key, &fields);
            }
        }).map_err(text)?;
        Ok(Link { updates, puts, wake, place })
    }

    pub fn fd(&self) -> RawFd {
        self.wake.as_raw_fd()
    }

    /// Buckets nestlocal changed since the last call.
    pub fn updates(&mut self) -> Vec<(String, Fields)> {
        let mut drain = [0u8; 64];
        while matches!(self.wake.read(&mut drain), Ok(n) if n > 0) {}
        self.updates.try_iter().collect()
    }

    pub fn put(&self, key: String, fields: Fields) {
        let _ = self.puts.send((key, fields));
    }

    /// Asks the weather for `place` ("<postal code>,<country>") from now
    /// on, at once when it changes; None, where the unit's address is.
    pub fn locate(&self, place: Option<String>) {
        let mut at = self.place.lock().unwrap();
        if *at != place {
            debug!("cloud: the weather now for {}", place.as_deref().unwrap_or("where the unit is"));
        }
        *at = place;
    }
}

fn subscribe(tls: &Arc<rustls::ClientConfig>, entry: &Url, transport: &Mutex<Option<Url>>, weather_url: &Mutex<Option<String>>,
             buckets: &Mutex<Buckets>, updates: &mpsc::Sender<(String, Fields)>, mut wake: UnixStream) {
    let mut status = Status(String::new());
    loop {
        let started = Instant::now();
        let current = transport.lock().unwrap().clone();
        let subscribed = current.is_some();
        let result = match current {
            None => {
                debug!("cloud: asking {}:{}{} for the transport", entry.host, entry.port, entry.path);
                post(tls, entry, &entry.path, b"{}", REQUEST).and_then(|body| {
                let answer: Json = serde_json::from_slice(&body).map_err(|e| format!("/entry: {e}"))?;
                let url = answer["transport_url"].as_str().ok_or("/entry has no transport_url")?;
                let url = parse_url(url)?;
                if let Some(w) = answer["weather_url"].as_str() {
                    debug!("cloud: weather at {w}");
                    *weather_url.lock().unwrap() = Some(w.to_string());
                }
                status.report(crate::log::Level::Info, format!("following nestlocal at {}:{}{}", url.host, url.port, url.path));
                *transport.lock().unwrap() = Some(url);
                Ok(())
                })
            }
            Some(url) => {
                let body = {
                    let b = buckets.lock().unwrap();
                    debug!("cloud: subscribing to {} buckets, {} sent whole", b.by_key.len(), b.asked.len());
                    b.subscribe_body()
                };
                let path = format!("{}/v7/subscribe", url.path.trim_end_matches('/'));
                post(tls, &url, &path, &body, HOLD).and_then(|answer| {
                    let changed = buckets.lock().unwrap().take(&answer)?;
                    for (key, fields) in &changed {
                        debug!("cloud: {key} changed, {} fields", fields.len());
                        if let (true, Some(structure)) = (key.starts_with("link."), fields.get("structure").and_then(Json::as_str)) {
                            if let std::collections::btree_map::Entry::Vacant(e) = buckets.lock().unwrap().by_key.entry(structure.to_string()) {
                                debug!("cloud: following {structure} too");
                                e.insert(Bucket::default());
                            }
                        }
                    }
                    if !changed.is_empty() {
                        for update in changed {
                            let _ = updates.send(update);
                        }
                        let _ = wake.write(&[1]);
                    }
                    Ok(())
                })
            }
        };
        if let Err(e) = result {
            status.report(crate::log::Level::Warn, format!("{e}; retrying"));
            debug!("cloud: asking for the transport again in 5 s");
            *transport.lock().unwrap() = None;
            std::thread::sleep(Duration::from_secs(5));
        } else if started.elapsed() < Duration::from_secs(1) {
            if subscribed {
                debug!("cloud: answered at once; subscribing again in 1 s");
            }
            // Answered at once: nestlocal had something to say, or asks
            // for a bucket it lacks. Either way, not a reason to spin.
            std::thread::sleep(Duration::from_secs(1));
        }
    }
}

/// How often stock asks for the weather (its weatherpollinterval).
const WEATHER_POLL: Duration = Duration::from_secs(3900);
/// How soon to ask again when nestlocal had no weather yet.
const WEATHER_RETRY: Duration = Duration::from_secs(300);
/// Flags of the device bucket only the unit writes, besides `has_*`.
const UNIT_FLAGS: [&str; 5] = ["preconditioning_active", "preconditioning_ready", "heatpump_ready", "sunlight_correction_ready", "sunlight_correction_active"];
/// The update key the weather comes under, beside the buckets'.
pub const WEATHER: &str = "weather";

/// Asks nestlocal's weather_url for the weather at the location set
/// ("<postal code>,<country>"), or else where the unit is ("ipv4", as
/// stock's geo lookup asks), at start, every WEATHER_POLL and when the
/// location changes, and hands its `current` on as a WEATHER update, with
/// the `city`, `state` and `zip` its `location` names and its forecast's
/// `hourly` list.
fn weather(tls: &Arc<rustls::ClientConfig>, weather_url: &Mutex<Option<String>>, place: &Mutex<Option<String>>,
           updates: &mpsc::Sender<(String, Fields)>, mut wake: UnixStream) {
    loop {
        let Some(base) = weather_url.lock().unwrap().clone() else {
            std::thread::sleep(Duration::from_secs(2));
            continue;
        };
        let asked = place.lock().unwrap().clone();
        let query = asked.clone().unwrap_or_else(|| "ipv4".into());
        debug!("cloud: asking the weather for {query}");
        let current = parse_url(&format!("{base}{query}")).and_then(|url| {
            let body = request(tls, &url, "GET", &url.path, None, REQUEST)?;
            let answer: Json = serde_json::from_slice(&body).map_err(|e| format!("weather: {e}"))?;
            let Some(found) = answer.as_object().and_then(|o| o.values().next()) else { return Ok(None) };
            Ok(found["current"].as_object().cloned().map(|mut current| {
                for key in ["city", "state", "zip"] {
                    if let Some(v) = found["location"].get(key) {
                        current.insert(key.into(), v.clone());
                    }
                }
                if let Some(hourly) = found["forecast"].get("hourly") {
                    current.insert("hourly".into(), hourly.clone());
                }
                current
            }))
        });
        let wait = match current {
            Ok(Some(current)) => {
                debug!("cloud: weather for {query}: {} fields; asking again in {} s", current.len(), WEATHER_POLL.as_secs());
                let _ = updates.send((WEATHER.to_string(), current));
                let _ = wake.write(&[1]);
                WEATHER_POLL
            }
            Ok(None) => {
                debug!("cloud: no weather for {query} yet; asking again in {} s", WEATHER_RETRY.as_secs());
                WEATHER_RETRY
            }
            Err(e) => {
                warn!("cloud: weather: {e}; asking again in {} s", WEATHER_RETRY.as_secs());
                WEATHER_RETRY
            }
        };
        let until = Instant::now() + wait;
        while Instant::now() < until && *place.lock().unwrap() == asked {
            std::thread::sleep(Duration::from_secs(2));
        }
    }
}

fn put(tls: &Arc<rustls::ClientConfig>, transport: &Mutex<Option<Url>>, buckets: &Mutex<Buckets>, key: &str, fields: &Fields) {
    let body = json!({"objects": [{"object_key": key, "value": fields}]}).to_string();
    for attempt in 1..=3 {
        let Some(url) = transport.lock().unwrap().clone() else {
            if attempt == 1 {
                debug!("cloud: {key} waits for the transport");
            }
            std::thread::sleep(Duration::from_secs(2));
            if attempt == 3 {
                error!("cloud: {key} not sent: no transport");
            }
            continue;
        };
        let path = format!("{}/v7/put", url.path.trim_end_matches('/'));
        match post(tls, &url, &path, body.as_bytes(), REQUEST) {
            Ok(answer) => {
                buckets.lock().unwrap().sent(key, fields, &answer);
                debug!("cloud: sent {key} {}", Json::Object(fields.clone()));
                return;
            }
            Err(e) if attempt == 3 => error!("cloud: {key} not sent: {e}"),
            Err(e) => {
                warn!("cloud: {key} not sent ({attempt}/3): {e}; again in 2 s");
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

/// nestlocal's target_temperature_type for the menus' mode, and back.
fn mode_to_type(mode: &str) -> &str {
    if mode == "heat-cool" { "range" } else { mode }
}

fn type_to_mode(t: &str) -> &str {
    if t == "range" { "heat-cool" } else { t }
}

fn differs(a: f32, b: f32) -> bool {
    (a - b).abs() > 0.01
}

/// Between the engine and the buckets: what both last agreed on, so a
/// change from either side is told apart from an echo.
pub struct Sync {
    shared: String,
    device: String,
    schedule_key: String,
    /// The week's setpoints as last agreed with the schedule bucket.
    schedule: Option<crate::schedule::Schedule>,
    target: Option<f32>,
    low: Option<f32>,
    high: Option<f32>,
    /// The room's temperature last sent, when the backplate measures it.
    room: Option<f32>,
    /// The device's label as agreed with the shared bucket.
    label: Option<String>,
    /// auto_away and its learning as last sent.
    away: Option<(bool, bool)>,
    /// The heater, air conditioner and fan states last sent.
    switches: Option<[bool; 14]>,
    fan_cooling: Option<bool>,
    /// The filter reminder's level, due, run time and change date as sent.
    filter: Option<(u8, bool, u64, i64)>,
    /// fan_current_speed as last sent.
    fan_speed: Option<&'static str>,
    /// Eco on, its temperatures (each when used), and the fan run's end,
    /// as last agreed with the device bucket.
    eco: Option<bool>,
    eco_temps: Option<(Option<f32>, Option<f32>)>,
    safety: Option<(Option<f32>, Option<f32>)>,
    fan_run: Option<f64>,
    mode: Option<String>,
    units: Option<String>,
    click: Option<String>,
    remind: Option<String>,
    farsight: Option<String>,
    /// The on/off settings, as SWITCHES lists them, the numbers NUMBERS
    /// lists, and whether the fan schedule is on.
    toggles: [Option<String>; SWITCHES.len()],
    numbers: [Option<f64>; NUMBERS.len()],
    fan_schedule: Option<String>,
    /// The room the thermostat is in (Placement).
    place: Option<String>,
    /// `dehumidifier_type` as last seen.
    dehumidifier_type: Option<String>,
    /// The unit-written flags (the equipment's `has_*`, Early-On's and
    /// the heat pump's) as the bucket has them, once seen.
    has: Option<std::collections::BTreeMap<String, bool>>,
    /// `time_to_target` as last sent.
    time_to_target: Option<i64>,
    /// `heat_link_connection` and `fan_cooling_readiness` as last sent.
    heat_link: Option<i64>,
    airwave_ready: Option<bool>,
    /// `is_furnace_shutdown` as last sent.
    furnace_shutdown: Option<bool>,
    /// The structure bucket's key, once the link bucket named it, and
    /// its home facts as HOME lists them.
    structure: Option<String>,
    home: [Option<String>; HOME.len()],
    /// The device bucket's equipment texts as EQUIPMENT lists them.
    equipment: [Option<String>; EQUIPMENT.len()],
    /// Its fan speeds as FAN_SPEEDS lists them.
    fan_speeds: [Option<String>; FAN_SPEEDS.len()],
    /// The numbers OPTIONAL lists, None for off, once seen.
    optional: [Option<Option<f64>>; OPTIONAL.len()],
}

impl Sync {
    pub fn new(serial: &str) -> Sync {
        Sync { shared: format!("shared.{serial}"), device: format!("device.{serial}"), schedule_key: format!("schedule.{serial}"), schedule: None, target: None, low: None, high: None, room: None, away: None, label: None, switches: None, fan_cooling: None, fan_speed: None, filter: None,
               eco: None, eco_temps: None, safety: None, fan_run: None, mode: None, units: None, click: None, remind: None, farsight: None,
               toggles: Default::default(), numbers: Default::default(), fan_schedule: None, place: None, heat_link: None, airwave_ready: None, furnace_shutdown: None, time_to_target: None, has: None, dehumidifier_type: None,
               structure: None, home: Default::default(), equipment: Default::default(), fan_speeds: Default::default(), optional: Default::default() }
    }

    /// Applies a bucket nestlocal sent; true when the screen changes. A
    /// value the UI changed and has not sent yet stays, and is sent.
    pub fn pull(&mut self, engine: &mut Engine, key: &str, value: &Fields) -> bool {
        let mut changed = false;
        if key == WEATHER {
            let num = |k: &str| value.get(k).and_then(Json::as_f64);
            // Stock's parser takes the weather only with both (0x39d6c0).
            if num("temp_c").is_none() || num("humidity").is_none() {
                warn!("weather parser: Error - did not find Temp and Humidity");
                return false;
            }
            let text = |k: &str| value.get(k).and_then(Json::as_str);
            let placed = engine.set_place(text("city"), text("state"), text("zip"));
            // Each hour in its place by its `hour` (1 to 48), as nestlocal numbers them.
            let mut hours = vec![None; 48];
            for (i, h) in value.get("hourly").and_then(Json::as_array).into_iter().flatten().enumerate() {
                let at = h["hour"].as_u64().map_or(i, |n| (n as usize).wrapping_sub(1));
                if let (Some(slot), Some(time), Some(temp)) = (hours.get_mut(at), h["time"].as_i64(), h["temp_c"].as_f64()) {
                    *slot = Some((time, temp as f32));
                }
            }
            let forecast = engine.set_forecast(hours);
            return engine.set_weather(num("temp_c").map(|t| t as f32), text("icon"), num("sunrise"), num("sunset")) | placed | forecast;
        }
        if key.starts_with("link.") {
            if let Some(structure) = value.get("structure").and_then(Json::as_str) {
                if self.structure.as_deref() != Some(structure) {
                    debug!("cloud: the home's facts are in {structure}");
                }
                self.structure = Some(structure.to_string());
            }
        }
        if self.structure.as_deref() == Some(key) {
            for ((setting, field), agreed) in HOME.iter().zip(&mut self.home) {
                if let Some(v) = value.get(*field).and_then(Json::as_str) {
                    changed |= pull_text(agreed, engine, setting, v);
                }
            }
        }
        if key == self.schedule_key {
            let kind = value.get("schedule_mode").and_then(Json::as_str).unwrap_or(engine.schedule_kind());
            if let Some(days) = value.get("days").filter(|_| kind != engine.schedule_kind()) {
                debug!("cloud: the {kind} schedule kept aside, {} in use", engine.schedule_kind());
                engine.set_schedule_aside(kind, &crate::schedule::Schedule::from_days(days));
            } else if let Some(days) = value.get("days") {
                let remote = crate::schedule::Schedule::from_days(days);
                let pending = self.schedule.as_ref().is_some_and(|agreed| *agreed != engine.schedule);
                if !pending && remote != engine.schedule {
                    info!("cloud: the {kind} schedule applied");
                    engine.set_schedule(remote.clone());
                    changed = true;
                }
                self.schedule = Some(remote);
            }
        }
        if key == self.shared {
            // The device's label (Placement's ADD LABEL).
            if let Some(name) = value.get("name").and_then(Json::as_str) {
                changed |= pull_text(&mut self.label, engine, crate::engine::LABEL, name);
            }
            self.time_to_target = Some(value.get("time_to_target").and_then(Json::as_i64).unwrap_or(0));
            // A new mode first, giving back its setpoint; a target sent with
            // it then still counts.
            let mut restored = false;
            if let Some(t) = value.get("target_temperature_type").and_then(Json::as_str) {
                let before = engine.settings.text(MODE).map(str::to_string);
                changed |= pull_text(&mut self.mode, engine, MODE, type_to_mode(t));
                if engine.settings.text(MODE).map(str::to_string) != before {
                    engine.setpoint_tick();
                    restored = true;
                }
            }
            if let Some(t) = value.get("target_temperature").and_then(Json::as_f64).map(|t| t as f32) {
                let pending = self.target.is_some_and(|agreed| differs(agreed, engine.thermostat.target));
                let sent = self.target.is_none_or(|agreed| differs(agreed, t));
                if (!pending || (restored && sent)) && differs(t, engine.thermostat.target) {
                    info!("cloud: target set to {t}");
                    engine.set_target(t);
                    changed = true;
                }
                self.target = Some(t);
            }
            // With the unit's own backplate the room is measured here and
            // nestlocal hears it from us.
            if let Some(c) = value.get("current_temperature").and_then(Json::as_f64).map(|c| c as f32).filter(|_| !engine.local_sensor()) {
                changed |= differs(c, engine.thermostat.current);
                engine.thermostat.current = c;
            }
            let th = &mut engine.thermostat;
            for (field, slot, agreed) in [("target_temperature_low", &mut th.low, &mut self.low),
                                          ("target_temperature_high", &mut th.high, &mut self.high)] {
                if let Some(v) = value.get(field).and_then(Json::as_f64).map(|v| v as f32) {
                    let pending = agreed.is_some_and(|a| differs(a, *slot));
                    let sent = agreed.is_none_or(|a| differs(a, v));
                    if (!pending || (restored && sent)) && differs(v, *slot) {
                        info!("cloud: {field} set to {v}");
                        *slot = v;
                        changed = true;
                    }
                    *agreed = Some(v);
                }
            }
            for (field, slot) in [("can_heat", &mut th.can_heat), ("can_cool", &mut th.can_cool)] {
                if let Some(v) = value.get(field).and_then(Json::as_bool) {
                    changed |= v != *slot;
                    *slot = v;
                }
            }
            if let Some(t) = value.get("compressor_lockout_timeout").and_then(Json::as_i64) {
                engine.hvac.equipment.backplate_lockout = t as u16;
            }
        }
        if (key == self.shared || key == self.device) && !engine.local_sensor() {
            if let Some(h) = value.get("current_humidity").and_then(Json::as_f64).map(|h| h as f32) {
                changed |= engine.thermostat.humidity.is_none_or(|old| differs(old, h));
                engine.thermostat.humidity = Some(h);
            }
        }
        if key == self.device {
            // The equipment the setup described, for the HVAC control.
            let text = |k: &str| value.get(k).and_then(Json::as_str);
            for (field, slot) in [("leaf_threshold_heat", &mut engine.thermostat.leaf.0), ("leaf_threshold_cool", &mut engine.thermostat.leaf.1)] {
                if let Some(t) = value.get(field).and_then(Json::as_f64).map(|t| t as f32) {
                    changed |= slot.is_none_or(|old| differs(old, t));
                    *slot = Some(t);
                }
            }
            // The cloud found the heating short cycling (stock's 0xface0,
            // field 0x16b).
            if let Some(status) = text("hvac_cycle_status") {
                changed |= engine.set_short_cycle(status.eq_ignore_ascii_case("heating_short_cycle"), true);
            }
            let eq = &mut engine.hvac.equipment;
            if let Some(mode) = text("power_steal_mode") {
                eq.power_steal = match mode.to_ascii_lowercase().as_str() {
                    "normal" | "unspecified" => 0,
                    "disabled" => 1,
                    "disabled_while_heating" => 2,
                    "disabled_while_cooling" => 3,
                    _ => 4,
                };
            }
            // What the setup made of the Y2 and STAR terminals (stock's
            // 0xc5660 and 0xc456c), and a heat pump's furnace.
            if let Some(y2) = text("y2_type") {
                eq.y2 = match y2.to_ascii_lowercase().as_str() { "heatingandcooling" => 0, "heating" => 1, "cooling" => 2, "g2" => 3, _ => -1 };
            }
            if let Some(star) = text("star_type") {
                eq.star = match star.to_ascii_lowercase().as_str() {
                    "emergency heat" | "emer-heat" => 0,
                    "humidifier" | "hum" => 1,
                    "dehumidifier" | "dehum" => 2,
                    "w3" => 3,
                    "waterheater" => 4,
                    "g3" => 5,
                    "ob" => 6,
                    "y2" => 7,
                    "w2" => 8,
                    _ => -1,
                };
            }
            if let Some(f) = value.get("dual_fuel_selected").and_then(Json::as_bool) {
                eq.dual_fuel = f;
            }
            if let Some(b) = value.get("dual_fuel_breakpoint").and_then(Json::as_f64) {
                eq.dual_fuel_breakpoint = b as f32;
            }
            if let Some(o) = text("dual_fuel_breakpoint_override") {
                eq.dual_fuel_override = match o.to_ascii_uppercase().as_str() { "ALWAYS-ALT" => 1, "ALWAYS-PRIMARY" => 2, _ => 0 };
            }
            if let Some(t) = text("humidifier_type") {
                eq.humidity.humidifier = if t.eq_ignore_ascii_case("BYPASS") { 2 } else if t == "steam" { 1 } else { 0 };
            }
            if let Some(t) = text("dehumidifier_type") {
                eq.humidity.dehumidifier = if t.eq_ignore_ascii_case("AC-INTEGRATED") { 2 } else if t.eq_ignore_ascii_case("STAND-ALONE") { 1 } else { 0 };
                // A new type sets Cool to Dry (stock's 0xc43f0): on for an
                // AC-integrated dehumidifier on STAR, off otherwise.
                if self.dehumidifier_type.as_deref().is_some_and(|old| old != t) {
                    let on = eq.star == 2 && eq.humidity.dehumidifier == 2;
                    info!("cloud: dehumidifier now {t}; sense.dry set to {}", if on { "on" } else { "off" });
                    engine.settings.set("sense.dry", Value::Text(if on { "on" } else { "off" }.into()));
                    changed = true;
                }
                self.dehumidifier_type = Some(t.to_string());
            }
            if eq.star != 1 {
                eq.humidity.humidifier = 0;
            }
            if eq.star != 2 {
                eq.humidity.dehumidifier = 0;
            }
            let flag = |k: &str| value.get(k).and_then(Json::as_bool);
            let target = value.get("target_humidity").and_then(Json::as_f64).map(|h| h as f32);
            if target.is_some() || flag("target_humidity_enabled").is_some() {
                eq.humidity.target = target.filter(|_| flag("target_humidity_enabled").unwrap_or(true));
            }
            if let Some(on) = flag("humidity_control_lockout_enabled") {
                let time = |k: &str| value.get(k).and_then(Json::as_u64).map(|t| (t % 86400) as u32);
                eq.humidity.quiet = on.then(|| time("humidity_control_lockout_start_time").zip(time("humidity_control_lockout_end_time"))).flatten();
            }
            if let Some(on) = flag("humidifier_fan_activation") {
                eq.humidifier_fan = on;
            }
            if let Some(on) = flag("dehumidifier_fan_activation") {
                eq.dehumidifier_fan = on;
            }
            if let Some(o) = text("dehumidifier_orientation_selected") {
                eq.dehumidifier_reversed = o.eq_ignore_ascii_case("REVERSED");
            }
            // The heat pump's settings.
            for (key, slot) in [("heat_pump_comp_threshold", &mut eq.compressor_lockout), ("heat_pump_aux_threshold", &mut eq.aux_lockout)] {
                let on = value.get(format!("{key}_enabled").as_str()).and_then(Json::as_bool);
                let t = value.get(key).and_then(Json::as_f64).map(|t| t as f32);
                if on.is_some() || t.is_some() {
                    *slot = t.filter(|_| on.unwrap_or(true));
                }
            }
            if let Some(s) = text("heatpump_savings") {
                eq.savings = match s { "max-savings" => 0, "balanced" => 1, "off" => 3, _ => 2 };
            }
            if let Some(on) = flag("ob_persistence") {
                eq.ob_persistence = on;
            }
            self.airwave_ready = Some(text("fan_cooling_readiness") == Some("ready"));
            self.furnace_shutdown = Some(flag("is_furnace_shutdown").unwrap_or(false));
            let seen = self.has.get_or_insert_with(Default::default);
            for (key, v) in value.iter().filter(|(k, _)| k.starts_with("has_") || UNIT_FLAGS.contains(&k.as_str())) {
                if let Some(b) = v.as_bool() {
                    seen.insert(key.clone(), b);
                }
            }
            let speed = |v: &str| v.strip_prefix("stage").and_then(|n| n.parse::<u8>().ok()).filter(|n| (1..=3).contains(n));
            if let Some(n) = text("fan_capabilities").and_then(speed) {
                eq.fan_stages = n;
            }
            if let Some(o) = text("ob_orientation") {
                eq.ob_cool = o != "B";
            }
            // How each heat switch's heat comes (stock's 0xface0).
            use crate::hvac::wiring::Switch as S;
            for (switch, prefix) in [(S::Stage1Heat, "heater"), (S::Stage2Heat, "heat_x2"), (S::Stage3Heat, "heat_x3"), (S::Stage1AltHeat, "alt_heat"),
                                     (S::Stage2AltHeat, "alt_heat_x2"), (S::AuxHeat, "aux_heat"), (S::EmergencyHeat, "emer_heat")] {
                if let Some(d) = text(&format!("{prefix}_delivery")) {
                    eq.delivery[switch as usize] = match d.to_ascii_lowercase().as_str() {
                        "forced-air" => 0, "in-floor-radiant" => 1, "radiator" => 2, "electric-strip" => 3, _ => -1,
                    };
                }
                if let Some(src) = text(&format!("{prefix}_source")) {
                    let code = match src.to_ascii_lowercase().as_str() {
                        "gas" => 0, "electric" => 1, "oil" => 2, "lp" => 3, "geothermal" => 4, "pellets" => 5, "district_heating" => 6, _ => -1,
                    };
                    // Pellets and district heating only for the furnace.
                    if code < 5 || switch == S::Stage1Heat {
                        eq.source[switch as usize] = code;
                    }
                }
            }
            if let Some(mode) = value.get("eco").and_then(|e| e.get("mode")).and_then(Json::as_str) {
                let on = matches!(mode, "manual-eco" | "auto-eco");
                engine.settings.set(crate::engine::ECO_AUTO, Value::Bool(mode == "auto-eco"));
                let pending = self.eco.is_some_and(|agreed| agreed != engine.eco());
                if !pending && on != engine.eco() {
                    info!("cloud: eco {}", if on { "on" } else { "off" });
                    engine.settings.set(crate::engine::ECO, Value::Bool(on));
                    changed = true;
                }
                self.eco = Some(on);
            }
            // Eco's and the safety temperatures: a value and whether it is used.
            for (agreed, fields, keys) in [(&mut self.eco_temps, ECO_FIELDS, (ECO_HEAT, ECO_COOL)), (&mut self.safety, SAFETY_FIELDS, (SAFETY_LOW, SAFETY_HIGH))] {
                let side = |t: &str| -> Option<Option<f32>> {
                    let c = value.get(t).and_then(Json::as_f64)? as f32;
                    let on = value.get(format!("{t}_enabled").as_str()).and_then(Json::as_bool).unwrap_or(true);
                    Some(on.then_some(c))
                };
                if let (Some(low), Some(high)) = (side(fields.0), side(fields.1)) {
                    let local = engine.temperatures(keys.0, keys.1);
                    let pending = agreed.is_some_and(|a| a != local);
                    if !pending && (low, high) != local {
                        info!("cloud: {} set to {low:?}, {} to {high:?}", keys.0, keys.1);
                        for (k, v) in [(keys.0, low), (keys.1, high)] {
                            engine.settings.set(k, v.map_or(Value::Text("off".into()), |c| Value::Number(f64::from(c))));
                        }
                        changed = true;
                    }
                    *agreed = Some((low, high));
                }
            }
            if let Some(timeout) = value.get("fan_timer_timeout").and_then(Json::as_f64) {
                let ends = if timeout > wall() { timeout } else { 0.0 };
                let pending = self.fan_run.is_some_and(|agreed| agreed != engine.fan_run().0);
                if !pending && ends != engine.fan_run().0 {
                    let minutes = value.get("fan_timer_duration").and_then(Json::as_f64).map_or(15, |d| (d / 60.0).round() as u64);
                    info!("cloud: fan run until {ends:.0} ({minutes} min)");
                    engine.set_fan_run(ends, minutes);
                    changed = true;
                }
                self.fan_run = Some(engine.fan_run().0);
            }
            if let Some(scale) = value.get("temperature_scale").and_then(Json::as_str).filter(|s| matches!(*s, "C" | "F")) {
                changed |= pull_text(&mut self.units, engine, UNITS, scale);
            }
            // Waking on press only is should_wake_on_approach off; otherwise
            // farsight_screen names the view.
            let wake = value.get("should_wake_on_approach").and_then(Json::as_bool);
            let screen = value.get("farsight_screen").and_then(Json::as_str);
            if let Some(view) = match (wake, screen) {
                (Some(false), _) => Some("press"),
                (_, Some(s)) if FARSIGHT_VIEWS.contains(&s) => Some(s),
                _ => None,
            } {
                changed |= pull_text(&mut self.farsight, engine, crate::engine::FARSIGHT, view);
            }
            if let Some(on) = value.get("filter_reminder_enabled").and_then(Json::as_bool) {
                changed |= pull_text(&mut self.remind, engine, REMIND, if on { "on" } else { "off" });
            }
            for ((setting, field), agreed) in EQUIPMENT.iter().zip(&mut self.equipment) {
                if let Some(v) = value.get(*field).and_then(Json::as_str) {
                    changed |= pull_text(agreed, engine, setting, v);
                }
            }
            for ((setting, field), agreed) in FAN_SPEEDS.iter().zip(&mut self.fan_speeds) {
                if let Some(v) = value.get(*field).and_then(Json::as_str).filter(|v| *v == "auto" || v.starts_with("stage")) {
                    changed |= pull_text(agreed, engine, setting, v);
                }
            }
            if let Some(click) = value.get("click_sound").and_then(Json::as_str).filter(|s| matches!(*s, "on" | "off")) {
                changed |= pull_text(&mut self.click, engine, crate::model::CLICK, click);
            }
            for ((key, field), agreed) in SWITCHES.iter().zip(&mut self.toggles) {
                if let Some(on) = value.get(*field).and_then(Json::as_bool) {
                    changed |= pull_text(agreed, engine, key, if on { "on" } else { "off" });
                }
            }
            for ((key, field, flag), agreed) in OPTIONAL.iter().zip(&mut self.optional) {
                let on = value.get(*flag).and_then(Json::as_bool);
                let v = value.get(*field).and_then(Json::as_f64);
                if on.is_none() && v.is_none() {
                    continue;
                }
                let remote = v.filter(|_| on != Some(false));
                let local = number(engine, key);
                let pending = agreed.is_some_and(|a| a != local);
                *agreed = Some(remote);
                if !pending && (local != remote || engine.settings.get(key).is_none()) {
                    info!("cloud: {key} set to {remote:?}");
                    engine.settings.set(key, remote.map_or(Value::Text("off".into()), Value::Number));
                    changed = true;
                }
            }
            for ((key, field, scale), agreed) in NUMBERS.iter().zip(&mut self.numbers) {
                if let Some(raw) = value.get(*field).and_then(Json::as_f64) {
                    let remote = (raw / scale).round();
                    let local = number(engine, key);
                    let pending = agreed.is_some() && *agreed != local;
                    *agreed = Some(remote);
                    if !pending && local != Some(remote) {
                        info!("cloud: {key} set to {remote}");
                        engine.settings.set(key, Value::Number(remote));
                        changed = true;
                    }
                }
            }
            if let Some(place) = value.get("where_id").and_then(Json::as_str) {
                changed |= pull_text(&mut self.place, engine, PLACE, place);
            }
            if let Some(mode) = value.get("fan_mode").and_then(Json::as_str) {
                changed |= pull_text(&mut self.fan_schedule, engine, FAN_SCHEDULE, if mode == "duty-cycle" { "on" } else { "off" });
            }
        }
        changed
    }

    /// What the UI changed since the last agreement, once it is still.
    pub fn push(&mut self, engine: &Engine) -> Vec<(String, Fields)> {
        if engine.idle() < SETTLE {
            return Vec::new();
        }
        // A form's fields wait for its DONE or SET, as stock sends them.
        let held = engine.held_keys();
        let held = |key: &str| held.iter().any(|k| k == key);
        let mut shared = Fields::new();
        if let Some(name) = engine.settings.text(crate::engine::LABEL).filter(|n| self.label.as_deref().is_some_and(|a| a != *n)) {
            shared.insert("name".into(), json!(name));
            self.label = Some(name.to_string());
        }
        if self.target.is_some_and(|agreed| differs(agreed, engine.thermostat.target)) {
            let t = engine.thermostat.target;
            shared.insert("target_temperature".into(), json!((t as f64 * 1000.0).round() / 1000.0));
            self.target = Some(t);
        }
        // Home/Away Assist's finding and training (stock's shared
        // auto_away and auto_away_learning).
        let away = (engine.away(), engine.away_ready());
        if engine.local_sensor() && self.away != Some(away) {
            shared.insert("auto_away".into(), json!(i32::from(away.0)));
            shared.insert("auto_away_learning".into(), json!(if away.1 { "ready" } else { "training" }));
            self.away = Some(away);
        }
        let th = &engine.thermostat;
        if engine.local_sensor() && self.room.is_none_or(|r| differs(r, th.current)) {
            shared.insert("current_temperature".into(), json!((th.current as f64 * 100.0).round() / 100.0));
            self.room = Some(th.current);
        }
        if engine.local_sensor() {
            use crate::hvac::wiring::Switch;
            let h = &engine.hvac;
            // The fan's state counts its second and third stages (stock's 0x1034a4).
            let fan = h.is_on(Switch::Fan) || h.is_on(Switch::Stage2Fan) || h.is_on(Switch::Stage3Fan);
            let states = [h.is_on(Switch::Stage1Heat), h.is_on(Switch::Stage1Cool), fan,
                          h.is_on(Switch::Stage2Heat), h.is_on(Switch::Stage2Cool), h.is_on(Switch::Stage3Heat), h.is_on(Switch::Stage3Cool),
                          h.is_on(Switch::Humidifier), h.is_on(Switch::Dehumidifier), h.is_on(Switch::AcDehumidifier),
                          h.is_on(Switch::AuxHeat), h.is_on(Switch::EmergencyHeat), h.is_on(Switch::Stage1AltHeat), h.is_on(Switch::Stage2AltHeat)];
            if self.switches != Some(states) {
                for (field, on) in ["hvac_heater_state", "hvac_ac_state", "hvac_fan_state", "hvac_heat_x2_state", "hvac_cool_x2_state",
                                    "hvac_heat_x3_state", "hvac_cool_x3_state", "humidifier_state", "dehumidifier_state",
                                    "auto_dehum_state", "hvac_aux_heater_state", "hvac_emer_heat_state", "hvac_alt_heat_state",
                                    "hvac_alt_heat_x2_state"].iter().zip(states) {
                    shared.insert((*field).into(), json!(on));
                }
                self.switches = Some(states);
            }
        }
        for (field, value, agreed) in [("target_temperature_low", th.low, &mut self.low),
                                       ("target_temperature_high", th.high, &mut self.high)] {
            if agreed.is_some_and(|a| differs(a, value)) {
                shared.insert(field.into(), json!((value as f64 * 1000.0).round() / 1000.0));
                *agreed = Some(value);
            }
        }
        let mode = engine.settings.text(MODE).unwrap_or("heat");
        if self.mode.as_deref().is_some_and(|m| m != mode) {
            shared.insert("target_temperature_type".into(), json!(mode_to_type(mode)));
            self.mode = Some(mode.to_string());
        }
        // When the room should reach the target, as stock publishes it
        // (0x4e33f0): within 2.5 h, else 0, sent when it moves 300 s.
        if let Some(sent) = self.time_to_target {
            let eta = engine.time_to_target_seconds().filter(|s| *s <= 9000.0).map_or(0, |s| (wall() + s) as i64);
            if (eta == 0) != (sent == 0) || (eta - sent).abs() >= 300 {
                shared.insert("time_to_target".into(), json!(eta));
                self.time_to_target = Some(eta);
            }
        }
        let mut out = Vec::new();
        if !shared.is_empty() {
            out.push((self.shared.clone(), shared));
        }
        // The equipment, from the wiring, and the flags only the unit
        // writes (stock's preconditioning_ready and heatpump_ready are set
        // once and stay true).
        let flag = |k: &str| engine.settings.get(k) == Some(&Value::Bool(true));
        let flags = [("preconditioning_active", engine.preconditioning().is_some()), ("preconditioning_ready", true), ("heatpump_ready", true),
                     ("sunlight_correction_ready", flag(crate::engine::SUNBLOCK_READY)), ("sunlight_correction_active", flag(crate::engine::SUNBLOCK_ACTIVE))];
        if let Some(seen) = &mut self.has {
            let changed: Map<String, Json> = flags.iter().filter(|(k, v)| seen.get(*k) != Some(v)).map(|(k, v)| (k.to_string(), json!(v))).collect();
            if !changed.is_empty() {
                for (k, v) in flags {
                    seen.insert(k.to_string(), v);
                }
                out.push((self.device.clone(), changed));
            }
        }
        if let (Some(seen), Some(has)) = (&mut self.has, engine.hvac.has()) {
            let changed: Map<String, Json> = has.iter().filter(|(k, v)| seen.get(*k) != Some(v)).map(|(k, v)| (k.to_string(), json!(v))).collect();
            if !changed.is_empty() {
                for (k, v) in has {
                    seen.insert(k.to_string(), v);
                }
                out.push((self.device.clone(), changed));
            }
        }
        // Airwave's readiness, which only the unit writes.
        let ready = matches!(engine.settings.get(crate::engine::AIRWAVE_READY), Some(Value::Bool(true)));
        if self.airwave_ready.is_some_and(|a| a != ready) {
            out.push((self.device.clone(), Map::from_iter([("fan_cooling_readiness".to_string(), json!(if ready { "ready" } else { "not ready" }))])));
            self.airwave_ready = Some(ready);
        }
        let fan_cooling = engine.local_sensor().then(|| engine.hvac.fan_cooling());
        if fan_cooling.is_some() && fan_cooling != self.fan_cooling {
            out.push((self.device.clone(), Map::from_iter([("fan_cooling_state".to_string(), json!(fan_cooling))])));
            self.fan_cooling = fan_cooling;
        }
        // The filter reminder's state, which only the unit writes.
        let setting = |k: &str| match engine.settings.get(k) { Some(&Value::Number(n)) => n, _ => 0.0 };
        let filter = (setting(crate::engine::FILTER_LEVEL) as u8, engine.settings.get(crate::engine::FILTER_NEEDED) == Some(&Value::Bool(true)),
                      setting(crate::engine::FILTER_RUNTIME) as u64, setting(crate::engine::FILTER_CHANGED) as i64);
        if engine.local_sensor() && self.filter != Some(filter) {
            let mut f = Map::from_iter([("filter_reminder_level".to_string(), json!(filter.0)), ("filter_replacement_needed".to_string(), json!(filter.1)),
                                        ("filter_runtime_sec".to_string(), json!(filter.2))]);
            if filter.3 > 0 {
                f.insert("filter_changed_date".into(), json!(filter.3));
            }
            out.push((self.device.clone(), f));
            self.filter = Some(filter);
        }
        // The fan's speed as its switches run it (stock's fan_current_speed).
        let speed = engine.local_sensor().then(|| {
            use crate::hvac::wiring::Switch;
            let h = &engine.hvac;
            if h.is_on(Switch::Stage3Fan) { "stage3" } else if h.is_on(Switch::Stage2Fan) { "stage2" } else if h.is_on(Switch::Fan) { "stage1" } else { "off" }
        });
        if speed.is_some() && speed != self.fan_speed {
            out.push((self.device.clone(), Map::from_iter([("fan_current_speed".to_string(), json!(speed))])));
            self.fan_speed = speed;
        }
        let mut device = Fields::new();
        if self.eco.is_some_and(|agreed| agreed != engine.eco()) {
            let auto = engine.settings.get(crate::engine::ECO_AUTO) == Some(&Value::Bool(true));
            let mode = if !engine.eco() { "schedule" } else if auto { "auto-eco" } else { "manual-eco" };
            // touched_by 2: changed at the unit, as stock marks it.
            device.insert("eco".into(), json!({ "mode": mode, "mode_update_timestamp": wall() as i64, "touched_by": 2 }));
            self.eco = Some(engine.eco());
        }
        for (agreed, fields, keys) in [(&mut self.eco_temps, ECO_FIELDS, (ECO_HEAT, ECO_COOL)), (&mut self.safety, SAFETY_FIELDS, (SAFETY_LOW, SAFETY_HIGH))] {
            let temps = engine.temperatures(keys.0, keys.1);
            if agreed.is_some_and(|a| a != temps) && !held(keys.0) && !held(keys.1) {
                for (field, t) in [(fields.0, temps.0), (fields.1, temps.1)] {
                    device.insert(format!("{field}_enabled"), json!(t.is_some()));
                    if let Some(c) = t {
                        device.insert(field.into(), json!((c as f64 * 1000.0).round() / 1000.0));
                    }
                }
                *agreed = Some(temps);
            }
        }
        let (ends, minutes) = engine.fan_run();
        if self.fan_run.is_some_and(|agreed| agreed != ends) {
            device.insert("fan_timer_timeout".into(), json!(ends as i64));
            if ends > 0.0 {
                device.insert("fan_timer_duration".into(), json!(minutes * 60));
            }
            self.fan_run = Some(ends);
        }
        if !device.is_empty() {
            out.push((self.device.clone(), device));
        }
        let units = engine.settings.text(UNITS).unwrap_or("C");
        if self.units.as_deref().is_some_and(|u| u != units) {
            out.push((self.device.clone(), Map::from_iter([("temperature_scale".to_string(), json!(units))])));
            self.units = Some(units.to_string());
        }
        let view = engine.settings.text(crate::engine::FARSIGHT).unwrap_or("target_temp");
        if self.farsight.as_deref().is_some_and(|v| v != view) {
            let mut f = Map::from_iter([("should_wake_on_approach".to_string(), json!(view != "press"))]);
            if FARSIGHT_VIEWS.contains(&view) {
                f.insert("farsight_screen".into(), json!(view));
            }
            out.push((self.device.clone(), f));
            self.farsight = Some(view.to_string());
        }
        let remind = engine.settings.text(REMIND).unwrap_or("off");
        if self.remind.as_deref().is_some_and(|r| r != remind) {
            out.push((self.device.clone(), Map::from_iter([("filter_reminder_enabled".to_string(), json!(remind == "on"))])));
            self.remind = Some(remind.to_string());
        }
        let click = engine.settings.text(crate::model::CLICK).unwrap_or("on");
        if self.click.as_deref().is_some_and(|c| c != click) {
            out.push((self.device.clone(), Map::from_iter([("click_sound".to_string(), json!(click))])));
            self.click = Some(click.to_string());
        }
        for ((setting, field), agreed) in EQUIPMENT.iter().zip(&mut self.equipment) {
            if let Some(v) = engine.settings.text(setting).filter(|v| agreed.as_deref().is_some_and(|a| a != *v)) {
                out.push((self.device.clone(), Map::from_iter([(field.to_string(), json!(v))])));
                *agreed = Some(v.to_string());
            }
        }
        for ((setting, field), agreed) in FAN_SPEEDS.iter().zip(&mut self.fan_speeds) {
            if let Some(v) = engine.settings.text(setting).filter(|v| agreed.as_deref().is_some_and(|a| a != *v) && !held(setting)) {
                out.push((self.device.clone(), Map::from_iter([(field.to_string(), json!(v))])));
                *agreed = Some(v.to_string());
            }
        }
        for ((key, field), agreed) in SWITCHES.iter().zip(&mut self.toggles) {
            let Some(v) = engine.settings.text(key) else { continue };
            if agreed.as_deref().is_some_and(|a| a != v) {
                out.push((self.device.clone(), Map::from_iter([(field.to_string(), json!(v == "on"))])));
                *agreed = Some(v.to_string());
            }
        }
        for ((key, field, flag), agreed) in OPTIONAL.iter().zip(&mut self.optional) {
            let local = number(engine, key);
            if engine.settings.get(key).is_some() && agreed.is_some_and(|a| a != local) && !held(key) {
                let mut f = Map::new();
                if let Some(v) = local {
                    f.insert(field.to_string(), json!(v));
                }
                if !flag.is_empty() {
                    f.insert(flag.to_string(), json!(local.is_some()));
                }
                if !f.is_empty() {
                    out.push((self.device.clone(), f));
                }
                *agreed = Some(local);
            }
        }
        for ((key, field, scale), agreed) in NUMBERS.iter().zip(&mut self.numbers) {
            let local = number(engine, key);
            if agreed.is_some() && local.is_some() && local != *agreed && !held(key) {
                out.push((self.device.clone(), Map::from_iter([(field.to_string(), json!(local.unwrap_or(0.0) * scale))])));
                *agreed = local;
            }
        }
        // Stock pushes it whenever it changes, from the start.
        let heat_link = engine.heat_link_connection();
        if heat_link.is_some() && heat_link != self.heat_link {
            out.push((self.device.clone(), Map::from_iter([("heat_link_connection".to_string(), json!(heat_link))])));
            self.heat_link = heat_link;
        }
        // The furnace shutdown detection's flag, which only the unit
        // writes, sent as it differs (stock's 0xbdd60 marks it for the
        // device bucket).
        let furnace = engine.furnace_shutdown();
        if self.furnace_shutdown.is_some_and(|a| a != furnace) {
            out.push((self.device.clone(), Map::from_iter([("is_furnace_shutdown".to_string(), json!(furnace))])));
            self.furnace_shutdown = Some(furnace);
        }
        if let Some(place) = engine.settings.text(PLACE).filter(|p| self.place.as_deref().is_some_and(|a| a != *p)) {
            out.push((self.device.clone(), Map::from_iter([("where_id".to_string(), json!(place))])));
            self.place = Some(place.to_string());
        }
        if let Some(structure) = &self.structure {
            let mut facts = Fields::new();
            for ((setting, field), agreed) in HOME.iter().zip(&mut self.home) {
                if let Some(v) = engine.settings.text(setting).filter(|v| agreed.as_deref().is_some_and(|a| a != *v)) {
                    facts.insert(field.to_string(), json!(v));
                    *agreed = Some(v.to_string());
                }
            }
            if !facts.is_empty() {
                let place: Fields = facts.iter().filter(|(k, _)| PLACE_FIELDS.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
                if !place.is_empty() {
                    out.push((self.device.clone(), place));
                }
                out.push((structure.clone(), facts));
            }
        }
        if self.schedule.as_ref().is_some_and(|agreed| *agreed != engine.schedule) {
            let kind = engine.schedule_kind();
            out.push((self.schedule_key.clone(), Map::from_iter([("days".to_string(), engine.schedule.to_days(kind, wall() as i64)),
                                                                  ("schedule_mode".to_string(), json!(kind))])));
            self.schedule = Some(engine.schedule.clone());
        }
        let schedule = engine.settings.text(FAN_SCHEDULE).unwrap_or("off");
        if self.fan_schedule.as_deref().is_some_and(|a| a != schedule) && !held(FAN_SCHEDULE) {
            let mode = if schedule == "on" { "duty-cycle" } else { "auto" };
            out.push((self.device.clone(), Map::from_iter([("fan_mode".to_string(), json!(mode))])));
            self.fan_schedule = Some(schedule.to_string());
        }
        out
    }
}

/// Farsight's views, as farsight_screen names them.
const FARSIGHT_VIEWS: [&str; 5] = ["target_temp", "current_temp", "analog_clock", "digital_clock", "weather"];

/// The Reminders choice: whether to be reminded to change the air filter.
const REMIND: &str = "filter.remind";

/// Settings "on" or "off" and the device bucket's fields for them: Nest
/// Sense's, Home/Away Assist and Equipment Settings' (dual fuel, quiet time).
const SWITCHES: [(&str, &str); 11] = [("sense.radiant", "radiant_control_enabled"), ("filter.remind", "filter_reminder_enabled"), (crate::engine::EMERGENCY, "emer_heat_enable"), ("sense.learning", "learning_mode"), ("sense.early", "preconditioning_enabled"),
    ("sense.dry", "auto_dehum_enabled"), ("sense.sunblock", "sunlight_correction_enabled"), (crate::engine::AIRWAVE, "fan_cooling_enabled"),
    ("away.assist", "auto_away_enable"), (crate::equipment::DUAL_FUEL, "dual_fuel_selected"), (crate::equipment::QUIET, "humidity_control_lockout_enabled")];
/// Numbers kept as settings, the device bucket's fields for them, and
/// what one unit of the setting is in the field: the filter reminder's
/// thresholds, Early-On's hours, and the fan schedule's minutes an hour and
/// hours of the day, and quiet time's hours, as seconds.
const NUMBERS: [(&str, &str, f64); 8] = [(crate::engine::FILTER_THRESHOLD, "filter_replacement_threshold_sec", 1.0),
    (crate::engine::FILTER_SECOND, "second_filter_reminder_threshold_sec", 1.0), ("sense.early.hours", "max_nighttime_preconditioning_seconds", 3600.0),
    (crate::engine::FAN_DUTY, "fan_duty_cycle", 60.0), (crate::engine::FAN_START, "fan_duty_start_time", 3600.0),
    (crate::engine::FAN_END, "fan_duty_end_time", 3600.0), (crate::equipment::QUIET_START, "humidity_control_lockout_start_time", 3600.0),
    (crate::equipment::QUIET_END, "humidity_control_lockout_end_time", 3600.0)];
/// Location's Construction answers and its place, in the structure
/// bucket; the place goes in the device bucket too, as stock's does.
const HOME: [(&str, &str); 5] = [("home.type", "house_type"), ("home.thermostats", "num_thermostats"), ("home.built", "renovation_date"),
    (crate::engine::POSTAL, "postal_code"), (crate::engine::COUNTRY, "country_code")];
const PLACE_FIELDS: [&str; 2] = ["postal_code", "country_code"];
/// PRO SETUP's answers and the device bucket's fields for them.
/// Equipment Settings' texts and Heat Pump Balance's level follow.
const EQUIPMENT: [(&str, &str); 11] = [(crate::engine::BOILER, "boiler_type"), (crate::engine::HEAT_CONTROL, "heat_link_heat_type"),
    (crate::engine::HEAT_SOURCE, "heater_source"), (crate::engine::HEAT_DELIVERY, "heater_delivery"),
    (crate::engine::HOT_WATER_CONTROL, "heat_link_hot_water_type"), (crate::equipment::OB, "ob_orientation"),
    (crate::equipment::STAR, "star_type"), (crate::equipment::Y2, "y2_type"), (crate::equipment::OVERRIDE, "dual_fuel_breakpoint_override"),
    (crate::equipment::DEHUMIDIFIER_TYPE, "dehumidifier_type"), (crate::engine::BALANCE_LEVEL, "heatpump_savings")];
/// Equipment Settings' numbers that may be off: the setting, its field,
/// and the field that enables it (none: always on).
const OPTIONAL: [(&str, &str, &str); 4] = [
    (crate::equipment::COMPRESSOR, "heat_pump_comp_threshold", "heat_pump_comp_threshold_enabled"),
    (crate::equipment::AUX, "heat_pump_aux_threshold", "heat_pump_aux_threshold_enabled"),
    (crate::equipment::HUMIDITY, "target_humidity", "target_humidity_enabled"),
    (crate::equipment::BREAKPOINT, "dual_fuel_breakpoint", "")];
/// The fan's speeds and the device bucket's fields for them.
const FAN_SPEEDS: [(&str, &str); 3] = [(crate::engine::FAN_TIMER_SPEED, "fan_timer_speed"), (crate::engine::FAN_SCHEDULE_SPEED, "fan_schedule_speed"),
    (crate::engine::FAN_HEAT_COOL_SPEED, "fan_heat_cool_speed")];
/// The room the thermostat is in, as the device bucket's where_id.
const PLACE: &str = "placement.where";
/// Whether the fan schedule is on: the device bucket's fan_mode
/// "duty-cycle" (and "auto" when off).
const FAN_SCHEDULE: &str = crate::engine::FAN_SCHEDULE;

fn number(engine: &Engine, key: &str) -> Option<f64> {
    match engine.settings.get(key) {
        Some(&Value::Number(n)) => Some(n),
        _ => None,
    }
}

/// The device bucket's fields for Eco's and the safety temperatures.
const ECO_FIELDS: (&str, &str) = ("away_temperature_low", "away_temperature_high");
const SAFETY_FIELDS: (&str, &str) = ("lower_safety_temp", "upper_safety_temp");

/// Seconds since the epoch.
fn wall() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn pull_text(agreed: &mut Option<String>, engine: &mut Engine, key: &str, remote: &str) -> bool {
    let local = engine.settings.text(key).map(str::to_string);
    let pending = agreed.is_some() && *agreed != local;
    *agreed = Some(remote.to_string());
    if pending || local.as_deref() == Some(remote) {
        return false;
    }
    info!("cloud: {key} set to {remote}");
    engine.settings.set(key, Value::Text(remote.to_string()));
    true
}

/// The unit's serial: its hostname.
pub fn hostname() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: gethostname writes at most buf.len() bytes into buf.
    unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::Tree;
    use crate::engine::Input;
    use crate::model::Settings;

    fn engine() -> Engine {
        let tree = Tree { values: Vec::new(), title: "Menu".into(), back: "Back".into(), items: vec![] };
        Engine::new(tree, vec![(UNITS.into(), Value::Text("C".into())), (MODE.into(), Value::Text("heat".into()))], Settings::load(None))
    }

    fn fields(v: Json) -> Fields {
        v.as_object().unwrap().clone()
    }

    fn settled(e: &mut Engine) {
        e.input(crate::engine::Input::Press);
        std::thread::sleep(SETTLE);
    }

    #[test]
    fn urls_split_into_host_port_and_path() {
        assert_eq!(parse_url("https://127.0.0.1/entry").unwrap(), Url { host: "127.0.0.1".into(), port: 443, path: "/entry".into() });
        assert_eq!(parse_url("https://localhost:8443").unwrap(), Url { host: "localhost".into(), port: 8443, path: "/".into() });
        assert!(parse_url("http://127.0.0.1/entry").is_err());
    }

    #[test]
    fn responses_are_read_sized_or_chunked() {
        let sized = "HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n{}{}trailing";
        assert_eq!(read_response(sized.as_bytes()).unwrap(), (200, b"{}{}".to_vec()));
        let chunked = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n{\"a\r\n5\r\n\":1}\n\r\n0\r\n\r\n";
        assert_eq!(read_response(chunked.as_bytes()).unwrap(), (200, b"{\"a\":1}\n".to_vec()));
        let refused = "HTTP/1.1 400 Bad Request\r\nContent-Length: 9\r\n\r\nbad json\n";
        assert_eq!(read_response(refused.as_bytes()).unwrap_err(), "HTTP 400: bad json");
    }

    #[test]
    fn subscribes_take_newer_buckets_and_send_ours_when_asked() {
        let mut b = Buckets::default();
        b.by_key.insert("shared.S".into(), Bucket::default());
        b.by_key.insert("device.S".into(), Bucket { revision: 2, timestamp: 900, value: Some(fields(json!({"k": 1}))) });
        let first: Json = serde_json::from_slice(&b.subscribe_body()).unwrap();
        assert_eq!(first["objects"][1], json!({"object_key": "shared.S", "object_revision": 0, "object_timestamp": 0}));

        let changed = b.take(br#"{"objects":[
            {"object_key":"shared.S","object_revision":4,"object_timestamp":1000,"value":{"target_temperature":20}},
            {"object_key":"device.S","object_revision":1,"object_timestamp":500},
            {"object_key":"other.S","object_revision":1,"object_timestamp":1,"value":{}}]}"#).unwrap();
        assert_eq!(changed, vec![("shared.S".to_string(), fields(json!({"target_temperature": 20})))]);
        let next: Json = serde_json::from_slice(&b.subscribe_body()).unwrap();
        assert_eq!(next["objects"][0], json!({"object_key": "device.S", "object_revision": 2, "object_timestamp": 900, "value": {"k": 1}}));
        assert_eq!(next["objects"][1], json!({"object_key": "shared.S", "object_revision": 4, "object_timestamp": 1000}));

        b.sent("shared.S", &fields(json!({"target_temperature": 21})),
               br#"{"objects":[{"object_key":"shared.S","object_revision":5,"object_timestamp":1100}]}"#);
        let s = &b.by_key["shared.S"];
        assert_eq!((s.revision, s.timestamp, s.value.as_ref().unwrap()["target_temperature"].clone()), (5, 1100, json!(21)));
        // A put before the bucket came whole leaves it unknown, for the subscribe to fetch.
        b.by_key.insert("user.S".into(), Bucket::default());
        b.sent("user.S", &fields(json!({"k": 2})), br#"{"objects":[{"object_key":"user.S","object_revision":7,"object_timestamp":1200}]}"#);
        assert_eq!((b.by_key["user.S"].revision, b.by_key["user.S"].value.is_none()), (0, true));
    }

    #[test]
    fn nestlocals_buckets_set_the_ui_and_its_changes_go_back() {
        let mut e = engine();
        let mut s = Sync::new("S");
        assert!(s.push(&e).is_empty(), "nothing goes out before nestlocal is heard");
        assert!(s.pull(&mut e, "shared.S", &fields(json!({
            "target_temperature": 19.5, "current_temperature": 20.25, "target_temperature_type": "range"}))));
        assert!(s.pull(&mut e, "device.S", &fields(json!({"temperature_scale": "F", "preconditioning_active": false,
                                                         "preconditioning_ready": true, "heatpump_ready": true, "sunlight_correction_ready": false, "sunlight_correction_active": false}))));
        assert_eq!((e.thermostat.target, e.thermostat.current), (19.5, 20.25));
        assert_eq!((e.settings.text(MODE), e.settings.text(UNITS)), (Some("heat-cool"), Some("F")));
        settled(&mut e);
        assert!(s.push(&e).is_empty(), "what came from nestlocal is not echoed");

        e.settings.set(MODE, Value::Text("cool".into()));
        e.input(crate::engine::Input::Wheel(2));
        assert!(s.push(&e).is_empty(), "not while the wheel turns");
        e.settings.set(UNITS, Value::Text("C".into()));
        settled(&mut e);
        let out = s.push(&e);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, "shared.S");
        assert_eq!(out[0].1["target_temperature_type"], json!("cool"));
        assert!((out[0].1["target_temperature"].as_f64().unwrap() - 20.556).abs() < 0.001);
        assert_eq!(out[1], ("device.S".to_string(), fields(json!({"temperature_scale": "C"}))));
        assert!(s.push(&e).is_empty());
    }

    #[test]
    fn the_fan_schedule_follows_the_device_bucket_both_ways() {
        let mut e = engine();
        let mut s = Sync::new("S");
        assert!(s.pull(&mut e, "device.S", &fields(json!({
            "fan_mode": "duty-cycle", "fan_duty_cycle": 900, "fan_duty_start_time": 28800, "fan_duty_end_time": 61200}))));
        assert_eq!(e.settings.text(crate::engine::FAN_SCHEDULE), Some("on"));
        assert_eq!(e.settings.get(crate::engine::FAN_DUTY), Some(&Value::Number(15.0)));
        assert_eq!(e.settings.get(crate::engine::FAN_END), Some(&Value::Number(17.0)));
        e.settings.set(crate::engine::FAN_SCHEDULE, Value::Text("off".into()));
        e.settings.set(crate::engine::FAN_START, Value::Number(9.0));
        settled(&mut e);
        let out = s.push(&e);
        let sent: Map<String, Json> = out.into_iter().flat_map(|(_, f)| f).collect();
        assert_eq!((&sent["fan_mode"], &sent["fan_duty_start_time"]), (&json!("auto"), &json!(32400.0)));
    }

    #[test]
    fn the_fan_speeds_follow_the_device_bucket_both_ways() {
        let mut e = engine();
        let mut s = Sync::new("S");
        assert!(s.pull(&mut e, "device.S", &fields(json!({
            "fan_timer_speed": "stage2", "fan_schedule_speed": "stage3", "fan_heat_cool_speed": "auto"}))));
        assert_eq!(e.settings.text(crate::engine::FAN_TIMER_SPEED), Some("stage2"));
        assert_eq!(e.settings.text(crate::engine::FAN_SCHEDULE_SPEED), Some("stage3"));
        e.settings.set(crate::engine::FAN_HEAT_COOL_SPEED, Value::Text("stage1".into()));
        settled(&mut e);
        let sent: Map<String, Json> = s.push(&e).into_iter().flat_map(|(_, f)| f).collect();
        assert_eq!(sent.get("fan_heat_cool_speed"), Some(&json!("stage1")));
        assert!(!sent.contains_key("fan_timer_speed"), "only what changed");
    }

    #[test]
    fn a_forms_fields_wait_for_its_done_and_an_abandoned_form_sends_nothing() {
        use crate::menu::{Field, Item, Kind, Shown};
        let field = Field { label: "Duty".into(), short: String::new(), key: crate::engine::FAN_DUTY.into(), min: 15.0, max: 60.0, step: 15.0, off: None, unit: Some("min".into()), options: Vec::new() };
        let form = Item { label: "Fan Schedule".into(), icon: None, style: None, prompt: None, shown: Shown::default(),
                          kind: Kind::Form { fields: vec![field], more: None, link: None, more_first: false, enable: None, action: None } };
        let tree = Tree { values: Vec::new(), title: "Menu".into(), back: "Back".into(), items: vec![form] };
        let mut e = Engine::new(tree, vec![(UNITS.into(), Value::Text("C".into())), (MODE.into(), Value::Text("heat".into()))], Settings::load(None));
        let mut s = Sync::new("S");
        assert!(s.pull(&mut e, "device.S", &fields(json!({"fan_duty_cycle": 900}))));
        // Open the menus on the form, open it, change its field and set it (the form stays open).
        for input in [Input::Press, Input::Release, Input::Wheel(-1), Input::Press, Input::Release, Input::Press, Input::Release, Input::Wheel(1), Input::Press, Input::Release] {
            e.input(input);
        }
        assert!(e.nav_form_open() && e.settings.get(crate::engine::FAN_DUTY) == Some(&Value::Number(30.0)));
        settled(&mut e);
        assert!(s.push(&e).iter().all(|(_, f)| !f.contains_key("fan_duty_cycle")), "held while the form is open");
        e.set_display_idle(std::time::Duration::ZERO);
        assert!(e.tick(), "the display sleeps: the form is abandoned");
        assert_eq!(e.settings.get(crate::engine::FAN_DUTY), Some(&Value::Number(15.0)));
        settled(&mut e);
        assert!(s.push(&e).iter().all(|(_, f)| !f.contains_key("fan_duty_cycle")), "nothing to send");
    }

    #[test]
    fn the_weather_gives_the_outside_temperature_and_stocks_picture() {
        let mut e = engine();
        let mut s = Sync::new("S");
        assert_eq!(e.ui(480.0, 480.0).thermostat.outdoor, None);
        let day = wall();
        assert!(s.pull(&mut e, WEATHER, &fields(json!({"temp_c": 12.4, "humidity": 70, "icon": "partlycloudy",
                                                        "sunrise": day - 3600.0, "sunset": day + 3600.0}))));
        let t = e.ui(480.0, 480.0).thermostat;
        assert_eq!((t.outdoor, t.weather_image), (Some(12.4), Some("Weather_PartlyCloudyDay.png")));
        s.pull(&mut e, WEATHER, &fields(json!({"temp_c": 1.0, "humidity": 90, "icon": "snow"})));
        assert_eq!(e.ui(480.0, 480.0).thermostat.weather_image, Some("Weather_Snow.png"));
        s.pull(&mut e, WEATHER, &fields(json!({"temp_c": 1.0, "humidity": 90, "icon": "clear", "sunrise": day + 60.0, "sunset": day + 3600.0})));
        assert_eq!(e.ui(480.0, 480.0).thermostat.weather_image, Some("Weather_ClearNight.png"));
    }

    #[test]
    fn the_weathers_place_is_what_locations_tile_says() {
        let mut e = engine();
        let mut s = Sync::new("S");
        assert_eq!(e.weather_place(), None);
        e.settings.set(crate::engine::COUNTRY, Value::Text("US".into()));
        e.settings.set(crate::engine::POSTAL, Value::Text("94043".into()));
        assert_eq!(e.weather_place().as_deref(), Some("94043,US"));
        assert!(s.pull(&mut e, WEATHER, &fields(json!({"temp_c": 15.0, "humidity": 56, "city": "Mountain View", "state": "CA", "zip": "94043"}))));
        assert_eq!(e.settings.text(crate::engine::PLACE), Some("Mountain View\nCA\n94043"));
        assert_eq!(e.settings.text(crate::engine::PLACE_ASKED), Some("Mountain View, CA\n94043"));
    }

    #[test]
    fn a_mode_from_nestlocal_gives_back_that_modes_setpoint_and_says_so() {
        let mut e = engine();
        let mut s = Sync::new("S");
        s.pull(&mut e, "shared.S", &fields(json!({"target_temperature": 21.0, "target_temperature_type": "heat"})));
        e.tick();
        s.pull(&mut e, "shared.S", &fields(json!({"target_temperature": 21.0, "target_temperature_type": "cool"})));
        s.pull(&mut e, "shared.S", &fields(json!({"target_temperature": 24.0, "target_temperature_type": "cool"})));
        assert_eq!(e.thermostat.target, 24.0);
        s.pull(&mut e, "shared.S", &fields(json!({"target_temperature": 24.0, "target_temperature_type": "heat"})));
        assert_eq!(e.thermostat.target, 21.0, "heat's own, not the bucket's stale one");
        settled(&mut e);
        let out = s.push(&e);
        assert!(out.iter().any(|(_, f)| f.get("target_temperature") == Some(&json!(21.0))), "{out:?}");
        // Both at once: the target sent wins.
        s.pull(&mut e, "shared.S", &fields(json!({"target_temperature": 25.5, "target_temperature_type": "cool"})));
        assert_eq!(e.thermostat.target, 25.5);
    }

    #[test]
    fn a_new_dehumidifier_type_sets_cool_to_dry() {
        let mut e = engine();
        let mut s = Sync::new("S");
        s.pull(&mut e, "device.S", &fields(json!({"star_type": "dehum", "dehumidifier_type": "STAND-ALONE"})));
        assert_eq!(e.settings.text("sense.dry"), None, "the first one seen is only kept");
        s.pull(&mut e, "device.S", &fields(json!({"dehumidifier_type": "AC-INTEGRATED"})));
        assert_eq!(e.settings.text("sense.dry"), Some("on"));
        s.pull(&mut e, "device.S", &fields(json!({"dehumidifier_type": "STAND-ALONE"})));
        assert_eq!(e.settings.text("sense.dry"), Some("off"));
    }

    #[test]
    fn the_schedule_follows_the_schedule_bucket_both_ways() {
        use crate::schedule::{Setpoint, DAY};
        let mut e = engine();
        let mut s = Sync::new("S");
        assert!(s.pull(&mut e, "schedule.S", &fields(json!({"days": {"2": {
            "0": {"entry_type": "continuation", "temp": 20.0, "time": 0, "type": "COOL"},
            "1": {"entry_type": "setpoint", "temp": 20.0, "time": 48600, "type": "COOL"}}}}))));
        assert_eq!(e.schedule.points, vec![Setpoint { minute: 2 * DAY + 810, temp: 20.0, high: None }]);
        e.schedule.set(Setpoint { minute: 285, temp: 22.5, high: None });
        settled(&mut e);
        let out = s.push(&e);
        let (key, sent) = out.into_iter().find(|(k, _)| k == "schedule.S").expect("the schedule sent");
        assert_eq!(key, "schedule.S");
        assert_eq!(sent["days"]["0"]["1"]["time"], json!(17100));
        assert_eq!(sent["days"]["0"]["1"]["temp"], json!(22.5));
        assert_eq!(sent["days"]["0"]["0"]["entry_type"], json!("continuation"));
        assert!(s.push(&e).iter().all(|(k, _)| k != "schedule.S"), "sent once");
        assert_eq!(sent["schedule_mode"], json!("HEAT"));
        // Another type's schedule is kept aside, not followed.
        assert!(!s.pull(&mut e, "schedule.S", &fields(json!({"schedule_mode": "RANGE", "days": {"1": {
            "0": {"entry_type": "setpoint", "temp-min": 19.0, "temp-max": 26.0, "time": 3600, "type": "RANGE"}}}}))));
        assert_eq!(e.schedule.points.len(), 2);
        assert!(e.settings.text("schedule.RANGE").is_some_and(|t| t.contains("temp-max")));
    }

    #[test]
    fn eco_and_the_fan_run_follow_the_device_bucket_both_ways() {
        let mut e = engine();
        let mut s = Sync::new("S");
        let later = wall() + 600.0;
        assert!(s.pull(&mut e, "device.S", &fields(json!({
            "eco": {"mode": "manual-eco", "mode_update_timestamp": 1, "touched_by": 4},
            "away_temperature_low": 8.889, "away_temperature_low_enabled": true,
            "away_temperature_high": 24.444, "away_temperature_high_enabled": false,
            "fan_timer_timeout": later, "fan_timer_duration": 1800,
            "preconditioning_active": false, "preconditioning_ready": true, "heatpump_ready": true, "sunlight_correction_ready": false, "sunlight_correction_active": false}))));
        assert!(e.eco());
        assert_eq!(e.eco_temperatures(), (Some(8.889), None));
        assert!(e.fan_running());
        assert_eq!(e.hvac_mode(), crate::hvac::control::Mode::Heat(8.889));
        settled(&mut e);
        assert!(s.push(&e).is_empty(), "nothing echoed");

        e.settings.set(crate::engine::ECO, Value::Bool(false));
        e.set_fan_run(0.0, 0);
        settled(&mut e);
        let out = s.push(&e);
        assert_eq!(out[0].0, "device.S");
        assert_eq!((&out[0].1["eco"]["mode"], &out[0].1["eco"]["touched_by"]), (&json!("schedule"), &json!(2)));
        assert_eq!(out[0].1["fan_timer_timeout"], json!(0));
        assert!(s.push(&e).is_empty());
    }

    #[test]
    fn the_furnace_shutdown_flag_goes_up_as_it_differs() {
        let mut e = engine();
        let mut s = Sync::new("S");
        settled(&mut e);
        e.set_furnace_shutdown(true);
        assert!(s.push(&e).is_empty(), "not before the device bucket came");
        s.pull(&mut e, "device.S", &fields(json!({"is_furnace_shutdown": false})));
        settled(&mut e);
        let out = s.push(&e);
        assert_eq!(out.last(), Some(&("device.S".to_string(), fields(json!({"is_furnace_shutdown": true})))), "with the unit's other flags");
        assert!(s.push(&e).is_empty(), "sent once");
        e.set_furnace_shutdown(false);
        assert_eq!(s.push(&e), vec![("device.S".to_string(), fields(json!({"is_furnace_shutdown": false})))]);
    }

    #[test]
    fn the_range_set_on_the_dial_goes_back() {
        let mut e = engine();
        let mut s = Sync::new("S");
        s.pull(&mut e, "shared.S", &fields(json!({"target_temperature_type": "range",
            "target_temperature_low": 20.0, "target_temperature_high": 24.0})));
        settled(&mut e);
        assert!(s.push(&e).is_empty());
        for input in [Input::Wheel(1), Input::Press, Input::Release, Input::Wheel(2)] {
            e.input(input);
        }
        assert!(!s.pull(&mut e, "shared.S", &fields(json!({"target_temperature_high": 24.0}))), "an older bucket waits");
        settled(&mut e);
        assert_eq!(s.push(&e), vec![("shared.S".to_string(), fields(json!({"target_temperature_high": 25.0})))]);
    }

    #[test]
    fn an_unsent_ui_change_outlives_an_older_bucket() {
        let mut e = engine();
        let mut s = Sync::new("S");
        s.pull(&mut e, "shared.S", &fields(json!({"target_temperature": 20.0, "target_temperature_type": "heat"})));
        e.input(crate::engine::Input::Wheel(2));
        assert!(!s.pull(&mut e, "shared.S", &fields(json!({"target_temperature": 20.0}))));
        assert_eq!(e.thermostat.target, 21.0);
        settled(&mut e);
        assert_eq!(s.push(&e)[0].1["target_temperature"], json!(21.0));
    }
}
