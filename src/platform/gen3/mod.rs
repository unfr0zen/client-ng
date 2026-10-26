//! The gen 3 (i.MX6 SoloX, Vivante GPU): drawn through EGL and GLES2.

pub mod quads;
pub mod render;
#[cfg(target_os = "linux")]
pub mod egl;

/// The backplate's MCU.
pub const BACKPLATE: &str = "/dev/ttysensormcu0";
/// Its detect and reset lines, when product.config does not name them.
pub const BACKPLATE_GPIOS: &str = "/sys/bus/platform/devices/0.sensor-mcu-gpios";
/// The ring's axis (REL_DIAL), about 120 counts a detent, negative
/// clockwise; the counts the client works in, so scaled by 1/1.
pub const RING: (u16, i32, i32) = (0x07, 1, 1);
/// The button (KEY_ENTER, `gpio-keys`).
pub const BUTTON: u16 = 28;

/// The battery's voltage, as stock reads it: the first supply of type
/// Battery under `/sys/class/power_supply`; 0 when there is none.
pub fn battery() -> f32 {
    let Ok(dir) = std::fs::read_dir("/sys/class/power_supply") else { return 0.0 };
    let mut supplies: Vec<_> = dir.flatten().map(|e| e.path()).collect();
    supplies.sort();
    supplies.into_iter()
        .find(|p| std::fs::read_to_string(p.join("type")).is_ok_and(|t| t.trim() == "Battery"))
        .and_then(|p| std::fs::read_to_string(p.join("voltage_now")).ok())
        .and_then(|v| v.trim().parse::<f32>().ok())
        .map_or(0.0, |uv| uv / 1e6)
}
