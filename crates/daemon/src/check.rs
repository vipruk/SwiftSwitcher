//! `swiftswitcher check`: read-only environment diagnostics.
//!
//! Nothing here creates a device or emits events. The settings window shows
//! the same [`Item`]s, with a [`Fix`] where it can repair one.

use crate::corrector::layout_status;
use crate::desktop::{Desktop, GnomeDesktop};
use crate::input::{Kind, VIRTUAL_DEVICE_NAME, device_kind, input_paths};
use crate::setup::{EXTENSION_UUID, Fix};
use evdev::Device;

const SETUP_FIX: &str = "run scripts/dev-setup.sh";

/// The result of one check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// Stable name: keyboard, pointer, uinput, dbus or layouts.
    pub id: &'static str,
    pub ok: bool,
    /// English: what failed and how to fix it by hand. Empty when ok.
    pub detail: String,
    /// A fix the settings window can run. None when ok.
    pub fix: Option<Fix>,
}

impl Item {
    fn new(id: &'static str, result: Result<(), String>, fix: Option<Fix>) -> Item {
        match result {
            Ok(()) => Item {
                id,
                ok: true,
                detail: String::new(),
                fix: None,
            },
            Err(detail) => Item {
                id,
                ok: false,
                detail,
                fix,
            },
        }
    }
}

/// One line per check: `name: OK` or `name: FAIL: fix`.
pub fn format_report(items: &[Item]) -> String {
    items
        .iter()
        .map(|i| {
            if i.ok {
                format!("{}: OK\n", i.id)
            } else {
                format!("{}: FAIL: {}\n", i.id, i.detail)
            }
        })
        .collect()
}

/// How a device of some kind can be reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Access {
    Readable,
    /// Some device could not be opened for lack of permission.
    Denied,
    Missing,
}

