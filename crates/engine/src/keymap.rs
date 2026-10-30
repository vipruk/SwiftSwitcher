//! US <-> RU keymap by physical key (Linux input-event-codes).

/// Linux input-event-code of a physical key.
pub type KeyCode = u16;

pub const KEY_SPACE: KeyCode = 57;
pub const KEY_BACKSPACE: KeyCode = 14;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layout {
    En,
    Ru,
}

impl Layout {
    pub fn other(self) -> Layout {
        match self {
            Layout::En => Layout::Ru,
            Layout::Ru => Layout::En,
        }
    }

    /// Maps a GNOME input-source id (`us` / `ru`) to a layout.
    pub fn from_source_id(id: &str) -> Option<Layout> {
        match id {
            "us" => Some(Layout::En),
            "ru" => Some(Layout::Ru),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stroke {
    pub code: KeyCode,
    pub shift: bool,
    pub caps: bool,
}

/// Physical key codes, in the same order as the table rows below.
const CODES: [KeyCode; 47] = [
    41, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, // grave, digits, minus, equal
    16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, // q..p, braces
    30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, // a..l, semicolon, apostrophe
    43, // backslash
    44, 45, 46, 47, 48, 49, 50, 51, 52, 53, // z..m, comma, dot, slash
];

const EN_BASE: &str = "`1234567890-=qwertyuiop[]asdfghjkl;'\\zxcvbnm,./";
const EN_SHIFT: &str = "~!@#$%^&*()_+QWERTYUIOP{}ASDFGHJKL:\"|ZXCVBNM<>?";
const RU_BASE: &str = "ё1234567890-=йцукенгшщзхъфывапролджэ\\ячсмитьбю.";
const RU_SHIFT: &str = "Ё!\"№;%:?*()_+ЙЦУКЕНГШЩЗХЪФЫВАПРОЛДЖЭ/ЯЧСМИТЬБЮ,";

/// (unshifted, shifted) rows for a layout.
fn rows(layout: Layout) -> (&'static str, &'static str) {
    match layout {
        Layout::En => (EN_BASE, EN_SHIFT),
        Layout::Ru => (RU_BASE, RU_SHIFT),
    }
}

/// Character produced by a stroke. Letters are uppercase iff `shift ^ caps`;
/// non-letters depend on `shift` only.
pub fn char_for(s: Stroke, layout: Layout) -> Option<char> {
    if s.code == KEY_SPACE {
        return Some(' ');
    }
    let idx = CODES.iter().position(|&c| c == s.code)?;
    let (base, shifted) = rows(layout);
    let lower = base.chars().nth(idx)?;
    let upper_wanted = if lower.is_alphabetic() {
        s.shift ^ s.caps
    } else {
        s.shift
    };
    if upper_wanted {
        shifted.chars().nth(idx)
    } else {
        Some(lower)
    }
}

/// Stroke that types `c` in `layout` (caps is always false).
pub fn stroke_for(c: char, layout: Layout) -> Option<Stroke> {
    if c == ' ' {
        return Some(Stroke {
            code: KEY_SPACE,
            shift: false,
            caps: false,
        });
    }
    let (base, shifted) = rows(layout);
    let (idx, shift) = match base.chars().position(|b| b == c) {
        Some(i) => (i, false),
        None => (shifted.chars().position(|b| b == c)?, true),
    };
    Some(Stroke {
        code: CODES[idx],
        shift,
        caps: false,
    })
}

/// Re-types `text` (written in layout `from`) as if the other layout was
/// active. Unmapped characters pass through unchanged.
pub fn convert_text(text: &str, from: Layout) -> String {
    text.chars()
        .map(|c| {
            stroke_for(c, from)
                .and_then(|s| char_for(s, from.other()))
                .unwrap_or(c)
        })
        .collect()
}

/// True for letters of the layout's alphabet, in any case.
pub fn is_letter(c: char, layout: Layout) -> bool {
    match layout {
        Layout::En => c.is_ascii_alphabetic(),
        Layout::Ru => matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_en_to_ru() {
        assert_eq!(convert_text("ghbdtn", Layout::En), "привет");
        assert_eq!(
            convert_text("Ghbdtn? rfr ltkf", Layout::En),
            "Привет, как дела"
        );
    }

    #[test]
    fn converts_ru_to_en() {
        assert_eq!(convert_text("руддщ", Layout::Ru), "hello");
        assert_eq!(convert_text("ёжик", Layout::Ru), "`;br");
    }

    #[test]
    fn caps_affects_letters_only() {
        let s = Stroke {
            code: 39,
            shift: false,
            caps: true,
        };
        assert_eq!(char_for(s, Layout::Ru), Some('Ж'));
        assert_eq!(char_for(s, Layout::En), Some(';'));
        let s = Stroke {
            code: 39,
            shift: true,
            caps: true,
        };
        assert_eq!(char_for(s, Layout::Ru), Some('ж'));
        assert_eq!(char_for(s, Layout::En), Some(':'));
    }

    #[test]
    fn round_trip_all_table_chars() {
        for l in [Layout::En, Layout::Ru] {
            for row in [rows(l).0, rows(l).1] {
                for c in row.chars() {
                    let s = stroke_for(c, l).unwrap();
                    assert!(!s.caps);
                    assert_eq!(char_for(s, l), Some(c));
                }
            }
        }
        assert_eq!(
            char_for(stroke_for(' ', Layout::Ru).unwrap(), Layout::En),
            Some(' ')
        );
    }

    #[test]
    fn layout_helpers() {
        assert_eq!(Layout::En.other(), Layout::Ru);
        assert_eq!(Layout::from_source_id("us"), Some(Layout::En));
        assert_eq!(Layout::from_source_id("ru"), Some(Layout::Ru));
        assert_eq!(Layout::from_source_id("de"), None);
        assert!(is_letter('Q', Layout::En) && !is_letter('й', Layout::En));
        assert!(
            is_letter('ё', Layout::Ru) && is_letter('Ё', Layout::Ru) && !is_letter(',', Layout::Ru)
        );
    }
}
