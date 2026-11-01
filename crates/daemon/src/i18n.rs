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
    HotkeyLastHint,
    HotkeySelectionHint,
    HotkeyChangeCase,
    HotkeyChangeCaseHint,
    Change,
    Reset,
    PressShortcut,
    PressShortcutHint,
    ErrModifierOnly,
    ErrUnsupportedKey,
    ErrTypingKey,
    ErrAppShortcut,
    ErrDuplicate,
    Cancel,
    HotkeysTip,
    NeverConvert,
    NeverConvertHint,
    AlwaysConvert,
    AlwaysConvertHint,
    WordPlaceholder,
    Remove,
    ExcludedApps,
    ExcludedAppsHint,
    AppPlaceholder,
    AddRunningApp,
    RunningApps,
    TypeAppId,
    AddButton,
    Version,
    Website,
    Privacy,
    PrivacyReads,
    PrivacyLocal,
    PrivacyUinput,
    Licenses,
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
        Msg::HotkeyLastHint,
        Msg::HotkeySelectionHint,
        Msg::HotkeyChangeCase,
        Msg::HotkeyChangeCaseHint,
        Msg::Change,
        Msg::Reset,
        Msg::PressShortcut,
        Msg::PressShortcutHint,
        Msg::ErrModifierOnly,
        Msg::ErrUnsupportedKey,
        Msg::ErrTypingKey,
        Msg::ErrAppShortcut,
        Msg::ErrDuplicate,
        Msg::Cancel,
        Msg::HotkeysTip,
        Msg::NeverConvert,
        Msg::NeverConvertHint,
        Msg::AlwaysConvert,
        Msg::AlwaysConvertHint,
        Msg::WordPlaceholder,
        Msg::Remove,
        Msg::ExcludedApps,
        Msg::ExcludedAppsHint,
        Msg::AppPlaceholder,
        Msg::AddRunningApp,
        Msg::RunningApps,
        Msg::TypeAppId,
        Msg::AddButton,
        Msg::Version,
        Msg::Website,
        Msg::Privacy,
        Msg::PrivacyReads,
        Msg::PrivacyLocal,
        Msg::PrivacyUinput,
        Msg::Licenses,
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
        (Msg::HotkeyLastHint, Lang::En) => "Convert last word / undo correction",
        (Msg::HotkeyLastHint, Lang::Ru) => "Исправить последнее слово / отменить исправление",
        (Msg::HotkeySelectionHint, Lang::En) => "Convert the selected text",
        (Msg::HotkeySelectionHint, Lang::Ru) => "Исправить выделенный текст",
        (Msg::HotkeyChangeCase, Lang::En) => "Change case of selection",
        (Msg::HotkeyChangeCase, Lang::Ru) => "Сменить регистр выделенного",
        (Msg::HotkeyChangeCaseHint, Lang::En) => {
            "Invert the case of each letter; the layout is not switched"
        }
        (Msg::HotkeyChangeCaseHint, Lang::Ru) => {
            "Инвертировать регистр каждой буквы; раскладка не меняется"
        }
        (Msg::Change, Lang::En) => "Change",
        (Msg::Change, Lang::Ru) => "Изменить",
        (Msg::Reset, Lang::En) => "Reset",
        (Msg::Reset, Lang::Ru) => "Сбросить",
        (Msg::PressShortcut, Lang::En) => "Press the new shortcut",
        (Msg::PressShortcut, Lang::Ru) => "Нажмите новое сочетание",
        (Msg::PressShortcutHint, Lang::En) => "Esc cancels.",
        (Msg::PressShortcutHint, Lang::Ru) => "Esc отменяет.",
        (Msg::ErrModifierOnly, Lang::En) => {
            "Press a key together with the modifiers, not modifiers alone"
        }
        (Msg::ErrModifierOnly, Lang::Ru) => {
            "Нажмите клавишу вместе с модификаторами, а не одни модификаторы"
        }
        (Msg::ErrUnsupportedKey, Lang::En) => {
            "This key is not supported. Use Pause, ScrollLock, Insert, Space, F1–F12 or a letter."
        }
        (Msg::ErrUnsupportedKey, Lang::Ru) => {
            "Эта клавиша не поддерживается. Используйте Pause, ScrollLock, Insert, пробел, F1–F12 или букву."
        }
        (Msg::ErrTypingKey, Lang::En) => {
            "A letter or Space types text, so it cannot be a shortcut on its own. Add Alt or Super, or use Pause, ScrollLock, Insert or F1–F12."
        }
        (Msg::ErrTypingKey, Lang::Ru) => {
            "Буква или пробел печатают текст, поэтому сами по себе не могут быть сочетанием. Добавьте Alt или Super либо используйте Pause, ScrollLock, Insert или F1–F12."
        }
        (Msg::ErrAppShortcut, Lang::En) => {
            "Ctrl with a letter or Space is an application shortcut (copy, select all…). Add Alt or Super, or use Pause, ScrollLock, Insert or F1–F12."
        }
        (Msg::ErrAppShortcut, Lang::Ru) => {
            "Ctrl с буквой или пробелом — это сочетание приложений (копировать, выделить всё…). Добавьте Alt или Super либо используйте Pause, ScrollLock, Insert или F1–F12."
        }
        (Msg::ErrDuplicate, Lang::En) => "The other command already uses this shortcut",
        (Msg::ErrDuplicate, Lang::Ru) => "Это сочетание уже занято другой командой",
        (Msg::Cancel, Lang::En) => "Cancel",
        (Msg::Cancel, Lang::Ru) => "Отмена",
        (Msg::HotkeysTip, Lang::En) => "No Pause key on your laptop? Try Ctrl+Alt+Space.",
        (Msg::HotkeysTip, Lang::Ru) => "Нет клавиши Pause на ноутбуке? Попробуйте Ctrl+Alt+Space.",
        (Msg::NeverConvert, Lang::En) => "Never convert",
        (Msg::NeverConvert, Lang::Ru) => "Никогда не исправлять",
        (Msg::NeverConvertHint, Lang::En) => "Words typed exactly like this are left alone.",
        (Msg::NeverConvertHint, Lang::Ru) => "Слова, набранные именно так, остаются как есть.",
        (Msg::AlwaysConvert, Lang::En) => "Always convert",
        (Msg::AlwaysConvert, Lang::Ru) => "Всегда исправлять",
        (Msg::AlwaysConvertHint, Lang::En) => "Words typed exactly like this are always converted.",
        (Msg::AlwaysConvertHint, Lang::Ru) => "Слова, набранные именно так, исправляются всегда.",
        (Msg::WordPlaceholder, Lang::En) => "Add a word",
        (Msg::WordPlaceholder, Lang::Ru) => "Добавить слово",
        (Msg::Remove, Lang::En) => "Remove",
        (Msg::Remove, Lang::Ru) => "Удалить",
        (Msg::ExcludedApps, Lang::En) => "Excluded applications",
        (Msg::ExcludedApps, Lang::Ru) => "Исключённые приложения",
        (Msg::ExcludedAppsHint, Lang::En) => "Auto-correction is off while one of these has focus.",
        (Msg::ExcludedAppsHint, Lang::Ru) => {
            "Пока в фокусе одно из этих приложений, автоисправление отключено."
        }
        (Msg::AppPlaceholder, Lang::En) => "Add an application id",
        (Msg::AppPlaceholder, Lang::Ru) => "Добавить идентификатор приложения",
        (Msg::AddRunningApp, Lang::En) => "Add running app…",
        (Msg::AddRunningApp, Lang::Ru) => "Добавить запущенное приложение…",
        (Msg::RunningApps, Lang::En) => "Running applications",
        (Msg::RunningApps, Lang::Ru) => "Запущенные приложения",
        (Msg::TypeAppId, Lang::En) => "Or type an application id",
        (Msg::TypeAppId, Lang::Ru) => "Или введите идентификатор приложения",
        (Msg::AddButton, Lang::En) => "Add",
        (Msg::AddButton, Lang::Ru) => "Добавить",
        (Msg::Version, Lang::En) => "Version",
        (Msg::Version, Lang::Ru) => "Версия",
        (Msg::Website, Lang::En) => "Website",
        (Msg::Website, Lang::Ru) => "Сайт",
        (Msg::Privacy, Lang::En) => "Privacy",
        (Msg::Privacy, Lang::Ru) => "Конфиденциальность",
        (Msg::PrivacyReads, Lang::En) => {
            "It reads all keyboard and pointer input, the id of the focused application, and the clipboard only while converting a selection. The clipboard is then restored, and its contents are never sent over D-Bus."
        }
        (Msg::PrivacyReads, Lang::Ru) => {
            "Программа читает весь ввод с клавиатуры и указателя, идентификатор активного приложения и буфер обмена только во время исправления выделенного текста. Затем буфер обмена восстанавливается, а его содержимое никогда не передаётся по D-Bus."
        }
        (Msg::PrivacyLocal, Lang::En) => {
            "SwiftSwitcher reads all keystrokes locally in order to work. Nothing typed is logged or sent anywhere: there is no network code. The only files it writes are: its config; the never and always word lists; undos.txt, which records the words you undid and how often, until they are learned; the autostart entry while Start on login is on; and an empty marker noting that autostart was set up."
        }
        (Msg::PrivacyLocal, Lang::Ru) => {
            "SwiftSwitcher читает все нажатия клавиш локально, чтобы работать. Ничто из набранного не записывается в журнал и никуда не отправляется: сетевого кода нет. Программа пишет только эти файлы: свои настройки; списки слов «никогда» и «всегда»; undos.txt, где записаны отменённые вами слова и сколько раз их отменяли, пока они не выучены; файл автозапуска, пока включён запуск при входе; и пустой маркер о том, что автозапуск был настроен."
        }
        (Msg::PrivacyUinput, Lang::En) => {
            "Device access is granted by a udev rule using the uaccess tag: the active desktop session gets read access to your keyboards, mice and touchpads, and write access to /dev/uinput. This is honest but broad: any program running in your active session can read those devices (see everything you type), and, through /dev/uinput, create a virtual keyboard that types into any window, not only SwiftSwitcher."
        }
        (Msg::PrivacyUinput, Lang::Ru) => {
            "Доступ к устройствам даёт правило udev с тегом uaccess: активный сеанс получает право чтения ваших клавиатур, мышей и тачпадов и право записи в /dev/uinput. Это честно, но широко: любая программа в вашем активном сеансе может читать эти устройства (видеть всё, что вы набираете) и через /dev/uinput создавать виртуальную клавиатуру, которая печатает в любое окно, а не только в SwiftSwitcher."
        }
        (Msg::Licenses, Lang::En) => "Code: MIT. Data: CC-BY-SA-4.0.",
        (Msg::Licenses, Lang::Ru) => "Код: MIT. Данные: CC-BY-SA-4.0.",
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
