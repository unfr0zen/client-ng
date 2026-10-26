//! An HTTP server for tools (tools/desktop-mcp.py) to see the screen and
//! drive the inputs, on the paths nest-qemu's ui.py serves them:
//!
//! | | |
//! | --- | --- |
//! | `GET /screen.png` | the last frame shown, as many pixels as drawn (`--pixels`) |
//! | `GET /screen` | each frame as it is shown, server-sent events of base64 PNG |
//! | `GET /state` | the wheel's position, the backplate in use, the serial |
//! | `POST /button/press`, `/button/release` | the button |
//! | `POST /wheel/turn` | `{"delta": detents}`, clockwise positive: one wheel event; the position after. `{"notches": n}` turns it as the window's wheel does |
//! | `POST /backplate` | a JSON object for backplate.py's control (the room, the wiring); `{}` reads its state |
//! | `POST /cloud` | `{"key": {fields}}`: buckets as nestlocal sends them (`shared.SERIAL`, `device.SERIAL`) and `weather` (`temp_c`, `humidity`, `icon`, `sunrise`, `sunset`, the forecast's `hourly` list of `hour`, `time`, `temp_c`, and the place it is for: `city`, `state`, `zip`) |
//! | `POST /approach` | someone comes near, as the thermopile or the near PIR tells it (backplate.py's presence is the PIR, across the room) |
//! | `POST /menu/open` | `{"path": ["Settings", "Nest Sense"], "at": n}`: the menus opened on the submenu the labels lead to, on its entry `n` (from 0, as shown) when given |
//! | `POST /quit` | stops the client as SIGTERM does |

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Condvar, Mutex};

use super::input::Event;
use super::serial;

/// How many frames have been shown, the last (RGBA) and its pixels each way.
static FRAME: Mutex<(u64, Vec<u8>, usize)> = Mutex::new((0, Vec::new(), 0));
static SHOWN: Condvar = Condvar::new();
/// Input from requests, not yet read by the client.
static INPUTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());
/// Detents turned, clockwise positive.
static WHEEL: AtomicI64 = AtomicI64::new(0);
/// Cloud updates from requests, not yet applied; and the serial whose
/// buckets the client follows.
static CLOUD: Mutex<Vec<(String, crate::net::Fields)>> = Mutex::new(Vec::new());
static SERIAL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
/// Menus to open from requests, not yet opened: the labels and the entry.
static MENU: Mutex<Vec<(Vec<String>, Option<usize>)>> = Mutex::new(Vec::new());
/// Someone came near, from a request not yet read.
static APPROACH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_serial(serial: &str) {
    let _ = SERIAL.set(serial.to_string());
}

pub fn take_cloud() -> Vec<(String, crate::net::Fields)> {
    std::mem::take(&mut *CLOUD.lock().unwrap())
}

pub fn take_menu() -> Vec<(Vec<String>, Option<usize>)> {
    std::mem::take(&mut *MENU.lock().unwrap())
}

pub fn take_approach() -> bool {
    APPROACH.swap(false, Ordering::Relaxed)
}

/// A frame has been shown.
pub fn shown(rgba: &[u8], side: usize) {
    let mut frame = FRAME.lock().unwrap();
    frame.0 += 1;
    frame.2 = side;
    frame.1.clear();
    frame.1.extend_from_slice(rgba);
    SHOWN.notify_all();
}

pub fn take_inputs() -> Vec<Event> {
    std::mem::take(&mut *INPUTS.lock().unwrap())
}

/// The ring turned, from the window or a request.
pub fn turned(detents: i32) {
    WHEEL.fetch_add(i64::from(detents), Ordering::Relaxed);
}

/// Serves `addr` (HOST:PORT) from a thread of its own.
pub fn start(addr: &str) -> Result<(), String> {
    let listener = TcpListener::bind(addr).map_err(|e| format!("control: {addr}: {e}"))?;
    info!("control: http://{}", listener.local_addr().map_err(|e| e.to_string())?);
    std::thread::Builder::new().name("control".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = std::thread::Builder::new().name("control-request".into()).spawn(move || {
                if let Err(e) = serve(stream) {
                    warn!("control: {e}");
                }
            });
        }
    }).map_err(|e| format!("control: {e}"))?;
    Ok(())
}

