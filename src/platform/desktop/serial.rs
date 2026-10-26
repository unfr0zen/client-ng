//! The backplate's UART on the desktop: nest-qemu's backplate.py, the
//! backplate nlclient is traced against, speaking the frames over TCP. The
//! client listens and starts backplate.py, which connects as it does to
//! QEMU's UART and takes changes to the room and the wiring on its
//! control port (`control`'s `/backplate`). backplate.py is found as
//! tools/nest.py finds nest-qemu: at `$NEST_QEMU`, else beside this
//! repository as `nest-qemu` or `qemu`.

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::fd::{AsRawFd, RawFd};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// backplate.py's control port, once it runs.
pub static CONTROL: OnceLock<u16> = OnceLock::new();

/// How long backplate.py has to connect.
const CONNECT: Duration = Duration::from_secs(5);

fn backplate_py() -> Option<PathBuf> {
    let repo = crate::platform::checkout();
    let roots = std::env::var_os("NEST_QEMU").map(PathBuf::from).into_iter()
        .chain(["nest-qemu", "qemu"].map(|n| repo.join("..").join(n)));
    roots.map(|r| r.join("ui/backplate.py")).find(|p| p.is_file())
}

/// backplate.py's lines into the log, as the simulator's: its frames at
/// trace, the rest (circuits switched, which the client logs itself) at
/// debug.
fn relay(from: impl std::io::Read + Send + 'static) {
    std::thread::spawn(move || {
        for line in std::io::BufRead::lines(std::io::BufReader::new(from)).map_while(Result::ok) {
            let line = line.trim_end();
            let what = line.strip_prefix("backplate: ").unwrap_or(line);
            if ["->", "<-", "raw:"].iter().any(|p| what.starts_with(p)) {
                trace!("backplate.py: {what}");
            } else if !what.is_empty() {
                debug!("backplate.py: {what}");
            }
        }
    });
}

/// A free port on the loopback.
fn free_port() -> Result<u16, String> {
    TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map(|a| a.port()).map_err(|e| e.to_string())
}

pub struct Port {
    listener: TcpListener,
    /// backplate.py's connection; until it connects again after dropping
    /// it, the listener is polled instead.
    stream: Option<TcpStream>,
}

impl Port {
    /// Starts backplate.py and takes its connection. `_path` and
    /// `_flow_control` are the unit's UART's.
    pub fn open(_path: &str, _flow_control: bool) -> Result<Port, String> {
        let script = backplate_py().ok_or("no backplate.py: set NEST_QEMU to nest-qemu's checkout, or clone it beside this repository")?;
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("listen: {e}"))?;
        let serial = listener.local_addr().map_err(|e| e.to_string())?.port();
        let control = free_port()?;
        // --parent: it goes when the client does. Its frames are traced
        // only at trace, as the client's own are.
        let mut child = std::process::Command::new("python3").arg(&script)
            .args(["--serial", &format!("tcp:127.0.0.1:{serial}"), "--control", &format!("tcp:127.0.0.1:{control}"),
                   "--parent", &std::process::id().to_string()])
            .args(crate::log::enabled(crate::log::Level::Trace).then_some("--debug"))
            .stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped())
            .spawn().map_err(|e| format!("python3 {}: {e}", crate::log::path(&script)))?;
        if let Some(out) = child.stdout.take() {
            relay(out);
        }
        if let Some(err) = child.stderr.take() {
            relay(err);
        }
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let started = Instant::now();
        let stream = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == ErrorKind::WouldBlock && started.elapsed() < CONNECT => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => return Err(format!("{}: did not connect: {e}", crate::log::path(&script))),
            }
        };
        stream.set_nonblocking(false).map_err(|e| e.to_string())?;
        let _ = stream.set_nodelay(true);
        let _ = CONTROL.set(control);
        info!("backplate: {} on port {serial}, its control on {control}", crate::log::path(&script));
        Ok(Port { listener, stream: Some(stream) })
    }

    pub fn fd(&self) -> RawFd {
        self.stream.as_ref().map_or(self.listener.as_raw_fd(), AsRawFd::as_raw_fd)
    }

    /// One read, once `poll` says bytes wait; with the connection gone,
    /// takes the next one.
    pub fn read(&mut self) -> Vec<u8> {
        let Some(stream) = &mut self.stream else {
            if let Ok((s, _)) = self.listener.accept() {
                let _ = s.set_nonblocking(false);
                let _ = s.set_nodelay(true);
                info!("backplate: backplate.py connected again");
                self.stream = Some(s);
            }
            return Vec::new();
        };
        let mut buf = [0u8; 2048];
        match stream.read(&mut buf) {
            Ok(n) if n > 0 => buf[..n].to_vec(),
            Err(e) if e.kind() == ErrorKind::Interrupted => Vec::new(),
            _ => {
                warn!("backplate: backplate.py hung up");
                self.stream = None;
                Vec::new()
            }
        }
    }

    pub fn unread(&self) -> usize {
        let Some(stream) = &self.stream else { return 0 };
        let mut n: libc::c_int = 0;
        // SAFETY: FIONREAD writes one int; the socket is open.
        unsafe { libc::ioctl(stream.as_raw_fd(), libc::FIONREAD, &mut n) };
        usize::try_from(n).unwrap_or(0)
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        match &mut self.stream {
            Some(s) => s.write_all(bytes).map_err(|e| format!("backplate: write failed: {e}")),
            None => Err("backplate: write failed, backplate.py is not connected".into()),
        }
    }

    /// TCP has nothing to drain.
    pub fn drain(&mut self) {}

    /// Discards what has arrived and not been read.
    pub fn discard(&mut self) {
        let Some(s) = &mut self.stream else { return };
        let _ = s.set_nonblocking(true);
        let mut buf = [0u8; 2048];
        while matches!(s.read(&mut buf), Ok(n) if n > 0) {}
        let _ = s.set_nonblocking(false);
    }

    /// TCP carries no break: backplate.py resets on the reset request that
    /// follows it.
    pub fn send_break(&mut self) {}
}
