//! A GLES2 context on the framebuffer through Vivante's EGL.
//!
//! The libraries are the unit's own (libEGL, libGLESv2 and the libGAL they
//! sit on), loaded at run time: nothing proprietary is needed to build, and
//! a unit without them fails with their names. Vivante's fbdev EGL takes its
//! native display and window from its own fbGetDisplayByIndex and
//! fbCreateWindow.

use std::ffi::{c_char, c_void, CString};

use libloading::Library;

type Display = *mut c_void;
type Config = *mut c_void;
type Context = *mut c_void;
type Surface = *mut c_void;
type Native = *mut c_void;

const EGL_NONE: i32 = 0x3038;
const EGL_EXTENSIONS: i32 = 0x3055;
const EGL_SYNC_FENCE_KHR: u32 = 0x30f9;
const EGL_CONDITION_SATISFIED_KHR: u32 = 0x30f6;
const EGL_SYNC_FLUSH_COMMANDS_BIT_KHR: i32 = 1;
const EGL_FOREVER_KHR: u64 = u64::MAX;
const EGL_RED_SIZE: i32 = 0x3024;
const EGL_GREEN_SIZE: i32 = 0x3023;
const EGL_BLUE_SIZE: i32 = 0x3022;
const EGL_ALPHA_SIZE: i32 = 0x3021;
const EGL_STENCIL_SIZE: i32 = 0x3026;
const EGL_SURFACE_TYPE: i32 = 0x3033;
const EGL_WINDOW_BIT: i32 = 0x0004;
const EGL_RENDERABLE_TYPE: i32 = 0x3040;
const EGL_OPENGL_ES2_BIT: i32 = 0x0004;
const EGL_CONTEXT_CLIENT_VERSION: i32 = 0x3098;
const EGL_OPENGL_ES_API: u32 = 0x30A0;

/// The EGL calls used, resolved once.
struct Egl {
    get_display: unsafe extern "C" fn(Native) -> Display,
    initialize: unsafe extern "C" fn(Display, *mut i32, *mut i32) -> u32,
    bind_api: unsafe extern "C" fn(u32) -> u32,
    choose_config: unsafe extern "C" fn(Display, *const i32, *mut Config, i32, *mut i32) -> u32,
    create_window_surface: unsafe extern "C" fn(Display, Config, Native, *const i32) -> Surface,
    create_context: unsafe extern "C" fn(Display, Config, Context, *const i32) -> Context,
    make_current: unsafe extern "C" fn(Display, Surface, Surface, Context) -> u32,
    swap_buffers: unsafe extern "C" fn(Display, Surface) -> u32,
    swap_interval: unsafe extern "C" fn(Display, i32) -> u32,
    get_error: unsafe extern "C" fn() -> i32,
    query_string: unsafe extern "C" fn(Display, i32) -> *const c_char,
    get_proc_address: unsafe extern "C" fn(*const c_char) -> *const c_void,
    fb_get_display_by_index: unsafe extern "C" fn(i32) -> Native,
    fb_get_display_geometry: unsafe extern "C" fn(Native, *mut i32, *mut i32),
    fb_create_window: unsafe extern "C" fn(Native, i32, i32, i32, i32) -> Native,
}

/// The framebuffer's wait for its panel's next vertical sync
/// (FBIO_WAITFORVSYNC, _IOW('F', 0x20, u32)).
const FBIO_WAITFORVSYNC: libc::c_ulong = 0x4004_4620;

/// The display controller's scan-out registers (i.MX LCDIF): the page the
/// panel shows (CUR_BUF) and the one it takes at its next frame
/// (NEXT_BUF). A page flip is pending while they differ.
struct Scanout {
    regs: *const u32,
}

impl Scanout {
    const CUR_BUF: usize = 0x40 / 4;
    const NEXT_BUF: usize = 0x50 / 4;

    /// The LCDIF named in /proc/iomem, mapped through /dev/mem.
    fn open() -> Result<Scanout, String> {
        let iomem = std::fs::read_to_string("/proc/iomem").map_err(|e| format!("/proc/iomem: {e}"))?;
        let line = iomem.lines().find(|l| l.contains("lcdif")).ok_or("no LCDIF in /proc/iomem")?;
        let start = line.trim().split('-').next().unwrap_or("");
        let base = libc::off_t::from_str_radix(start, 16).map_err(|e| format!("LCDIF at {start:?}: {e}"))?;
        let mem = std::fs::File::open("/dev/mem").map_err(|e| format!("/dev/mem: {e}"))?;
        use std::os::fd::AsRawFd;
        // SAFETY: a read-only shared mapping of the LCDIF's first page, kept for the process's life.
        let regs = unsafe { libc::mmap(std::ptr::null_mut(), 4096, libc::PROT_READ, libc::MAP_SHARED, mem.as_raw_fd(), base) };
        if regs == libc::MAP_FAILED {
            return Err(format!("mapping the LCDIF at {base:#x}: {}", std::io::Error::last_os_error()));
        }
        Ok(Scanout { regs: regs as *const u32 })
    }

