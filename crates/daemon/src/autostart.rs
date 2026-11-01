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

/// `Exec="<exec>" run`, quoted per the Desktop Entry spec: inside the
/// quotes `"`, `` ` ``, `$` and `\` take a backslash, and the string-value
/// escaping then doubles every backslash once more; `%` becomes `%%`.
fn exec_line(exec: &Path) -> String {
    let mut quoted = String::new();
    for c in exec.to_string_lossy().chars() {
        match c {
            '"' | '`' | '$' => {
                quoted.push('\\');
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
        // Reserved characters are escaped inside the quotes.
        let e = entry(Path::new("/opt/a\"b$c`d\\e%f"));
        assert!(
            e.lines()
                .any(|l| l == "Exec=\"/opt/a\\\"b\\$c\\`d\\\\\\\\e%%f\" run"),
            "{e}"
        );
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
