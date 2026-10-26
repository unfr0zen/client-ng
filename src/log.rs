//! The client's log: a line on stderr each, `HH:MM:SS.mmm LEVEL area: what`,
//! the time the host's local time. Lines above the level in use are left
//! out: `--log LEVEL` or `CLIENT_NG_LOG` sets it, `info` unless either
//! does. On a terminal the time is grey and the level coloured, unless
//! NO_COLOR is set; the unit keeps stderr, plain, in /tmp/client-ng.log.
//!
//! `error!` is what failed and stays failed, `warn!` what failed and is
//! worked round, `info!` what changed (a screen, a setting, the system's
//! state, a link coming or going), `debug!` the steps to it (input, each
//! request and reply, decisions and their reasons) and `trace!` what
//! comes each frame or each frame of the backplate's link.
//!
//! Files are named by `path`, never in full.
//!
//! A thread writes the lines, so the render loop never waits on stderr;
//! `flush` waits for what is queued, at exit and when panicking.

use std::fmt;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error = 1,
    Warn,
    Info,
    Debug,
    Trace,
}

static LEVEL: AtomicU8 = AtomicU8::new(Level::Info as u8);

impl Level {
    pub fn parse(name: &str) -> Option<Level> {
        Some(match name.to_ascii_lowercase().as_str() {
            "error" => Level::Error,
            "warn" | "warning" => Level::Warn,
            "info" => Level::Info,
            "debug" => Level::Debug,
            "trace" => Level::Trace,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN ",
            Level::Info => "INFO ",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }

    /// The level's colour on a terminal, as an ANSI SGR code.
    fn colour(self) -> &'static str {
        match self {
            Level::Error => "1;31",
            Level::Warn => "33",
            Level::Info => "32",
            Level::Debug => "36",
            Level::Trace => "90",
        }
    }
}

/// Whether lines are coloured: stderr a terminal and NO_COLOR unset.
fn coloured() -> bool {
    static COLOURED: OnceLock<bool> = OnceLock::new();
    // SAFETY: isatty only reads the descriptor's state.
    *COLOURED.get_or_init(|| unsafe { libc::isatty(2) == 1 } && std::env::var_os("NO_COLOR").is_none())
}

/// A line as written: plain, or with the time grey and the level coloured.
fn line(time: &str, level: Level, what: fmt::Arguments, colour: bool) -> String {
    if colour {
        format!("\x1b[90m{time}\x1b[0m \x1b[{}m{}\x1b[0m {what}\n", level.colour(), level.name())
    } else {
        format!("{time} {} {what}\n", level.name())
    }
}

/// How `p` is named in the log: from the working directory when under it
/// or beside it (sharing more than the top folder), else by its name alone.
pub fn path(p: &Path) -> String {
    match std::env::current_dir() {
        Ok(cwd) => relative(p, &cwd),
        Err(_) => p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned()),
    }
}

fn relative(p: &Path, cwd: &Path) -> String {
    if p.is_relative() {
        return p.display().to_string();
    }
    let (ps, cs): (Vec<Component>, Vec<Component>) = (p.components().collect(), cwd.components().collect());
    let common = ps.iter().zip(&cs).take_while(|(a, b)| a == b).count();
    // The root and a top folder (/Users, /tmp, /nestlabs) are not enough.
    if common < 3 && common < cs.len() {
        return p.file_name().map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into_owned());
    }
    let mut out: PathBuf = std::iter::repeat_n(Component::ParentDir, cs.len() - common).collect();
    out.extend(&ps[common..]);
    if out.as_os_str().is_empty() { ".".into() } else { out.display().to_string() }
}

pub fn set(level: Level) {
    LEVEL.store(level as u8, Ordering::Relaxed);
}

/// Whether lines at `level` are written: for work done only to log.
pub fn enabled(level: Level) -> bool {
    level as u8 <= LEVEL.load(Ordering::Relaxed)
}

/// The local time of day, to the millisecond.
fn now() -> String {
    let mut tv = libc::timeval { tv_sec: 0, tv_usec: 0 };
    // SAFETY: gettimeofday and localtime_r write only into the structs given.
    let tm = unsafe {
        libc::gettimeofday(&mut tv, std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&tv.tv_sec, &mut tm);
        tm
    };
    format!("{:02}:{:02}:{:02}.{:03}", tm.tm_hour, tm.tm_min, tm.tm_sec, tv.tv_usec / 1000)
}

/// What the writing thread is sent: a line, or a request to write all
/// queued so far and say so.
enum Queued {
    Line(String),
    Flush(std::sync::mpsc::Sender<()>),
}

