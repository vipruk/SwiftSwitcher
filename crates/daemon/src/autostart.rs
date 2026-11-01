//! Start on login: the XDG autostart entry
//! `~/.config/autostart/io.github.vipruk.SwiftSwitcher.desktop`.

use std::io;
use std::path::{Path, PathBuf};

const FILE: &str = "io.github.vipruk.SwiftSwitcher.desktop";

/// The program to start on login: the AppImage itself when running from
/// one (its FUSE mount path changes every run), else this executable.
pub fn exec_path() -> PathBuf {
    match std::env::var_os("APPIMAGE").filter(|v| !v.is_empty()) {
        Some(p) => PathBuf::from(p),
        None => std::env::current_exe().unwrap_or_else(|_| PathBuf::from("swiftswitcher")),
    }
}

/// `~/.config/autostart`.
pub fn autostart_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("autostart")
}

/// `$XDG_STATE_HOME/swiftswitcher/autostart-initialized` (falling back to
/// `~/.local/state`): present once autostart has been turned on by default.
pub fn marker_path() -> PathBuf {
    dirs::state_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local").join("state")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("swiftswitcher")
        .join("autostart-initialized")
}

/// Start on login is on by default: the first time (no `marker`), an entry
/// is written unless one exists, then the marker. Once the marker exists
/// nothing changes, so turning the switch off later is respected.
/// Returns whether an entry was written.
pub fn init_default(dir: &Path, marker: &Path, exec: &Path) -> io::Result<bool> {
    if marker.exists() {
        return Ok(false);
    }
    let write = !is_enabled(dir);
    if write {
        set_enabled(dir, exec, true)?;
    }
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(marker, "")?;
    Ok(write)
}

/// `Exec="<exec>" run`, quoted per the Desktop Entry spec: inside the
/// quotes `"`, `` ` ``, `$` and `\` take a backslash, and the string-value
/// escaping (undone first by readers) doubles every backslash once more, so
/// `$` is written `\\$` and `\` is written `\\\\`; `%` becomes `%%`.
fn exec_line(exec: &Path) -> String {
    let mut quoted = String::new();
    for c in exec.to_string_lossy().chars() {
        match c {
            '"' | '`' | '$' => {
                quoted.push_str("\\\\");
                quoted.push(c);
            }
            '\\' => quoted.push_str("\\\\\\\\"),
            '%' => quoted.push_str("%%"),
            _ => quoted.push(c),
        }
    }
    format!("Exec=\"{quoted}\" run")
}

/// The desktop entry that starts `<exec> run`.
pub fn entry(exec: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=SwiftSwitcher\n\
         {}\n\
         Icon=io.github.vipruk.SwiftSwitcher\n\
         NoDisplay=true\n\
         X-GNOME-Autostart-enabled=true\n",
        exec_line(exec)
    )
}

pub fn is_enabled(dir: &Path) -> bool {
    dir.join(FILE).is_file()
}

/// Writes (atomically) or removes the entry.
pub fn set_enabled(dir: &Path, exec: &Path, on: bool) -> io::Result<()> {
    let path = dir.join(FILE);
    if on {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!("{FILE}.tmp"));
        std::fs::write(&tmp, entry(exec))?;
        std::fs::rename(&tmp, &path)
    } else {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            r => r,
        }
    }
}

/// True when an entry exists but starts something other than `exec`.
pub fn is_stale(dir: &Path, exec: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(dir.join(FILE)) else {
        return false;
    };
    let want = exec_line(exec);
    !text.lines().any(|l| l == want)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_quotes_exec() {
        let e = entry(Path::new("/home/u/My Apps/SwiftSwitcher.AppImage"));
        assert!(e.starts_with("[Desktop Entry]\n"), "{e}");
        for line in [
            "Type=Application",
            "Name=SwiftSwitcher",
            "Exec=\"/home/u/My Apps/SwiftSwitcher.AppImage\" run",
            "Icon=io.github.vipruk.SwiftSwitcher",
            "NoDisplay=true",
            "X-GNOME-Autostart-enabled=true",
        ] {
            assert!(e.lines().any(|l| l == line), "missing {line:?} in {e}");
        }
        // Inside the quotes `"`, `` ` ``, `$` and `\` take a backslash; the
        // string-value escaping then doubles every backslash (the spec's own
        // example writes `\\$`). `%` is a field code prefix.
        let e = entry(Path::new(r#"/home/u/My Apps/a"b$c`d\e%f"#));
        let want = r#"Exec="/home/u/My Apps/a\\"b\\$c\\`d\\\\e%%f" run"#;
        assert!(e.lines().any(|l| l == want), "{e}");
    }

    #[test]
    fn set_enabled_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let auto = dir.path().join("autostart");
        let exec = Path::new("/usr/bin/swiftswitcher");
        assert!(!is_enabled(&auto));
        set_enabled(&auto, exec, true).unwrap();
        assert!(is_enabled(&auto));
        assert_eq!(
            std::fs::read_to_string(auto.join(FILE)).unwrap(),
            entry(exec)
        );
        assert!(!auto.join(format!("{FILE}.tmp")).exists());
        set_enabled(&auto, exec, false).unwrap();
        assert!(!is_enabled(&auto));
        // Disabling twice is fine.
        set_enabled(&auto, exec, false).unwrap();
    }

    #[test]
    fn first_open_enables_once() {
        let dir = tempfile::tempdir().unwrap();
        let auto = dir.path().join("autostart");
        let marker = dir.path().join("state/swiftswitcher/autostart-initialized");
        let exec = Path::new("/usr/bin/swiftswitcher");
        // No marker, no entry: enable and write the marker.
        assert!(init_default(&auto, &marker, exec).unwrap());
        assert!(is_enabled(&auto));
        assert!(marker.is_file());
        // The user turns it off; the marker keeps it off.
        set_enabled(&auto, exec, false).unwrap();
        assert!(!init_default(&auto, &marker, exec).unwrap());
        assert!(!is_enabled(&auto));
    }

    #[test]
    fn first_open_keeps_existing_entry() {
        let dir = tempfile::tempdir().unwrap();
        let auto = dir.path().join("autostart");
        let marker = dir.path().join("autostart-initialized");
        let old = Path::new("/opt/old/swiftswitcher");
        set_enabled(&auto, old, true).unwrap();
        assert!(!init_default(&auto, &marker, Path::new("/usr/bin/swiftswitcher")).unwrap());
        assert!(!is_stale(&auto, old), "the existing entry is not rewritten");
        assert!(marker.is_file());
    }

    #[test]
    fn stale_exec_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        let old = Path::new("/home/u/Downloads/SwiftSwitcher-0.1.AppImage");
        let new = Path::new("/home/u/Apps/SwiftSwitcher-0.2.AppImage");
        assert!(!is_stale(dir.path(), new), "no entry is not stale");
        set_enabled(dir.path(), old, true).unwrap();
        assert!(is_stale(dir.path(), new));
        assert!(!is_stale(dir.path(), old));
        set_enabled(dir.path(), new, true).unwrap();
        assert!(!is_stale(dir.path(), new));
    }
}
