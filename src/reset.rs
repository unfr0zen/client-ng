//! Stock's Reset › Network and Reset › All Settings (nlresetscreen,
//! 0x19c6cc and 0x19c2ec), run off the UI's thread: the network's saved
//! services and passphrases erased under a stopped connman; or every
//! setting, data and log the unit keeps, then a restart into setup.

use std::path::Path;
use std::process::Command;

/// connman's store on the unit, and the directories All Settings empties.
const CONNMAN: &str = "/nestlabs/etc/user/connman";
const ERASED: [&str; 3] = ["/nestlabs/var/data", "/nestlabs/etc/user", "/nestlabs/var/log"];
/// What All Settings keeps in them: the time zone.
const KEPT: &str = "localtime";

/// Empties a directory, keeping the entries named `keep`.
pub fn clean(dir: &Path, keep: &[&str]) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    debug!("reset: emptying {}", crate::log::path(dir));
    for e in entries.flatten() {
        if keep.iter().any(|k| e.file_name() == *k) {
            continue;
        }
        let path = e.path();
        let result = if path.is_dir() && !path.is_symlink() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        if let Err(err) = result {
            error!("reset: {}: {err}", crate::log::path(&path));
        }
    }
}

fn run(cmd: &str, args: &[&str]) {
    // The desktop's system is the host's: its network, clock and power
    // are left alone.
    if cfg!(feature = "desktop") {
        info!("reset: {cmd} {}: not on the desktop", args.join(" "));
        return;
    }
    debug!("reset: {cmd} {}", args.join(" "));
    match Command::new(cmd).args(args).status() {
        Ok(s) if s.success() => {}
        Ok(s) => error!("reset: {cmd}: {s}"),
        Err(e) => error!("reset: {cmd}: {e}"),
    }
}

/// Network: connman stopped, its store emptied, started again.
pub fn network() {
    run("/etc/init.d/connman", &["stop"]);
    clean(Path::new(CONNMAN), &[]);
    run("/etc/init.d/connman", &["start"]);
}

/// All Settings: connman stopped; data, user settings and logs emptied
/// but the time zone; the client's own settings file gone; the clock at
/// 2000-01-01; then, after `goodbye` has had a moment on screen, a
/// restart, into the first-run setup.
pub fn all(settings: Option<&Path>, goodbye: impl FnOnce()) {
    run("/etc/init.d/connman", &["stop"]);
    for dir in ERASED {
        clean(Path::new(dir), &[KEPT]);
    }
    clean(Path::new(CONNMAN), &[]);
    if let Some(file) = settings {
        let _ = std::fs::remove_file(file);
    }
    run("date", &["-s", "2000-01-01 00:00:00"]);
    run("hwclock", &["-w", "-u"]);
    goodbye();
    std::thread::sleep(std::time::Duration::from_secs(3));
    run("reboot", &[]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleaning_keeps_only_whats_named() {
        let dir = std::env::temp_dir().join(format!("unfr0zen-reset-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        for f in ["localtime", "settings.config", "sub/x"] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        clean(&dir, &[KEPT]);
        let left: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(left, ["localtime"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
