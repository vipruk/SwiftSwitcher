//! The General page: language, auto-correction, start on login, the
//! background service and the setup check.

use super::{
    DaemonState, Page, Shell, blocking, daemon_owned, daemon_state, new_page, off_thread,
    start_daemon, update_config,
};
use crate::autostart;
use crate::check::{self, Item};
use crate::config::{Config, config_path};
use crate::i18n::{Lang, Msg, problem_text, tr};
use crate::setup;
use adw::prelude::*;
use gtk::glib;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

const LANGS: [Lang; 2] = [Lang::En, Lang::Ru];
const DAEMON_POLL: Duration = Duration::from_secs(3);
/// Gives a freshly started daemon time to own its bus name.
const START_SETTLE: Duration = Duration::from_secs(1);
/// A status call that takes longer leaves the row as it was.
const STATUS_TIMEOUT: Duration = Duration::from_secs(1);

/// Once per window: the first open ever turns Start on login on (R8), and
/// an enabled entry that starts an old path (say, an AppImage that moved)
/// is rewritten for this executable.
pub(super) fn on_open() {
    let dir = autostart::autostart_dir();
    let exec = autostart::exec_path();
    match autostart::init_default(&dir, &autostart::marker_path(), &exec) {
        Ok(true) => log::info!("start on login turned on by default"),
        Ok(false) => {}
        Err(e) => log::warn!("cannot set up start on login: {e}"),
    }
    if autostart::is_enabled(&dir) && autostart::is_stale(&dir, &exec) {
        match autostart::set_enabled(&dir, &exec, true) {
            Ok(()) => log::info!("autostart entry updated for {}", exec.display()),
            Err(e) => log::warn!("cannot update the autostart entry: {e}"),
        }
    }
}

pub(super) fn page(shell: &Shell, lang: Lang) -> adw::PreferencesPage {
    let page = new_page(Page::General, lang);
    page.add(&behavior_group(shell, lang));
    page.add(&daemon_group(shell, lang));
    let banner = adw::Banner::new(tr(lang, Msg::Relogin));
    let banner_group = adw::PreferencesGroup::new();
    banner_group.add(&banner);
    page.add(&banner_group);
    let checks = Rc::new(Checks {
        group: adw::PreferencesGroup::builder()
            .title(tr(lang, Msg::SetupCheck))
            .build(),
        rows: RefCell::default(),
        banner,
        shell: shell.clone(),
        lang,
    });
    page.add(&checks.group);
    run_checks(checks);
    page
}

fn save(shell: &Shell, lang: Lang, f: impl FnOnce(&mut Config)) {
    if let Err(e) = update_config(f) {
        log::warn!("cannot save {}: {e:#}", config_path().display());
        shell.toast(tr(lang, Msg::SaveFailed));
    }
}

fn behavior_group(shell: &Shell, lang: Lang) -> adw::PreferencesGroup {
    let config = Config::load_or_default(&config_path()).unwrap_or_default();
    let group = adw::PreferencesGroup::new();

    let names = gtk::StringList::new(&[tr(lang, Msg::LangEnglish), tr(lang, Msg::LangRussian)]);
    let language = adw::ComboRow::builder()
        .title(tr(lang, Msg::Language))
        .model(&names)
        .selected(LANGS.iter().position(|&l| l == lang).unwrap_or(0) as u32)
        .build();
    let s = shell.clone();
    language.connect_selected_notify(move |row| {
        let Some(&new) = LANGS.get(row.selected() as usize) else {
            return;
        };
        if new == lang {
            return;
        }
        save(&s, lang, |c| c.language = new.code().to_string());
        // Not from inside this row's own signal: the rebuild destroys it.
        let s = s.clone();
        glib::idle_add_local_once(move || s.rebuild(new));
    });
    group.add(&language);

    let auto = adw::SwitchRow::builder()
        .title(tr(lang, Msg::AutoCorrection))
        .subtitle(tr(lang, Msg::AutoCorrectionHint))
        .active(config.auto_correct)
        .build();
    let s = shell.clone();
    auto.connect_active_notify(move |row| {
        let on = row.is_active();
        save(&s, lang, |c| c.auto_correct = on);
    });
    group.add(&auto);

    let dir = autostart::autostart_dir();
    let login = adw::SwitchRow::builder()
        .title(tr(lang, Msg::StartOnLogin))
        .active(autostart::is_enabled(&dir))
        .build();
    let s = shell.clone();
    login.connect_active_notify(move |row| {
        let on = row.is_active();
        if let Err(e) = autostart::set_enabled(&dir, &autostart::exec_path(), on) {
            log::warn!("cannot change the autostart entry: {e}");
            s.toast(tr(lang, Msg::SaveFailed));
        }
    });
    group.add(&login);
    group
}

