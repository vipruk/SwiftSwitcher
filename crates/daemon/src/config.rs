use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use swift_engine::hotkey::{Hotkey, HotkeyAction};

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Config {
    pub auto_correct: bool,
    pub convert_last: String,
    pub convert_selection: String,
    pub excluded_apps: Vec<String>,
    pub terminal_apps: Vec<String>,
    pub idle_reset_secs: u64,
    pub settle_ms: u64,
    pub learn_after_undos: u32,
    pub language: String,
}

impl Default for Config {
    fn default() -> Self {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect();
        Config {
            auto_correct: true,
            convert_last: "Pause".into(),
            convert_selection: "Shift+Pause".into(),
            excluded_apps: s(&["org.keepassxc.KeePassXC", "bitwarden", "1password"]),
            terminal_apps: s(&[
                "org.gnome.Terminal",
                "org.gnome.Ptyxis",
                "org.gnome.Console",
                "kitty",
                "Alacritty",
                "org.wezfurlong.wezterm",
                "foot",
                "com.gexperts.Tilix",
                "xterm",
                "org.kde.konsole",
            ]),
            idle_reset_secs: 5,
            settle_ms: 40,
            learn_after_undos: 3,
            language: "en".into(),
        }
    }
}

fn contains_ci(list: &[String], app_id: &str) -> bool {
    !app_id.is_empty() && list.iter().any(|a| a.eq_ignore_ascii_case(app_id))
}

impl Config {
    /// Missing file gives defaults; a malformed file is an error.
    pub fn load_or_default(path: &Path) -> anyhow::Result<Config> {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("parsing config {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(e).with_context(|| format!("reading config {}", path.display())),
        }
    }

    /// Atomic: writes `<name>.tmp` next to the file, then renames it.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let text = toml::to_string_pretty(self).context("serializing config")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))
    }

    pub fn hotkeys(&self) -> anyhow::Result<Vec<(Hotkey, HotkeyAction)>> {
        let parse = |field: &str, s: &str| {
            Hotkey::parse(s).map_err(|e| anyhow::anyhow!("invalid {field} hotkey {s:?}: {e}"))
        };
        Ok(vec![
            (
                parse("convert_last", &self.convert_last)?,
                HotkeyAction::ConvertLast,
            ),
            (
                parse("convert_selection", &self.convert_selection)?,
                HotkeyAction::ConvertSelection,
            ),
        ])
    }

    pub fn is_excluded(&self, app_id: &str) -> bool {
        contains_ci(&self.excluded_apps, app_id)
    }

    pub fn is_terminal(&self, app_id: &str) -> bool {
        contains_ci(&self.terminal_apps, app_id)
    }
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("swiftswitcher")
        .join("config.toml")
}

pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("swiftswitcher")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &tempfile::TempDir, text: &str) -> PathBuf {
        let p = dir.path().join("config.toml");
        std::fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let c = Config::load_or_default(&dir.path().join("nope.toml")).unwrap();
        assert_eq!(c, Config::default());
        assert!(c.auto_correct);
        assert_eq!(c.convert_last, "Pause");
        assert_eq!(c.convert_selection, "Shift+Pause");
        assert_eq!(c.idle_reset_secs, 5);
        assert_eq!(c.settle_ms, 40);
        assert_eq!(c.learn_after_undos, 3);
        assert_eq!(c.excluded_apps.len(), 3);
        assert_eq!(c.terminal_apps.len(), 10);
    }

    #[test]
    fn partial_file_keeps_other_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(&dir, "auto_correct = false\n");
        let c = Config::load_or_default(&p).unwrap();
        assert!(!c.auto_correct);
        assert_eq!(
            c,
            Config {
                auto_correct: false,
                ..Config::default()
            }
        );
    }

    #[test]
    fn malformed_file_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = write(&dir, "auto_correct = [oops");
        assert!(Config::load_or_default(&p).is_err());
    }

    #[test]
    fn default_hotkeys_parse() {
        let h = Config::default().hotkeys().unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].1, HotkeyAction::ConvertLast);
        assert_eq!(h[1].1, HotkeyAction::ConvertSelection);
        assert!(h[1].0.shift);
    }

    #[test]
    fn bad_hotkey_names_field() {
        let c = Config {
            convert_last: "Bogus+Nope".into(),
            ..Config::default()
        };
        let e = c.hotkeys().unwrap_err().to_string();
        assert!(e.contains("convert_last"), "{e}");
        let c = Config {
            convert_selection: "???".into(),
            ..Config::default()
        };
        let e = c.hotkeys().unwrap_err().to_string();
        assert!(e.contains("convert_selection"), "{e}");
    }

    #[test]
    fn app_matching_is_case_insensitive() {
        let c = Config::default();
        assert!(c.is_excluded("BITWARDEN"));
        assert!(c.is_excluded("org.keepassxc.keepassxc"));
        assert!(!c.is_excluded("firefox"));
        assert!(!c.is_excluded(""));
        assert!(c.is_terminal("KITTY"));
        assert!(c.is_terminal("alacritty"));
        assert!(!c.is_terminal("firefox"));
        assert!(!c.is_terminal(""));
    }

    #[test]
    fn paths_end_as_specified() {
        assert!(config_path().ends_with("swiftswitcher/config.toml"));
        assert!(data_dir().ends_with("swiftswitcher"));
    }

    #[test]
    fn save_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub").join("config.toml");
        let c = Config {
            auto_correct: false,
            convert_last: "Ctrl+Alt+Space".into(),
            language: "ru".into(),
            learn_after_undos: 7,
            ..Config::default()
        };
        c.save(&p).unwrap();
        assert_eq!(Config::load_or_default(&p).unwrap(), c);
        assert!(!dir.path().join("sub").join("config.toml.tmp").exists());
    }

    #[test]
    fn language_defaults_to_en() {
        assert_eq!(Config::default().language, "en");
    }
}
