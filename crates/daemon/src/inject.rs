//! Synthesizes key events through a uinput virtual keyboard.

#![allow(dead_code)]

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

pub struct UinputInjector {
    dev: VirtualDevice,
}

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
        Ok(Self { dev })
    }

    /// Emits one key event followed by SYN_REPORT, then waits 2 ms.
    fn send(&mut self, code: KeyCode, value: i32) -> anyhow::Result<()> {
        // `emit` appends the SYN_REPORT itself.
        self.dev
            .emit(&[InputEvent::new(EventType::KEY.0, code, value)])?;
        std::thread::sleep(SETTLE);
        Ok(())
    }

    fn press_release(&mut self, code: KeyCode) -> anyhow::Result<()> {
        self.send(code, 1)?;
        self.send(code, 0)
    }
}

impl Injector for UinputInjector {
    fn tap(&mut self, s: Stroke) -> anyhow::Result<()> {
        if s.shift {
            self.send(KEY_LEFTSHIFT, 1)?;
        }
        let r = self.press_release(s.code);
        if s.shift {
            self.send(KEY_LEFTSHIFT, 0)?;
        }
        r
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
        let r = self.press_release(key);
        for &m in mods.iter().rev() {
            self.send(m, 0)?;
        }
        r
    }
}
