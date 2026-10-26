//! The wheel and the button, from evdev.
//!
//! The wheel reports REL_DIAL counts (the gen 3's `2280000.adc` device),
//! about 120 to a detent and negative clockwise; they are gathered into
//! steps of `step` counts (two detents by default), clockwise positive,
//! each counted once its count passes halfway, so the sensor's small
//! variations neither drop nor add one. The button is KEY_ENTER (`gpio-keys`). Every
//! event device is read, so a keyboard's Enter works too while developing.
//! Another unit's ring and button (platform::Device) are read the same way.
//!
//! A thread of its own reads the devices, as stock's rotary thread does, so
//! an event is taken and timed as it comes, however long a frame takes:
//! the wheel's physics run on the events' own times.

use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::time::Instant;

use crate::engine::Input;

const EV_KEY: u16 = 0x01;
const EV_REL: u16 = 0x02;
const KEY_ENTER: u16 = 28;

pub struct Devices {
    /// Events as the reading thread took them, with when.
    events: std::sync::mpsc::Receiver<(libc::input_event, Instant)>,
    /// Readable when events wait.
    wake: File,
    /// Dial counts to a detent; negative when counts run counterclockwise.
    step: i32,
    /// Counts not yet a whole detent.
    dial: i32,
    /// The unit's ring axis and the sign making its counts the gen 3's,
    /// and its button.
    ring: (u16, i32, i32),
    button: u16,
    /// Each wheel event's counts (the gen 3's), past the hysteresis, not
    /// yet taken, with when it came; and the hysteresis: the turn's
    /// direction and the counts held back since it reversed.
    counts: Vec<(i32, Instant)>,
    turning: (i32, i32),
}

/// Counts a reversal of the wheel swallows, as stock's rotary thread does
/// (rotaryhysteresis, bin 0x20faf8).
const HYSTERESIS: i32 = 20;

impl Devices {
    pub fn open(step: i32, device: crate::platform::Device) -> Devices {
        let mut files = Vec::new();
        let mut entries: Vec<_> = std::fs::read_dir("/dev/input").into_iter().flatten().flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("event")))
            .collect();
        entries.sort();
        for path in entries {
            match OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(&path) {
                Ok(file) => {
                    if crate::log::enabled(crate::log::Level::Debug) {
                        let node = path.file_name().unwrap_or_default();
                        let name = std::fs::read_to_string(std::path::Path::new("/sys/class/input").join(node).join("device/name"))
                            .unwrap_or_default();
                        debug!("input: {} ({}) opened", crate::log::path(&path), name.trim());
                    }
                    files.push(file);
                }
                Err(e) => warn!("input: {}: {e}; read without it", crate::log::path(&path)),
            }
        }
        let (ring, button) = (device.ring(), device.button());
        info!("input: {} event devices, ring axis {:#x} (counts x{}/{}), button {button}, {step} counts a step",
              files.len(), ring.0, ring.1, ring.2);
        let mut fds = [0; 2];
        // SAFETY: pipe2 fills both descriptors, which the Files then own.
        let (wake, ring_bell) = unsafe {
            if libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) != 0 {
                error!("input: pipe: {}; input is not read", std::io::Error::last_os_error());
                files.clear();
            }
            (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1]))
        };
        let (sender, events) = std::sync::mpsc::channel();
        if let Err(e) = std::thread::Builder::new().name("input".into()).spawn(move || read_devices(files, sender, ring_bell)) {
            error!("input: thread: {e}; input is not read");
        }
        Devices { events, wake, step, dial: 0, ring, button, counts: Vec::new(), turning: (0, 0) }
    }

    /// How far the ring has turned towards its next step: -0.5 to 0.5 of
    /// one, clockwise positive.
    pub fn pending(&self) -> f32 {
        self.dial as f32 / (self.step * self.ring.2) as f32
    }

    /// Readable when events wait.
    pub fn fds(&self) -> Vec<RawFd> {
        vec![self.wake.as_raw_fd()]
    }

    /// Everything waiting, with consecutive wheel detents added together.
    pub fn read(&mut self) -> Vec<Input> {
        let mut drained = [0u8; 64];
        while matches!(self.wake.read(&mut drained), Ok(n) if n > 0) {}
        let mut out = Vec::new();
        while let Ok((event, at)) = self.events.try_recv() {
            trace!("input: event type {:#x} code {:#x} value {}", event.type_, event.code, event.value);
            if event.type_ == EV_REL && event.code == self.ring.0 {
                let counts = self.ring.1 * event.value / self.ring.2.max(1);
                match hysteresis(&mut self.turning, counts) {
                    Some(c) => self.counts.push((c, at)),
                    None if counts != 0 => trace!("input: {counts} counts held back by the hysteresis ({} so far)", self.turning.1),
                    None => {}
                }
            }
            let Some(input) = decode(event.type_, event.code, event.value, self.ring, self.button, self.step, &mut self.dial) else { continue };
            match input {
                Input::Wheel(steps) | Input::Notch(steps) => debug!("input: wheel {steps:+} steps"),
                Input::Press => debug!("input: button pressed"),
                Input::Release => debug!("input: button released"),
            }
            match (out.last_mut(), input) {
                (Some(Input::Wheel(total)), Input::Wheel(more)) => *total += more,
                _ => out.push(input),
            }
        }
        out
    }

    /// The wheel events' counts since last taken, each with when it came.
    pub fn take_counts(&mut self) -> Vec<(i32, Instant)> {
        std::mem::take(&mut self.counts)
    }
}

