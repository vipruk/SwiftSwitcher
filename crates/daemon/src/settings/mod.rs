//! The settings window (`swiftswitcher settings`), a GTK process of its own.
//!
//! Blocking work and zbus calls run on a tokio runtime kept in a
//! `OnceLock`; the GTK thread awaits their `JoinHandle`s from
//! `glib::spawn_future_local` and is never blocked.

mod about;
mod apps;
mod dictionary;
mod general;
mod hotkeys;
mod list;

use crate::autostart;
use crate::config::{Config, config_path};
use crate::i18n::{Lang, Msg, tr};
use crate::service::{self, DaemonProxy};
use crate::status::Problem;
use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;
use std::sync::OnceLock;

pub const APP_ID: &str = "io.github.vipruk.SwiftSwitcher.Settings";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Page {
    #[default]
    General,
    Hotkeys,
    Dictionary,
    Apps,
    About,
}

impl Page {
    const ALL: [Page; 5] = [
        Page::General,
        Page::Hotkeys,
        Page::Dictionary,
        Page::Apps,
        Page::About,
    ];

    /// The `--page` value, also the `AdwPreferencesPage` name.
    fn name(self) -> &'static str {
        match self {
            Page::General => "general",
            Page::Hotkeys => "hotkeys",
            Page::Dictionary => "dictionary",
            Page::Apps => "apps",
            Page::About => "about",
        }
    }

    fn from_name(s: &str) -> Option<Page> {
        Page::ALL.into_iter().find(|p| p.name() == s)
    }

    fn title(self) -> Msg {
        match self {
            Page::General => Msg::PageGeneral,
            Page::Hotkeys => Msg::PageHotkeys,
            Page::Dictionary => Msg::PageDictionary,
            Page::Apps => Msg::PageApps,
            Page::About => Msg::About,
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Page::General => "preferences-system-symbolic",
            Page::Hotkeys => "input-keyboard-symbolic",
            Page::Dictionary => "accessories-dictionary-symbolic",
            Page::Apps => "view-app-grid-symbolic",
            Page::About => "help-about-symbolic",
        }
    }
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("settings-io")
            .enable_all()
            .build()
            .expect("cannot start the tokio runtime")
    })
}

/// Runs `f` on the tokio runtime. None if it panicked.
async fn off_thread<F>(f: F) -> Option<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    runtime().spawn(f).await.ok()
}

/// Runs blocking `f` on the tokio runtime's blocking pool.
async fn blocking<F, T>(f: F) -> Option<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    runtime().spawn_blocking(f).await.ok()
}

