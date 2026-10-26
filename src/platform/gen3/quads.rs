//! Textured shapes in batches: the cached images most of a frame is made of
//! (layers, symbols, runs of text), drawn with one small program and one
//! GL draw for each run of the same texture and tint. femtovg draws each of
//! its shapes on its own, with a deal of state for each, which on the
//! unit's driver costs more than the pixels. The tint is a uniform, not a
//! varying: each input a pixel interpolates costs the GPU, and nest-qemu's
//! model of it most.

use glow::HasContext;

/// A vertex: its place in panel pixels and its texture coordinates (u·q,
/// v·q, q: q is 1 but in perspective).
const FLOATS: usize = 5;

const VERTEX: &str = "
attribute vec2 a_pos;
attribute vec3 a_uv;
uniform vec2 u_view;
varying vec3 v_uv;
void main() {
    v_uv = a_uv;
    gl_Position = vec4(2.0 * a_pos.x / u_view.x - 1.0, 1.0 - 2.0 * a_pos.y / u_view.y, 0.0, 1.0);
}";

const FRAGMENT: &str = "
precision mediump float;
uniform sampler2D u_tex;
uniform vec4 u_color;
varying vec3 v_uv;
void main() {
    gl_FragColor = texture2DProj(u_tex, v_uv) * u_color;
}";

/// A run of vertices drawn with one texture and tint: whether it blends
/// (an opaque image need not), its first vertex and count.
struct Batch {
    texture: glow::Texture,
    rgba: [f32; 4],
    blend: bool,
    first: usize,
    count: usize,
}

pub struct Quads {
    gl: glow::Context,
    program: glow::Program,
    buffer: glow::Buffer,
    view: Option<glow::UniformLocation>,
    texture: Option<glow::UniformLocation>,
    color: Option<glow::UniformLocation>,
    vertices: Vec<f32>,
    batches: Vec<Batch>,
    /// The texture last cleared by clear_texture, and the framebuffer
    /// it is cleared through.
    cleared: Option<(glow::Texture, glow::Framebuffer)>,
}

