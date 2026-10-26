//! The Linux framebuffer, for units drawn on the CPU: frames are copied
//! into its mapped memory in whatever pixel format the driver reports.

use std::fs::OpenOptions;
use std::os::fd::AsRawFd;

const FBIOGET_VSCREENINFO: libc::c_ulong = 0x4600;
const FBIOGET_FSCREENINFO: libc::c_ulong = 0x4602;
const FBIOPAN_DISPLAY: libc::c_ulong = 0x4606;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Bitfield {
    offset: u32,
    length: u32,
    msb_right: u32,
}

#[repr(C)]
#[derive(Default)]
struct VarScreeninfo {
    xres: u32,
    yres: u32,
    xres_virtual: u32,
    yres_virtual: u32,
    xoffset: u32,
    yoffset: u32,
    bits_per_pixel: u32,
    grayscale: u32,
    red: Bitfield,
    green: Bitfield,
    blue: Bitfield,
    transp: Bitfield,
    rest: [u32; 24],
}

#[repr(C)]
struct FixScreeninfo {
    id: [u8; 16],
    smem_start: libc::c_ulong,
    smem_len: u32,
    kind: u32,
    type_aux: u32,
    visual: u32,
    xpanstep: u16,
    ypanstep: u16,
    ywrapstep: u16,
    line_length: u32,
    rest: [u8; 64],
}

pub struct Framebuffer {
    pub width: u32,
    pub height: u32,
    map: *mut u8,
    len: usize,
    stride: usize,
    bytes: usize,
    /// Each channel's shift and width.
    red: (u32, u32),
    green: (u32, u32),
    blue: (u32, u32),
    _file: std::fs::File,
}

impl Framebuffer {
    pub fn open(path: &str) -> Result<Framebuffer, String> {
        let name = crate::log::path(std::path::Path::new(path));
        let file = OpenOptions::new().read(true).write(true).open(path).map_err(|e| format!("{path}: {e}"))?;
        let fd = file.as_raw_fd();
        let mut var = VarScreeninfo::default();
        // SAFETY: FBIOGET_* fill the structs they are given, laid out as the kernel's.
        let mut fix: FixScreeninfo = unsafe { std::mem::zeroed() };
        // SAFETY: as above.
        if unsafe { libc::ioctl(fd, FBIOGET_VSCREENINFO as _, &mut var) } < 0 || unsafe { libc::ioctl(fd, FBIOGET_FSCREENINFO as _, &mut fix) } < 0 {
            return Err(format!("{path}: {}", std::io::Error::last_os_error()));
        }
        // Frames go to the first page: shown from it, should a previous
        // client have left the panel on another.
        if var.yoffset != 0 || var.xoffset != 0 {
            var.xoffset = 0;
            var.yoffset = 0;
            // SAFETY: as above, with the struct the kernel filled.
            if unsafe { libc::ioctl(fd, FBIOPAN_DISPLAY as _, &mut var) } < 0 {
                warn!("display: {name}: pan to the first page: {}", std::io::Error::last_os_error());
            } else {
                debug!("display: {name} panned to the first page");
            }
        }
        let bytes = (var.bits_per_pixel / 8) as usize;
        if bytes != 2 && bytes != 4 {
            return Err(format!("{path}: {} bits a pixel", var.bits_per_pixel));
        }
        let len = fix.smem_len as usize;
        // SAFETY: maps the framebuffer's memory, its length as the driver reports.
        let map = unsafe { libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0) };
        if map == libc::MAP_FAILED {
            return Err(format!("{path}: mmap: {}", std::io::Error::last_os_error()));
        }
        info!("display: {name} {}x{} ({}x{} virtual), {} bits a pixel, {} bytes a line", var.xres, var.yres,
              var.xres_virtual, var.yres_virtual, var.bits_per_pixel, fix.line_length);
        debug!("display: {name} red {}+{}, green {}+{}, blue {}+{}, {len} bytes mapped", var.red.offset, var.red.length,
               var.green.offset, var.green.length, var.blue.offset, var.blue.length);
        Ok(Framebuffer {
            width: var.xres, height: var.yres, map: map.cast(), len, stride: fix.line_length as usize, bytes,
            red: (var.red.offset, var.red.length), green: (var.green.offset, var.green.length), blue: (var.blue.offset, var.blue.length),
            _file: file,
        })
    }

    /// Copies a frame of premultiplied RGBA, drawn over black, onto the panel.
    pub fn show(&mut self, rgba: &[u8], width: u32) {
        let pack = |v: u8, (shift, bits): (u32, u32)| (u32::from(v) >> (8 - bits.min(8))) << shift;
        let rows = self.height.min((rgba.len() / 4 / width.max(1) as usize) as u32);
        for y in 0..rows as usize {
            let line = &rgba[y * width as usize * 4..][..width.min(self.width) as usize * 4];
            let start = y * self.stride;
            if start + line.len() / 4 * self.bytes > self.len {
                break;
            }
            // SAFETY: the row lies inside the mapping, checked above.
            let out = unsafe { std::slice::from_raw_parts_mut(self.map.add(start), line.len() / 4 * self.bytes) };
            for (px, o) in line.as_chunks::<4>().0.iter().zip(out.chunks_exact_mut(self.bytes)) {
                let v = pack(px[0], self.red) | pack(px[1], self.green) | pack(px[2], self.blue);
                o.copy_from_slice(&v.to_le_bytes()[..self.bytes]);
            }
        }
    }
}

impl Drop for Framebuffer {
    fn drop(&mut self) {
        // SAFETY: unmaps what open mapped.
        unsafe { libc::munmap(self.map.cast(), self.len) };
    }
}
