//! The window's pixels through OpenGL, which keeps their alpha: the
//! window is clear round the unit and its shadow is see-through. Each
//! frame lays the chrome (`device::chrome`), the face in its round
//! display, the cover glass, the status light when lit and the frame
//! rate's label over a clear window, all premultiplied.

use std::ffi::CString;
use std::num::NonZeroU32;
use std::rc::Rc;

use glow::HasContext;
use glutin::config::{Config, ConfigTemplateBuilder, GlConfig};
use glutin::context::{ContextApi, ContextAttributesBuilder, NotCurrentGlContext, PossiblyCurrentContext};
use glutin::display::{GetGlDisplay, GlDisplay};
use glutin::surface::{GlSurface, Surface, SwapInterval, WindowSurface};
use glutin_winit::{DisplayBuilder, GlWindow};
use tiny_skia::Pixmap;
use winit::event_loop::ActiveEventLoop;
use winit::raw_window_handle::HasWindowHandle;
use winit::window::{Window, WindowAttributes};

use super::device;

const VERTEX: &str = "
attribute vec2 corner;
uniform vec4 rect;
varying vec2 uv;
void main() {
    uv = corner;
    gl_Position = vec4(mix(rect.xy, rect.zw, corner), 0.0, 1.0);
}";

/// A texture, its premultiplied colours as they are, or inside a circle
/// filling it with its edge softened over `edge` of its radius.
const FRAGMENT: &str = "
uniform sampler2D image;
uniform float edge;
varying vec2 uv;
void main() {
    vec4 c = texture2D(image, uv);
    if (edge > 0.0) {
        c *= clamp((1.0 - length(uv - 0.5) * 2.0) / edge, 0.0, 1.0);
    }
    OUT = c;
}";

pub struct Gl {
    pub window: Rc<Window>,
    context: PossiblyCurrentContext,
    surface: Surface<WindowSurface>,
    gl: glow::Context,
    program: glow::Program,
    rect: Option<glow::UniformLocation>,
    edge: Option<glow::UniformLocation>,
    chrome: glow::Texture,
    cover: glow::Texture,
    face: glow::Texture,
    label: glow::Texture,
    led: glow::Texture,
    led_at: (f32, f32, f32, f32),
    /// Whether the status light is lit.
    pub lit: bool,
    /// The face's side and the label's size and place, as last uploaded.
    face_side: usize,
    label_at: (f32, f32, f32, f32),
    label_text: String,
    /// Pixels a unit of the canvas.
    k: f32,
}

/// The configuration with alpha, see-through where the platform offers it.
fn pick(configs: Box<dyn Iterator<Item = Config> + '_>) -> Config {
    configs.reduce(|a, b| {
        let see_through = |c: &Config| c.supports_transparency().unwrap_or(false);
        if see_through(&b) && !see_through(&a) || see_through(&b) == see_through(&a) && b.alpha_size() > a.alpha_size() { b } else { a }
    }).expect("an OpenGL configuration")
}

unsafe fn texture(gl: &glow::Context) -> Result<glow::Texture, String> {
    let t = gl.create_texture()?;
    gl.bind_texture(glow::TEXTURE_2D, Some(t));
    for (name, value) in [(glow::TEXTURE_MIN_FILTER, glow::LINEAR), (glow::TEXTURE_MAG_FILTER, glow::LINEAR),
                          (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE), (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE)] {
        gl.tex_parameter_i32(glow::TEXTURE_2D, name, value as i32);
    }
    Ok(t)
}

unsafe fn upload(gl: &glow::Context, t: glow::Texture, w: usize, h: usize, rgba: &[u8]) {
    gl.bind_texture(glow::TEXTURE_2D, Some(t));
    gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA as i32, w as i32, h as i32, 0, glow::RGBA, glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(rgba)));
}

unsafe fn upload_pixmap(gl: &glow::Context, t: glow::Texture, pm: &Pixmap) {
    upload(gl, t, pm.width() as usize, pm.height() as usize, pm.data());
}

