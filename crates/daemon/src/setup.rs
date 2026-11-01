//! One-click fixes offered by the setup check in the settings window.
//!
//! [`Fix::command`] is pure and only builds argv lists; [`apply`] runs them.

use crate::paths;
use anyhow::{Context, Result, bail};
use std::path::PathBuf;

pub const EXTENSION_UUID: &str = "swiftswitcher@vipruk.github.io";

/// Runs as root under pkexec. `$1` is a copy of the rule in the user's
/// runtime dir: root cannot read the user's AppImage FUSE mount.
const UDEV_SCRIPT: &str = "install -m 0644 \"$1\" /etc/udev/rules.d/70-swiftswitcher.rules \
    && udevadm control --reload \
    && udevadm trigger --action=change --subsystem-match=input --subsystem-match=misc";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fix {
    /// `gnome-extensions enable`, falling back to `enabled-extensions`.
    EnableExtension,
    /// AppImage only: copy the bundled extension, then enable it.
    InstallExtension,
    /// AppImage only: install the udev rule through pkexec.
    InstallUdevRule,
}

impl Fix {
    /// Whether the change only takes effect after logging out and back in.
    pub fn needs_relogin(self) -> bool {
        matches!(self, Fix::EnableExtension | Fix::InstallExtension)
    }
}

/// Where the fixes read from and write to.
#[derive(Clone, Debug)]
pub struct Ctx {
    pub uuid: String,
    /// The bundled extension directory.
    pub extension_src: PathBuf,
    /// `~/.local/share/gnome-shell/extensions/<uuid>`.
    pub extension_dest: PathBuf,
    /// The bundled udev rule.
    pub rule_src: PathBuf,
    /// `$XDG_RUNTIME_DIR/swiftswitcher-70.rules`.
    pub rule_tmp: PathBuf,
}

impl Ctx {
    pub fn current() -> Ctx {
        let data = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        let runtime = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir);
        Ctx {
            uuid: EXTENSION_UUID.to_string(),
            extension_src: paths::extension_dir(),
            extension_dest: data.join("gnome-shell/extensions").join(EXTENSION_UUID),
            rule_src: paths::udev_rule(),
            rule_tmp: runtime.join("swiftswitcher-70.rules"),
        }
    }
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

fn enable_argv(uuid: &str) -> Vec<String> {
    argv(&["gnome-extensions", "enable", uuid])
}

fn path_str(p: &std::path::Path) -> String {
    p.to_string_lossy().into_owned()
}

impl Fix {
    /// The commands to run, in order.
    pub fn command(&self, ctx: &Ctx) -> Vec<Vec<String>> {
        match self {
            Fix::EnableExtension => vec![enable_argv(&ctx.uuid)],
            Fix::InstallExtension => {
                let dest = path_str(&ctx.extension_dest);
                let parent = ctx
                    .extension_dest
                    .parent()
                    .map(path_str)
                    .unwrap_or_else(|| ".".into());
                vec![
                    // A stale copy or a link into an old FUSE mount.
                    argv(&["rm", "-rf", &dest]),
                    argv(&["mkdir", "-p", &parent]),
                    argv(&["cp", "-r", &path_str(&ctx.extension_src), &dest]),
                    enable_argv(&ctx.uuid),
                ]
            }
            Fix::InstallUdevRule => {
                let tmp = path_str(&ctx.rule_tmp);
                vec![
                    argv(&["cp", &path_str(&ctx.rule_src), &tmp]),
                    argv(&["pkexec", "/bin/sh", "-c", UDEV_SCRIPT, "sh", &tmp]),
                ]
            }
        }
    }
}

/// The new `org.gnome.shell enabled-extensions` value with `uuid` added,
/// or None when it is already there. `current` is `gsettings get` output.
pub fn appended_extensions(current: &str, uuid: &str) -> Option<String> {
    let current = current.trim();
    if current.contains(&format!("'{uuid}'")) {
        return None;
    }
    Some(match current {
        "@as []" | "[]" | "" => format!("['{uuid}']"),
        _ => format!("{}, '{uuid}']", current.trim_end_matches(']')),
    })
}

fn run(argv: &[String]) -> Result<()> {
    let status = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(std::process::Stdio::null())
        .status()
        .with_context(|| format!("starting {}", argv[0]))?;
    if !status.success() {
        bail!("{} failed ({status})", argv[0]);
    }
    Ok(())
}

