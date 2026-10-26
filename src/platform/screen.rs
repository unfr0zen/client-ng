//! What frames are drawn on: the GPU through EGL where the unit has one
//! (the gen 3's Vivante), else the CPU into the framebuffer (the gen 2).

use std::path::Path;

use crate::display::Geometry;
use crate::paint::{Counts, Painter};
use crate::platform;

pub enum Screen {
    Gpu { gl: platform::gen3::egl::Gl, renderer: Box<platform::gen3::render::Renderer> },
    Cpu { fb: platform::gen2::fb::Framebuffer, renderer: Box<platform::gen2::soft::Renderer> },
}

impl Screen {
    /// The GPU's on a unit that has one, or the framebuffer when its EGL
    /// cannot be had or `cpu` asks for it.
    pub fn open(device: platform::Device, cpu: bool) -> Result<Screen, String> {
        let gpu = if cpu { Err("--cpu".to_string()) } else if device.gpu() { Self::gpu() } else { Err(format!("{device:?}: no GPU")) };
        match gpu {
            Ok(s) => Ok(s),
            Err(e) => {
                // A fallback only where the GPU was wanted.
                if cpu || !device.gpu() {
                    info!("display: {e}; drawing on the CPU");
                } else {
                    warn!("display: {e}; drawing on the CPU");
                }
                let fb = platform::gen2::fb::Framebuffer::open("/dev/fb0")?;
                let renderer = platform::gen2::soft::Renderer::new(fb.width, fb.height)?;
                Ok(Screen::Cpu { fb, renderer: Box::new(renderer) })
            }
        }
    }

    fn gpu() -> Result<Screen, String> {
        // Vivante's fbdev EGL page-flips only when asked to; with one buffer
        // the panel keeps showing whichever page the previous client left it on.
        if std::env::var_os("FB_MULTI_BUFFER").is_none() {
            std::env::set_var("FB_MULTI_BUFFER", "3");
        }
        let gl = platform::gen3::egl::Gl::open()?;
        // SAFETY: the loader returns GL functions of the context made current above.
        let gles = unsafe { femtovg::renderer::OpenGl::new_from_function(|name| gl.proc_address(name)) }
            .map_err(|e| format!("GLES2 renderer: {e:?}"))?;
        let canvas = femtovg::Canvas::new(gles).map_err(|e| format!("canvas: {e:?}"))?;
        // SAFETY: as above; the batcher draws on the same, current context.
        let quads = unsafe { platform::gen3::quads::Quads::new(glow::Context::from_loader_function(|name| gl.proc_address(name))) }
            .map_err(|e| warn!("quads: {e}; femtovg draws everything")).ok();
        let (x, y, w, h) = gl.window;
        info!("display: GPU renderer, {w}x{h} pixels at ({x}, {y}) of {}x{}, {}", gl.width, gl.height,
              if quads.is_some() { "textures batched" } else { "femtovg only" });
        let renderer = platform::gen3::render::Renderer::new(canvas, quads, (x as f32, y as f32, w, h))?;
        Ok(Screen::Gpu { gl, renderer: Box::new(renderer) })
    }

    /// Whether frames need pacing to the panel by the clock: on the
    /// framebuffer, where nothing else paces them, and on the GPU where
    /// its swaps cannot wait for the panel's sync (nest-qemu's cannot).
    pub fn paced(&self) -> bool {
        match self {
            Screen::Gpu { gl, .. } => !gl.vsynced(),
            Screen::Cpu { .. } => true,
        }
    }

    /// The framebuffer's size in pixels.
    pub fn size(&self) -> (u32, u32) {
        match self {
            Screen::Gpu { gl, .. } => (gl.width, gl.height),
            Screen::Cpu { fb, .. } => (fb.width, fb.height),
        }
    }

    pub fn use_fonts(&mut self, fonts: &[(String, std::path::PathBuf)]) -> Result<(), String> {
        match self {
            Screen::Gpu { renderer, .. } => renderer.use_fonts(fonts),
            Screen::Cpu { renderer, .. } => renderer.use_fonts(fonts),
        }
    }

    /// Starts a frame and draws it with `draw`, given the painter for
    /// `dir`'s images.
    pub fn draw<R>(&mut self, geometry: &Geometry, design: f32, dir: &Path, draw: impl FnOnce(&mut dyn Painter) -> R) -> R {
        match self {
            Screen::Gpu { renderer, .. } => {
                renderer.begin(geometry, design);
                draw(&mut renderer.painter(dir))
            }
            Screen::Cpu { renderer, .. } => {
                renderer.begin(geometry, design);
                draw(&mut renderer.painter(dir))
            }
        }
    }

    /// Everything drawn, in order, flushed to the GPU.
    pub fn finish(&mut self) {
        if let Screen::Gpu { renderer, .. } = self {
            renderer.finish();
        }
    }

    /// Takes in images decoded off the render thread since last asked;
    /// true when any came, which asks for a frame with them.
    pub fn take_decoded(&mut self) -> bool {
        match self {
            Screen::Gpu { renderer, .. } => renderer.take_decoded(),
            Screen::Cpu { .. } => false,
        }
    }

    /// The frame onto the panel.
    pub fn swap(&mut self) -> Result<(), String> {
        let started = crate::log::enabled(crate::log::Level::Trace).then(std::time::Instant::now);
        let swapped = match self {
            Screen::Gpu { gl, .. } => gl.swap(),
            Screen::Cpu { fb, renderer } => {
                let width = renderer.pixmap.width();
                fb.show(renderer.pixmap.data(), width);
                Ok(())
            }
        };
        if let Some(started) = started {
            trace!("display: swap {:.1} ms", started.elapsed().as_secs_f64() * 1000.0);
        }
        swapped
    }

    /// Shapes drawn and time spent painting since last taken, and on the
    /// GPU the layers drawn again, their time and the flushes.
    pub fn take_counts(&mut self) -> Counts {
        match self {
            Screen::Gpu { gl, renderer } => {
                let (gpu, sync) = gl.waited.take();
                Counts {
                    shapes: std::mem::take(&mut renderer.shapes), painting: std::mem::take(&mut renderer.painting),
                    layers: std::mem::take(&mut renderer.layers_drawn), layer_time: std::mem::take(&mut renderer.layer_time),
                    flushes: std::mem::take(&mut renderer.flushes), gpu, sync,
                }
            }
            Screen::Cpu { renderer, .. } => Counts {
                shapes: std::mem::take(&mut renderer.shapes), painting: std::mem::take(&mut renderer.painting),
                ..Default::default()
            },
        }
    }
}