/// Reads the event devices as events come, stamping each, and rings
/// `bell` for the render loop; ahead of the rest, as the render loop is.
fn read_devices(mut files: Vec<File>, events: std::sync::mpsc::Sender<(libc::input_event, Instant)>, mut bell: File) {
    use std::io::Write;
    const SIZE: usize = std::mem::size_of::<libc::input_event>();
    if files.is_empty() {
        return;
    }
    crate::platform::prioritize();
    let mut fds: Vec<libc::pollfd> = files.iter().map(|f| libc::pollfd { fd: f.as_raw_fd(), events: libc::POLLIN, revents: 0 }).collect();
    loop {
        // SAFETY: fds is a live array of pollfd for the call's duration.
        if unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, -1) } < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        let at = Instant::now();
        let mut any = false;
        for (file, fd) in files.iter_mut().zip(&fds) {
            if fd.revents == 0 {
                continue;
            }
            let mut buffer = [0u8; SIZE * 32];
            loop {
                let n = match file.read(&mut buffer) {
                    Ok(n) if n >= SIZE => n,
                    _ => break,
                };
                for chunk in buffer[..n - n % SIZE].as_chunks::<SIZE>().0 {
                    // SAFETY: a whole input_event, as the kernel wrote it.
                    let event: libc::input_event = unsafe { std::ptr::read_unaligned(chunk.as_ptr().cast()) };
                    if events.send((event, at)).is_err() {
                        return;
                    }
                    any = true;
                }
            }
        }
        if any {
            let _ = bell.write(&[1]);
        }
    }
}

/// `counts` past the hysteresis: a turn against the last direction is held
/// back until HYSTERESIS counts have come that way.
fn hysteresis((direction, held): &mut (i32, i32), counts: i32) -> Option<i32> {
    if counts == 0 {
        return None;
    }
    if *direction != 0 && counts.signum() != *direction {
        *held += counts.abs();
        if *held < HYSTERESIS {
            return None;
        }
        *direction = counts.signum();
        *held = 0;
        return Some(counts);
    }
    *direction = counts.signum();
    *held = 0;
    Some(counts)
}

