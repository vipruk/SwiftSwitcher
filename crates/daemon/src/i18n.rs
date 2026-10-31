//! UI strings. English and Russian live in one exhaustive table, so a
//! missing translation is a compile error.

// Consumers arrive in later tasks.
#![allow(dead_code)]

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Ru,
}

impl Lang {
    /// Unknown values fall back to English.
    pub fn from_config(s: &str) -> Lang {
        match s.trim().to_ascii_lowercase().as_str() {
            "ru" => Lang::Ru,
            _ => Lang::En,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Msg {
    AppName,
    AutoCorrection,
}

impl Msg {
    pub const ALL: &'static [Msg] = &[Msg::AppName, Msg::AutoCorrection];

    /// Proper names that read the same in every language.
    fn is_proper_name(self) -> bool {
        matches!(self, Msg::AppName)
    }
}

pub fn tr(lang: Lang, m: Msg) -> &'static str {
    match (m, lang) {
        (Msg::AppName, _) => "SwiftSwitcher",
        (Msg::AutoCorrection, Lang::En) => "Auto-correction",
        (Msg::AutoCorrection, Lang::Ru) => "Автоисправление",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_language_is_english() {
        assert_eq!(Lang::from_config("de"), Lang::En);
        assert_eq!(Lang::from_config(""), Lang::En);
        assert_eq!(Lang::from_config("en"), Lang::En);
        assert_eq!(Lang::from_config("ru"), Lang::Ru);
    }

    #[test]
    fn every_msg_has_both() {
        for &m in Msg::ALL {
            let (en, ru) = (tr(Lang::En, m), tr(Lang::Ru, m));
            assert!(!en.is_empty() && !ru.is_empty(), "{m:?}");
            if !m.is_proper_name() {
                assert_ne!(en, ru, "{m:?}");
            }
        }
    }
}
