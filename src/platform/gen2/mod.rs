//! The gen 2 (TI AM3703, no GPU, 64 MB): drawn on the CPU into the
//! framebuffer.

pub mod soft;
#[cfg(target_os = "linux")]
pub mod fb;

/// Its /proc/cpuinfo Hardware line.
pub const HARDWARE: &str = "Nest J49";
/// The backplate, on the AM3703's UART3.
pub const BACKPLATE: &str = "/dev/ttyO2";
/// Its detect (gpio65) and reset (gpio104) lines, when product.config's
/// `backplatedevice` does not name them.
pub const BACKPLATE_GPIOS: &str = "/sys/devices/platform/diamond-backplate";
/// The ring (`avago-adbs-a330`, REL_X): about 2400 counts a turn, 100 a
/// detent, negative clockwise as the gen 3's; scaled by 6/5 to the gen
/// 3's 120, so a menu row takes the 200 counts stock's takes.
pub const RING: (u16, i32, i32) = (0x00, 6, 5);
/// The button (`twl4030_pwrbutton`, KEY_POWER).
pub const BUTTON: u16 = 116;
/// nlheartbeatd takes the UI's heartbeats unsolicited, every this often
/// (its plist's Period).
pub const HEARTBEAT: std::time::Duration = std::time::Duration::from_secs(5);

/// The battery, in volts: the TWL4030's ADCIN0 behind a divide-by-three
/// (stock's nlReadBatteryLevel), 0 when it cannot be read.
pub fn battery() -> f32 {
    std::fs::read_to_string("/sys/devices/platform/twl4030_madc_hwmon.0/in0_battery_type").ok()
        .and_then(|mv| mv.trim().parse::<f32>().ok())
        .map_or(0.0, |mv| 3.0 * mv / 1000.0)
}