    fn read(&self, register: usize) -> u32 {
        // SAFETY: the registers read lie in the page mapped in open().
        unsafe { std::ptr::read_volatile(self.regs.add(register)) }
    }

    /// The page the panel takes at its next frame.
    fn next(&self) -> u32 {
        self.read(Self::NEXT_BUF)
    }

    /// Whether the panel has yet to take the page last flipped to.
    fn pending(&self) -> bool {
        self.read(Self::CUR_BUF) != self.next()
    }
}

/// EGL_KHR_fence_sync: a fence after each frame's commands, waited on
/// before the frame is taken as done.
struct Fences {
    create: unsafe extern "C" fn(Display, u32, *const i32) -> *mut c_void,
    wait: unsafe extern "C" fn(Display, *mut c_void, i32, u64) -> u32,
    destroy: unsafe extern "C" fn(Display, *mut c_void) -> u32,
}

impl Fences {
    fn load(egl: &Egl, display: Display) -> Option<Self> {
        // SAFETY: EGL entry points have the signatures specified by EGL_KHR_fence_sync.
        unsafe {
            let extensions = (egl.query_string)(display, EGL_EXTENSIONS);
            if extensions.is_null() || !std::ffi::CStr::from_ptr(extensions).to_bytes().split(|b| *b == b' ')
                .any(|e| e == b"EGL_KHR_fence_sync") { return None; }
            let create = (egl.get_proc_address)(c"eglCreateSyncKHR".as_ptr());
            let wait = (egl.get_proc_address)(c"eglClientWaitSyncKHR".as_ptr());
            let destroy = (egl.get_proc_address)(c"eglDestroySyncKHR".as_ptr());
            if create.is_null() || wait.is_null() || destroy.is_null() { return None; }
            Some(Self {
                create: std::mem::transmute::<*const c_void, unsafe extern "C" fn(Display, u32, *const i32) -> *mut c_void>(create),
                wait: std::mem::transmute::<*const c_void, unsafe extern "C" fn(Display, *mut c_void, i32, u64) -> u32>(wait),
                destroy: std::mem::transmute::<*const c_void, unsafe extern "C" fn(Display, *mut c_void) -> u32>(destroy),
            })
        }
    }

    /// Waits until every command issued so far has run.
    ///
    /// # Safety
    /// `display` is open, with the context current.
    unsafe fn finish(&self, display: Display) -> Result<(), String> {
        let fence = (self.create)(display, EGL_SYNC_FENCE_KHR, [EGL_NONE].as_ptr());
        if fence.is_null() { return Err("eglCreateSyncKHR failed".into()); }
        let result = (self.wait)(display, fence, EGL_SYNC_FLUSH_COMMANDS_BIT_KHR, EGL_FOREVER_KHR);
        (self.destroy)(display, fence);
        if result != EGL_CONDITION_SATISFIED_KHR { return Err("eglClientWaitSyncKHR failed".into()); }
        Ok(())
    }
}

/// The current GLES2 context and its window.
pub struct Gl {
    egl: Egl,
    gles: Library,
    _egl_lib: Library,
    display: Display,
    surface: Surface,
    fences: Option<Fences>,
    finish: extern "C" fn(),
    /// The framebuffer, for its vertical sync; none where the wait is
    /// not offered, said once.
    fb: std::cell::Cell<Option<std::fs::File>>,
    /// Where the panel's page flips can be seen; none where the LCDIF
    /// cannot be mapped.
    scanout: Option<Scanout>,
    /// Time spent waiting for the GPU and for the panel's sync since
    /// last taken.
    pub waited: std::cell::Cell<(std::time::Duration, std::time::Duration)>,
    pub width: u32,
    pub height: u32,
    /// The window frames are drawn in on the framebuffer: its corner and
    /// size, the face's square.
    pub window: (i32, i32, u32, u32),
}

fn open(names: &[&str]) -> Result<Library, String> {
    let mut errors = Vec::new();
    for name in names {
        // SAFETY: the vendor libraries run no initialisers that depend on us.
        match unsafe { Library::new(*name) } {
            Ok(lib) => {
                debug!("egl: {name} loaded");
                return Ok(lib);
            }
            Err(e) => {
                debug!("egl: {name}: {e}");
                errors.push(e.to_string());
            }
        }
    }
    Err(errors.join("; "))
}

macro_rules! sym {
    ($lib:expr, $name:literal) => {
        // SAFETY: the declared type is the EGL/Vivante signature of $name.
        *unsafe { $lib.get($name) }.map_err(|e| format!("{}: {e}", String::from_utf8_lossy(&$name[..$name.len() - 1])))?
    };
}

