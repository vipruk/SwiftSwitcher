mod check;
mod config;
mod corrector;
mod desktop;
mod inject;
mod input;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::sync::Arc;
use swift_engine::dict::UserDict;
use tokio::sync::mpsc;

#[derive(Parser)]
#[command(version, about = "Automatic RU/EN wrong-layout corrector")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start the daemon (the default).
    Run,
    /// Check that the environment is set up correctly.
    Check,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match Cli::parse().command.unwrap_or(Command::Run) {
        Command::Run => run().await,
        Command::Check => {
            if !check::run().await {
                std::process::exit(1);
            }
            Ok(())
        }
    }
}

async fn run() -> Result<()> {
    let config = config::Config::load_or_default(&config::config_path())?;
    let data_dir = config::data_dir();
    let dict = UserDict::load(&data_dir)?;
    // The same hotkeys must reach `forward` and the Corrector.
    let hotkeys = config.hotkeys()?;
    let desktop = desktop::GnomeDesktop::connect().await?;
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
    input::spawn_reader(raw_tx)?;
    tokio::spawn(corrector::forward(raw_rx, tx, activity, hotkeys));
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
            _ = tokio::signal::ctrl_c() => break,
            _ = term.recv() => break,
        }
    }
    log::info!("shutting down");
    Ok(())
}