fn daemon_group(shell: &Shell, lang: Lang) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::new();
    let row = adw::ActionRow::builder()
        .title(tr(lang, Msg::Daemon))
        .build();
    let start = gtk::Button::builder()
        .label(tr(lang, Msg::Start))
        .valign(gtk::Align::Center)
        .visible(false)
        .build();
    row.add_suffix(&start);
    group.add(&row);

    let target = DaemonRow {
        row: row.downgrade(),
        start: start.downgrade(),
        pending: Rc::default(),
    };
    let s = shell.clone();
    let t = target.clone();
    start.connect_clicked(move |_| {
        if let Err(e) = start_daemon() {
            log::warn!("cannot start the daemon: {e}");
            s.toast(tr(lang, Msg::StartFailed));
            return;
        }
        let t = t.clone();
        glib::timeout_add_local_once(START_SETTLE, move || t.refresh(lang));
    });

    target.refresh(lang);
    // Stops once a rebuild or close drops the row.
    glib::timeout_add_local(DAEMON_POLL, move || {
        if target.row.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }
        target.refresh(lang);
        glib::ControlFlow::Continue
    });
    group
}

/// The daemon row, its Start button, and whether a status call is in flight.
#[derive(Clone)]
struct DaemonRow {
    row: glib::WeakRef<adw::ActionRow>,
    start: glib::WeakRef<gtk::Button>,
    pending: Rc<Cell<bool>>,
}

/// The daemon's state, or `Err` after [`STATUS_TIMEOUT`]. The timer is
/// created when the future is first polled, on the tokio runtime: building
/// `tokio::time::timeout` on the GTK thread panics (no reactor there).
async fn timed_daemon_state() -> Result<DaemonState, tokio::time::error::Elapsed> {
    tokio::time::timeout(STATUS_TIMEOUT, daemon_state()).await
}

impl DaemonRow {
    /// Asks the daemon for its status, at most one call at a time and for at
    /// most [`STATUS_TIMEOUT`]; a call that times out leaves the row as it is.
    fn refresh(&self, lang: Lang) {
        if self.pending.replace(true) {
            return;
        }
        let this = self.clone();
        glib::spawn_future_local(async move {
            let state = off_thread(timed_daemon_state()).await;
            this.pending.set(false);
            let Some(Ok(state)) = state else {
                return;
            };
            let (Some(row), Some(start)) = (this.row.upgrade(), this.start.upgrade()) else {
                return;
            };
            show_daemon(&row, &start, lang, state);
        });
    }
}

fn show_daemon(row: &adw::ActionRow, start: &gtk::Button, lang: Lang, state: DaemonState) {
    let subtitle = match state {
        DaemonState::NotRunning => tr(lang, Msg::DaemonNotRunning).to_string(),
        DaemonState::Running(None) => tr(lang, Msg::DaemonRunning).to_string(),
        DaemonState::Running(Some(p)) => format!(
            "{} · {}",
            tr(lang, Msg::DaemonRunning),
            problem_text(lang, p)
        ),
    };
    row.set_subtitle(&subtitle);
    start.set_visible(state == DaemonState::NotRunning);
}

/// The setup check group and the rows it currently shows.
struct Checks {
    group: adw::PreferencesGroup,
    rows: RefCell<Vec<adw::ActionRow>>,
    banner: adw::Banner,
    shell: Shell,
    lang: Lang,
}

