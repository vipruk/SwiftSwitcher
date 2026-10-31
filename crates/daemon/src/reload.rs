//! Hot reload: polls the config and dictionary files' mtimes.

use crate::Command;
use crate::config::Config;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use swift_engine::dict::UserDict;
use tokio::sync::mpsc::Sender;

/// Result of one [`Watcher::poll`].
#[derive(Debug)]
// Sent about once per config edit; boxing would only add noise.
#[allow(clippy::large_enum_variant)]
pub enum Poll {
    Unchanged,
    Reload(Config, UserDict),
    /// Something changed but could not be loaded; one warning was logged.
    Invalid,
}

/// Watches `[config, never, always]`; the two lists must share a directory,
/// which is where the dictionaries are loaded from.
pub struct Watcher {
    paths: [PathBuf; 3],
    seen: [Option<SystemTime>; 3],
}

impl Watcher {
    /// Records the current mtimes: files as they are now are not a change.
    pub fn new(paths: [PathBuf; 3]) -> Self {
        let seen = mtimes(&paths);
        Watcher { paths, seen }
    }

    /// Checks the mtimes once. A change reloads everything; a malformed
    /// config (or one with an invalid hotkey) is reported once per change.
    pub fn poll(&mut self) -> Poll {
        let now = mtimes(&self.paths);
        if now == self.seen {
            return Poll::Unchanged;
        }
        self.seen = now;
        let config = match Config::load_or_default(&self.paths[0]) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("config not reloaded, keeping the previous one: {e:#}");
                return Poll::Invalid;
            }
        };
        if let Err(e) = config.hotkeys() {
            log::warn!("config not reloaded, keeping the previous one: {e:#}");
            return Poll::Invalid;
        }
        let dir = self.paths[1].parent().unwrap_or(Path::new("."));
        match UserDict::load(dir) {
            Ok(dict) => Poll::Reload(config, dict),
            Err(e) => {
                log::warn!("dictionaries not reloaded: {e}");
                Poll::Invalid
            }
        }
    }

    /// Polls every `every` and sends each successful reload to `tx`.
    pub async fn run(mut self, every: Duration, tx: Sender<Command>) {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Poll::Reload(config, dict) = self.poll()
                && tx.send(Command::Reload(config, dict)).await.is_err()
            {
                break;
            }
        }
    }
}

/// None for a file that is missing or unreadable.
fn mtimes(paths: &[PathBuf; 3]) -> [Option<SystemTime>; 3] {
    paths
        .each_ref()
        .map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    fn set(path: &Path, text: &str, secs: u64) {
        std::fs::write(path, text).unwrap();
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 + secs);
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }

    fn paths(dir: &Path) -> [PathBuf; 3] {
        [
            dir.join("config.toml"),
            dir.join("never.txt"),
            dir.join("always.txt"),
        ]
    }

    #[tokio::test(start_paused = true)]
    async fn detects_mtime_change() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        set(&p[0], "auto_correct = true\n", 0);
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        tokio::spawn(Watcher::new(p.clone()).run(Duration::from_secs(1), tx));
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert!(rx.try_recv().is_err(), "nothing changed yet");

        set(&p[0], "auto_correct = false\n", 1);
        tokio::time::sleep(Duration::from_secs(1)).await;
        match rx.try_recv() {
            Ok(Command::Reload(c, _)) => assert!(!c.auto_correct),
            _ => panic!("expected a reload after the config changed"),
        }

        set(&p[1], "ghbdtn\n", 2);
        tokio::time::sleep(Duration::from_secs(1)).await;
        match rx.try_recv() {
            Ok(Command::Reload(c, d)) => {
                assert!(!c.auto_correct);
                assert!(d.never.contains("ghbdtn"));
            }
            _ => panic!("expected a reload after the never list changed"),
        }

        set(&p[2], "rfr\n", 3);
        tokio::time::sleep(Duration::from_secs(1)).await;
        match rx.try_recv() {
            Ok(Command::Reload(_, d)) => assert!(d.always.contains("rfr")),
            _ => panic!("expected a reload after the always list changed"),
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(rx.try_recv().is_err(), "no change, no reload");
    }

    #[test]
    fn malformed_reload_keeps_previous() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        let mut w = Watcher::new(p.clone());
        let (mut reloads, mut warnings) = (0, 0);
        let mut count = |poll: Poll| match poll {
            Poll::Unchanged => {}
            Poll::Reload(..) => reloads += 1,
            Poll::Invalid => warnings += 1,
        };
        set(&p[0], "auto_correct = false\n", 1);
        count(w.poll());
        count(w.poll());
        set(&p[0], "auto_correct = [oops", 2);
        count(w.poll());
        count(w.poll());
        set(&p[0], "convert_last = \"F9\"\n", 3);
        count(w.poll());
        count(w.poll());
        assert_eq!((reloads, warnings), (2, 1));
    }

    #[test]
    fn bad_hotkey_is_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        let mut w = Watcher::new(p.clone());
        set(&p[0], "convert_last = \"Bogus\"\n", 1);
        assert!(matches!(w.poll(), Poll::Invalid));
    }

    #[test]
    fn existing_files_at_start_are_not_a_change() {
        let dir = tempfile::tempdir().unwrap();
        let p = paths(dir.path());
        set(&p[0], "auto_correct = false\n", 1);
        set(&p[1], "x\n", 1);
        let mut w = Watcher::new(p.clone());
        assert!(matches!(w.poll(), Poll::Unchanged));
        std::fs::remove_file(&p[1]).unwrap();
        assert!(matches!(w.poll(), Poll::Reload(..)), "deletion is a change");
    }
}