impl Gl {
    /// A double-buffered GLES2 context over the whole framebuffer.
    pub fn open() -> Result<Gl, String> {
        let egl_lib = open(&["libEGL.so.1", "libEGL.so"])?;
        let gles = open(&["libGLESv2.so.2", "libGLESv2.so"])?;
        let egl = Egl {
            get_display: sym!(egl_lib, b"eglGetDisplay\0"),
            initialize: sym!(egl_lib, b"eglInitialize\0"),
            bind_api: sym!(egl_lib, b"eglBindAPI\0"),
            choose_config: sym!(egl_lib, b"eglChooseConfig\0"),
            create_window_surface: sym!(egl_lib, b"eglCreateWindowSurface\0"),
            create_context: sym!(egl_lib, b"eglCreateContext\0"),
            make_current: sym!(egl_lib, b"eglMakeCurrent\0"),
            swap_buffers: sym!(egl_lib, b"eglSwapBuffers\0"),
            swap_interval: sym!(egl_lib, b"eglSwapInterval\0"),
            get_error: sym!(egl_lib, b"eglGetError\0"),
            query_string: sym!(egl_lib, b"eglQueryString\0"),
            get_proc_address: sym!(egl_lib, b"eglGetProcAddress\0"),
            fb_get_display_by_index: sym!(egl_lib, b"fbGetDisplayByIndex\0"),
            fb_get_display_geometry: sym!(egl_lib, b"fbGetDisplayGeometry\0"),
            fb_create_window: sym!(egl_lib, b"fbCreateWindow\0"),
        };
        // SAFETY: EGL calls in the documented order, on this thread only.
        unsafe {
            let fb = (egl.fb_get_display_by_index)(0);
            if fb.is_null() {
                return Err("fbGetDisplayByIndex(0) found no framebuffer".into());
            }
            let (mut width, mut height) = (0, 0);
            (egl.fb_get_display_geometry)(fb, &mut width, &mut height);
            debug!("egl: framebuffer 0 is {width}x{height}");
            let display = (egl.get_display)(fb);
            let check = |ok: bool, what: &str| if ok { Ok(()) } else { Err(format!("{what}: EGL error {:#x}", (egl.get_error)())) };
            check(!display.is_null(), "eglGetDisplay")?;
            let (mut major, mut minor) = (0, 0);
            check((egl.initialize)(display, &mut major, &mut minor) != 0, "eglInitialize")?;
            check((egl.bind_api)(EGL_OPENGL_ES_API) != 0, "eglBindAPI")?;
            debug!("egl: EGL {major}.{minor} initialised, GLES bound");
            // femtovg fills paths through the stencil buffer.
            let attributes = [
                EGL_RED_SIZE, 8, EGL_GREEN_SIZE, 8, EGL_BLUE_SIZE, 8, EGL_ALPHA_SIZE, 0,
                EGL_STENCIL_SIZE, 8, EGL_SURFACE_TYPE, EGL_WINDOW_BIT,
                EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT, EGL_NONE,
            ];
            let mut config: Config = std::ptr::null_mut();
            let mut count = 0;
            check((egl.choose_config)(display, attributes.as_ptr(), &mut config, 1, &mut count) != 0 && count == 1,
                  "eglChooseConfig (RGB888 with an 8-bit stencil)")?;
            // As stock's: only the face's square, so each frame clears
            // and copies no more than shows.
            let g = crate::display::Geometry::for_framebuffer(width as u32, height as u32);
            let (wx, wy, side) = (g.origin.0 as i32, g.origin.1 as i32, g.face as i32);
            debug!("egl: RGB888 config with an 8-bit stencil chosen");
            let window = (egl.fb_create_window)(fb, wx, wy, side, side);
            check(!window.is_null(), "fbCreateWindow")?;
            debug!("egl: window {side}x{side} at ({wx}, {wy})");
            let surface = (egl.create_window_surface)(display, config, window, std::ptr::null());
            check(!surface.is_null(), "eglCreateWindowSurface")?;
            let context = (egl.create_context)(display, config, std::ptr::null_mut(), [EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE].as_ptr());
            check(!context.is_null(), "eglCreateContext")?;
            check((egl.make_current)(display, surface, surface, context) != 0, "eglMakeCurrent")?;
            if (egl.swap_interval)(display, 1) == 0 {
                warn!("egl: eglSwapInterval(1): EGL error {:#x}; swaps may not wait for the panel", (egl.get_error)());
            }
            info!("display: {width}x{height}, EGL {major}.{minor}");
            let finish: extern "C" fn() = *gles.get(b"glFinish\0").map_err(|e| format!("glFinish: {e}"))?;
            let fences = Fences::load(&egl, display);
            let fb = std::fs::File::options().read(true).write(true).open("/dev/fb0")
                .map_err(|e| warn!("display: /dev/fb0: {e}; frames are not held to the panel's sync")).ok();
            let scanout = Scanout::open().map_err(|e| warn!("display: {e}; each frame waits two of the panel's syncs")).ok();
            info!("display: frames finished {}, {}", if fences.is_some() { "with EGL_KHR_fence_sync" } else { "by glFinish" },
                  match (&scanout, &fb) {
                      (Some(_), Some(_)) => "each flipped to once the panel has taken the one before",
                      (None, Some(_)) => "then held two of the panel's syncs",
                      _ => "unsynced",
                  });
            Ok(Gl { fences, finish, fb: std::cell::Cell::new(fb), scanout, waited: std::cell::Cell::default(), egl, gles, _egl_lib: egl_lib,
                    display, surface, width: width as u32, height: height as u32, window: (wx, wy, side as u32, side as u32) })
        }
    }

