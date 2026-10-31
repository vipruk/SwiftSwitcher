//! Reads physical key and pointer events from `/dev/input/event*` (never grabbing).
//!
//! Event contents are never logged; only device paths appear in log lines.

use crate::corrector::Activity;
use evdev::{Device, EventType, KeyCode as EvKey, LedCode};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
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

/// Keys last seen down on one device and not yet released. When the device
/// vanishes (ENODEV) the kernel's synthesized key-ups never reach us, so the
/// reader replays them from this set to keep `Activity` consistent.
#[derive(Default)]
pub struct HeldKeys(std::collections::BTreeSet<u16>);

impl HeldKeys {
    /// Records a raw key event; BTN_* codes are not tracked (see `classify`).
    pub fn observe(&mut self, code: u16, value: i32) {
        if code >= BTN_MISC {
            return;
        }
        match value {
            0 => {
                self.0.remove(&code);
            }
            1 | 2 => {
                self.0.insert(code);
            }
            _ => {}
        }
    }

    /// Codes still down; empties the set.
    pub fn drain_releases(&mut self) -> Vec<u16> {
        std::mem::take(&mut self.0).into_iter().collect()
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

/// Whether a device reports clicks or taps (a keyboard with a built-in
/// touchpad counts too).
fn reports_pointer(dev: &Device) -> bool {
    dev.supported_keys()
        .is_some_and(|k| k.contains(EvKey::BTN_LEFT) || k.contains(EvKey::BTN_TOUCH))
}

/// Pointer availability after a scan: `Some(present)` when it changed since
/// `prev` (or on the first scan), i.e. when it must be logged.
pub fn pointer_change(prev: Option<bool>, pointers: usize) -> Option<bool> {
    let present = pointers > 0;
    (prev != Some(present)).then_some(present)
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
///
/// Without a readable pointer device a click could move the caret unseen,
/// so the reader keeps `activity`'s pointer flag up to date: automatic
/// correction is off while it is false (hotkeys keep working).
pub fn spawn_reader(tx: Sender<Physical>, activity: Arc<Activity>) -> anyhow::Result<()> {
    let open: Arc<Mutex<HashSet<PathBuf>>> = Arc::default();
    let pointers: Arc<AtomicUsize> = Arc::default();
    tokio::spawn(async move {
        let mut warned: HashSet<PathBuf> = HashSet::new();
        let mut pointer_state: Option<bool> = None;
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
                let pointer = reports_pointer(&dev);
                let Ok(mut stream) = dev.into_event_stream() else {
                    continue;
                };
                open.lock().unwrap().insert(path.clone());
                if pointer {
                    pointers.fetch_add(1, Ordering::SeqCst);
                }
                log::info!("reading {}", path.display());
                let (tx, open, p) = (tx.clone(), open.clone(), path.clone());
                let (pointers, activity) = (pointers.clone(), activity.clone());
                tokio::spawn(async move {
                    let mut held = HeldKeys::default();
                    while let Ok(ev) = stream.next_event().await {
                        let Some(input) = relevant(ev.event_type(), ev.code(), ev.value(), kind)
                        else {
                            continue;
                        };
                        if let Input::Key { code, value } = input {
                            held.observe(code, value);
                        }
                        let at = Instant::now();
                        if tx.send(Physical { input, at }).await.is_err() {
                            break;
                        }
                    }
                    // Unplugged mid-press: synthesize the key-ups evdev never delivered.
                    for code in held.drain_releases() {
                        let input = Input::Key { code, value: 0 };
                        if tx
                            .send(Physical {
                                input,
                                at: Instant::now(),
                            })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    log::info!("device gone: {}", p.display());
                    if pointer && pointers.fetch_sub(1, Ordering::SeqCst) == 1 {
                        // Last pointer gone: stop at once, not at the next scan.
                        activity.set_pointer_present(false);
                    }
                    open.lock().unwrap().remove(&p);
                });
            }
            let count = pointers.load(Ordering::SeqCst);
            activity.set_pointer_present(count > 0);
            if let Some(present) = pointer_change(pointer_state, count) {
                if present {
                    log::info!("pointer device available; automatic correction enabled");
                } else {
                    log::warn!(
                        "no mouse, touchpad or touchscreen can be read: clicks cannot reset \
                         the typing state, so automatic correction is off until one \
                         appears (hotkeys still work); see `swiftswitcher check`"
                    );
                }
                pointer_state = Some(present);
            }
            tokio::time::sleep(RESCAN).await;
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {

    #[test]
    fn held_keys_released_pair_leaves_nothing() {
        let mut h = HeldKeys::default();
        h.observe(30, 1);
        h.observe(30, 0);
        assert!(h.drain_releases().is_empty());
    }

    #[test]
    fn held_keys_repeat_still_held() {
        let mut h = HeldKeys::default();
        h.observe(30, 1);
        h.observe(30, 2);
        assert_eq!(h.drain_releases(), vec![30]);
        assert!(h.drain_releases().is_empty());
    }

    #[test]
    fn held_keys_only_unreleased_drained() {
        let mut h = HeldKeys::default();
        h.observe(30, 1);
        h.observe(31, 1);
        h.observe(30, 0);
        assert_eq!(h.drain_releases(), vec![31]);
    }

    #[test]
    fn held_keys_ignore_pointer_buttons() {
        let mut h = HeldKeys::default();
        h.observe(0x110, 1);
        h.observe(0x14a, 1);
        assert!(h.drain_releases().is_empty());
    }
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
    fn pointer_change_reports_transitions_only() {
        assert_eq!(pointer_change(None, 0), Some(false), "warn at startup");
        assert_eq!(pointer_change(None, 2), Some(true));
        assert_eq!(pointer_change(Some(false), 0), None);
        assert_eq!(pointer_change(Some(true), 1), None);
        assert_eq!(pointer_change(Some(true), 0), Some(false));
        assert_eq!(pointer_change(Some(false), 1), Some(true));
    }

    #[test]
    fn per_event_kind() {
        assert_eq!(event_kind(Kind::Keyboard, 0x110), Kind::Pointer);
        assert_eq!(event_kind(Kind::Keyboard, 30), Kind::Keyboard);
    }
}
