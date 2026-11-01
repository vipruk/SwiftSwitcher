//! Client for the GNOME Shell extension's D-Bus interface.

use anyhow::{Context, Result};
use std::future::Future;
use std::time::Duration;

const CALL_TIMEOUT: Duration = Duration::from_secs(1);

pub trait Desktop {
    /// Current input source id.
    async fn layout(&self) -> Result<String>;
    async fn layouts(&self) -> Result<Vec<String>>;
    async fn set_layout(&self, id: &str) -> Result<bool>;
    async fn focused_app(&self) -> Result<String>;
    /// Ids of the running apps, in the form `focused_app` reports.
    async fn running_apps(&self) -> Result<Vec<String>>;
    async fn selection_begin(&self) -> Result<bool>;
    /// Converts the clipboard; returns the input source id the text is now
    /// in, or "" if nothing was converted.
    async fn selection_convert(&self) -> Result<String>;
    /// Inverts the case of the clipboard text in place; false if there was
    /// nothing to change.
    async fn selection_change_case(&self) -> Result<bool>;
    async fn selection_end(&self) -> Result<bool>;
}

#[zbus::proxy(
    interface = "io.github.vipruk.SwiftSwitcher.Shell",
    default_service = "io.github.vipruk.SwiftSwitcher.Shell",
    default_path = "/io/github/vipruk/SwiftSwitcher/Shell"
)]
trait Shell {
    fn get_layout(&self) -> zbus::Result<String>;
    fn list_layouts(&self) -> zbus::Result<Vec<String>>;
    fn set_layout(&self, id: &str) -> zbus::Result<bool>;
    fn get_focused_app(&self) -> zbus::Result<String>;
    fn list_running_apps(&self) -> zbus::Result<Vec<String>>;
    fn selection_begin(&self) -> zbus::Result<bool>;
    fn selection_convert(&self) -> zbus::Result<String>;
    fn selection_change_case(&self) -> zbus::Result<bool>;
    fn selection_end(&self) -> zbus::Result<bool>;
}

pub struct GnomeDesktop {
    proxy: ShellProxy<'static>,
}

impl GnomeDesktop {
    pub async fn connect() -> Result<Self> {
        Self::on(&session().await?).await
    }

    /// A client on an existing connection; several may share one.
    pub async fn on(conn: &zbus::Connection) -> Result<Self> {
        let proxy = tokio::time::timeout(CALL_TIMEOUT, ShellProxy::new(conn))
            .await
            .context("proxy creation timed out")??;
        Ok(Self { proxy })
    }
}

/// Connects to the session bus, with the same timeout as a call.
pub async fn session() -> Result<zbus::Connection> {
    tokio::time::timeout(CALL_TIMEOUT, zbus::Connection::session())
        .await
        .context("session bus connect timed out")?
        .context("connecting to the session bus")
}

async fn call<T>(f: impl Future<Output = zbus::Result<T>>) -> Result<T> {
    Ok(tokio::time::timeout(CALL_TIMEOUT, f)
        .await
        .context("D-Bus call timed out")??)
}

impl Desktop for GnomeDesktop {
    async fn layout(&self) -> Result<String> {
        call(self.proxy.get_layout()).await
    }
    async fn layouts(&self) -> Result<Vec<String>> {
        call(self.proxy.list_layouts()).await
    }
    async fn set_layout(&self, id: &str) -> Result<bool> {
        call(self.proxy.set_layout(id)).await
    }
    async fn focused_app(&self) -> Result<String> {
        call(self.proxy.get_focused_app()).await
    }
    async fn running_apps(&self) -> Result<Vec<String>> {
        call(self.proxy.list_running_apps()).await
    }
    async fn selection_begin(&self) -> Result<bool> {
        call(self.proxy.selection_begin()).await
    }
    async fn selection_convert(&self) -> Result<String> {
        call(self.proxy.selection_convert()).await
    }
    async fn selection_change_case(&self) -> Result<bool> {
        call(self.proxy.selection_change_case()).await
    }
    async fn selection_end(&self) -> Result<bool> {
        call(self.proxy.selection_end()).await
    }
}
