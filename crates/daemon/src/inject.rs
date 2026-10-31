//! Synthesizes key events through a uinput virtual keyboard.

use crate::input::VIRTUAL_DEVICE_NAME;
use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, EventType, InputEvent, KeyCode as EvKey};
use std::time::Duration;
use swift_engine::keymap::{KEY_BACKSPACE, KeyCode, Stroke};

const KEY_LEFTSHIFT: KeyCode = 42;
const SETTLE: Duration = Duration::from_millis(2);

pub trait Injector {
    fn tap(&mut self, s: Stroke) -> anyhow::Result<()>;
    fn backspace(&mut self, n: usize) -> anyhow::Result<()>;
    fn chord(&mut self, mods: &[KeyCode], key: KeyCode) -> anyhow::Result<()>;
}

/// Destination for raw key events; one write is one event plus SYN_REPORT.
pub trait Sink {
    fn write(&mut self, code: KeyCode, value: i32) -> anyhow::Result<()>;
}

pub struct DeviceSink(VirtualDevice);

impl Sink for DeviceSink {
    fn write(&mut self, code: KeyCode, value: i32) -> anyhow::Result<()> {
        // `emit` appends the SYN_REPORT itself.
        self.0
            .emit(&[InputEvent::new(EventType::KEY.0, code, value)])?;
        Ok(())
    }
}

/// Tracks which keys are down so that any failure (or drop) releases them.
pub struct Keys<S: Sink> {
    sink: S,
    held: Vec<KeyCode>,
}

pub type UinputInjector = Keys<DeviceSink>;

impl UinputInjector {
    pub fn new() -> anyhow::Result<Self> {
        let mut keys = AttributeSet::<EvKey>::new();
        for code in 1..0x100u16 {
            keys.insert(EvKey::new(code));
        }
        let dev = VirtualDevice::builder()?
            .name(VIRTUAL_DEVICE_NAME)
            .with_keys(&keys)?
            .build()?;
        Ok(Keys {
            sink: DeviceSink(dev),
            held: Vec::new(),
        })
    }
}

impl<S: Sink> Keys<S> {
    /// Best-effort release of everything still held, newest first.
    fn release_all(&mut self) {
        while let Some(code) = self.held.pop() {
            let _ = self.sink.write(code, 0);
            std::thread::sleep(SETTLE);
        }
    }

    /// Emits one key event followed by SYN_REPORT, then waits 2 ms.
    /// On failure every held key is released before the error is returned.
    fn send(&mut self, code: KeyCode, value: i32) -> anyhow::Result<()> {
        if let Err(e) = self.sink.write(code, value) {
            self.release_all();
            return Err(e);
        }
        if value == 0 {
            self.held.retain(|&c| c != code);
        } else if !self.held.contains(&code) {
            self.held.push(code);
        }
        std::thread::sleep(SETTLE);
        Ok(())
    }

    fn press_release(&mut self, code: KeyCode) -> anyhow::Result<()> {
        self.send(code, 1)?;
        self.send(code, 0)
    }
}

impl<S: Sink> Drop for Keys<S> {
    fn drop(&mut self) {
        self.release_all();
    }
}

impl<S: Sink> Injector for Keys<S> {
    fn tap(&mut self, s: Stroke) -> anyhow::Result<()> {
        if s.shift {
            self.send(KEY_LEFTSHIFT, 1)?;
        }
        self.press_release(s.code)?;
        if s.shift {
            self.send(KEY_LEFTSHIFT, 0)?;
        }
        Ok(())
    }

    fn backspace(&mut self, n: usize) -> anyhow::Result<()> {
        for _ in 0..n {
            self.press_release(KEY_BACKSPACE)?;
        }
        Ok(())
    }

    fn chord(&mut self, mods: &[KeyCode], key: KeyCode) -> anyhow::Result<()> {
        for &m in mods {
            self.send(m, 1)?;
        }
        self.press_release(key)?;
        for &m in mods.iter().rev() {
            self.send(m, 0)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Log = Rc<RefCell<Vec<(KeyCode, i32)>>>;

    /// Records writes; the `fail_on`th write (1-based) fails once.
    struct TestSink {
        log: Log,
        count: usize,
        fail_on: usize,
    }

    impl Sink for TestSink {
        fn write(&mut self, code: KeyCode, value: i32) -> anyhow::Result<()> {
            self.count += 1;
            if self.count == self.fail_on {
                anyhow::bail!("injected failure");
            }
            self.log.borrow_mut().push((code, value));
            Ok(())
        }
    }

    fn keys(fail_on: usize) -> (Keys<TestSink>, Log) {
        let log: Log = Rc::default();
        let k = Keys {
            sink: TestSink {
                log: log.clone(),
                count: 0,
                fail_on,
            },
            held: Vec::new(),
        };
        (k, log)
    }

    fn net_down(log: &Log) -> Vec<KeyCode> {
        let mut down: Vec<KeyCode> = Vec::new();
        for &(c, v) in log.borrow().iter() {
            if v == 0 {
                down.retain(|&d| d != c);
            } else {
                down.push(c);
            }
        }
        down
    }

    #[test]
    fn chord_ok_orders_events() {
        let (mut k, log) = keys(0);
        k.chord(&[29, 42], 30).unwrap();
        assert_eq!(
            *log.borrow(),
            vec![(29, 1), (42, 1), (30, 1), (30, 0), (42, 0), (29, 0)]
        );
    }

    #[test]
    fn chord_failure_on_second_modifier_releases_first() {
        let (mut k, log) = keys(2);
        assert!(k.chord(&[29, 42], 30).is_err());
        assert!(net_down(&log).is_empty());
        assert!(log.borrow().contains(&(29, 0)));
    }

    #[test]
    fn chord_failure_on_release_still_releases_rest() {
        // writes: 29d 42d 30d 30u 42u(fail=5th) 29u
        let (mut k, log) = keys(5);
        assert!(k.chord(&[29, 42], 30).is_err());
        assert!(net_down(&log).is_empty());
    }

    #[test]
    fn tap_failure_on_key_press_releases_shift() {
        let (mut k, log) = keys(2);
        let s = Stroke {
            code: 30,
            shift: true,
            caps: false,
        };
        assert!(k.tap(s).is_err());
        assert!(net_down(&log).is_empty());
        assert!(log.borrow().contains(&(KEY_LEFTSHIFT, 0)));
    }

    #[test]
    fn backspace_failure_leaves_nothing_down() {
        let (mut k, log) = keys(3);
        assert!(k.backspace(3).is_err());
        assert!(net_down(&log).is_empty());
    }

    #[test]
    fn drop_releases_held() {
        let (mut k, log) = keys(0);
        k.send(29, 1).unwrap();
        drop(k);
        assert!(net_down(&log).is_empty());
    }
}