fn check_title(id: &str) -> Msg {
    match id {
        "keyboard" => Msg::CheckKeyboard,
        "pointer" => Msg::CheckPointer,
        "uinput" => Msg::CheckUinput,
        "dbus" => Msg::CheckDbus,
        _ => Msg::CheckLayouts,
    }
}

fn check_failure(id: &str) -> Msg {
    match id {
        "keyboard" => Msg::FailKeyboard,
        "pointer" => Msg::ProblemPointer,
        "uinput" => Msg::FailUinput,
        "dbus" => Msg::ProblemExtension,
        _ => Msg::ProblemLayouts,
    }
}

/// Runs the checks off the GTK thread, then shows them. After the first
/// check since opening, starts the daemon if everything passed and it is
/// not running.
fn run_checks(checks: Rc<Checks>) {
    glib::spawn_future_local(async move {
        let Some(items) = off_thread(check::run_items()).await else {
            return;
        };
        for i in items.iter().filter(|i| !i.ok) {
            log::info!("setup check {}: {}", i.id, i.detail);
        }
        show_items(&checks, &items);
        if checks.shell.take_auto_start()
            && items.iter().all(|i| i.ok)
            && off_thread(daemon_owned()).await == Some(false)
        {
            log::info!("starting the daemon");
            if let Err(e) = start_daemon() {
                log::warn!("cannot start the daemon: {e}");
            }
        }
    });
}

fn show_items(checks: &Rc<Checks>, items: &[Item]) {
    let lang = checks.lang;
    for row in checks.rows.borrow_mut().drain(..) {
        checks.group.remove(&row);
    }
    let mut rows = Vec::new();
    for item in items {
        let (icon, subtitle) = if item.ok {
            ("emblem-ok-symbolic", tr(lang, Msg::CheckPassed))
        } else {
            ("dialog-warning-symbolic", tr(lang, check_failure(item.id)))
        };
        let row = adw::ActionRow::builder()
            .title(tr(lang, check_title(item.id)))
            .subtitle(subtitle)
            .build();
        row.add_prefix(&gtk::Image::from_icon_name(icon));
        if let Some(fix) = item.fix {
            let button = gtk::Button::builder()
                .label(tr(lang, Msg::Fix))
                .valign(gtk::Align::Center)
                .build();
            let c = checks.clone();
            button.connect_clicked(move |b| {
                b.set_sensitive(false);
                apply_fix(c.clone(), fix);
            });
            row.add_suffix(&button);
        }
        checks.group.add(&row);
        rows.push(row);
    }
    *checks.rows.borrow_mut() = rows;
}

fn apply_fix(checks: Rc<Checks>, fix: setup::Fix) {
    glib::spawn_future_local(async move {
        let lang = checks.lang;
        match blocking(move || setup::apply(fix)).await {
            Some(Ok(())) => {
                log::info!("fix {fix:?} applied");
                if fix.needs_relogin() {
                    checks.banner.set_revealed(true);
                }
            }
            Some(Err(e)) => {
                log::warn!("fix {fix:?} failed: {e:#}");
                checks.shell.toast(tr(lang, Msg::FixFailed));
            }
            None => checks.shell.toast(tr(lang, Msg::FixFailed)),
        }
        run_checks(checks);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_timeout_is_built_off_the_gtk_thread() {
        // This test thread has no tokio runtime, like the GTK main thread.
        let fut = off_thread(timed_daemon_state());
        // Completing at all (no panic) is the point; without a daemon on the
        // bus the state is whatever daemon_state reports.
        let _ = super::super::runtime().block_on(fut);
    }

    #[test]
    fn every_check_has_ui_text() {
        let ids = ["keyboard", "pointer", "uinput", "dbus", "layouts"];
        let titles: std::collections::HashSet<_> = ids.iter().map(|i| check_title(i)).collect();
        assert_eq!(titles.len(), ids.len());
        let fails: std::collections::HashSet<_> = ids.iter().map(|i| check_failure(i)).collect();
        assert_eq!(fails.len(), ids.len());
    }
}
