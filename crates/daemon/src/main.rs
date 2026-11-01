mod autostart;
mod check;
mod config;
mod corrector;
mod desktop;
mod i18n;
mod inject;
mod input;
mod paths;
mod reload;
mod service;
mod settings;
mod setup;
mod status;
mod tray;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::sync::Arc;
use swift_engine::dict::UserDict;
use tokio::sync::{mpsc, watch};

#[derive(Parser)]
#[command(version, about = "Automatic RU/EN wrong-layout corrector")]
struct Cli {
    #[command(subcommand)]
    command: Option<Mode>,
}

#[derive(Subcommand)]
enum Mode {
    /// Start the daemon (the default).
    Run,
    /// Check that the environment is set up correctly.
    Check,
    /// Open the settings window.
    Settings {
        /// The page to show.
        #[arg(long, value_enum, default_value_t)]
        page: settings::Page,
    },
}

/// Runtime requests to the daemon's main loop.
// Sent about once per config edit; boxing would only add noise.
#[allow(clippy::large_enum_variant)]
pub enum Command {
    /// The config and dictionaries changed on disk and parsed.
    Reload(config::Config, UserDict),
    ConvertSelection,
    Quit,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match Cli::parse().command.unwrap_or(Mode::Run) {
        Mode::Run => tokio::runtime::Runtime::new()?.block_on(run()),
        Mode::Check => {
            if !tokio::runtime::Runtime::new()?.block_on(check::run()) {
                std::process::exit(1);
            }
            Ok(())
        }
        // GTK owns this thread; the window keeps its own tokio runtime.
        Mode::Settings { page } => {
            let code = settings::main(page);
            std::process::exit(i32::from(code.get()))
        }
    }
}

/// How often the config and dictionary files are checked for changes.
const RELOAD_POLL: std::time::Duration = std::time::Duration::from_secs(1);

async fn run() -> Result<()> {
    let config_path = config::config_path();
    let data_dir = config::data_dir();
    // Before loading, so a change made while starting up is not missed.
    let watcher = reload::Watcher::new([
        config_path.clone(),
        data_dir.join("never.txt"),
        data_dir.join("always.txt"),
    ]);
    let config = config::Config::load_or_default(&config_path)?;
    let dict = UserDict::load(&data_dir)?;

    // Single instance: own the bus name before opening any device or
    // creating the uinput device.
    let conn = desktop::session().await?;
    let (status_tx, status_rx) = watch::channel(status::Status {
        auto_correct: config.auto_correct,
        problem: None,
    });
    let (cmd_tx, mut cmd_rx) = mpsc::channel(16);
    service::serve(&conn, status_rx.clone(), cmd_tx.clone()).await?;
    if !service::acquire(&conn).await? {
        log::info!("swiftswitcher is already running");
        return Ok(());
    }

    let desktop = desktop::GnomeDesktop::on(&conn).await?;
    {
        use desktop::Desktop;
        match desktop.layouts().await {
            Ok(l) => match corrector::layout_status(&l) {
                Ok(()) => log::info!("layouts ok: {l:?}"),
                Err(why) => log::warn!("auto-correction disabled: {why}"),
            },
            Err(e) => log::warn!("cannot list layouts: {e:#}"),
        }
    }
    let injector = inject::UinputInjector::new()?;
    let activity = Arc::new(corrector::Activity::new());
    let (config_tx, config_rx) = watch::channel(config.clone());
    let mut corrector = corrector::Corrector::new(
        desktop,
        injector,
        config,
        dict,
        data_dir,
        activity.clone(),
        input::initial_caps_lock(),
    )?;

    let (raw_tx, raw_rx) = mpsc::channel(256);
    let (tx, mut rx) = mpsc::channel(256);
    input::spawn_reader(raw_tx, activity.clone())?;
    tokio::spawn(corrector::forward(
        raw_rx,
        tx,
        activity.clone(),
        corrector.shared_hotkeys(),
    ));
    tokio::spawn(status::refresh(
        desktop::GnomeDesktop::on(&conn).await?,
        activity,
        config_rx,
        status_tx,
    ));
    // Off the startup path: a slow StatusNotifierWatcher must not delay
    // the event loop.
    let tray_tx = cmd_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = tray::spawn(status_rx, tray_tx).await {
            log::warn!("tray icon failed: {e:#}");
        }
    });
    tokio::spawn(watcher.run(RELOAD_POLL, cmd_tx));
    log::info!("swiftswitcher running");

    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    loop {
        tokio::select! {
            ev = rx.recv() => {
                let Some(ev) = ev else { anyhow::bail!("input channel closed") };
                if let Err(e) = corrector.handle(ev).await {
                    log::warn!("handling failed: {e:#}");
                }
            }
            Some(cmd) = cmd_rx.recv() => match cmd {
                Command::Reload(config, dict) => {
                    let shared = config.clone();
                    match corrector.apply(config, dict) {
                        Ok(()) => {
                            log::info!("configuration reloaded");
                            config_tx.send_replace(shared);
                        }
                        Err(e) => log::warn!("configuration not applied: {e:#}"),
                    }
                }
                Command::ConvertSelection => {
                    if let Err(e) = corrector.convert_selection_now().await {
                        log::warn!("selection convert failed: {e:#}");
                    }
                }
                Command::Quit => break,
            },
            _ = tokio::signal::ctrl_c() => break,
            _ = term.recv() => break,
        }
    }
    log::info!("shutting down");
    Ok(())
}
