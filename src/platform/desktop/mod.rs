//! The desktop build (`--features desktop`): the client on a Linux or macOS
//! host, in a window standing in for the unit's panel, ring and button.
//! The modules here take the place of the unit's own under the same names,
//! so the client's loop runs as it does on the unit. The window is drawn
//! by the CPU painter; nest-qemu's backplate.py, beside the repository,
//! stands in for the backplate (`serial`), and an HTTP server (`control`)
//! lets tools look at the screen and drive the inputs.
//!
//! The window (winit, shown through OpenGL) is round: the unit as
//! nest-qemu's page draws it (`device`), with its shadow, on a clear
//! borderless window, its status light blinking each second as the
//! page's does. The lens is the button, the steel ring moves the window
//! and Ctrl+Q closes it. It is pumped on the main thread: each event comes
//! in on its own, so a tap's press and release are both seen however close
//! together they come.
//!
//! `--headless` opens no window: frames and input go through `control`
//! alone, and the client takes no focus and shows nothing on the desktop.

pub mod backlight;
pub mod beeper;
pub mod bus;
pub mod connman;
pub mod control;
mod device;
mod gl;
pub mod input;
pub mod screen;
pub mod serial;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::pump_events::EventLoopExtPumpEvents;
use winit::window::{CursorIcon, Window, WindowId};

use device::Part;
use input::Event;

/// The unit's size in points, its steel ring across: the gen 3's panel's
/// width. The window adds the shadow's margin round it, and is smaller
/// where the display's height leaves no room for it.
pub const SIZE: usize = 480;
/// The share of the display's height the window may take.
const ROOM: f64 = 0.9;
/// No window: `--headless`.
pub static HEADLESS: AtomicBool = AtomicBool::new(false);
/// The face's pixels across without a window, unless `--pixels`: the gen 3's.
const HEADLESS_SIDE: usize = 480;

/// The status light's blink: lit this long each second.
const BLINK: Duration = Duration::from_millis(40);

/// Scroll a notch: a mouse wheel's event, however many lines macOS's
/// acceleration makes it, or a trackpad's pixels.
const LINES: f64 = 1.0;
const PIXELS: f64 = 80.0;

thread_local! {
    /// The window, opened by the screen on the main thread, which also
    /// reads the input from it.
    static DESKTOP: RefCell<Option<Desktop>> = const { RefCell::new(None) };
}

struct Desktop {
    events: EventLoop<()>,
    app: App,
}

#[derive(Default)]
struct App {
    window: Option<gl::Gl>,
    /// When the client's frames of the last second were shown: the frame
    /// rate's label, counted again at each redraw, so it falls to 0 while
    /// the client draws nothing.
    shown: VecDeque<Instant>,
    /// The pointer, in pixels, and whether the button is held from it.
    cursor: (f64, f64),
    pressing: bool,
    modifiers: ModifiersState,
    /// When the window opened, which the status light blinks from.
    opened: Option<Instant>,
    /// What came in since last taken.
    input: Vec<Event>,
    closed: bool,
    /// Scroll's sign to clockwise, and scroll not yet a whole notch.
    scroll_sign: f64,
    scrolled: f64,
    /// The part the pointer was last over.
    part: Option<Part>,
}

impl App {
    fn turn(&mut self, amount: f64, per_notch: f64) {
        self.scrolled += amount * self.scroll_sign / per_notch;
        let notches = self.scrolled.trunc();
        self.scrolled -= notches;
        trace!("window: scroll {amount:+.2} ({per_notch} a notch), {notches} notches, {:+.2} left", self.scrolled);
        if notches != 0.0 {
            debug!("window: scrolled {notches:+} notches");
            self.input.push(Event::Notch(notches as i32));
        }
    }

    /// What the pointer is over.
    fn under(&self, gl: &gl::Gl) -> Part {
        let k = f64::from(gl.window.inner_size().width) / f64::from(device::CANVAS);
        device::part((self.cursor.0 / k) as f32, (self.cursor.1 / k) as f32)
    }

