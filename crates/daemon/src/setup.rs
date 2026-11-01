//! One-click fixes offered by the setup check in the settings window.
//!
//! [`Fix::command`] is pure and only builds argv lists; [`apply`] runs them.

use crate::paths;
use anyhow::{Context, Result, bail};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

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
    /// `$XDG_RUNTIME_DIR/swiftswitcher-70.rules`, or None when there is no
    /// private runtime dir: root must not read a file others can swap.
    pub rule_tmp: Option<PathBuf>,
}

impl Ctx {
    pub fn current() -> Ctx {
        let data = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        let runtime = private_runtime_dir(std::env::var_os("XDG_RUNTIME_DIR").as_deref());
        Ctx {
            uuid: EXTENSION_UUID.to_string(),
            extension_src: paths::extension_dir(),
            extension_dest: data.join("gnome-shell/extensions").join(EXTENSION_UUID),
            rule_src: paths::udev_rule(),
            rule_tmp: runtime.map(|r| r.join("swiftswitcher-70.rules")),
        }
    }
}

/// `dir` if it is an absolute, real (not symlinked) directory owned by this
/// user with no group or other permissions.
pub fn private_runtime_dir(dir: Option<&OsStr>) -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    let dir = Path::new(dir.filter(|d| !d.is_empty())?);
    if !dir.is_absolute() {
        return None;
    }
    let meta = std::fs::symlink_metadata(dir).ok()?;
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    let private = meta.is_dir() && meta.uid() == uid && meta.mode() & 0o077 == 0;
    private.then(|| dir.to_path_buf())
}

/// Copies the rule to `tmp` as a new 0600 file. Removes whatever is at
/// `tmp` first, without following it.
fn stage_rule(src: &Path, tmp: &Path) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let rule = std::fs::read(src).with_context(|| format!("reading {}", src.display()))?;
    match std::fs::remove_file(tmp) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(e).with_context(|| format!("removing {}", tmp.display()));
        }
        _ => {}
    }
    // O_EXCL: fails rather than follows anything created in between.
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(tmp)
        .with_context(|| format!("creating {}", tmp.display()))?;
    f.write_all(&rule)
        .with_context(|| format!("writing {}", tmp.display()))
}

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

fn enable_argv(uuid: &str) -> Vec<String> {
    argv(&["gnome-extensions", "enable", uuid])
}

fn udev_tmp(ctx: &Ctx) -> Result<&Path> {
    match &ctx.rule_tmp {
        Some(t) => Ok(t),
        None => bail!(
            "XDG_RUNTIME_DIR is unset or not a private directory owned by you; \
             refusing to stage the udev rule for root elsewhere"
        ),
    }
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

impl Fix {
    /// The commands to run, in order. For the udev rule, [`apply`] first
    /// stages the rule at `ctx.rule_tmp`; without a private runtime dir that
    /// fix is refused.
    pub fn command(&self, ctx: &Ctx) -> Result<Vec<Vec<String>>> {
        Ok(match self {
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
                let tmp = path_str(udev_tmp(ctx)?);
                vec![argv(&["pkexec", "/bin/sh", "-c", UDEV_SCRIPT, "sh", &tmp])]
            }
        })
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
    let commands = fix.command(&ctx)?;
    if fix == Fix::InstallUdevRule {
        let tmp = udev_tmp(&ctx)?;
        stage_rule(&ctx.rule_src, tmp)?;
        let result = commands.iter().try_for_each(|argv| run(argv));
        if let Err(e) = std::fs::remove_file(tmp) {
            log::warn!("cannot remove {}: {e}", tmp.display());
        }
        return result;
    }
    let enable = enable_argv(&ctx.uuid);
    for argv in commands {
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
            rule_tmp: Some(PathBuf::from("/run/user/1000/swiftswitcher-70.rules")),
        }
    }

    #[test]
    fn udev_fix_never_runs_appimage_as_root() {
        let appimage = "/home/u/Apps/SwiftSwitcher.AppImage";
        let cmds = Fix::InstallUdevRule.command(&appimage_ctx()).unwrap();
        // The rule is staged by apply(), as the user; root runs one command.
        assert_eq!(cmds.len(), 1, "{cmds:?}");
        let argv = &cmds[0];
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
    }

    #[test]
    fn udev_fix_refused_without_private_runtime_dir() {
        let ctx = Ctx {
            rule_tmp: None,
            ..appimage_ctx()
        };
        let e = Fix::InstallUdevRule.command(&ctx).unwrap_err().to_string();
        assert!(e.contains("XDG_RUNTIME_DIR"), "{e}");
        // The other fixes do not need it.
        assert!(Fix::EnableExtension.command(&ctx).is_ok());
        assert!(Fix::InstallExtension.command(&ctx).is_ok());
    }

    #[test]
    fn runtime_dir_must_be_private() {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(private_runtime_dir(None), None);
        assert_eq!(private_runtime_dir(Some(OsStr::new(""))), None);
        assert_eq!(private_runtime_dir(Some(OsStr::new("run/user/1"))), None);
        assert_eq!(
            private_runtime_dir(Some(OsStr::new("/nonexistent/swiftswitcher"))),
            None
        );
        let dir = tempfile::tempdir().unwrap();
        let private = dir.path().join("private");
        std::fs::create_dir(&private).unwrap();
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            private_runtime_dir(Some(private.as_os_str())),
            Some(private.clone())
        );
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&private, &link).unwrap();
        assert_eq!(private_runtime_dir(Some(link.as_os_str())), None);
        let shared = dir.path().join("shared");
        std::fs::create_dir(&shared).unwrap();
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(private_runtime_dir(Some(shared.as_os_str())), None);
        // Not owned by this user (root owns /).
        assert_eq!(private_runtime_dir(Some(OsStr::new("/"))), None);
    }

    #[test]
    fn staged_rule_is_new_and_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("rule");
        std::fs::write(&src, "KERNEL==\"uinput\"\n").unwrap();
        let tmp = dir.path().join("swiftswitcher-70.rules");
        stage_rule(&src, &tmp).unwrap();
        assert_eq!(
            std::fs::read_to_string(&tmp).unwrap(),
            "KERNEL==\"uinput\"\n"
        );
        let mode = std::fs::metadata(&tmp).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        // A planted symlink is replaced, never followed.
        std::fs::remove_file(&tmp).unwrap();
        let victim = dir.path().join("victim");
        std::fs::write(&victim, "keep").unwrap();
        std::os::unix::fs::symlink(&victim, &tmp).unwrap();
        stage_rule(&src, &tmp).unwrap();
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
        assert!(
            !std::fs::symlink_metadata(&tmp)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_to_string(&tmp).unwrap(),
            "KERNEL==\"uinput\"\n"
        );
    }

    #[test]
    fn extension_fixes_enable_last() {
        let ctx = appimage_ctx();
        let enable = ["gnome-extensions", "enable", EXTENSION_UUID];
        assert_eq!(Fix::EnableExtension.command(&ctx).unwrap(), [enable]);
        let cmds = Fix::InstallExtension.command(&ctx).unwrap();
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
