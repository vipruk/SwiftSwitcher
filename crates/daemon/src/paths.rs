//! Locations of the files shipped with the application.
//!
//! Layout under the resource root:
//!
//! ```text
//! icons/                                   icon-theme root (contains hicolor/)
//! extension/swiftswitcher@vipruk.github.io/  GNOME Shell extension
//! udev/70-swiftswitcher.rules              uaccess rule
//! ```
//!
//! The root is `$APPDIR/usr/share/swiftswitcher` in an AppImage,
//! `/usr/share/swiftswitcher` for a system install, and the repository
//! checkout for dev runs. In the repository the same files live at
//! `data/icons`, `extension/swiftswitcher@vipruk.github.io` and
//! `packaging/udev/70-swiftswitcher.rules`.

use std::path::{Path, PathBuf};

const EXT_ID: &str = "swiftswitcher@vipruk.github.io";
const RULE: &str = "70-swiftswitcher.rules";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Root {
    AppImage(PathBuf),
    System,
    Dev,
}

fn choose_root(appdir: Option<&Path>, exe: &Path) -> Root {
    if let Some(a) = appdir {
        Root::AppImage(a.join("usr/share/swiftswitcher"))
    } else if exe.starts_with("/usr/") {
        Root::System
    } else {
        Root::Dev
    }
}

fn root() -> Root {
    let appdir = std::env::var_os("APPDIR").filter(|v| !v.is_empty());
    let exe = std::env::current_exe().unwrap_or_default();
    choose_root(appdir.as_deref().map(Path::new), &exe)
}

fn dev_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

#[allow(dead_code)] // for packaging tasks
pub fn resource_root() -> PathBuf {
    match root() {
        Root::AppImage(p) => p,
        Root::System => PathBuf::from("/usr/share/swiftswitcher"),
        Root::Dev => dev_root(),
    }
}

fn resolve(root: &Root, installed: &str, dev: &str) -> PathBuf {
    match root {
        Root::AppImage(p) => p.join(installed),
        Root::System => Path::new("/usr/share/swiftswitcher").join(installed),
        Root::Dev => dev_root().join(dev),
    }
}

pub fn icons_dir() -> PathBuf {
    resolve(&root(), "icons", "data/icons")
}

pub fn extension_dir() -> PathBuf {
    resolve(
        &root(),
        &format!("extension/{EXT_ID}"),
        &format!("extension/{EXT_ID}"),
    )
}

pub fn udev_rule() -> PathBuf {
    resolve(
        &root(),
        &format!("udev/{RULE}"),
        &format!("packaging/udev/{RULE}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appdir_wins() {
        let r = choose_root(
            Some(Path::new("/tmp/.mount_x")),
            Path::new("/usr/bin/swiftswitcher"),
        );
        assert_eq!(
            r,
            Root::AppImage(PathBuf::from("/tmp/.mount_x/usr/share/swiftswitcher"))
        );
        assert_eq!(
            resolve(&r, "icons", "data/icons"),
            PathBuf::from("/tmp/.mount_x/usr/share/swiftswitcher/icons")
        );
    }

    #[test]
    fn usr_exe_is_system() {
        let r = choose_root(None, Path::new("/usr/bin/swiftswitcher"));
        assert_eq!(r, Root::System);
        assert_eq!(
            resolve(&r, &format!("udev/{RULE}"), "x"),
            PathBuf::from("/usr/share/swiftswitcher/udev/70-swiftswitcher.rules")
        );
    }

    #[test]
    fn other_exe_is_dev_repo() {
        let r = choose_root(None, Path::new("/home/u/repo/target/debug/swiftswitcher"));
        assert_eq!(r, Root::Dev);
        let icons = resolve(&r, "icons", "data/icons");
        assert!(icons.ends_with("data/icons"));
        let rule = udev_rule_dev();
        assert!(rule.exists(), "{}", rule.display());
    }

    fn udev_rule_dev() -> PathBuf {
        resolve(&Root::Dev, "", &format!("packaging/udev/{RULE}"))
    }

    #[test]
    fn dev_extension_path() {
        let p = resolve(&Root::Dev, "", &format!("extension/{EXT_ID}"));
        assert!(p.ends_with("extension/swiftswitcher@vipruk.github.io"));
    }
}