    /// The pointer moved: its cursor says what the part under it does, a
    /// hand over the button and the scroll arrows over the ring.
    fn hover(&mut self, x: f64, y: f64) {
        self.cursor = (x, y);
        trace!("window: pointer at ({x:.0}, {y:.0})");
        let Some(gl) = &self.window else { return };
        let part = self.under(gl);
        if self.part != Some(part) {
            debug!("window: pointer over {}", name(part));
            self.part = Some(part);
        }
        gl.window.set_cursor(match part {
            Part::Button => CursorIcon::Pointer,
            Part::Ring => CursorIcon::AllScroll,
            Part::Outside => CursorIcon::Default,
        });
    }

    /// The left button pressed or released where the pointer is.
    fn click(&mut self, pressed: bool) {
        let Some(gl) = &self.window else { return };
        if !pressed {
            if std::mem::take(&mut self.pressing) {
                debug!("window: mouse released, the button with it");
                self.input.push(Event::Release);
            } else {
                debug!("window: mouse released");
            }
            return;
        }
        let part = self.under(gl);
        debug!("window: mouse pressed on {}", name(part));
        match part {
            Part::Button => {
                self.pressing = true;
                self.input.push(Event::Press);
            }
            Part::Ring => {
                if let Err(e) = gl.window.drag_window() {
                    debug!("window: drag: {e}");
                }
            }
            Part::Outside => {}
        }
    }

    /// Lights the status light for BLINK each second, redrawing the window
    /// as it changes whether or not the client draws.
    fn blink(&mut self) {
        let Some(opened) = self.opened else { return };
        let lit = opened.elapsed().subsec_nanos() < BLINK.as_nanos() as u32;
        if self.window.as_ref().is_some_and(|gl| gl.lit != lit) {
            let label = frame_rate(&mut self.shown);
            if let Some(gl) = &mut self.window {
                gl.lit = lit;
                gl.present(None, &label);
            }
        }
    }

    /// The window's points across: the unit at SIZE with its margin, or
    /// smaller to fit the display.
    fn points(events: &ActiveEventLoop) -> f64 {
        let whole = SIZE as f64 * f64::from(device::CANVAS) / f64::from(device::UNIT);
        let Some(m) = events.primary_monitor() else {
            debug!("window: no primary display; {whole:.0} points across");
            return whole;
        };
        let points = whole.min(f64::from(m.size().height) / m.scale_factor() * ROOM);
        debug!("window: display {}x{} pixels at scale {}; {points:.0} points across of {whole:.0}", m.size().width,
               m.size().height, m.scale_factor());
        points
    }
}

/// A part of the unit, as the log names it.
fn name(part: Part) -> &'static str {
    match part {
        Part::Button => "the lens (the button)",
        Part::Ring => "the ring",
        Part::Outside => "outside the unit",
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, events: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let points = Self::points(events);
        let attributes = Window::default_attributes().with_title("client-ng")
            .with_inner_size(winit::dpi::LogicalSize::new(points, points))
            .with_decorations(false).with_transparent(true).with_resizable(false);
        // The unit casts its own shadow.
        #[cfg(target_os = "macos")]
        let attributes = winit::platform::macos::WindowAttributesExtMacOS::with_has_shadow(attributes, false);
        match gl::Gl::open(events, attributes) {
            Ok(gl) => {
                self.window = Some(gl);
                self.opened = Some(Instant::now());
            }
            Err(e) => error!("window: {e}"),
        }
    }

    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                info!("window: closed");
                self.closed = true;
            }
            WindowEvent::Resized(size) => {
                debug!("window: {}x{} pixels", size.width, size.height);
                let label = frame_rate(&mut self.shown);
                if let Some(gl) = &mut self.window {
                    gl.present(None, &label);
                }
            }
            WindowEvent::RedrawRequested => {
                let label = frame_rate(&mut self.shown);
                if let Some(gl) = &mut self.window {
                    gl.present(None, &label);
                }
            }
            WindowEvent::CursorMoved { position, .. } => self.hover(position.x, position.y),
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => self.click(state == ElementState::Pressed),
            WindowEvent::MouseWheel { delta: MouseScrollDelta::LineDelta(_, y), .. } => {
                self.turn(f64::from(y).clamp(-LINES, LINES), LINES);
            }
            WindowEvent::MouseWheel { delta: MouseScrollDelta::PixelDelta(p), .. } => self.turn(p.y, PIXELS),
            WindowEvent::ModifiersChanged(m) => {
                debug!("window: modifiers {:?}", m.state());
                self.modifiers = m.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                debug!("window: key {:?} {}{}", event.logical_key, if pressed { "pressed" } else { "released" },
                       if event.repeat { " (repeat)" } else { "" });
                match event.logical_key {
                    Key::Character(ref c) if pressed && self.modifiers.control_key() && c.eq_ignore_ascii_case("q") => {
                        info!("window: closed by Ctrl+Q");
                        self.closed = true;
                    }
                    Key::Named(NamedKey::ArrowUp | NamedKey::ArrowRight) if pressed => self.input.push(Event::Notch(1)),
                    Key::Named(NamedKey::ArrowDown | NamedKey::ArrowLeft) if pressed => self.input.push(Event::Notch(-1)),
                    Key::Named(NamedKey::Space | NamedKey::Enter) if !event.repeat => {
                        self.input.push(if pressed { Event::Press } else { Event::Release });
                    }
                    _ => {}
                }
            }
            ref other => trace!("window: {other:?}"),
        }
    }
}

