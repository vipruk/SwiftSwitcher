//! UI strings. English and Russian live in one exhaustive table, so a
//! missing translation is a compile error.

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

    /// The config value, the inverse of [`Lang::from_config`].
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ru => "ru",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    PageGeneral,
    PageHotkeys,
    PageDictionary,
    PageApps,
    Language,
    LangEnglish,
    LangRussian,
    AutoCorrectionHint,
    StartOnLogin,
    SaveFailed,
    Daemon,
    DaemonRunning,
    DaemonNotRunning,
    Start,
    StartFailed,
    SetupCheck,
    CheckPassed,
    CheckKeyboard,
    CheckPointer,
    CheckUinput,
    CheckDbus,
    CheckLayouts,
    FailKeyboard,
    FailUinput,
    Fix,
    FixFailed,
    Relogin,
}

#[cfg(test)]
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
        Msg::PageGeneral,
        Msg::PageHotkeys,
        Msg::PageDictionary,
        Msg::PageApps,
        Msg::Language,
        Msg::LangEnglish,
        Msg::LangRussian,
        Msg::AutoCorrectionHint,
        Msg::StartOnLogin,
        Msg::SaveFailed,
        Msg::Daemon,
        Msg::DaemonRunning,
        Msg::DaemonNotRunning,
        Msg::Start,
        Msg::StartFailed,
        Msg::SetupCheck,
        Msg::CheckPassed,
        Msg::CheckKeyboard,
        Msg::CheckPointer,
        Msg::CheckUinput,
        Msg::CheckDbus,
        Msg::CheckLayouts,
        Msg::FailKeyboard,
        Msg::FailUinput,
        Msg::Fix,
        Msg::FixFailed,
        Msg::Relogin,
    ];

    /// Proper names that read the same in every language.
    fn is_proper_name(self) -> bool {
        matches!(self, Msg::AppName | Msg::LangEnglish | Msg::LangRussian)
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
        (Msg::PageGeneral, Lang::En) => "General",
        (Msg::PageGeneral, Lang::Ru) => "Общие",
        (Msg::PageHotkeys, Lang::En) => "Hotkeys",
        (Msg::PageHotkeys, Lang::Ru) => "Горячие клавиши",
        (Msg::PageDictionary, Lang::En) => "Dictionary",
        (Msg::PageDictionary, Lang::Ru) => "Словарь",
        (Msg::PageApps, Lang::En) => "Applications",
        (Msg::PageApps, Lang::Ru) => "Приложения",
        (Msg::Language, Lang::En) => "Language",
        (Msg::Language, Lang::Ru) => "Язык",
        (Msg::LangEnglish, _) => "English",
        (Msg::LangRussian, _) => "Русский",
        (Msg::AutoCorrectionHint, Lang::En) => "Fix words typed in the wrong layout",
        (Msg::AutoCorrectionHint, Lang::Ru) => "Исправлять слова, набранные не в той раскладке",
        (Msg::StartOnLogin, Lang::En) => "Start on login",
        (Msg::StartOnLogin, Lang::Ru) => "Запускать при входе",
        (Msg::SaveFailed, Lang::En) => "Could not save the settings",
        (Msg::SaveFailed, Lang::Ru) => "Не удалось сохранить настройки",
        (Msg::Daemon, Lang::En) => "Background service",
        (Msg::Daemon, Lang::Ru) => "Фоновая служба",
        (Msg::DaemonRunning, Lang::En) => "Running",
        (Msg::DaemonRunning, Lang::Ru) => "Работает",
        (Msg::DaemonNotRunning, Lang::En) => "Not running",
        (Msg::DaemonNotRunning, Lang::Ru) => "Не запущена",
        (Msg::Start, Lang::En) => "Start",
        (Msg::Start, Lang::Ru) => "Запустить",
        (Msg::StartFailed, Lang::En) => "Could not start the background service",
        (Msg::StartFailed, Lang::Ru) => "Не удалось запустить фоновую службу",
        (Msg::SetupCheck, Lang::En) => "Setup check",
        (Msg::SetupCheck, Lang::Ru) => "Проверка настройки",
        (Msg::CheckPassed, Lang::En) => "Passed",
        (Msg::CheckPassed, Lang::Ru) => "В порядке",
        (Msg::CheckKeyboard, Lang::En) => "Keyboard access",
        (Msg::CheckKeyboard, Lang::Ru) => "Доступ к клавиатуре",
        (Msg::CheckPointer, Lang::En) => "Mouse or touchpad access",
        (Msg::CheckPointer, Lang::Ru) => "Доступ к мыши или тачпаду",
        (Msg::CheckUinput, Lang::En) => "Virtual keyboard (/dev/uinput)",
        (Msg::CheckUinput, Lang::Ru) => "Виртуальная клавиатура (/dev/uinput)",
        (Msg::CheckDbus, Lang::En) => "GNOME Shell extension",
        (Msg::CheckDbus, Lang::Ru) => "Расширение GNOME Shell",
        (Msg::CheckLayouts, Lang::En) => "Input sources",
        (Msg::CheckLayouts, Lang::Ru) => "Раскладки",
        (Msg::FailKeyboard, Lang::En) => "No readable keyboard was found",
        (Msg::FailKeyboard, Lang::Ru) => "Не найдено доступной клавиатуры",
        (Msg::FailUinput, Lang::En) => "Cannot write to /dev/uinput",
        (Msg::FailUinput, Lang::Ru) => "Нет доступа на запись к /dev/uinput",
        (Msg::Fix, Lang::En) => "Fix",
        (Msg::Fix, Lang::Ru) => "Исправить",
        (Msg::FixFailed, Lang::En) => "The fix did not succeed",
        (Msg::FixFailed, Lang::Ru) => "Исправить не удалось",
        (Msg::Relogin, Lang::En) => "Log out and back in to finish",
        (Msg::Relogin, Lang::Ru) => "Чтобы завершить, выйдите из системы и войдите снова",
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
        for l in [Lang::En, Lang::Ru] {
            assert_eq!(Lang::from_config(l.code()), l);
        }
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