/// Records the extension in `enabled-extensions`, so GNOME Shell loads it
/// at the next login (the logic of `scripts/dev-setup.sh`).
fn enable_by_gsettings(uuid: &str) -> Result<()> {
    let out = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.shell", "enabled-extensions"])
        .output()
        .context("starting gsettings")?;
    if !out.status.success() {
        bail!("gsettings get failed ({})", out.status);
    }
    let current = String::from_utf8_lossy(&out.stdout);
    match appended_extensions(&current, uuid) {
        Some(new) => run(&[
            "gsettings".into(),
            "set".into(),
            "org.gnome.shell".into(),
            "enabled-extensions".into(),
            new,
        ]),
        None => Ok(()),
    }
}

/// Runs the fix. Blocks (pkexec waits for the password): call it off the
/// GTK thread.
pub fn apply(fix: Fix) -> Result<()> {
    let ctx = Ctx::current();
    let enable = enable_argv(&ctx.uuid);
    for argv in fix.command(&ctx) {
        if argv == enable {
            if let Err(e) = run(&argv) {
                log::info!("{e:#}; adding the extension to enabled-extensions");
                enable_by_gsettings(&ctx.uuid)?;
            }
        } else {
            run(&argv)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn appimage_ctx() -> Ctx {
        Ctx {
            uuid: EXTENSION_UUID.to_string(),
            extension_src: PathBuf::from(
                "/tmp/.mount_SwiftAbc/usr/share/swiftswitcher/extension/swiftswitcher@vipruk.github.io",
            ),
            extension_dest: PathBuf::from(
                "/home/u/.local/share/gnome-shell/extensions/swiftswitcher@vipruk.github.io",
            ),
            rule_src: PathBuf::from(
                "/tmp/.mount_SwiftAbc/usr/share/swiftswitcher/udev/70-swiftswitcher.rules",
            ),
            rule_tmp: PathBuf::from("/run/user/1000/swiftswitcher-70.rules"),
        }
    }

    #[test]
    fn udev_fix_never_runs_appimage_as_root() {
        let appimage = "/home/u/Apps/SwiftSwitcher.AppImage";
        let cmds = Fix::InstallUdevRule.command(&appimage_ctx());
        let root: Vec<_> = cmds.iter().filter(|c| c[0] == "pkexec").collect();
        assert_eq!(root.len(), 1, "{cmds:?}");
        let argv = root[0];
        assert_eq!(&argv[..3], ["pkexec", "/bin/sh", "-c"]);
        assert_eq!(argv[3], UDEV_SCRIPT);
        assert_eq!(&argv[4..], ["sh", "/run/user/1000/swiftswitcher-70.rules"]);
        for a in argv {
            assert!(!a.contains(appimage), "{a}");
            assert!(
                !a.contains(".mount_"),
                "root must not read the FUSE mount: {a}"
            );
        }
        // The copy into the runtime dir comes first and runs as the user.
        assert_eq!(
            cmds[0],
            [
                "cp",
                "/tmp/.mount_SwiftAbc/usr/share/swiftswitcher/udev/70-swiftswitcher.rules",
                "/run/user/1000/swiftswitcher-70.rules",
            ]
        );
        assert_eq!(cmds.last().unwrap()[0], "pkexec");
    }

    #[test]
    fn extension_fixes_enable_last() {
        let ctx = appimage_ctx();
        let enable = ["gnome-extensions", "enable", EXTENSION_UUID];
        assert_eq!(Fix::EnableExtension.command(&ctx), [enable]);
        let cmds = Fix::InstallExtension.command(&ctx);
        assert_eq!(cmds.last().unwrap(), &enable);
        let dest = ctx.extension_dest.to_str().unwrap();
        let src = ctx.extension_src.to_str().unwrap();
        assert!(
            cmds.iter().any(|c| c == &["cp", "-r", src, dest]),
            "{cmds:?}"
        );
        assert!(cmds.iter().all(|c| c[0] != "pkexec" && c[0] != "sudo"));
        assert!(Fix::InstallExtension.needs_relogin());
        assert!(!Fix::InstallUdevRule.needs_relogin());
    }

    #[test]
    fn enable_extension_appends_to_list() {
        let u = EXTENSION_UUID;
        assert_eq!(
            appended_extensions("@as []\n", u).as_deref(),
            Some("['swiftswitcher@vipruk.github.io']")
        );
        assert_eq!(
            appended_extensions("[]", u).as_deref(),
            Some("['swiftswitcher@vipruk.github.io']")
        );
        assert_eq!(
            appended_extensions("['a@b', 'c@d']\n", u).as_deref(),
            Some("['a@b', 'c@d', 'swiftswitcher@vipruk.github.io']")
        );
        assert_eq!(
            appended_extensions("['a@b', 'swiftswitcher@vipruk.github.io']\n", u),
            None
        );
    }
}