fn device_access(kind: Kind) -> Access {
    let mut denied = false;
    for path in input_paths() {
        match Device::open(&path) {
            Ok(dev) => {
                if dev.name() != Some(VIRTUAL_DEVICE_NAME) && device_kind(&dev) == Some(kind) {
                    return Access::Readable;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => denied = true,
            Err(_) => {}
        }
    }
    if denied {
        Access::Denied
    } else {
        Access::Missing
    }
}

fn device_result(access: Access, what: &str) -> Result<(), String> {
    match access {
        Access::Readable => Ok(()),
        Access::Denied => Err(device_hint(what, true)),
        Access::Missing => Err(device_hint(what, false)),
    }
}

fn device_hint(what: &str, denied: bool) -> String {
    if denied {
        format!("no readable {what} device; {SETUP_FIX}, then unplug and replug it")
    } else {
        format!(
            "no {what} device found in /dev/input; connect one (in a VM or container, \
             pass the host's /dev/input through)"
        )
    }
}

fn bus_hint(e: &dyn std::fmt::Display) -> String {
    format!(
        "cannot connect to the session bus ({e}); run this in a terminal inside your \
         GNOME session as yourself, not over ssh, with sudo or as another user \
         (DBUS_SESSION_BUS_ADDRESS must point at the session bus)"
    )
}

fn uinput_check() -> Result<(), String> {
    // Open only: never create a device or emit events.
    std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/uinput")
        .map(drop)
        .map_err(|e| format!("cannot open /dev/uinput for writing ({e}); {SETUP_FIX}"))
}

/// Whether the fixes that ship files from the bundle apply.
fn in_appimage() -> bool {
    std::env::var_os("APPDIR").is_some_and(|v| !v.is_empty())
}

/// The udev rule grants access; only an AppImage needs to install it.
fn access_fix(appimage: bool) -> Option<Fix> {
    appimage.then_some(Fix::InstallUdevRule)
}

/// An installed extension can be enabled; an AppImage can install its own.
fn extension_fix(appimage: bool, installed: bool) -> Option<Fix> {
    match (installed, appimage) {
        (true, _) => Some(Fix::EnableExtension),
        (false, true) => Some(Fix::InstallExtension),
        (false, false) => None,
    }
}

fn extension_installed() -> bool {
    let user = dirs::data_dir().map(|d| d.join("gnome-shell/extensions").join(EXTENSION_UUID));
    let system = std::path::Path::new("/usr/share/gnome-shell/extensions").join(EXTENSION_UUID);
    user.is_some_and(|p| p.exists()) || system.exists()
}

/// Runs all checks. Opens devices and calls D-Bus: keep it off the GTK thread.
pub async fn run_items() -> Vec<Item> {
    let appimage = in_appimage();
    let keyboard = device_access(Kind::Keyboard);
    let pointer = device_access(Kind::Pointer);
    let uinput = uinput_check();
    let uinput_denied = uinput.is_err();
    let mut items = vec![
        Item::new(
            "keyboard",
            device_result(keyboard, "keyboard"),
            (keyboard == Access::Denied)
                .then(|| access_fix(appimage))
                .flatten(),
        ),
        Item::new(
            "pointer",
            device_result(pointer, "mouse/touchpad").map_err(|e| {
                format!(
                    "{e} (without it clicks and taps cannot reset the typing state, so \
                     automatic correction stays off; hotkeys still work)"
                )
            }),
            (pointer == Access::Denied)
                .then(|| access_fix(appimage))
                .flatten(),
        ),
        Item::new(
            "uinput",
            uinput,
            uinput_denied.then(|| access_fix(appimage)).flatten(),
        ),
    ];
    let (layouts, dbus_fix) = match GnomeDesktop::connect().await {
        Ok(d) => (
            d.layouts().await.map_err(|e| {
                format!(
                    "GNOME extension not reachable ({e}); run scripts/dev-setup.sh, then log out and back in"
                )
            }),
            extension_fix(appimage, extension_installed()),
        ),
        Err(e) => (Err(bus_hint(&e)), None),
    };
    items.push(Item::new(
        "dbus",
        layouts.as_ref().map(drop).map_err(|e| e.clone()),
        dbus_fix,
    ));
    items.push(Item::new(
        "layouts",
        match &layouts {
            Ok(l) => layout_status(l)
                .map_err(|e| format!("{e}; set input sources to US and Russian in GNOME Settings")),
            Err(_) => Err("skipped: D-Bus unavailable".to_string()),
        },
        None,
    ));
    items
}

/// Runs all checks and prints the report. Returns whether all passed.
pub async fn run() -> bool {
    let items = run_items().await;
    print!("{}", format_report(&items));
    items.iter().all(|i| i.ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &'static str, r: Result<(), &str>) -> Item {
        Item::new(id, r.map_err(str::to_string), None)
    }

    #[test]
    fn report_unchanged() {
        let items = [
            item("keyboard", Ok(())),
            item("pointer", Ok(())),
            item("uinput", Err("run scripts/dev-setup.sh")),
            item("dbus", Ok(())),
            item("layouts", Err("skipped: D-Bus unavailable")),
        ];
        assert_eq!(
            format_report(&items),
            "keyboard: OK\npointer: OK\nuinput: FAIL: run scripts/dev-setup.sh\n\
             dbus: OK\nlayouts: FAIL: skipped: D-Bus unavailable\n"
        );
        let all_ok = [item("keyboard", Ok(())), item("layouts", Ok(()))];
        assert_eq!(format_report(&all_ok), "keyboard: OK\nlayouts: OK\n");
    }

    #[test]
    fn hints_name_a_fix() {
        assert!(device_hint("keyboard", true).contains(SETUP_FIX));
        assert!(device_hint("keyboard", false).contains("connect one"));
        let bus = bus_hint(&"no address");
        assert!(bus.contains("inside your GNOME session"));
        assert!(bus.contains("DBUS_SESSION_BUS_ADDRESS"));
    }

    #[test]
    fn fixes_offered() {
        assert_eq!(access_fix(true), Some(Fix::InstallUdevRule));
        assert_eq!(access_fix(false), None);
        assert_eq!(extension_fix(false, true), Some(Fix::EnableExtension));
        assert_eq!(extension_fix(true, true), Some(Fix::EnableExtension));
        assert_eq!(extension_fix(true, false), Some(Fix::InstallExtension));
        assert_eq!(extension_fix(false, false), None);
        let ok = Item::new("dbus", Ok(()), Some(Fix::EnableExtension));
        assert!(ok.ok && ok.fix.is_none() && ok.detail.is_empty());
    }
}
