//! The backplate's UART and GPIOs, set up as stock nlclient does (from its
//! product.config and its serial code): `/dev/ttysensormcu0` opened for
//! blocking reads and writes, 115200 8N1 raw on fresh settings, RTS/CTS
//! flow control when `backplateflowcontrol` is 1, VMIN 1 and VTIME 1; a
//! break of `tcsendbreak(fd, 10)` before each reset request; `detect#` and
//! `reset` under product.config's `backplatedevice`, else the unit's.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::path::{Path, PathBuf};

use crate::backplate::frame::Decoder;
use crate::backplate::update::Step;
use crate::backplate::link::{kind, Identity, Link, Out, Report};
use crate::backplate::sim::{self, Settings, Sim};
use crate::hvac::wiring::Switch;

/// Stock nlclient's configuration, where its simulator's settings live.
pub const CLIENT_CONFIG: &str = "/nestlabs/etc/client.config";
/// The unit's product settings, where the UART's flow control is set.
pub const PRODUCT_CONFIG: &str = "/nestlabs/etc/product.config";

/// A setting's value in one of stock's `<a key="…" value="…"/>` files.
fn setting(config: &str, key: &str) -> Option<String> {
    config.split("<a ").skip(1).find_map(|tag| {
        let get = |name: &str| tag.split(&format!("{name}=\"")).nth(1).and_then(|r| r.split('"').next());
        (get("key") == Some(key)).then(|| get("value")).flatten().map(str::to_string)
    })
}

#[cfg(feature = "desktop")]
use super::desktop::serial::Port;

#[cfg(not(feature = "desktop"))]
pub struct Port {
    tty: File,
}

#[cfg(not(feature = "desktop"))]
impl Port {
    /// Opens the UART as stock does: plain O_RDWR, and settings built from
    /// zero rather than changed from the current ones.
    pub fn open(path: &str, flow_control: bool) -> Result<Port, String> {
        let tty = std::fs::OpenOptions::new().read(true).write(true).open(path).map_err(|e| format!("{}: {e}", crate::log::path(Path::new(path))))?;
        let fd = tty.as_raw_fd();
        // SAFETY: termios is plain data, valid when zeroed; fd is open.
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            t.c_cflag = libc::B115200 | libc::CS8 | libc::CREAD | libc::CLOCAL;
            if flow_control {
                t.c_cflag |= libc::CRTSCTS;
            }
            t.c_cc[libc::VMIN] = 1;
            t.c_cc[libc::VTIME] = 1;
            libc::tcflush(fd, libc::TCIFLUSH);
            if libc::tcsetattr(fd, libc::TCSANOW, &t) != 0 {
                return Err(format!("{}: tcsetattr: {}", crate::log::path(Path::new(path)), std::io::Error::last_os_error()));
            }
        }
        Ok(Port { tty })
    }

    pub fn fd(&self) -> RawFd {
        self.tty.as_raw_fd()
    }

    /// One read, once `poll` says bytes wait: VTIME ends it 100 ms after
    /// the last byte.
    pub fn read(&mut self) -> Vec<u8> {
        let mut buf = [0u8; 2048];
        match self.tty.read(&mut buf) {
            Ok(n) => buf[..n].to_vec(),
            Err(_) => Vec::new(),
        }
    }

    /// Bytes received and not yet read.
    pub fn unread(&self) -> usize {
        let mut n: libc::c_int = 0;
        // SAFETY: FIONREAD writes one int; the fd is open.
        unsafe { libc::ioctl(self.tty.as_raw_fd(), libc::FIONREAD, &mut n) };
        usize::try_from(n).unwrap_or(0)
    }

    /// Writes a whole frame, blocking until the driver takes it.
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        let mut done = 0;
        while done < bytes.len() {
            match self.tty.write(&bytes[done..]) {
                Ok(0) => break,
                Ok(n) => done += n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        if done < bytes.len() {
            return Err(format!("backplate: write failed, wrote {done} of {}", bytes.len()));
        }
        Ok(())
    }

    /// Waits until what was written has gone out.
    pub fn drain(&mut self) {
        // SAFETY: the fd is open for the Port's lifetime.
        unsafe { libc::tcdrain(self.tty.as_raw_fd()) };
    }

    /// Discards what is waiting either way, as stock does before a reset.
    pub fn discard(&mut self) {
        // SAFETY: the fd is open for the Port's lifetime.
        unsafe { libc::tcflush(self.tty.as_raw_fd(), libc::TCIOFLUSH) };
    }

    pub fn send_break(&mut self) {
        // SAFETY: the fd is open for the Port's lifetime.
        unsafe { libc::tcsendbreak(self.tty.as_raw_fd(), 10) };
    }
}

