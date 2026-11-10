//! Watches: standing expectations.
//!
//! A watch is a check the model would otherwise re-run by hand after every change. The harness runs
//! it instead, and speaks up only when the result *changes*: a regression is announced the moment it
//! is introduced, a fix is confirmed without being asked for, and "still passing" costs one line.

use crate::guard::{self, Expect};
use crate::locate;
use crate::state::Session;
use crate::text::{bound, squeeze, tail};
use crate::tools::jobs::kill_group;
use crate::tools::shell::{ExecEnd, StateKind, exec};
use crate::util::{fnv64, one_line};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WatchState {
    Unknown,
    Pass,
    Fail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WatchChange {
    /// First result for this watch.
    New,
    /// Pass became fail or the reverse.
    Flip,
    /// Still failing, but differently.
    Changed,
    Same,
}

#[derive(Clone, Debug)]
pub struct Watch {
    pub name: String,
    pub cmd: String,
    pub cwd: PathBuf,
    pub expect: Option<Expect>,
    pub state: WatchState,
    digest: u64,
    /// Workspace epoch this watch last ran against.
    pub epoch: u64,
    pub summary: String,
}

#[derive(Default)]
pub struct Watches {
    pub items: Vec<Watch>,
}

#[derive(Clone, Debug)]
pub struct WatchReport {
    pub name: String,
    pub cmd: String,
    pub state: WatchState,
    pub change: WatchChange,
    /// Text for the model. Empty for an unchanged pass, which is summarised with its peers.
    pub detail: String,
}

const WATCH_TIMEOUT: Duration = Duration::from_secs(300);

static VOLATILE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\d+(?:\.\d+)?\s*(?:ms|µs|us|ns|s|sec|secs|seconds|m|min|mins)\b|\b\d{1,2}:\d{2}(?::\d{2})?\b|0x[0-9a-fA-F]+|\d{4}-\d{2}-\d{2}[T ][\d:.]+Z?|/tmp/[\w./-]+").unwrap()
});

/// Digest of the output with timings, addresses and temp paths masked, so two runs that differ only
/// in how long they took count as the same result.
fn digest(text: &str) -> u64 {
    fnv64(VOLATILE.replace_all(text, "~").as_bytes())
}

impl Watches {
    pub fn set(&mut self, name: &str, cmd: &str, cwd: PathBuf, expect: Option<Expect>) -> usize {
        let fresh = Watch { name: name.to_string(), cmd: cmd.to_string(), cwd, expect, state: WatchState::Unknown, digest: 0, epoch: 0, summary: String::new() };
        match self.items.iter().position(|w| w.name == name) {
            Some(i) => {
                self.items[i] = fresh;
                i
            }
            None => {
                self.items.push(fresh);
                self.items.len() - 1
            }
        }
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|w| w.name != name);
        self.items.len() != before
    }

    pub fn all_pass(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|w| w.state == WatchState::Pass)
    }

    pub fn names(&self) -> String {
        self.items.iter().map(|w| w.name.as_str()).collect::<Vec<_>>().join(", ")
    }
}