/// The writing thread's queue, started on the first line.
fn queue() -> &'static std::sync::Mutex<std::sync::mpsc::Sender<Queued>> {
    static QUEUE: OnceLock<std::sync::Mutex<std::sync::mpsc::Sender<Queued>>> = OnceLock::new();
    QUEUE.get_or_init(|| {
        let (sender, lines) = std::sync::mpsc::channel();
        let _ = std::thread::Builder::new().name("log".into()).spawn(move || {
            while let Ok(queued) = lines.recv() {
                let mut err = std::io::stderr().lock();
                match queued {
                    Queued::Line(line) => { let _ = err.write_all(line.as_bytes()); }
                    Queued::Flush(done) => { let _ = err.flush(); let _ = done.send(()); }
                }
            }
        });
        std::sync::Mutex::new(sender)
    })
}

/// Writes a line at `level`, whole, so lines from threads never interleave.
pub fn write(level: Level, what: fmt::Arguments) {
    if enabled(level) {
        let line = line(&now(), level, what, coloured());
        let queue = queue().lock().unwrap_or_else(|e| e.into_inner());
        if queue.send(Queued::Line(line.clone())).is_err() {
            let _ = std::io::stderr().lock().write_all(line.as_bytes());
        }
    }
}

/// Waits for every line queued so far to be written (a second at most).
pub fn flush() {
    let (done, written) = std::sync::mpsc::channel();
    let sent = queue().lock().unwrap_or_else(|e| e.into_inner()).send(Queued::Flush(done)).is_ok();
    if sent {
        let _ = written.recv_timeout(std::time::Duration::from_secs(1));
    }
}

macro_rules! error {
    ($($arg:tt)*) => { $crate::log::write($crate::log::Level::Error, format_args!($($arg)*)) };
}
macro_rules! warn {
    ($($arg:tt)*) => { $crate::log::write($crate::log::Level::Warn, format_args!($($arg)*)) };
}
macro_rules! info {
    ($($arg:tt)*) => { $crate::log::write($crate::log::Level::Info, format_args!($($arg)*)) };
}
macro_rules! debug {
    ($($arg:tt)*) => { $crate::log::write($crate::log::Level::Debug, format_args!($($arg)*)) };
}
macro_rules! trace {
    ($($arg:tt)*) => { $crate::log::write($crate::log::Level::Trace, format_args!($($arg)*)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_named_either_case_and_ordered_by_detail() {
        assert_eq!(Level::parse("DEBUG"), Some(Level::Debug));
        assert_eq!(Level::parse("warning"), Some(Level::Warn));
        assert_eq!(Level::parse("loud"), None);
        assert!(Level::Error < Level::Warn && Level::Debug < Level::Trace);
    }

    #[test]
    fn colour_wraps_the_time_and_level_and_plain_has_none() {
        let plain = line("12:00:00.000", Level::Warn, format_args!("x: y"), false);
        assert_eq!(plain, "12:00:00.000 WARN  x: y\n");
        let coloured = line("12:00:00.000", Level::Warn, format_args!("x: y"), true);
        assert_eq!(coloured, "\x1b[90m12:00:00.000\x1b[0m \x1b[33mWARN \x1b[0m x: y\n");
    }

    #[test]
    fn files_are_named_from_the_working_directory_or_by_name() {
        let cwd = Path::new("/Users/me/dev/unfr0zen/client-ng");
        assert_eq!(relative(Path::new("/Users/me/dev/unfr0zen/client-ng/skins/pixel"), cwd), "skins/pixel");
        assert_eq!(relative(Path::new("/Users/me/dev/unfr0zen/qemu/ui/backplate.py"), cwd), "../qemu/ui/backplate.py");
        assert_eq!(relative(Path::new("/opt/homebrew/lib/libfreetype.6.dylib"), cwd), "libfreetype.6.dylib");
        assert_eq!(relative(Path::new("/Users/other/settings.json"), cwd), "settings.json");
        assert_eq!(relative(cwd, cwd), ".");
        assert_eq!(relative(Path::new("skins/classic"), cwd), "skins/classic");
        let unit = Path::new("/media/data/client-ng");
        assert_eq!(relative(Path::new("/media/data/client-ng/settings.json"), unit), "settings.json");
        assert_eq!(relative(Path::new("/nestlabs/share/client-ng/skins"), unit), "skins");
    }

    #[test]
    fn the_time_of_day_has_milliseconds() {
        let t = now();
        assert_eq!(t.len(), 12, "{t}");
        assert_eq!((&t[2..3], &t[5..6], &t[8..9]), (":", ":", "."));
    }
}