/// Whether the detect line says a backplate is attached: `None` when the
/// line is absent.
pub fn detected(gpios: &Path) -> Option<bool> {
    std::fs::read_to_string(gpios.join("detect#/value")).ok().map(|v| v.trim() == "1")
}

/// The unit's battery, in volts.
fn battery() -> f32 {
    super::Device::current().battery()
}

/// Restarts the backplate with its reset line: high for 20 ms.
fn pulse_reset(gpios: &Path) {
    let line = gpios.join("reset/value");
    if std::fs::write(&line, "1").is_err() {
        error!("backplate: failed to set the reset line to 1 ({})", crate::log::path(&line));
    }
    std::thread::sleep(std::time::Duration::from_millis(20));
    if std::fs::write(&line, "0").is_err() {
        error!("backplate: failed to set the reset line back to 0 ({})", crate::log::path(&line));
    }
}

/// How often the detect line is read, and how long a change must hold.
const DETECT_READ: f64 = 0.25;
const DEBOUNCE: f64 = 0.75;

/// What the UI asks of the backplate.
enum Command {
    Switches(Vec<Switch>, [Option<u8>; 19], bool, bool),
    PowerSteal(u8),
    UserInput,
    Quiet(crate::backplate::link::Wake),
    Woke,
    TempComp(bool),
    I2cAllowed(bool),
    WakeOnApproach(bool),
    Safety(Option<[u8; 16]>),
}

/// The backplate the client drives, on a thread of its own as stock's
/// controller is: the unit's own, and stock's simulator when its settings
/// ask for it, in place of the backplate or over it.
pub struct Backplate {
    commands: std::sync::mpsc::Sender<Command>,
    reports: std::sync::mpsc::Receiver<Report>,
    /// Readable when reports wait.
    wake: File,
    simulated: bool,
}

