//! Hotkey definitions and parsing.

use crate::keymap::{KeyCode, Layout, stroke_for};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub code: KeyCode,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

/// Why a hotkey is refused as a binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsafe {
    /// A letter or Space without Ctrl, Alt or Meta: it types text.
    Typing,
    /// A letter or Space with Ctrl (and maybe Shift) only: an application
    /// shortcut such as Ctrl+A, or the Ctrl(+Shift)+C/V the daemon injects.
    AppShortcut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyAction {
    ConvertLast,
    ConvertSelection,
    ChangeCase,
}

impl Hotkey {
    /// Parses strings like "Pause", "Shift+Pause", "Ctrl+Alt+Space".
    pub fn parse(s: &str) -> Result<Hotkey, String> {
        let parts: Vec<&str> = s.split('+').map(str::trim).collect();
        let (key, mods) = parts.split_last().ok_or("empty hotkey")?;
        let mut h = Hotkey {
            code: key_code(key)?,
            shift: false,
            ctrl: false,
            alt: false,
            meta: false,
        };
        for m in mods {
            let flag = match m.to_ascii_lowercase().as_str() {
                "shift" => &mut h.shift,
                "ctrl" | "control" => &mut h.ctrl,
                "alt" => &mut h.alt,
                "meta" | "super" | "win" => &mut h.meta,
                _ => return Err(format!("unknown modifier: {m}")),
            };
            *flag = true;
        }
        Ok(h)
    }

    /// Whether the hotkey is safe to bind. Hotkeys are never swallowed, so a
    /// key that types text or an application shortcut would fire on normal
    /// use. Non-typing keys (Pause, ScrollLock, Insert, F1-F12) are fine with
    /// any modifiers; a letter or Space needs Alt or Meta.
    pub fn check_safe(&self) -> Result<(), Unsafe> {
        let typing = self.code == 57 || is_letter(self.code);
        if !typing || self.alt || self.meta {
            Ok(())
        } else if self.ctrl {
            Err(Unsafe::AppShortcut)
        } else {
            Err(Unsafe::Typing)
        }
    }

    /// Canonical form: `Ctrl+Alt+Shift+Meta+Key`. Parses back to the same hotkey
    /// when the key is supported; an unsupported key is shown as `Key<code>`.
    pub fn name(&self) -> String {
        let mut out = String::new();
        for (on, label) in [
            (self.ctrl, "Ctrl"),
            (self.alt, "Alt"),
            (self.shift, "Shift"),
            (self.meta, "Meta"),
        ] {
            if on {
                out.push_str(label);
                out.push('+');
            }
        }
        match key_name(self.code) {
            Some(n) => out.push_str(n),
            None => out.push_str(&format!("Key{}", self.code)),
        }
        out
    }
}

const FKEYS: [&str; 12] = [
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
];
const LETTERS: [&str; 26] = [
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S",
    "T", "U", "V", "W", "X", "Y", "Z",
];

fn is_letter(code: KeyCode) -> bool {
    key_name(code).is_some_and(|n| n.len() == 1)
}

/// Canonical name of a supported hotkey key, `None` for anything else.
pub fn key_name(code: KeyCode) -> Option<&'static str> {
    match code {
        119 => Some("Pause"),
        70 => Some("ScrollLock"),
        57 => Some("Space"),
        110 => Some("Insert"),
        59..=68 => Some(FKEYS[usize::from(code - 59)]),
        87 => Some("F11"),
        88 => Some("F12"),
        _ => LETTERS.iter().enumerate().find_map(|(i, name)| {
            let c = char::from(b'a' + i as u8);
            (stroke_for(c, Layout::En)?.code == code).then_some(*name)
        }),
    }
}

