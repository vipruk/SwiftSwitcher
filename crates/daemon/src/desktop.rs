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
    async fn selection_begin(&self) -> Result<bool>;
    async fn selection_convert(&self) -> Result<bool>;
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
    fn selection_begin(&self) -> zbus::Result<bool>;
    fn selection_convert(&self) -> zbus::Result<bool>;
    fn selection_end(&self) -> zbus::Result<bool>;
}

pub struct GnomeDesktop {
    proxy: ShellProxy<'static>,
}

impl GnomeDesktop {
    pub async fn connect() -> Result<Self> {
        let conn = tokio::time::timeout(CALL_TIMEOUT, zbus::Connection::session())
            .await
            .context("session bus connect timed out")??;
        let proxy = tokio::time::timeout(CALL_TIMEOUT, ShellProxy::new(&conn))
            .await
            .context("proxy creation timed out")??;
        Ok(Self { proxy })
    }
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
    async fn selection_begin(&self) -> Result<bool> {
        call(self.proxy.selection_begin()).await
    }
    async fn selection_convert(&self) -> Result<bool> {
        call(self.proxy.selection_convert()).await
    }
    async fn selection_end(&self) -> Result<bool> {
        call(self.proxy.selection_end()).await
    }
}
