//! No backlight: the host's own is left alone. Brightness levels are the
//! unit's, for the menus.

pub use crate::platform::backlight_level::level;

pub struct Backlight;

impl Backlight {
    pub fn open() -> Result<Backlight, String> {
        Err("none on the desktop".into())
    }

    pub fn level(&mut self, _share: f32, _lit: bool) {}

    pub fn set(&mut self, _on: bool) {}
}