impl Backplate {
    /// The unit's backplate when its UART opens, attached or not (the
    /// detect line is followed from then on), or stock's simulator in its
    /// place under `backplatesimulationrun` or `simulate`. Without either,
    /// stock carries on with no backplate. Its UART is `serial`.
    pub fn open(serial: &str, simulate: bool) -> Result<Backplate, String> {
        let product = std::fs::read_to_string(PRODUCT_CONFIG).unwrap_or_default();
        let client = std::fs::read_to_string(CLIENT_CONFIG).unwrap_or_default();
        let mut settings = Settings::parse(&format!("{product}{client}"));
        settings.run |= simulate;
        let port = if settings.run {
            None
        } else {
            let flow_control = setting(&product, "backplateflowcontrol").is_some_and(|v| v.trim() == "1");
            Port::open(serial, flow_control).map_err(|e| warn!("backplate: {e}; backplate not connected")).ok()
        };
        // The desktop has no backplate of its own: without backplate.py,
        // stock's simulator stands in.
        #[cfg(feature = "desktop")]
        {
            settings.run |= port.is_none();
        }
        let simulated = settings.run;
        if let Some(t) = settings.hvac_type {
            info!("backplate simulator: hvac type {t}");
        }
        if simulated {
            info!("backplate: stock's simulator runs in its place{}", settings.model().map_or(String::new(), |m| format!(", as {m}")));
        } else if settings.temp_and_hum {
            info!("backplate: the simulator supplies the room");
        }
        #[cfg(not(feature = "desktop"))]
        let mut fds = [0; 2];
        // SAFETY: pipe2 fills both descriptors, which the Files then own.
        #[cfg(not(feature = "desktop"))]
        let (wake, ring) = unsafe {
            if libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) != 0 {
                return Err(format!("backplate: pipe: {}", std::io::Error::last_os_error()));
            }
            use std::os::fd::FromRawFd;
            (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1]))
        };
        // macOS has no pipe2: a socket pair, non-blocking.
        #[cfg(feature = "desktop")]
        let (wake, ring) = {
            let (wake, ring) = std::os::unix::net::UnixStream::pair().map_err(|e| format!("backplate: pipe: {e}"))?;
            wake.set_nonblocking(true).and_then(|()| ring.set_nonblocking(true)).map_err(|e| format!("backplate: pipe: {e}"))?;
            (File::from(std::os::fd::OwnedFd::from(wake)), File::from(std::os::fd::OwnedFd::from(ring)))
        };
        let (commands, inbox) = std::sync::mpsc::channel();
        let (outbox, reports) = std::sync::mpsc::channel();
        std::thread::Builder::new().name("backplate".into())
            .spawn(move || run(port, settings, inbox, outbox, ring)).map_err(|e| format!("backplate: thread: {e}"))?;
        Ok(Backplate { commands, reports, wake, simulated })
    }

    pub fn simulated(&self) -> bool {
        self.simulated
    }

    /// Readable when the backplate has reported something.
    pub fn fd(&self) -> RawFd {
        self.wake.as_raw_fd()
    }

    /// What the backplate reported since the last call.
    pub fn reports(&mut self) -> Vec<Report> {
        let mut drained = [0u8; 64];
        while matches!(self.wake.read(&mut drained), Ok(n) if n > 0) {}
        self.reports.try_iter().collect()
    }

    /// The user is at the unit: stock tells the backplate on input.
    pub fn user_input(&self) {
        let _ = self.commands.send(Command::UserInput);
    }

    /// The unit is about to sleep: what should wake it.
    pub fn quiet(&self, wake: crate::backplate::link::Wake) {
        let _ = self.commands.send(Command::Quiet(wake));
    }

    /// Sunblock's temperature compensation on or off.
    pub fn temp_comp(&self, on: bool) {
        let _ = self.commands.send(Command::TempComp(on));
    }

    /// The unit woke from a sleep.
    pub fn woke(&self) {
        let _ = self.commands.send(Command::Woke);
    }

    /// `power_steal_mode`, as stock numbers it.
    /// The backplate's own safety temperatures (0x89), or none.
    pub fn safety(&self, payload: Option<[u8; 16]>) {
        let _ = self.commands.send(Command::Safety(payload));
    }

    /// Whether the unit wakes as someone approaches: the thermopile may
    /// wake it then (it must also have one).
    pub fn wake_on_approach(&self, on: bool) {
        let _ = self.commands.send(Command::WakeOnApproach(on));
    }

    /// Whether the sensors' alert may be raised, as kept from before.
    pub fn i2c_allowed(&self, allowed: bool) {
        let _ = self.commands.send(Command::I2cAllowed(allowed));
    }

    pub fn power_steal(&self, mode: u8) {
        let _ = self.commands.send(Command::PowerSteal(mode));
    }

    /// The HVAC control's switches on, and each one's circuit.
    pub fn switches(&self, on: &[Switch], circuits: [Option<u8>; 19], dehumidifier_reversed: bool, ptac: bool) {
        let _ = self.commands.send(Command::Switches(on.to_vec(), circuits, dehumidifier_reversed, ptac));
    }
}

/// The unit's backplate: its UART, the link, and the detect line.
struct Unit {
    port: Port,
    link: Link,
    decoder: Decoder,
    /// The detect line as last acted on, when it was last read, and when
    /// it was first seen to differ.
    attached: Option<bool>,
    read_detect: f64,
    changed: Option<f64>,
    /// The detect and reset lines' device.
    gpios: PathBuf,
    /// The last write failed: said once until one succeeds.
    write_failed: bool,
}

impl Unit {
    /// Reads what arrived, follows the detect line, and runs the link's
    /// turn; `exiting` sends stock's reset on exit.
    fn turn(&mut self, readable: bool, t: f64, exiting: bool) {
        let bytes = if readable { self.port.read() } else { Vec::new() };
        let frames = self.decoder.push(&bytes);
        let corrupt = self.decoder.take_corrupt();
        // Stock drops what arrives while the detect line says detached.
        if self.attached != Some(false) {
            if !bytes.is_empty() {
                self.link.read(t);
            }
            for f in frames {
                self.link.receive(&f, t);
            }
            for kind in corrupt {
                self.link.corrupt(kind, battery);
            }
        }
        // Stock acts on a change of the detect line once it has held for
        // 750 ms.
        if t - self.read_detect >= DETECT_READ {
            self.read_detect = t;
            let line = detected(&self.gpios);
            match self.changed {
                _ if line == self.attached => self.changed = None,
                Some(since) if t - since >= DEBOUNCE => {
                    self.changed = None;
                    self.attached = line;
                    if let Some(a) = line {
                        info!("backplate: {}", if a { "attached" } else { "detached" });
                        self.link.detected(a, battery);
                    }
                }
                Some(_) => {}
                None => self.changed = Some(t),
            }
        }
        if exiting {
            self.link.exit();
        }
        self.link.poll(t, self.port.unread(), battery);
        for o in self.link.take_out() {
            match o {
                Out::Break => {
                    trace!("backplate: tx break");
                    self.port.discard();
                    self.decoder.clear();
                    self.port.send_break();
                }
                Out::Send(f) => {
                    if crate::log::enabled(crate::log::Level::Trace) {
                        trace!("backplate: tx {}", crate::backplate::link::frame(&f));
                    }
                    match self.port.write(&f.encode()) {
                        Err(e) if !self.write_failed => {
                            warn!("{e}");
                            self.write_failed = true;
                        }
                        Err(e) => trace!("{e}"),
                        Ok(()) if self.write_failed => {
                            info!("backplate: writing again");
                            self.write_failed = false;
                        }
                        Ok(()) => {}
                    }
                    // Stock waits for these to leave before going on.
                    if matches!(f.kind, kind::QUIET | kind::GET_ERROR | kind::GET_OC_ERRORS | kind::GET_TP_ERRORS) {
                        self.port.drain();
                    }
                }
                Out::HardReset => {
                    trace!("backplate: tx reset pulse");
                    self.port.discard();
                    self.decoder.clear();
                    pulse_reset(&self.gpios);
                }
            }
        }
    }
}