impl Quads {
    /// # Safety
    /// The GL context `gl` loads from must be current, now and when drawing.
    pub unsafe fn new(gl: glow::Context) -> Result<Quads, String> {
        let compile = |kind: u32, source: &str| -> Result<glow::Shader, String> {
            let shader = gl.create_shader(kind)?;
            gl.shader_source(shader, source);
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                return Err(gl.get_shader_info_log(shader));
            }
            Ok(shader)
        };
        let (vs, fs) = (compile(glow::VERTEX_SHADER, VERTEX)?, compile(glow::FRAGMENT_SHADER, FRAGMENT)?);
        let program = gl.create_program()?;
        gl.attach_shader(program, vs);
        gl.attach_shader(program, fs);
        for (i, name) in ["a_pos", "a_uv"].iter().enumerate() {
            gl.bind_attrib_location(program, i as u32, name);
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            return Err(gl.get_program_info_log(program));
        }
        gl.delete_shader(vs);
        gl.delete_shader(fs);
        let buffer = gl.create_buffer()?;
        let view = gl.get_uniform_location(program, "u_view");
        let texture = gl.get_uniform_location(program, "u_tex");
        let color = gl.get_uniform_location(program, "u_color");
        Ok(Quads { gl, program, buffer, view, texture, color, vertices: Vec::new(), batches: Vec::new(), cleared: None })
    }

    pub fn is_empty(&self) -> bool {
        self.batches.is_empty()
    }

    /// Clears the bound framebuffer's colour, all of it: a clear the GPU
    /// can make by marking its tiles, where one with a scissor or the
    /// stencil writes every pixel.
    ///
    /// # Safety
    /// The context must be current.
    pub unsafe fn clear(&self, rgba: [f32; 4]) {
        self.clear_bound(rgba);
    }

    /// Clears all of `texture` the same way, through a framebuffer of
    /// its own kept for the next time, and leaves it bound.
    ///
    /// # Safety
    /// The context must be current and `texture` alive.
    pub unsafe fn clear_texture(&mut self, texture: glow::Texture, rgba: [f32; 4]) {
        let fbo = match self.cleared {
            Some((t, fbo)) if t == texture => fbo,
            old => {
                if let Some((_, fbo)) = old { self.gl.delete_framebuffer(fbo); }
                let Ok(fbo) = self.gl.create_framebuffer() else { return };
                self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
                self.gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(texture), 0);
                self.cleared = Some((texture, fbo));
                fbo
            }
        };
        self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        self.clear_bound(rgba);
    }

    unsafe fn clear_bound(&self, rgba: [f32; 4]) {
        self.gl.disable(glow::SCISSOR_TEST);
        self.gl.clear_color(rgba[0], rgba[1], rgba[2], rgba[3]);
        self.gl.clear(glow::COLOR_BUFFER_BIT);
    }

    /// Draws all of `texture`, upside down as femtovg draws into its
    /// images, over the panel, `width` by `height` pixels, opaque. All
    /// of the square, corners too: the unit's driver writes into the page
    /// on show when a frame leaves any of the window unwritten.
    ///
    /// # Safety
    /// The context must be current and `texture` alive.
    pub unsafe fn present(&mut self, texture: glow::Texture, width: f32, height: f32) {
        self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        self.push(texture, &[[0.0, 0.0, 0.0, 1.0], [width, 0.0, 1.0, 1.0], [width, height, 1.0, 0.0], [0.0, height, 0.0, 0.0]],
                  [1.0, 1.0, 1.0, 1.0], false);
        self.draw(width as u32, height as u32);
    }

    /// A convex polygon of `texture`, its corners as (x, y, u, v) in panel
    /// pixels and texture coordinates, multiplied by the premultiplied
    /// colour `rgba`; `blend` is false where the result is opaque.
    pub fn push(&mut self, texture: glow::Texture, corners: &[[f32; 4]], rgba: [f32; 4], blend: bool) {
        let mut points = [[0f32; 5]; 16];
        let n = corners.len().min(points.len());
        for (point, &[x, y, u, v]) in points.iter_mut().zip(corners) {
            *point = [x, y, u, v, 1.0];
        }
        self.push_projective(texture, &points[..n], rgba, blend);
    }

    /// As `push`, its corners (x, y, u·q, v·q, q): sampled in perspective.
    pub fn push_projective(&mut self, texture: glow::Texture, corners: &[[f32; 5]], rgba: [f32; 4], blend: bool) {
        if corners.len() < 3 {
            return;
        }
        let first = self.vertices.len() / FLOATS;
        for i in 1..corners.len() - 1 {
            for c in [corners[0], corners[i], corners[i + 1]] {
                self.vertices.extend_from_slice(&c);
            }
        }
        let count = 3 * (corners.len() - 2);
        match self.batches.last_mut() {
            Some(b) if b.texture == texture && b.rgba == rgba && b.blend == blend => b.count += count,
            _ => self.batches.push(Batch { texture, rgba, blend, first, count }),
        }
    }

    /// Draws what was pushed onto the bound framebuffer, `width` by
    /// `height` pixels, and forgets it.
    ///
    /// # Safety
    /// The context must be current, and every texture pushed still alive.
    pub unsafe fn draw(&mut self, width: u32, height: u32) {
        if self.batches.is_empty() {
            return;
        }
        let gl = &self.gl;
        gl.viewport(0, 0, width as i32, height as i32);
        gl.use_program(Some(self.program));
        gl.disable(glow::CULL_FACE);
        gl.disable(glow::STENCIL_TEST);
        gl.disable(glow::SCISSOR_TEST);
        gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
        gl.uniform_2_f32(self.view.as_ref(), width as f32, height as f32);
        gl.uniform_1_i32(self.texture.as_ref(), 0);
        gl.active_texture(glow::TEXTURE0);
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.buffer));
        let bytes = std::slice::from_raw_parts(self.vertices.as_ptr().cast::<u8>(), self.vertices.len() * 4);
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STREAM_DRAW);
        let stride = (FLOATS * 4) as i32;
        for (index, (size, offset)) in [(2, 0), (3, 2)].into_iter().enumerate() {
            gl.enable_vertex_attrib_array(index as u32);
            gl.vertex_attrib_pointer_f32(index as u32, size, glow::FLOAT, false, stride, offset * 4);
        }
        let mut blending = None;
        for b in &self.batches {
            if blending != Some(b.blend) {
                if b.blend { gl.enable(glow::BLEND) } else { gl.disable(glow::BLEND) }
                blending = Some(b.blend);
            }
            gl.uniform_4_f32_slice(self.color.as_ref(), &b.rgba);
            gl.bind_texture(glow::TEXTURE_2D, Some(b.texture));
            gl.draw_arrays(glow::TRIANGLES, b.first as i32, b.count as i32);
        }
        gl.enable(glow::BLEND);
        for index in 0..2 {
            gl.disable_vertex_attrib_array(index);
        }
        gl.bind_buffer(glow::ARRAY_BUFFER, None);
        gl.bind_texture(glow::TEXTURE_2D, None);
        gl.use_program(None);
        self.vertices.clear();
        self.batches.clear();
    }
}
