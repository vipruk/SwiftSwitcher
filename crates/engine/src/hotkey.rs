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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyAction {
    ConvertLast,
    ConvertSelection,
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
