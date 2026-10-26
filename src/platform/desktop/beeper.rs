//! No piezo on the desktop: the client stays silent.

pub struct Beeper;

impl Beeper {
    pub fn open() -> Result<Beeper, String> {
        Err("no piezo on the desktop".into())
    }

    pub fn click(&mut self) {}
}