/// The shaders' headers for the context's GLSL: ES 1.00, 1.20, or 1.50
/// for a core profile (macOS's), which renames the old words.
fn headers(version: &glow::Version) -> (&'static str, &'static str) {
    if version.is_embedded {
        ("#version 100\n", "#version 100\nprecision mediump float;\n#define OUT gl_FragColor\n")
    } else if version.major >= 3 {
        ("#version 150\n#define attribute in\n#define varying out\n",
         "#version 150\n#define varying in\n#define texture2D texture\nout vec4 color_out;\n#define OUT color_out\n")
    } else {
        ("#version 120\n", "#version 120\n#define OUT gl_FragColor\n")
    }
}

unsafe fn compile(gl: &glow::Context) -> Result<glow::Program, String> {
    let (vertex, fragment) = headers(gl.version());
    let program = gl.create_program()?;
    for (kind, header, source) in [(glow::VERTEX_SHADER, vertex, VERTEX), (glow::FRAGMENT_SHADER, fragment, FRAGMENT)] {
        let shader = gl.create_shader(kind)?;
        gl.shader_source(shader, &format!("{header}{source}"));
        gl.compile_shader(shader);
        if !gl.get_shader_compile_status(shader) {
            return Err(gl.get_shader_info_log(shader));
        }
        gl.attach_shader(program, shader);
    }
    gl.bind_attrib_location(program, 0, "corner");
    gl.link_program(program);
    if !gl.get_program_link_status(program) {
        return Err(gl.get_program_info_log(program));
    }
    Ok(program)
}

