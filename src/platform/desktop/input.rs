//! The ring and the button from the window and from `control`. A notch
//! of a mouse wheel, 80 pixels of a trackpad or a press of an arrow key
//! (clockwise up and right, held to repeat) is a step of the dial, the
//! same way whether or not macOS's natural scrolling is on; the left
//! mouse button, Space and Enter are the unit's button, held while they
//! are held. `control`'s turns are the ring's own detents.
//!
//! `control`'s turns reach the client as the gen 3's ring reports one, a
//! wheel event of 120 counts a detent, negative clockwise, gathered into
//! steps as `platform::input` gathers them, and move the top menu's ring
//! and Settings' carousel by their push (`scroll`). A whole detent is past
//! the ring's reversal hysteresis (20 counts), so none is applied.
//!
//! A notch is not the ring's push, which a run of notches would build into
//! a fling: it is a step that moves a list, the ring or the carousel by
//! one entry (`Input::Notch`), so each can be picked.

use std::os::fd::RawFd;

use crate::engine::Input;

use super::control;

/// Milliseconds the loop may wait for input: the window's events are read
/// only when the window is pumped, on this thread.
pub const PUMP_MS: i32 = 10;

/// The gen 3's counts a detent.
const DETENT: i32 = 120;
/// A notch's detents, for the wheel's position.
const NOTCH_DETENTS: i32 = 2;

/// Scroll's sign to clockwise: macOS hands wheel deltas on turned round
/// by its natural scrolling unless that is off
/// (com.apple.swipescrolldirection 0).
pub fn scroll_sign() -> f64 {
    let natural = cfg!(target_os = "macos")
        && std::process::Command::new("defaults").args(["read", "-g", "com.apple.swipescrolldirection"]).output()
            .map_or(true, |o| String::from_utf8_lossy(&o.stdout).trim() != "0");
    debug!("input: natural scrolling {}", if natural { "on, scroll turned round" } else { "off" });
    if natural { -1.0 } else { 1.0 }
}

/// A turn or the button, from the window or from `control`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// The ring's detents, clockwise positive.
    Turn(i32),
    /// The window's notches, clockwise positive.
    Notch(i32),
    Press,
    Release,
}

pub struct Devices {
    /// Dial counts to a step; negative when counts run counterclockwise.
    step: i32,
    /// Counts not yet a whole step.
    dial: i32,
    /// Each wheel event's counts, not yet taken, with when it came.
    counts: Vec<(i32, std::time::Instant)>,
}

impl Devices {
    pub fn open(step: i32, _device: crate::platform::Device) -> Devices {
        Devices { step, dial: 0, counts: Vec::new() }
    }

    pub fn pending(&self) -> f32 {
        self.dial as f32 / self.step as f32
    }

    /// Nothing to poll: the window is pumped every PUMP_MS instead.
    pub fn fds(&self) -> Vec<RawFd> {
        Vec::new()
    }

    /// Pumps the window, and returns everything waiting with consecutive
    /// wheel steps added together. A closed window stops the client as
    /// SIGTERM does.
    pub fn read(&mut self) -> Vec<Input> {
        let (mut events, open) = super::pump();
        if !open {
            // SAFETY: raising a signal whose handler only stores to an atomic.
            unsafe { libc::raise(libc::SIGTERM) };
        }
        events.extend(control::take_inputs());
        let mut out = Vec::new();
        for event in events {
            let input = match event {
                Event::Notch(n) => {
                    control::turned(n * NOTCH_DETENTS);
                    debug!("input: {n:+} notches");
                    Input::Notch(n)
                }
                Event::Turn(detents) => {
                    control::turned(detents);
                    self.counts.push((-DETENT * detents, std::time::Instant::now()));
                    self.dial += -DETENT * detents;
                    let steps = (self.dial as f32 / self.step as f32).round() as i32;
                    self.dial -= steps * self.step;
                    debug!("input: {event:?}: {detents:+} detents, {steps:+} steps, {} counts to the next", self.dial);
                    if steps == 0 {
                        continue;
                    }
                    Input::Wheel(steps)
                }
                Event::Press => {
                    debug!("input: button pressed");
                    Input::Press
                }
                Event::Release => {
                    debug!("input: button released");
                    Input::Release
                }
            };
            match (out.last_mut(), input) {
                (Some(Input::Wheel(total)), Input::Wheel(more)) | (Some(Input::Notch(total)), Input::Notch(more)) => *total += more,
                _ => out.push(input),
            }
        }
        out
    }

    pub fn take_counts(&mut self) -> Vec<(i32, std::time::Instant)> {
        std::mem::take(&mut self.counts)
    }
}
