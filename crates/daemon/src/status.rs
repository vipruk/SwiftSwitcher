//! What the daemon reports about itself: whether it corrects, and if not, why.

use crate::config::Config;
use crate::corrector::{Activity, layout_status};
use crate::desktop::Desktop;
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub auto_correct: bool,
    /// Why the daemon is not correcting, or None when nothing is wrong.
    pub problem: Option<String>,
}

/// Pure: the first reason that applies, in the order the daemon checks them.
pub fn compute(config: &Config, desktop_layouts: Result<Vec<String>>, pointer: bool) -> Status {
    let problem = match desktop_layouts {
        Err(_) => Some("the GNOME Shell extension is not reachable".to_string()),
        Ok(l) => match layout_status(&l) {
            Err(why) => Some(why),
            Ok(()) if !pointer => {
                Some("no pointer device is readable, so automatic correction is off".to_string())
            }
            Ok(()) => None,
        },
    };
    Status {
        auto_correct: config.auto_correct,
        problem,
    }
}

const REFRESH: Duration = Duration::from_secs(3);

/// Keeps `tx` current: every 3 s, and at once whenever `config` changes.
pub async fn refresh(
    desktop: impl Desktop,
    activity: Arc<Activity>,
    mut config: watch::Receiver<Config>,
    tx: watch::Sender<Status>,
) {
    let mut tick = tokio::time::interval(REFRESH);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tick.tick().await; // the first tick is immediate
    loop {
        let layouts = desktop.layouts().await;
        let s = compute(
            &config.borrow_and_update(),
            layouts,
            activity.pointer_present(),
        );
        tx.send_if_modified(|cur| {
            let changed = *cur != s;
            if changed {
                match &s.problem {
                    Some(p) => log::info!("status: {p}"),
                    None => log::info!("status: ok"),
                }
                *cur = s;
            }
            changed
        });
        tokio::select! {
            _ = tick.tick() => {}
            r = config.changed() => if r.is_err() { break },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layouts(v: &[&str]) -> Result<Vec<String>> {
        Ok(v.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn compute_reasons() {
        let c = Config::default();
        let ok = compute(&c, layouts(&["us", "ru"]), true);
        assert_eq!(
            ok,
            Status {
                auto_correct: true,
                problem: None
            }
        );

        let off = Config {
            auto_correct: false,
            ..Config::default()
        };
        assert!(!compute(&off, layouts(&["us", "ru"]), true).auto_correct);

        let p = compute(&c, Err(anyhow::anyhow!("timed out")), true)
            .problem
            .unwrap();
        assert!(p.contains("extension"), "{p}");

        let p = compute(&c, layouts(&["us", "de", "ru"]), true)
            .problem
            .unwrap();
        assert!(p.contains("\"us\" and \"ru\""), "{p}");

        let p = compute(&c, layouts(&["us", "ru"]), false).problem.unwrap();
        assert!(p.contains("pointer"), "{p}");
    }
}