    /// A GL function by name: libGLESv2's own export first, then EGL's.
    pub fn proc_address(&self, name: &str) -> *const c_void {
        let Ok(cname) = CString::new(name) else { return std::ptr::null() };
        // SAFETY: only the address is taken; callers cast it to the GL signature.
        let direct = unsafe { self.gles.get::<*const c_void>(cname.as_bytes_with_nul()) }.map(|s| *s).ok();
        match direct {
            Some(p) if !p.is_null() => p,
            // SAFETY: a NUL-terminated name, as eglGetProcAddress takes.
            _ => unsafe { (self.egl.get_proc_address)(cname.as_ptr()) },
        }
    }

    /// Whether swaps wait for the panel's sync, and so pace the frames.
    pub fn vsynced(&self) -> bool {
        let fb = self.fb.take();
        let held = fb.is_some();
        self.fb.set(fb);
        held
    }

    /// Waits for the panel's next vertical sync, and whether it could.
    /// The driver counts a sync that came before the call as the next
    /// one, so one wait can return at once.
    fn vsync(&self) -> bool {
        let Some(fb) = self.fb.take() else { return false };
        use std::os::fd::AsRawFd;
        let arg: u32 = 0;
        // SAFETY: FBIO_WAITFORVSYNC reads a u32 (the CRTC, 0) and returns after the sync.
        if unsafe { libc::ioctl(fb.as_raw_fd(), FBIO_WAITFORVSYNC, &arg) } == 0 {
            self.fb.set(Some(fb));
            true
        } else {
            warn!("display: FBIO_WAITFORVSYNC: {}; frames are not held to the panel's sync", std::io::Error::last_os_error());
            false
        }
    }

    /// Shows the frame drawn, and returns once the GPU has finished it and
    /// the driver has flipped to its page.
    ///
    /// The unit's driver flips from a thread of its own once the GPU is
    /// done, as late as that thread gets to run, and without waiting for
    /// the panel to take the page flipped to before; a second flip in that
    /// time leaves the panel on the older page, which the next frame is
    /// then drawn into, torn on the panel. So each swap returns only once
    /// its flip is made, and the next waits until the panel has taken it:
    /// with three pages the one drawn into is never the one shown nor the
    /// one about to be. The GPU's finish keeps the next frame from reusing
    /// resources still in flight. Swaps run at the panel's rate at most.
    pub fn swap(&self) -> Result<(), String> {
        let started = std::time::Instant::now();
        if let Some(scanout) = &self.scanout {
            for _ in 0..4 {
                if !scanout.pending() || !self.vsync() { break; }
            }
        }
        let flipped = std::time::Instant::now();
        let before = self.scanout.as_ref().map(Scanout::next);
        // SAFETY: the display and surface made current in open().
        if unsafe { (self.egl.swap_buffers)(self.display, self.surface) } == 0 {
            return Err(format!("eglSwapBuffers: EGL error {:#x}", unsafe { (self.egl.get_error)() }));
        }
        let swapped = std::time::Instant::now();
        match &self.fences {
            // SAFETY: the display is open with the context current.
            Some(fences) => unsafe { fences.finish(self.display)? },
            None => (self.finish)(),
        }
        let finished = std::time::Instant::now();
        if let (Some(scanout), Some(before)) = (&self.scanout, before) {
            // The driver's flip, a frame at most after the GPU is done.
            while scanout.next() == before && finished.elapsed() < std::time::Duration::from_millis(20) {
                std::thread::sleep(std::time::Duration::from_micros(250));
            }
        }
        if self.scanout.is_none() && self.vsync() {
            self.vsync();
        }
        let (gpu, sync) = self.waited.get();
        self.waited.set((gpu + (finished - swapped), sync + (flipped - started) + finished.elapsed()));
        Ok(())
    }
}
