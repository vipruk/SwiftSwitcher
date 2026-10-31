//! `swiftswitcher check`: read-only environment diagnostics.
//!
//! Nothing here creates a device or emits events.

use crate::corrector::layout_status;
use crate::desktop::{Desktop, GnomeDesktop};
use crate::input::{Kind, VIRTUAL_DEVICE_NAME, device_kind, input_paths};
use evdev::Device;

const SETUP_FIX: &str = "run scripts/dev-setup.sh";

/// One line per check: `name: OK` or `name: FAIL: fix`.
pub fn format_report<E: AsRef<str>>(results: &[(&str, Result<(), E>)]) -> String {
    results
        .iter()
        .map(|(name, r)| match r {
            Ok(()) => format!("{name}: OK\n"),
            Err(fix) => format!("{name}: FAIL: {}\n", fix.as_ref()),
        })
        .collect()
}

/// Looks for a readable device of the given kind.
fn device_check(kind: Kind, what: &str) -> Result<(), String> {
    let mut denied = false;
    for path in input_paths() {
        match Device::open(&path) {
            Ok(dev) => {
                if dev.name() != Some(VIRTUAL_DEVICE_NAME) && device_kind(&dev) == Some(kind) {
                    return Ok(());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => denied = true,
            Err(_) => {}
        }
    }
    Err(device_hint(what, denied))
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

/// Runs all checks and prints the report. Returns whether all passed.
pub async fn run() -> bool {
    let mut results: Vec<(&str, Result<(), String>)> = vec![
        ("keyboard", device_check(Kind::Keyboard, "keyboard")),
        (
            "pointer",
            device_check(Kind::Pointer, "mouse/touchpad").map_err(|e| {
                format!(
                    "{e} (without it clicks and taps cannot reset the typing state, so \
                     automatic correction stays off; hotkeys still work)"
                )
            }),
        ),
        ("uinput", uinput_check()),
    ];
    let desktop = GnomeDesktop::connect().await;
    let layouts = match desktop {
        Ok(d) => d.layouts().await.map_err(|e| {
            format!(
                "GNOME extension not reachable ({e}); run scripts/dev-setup.sh, then log out and back in"
            )
        }),
        Err(e) => Err(bus_hint(&e)),
    };
    results.push(("dbus", layouts.as_ref().map(drop).map_err(|e| e.clone())));
    results.push((
        "layouts",
        match &layouts {
            Ok(l) => layout_status(l)
                .map_err(|e| format!("{e}; set input sources to US and Russian in GNOME Settings")),
            Err(_) => Err("skipped: D-Bus unavailable".to_string()),
        },
    ));
    print!("{}", format_report(&results));
    results.iter().all(|(_, r)| r.is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failing_check_shows_fix() {
        assert_eq!(
            format_report(&[("uinput", Err("run scripts/dev-setup.sh"))]),
            "uinput: FAIL: run scripts/dev-setup.sh\n"
        );
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
    fn all_ok_lines_end_in_ok() {
        let r: &[(&str, Result<(), &str>)] = &[("keyboard", Ok(())), ("layouts", Ok(()))];
        let out = format_report(r);
        assert_eq!(out, "keyboard: OK\nlayouts: OK\n");
        assert!(out.lines().all(|l| l.ends_with("OK")));
    }
}