/// Keeps macOS from napping the client: App Nap slows the loop of an app
/// whose windows are hidden to a step every second or two.
#[cfg(target_os = "macos")]
fn stay_awake() {
    use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
    // SAFETY: an activity begun on the shared process info, kept for the
    // process's life.
    let activity = unsafe {
        NSProcessInfo::processInfo().beginActivityWithOptions_reason(NSActivityOptions::NSActivityUserInteractive,
                                                                      &NSString::from_str("drawing the thermostat's screen"))
    };
    std::mem::forget(activity);
}

/// Opens the window, once the event loop has started; the face's pixels
/// across in it.
fn open(scroll_sign: f64) -> Result<usize, String> {
    #[cfg(target_os = "macos")]
    stay_awake();
    if HEADLESS.load(Ordering::Relaxed) {
        info!("window: none (--headless); the screen and input through the control server");
        return Ok(HEADLESS_SIDE);
    }
    let events = EventLoop::new().map_err(|e| format!("window: {e}"))?;
    let mut d = Desktop { events, app: App { scroll_sign, ..App::default() } };
    for _ in 0..100 {
        d.events.pump_app_events(Some(Duration::from_millis(10)), &mut d.app);
        if let Some(gl) = &d.app.window {
            let size = gl.window.inner_size();
            let k = f64::from(size.width) / f64::from(device::CANVAS);
            let side = (k * f64::from(2.0 * device::DISPLAY)).round() as usize;
            info!("window: {}x{} pixels at scale {}, the face {side} pixels across", size.width, size.height,
                  gl.window.scale_factor());
            DESKTOP.with(|w| *w.borrow_mut() = Some(d));
            return Ok(side);
        }
    }
    Err("window: not opened".into())
}

/// Pumps the window: the input since last taken, and whether it is open.
fn pump() -> (Vec<Event>, bool) {
    DESKTOP.with(|d| {
        let mut d = d.borrow_mut();
        let Some(d) = d.as_mut() else { return (Vec::new(), true) };
        d.events.pump_app_events(Some(Duration::ZERO), &mut d.app);
        d.app.blink();
        (std::mem::take(&mut d.app.input), !d.app.closed)
    })
}

/// The frames shown in the last second, as the label says them.
fn frame_rate(shown: &mut VecDeque<Instant>) -> String {
    let now = Instant::now();
    while shown.front().is_some_and(|&t| now - t > Duration::from_secs(1)) {
        shown.pop_front();
    }
    format!("{} FPS", shown.len())
}

/// Shows a frame, premultiplied RGBA `side` pixels each way, with the
/// frame rate under it; the frames shown in the last second.
fn show(frame: &[u8], side: usize) -> usize {
    DESKTOP.with(|d| {
        let mut d = d.borrow_mut();
        let Some(d) = d.as_mut() else { return 0 };
        d.app.shown.push_back(Instant::now());
        let label = frame_rate(&mut d.app.shown);
        if let Some(gl) = &mut d.app.window {
            gl.present(Some((frame, side)), &label);
        }
        d.app.shown.len()
    })
}
