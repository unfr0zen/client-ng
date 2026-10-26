//! FreeType, as stock nlclient sets its text: loaded at run time (the
//! unit's own /usr/lib/libfreetype.so.6, FreeType 2.6, as the GPU's
//! libraries are), so nothing is linked at build. Only what text setting
//! needs is bound: faces from memory, a size, and a glyph's unhinted
//! coverage, its placement and its advance. The structures are FreeType's
//! public ones up to the fields read, in C's types, so they hold on the
//! units' 32-bit ARM as on a 64-bit host.

use std::ffi::{c_char, c_int, c_long, c_short, c_uint, c_ulong, c_ushort, c_void};
use std::path::PathBuf;
use std::rc::Rc;

use libloading::Library;

type FtError = c_int;
type FtLibrary = *mut c_void;
type FtFace = *mut FaceRec;

const LOAD_NO_HINTING: i32 = 1 << 1;
const LOAD_NO_BITMAP: i32 = 1 << 3;
const RENDER_MODE_NORMAL: c_uint = 0;

#[repr(C)]
struct Generic {
    data: *mut c_void,
    finalizer: *mut c_void,
}

#[repr(C)]
struct FaceRec {
    num_faces: c_long,
    face_index: c_long,
    face_flags: c_long,
    style_flags: c_long,
    num_glyphs: c_long,
    family_name: *mut c_char,
    style_name: *mut c_char,
    num_fixed_sizes: c_int,
    available_sizes: *mut c_void,
    num_charmaps: c_int,
    charmaps: *mut c_void,
    generic: Generic,
    bbox: [c_long; 4],
    units_per_em: c_ushort,
    ascender: c_short,
    descender: c_short,
    height: c_short,
    max_advance_width: c_short,
    max_advance_height: c_short,
    underline_position: c_short,
    underline_thickness: c_short,
    glyph: *mut GlyphSlotRec,
}

#[repr(C)]
struct Bitmap {
    rows: c_uint,
    width: c_uint,
    pitch: c_int,
    buffer: *mut u8,
    num_grays: c_ushort,
    pixel_mode: u8,
    palette_mode: u8,
    palette: *mut c_void,
}

#[repr(C)]
struct GlyphSlotRec {
    library: FtLibrary,
    face: FtFace,
    next: *mut GlyphSlotRec,
    reserved: c_uint,
    generic: Generic,
    metrics: [c_long; 8],
    linear_hori_advance: c_long,
    linear_vert_advance: c_long,
    advance: [c_long; 2],
    format: c_uint,
    bitmap: Bitmap,
    bitmap_left: c_int,
    bitmap_top: c_int,
}

struct Api {
    _lib: Library,
    library: FtLibrary,
    new_memory_face: unsafe extern "C" fn(FtLibrary, *const u8, c_long, c_long, *mut FtFace) -> FtError,
    done_face: unsafe extern "C" fn(FtFace) -> FtError,
    set_char_size: unsafe extern "C" fn(FtFace, c_long, c_long, c_uint, c_uint) -> FtError,
    get_char_index: unsafe extern "C" fn(FtFace, c_ulong) -> c_uint,
    load_glyph: unsafe extern "C" fn(FtFace, c_uint, i32) -> FtError,
    render_glyph: unsafe extern "C" fn(*mut GlyphSlotRec, c_uint) -> FtError,
}

/// Where FreeType is looked for: `$CLIENT_FREETYPE`, the unit's, then (on
/// the desktop) FreeType 2.6 built by tools/freetype.sh and the host's.
fn candidates() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::env::var_os("CLIENT_FREETYPE").map(PathBuf::from).into_iter().collect();
    out.push("libfreetype.so.6".into());
    #[cfg(feature = "desktop")]
    {
        let built = crate::platform::checkout().join(".work/freetype/lib");
        out.extend([built.join("libfreetype.6.dylib"), built.join("libfreetype.so.6")]);
        out.extend(["libfreetype.6.dylib".into(), "/usr/local/lib/libfreetype.6.dylib".into(), "/opt/homebrew/lib/libfreetype.6.dylib".into()]);
    }
    out
}

fn sym<T: Copy>(lib: &Library, name: &[u8]) -> Result<T, String> {
    // SAFETY: each caller declares T as FreeType's signature of `name`.
    unsafe { lib.get::<T>(name) }.map(|s| *s).map_err(|e| format!("{}: {e}", String::from_utf8_lossy(&name[..name.len() - 1])))
}

