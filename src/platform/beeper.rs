//! The piezo, through the kernel's pwm-beeper input device: an EV_SND
//! SND_TONE event starts a tone at its value in hertz, and 0 stops it.

use std::fs::File;
use std::io::Write;
use std::time::Duration;

const EV_SND: u16 = 0x12;
const SND_TONE: u16 = 0x02;

/// The stock click: 2 kHz for 3 ms.
const CLICK_HZ: i32 = 2000;
const CLICK: Duration = Duration::from_millis(3);

pub struct Beeper {
    /// Each click asked for; the beeper's thread plays them in turn.
    clicks: std::sync::mpsc::Sender<()>,
}

impl Beeper {
    /// The event device named pwm-beeper.
    pub fn open() -> Result<Beeper, String> {
        for entry in std::fs::read_dir("/sys/class/input").map_err(|e| e.to_string())?.flatten() {
            let name = std::fs::read_to_string(entry.path().join("device/name")).unwrap_or_default();
            let node = entry.file_name();
            if name.trim() == "pwm-beeper" && node.to_string_lossy().starts_with("event") {
                let path = std::path::Path::new("/dev/input").join(node);
                let mut file = File::options().write(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                info!("click: pwm-beeper at {}", crate::log::path(&path));
                // Played on a thread of their own: a click holds its tone for
                // 3 ms, which the render loop must not wait for.
                let (clicks, asked) = std::sync::mpsc::channel::<()>();
                std::thread::Builder::new().name("beeper".into()).spawn(move || {
                    while asked.recv().is_ok() {
                        trace!("click: {CLICK_HZ} Hz for {} ms", CLICK.as_millis());
                        tone(&mut file, CLICK_HZ);
                        std::thread::sleep(CLICK);
                        tone(&mut file, 0);
                    }
                }).map_err(|e| format!("beeper thread: {e}"))?;
                return Ok(Beeper { clicks });
            }
        }
        Err("no pwm-beeper input device".into())
    }

    /// One click, played as soon as the ones before it have been.
    pub fn click(&mut self) {
        let _ = self.clicks.send(());
    }
}

/// Starts a tone of `hz` on the beeper, or stops it with 0.
fn tone(file: &mut File, hz: i32) {
    // SAFETY: input_event is plain data; zeroed is a valid timestamp.
    let mut event: libc::input_event = unsafe { std::mem::zeroed() };
    (event.type_, event.code, event.value) = (EV_SND, SND_TONE, hz);
    // SAFETY: the bytes of a live input_event, as the kernel reads them.
    let bytes = unsafe {
        std::slice::from_raw_parts((&event as *const libc::input_event).cast::<u8>(), std::mem::size_of::<libc::input_event>())
    };
    if let Err(e) = file.write_all(bytes) {
        debug!("click: tone {hz} Hz: {e}");
    }
}