async fn session() -> zbus::Result<zbus::Connection> {
    static CONN: tokio::sync::OnceCell<zbus::Connection> = tokio::sync::OnceCell::const_new();
    CONN.get_or_try_init(zbus::Connection::session)
        .await
        .cloned()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DaemonState {
    NotRunning,
    Running(Option<Problem>),
}

/// Runs on the tokio runtime.
async fn daemon_state() -> DaemonState {
    let Ok(conn) = session().await else {
        return DaemonState::NotRunning;
    };
    let status = match DaemonProxy::new(&conn).await {
        Ok(p) => p.status().await,
        Err(e) => Err(e),
    };
    match status {
        Ok((_, code)) => DaemonState::Running(Problem::from_code(&code)),
        Err(_) => DaemonState::NotRunning,
    }
}

/// Whether someone owns the daemon's bus name. Runs on the tokio runtime.
async fn daemon_owned() -> bool {
    let owned = async {
        let conn = session().await?;
        let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
        let name = zbus::names::BusName::try_from(service::NAME)?;
        Ok::<_, zbus::Error>(dbus.name_has_owner(name).await?)
    };
    // When unsure, do not start a second daemon.
    owned.await.unwrap_or(true)
}

/// Starts `<exec> run` in its own process group, so it outlives this window.
fn start_daemon() -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    let mut child = std::process::Command::new(autostart::exec_path())
        .arg("run")
        .stdin(std::process::Stdio::null())
        .process_group(0)
        .spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Loads the config, changes it and saves it atomically.
fn update_config(f: impl FnOnce(&mut Config)) -> anyhow::Result<()> {
    let path = config_path();
    let mut c = Config::load_or_default(&path)?;
    f(&mut c);
    c.save(&path)
}

fn config_lang() -> Lang {
    let c = Config::load_or_default(&config_path()).unwrap_or_else(|e| {
        log::warn!("{e:#}");
        Config::default()
    });
    Lang::from_config(&c.language)
}

/// The window and its pages, rebuilt whole when the language changes.
#[derive(Clone)]
struct Shell {
    window: adw::PreferencesWindow,
    pages: Rc<RefCell<Vec<adw::PreferencesPage>>>,
    /// Item lists of the current pages. Their widget callbacks hold only weak
    /// references, so this is what keeps them alive; emptied on rebuild and
    /// when the window closes.
    lists: Rc<RefCell<Vec<Rc<list::ItemList>>>>,
    /// Set until the first setup check finishes after opening.
    auto_start_pending: Rc<Cell<bool>>,
}

impl Shell {
    fn rebuild(&self, lang: Lang) {
        let visible = self.window.visible_page_name();
        self.lists.borrow_mut().clear();
        for p in self.pages.borrow_mut().drain(..) {
            self.window.remove(&p);
        }
        let pages = vec![
            general::page(self, lang),
            hotkeys::page(self, lang),
            dictionary::page(self, lang),
            apps::page(self, lang),
            about::page(self, lang),
        ];
        for p in &pages {
            self.window.add(p);
        }
        *self.pages.borrow_mut() = pages;
        self.window.set_title(Some(tr(lang, Msg::AppName)));
        if let Some(v) = visible {
            self.window.set_visible_page_name(&v);
        }
    }

    /// Keeps `list` alive until the next rebuild or the window closing.
    fn keep(&self, list: &Rc<list::ItemList>) {
        self.lists.borrow_mut().push(list.clone());
    }

    fn toast(&self, text: &str) {
        self.window.add_toast(adw::Toast::new(text));
    }

    /// True once: on the first call after the window opened.
    fn take_auto_start(&self) -> bool {
        self.auto_start_pending.replace(false)
    }
}

fn new_page(page: Page, lang: Lang) -> adw::PreferencesPage {
    adw::PreferencesPage::builder()
        .name(page.name())
        .title(tr(lang, page.title()))
        .icon_name(page.icon())
        .build()
}

fn build_window(app: &adw::Application) -> adw::PreferencesWindow {
    let window = adw::PreferencesWindow::builder()
        .application(app)
        .default_width(640)
        .default_height(720)
        .search_enabled(false)
        .build();
    let shell = Shell {
        window: window.clone(),
        pages: Rc::default(),
        lists: Rc::default(),
        auto_start_pending: Rc::new(Cell::new(true)),
    };
    general::on_open();
    // The lists' callbacks hold the shell, so release them with the window.
    let lists = shell.lists.clone();
    window.connect_close_request(move |_| {
        lists.borrow_mut().clear();
        glib::Propagation::Proceed
    });
    shell.rebuild(config_lang());
    window
}

fn present(app: &adw::Application, page: Page) {
    let existing = app
        .windows()
        .into_iter()
        .find_map(|w| w.downcast::<adw::PreferencesWindow>().ok());
    let window = existing.unwrap_or_else(|| build_window(app));
    window.set_visible_page_name(page.name());
    window.present();
}

/// Runs the settings window. A second launch presents the first one's
/// window at the requested page.
pub fn main(page: Page) -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    app.connect_command_line(|app, cmd| {
        let page = cmd
            .arguments()
            .get(1)
            .and_then(|a| a.to_str())
            .and_then(Page::from_name)
            .unwrap_or_default();
        present(app, page);
        glib::ExitCode::SUCCESS
    });
    // Only the page reaches GApplication, which would reject our options.
    app.run_with_args(&["swiftswitcher-settings", page.name()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::ValueEnum;

    #[test]
    fn page_names_match_cli() {
        for p in Page::ALL {
            assert_eq!(Page::from_name(p.name()), Some(p));
            let cli = p.to_possible_value().unwrap();
            assert_eq!(cli.get_name(), p.name());
        }
        assert_eq!(Page::from_name("bogus"), None);
    }
}
