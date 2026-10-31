//! What the daemon reports about itself: whether it corrects, and if not, why.

use crate::config::Config;
use crate::corrector::{Activity, layout_status};
use crate::desktop::Desktop;
use crate::i18n::{Lang, problem_text};
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// Why the daemon is not correcting. The UI text is `i18n::problem_text`;
/// over D-Bus the stable [`Problem::code`] is sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The GNOME Shell extension does not answer.
    Extension,
    /// The input sources are not exactly one `us` and one `ru`.
    Layouts,
    /// No pointer device is read.
    Pointer,
}

impl Problem {
    #[allow(dead_code)] // read by the tray client (Task 3)
    pub const ALL: &'static [Problem] = &[Problem::Extension, Problem::Layouts, Problem::Pointer];

    /// Stable wire code, sent by the D-Bus `Status()` method.
    pub fn code(self) -> &'static str {
        match self {
            Problem::Extension => "extension",
            Problem::Layouts => "layouts",
            Problem::Pointer => "pointer",
        }
    }

    #[allow(dead_code)] // read by the tray client (Task 3)
    /// Inverse of [`Problem::code`]; "" and unknown codes give None.
    pub fn from_code(code: &str) -> Option<Problem> {
        Problem::ALL.iter().copied().find(|p| p.code() == code)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub auto_correct: bool,
    /// Why the daemon is not correcting, or None when nothing is wrong.
    pub problem: Option<Problem>,
}

/// Pure: the first reason that applies, in the order the daemon checks them.
pub fn compute(config: &Config, desktop_layouts: Result<Vec<String>>, pointer: bool) -> Status {
    let problem = match desktop_layouts {
        Err(_) => Some(Problem::Extension),
        Ok(l) if layout_status(&l).is_err() => Some(Problem::Layouts),
        Ok(_) if !pointer => Some(Problem::Pointer),
        Ok(_) => None,
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
                match s.problem {
                    Some(p) => log::info!("status: {}", problem_text(Lang::En, p)),
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

        let p = |l, ptr| compute(&c, l, ptr).problem;
        assert_eq!(
            p(Err(anyhow::anyhow!("timed out")), true),
            Some(Problem::Extension)
        );
        assert_eq!(
            p(Err(anyhow::anyhow!("timed out")), false),
            Some(Problem::Extension)
        );
        assert_eq!(
            p(layouts(&["us", "de", "ru"]), true),
            Some(Problem::Layouts)
        );
        assert_eq!(
            p(layouts(&["us", "de", "ru"]), false),
            Some(Problem::Layouts)
        );
        assert_eq!(p(layouts(&["us", "ru"]), false), Some(Problem::Pointer));
        assert_eq!(p(layouts(&["us", "ru"]), true), None);
    }

    #[test]
    fn problem_codes_round_trip() {
        let codes: Vec<_> = Problem::ALL.iter().map(|p| p.code()).collect();
        assert_eq!(codes, ["extension", "layouts", "pointer"]);
        for &p in Problem::ALL {
            assert_eq!(Problem::from_code(p.code()), Some(p));
        }
        assert_eq!(Problem::from_code(""), None);
        assert_eq!(Problem::from_code("bogus"), None);
    }
}
