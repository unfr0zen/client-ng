//! No system bus: on the desktop there is no nlheartbeatd to answer and no
//! nlsleepd to hold the host awake for.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sleep {
    pub sleeping: bool,
    pub woke: bool,
}

pub struct Bus;

impl Bus {
    pub fn open(_unsolicited: Option<std::time::Duration>) -> Result<Bus, String> {
        Err("none on the desktop".into())
    }

    pub fn hold(&mut self, _awake: bool) {}

    pub fn tick(&mut self, _dark: bool) -> Sleep {
        Sleep::default()
    }
}
