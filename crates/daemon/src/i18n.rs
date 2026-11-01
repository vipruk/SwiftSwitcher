//! UI strings. English and Russian live in one exhaustive table, so a
//! missing translation is a compile error.

// Consumers arrive in later tasks.
#![allow(dead_code)]

use crate::status::Problem;

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
    ConvertSelection,
    Settings,
    About,
    Quit,
    ProblemExtension,
    ProblemLayouts,
    ProblemPointer,
}

impl Msg {
    pub const ALL: &'static [Msg] = &[
        Msg::AppName,
        Msg::AutoCorrection,
        Msg::ConvertSelection,
        Msg::Settings,
        Msg::About,
        Msg::Quit,
        Msg::ProblemExtension,
        Msg::ProblemLayouts,
        Msg::ProblemPointer,
    ];

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
        (Msg::ConvertSelection, Lang::En) => "Convert selection",
        (Msg::ConvertSelection, Lang::Ru) => "Исправить выделенное",
        (Msg::Settings, Lang::En) => "Settings…",
        (Msg::Settings, Lang::Ru) => "Настройки…",
        (Msg::About, Lang::En) => "About",
        (Msg::About, Lang::Ru) => "О программе",
        (Msg::Quit, Lang::En) => "Quit",
        (Msg::Quit, Lang::Ru) => "Выход",
        (Msg::ProblemExtension, Lang::En) => "The GNOME Shell extension is not reachable",
        (Msg::ProblemExtension, Lang::Ru) => "Расширение GNOME Shell недоступно",
        (Msg::ProblemLayouts, Lang::En) => {
            "Exactly two input sources are required: English (US) and Russian"
        }
        (Msg::ProblemLayouts, Lang::Ru) => "Нужны ровно две раскладки: английская (US) и русская",
        (Msg::ProblemPointer, Lang::En) => {
            "No pointer device is readable, so automatic correction is off"
        }
        (Msg::ProblemPointer, Lang::Ru) => {
            "Мышь или тачпад недоступны, поэтому автоисправление выключено"
        }
    }
}

/// UI text for a status problem, for the tray and the settings window.
pub fn problem_text(lang: Lang, p: Problem) -> &'static str {
    let m = match p {
        Problem::Extension => Msg::ProblemExtension,
        Problem::Layouts => Msg::ProblemLayouts,
        Problem::Pointer => Msg::ProblemPointer,
    };
    tr(lang, m)
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
    fn problem_text_per_reason() {
        assert_eq!(
            problem_text(Lang::En, Problem::Extension),
            tr(Lang::En, Msg::ProblemExtension)
        );
        assert_eq!(
            problem_text(Lang::Ru, Problem::Layouts),
            tr(Lang::Ru, Msg::ProblemLayouts)
        );
        assert_eq!(
            problem_text(Lang::Ru, Problem::Pointer),
            tr(Lang::Ru, Msg::ProblemPointer)
        );
        let en: std::collections::HashSet<_> = Problem::ALL
            .iter()
            .map(|&p| problem_text(Lang::En, p))
            .collect();
        assert_eq!(en.len(), Problem::ALL.len(), "distinct texts");
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