fn serve(stream: TcpStream) -> std::io::Result<()> {
    let started = std::time::Instant::now();
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or_default().to_string(), parts.next().unwrap_or_default().to_string());
    let mut length = 0;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 || line.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    if !body.is_empty() {
        trace!("control: {method} {path} sends {}", String::from_utf8_lossy(&body[..body.len().min(512)]));
    }
    let mut out = stream;
    let json = |v: serde_json::Value| (200, "application/json", v.to_string().into_bytes());
    let (status, kind, reply) = match (method.as_str(), path.as_str()) {
        ("GET", "/screen.png") => match png() {
            Some(png) => (200, "image/png", png),
            None => (503, "text/plain", b"no frame yet\n".to_vec()),
        },
        ("GET", "/screen") => {
            debug!("control: GET /screen: streaming frames");
            // It ends only when the reader goes, which is not a failure.
            let ended = stream_frames(out).err().map_or(String::new(), |e| format!(": {e}"));
            debug!("control: GET /screen: stream ended after {} s{ended}", started.elapsed().as_secs());
            return Ok(());
        }
        ("GET", "/state") => json(serde_json::json!({
            "name": "client-ng (desktop)",
            "linked": true,
            "wheel": WHEEL.load(Ordering::Relaxed),
            "backplate": serial::CONTROL.get().map_or("stock's simulator", |_| "backplate.py"),
            "serial": SERIAL.get(),
        })),
        ("GET", "/wheel") => json(serde_json::json!({ "position": WHEEL.load(Ordering::Relaxed) })),
        ("POST", "/button/press") => input(Event::Press),
        ("POST", "/button/release") => input(Event::Release),
        ("POST", "/wheel/turn") => {
            let want = serde_json::from_slice::<serde_json::Value>(&body).unwrap_or_default();
            let turn = match (want["delta"].as_i64(), want["notches"].as_i64()) {
                (Some(d), _) => Some(Event::Turn(d as i32)),
                (_, Some(n)) => Some(Event::Notch(n as i32)),
                _ => None,
            };
            match turn {
                Some(turn) => {
                    INPUTS.lock().unwrap().push(turn);
                    json(serde_json::json!({ "position": WHEEL.load(Ordering::Relaxed) }))
                }
                None => (400, "text/plain", b"bad delta\n".to_vec()),
            }
        }
        ("POST", "/backplate") => match backplate(&body) {
            Ok(state) => (200, "application/json", state),
            Err(e) => (503, "text/plain", format!("{e}\n").into_bytes()),
        },
        ("POST", "/cloud") => match serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(&body) {
            Ok(updates) if updates.values().all(serde_json::Value::is_object) => {
                let mut cloud = CLOUD.lock().unwrap();
                for (key, fields) in updates {
                    if let serde_json::Value::Object(f) = fields {
                        cloud.push((key, f));
                    }
                }
                (204, "text/plain", Vec::new())
            }
            _ => (400, "text/plain", b"a JSON object of objects\n".to_vec()),
        },
        ("POST", "/approach") => {
            APPROACH.store(true, Ordering::Relaxed);
            (204, "text/plain", Vec::new())
        }
        ("POST", "/menu/open") => {
            let want = serde_json::from_slice::<serde_json::Value>(&body).unwrap_or_default();
            let path: Option<Vec<String>> = want["path"].as_array().and_then(|p| p.iter().map(|l| l.as_str().map(str::to_string)).collect());
            match path {
                Some(path) if !path.is_empty() => {
                    MENU.lock().unwrap().push((path, want["at"].as_u64().map(|n| n as usize)));
                    (204, "text/plain", Vec::new())
                }
                _ => (400, "text/plain", b"a path of labels\n".to_vec()),
            }
        }
        ("POST", "/quit") => {
            // SAFETY: raising a signal whose handler only stores to an atomic.
            unsafe { libc::raise(libc::SIGTERM) };
            (204, "text/plain", Vec::new())
        }
        _ => (404, "text/plain", b"not found\n".to_vec()),
    };
    debug!("control: {method} {path}: {status}, {} bytes in, {} out, in {} ms", body.len(), reply.len(), started.elapsed().as_millis());
    write!(out, "HTTP/1.1 {status} {}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
           if status < 300 { "OK" } else { "Error" }, reply.len())?;
    out.write_all(&reply)
}

fn input(event: Event) -> (u16, &'static str, Vec<u8>) {
    INPUTS.lock().unwrap().push(event);
    (204, "text/plain", Vec::new())
}

/// An RGBA frame `side` pixels each way as an RGB PNG.
fn encode(rgba: &[u8], side: usize) -> Vec<u8> {
    use image::ImageEncoder;
    let rgb: Vec<u8> = rgba.as_chunks::<4>().0.iter().flat_map(|c| [c[0], c[1], c[2]]).collect();
    let mut png = Vec::new();
    let _ = image::codecs::png::PngEncoder::new(&mut png).write_image(&rgb, side as u32, side as u32, image::ExtendedColorType::Rgb8);
    png
}

fn png() -> Option<Vec<u8>> {
    let frame = FRAME.lock().unwrap();
    (frame.0 > 0).then(|| encode(&frame.1, frame.2))
}

/// Each frame shown, until the reader goes.
fn stream_frames(mut out: TcpStream) -> std::io::Result<()> {
    out.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n")?;
    let mut seen = 0;
    loop {
        let (rgba, side) = {
            let frame = SHOWN.wait_while(FRAME.lock().unwrap(), |f| f.0 == seen).unwrap();
            seen = frame.0;
            (frame.1.clone(), frame.2)
        };
        out.write_all(format!("data: {}\n\n", base64(&encode(&rgba, side))).as_bytes())?;
    }
}

/// One request to backplate.py's control port: a JSON object a line, the
/// state a line back.
fn backplate(want: &[u8]) -> Result<Vec<u8>, String> {
    let port = serial::CONTROL.get().ok_or("no backplate.py: stock's simulator stands in, without a control")?;
    let mut s = TcpStream::connect(("127.0.0.1", *port)).map_err(|e| format!("backplate.py: {e}"))?;
    let want = if want.iter().all(u8::is_ascii_whitespace) { b"{}".as_slice() } else { want };
    s.write_all(&[want, b"\n"].concat()).map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).map_err(|e| e.to_string())?;
    Ok(line.trim_end().as_bytes().to_vec())
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | u32::from(b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() { ABC[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_pads_as_the_standard_alphabet_does() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foo"), "Zm9v");
        assert_eq!(super::base64(b"foobar\xff"), "Zm9vYmFy/w==");
    }
}
