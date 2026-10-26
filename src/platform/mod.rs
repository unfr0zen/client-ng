//! The unit's display, input devices and piezo, and what differs between
//! the units the client runs on: each has its own folder, and one binary
//! serves both, the unit detected at start.

pub mod gen2;
pub mod gen3;

#[cfg(all(target_os = "linux", not(feature = "desktop")))]
pub mod backlight;
#[cfg(any(target_os = "linux", feature = "desktop"))]
pub mod backplate;
#[cfg(all(target_os = "linux", not(feature = "desktop")))]
pub mod beeper;
#[cfg(all(target_os = "linux", not(feature = "desktop")))]
pub mod bus;
#[cfg(all(target_os = "linux", not(feature = "desktop")))]
pub mod input;
#[cfg(all(target_os = "linux", not(feature = "desktop")))]
pub mod screen;

// The desktop build's window, ring, button and stand-ins, under the unit's
// modules' names.
#[cfg(feature = "desktop")]
pub mod desktop;
#[cfg(feature = "desktop")]
pub use desktop::{backlight, beeper, bus, input, screen};
// The unit's brightness levels, which the desktop's menus still offer.
#[cfg(feature = "desktop")]
#[path = "backlight.rs"]
mod backlight_level;

/// The desktop build's checkout: the folder above its binary holding its
/// Cargo.toml and skins, as cargo's target folders sit in it; else the
/// working directory. Found as it runs, so no build's path is kept in it.
#[cfg(feature = "desktop")]
pub fn checkout() -> std::path::PathBuf {
    std::env::current_exe().ok()
        .and_then(|exe| exe.ancestors().find(|d| d.join("Cargo.toml").is_file() && d.join("skins").is_dir()).map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

/// The unit's shared files (`/nestlabs/share`: its fonts, pictures and
/// strings). The desktop build reads a copy of nest-qemu's guest's
/// (`tools/nest.py unit-files`) at `$NEST_UNIT_SHARE`, else
/// `.work/unit/share` in this repository; they are not ours to ship.
/// Tests keep the unit's path, so the host's copy does not change them.
pub fn share() -> std::path::PathBuf {
    #[cfg(all(feature = "desktop", not(test)))]
    return std::env::var_os("NEST_UNIT_SHARE").map(std::path::PathBuf::from)
        .unwrap_or_else(|| checkout().join(".work/unit/share"));
    #[cfg(not(all(feature = "desktop", not(test))))]
    std::path::PathBuf::from("/nestlabs/share")
}

/// How far below the rest the render and input threads' nice value sits:
/// they draw and read the ring, so they go first when the unit's one core
/// is wanted by the backplate, the cloud link, the settings writer or
/// another daemon.
const UI_NICE: libc::c_int = -10;

/// Gives the calling thread (the render loop's, the input reader's)
/// priority over every other thread and process. Threads started from
/// it afterwards inherit it unless they `relax`.
pub fn prioritize() {
    // SAFETY: setpriority on the calling thread (Linux takes 0 as it).
    if unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, UI_NICE) } == 0 {
        debug!("{}: at nice {UI_NICE}", std::thread::current().name().unwrap_or("thread"));
    } else {
        debug!("{}: keeps its priority: {}", std::thread::current().name().unwrap_or("thread"), std::io::Error::last_os_error());
    }
}

/// Puts the calling thread, started from the render loop, back among the
/// rest.
pub fn relax() {
    // SAFETY: as above.
    unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, 0) };
}

/// The unit the client runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Gen3,
    Gen2,
}

impl Device {
    /// The unit, detected once.
    pub fn current() -> Device {
        static DEVICE: std::sync::OnceLock<Device> = std::sync::OnceLock::new();
        *DEVICE.get_or_init(Device::detect)
    }

    /// The gen 2 by its Hardware line, else the gen 3.
    fn detect() -> Device {
        let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        let hardware = cpuinfo.lines().find(|l| l.starts_with("Hardware"));
        debug!("device: /proc/cpuinfo {}", hardware.unwrap_or("has no Hardware line"));
        if hardware.is_some_and(|l| l.contains(gen2::HARDWARE)) { Device::Gen2 } else { Device::Gen3 }
    }

    pub fn backplate(self) -> &'static str {
        match self {
            Device::Gen3 => gen3::BACKPLATE,
            Device::Gen2 => gen2::BACKPLATE,
        }
    }

    /// The backplate's detect and reset lines' device.
    pub fn backplate_gpios(self) -> &'static str {
        match self {
            Device::Gen3 => gen3::BACKPLATE_GPIOS,
            Device::Gen2 => gen2::BACKPLATE_GPIOS,
        }
    }

    /// What its stock controller asks of the backplate.
    pub fn backplate_profile(self) -> crate::backplate::link::Profile {
        match self {
            Device::Gen3 => crate::backplate::link::Profile::GEN3,
            Device::Gen2 => crate::backplate::link::Profile::GEN2,
        }
    }

    /// The ring's axis, and the ratio (multiplier, divisor) that makes its
    /// counts the gen 3's.
    pub fn ring(self) -> (u16, i32, i32) {
        match self {
            Device::Gen3 => gen3::RING,
            Device::Gen2 => gen2::RING,
        }
    }

    pub fn button(self) -> u16 {
        match self {
            Device::Gen3 => gen3::BUTTON,
            Device::Gen2 => gen2::BUTTON,
        }
    }

    /// Where nlheartbeatd takes the UI's heartbeats unsolicited, how often
    /// to send them; else it calls the UI.
    pub fn heartbeat(self) -> Option<std::time::Duration> {
        match self {
            Device::Gen3 => None,
            Device::Gen2 => Some(gen2::HEARTBEAT),
        }
    }

    /// The battery, in volts (0 when it cannot be read).
    pub fn battery(self) -> f32 {
        match self {
            Device::Gen3 => gen3::battery(),
            Device::Gen2 => gen2::battery(),
        }
    }

    /// Whether it draws through a GPU.
    pub fn gpu(self) -> bool {
        self == Device::Gen3
    }
}