/// Runs one watch and classifies the result against its previous one.
pub async fn run_one(sess: &mut Session, idx: usize, cancel: &CancellationToken) -> WatchReport {
    let w = sess.watches.items[idx].clone();
    sess.evidence.before_command(&sess.ws);
    let result = exec(sess, &w.cmd, &w.cwd, StateKind::ReadOnly, WATCH_TIMEOUT, cancel).await;
    sess.evidence.after_command(&sess.ws, &w.cmd);

    let (state, text, why, spill) = match result {
        Err(e) => (WatchState::Fail, String::new(), format!("could not start: {e}"), String::new()),
        Ok(ex) => match ex.end {
            ExecEnd::Exited(exit) => {
                let verdict = guard::check(w.expect.as_ref(), exit.code, &ex.text);
                let state = if verdict.held { WatchState::Pass } else { WatchState::Fail };
                (state, ex.text, verdict.violations.join("; "), ex.spill)
            }
            ExecEnd::TimedOut(running) => {
                kill_group(running.pid);
                (WatchState::Fail, ex.text, format!("did not finish within {}s", WATCH_TIMEOUT.as_secs()), ex.spill)
            }
            ExecEnd::Cancelled => (WatchState::Unknown, ex.text, "cancelled".to_string(), ex.spill),
        },
    };

    let lines = squeeze(&text);
    let new_digest = digest(&text);
    let change = match (w.state, state) {
        (WatchState::Unknown, _) => WatchChange::New,
        (a, b) if a != b => WatchChange::Flip,
        (WatchState::Fail, WatchState::Fail) if w.digest != new_digest => WatchChange::Changed,
        _ => WatchChange::Same,
    };
    let last = tail(&lines, 1).first().map(|l| one_line(l, 160)).unwrap_or_default();

    let excerpt = |sess: &mut Session| {
        let view = bound(&lines, sess.limits.fail_head / 2, sess.limits.fail_tail / 2, sess.limits.fail_chars / 2, Some(&spill));
        let mut s = view.text.trim_end().to_string();
        if sess.features.expand {
            let step = sess.step;
            let block = locate::expand(&text, &sess.ws, &mut sess.seen, step, sess.limits.expand_radius, sess.limits.expand_windows, sess.limits.expand_chars);
            if !block.is_empty() {
                s.push_str("\n--- where (current file content; anchors work in edit) ---\n");
                s.push_str(block.trim_end());
            }
        }
        s
    };

    let name = &w.name;
    let detail = match (state, change) {
        (WatchState::Pass, WatchChange::New) => format!("watch {name}: PASS · {last}"),
        (WatchState::Pass, WatchChange::Flip) => format!("watch {name}: FAIL → PASS · {last}"),
        (WatchState::Pass, _) => String::new(),
        (WatchState::Fail, WatchChange::New) => format!("watch {name}: FAIL ({why})\n{}", excerpt(sess)),
        (WatchState::Fail, WatchChange::Flip) => format!("watch {name}: PASS → FAIL ({why}) — the change you just made broke it\n{}", excerpt(sess)),
        (WatchState::Fail, WatchChange::Changed) => format!("watch {name}: still FAIL ({why}), but the output changed\n{}", excerpt(sess)),
        (WatchState::Fail, WatchChange::Same) => format!("watch {name}: still FAIL, same output as before"),
        (WatchState::Unknown, _) => format!("watch {name}: not run ({why})"),
    };

    let item = &mut sess.watches.items[idx];
    item.state = state;
    item.digest = new_digest;
    item.summary = last;
    WatchReport { name: w.name, cmd: w.cmd, state, change, detail }
}

