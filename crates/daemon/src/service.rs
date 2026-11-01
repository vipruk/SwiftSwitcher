//! The daemon's own D-Bus service: single instance, status and quit.

use crate::Command;
#[cfg(test)]
use crate::status::Problem;
use crate::status::Status;
use tokio::sync::{mpsc, watch};
use zbus::fdo::RequestNameFlags;

pub const NAME: &str = "io.github.vipruk.SwiftSwitcher";
pub const PATH: &str = "/io/github/vipruk/SwiftSwitcher";

/// Requests the well-known name with `DoNotQueue`. True iff acquired;
/// false when another instance owns it.
pub async fn acquire(conn: &zbus::Connection) -> zbus::Result<bool> {
    match conn
        .request_name_with_flags(NAME, RequestNameFlags::DO_NOT_QUEUE)
        .await
    {
        Ok(_) => Ok(true),
        Err(zbus::Error::NameTaken) => Ok(false),
        Err(e) => Err(e),
    }
}

struct Daemon {
    status: watch::Receiver<Status>,
    cmd: mpsc::Sender<Command>,
}

#[zbus::interface(name = "io.github.vipruk.SwiftSwitcher.Daemon")]
impl Daemon {
    /// (auto_correct, problem code); the code is "" when there is no problem,
    /// else "extension", "layouts" or "pointer" (see `Problem::code`).
    fn status(&self) -> (bool, String) {
        let s = self.status.borrow();
        (
            s.auto_correct,
            s.problem.map_or("", |p| p.code()).to_string(),
        )
    }

    async fn quit(&self) {
        log::info!("quit requested over D-Bus");
        let _ = self.cmd.send(Command::Quit).await;
    }
}

/// Client side of the interface, for the settings window.
#[zbus::proxy(
    interface = "io.github.vipruk.SwiftSwitcher.Daemon",
    default_service = "io.github.vipruk.SwiftSwitcher",
    default_path = "/io/github/vipruk/SwiftSwitcher"
)]
pub trait Daemon {
    fn status(&self) -> zbus::Result<(bool, String)>;
    #[allow(dead_code)] // exercised by the tests
    fn quit(&self) -> zbus::Result<()>;
}

/// Exports the daemon interface at [`PATH`]. Call before [`acquire`], so no
/// call that arrives once the name is owned finds nothing there.
pub async fn serve(
    conn: &zbus::Connection,
    status_rx: watch::Receiver<Status>,
    cmd_tx: mpsc::Sender<Command>,
) -> zbus::Result<()> {
    let daemon = Daemon {
        status: status_rx,
        cmd: cmd_tx,
    };
    conn.object_server().at(PATH, daemon).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Stdio};

    /// A private session bus, killed on drop.
    struct Bus {
        child: Child,
        address: String,
    }

    impl Drop for Bus {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// A private session bus, or `None` (with a note) without `dbus-daemon`.
    fn private_bus() -> Option<Bus> {
        let spawned = std::process::Command::new("dbus-daemon")
            .args(["--session", "--print-address", "--nofork"])
            .stdout(Stdio::piped())
            .spawn();
        let mut child = match spawned {
            Ok(c) => c,
            Err(e) => {
                eprintln!("skipping: cannot run dbus-daemon ({e})");
                return None;
            }
        };
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        Some(Bus {
            child,
            address: line.trim().to_string(),
        })
    }

    async fn connect(bus: &Bus) -> zbus::Connection {
        zbus::connection::Builder::address(bus.address.as_str())
            .build()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn second_instance_exits() {
        let Some(bus) = private_bus() else {
            return;
        };
        let first = connect(&bus).await;
        let second = connect(&bus).await;
        assert!(
            acquire(&first).await.unwrap(),
            "first instance owns the name"
        );
        assert!(
            !acquire(&second).await.unwrap(),
            "second instance must exit"
        );
    }

    #[tokio::test]
    async fn status_and_quit_over_dbus() {
        let Some(bus) = private_bus() else {
            return;
        };
        let daemon = connect(&bus).await;
        let (status_tx, status_rx) = watch::channel(Status {
            auto_correct: true,
            problem: None,
        });
        let (cmd_tx, mut cmd_rx) = mpsc::channel(4);
        serve(&daemon, status_rx, cmd_tx).await.unwrap();
        assert!(acquire(&daemon).await.unwrap());

        let client = connect(&bus).await;
        let proxy = DaemonProxy::new(&client).await.unwrap();
        assert_eq!(proxy.status().await.unwrap(), (true, String::new()));
        status_tx.send_replace(Status {
            auto_correct: false,
            problem: Some(Problem::Pointer),
        });
        assert_eq!(
            proxy.status().await.unwrap(),
            (false, "pointer".to_string())
        );
        proxy.quit().await.unwrap();
        assert!(matches!(cmd_rx.recv().await, Some(Command::Quit)));
    }
}