fn key_code(name: &str) -> Result<KeyCode, String> {
    let lower = name.to_ascii_lowercase();
    let code = match lower.as_str() {
        "pause" => 119,
        "scrolllock" => 70,
        "space" => 57,
        "insert" => 110,
        "f11" => 87,
        "f12" => 88,
        _ => {
            if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u16>().ok())
                && (1..=10).contains(&n)
            {
                return Ok(58 + n);
            }
            let mut chars = lower.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_lowercase() => {
                    return stroke_for(c, Layout::En)
                        .map(|st| st.code)
                        .ok_or_else(|| format!("unknown key: {name}"));
                }
                _ => return Err(format!("unknown key: {name}")),
            }
        }
    };
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alt_pause_is_safe() {
        let h = Hotkey::parse("Alt+Pause").unwrap();
        assert!(h.alt && h.code == 119);
        assert_eq!(h.check_safe(), Ok(()));
    }

    #[test]
    fn parses_plain_and_modified() {
        let h = Hotkey::parse("Pause").unwrap();
        assert_eq!(h.code, 119);
        assert!(!h.shift && !h.ctrl && !h.alt && !h.meta);
        let h = Hotkey::parse("shift+pause").unwrap();
        assert!(h.shift && h.code == 119);
        let h = Hotkey::parse("Ctrl+Alt+Space").unwrap();
        assert!(h.ctrl && h.alt && !h.shift && h.code == 57);
        assert_eq!(Hotkey::parse("F12").unwrap().code, 88);
        assert_eq!(Hotkey::parse("F1").unwrap().code, 59);
        assert_eq!(Hotkey::parse("ScrollLock").unwrap().code, 70);
        assert_eq!(Hotkey::parse("Insert").unwrap().code, 110);
        assert_eq!(Hotkey::parse("A").unwrap().code, 30);
        assert_eq!(Hotkey::parse("Super+Z").unwrap().code, 44);
        assert!(Hotkey::parse("Super+Z").unwrap().meta);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Hotkey::parse("Ctrl+Bogus").is_err());
        assert!(Hotkey::parse("").is_err());
        assert!(Hotkey::parse("Ctrl+").is_err());
        assert!(Hotkey::parse("Ctrl").is_err());
        assert!(Hotkey::parse("Pause+Shift").is_err());
    }

    fn all_codes() -> Vec<KeyCode> {
        (0..=255).filter(|&c| key_name(c).is_some()).collect()
    }

    #[test]
    fn name_round_trips_every_supported_key() {
        let codes = all_codes();
        assert_eq!(codes.len(), 4 + 12 + 26);
        for code in codes {
            for bits in 0..16u8 {
                let h = Hotkey {
                    code,
                    shift: bits & 1 != 0,
                    ctrl: bits & 2 != 0,
                    alt: bits & 4 != 0,
                    meta: bits & 8 != 0,
                };
                assert_eq!(Hotkey::parse(&h.name()), Ok(h), "{}", h.name());
            }
        }
        assert_eq!(key_name(127), None);
    }

    fn safety(s: &str) -> Result<(), Unsafe> {
        Hotkey::parse(s).unwrap().check_safe()
    }

    #[test]
    fn refuses_typing_and_app_shortcuts() {
        for s in ["A", "Space", "Shift+A", "Shift+Space", "Q"] {
            assert_eq!(safety(s), Err(Unsafe::Typing), "{s}");
        }
        for s in [
            "Ctrl+A",
            "Ctrl+Shift+A",
            "Ctrl+C",
            "Ctrl+Shift+V",
            "Ctrl+Space",
        ] {
            assert_eq!(safety(s), Err(Unsafe::AppShortcut), "{s}");
        }
    }

    #[test]
    fn accepts_safe_bindings() {
        for s in [
            "Ctrl+Alt+Space",
            "Alt+Q",
            "Meta+Space",
            "Pause",
            "Shift+Pause",
            "F9",
            "Ctrl+F9",
            "ScrollLock",
            "Insert",
            "F12",
            "Ctrl+Meta+A",
        ] {
            assert_eq!(safety(s), Ok(()), "{s}");
        }
    }

    #[test]
    fn default_names() {
        assert_eq!(Hotkey::parse("shift+pause").unwrap().name(), "Shift+Pause");
        assert_eq!(
            Hotkey::parse("ctrl+alt+space").unwrap().name(),
            "Ctrl+Alt+Space"
        );
        assert_eq!(
            Hotkey::parse("super+shift+ctrl+alt+a").unwrap().name(),
            "Ctrl+Alt+Shift+Meta+A"
        );
    }
}