/// An event as input: the unit's ring axis, its button or Enter.
fn decode(kind: u16, code: u16, value: i32, (axis, mul, div): (u16, i32, i32), button: u16, step: i32, dial: &mut i32) -> Option<Input> {
    match (kind, code, value) {
        // Counts are kept scaled by `div` so the ratio loses none.
        (EV_REL, c, v) if c == axis => {
            *dial += mul * v;
            let step = step * div;
            let detents = (*dial as f32 / step as f32).round() as i32;
            *dial -= detents * step;
            (detents != 0).then_some(Input::Wheel(detents))
        }
        (EV_KEY, c, 1) if c == KEY_ENTER || c == button => Some(Input::Press),
        (EV_KEY, c, 0) if c == KEY_ENTER || c == button => Some(Input::Release),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{gen2, gen3};

    const GEN3: ((u16, i32, i32), u16) = (gen3::RING, gen3::BUTTON);
    const GEN2: ((u16, i32, i32), u16) = (gen2::RING, gen2::BUTTON);

    #[test]
    fn dial_counts_become_clockwise_detents_and_the_button_is_decoded() {
        // A step of one detent, to follow the sensor's counts one by one.
        const DIAL_STEP: i32 = -120;
        let mut dial = 0;
        // Three detents clockwise, as the gen 3 reported them.
        let counts = [-44, -75, -121, -56, -64];
        let detents: i32 = counts.iter()
            .filter_map(|&v| match decode(EV_REL, 0x07, v, GEN3.0, GEN3.1, DIAL_STEP, &mut dial) { Some(Input::Wheel(d)) => Some(d), _ => None })
            .sum();
        assert_eq!((detents, dial), (3, 0));
        assert_eq!(decode(EV_REL, 0x07, 50, GEN3.0, GEN3.1, DIAL_STEP, &mut dial), None, "under half a detent waits");
        assert_eq!(decode(EV_REL, 0x07, 20, GEN3.0, GEN3.1, DIAL_STEP, &mut dial), Some(Input::Wheel(-1)));
        assert_eq!(dial, -50, "the rest counts towards the next detent");
        // Counts add up across reports, so short ones lose nothing: 50
        // reports of 115 counts are 5750, 48 detents less 10 counts.
        let mut dial = 0;
        let turned: i32 = (0..50).filter_map(|_| match decode(EV_REL, 0x07, -115, GEN3.0, GEN3.1, DIAL_STEP, &mut dial) {
            Some(Input::Wheel(d)) => Some(d), _ => None }).sum();
        assert_eq!((turned, dial), (48, 10));
        assert_eq!(decode(EV_KEY, KEY_ENTER, 1, GEN3.0, GEN3.1, DIAL_STEP, &mut dial), Some(Input::Press));
        assert_eq!(decode(EV_KEY, KEY_ENTER, 0, GEN3.0, GEN3.1, DIAL_STEP, &mut dial), Some(Input::Release));
        assert_eq!(decode(EV_KEY, KEY_ENTER, 2, GEN3.0, GEN3.1, DIAL_STEP, &mut dial), None);
        assert_eq!(decode(EV_REL, 0x01, 3, GEN3.0, GEN3.1, DIAL_STEP, &mut dial), None);
        // The gen 2's ring: 100 counts a detent, negative clockwise, and its
        // power key.
        let mut dial = 0;
        assert_eq!(decode(EV_REL, 0x00, -100, GEN2.0, GEN2.1, DIAL_STEP, &mut dial), Some(Input::Wheel(1)));
        assert_eq!(decode(EV_REL, 0x00, 40, GEN2.0, GEN2.1, DIAL_STEP, &mut dial), None);
        assert_eq!(decode(EV_REL, 0x00, 60, GEN2.0, GEN2.1, DIAL_STEP, &mut dial), Some(Input::Wheel(-1)));
        assert_eq!(decode(EV_KEY, 116, 1, GEN2.0, GEN2.1, DIAL_STEP, &mut dial), Some(Input::Press));
    }
}