/// Re-runs every watch that has not seen the current state of the workspace. Returns the reports and
/// the text to append to the turn's last observation (empty when there is nothing to say).
pub async fn run_stale(sess: &mut Session, cancel: &CancellationToken) -> (Vec<WatchReport>, String) {
    let stale: Vec<usize> = (0..sess.watches.items.len()).filter(|i| sess.watches.items[*i].epoch != sess.evidence.epoch || sess.watches.items[*i].state == WatchState::Unknown).collect();
    if stale.is_empty() {
        return (Vec::new(), String::new());
    }
    let mut reports = Vec::new();
    for idx in stale {
        if cancel.is_cancelled() {
            break;
        }
        reports.push(run_one(sess, idx, cancel).await);
    }
    // The batch is atomic: watches that write caches must not make each other look stale forever.
    let epoch = sess.evidence.epoch;
    for w in &mut sess.watches.items {
        w.epoch = epoch;
    }
    if sess.watches.all_pass() {
        let names = sess.watches.names();
        sess.evidence.passed(format!("watch {names}"));
    }
    let mut text: Vec<String> = reports.iter().filter(|r| !r.detail.is_empty()).map(|r| r.detail.clone()).collect();
    let quiet: Vec<&str> = reports.iter().filter(|r| r.detail.is_empty()).map(|r| r.name.as_str()).collect();
    if !quiet.is_empty() {
        text.push(format!("watches still passing: {}", quiet.join(", ")));
    }
    (reports, text.join("\n"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::state::{Features, test_session};

    fn add(sess: &mut Session, name: &str, cmd: &str) -> usize {
        let cwd = sess.ws.cwd.clone();
        sess.watches.set(name, cmd, cwd, None)
    }

    #[tokio::test]
    async fn reports_only_changes_and_announces_regressions() {
        let mut s = test_session("watch", Features::presage());
        let cancel = CancellationToken::new();
        std::fs::write(s.ws.root.join("ok"), "").unwrap();
        add(&mut s, "tests", "test -f ok && echo '3 passed in 0.12s' || { echo 'FAILED: ok is missing'; exit 1; }");

        let (reports, text) = run_stale(&mut s, &cancel).await;
        assert_eq!(reports[0].change, WatchChange::New);
        assert_eq!(text, "watch tests: PASS · 3 passed in 0.12s");
        assert!(s.evidence.fresh());

        // Nothing changed: nothing is re-run, nothing is said.
        let (reports, text) = run_stale(&mut s, &cancel).await;
        assert!(reports.is_empty() && text.is_empty());

        // A change that keeps it green costs one short line.
        s.evidence.changed("edit");
        let (_, text) = run_stale(&mut s, &cancel).await;
        assert_eq!(text, "watches still passing: tests");
        assert!(s.evidence.fresh(), "a passing watch run is evidence for the new state");

        // A change that breaks it is called out as a regression, with the output.
        std::fs::remove_file(s.ws.root.join("ok")).unwrap();
        s.evidence.changed("edit");
        let (reports, text) = run_stale(&mut s, &cancel).await;
        assert_eq!((reports[0].state, reports[0].change), (WatchState::Fail, WatchChange::Flip));
        assert!(text.starts_with("watch tests: PASS → FAIL (expected exit 0, got exit 1) — the change you just made broke it\n"), "{text}");
        assert!(text.contains("FAILED: ok is missing"));
        assert!(!s.evidence.fresh());

        // Still broken in the same way: say so briefly, do not repeat the output.
        s.evidence.changed("edit");
        let (_, text) = run_stale(&mut s, &cancel).await;
        assert_eq!(text, "watch tests: still FAIL, same output as before");

        // Fixed.
        std::fs::write(s.ws.root.join("ok"), "").unwrap();
        s.evidence.changed("edit");
        let (_, text) = run_stale(&mut s, &cancel).await;
        assert_eq!(text, "watch tests: FAIL → PASS · 3 passed in 0.12s");
    }

    #[tokio::test]
    async fn timing_noise_is_not_a_change_but_a_different_failure_is() {
        let mut s = test_session("watch-digest", Features::presage());
        let cancel = CancellationToken::new();
        std::fs::write(s.ws.root.join("which"), "test_a").unwrap();
        add(&mut s, "t", "echo \"FAILED $(cat which) in 0.$(date +%N | cut -c1-3)s\"; exit 1");
        run_stale(&mut s, &cancel).await;
        s.evidence.changed("edit");
        let (r, _) = run_stale(&mut s, &cancel).await;
        assert_eq!(r[0].change, WatchChange::Same, "only the duration differed");

        std::fs::write(s.ws.root.join("which"), "test_b").unwrap();
        s.evidence.changed("edit");
        let (r, text) = run_stale(&mut s, &cancel).await;
        assert_eq!(r[0].change, WatchChange::Changed);
        assert!(text.contains("still FAIL") && text.contains("test_b"));
    }

    #[tokio::test]
    async fn watches_do_not_move_the_session_and_can_be_replaced_or_removed() {
        let mut s = test_session("watch-cwd", Features::presage());
        let cancel = CancellationToken::new();
        add(&mut s, "w", "cd / && true");
        run_stale(&mut s, &cancel).await;
        assert_eq!(s.ws.cwd, s.ws.root);

        add(&mut s, "w", "false");
        assert_eq!(s.watches.items.len(), 1);
        assert_eq!(s.watches.items[0].state, WatchState::Unknown, "a redefined watch starts over");
        assert!(s.watches.remove("w"));
        assert!(!s.watches.all_pass(), "no watches is not the same as all watches passing");
    }
}