impl Api {
    fn open() -> Result<Api, String> {
        let mut tried = Vec::new();
        for path in candidates() {
            // SAFETY: FreeType runs no initialisers that depend on us.
            match unsafe { Library::new(&path) } {
                Ok(lib) => {
                    let api = Api::bind(lib)?;
                    info!("text: FreeType from {}", crate::log::path(&path));
                    return Ok(api);
                }
                Err(e) => {
                    debug!("text: {}: {e}", crate::log::path(&path));
                    tried.push(format!("{}: {e}", path.display()));
                }
            }
        }
        Err(format!("no FreeType ({})", tried.join("; ")))
    }

    fn bind(lib: Library) -> Result<Api, String> {
        let init: unsafe extern "C" fn(*mut FtLibrary) -> FtError = sym(&lib, b"FT_Init_FreeType\0")?;
        let mut library = std::ptr::null_mut();
        // SAFETY: FT_Init_FreeType writes the new library's handle.
        if unsafe { init(&mut library) } != 0 {
            return Err("FT_Init_FreeType failed".into());
        }
        Ok(Api {
            new_memory_face: sym(&lib, b"FT_New_Memory_Face\0")?,
            done_face: sym(&lib, b"FT_Done_Face\0")?,
            set_char_size: sym(&lib, b"FT_Set_Char_Size\0")?,
            get_char_index: sym(&lib, b"FT_Get_Char_Index\0")?,
            load_glyph: sym(&lib, b"FT_Load_Glyph\0")?,
            render_glyph: sym(&lib, b"FT_Render_Glyph\0")?,
            library,
            _lib: lib,
        })
    }
}

thread_local! {
    /// FreeType, opened once on the thread that draws; None where it is not.
    static API: Option<Rc<Api>> = Api::open().map_err(|e| warn!("text: {e}; setting text without it")).ok().map(Rc::new);
}

/// A glyph's coverage (`width` by `rows`, a byte a pixel), its top-left
/// relative to its pen on the baseline, and its advance in pixels.
pub struct Glyph {
    pub width: usize,
    pub rows: usize,
    pub left: i32,
    pub top: i32,
    pub advance: f32,
    pub coverage: Vec<u8>,
}

/// A face, parsed from the font file's bytes, which it keeps.
pub struct Face {
    api: Rc<Api>,
    face: FtFace,
    _bytes: Rc<[u8]>,
    /// The size last set, in 64ths of a pixel.
    size: std::cell::Cell<c_long>,
}

impl Face {
    pub fn new(bytes: Rc<[u8]>) -> Option<Face> {
        let api = API.with(Clone::clone)?;
        let mut face = std::ptr::null_mut();
        // SAFETY: the bytes outlive the face, which keeps them.
        let err = unsafe { (api.new_memory_face)(api.library, bytes.as_ptr(), bytes.len() as c_long, 0, &mut face) };
        if err != 0 {
            warn!("text: FT_New_Memory_Face: error {err}; fontdue sets the face");
        }
        (err == 0).then(|| Face { api, face, _bytes: bytes, size: std::cell::Cell::new(0) })
    }

    /// `ch` at `px` pixels, unhinted (FreeType itself rounds the size to
    /// whole pixels for a TrueType face asking for it, as Akkurat does).
    pub fn glyph(&self, ch: char, px: f32) -> Option<Glyph> {
        let size = (px * 64.0).round() as c_long;
        // SAFETY: the face is open; the slot FreeType fills is read before
        // the next call on it.
        unsafe {
            if self.size.get() != size {
                if (self.api.set_char_size)(self.face, 0, size, 72, 72) != 0 {
                    return None;
                }
                self.size.set(size);
            }
            let index = (self.api.get_char_index)(self.face, ch as c_ulong);
            if (self.api.load_glyph)(self.face, index, LOAD_NO_HINTING | LOAD_NO_BITMAP) != 0 {
                return None;
            }
            let slot = (*self.face).glyph;
            let advance = (*slot).advance[0] as f32 / 64.0;
            if (self.api.render_glyph)(slot, RENDER_MODE_NORMAL) != 0 {
                return None;
            }
            let b = &(*slot).bitmap;
            let (rows, width) = (b.rows as usize, b.width as usize);
            let mut coverage = Vec::with_capacity(rows * width);
            for row in 0..rows {
                let line = b.buffer.offset(row as isize * b.pitch as isize);
                coverage.extend_from_slice(std::slice::from_raw_parts(line, width));
            }
            Some(Glyph { width, rows, left: (*slot).bitmap_left, top: (*slot).bitmap_top, advance, coverage })
        }
    }
}

impl Drop for Face {
    fn drop(&mut self) {
        // SAFETY: the face was opened by this library and is dropped once.
        unsafe { (self.api.done_face)(self.face) };
    }
}