/// The backplate thread: the unit's backplate and stock's simulator, and
/// the clock.
fn run(port: Option<Port>, settings: Settings, inbox: std::sync::mpsc::Receiver<Command>,
       outbox: std::sync::mpsc::Sender<Report>, mut ring: File) {
    let start = std::time::Instant::now();
    let now = || start.elapsed().as_secs_f64();
    sim::seed();
    let wiring = settings.wiring();
    // The thresholds come from client.config, else product.config (where
    // the unit keeps them), else stock's 100.
    let configs = [CLIENT_CONFIG, PRODUCT_CONFIG].map(|f| std::fs::read_to_string(f).unwrap_or_default());
    let threshold = |key: &str, default: i16| configs.iter().find_map(|c| setting(c, key)).and_then(|v| v.trim().parse::<i16>().ok())
        .filter(|v| *v != 0).unwrap_or(default);
    let tp_max = (threshold("thermopileleftrightmaxthreshold", 100), threshold("thermopileupdownmaxthreshold", 100));
    let tp_min = (threshold("thermopileleftrightminthreshold", -44), threshold("thermopileupdownminthreshold", -69));
    let has = |key: &str| configs.iter().find_map(|c| setting(c, key)).is_some_and(|v| v.trim() == "1");
    let has_tp = has("hasthermopilesensor");
    // The near PIR's threshold (`nearpirthreshold`), on a unit with one.
    let near_pir = has("hasnearpirsensor").then(|| threshold("nearpirthreshold", 15).max(0) as u16);
    let device = super::Device::current();
    let gpios = setting(&configs[1], "backplatedevice").map(|v| PathBuf::from(v.trim()))
        .unwrap_or_else(|| PathBuf::from(device.backplate_gpios()));
    let mut unit = port.map(|port| {
        let attached = detected(&gpios);
        info!("backplate: uart open, detect line {}", match attached { Some(true) => "attached", Some(false) => "detached", None => "absent" });
        let link = Link::new(now(), attached != Some(false)).simulating(settings.temp_and_hum, wiring.is_some())
            .thermopile(tp_max, tp_min, has_tp).profile(device.backplate_profile(), near_pir);
        Unit { port, link, decoder: Decoder::default(), attached, read_detect: 0.0, changed: None, gpios, write_failed: false }
    });
    // Stock's automatic backplate update and its firmware plist, both
    // named in client.config.
    let auto = setting(&configs[0], "autobackplateupdate").is_some_and(|v| v.trim() == "1");
    let plist = setting(&configs[0], "backplateupdatefile").map(|v| v.trim().to_string());
    let mut updater = auto.then(|| {
        let images = plist.as_deref().and_then(|p| std::fs::read_to_string(p).map_err(|e| error!("backplate: source property list read failure: {}: {e}", crate::log::path(Path::new(p)))).ok())
            .map(|xml| crate::backplate::update::images(&xml)).unwrap_or_default();
        debug!("backplate: automatic update on, {} images in {}", images.len(), plist.as_deref().map_or("no plist".into(), |p| crate::log::path(Path::new(p))));
        crate::backplate::update::Updater::new(images)
    }).filter(|_| unit.is_some());
    let mut held: Option<(f64, u8, Vec<Vec<u8>>)> = None;
    let mut battery_low = false;
    let mut reports = Vec::new();
    // What stock's simulator puts in place at start: the wiring, and in
    // place of the backplate its model and the room.
    if let Some((wires, pins)) = wiring {
        reports.extend([Report::Wires(wires), Report::Pins(pins)]);
    }
    let mut simulator = settings.run.then(|| Sim::new(settings.clone(), now()));
    if let Some(s) = &simulator {
        let model = settings.model().unwrap_or_else(|| "unknown".into());
        reports.push(Report::Ready(Identity { model, ..Identity::default() }));
        let (celsius, humidity) = s.room();
        reports.push(Report::Climate { celsius, humidity: (humidity != 0.0).then_some(humidity) });
    }
    // The client has gone: one last turn sends stock's reset on exit.
    let mut exiting = false;
    // When TemperatureLock last went out: at most once a second.
    let mut locked = -10.0;
    loop {
        let fd = unit.as_ref().map_or(-1, |u| u.port.fd());
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        // SAFETY: pfd lives for the call; a negative fd is ignored by poll.
        unsafe { libc::poll(&mut pfd, 1, 50) };
        let t = now();
        loop {
            let command = match inbox.try_recv() {
                Ok(c) => c,
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    exiting = true;
                    break;
                }
            };
            match command {
                Command::Switches(on, circuits, reversed, ptac) => {
                    if let Some(s) = &mut simulator {
                        s.set_equipment(&on);
                    }
                    if let Some(u) = &mut unit {
                        u.link.set_ptac(ptac);
                        u.link.switches(&on, circuits, reversed);
                    }
                }
                Command::UserInput if t - locked < 1.0 => {}
                command => {
                    let Some(u) = &mut unit else { continue };
                    match command {
                        Command::UserInput => {
                            locked = t;
                            u.link.temperature_lock();
                        }
                        Command::Quiet(wake) => u.link.quiet(wake),
                        Command::PowerSteal(mode) => u.link.power_steal(mode),
                        Command::Woke => u.link.woke(),
                        Command::TempComp(on) => u.link.temp_comp(on),
                        Command::I2cAllowed(a) => u.link.set_i2c_allowed(a),
                        Command::WakeOnApproach(on) => u.link.thermopile_wake(on && has_tp),
                        Command::Safety(p) => u.link.safety(p),
                        Command::Switches(..) => {}
                    }
                }
            }
        }
        if let Some(u) = &mut unit {
            u.turn(pfd.revents & libc::POLLIN != 0, t, exiting);
            if let Some((at, kind, records)) = held.take() {
                let volts = battery();
                if t < at {
                    held = Some((at, kind, records));
                } else if volts > 3.6 || volts == 0.0 {
                    battery_low = false;
                    reports.push(Report::Updating);
                    u.link.update(kind, records);
                } else {
                    // Said once a held update, then at debug.
                    if std::mem::replace(&mut battery_low, true) {
                        debug!("backplate: battery at {volts:.2} V, still too low for a backplate update; trying again in 30 s");
                    } else {
                        warn!("backplate: battery at {volts:.2} V, too low for a backplate update; trying again in 30 s");
                    }
                    held = Some((t + 30.0, kind, records));
                }
            }
            for r in u.link.take_reports() {
                // Each description: a firmware update to start, check or
                // go on with (stock's 0x306348), when client.config allows.
                if let (Report::Ready(id), Some(updater)) = (&r, updater.as_mut()) {
                    match updater.described(id) {
                        Step::Send { records, .. } if records.is_empty() => error!("backplate: srec source not present"),
                        // Held until the battery allows, tried every 30 s.
                        Step::Send { kind, records } => held = Some((t, kind, records)),
                        Step::Done(ok) => reports.push(Report::UpdateDone(ok)),
                        Step::Nothing => {}
                    }
                }
                // A simulated room stands in for the backplate's, which
                // stock leaves at the sensor's start values without a
                // simulation running.
                let r = match (r, &simulator) {
                    (Report::Climate { .. }, Some(s)) if settings.temp_and_hum => {
                        let (celsius, humidity) = s.room();
                        Report::Climate { celsius, humidity: (humidity != 0.0).then_some(humidity) }
                    }
                    (r, _) => r,
                };
                reports.push(r);
            }
        }
        if let Some(s) = &mut simulator {
            if s.poll(t) {
                let (celsius, humidity) = s.room();
                reports.push(Report::Climate { celsius, humidity: (humidity != 0.0).then_some(humidity) });
            }
        }
        if exiting {
            return;
        }
        if !reports.is_empty() {
            for r in reports.drain(..) {
                if outbox.send(r).is_err() {
                    return;
                }
            }
            let _ = ring.write(&[1]);
        }
    }
}
