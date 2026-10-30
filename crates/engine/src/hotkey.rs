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
}
