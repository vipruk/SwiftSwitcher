//! Reads physical key and pointer events from `/dev/input/event*` (never grabbing).
//!
//! Event contents are never logged; only device paths appear in log lines.

use evdev::{Device, EventType, KeyCode as EvKey, LedCode};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use swift_engine::tracker::Input;
use tokio::sync::mpsc::Sender;

/// Name of the injector's uinput device; events from it are ignored.
pub const VIRTUAL_DEVICE_NAME: &str = "SwiftSwitcher virtual keyboard";

const RESCAN: Duration = Duration::from_secs(2);
/// First code of the BTN_* range (BTN_MISC).
const BTN_MISC: u16 = 0x100;
const BTN_TOUCH: u16 = 0x14a;
const BTN_END: u16 = 0x160;

pub struct Physical {
    pub input: Input,
    pub at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Keyboard,
    Pointer,
}

/// Maps a raw EV_KEY event to an `Input`, or `None` if it is not relevant.
/// A device that is both keyboard and pointer is classified per event code.
pub fn classify(code: u16, value: i32, device_kind: Kind) -> Option<Input> {
    if code < BTN_MISC {
        if device_kind != Kind::Keyboard {
            return None;
        }
        return Some(Input::Key { code, value });
    }
    if !(BTN_MISC..BTN_END).contains(&code) || value != 1 {
        return None;
    }
    if code == BTN_TOUCH {
        Some(Input::Touch)
    } else {
        Some(Input::PointerButton)
    }
}

/// Full per-event filter used by the stream loop: only EV_KEY events matter.
fn relevant(ev_type: EventType, code: u16, value: i32, kind: Kind) -> Option<Input> {
    if ev_type != EventType::KEY {
        return None;
    }
    classify(code, value, event_kind(kind, code))
}

/// Which kinds of events a device can produce; `None` if irrelevant.
pub fn device_kind(dev: &Device) -> Option<Kind> {
    let keys = dev.supported_keys()?;
    if keys.contains(EvKey::KEY_A) && keys.contains(EvKey::KEY_Z) {
        Some(Kind::Keyboard)
    } else if keys.contains(EvKey::BTN_LEFT) || keys.contains(EvKey::BTN_TOUCH) {
        Some(Kind::Pointer)
    } else {
        None
    }
}

/// Classification that also lets a keyboard report pointer buttons.
fn event_kind(kind: Kind, code: u16) -> Kind {
    if code >= BTN_MISC {
        Kind::Pointer
    } else {
        kind
    }
}

pub fn input_paths() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir("/dev/input")
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("event"))
        })
        .collect();
    v.sort();
    v
}

/// Reads LED_CAPSL from the first readable keyboard device.
pub fn initial_caps_lock() -> Option<bool> {
    for path in input_paths() {
        let Ok(dev) = Device::open(&path) else {
            continue;
        };
        if dev.name() == Some(VIRTUAL_DEVICE_NAME) || device_kind(&dev) != Some(Kind::Keyboard) {
            continue;
        }
        if let Ok(leds) = dev.get_led_state() {
            return Some(leds.contains(LedCode::LED_CAPSL));
        }
    }
    None
}

/// Starts the hotplug scanner. Must be called inside a tokio runtime.
pub fn spawn_reader(tx: Sender<Physical>) -> anyhow::Result<()> {
    let open: Arc<Mutex<HashSet<PathBuf>>> = Arc::default();
    tokio::spawn(async move {
        let mut warned: HashSet<PathBuf> = HashSet::new();
        loop {
            for path in input_paths() {
                if open.lock().unwrap().contains(&path) {
                    continue;
                }
                let dev = match Device::open(&path) {
                    Ok(d) => d,
                    Err(e) => {
                        if warned.insert(path.clone()) {
                            log::warn!("cannot open {}: {e}", path.display());
                        }
                        continue;
                    }
                };
                if dev.name() == Some(VIRTUAL_DEVICE_NAME) {
                    continue;
                }
                let Some(kind) = device_kind(&dev) else {
                    continue;
                };
                let Ok(mut stream) = dev.into_event_stream() else {
                    continue;
                };
                open.lock().unwrap().insert(path.clone());
                log::info!("reading {}", path.display());
                let (tx, open, p) = (tx.clone(), open.clone(), path.clone());
                tokio::spawn(async move {
                    while let Ok(ev) = stream.next_event().await {
                        let Some(input) = relevant(ev.event_type(), ev.code(), ev.value(), kind)
                        else {
                            continue;
                        };
                        let at = Instant::now();
                        if tx.send(Physical { input, at }).await.is_err() {
                            break;
                        }
                    }
                    log::info!("device gone: {}", p.display());
                    open.lock().unwrap().remove(&p);
                });
            }
            tokio::time::sleep(RESCAN).await;
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_key_maps_to_key() {
        assert!(matches!(
            classify(30, 1, Kind::Keyboard),
            Some(Input::Key { code: 30, value: 1 })
        ));
        assert!(matches!(
            classify(30, 0, Kind::Keyboard),
            Some(Input::Key { code: 30, value: 0 })
        ));
    }

    #[test]
    fn pointer_button_down_only() {
        assert!(matches!(
            classify(0x110, 1, Kind::Pointer),
            Some(Input::PointerButton)
        ));
        assert!(classify(0x110, 0, Kind::Pointer).is_none());
    }

    #[test]
    fn touch_down() {
        assert!(matches!(
            classify(0x14a, 1, Kind::Pointer),
            Some(Input::Touch)
        ));
        assert!(classify(0x14a, 0, Kind::Pointer).is_none());
    }

    #[test]
    fn keyboard_keys_ignored_on_pointer_devices() {
        assert!(classify(30, 1, Kind::Pointer).is_none());
    }

    #[test]
    fn codes_outside_btn_range_ignored() {
        assert!(classify(0x170, 1, Kind::Pointer).is_none());
    }

    #[test]
    fn msc_events_ignored() {
        // MSC_SCAN is code 4; with EV_KEY it would look like a key.
        assert!(relevant(EventType::MISC, 4, 0x70004, Kind::Keyboard).is_none());
        assert!(relevant(EventType::MISC, 0x110, 1, Kind::Pointer).is_none());
        assert!(relevant(EventType::SYNCHRONIZATION, 0, 0, Kind::Keyboard).is_none());
    }

    #[test]
    fn relevant_passes_key_events() {
        assert!(matches!(
            relevant(EventType::KEY, 30, 1, Kind::Keyboard),
            Some(Input::Key { code: 30, value: 1 })
        ));
        assert!(matches!(
            relevant(EventType::KEY, 0x110, 1, Kind::Keyboard),
            Some(Input::PointerButton)
        ));
    }

    #[test]
    fn per_event_kind() {
        assert_eq!(event_kind(Kind::Keyboard, 0x110), Kind::Pointer);
        assert_eq!(event_kind(Kind::Keyboard, 30), Kind::Keyboard);
    }
}
