//! System tray icon (StatusNotifierItem via ksni).
//!
//! The icon follows the daemon `Status`. Labels follow the configured
//! language: the tray task polls the modification time of config.toml
//! every 2 s and re-reads the language when it changes (a reload
//! does not necessarily change `Status`, so that channel cannot be relied on).

use crate::Command;
use crate::config::{Config, config_path};
use crate::i18n::{Lang, Msg, problem_text, tr};
use crate::paths;
use crate::status::Status;
use anyhow::Result;
use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{MenuItem, Tray, TrayMethods};
use std::time::{Duration, SystemTime};
use tokio::sync::{mpsc, watch};

const ICON: &str = "io.github.vipruk.SwiftSwitcher-symbolic";
const ICON_OFF: &str = "io.github.vipruk.SwiftSwitcher-off-symbolic";

/// Lets the menu close and focus return to the previous window before
/// the selection is converted.
const CONVERT_DELAY: Duration = Duration::from_millis(300);
const LANG_POLL: Duration = Duration::from_secs(2);

struct SwiftTray {
    status: Status,
    lang: Lang,
    cmd_tx: mpsc::Sender<Command>,
}

impl SwiftTray {
    fn new(status: Status, lang: Lang, cmd_tx: mpsc::Sender<Command>) -> Self {
        SwiftTray {
            status,
            lang,
            cmd_tx,
        }
    }
}

/// Starts `swiftswitcher <args>` without waiting for it.
fn spawn_self(args: &[&str]) {
    let result =
        std::env::current_exe().and_then(|exe| std::process::Command::new(exe).args(args).spawn());
    match result {
        // Dropping the Child neither waits nor kills; reap it so it
        // does not stay a zombie.
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => log::warn!("cannot start `swiftswitcher {}`: {e}", args.join(" ")),
    }
}

fn toggle_auto_correct() {
    let path = config_path();
    let result = Config::load_or_default(&path).and_then(|mut c| {
        c.auto_correct = !c.auto_correct;
        c.save(&path)
    });
    if let Err(e) = result {
        log::warn!("cannot toggle auto-correction: {e:#}");
    }
}

impl Tray for SwiftTray {
    fn id(&self) -> String {
        "io.github.vipruk.SwiftSwitcher".into()
    }

    fn title(&self) -> String {
        tr(self.lang, Msg::AppName).into()
    }

    fn icon_theme_path(&self) -> String {
        paths::icons_dir().to_string_lossy().into_owned()
    }

    fn icon_name(&self) -> String {
        if self.status.auto_correct && self.status.problem.is_none() {
            ICON
        } else {
            ICON_OFF
        }
        .into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            icon_name: self.icon_name(),
            title: tr(self.lang, Msg::AppName).into(),
            description: self
                .status
                .problem
                .map(|p| problem_text(self.lang, p).to_string())
                .unwrap_or_default(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let l = self.lang;
        vec![
            CheckmarkItem {
                label: tr(l, Msg::AutoCorrection).into(),
                checked: self.status.auto_correct,
                // The reload path updates the status; the check mark
                // follows it.
                activate: Box::new(|_| toggle_auto_correct()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: tr(l, Msg::ConvertSelection).into(),
                activate: Box::new(|t: &mut Self| {
                    let tx = t.cmd_tx.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(CONVERT_DELAY);
                        let _ = tx.blocking_send(Command::ConvertSelection);
                    });
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: tr(l, Msg::Settings).into(),
                activate: Box::new(|_| spawn_self(&["settings"])),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: tr(l, Msg::About).into(),
                activate: Box::new(|_| spawn_self(&["settings", "--page", "about"])),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: tr(l, Msg::Quit).into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.cmd_tx.try_send(Command::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

fn mtime() -> Option<SystemTime> {
    std::fs::metadata(config_path())
        .and_then(|m| m.modified())
        .ok()
}

fn current_lang() -> Lang {
    let c = Config::load_or_default(&config_path()).unwrap_or_default();
    Lang::from_config(&c.language)
}

/// Shows the tray icon. Without a StatusNotifierWatcher this logs one
/// info line and returns Ok: the daemon works without a tray.
pub async fn spawn(
    mut status_rx: watch::Receiver<Status>,
    cmd_tx: mpsc::Sender<Command>,
) -> Result<()> {
    let tray = SwiftTray::new(status_rx.borrow().clone(), current_lang(), cmd_tx);
    let handle = match tray.spawn().await {
        Ok(h) => h,
        Err(e) => {
            log::info!("no tray icon: {e}");
            return Ok(());
        }
    };
    tokio::spawn(async move {
        let mut seen = mtime();
        let mut tick = tokio::time::interval(LANG_POLL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                r = status_rx.changed() => {
                    if r.is_err() { break }
                    let s = status_rx.borrow_and_update().clone();
                    if handle.update(|t| t.status = s).await.is_none() { break }
                }
                _ = tick.tick() => {
                    let now = mtime();
                    if now != seen {
                        seen = now;
                        let lang = current_lang();
                        if handle.update(|t| t.lang = lang).await.is_none() { break }
                    }
                }
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::{Problem, Status};
    use ksni::{MenuItem, Tray};

    fn tray(auto_correct: bool, problem: Option<Problem>) -> SwiftTray {
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        SwiftTray::new(
            Status {
                auto_correct,
                problem,
            },
            crate::i18n::Lang::En,
            tx,
        )
    }

    #[test]
    fn menu_reflects_status() {
        for on in [true, false] {
            let t = tray(on, None);
            let menu = t.menu();
            let actionable: Vec<_> = menu
                .iter()
                .filter(|i| !matches!(i, MenuItem::Separator))
                .collect();
            assert_eq!(actionable.len(), 5);
            match actionable[0] {
                MenuItem::Checkmark(c) => assert_eq!(c.checked, on),
                _ => panic!("first item must be the checkmark"),
            }
        }
        assert_eq!(tray(true, None).icon_name(), ICON);
        assert_eq!(tray(false, None).icon_name(), ICON_OFF);
        assert_eq!(tray(true, Some(Problem::Pointer)).icon_name(), ICON_OFF);
        assert!(tray(true, None).tool_tip().description.is_empty());
        assert!(
            !tray(true, Some(Problem::Layouts))
                .tool_tip()
                .description
                .is_empty()
        );
    }
}