impl Gl {
    /// Opens the window with `attributes` and draws its chrome to fill it.
    pub fn open(events: &ActiveEventLoop, attributes: WindowAttributes) -> Result<Gl, String> {
        let template = ConfigTemplateBuilder::new().with_alpha_size(8).with_transparency(true);
        let (window, config) = DisplayBuilder::new().with_window_attributes(Some(attributes))
            .build(events, template, pick).map_err(|e| e.to_string())?;
        let window = Rc::new(window.ok_or("no window")?);
        let k = window.inner_size().width as f32 / device::CANVAS;
        let display = config.display();
        let handle = window.window_handle().map_err(|e| e.to_string())?.as_raw();
        // SAFETY: the window outlives the context and the surface, which
        // `Gl` holds beside it.
        let context = unsafe {
            display.create_context(&config, &ContextAttributesBuilder::new().build(Some(handle)))
                .or_else(|e| {
                    warn!("window: OpenGL context: {e}; trying GLES");
                    display.create_context(&config, &ContextAttributesBuilder::new()
                        .with_context_api(ContextApi::Gles(None)).build(Some(handle)))
                })
        }.map_err(|e| e.to_string())?;
        let attributes = window.build_surface_attributes(Default::default()).map_err(|e| e.to_string())?;
        let surface = unsafe { display.create_window_surface(&config, &attributes) }.map_err(|e| e.to_string())?;
        let context = context.make_current(&surface).map_err(|e| e.to_string())?;
        // The client's loop keeps its own pace.
        if let Err(e) = surface.set_swap_interval(&context, SwapInterval::DontWait) {
            debug!("window: swap interval: {e}");
        }
        let gl = unsafe {
            glow::Context::from_loader_function(|s| display.get_proc_address(&CString::new(s).unwrap()).cast())
        };
        let version = gl.version();
        info!("window: OpenGL{} {}.{} ({}), {}-bit alpha, {}", if version.is_embedded { " ES" } else { "" }, version.major,
              version.minor, version.vendor_info, config.alpha_size(),
              if config.supports_transparency().unwrap_or(false) { "see-through" } else { "opaque" });
        unsafe {
            let program = compile(&gl)?;
            // A core profile draws only with a vertex array bound.
            if !gl.version().is_embedded && gl.version().major >= 3 {
                gl.bind_vertex_array(Some(gl.create_vertex_array()?));
            }
            let corners = gl.create_buffer()?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(corners));
            let quad: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &quad.iter().flat_map(|v| v.to_ne_bytes()).collect::<Vec<u8>>(),
                                    glow::STATIC_DRAW);
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 8, 0);
            gl.use_program(Some(program));
            gl.uniform_1_i32(gl.get_uniform_location(program, "image").as_ref(), 0);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            let (chrome, cover, face, label, led) = (texture(&gl)?, texture(&gl)?, texture(&gl)?, texture(&gl)?, texture(&gl)?);
            let started = std::time::Instant::now();
            upload_pixmap(&gl, chrome, &device::chrome(k));
            upload_pixmap(&gl, cover, &device::cover(k));
            let (light, x, y) = device::led(k);
            upload_pixmap(&gl, led, &light);
            debug!("window: chrome drawn at {k:.2} pixels a unit in {:.0} ms", started.elapsed().as_secs_f64() * 1000.0);
            let led_at = (x, y, light.width() as f32 / k, light.height() as f32 / k);
            Ok(Gl {
                rect: gl.get_uniform_location(program, "rect"),
                edge: gl.get_uniform_location(program, "edge"),
                window, context, surface, gl, program, chrome, cover, face, label, led, led_at, lit: false,
                face_side: 0, label_at: (0.0, 0.0, 0.0, 0.0), label_text: String::new(), k,
            })
        }
    }

    /// Draws `t` over canvas units (x, y, w, h), inside its circle when
    /// `edge` is above 0.
    unsafe fn quad(&self, t: glow::Texture, (x, y, w, h): (f32, f32, f32, f32), edge: f32) {
        let clip = |v: f32| v / device::CANVAS * 2.0 - 1.0;
        self.gl.bind_texture(glow::TEXTURE_2D, Some(t));
        self.gl.uniform_4_f32(self.rect.as_ref(), clip(x), -clip(y), clip(x + w), -clip(y + h));
        self.gl.uniform_1_f32(self.edge.as_ref(), edge);
        self.gl.draw_arrays(glow::TRIANGLE_STRIP, 0, 4);
    }

    /// Shows `face`, premultiplied RGBA `side` pixels each way, with `label`
    /// under it; with no face, the last one again.
    pub fn present(&mut self, face: Option<(&[u8], usize)>, label: &str) {
        let size = self.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
        unsafe {
            if let Some((rgba, side)) = face {
                upload(&self.gl, self.face, side, side, rgba);
                self.face_side = side;
            }
            if label != self.label_text {
                let (pm, x, y) = device::label(label, self.k);
                upload_pixmap(&self.gl, self.label, &pm);
                self.label_at = (x, y, pm.width() as f32 / self.k, pm.height() as f32 / self.k);
                self.label_text = label.to_string();
            }
            self.surface.resize(&self.context, w, h);
            self.gl.viewport(0, 0, w.get() as i32, h.get() as i32);
            self.gl.clear_color(0.0, 0.0, 0.0, 0.0);
            self.gl.clear(glow::COLOR_BUFFER_BIT);
            let canvas = (0.0, 0.0, device::CANVAS, device::CANVAS);
            self.quad(self.chrome, canvas, 0.0);
            if self.face_side > 0 {
                let (x, y, s) = device::face();
                self.quad(self.face, (x, y, s, s), 1.5 / (s / 2.0 * self.k));
            }
            self.quad(self.cover, canvas, 0.0);
            if self.lit {
                self.quad(self.led, self.led_at, 0.0);
            }
            if !self.label_text.is_empty() {
                self.quad(self.label, self.label_at, 0.0);
            }
        }
        match self.surface.swap_buffers(&self.context) {
            Ok(()) => trace!("window: presented{}{}", if face.is_some() { " a frame" } else { " again" }, if self.lit { ", light lit" } else { "" }),
            Err(e) => trace!("window: swap: {e}"),
        }
    }
}

impl Drop for Gl {
    fn drop(&mut self) {
        unsafe {
            self.gl.delete_program(self.program);
            for t in [self.chrome, self.cover, self.face, self.label, self.led] {
                self.gl.delete_texture(t);
            }
        }
    }
}
