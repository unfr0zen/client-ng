//! connman as nest-qemu's guest answers: its Wi-Fi powered and its
//! networks (QEMU, the one it is on, and three others), recorded from
//! `dbus-send --print-reply`, so the desktop shows the Wi-Fi state stock
//! does there. What would change the network does nothing.

/// The reply to a `dbus-send` to net.connman with `args`.
pub fn reply(args: &[&str]) -> Result<String, String> {
    match args.last().copied() {
        Some("net.connman.Manager.GetServices") => Ok(include_str!("connman/GetServices.txt").into()),
        Some("net.connman.Manager.GetTechnologies") => Ok(include_str!("connman/GetTechnologies.txt").into()),
        _ => {
            let call: Vec<&str> = args.iter().copied().filter(|a| !a.starts_with("--")).collect();
            debug!("wifi: {} does nothing on the desktop", call.join(" "));
            Ok(String::new())
        }
    }
}
