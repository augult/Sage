//! Watches: standing expectations.
//!
//! A watch is a check the model would otherwise re-run by hand after every change. The harness runs
//! it instead, and speaks up only when the result *changes*: a regression is announced the moment it
//! is introduced, a fix is confirmed without being asked for, and "still passing" costs one line.

use crate::guard::{self, Expect};
use crate::locate;
use crate::state::Session;
use crate::text::{bound, squeeze, tail};
use crate::tools::Completed;
use crate::tools::jobs::kill_group;
use crate::tools::shell::{ExecEnd, StateKind, exec};
use crate::util::{fnv64, one_line};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
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
    /// The epoch immediately after this command was last attempted. A command that mutates the
    /// workspace remains stale, but repeating it on that same state cannot certify its old result.
    attempted_at: Option<u64>,
    pub summary: String,
    /// Set up by the harness from the model's own test command, not by the `watch` tool.
    pub auto: bool,
    /// The command the task itself names: no other test run takes its place.
    pub named: bool,
}

#[derive(Default)]
pub struct Watches {
    pub items: Vec<Watch>,
}

/// What of a watch is kept with a session that is stored: the check itself, not its results.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WatchSpec {
    pub name: String,
    pub cmd: String,
    pub cwd: PathBuf,
    pub expect: Option<Expect>,
    pub auto: bool,
    pub named: bool,
}

impl Watches {
    pub fn specs(&self) -> Vec<WatchSpec> {
        self.items
            .iter()
            .map(|w| WatchSpec {
                name: w.name.clone(),
                cmd: w.cmd.clone(),
                cwd: w.cwd.clone(),
                expect: w.expect.clone(),
                auto: w.auto,
                named: w.named,
            })
            .collect()
    }

    /// Puts back watches as `specs` gave them. Each runs again, with the first result as new, once
    /// the workspace changes.
    pub fn restore(&mut self, specs: Vec<WatchSpec>) {
        self.items = specs
            .into_iter()
            .map(|s| Watch {
                name: s.name,
                cmd: s.cmd,
                cwd: s.cwd,
                expect: s.expect,
                state: WatchState::Unknown,
                digest: 0,
                epoch: 0,
                attempted_at: None,
                summary: String::new(),
                auto: s.auto,
                named: s.named,
            })
            .collect();
    }
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

/// Name of the watch the harness keeps on the model's test command.
pub const AUTO_NAME: &str = "tests";
/// A test run slower than this is not repeated after every change.
const AUTO_MAX_MS: u64 = 60_000;

static VOLATILE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\d+(?:\.\d+)?\s*(?:ms|µs|us|ns|s|sec|secs|seconds|m|min|mins)\b|\b\d{1,2}:\d{2}(?::\d{2})?\b|0x[0-9a-fA-F]+|\d{4}-\d{2}-\d{2}[T ][\d:.]+Z?|/tmp/[\w./-]+").unwrap()
});

/// Digest of the output with timings, addresses and temp paths masked, so two runs that differ only
/// in how long they took count as the same result.
pub(crate) fn digest(text: &str) -> u64 {
    fnv64(VOLATILE.replace_all(text, "~").as_bytes())
}

impl Watches {
    pub fn set(&mut self, name: &str, cmd: &str, cwd: PathBuf, expect: Option<Expect>) -> usize {
        let fresh = Watch {
            name: name.to_string(),
            cmd: cmd.to_string(),
            cwd,
            expect,
            state: WatchState::Unknown,
            digest: 0,
            epoch: 0,
            attempted_at: None,
            summary: String::new(),
            auto: false,
            named: false,
        };
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

    /// Every watch passes and has seen the workspace as it is at `epoch`.
    pub fn all_pass_at(&self, epoch: u64) -> bool {
        self.all_pass() && self.items.iter().all(|w| w.epoch == epoch)
    }

    pub fn names(&self) -> String {
        self.items
            .iter()
            .map(|w| w.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Runs one watch and classifies the result against its previous one.
pub async fn run_one(sess: &mut Session, idx: usize, cancel: &CancellationToken) -> WatchReport {
    run_within(sess, idx, WATCH_TIMEOUT, cancel).await.0
}

/// As `run_one`, with a time limit. Also returns the exit code if the command ran to its end.
async fn run_within(
    sess: &mut Session,
    idx: usize,
    limit: Duration,
    cancel: &CancellationToken,
) -> (WatchReport, Option<i32>) {
    let w = sess.watches.items[idx].clone();
    // A run cut short by the time budget says nothing about the code: its result stays unknown.
    let allowed = sess.within_budget(limit);
    let budget_cut = allowed < limit;
    let limit = allowed;
    sess.evidence.before_command(&sess.ws, sess.deadline).await;
    let result = exec(sess, &w.cmd, &w.cwd, StateKind::ReadOnly, limit, cancel).await;
    if matches!(&result, Ok(ex) if matches!(ex.end, ExecEnd::Budget)) {
        sess.evidence.changed("a watch stopped at the time budget");
    } else {
        sess.evidence
            .after_command(&sess.ws, &w.cmd, sess.deadline)
            .await;
    }

    let predicate_text = if let Ok(ex) = &result {
        ex.predicate_text(
            sess,
            w.expect.as_ref().is_some_and(Expect::has_output_predicates),
        )
        .await
    } else {
        None
    };
    let (state, text, why, spill, code) = match result {
        Err(e) => (
            WatchState::Fail,
            String::new(),
            format!("could not start: {e}"),
            String::new(),
            None,
        ),
        Ok(ex) => match ex.end {
            ExecEnd::Exited(exit) => {
                let verdict = guard::check_capture(
                    w.expect.as_ref(),
                    exit.code,
                    predicate_text.as_deref().unwrap_or(&ex.text),
                    ex.output_complete || predicate_text.is_some(),
                );
                let state = if verdict.held {
                    WatchState::Pass
                } else {
                    WatchState::Fail
                };
                (
                    state,
                    ex.text,
                    verdict.violations.join("; "),
                    ex.spill,
                    exit.code,
                )
            }
            ExecEnd::TimedOut(running) => {
                kill_group(running.pid);
                let (state, why) = if budget_cut {
                    (WatchState::Unknown, "the time budget ran out".to_string())
                } else {
                    (
                        WatchState::Fail,
                        format!("did not finish within {}s", limit.as_secs()),
                    )
                };
                (state, ex.text, why, ex.spill, None)
            }
            ExecEnd::Budget => (
                WatchState::Unknown,
                ex.text,
                "the run's time budget expired".to_string(),
                ex.spill,
                None,
            ),
            ExecEnd::Cancelled => (
                WatchState::Unknown,
                ex.text,
                "cancelled".to_string(),
                ex.spill,
                None,
            ),
        },
    };

    // What a check writes while it runs (a log, a coverage file, build output that is not ignored)
    // is its own output, so its result stands for the workspace as it left it. Had it stood only
    // for the state before, a check that writes a file on every run could never certify anything.
    // A watch that ran before it in the same round is stale now, and `run_stale` runs it again.
    sess.watches.items[idx].epoch = sess.evidence.epoch;
    sess.watches.items[idx].attempted_at =
        (state != WatchState::Unknown).then_some(sess.evidence.epoch);
    (settle(sess, idx, state, &text, &why, Some(&spill)), code)
}

/// Records a result for watch `idx` and words what changed. Without a `spill` handle the output is
/// one the model has just been shown (it ran the command itself), so none of it is repeated.
fn settle(
    sess: &mut Session,
    idx: usize,
    state: WatchState,
    text: &str,
    why: &str,
    spill: Option<&str>,
) -> WatchReport {
    let w = sess.watches.items[idx].clone();
    let lines = squeeze(text);
    let new_digest = digest(text);
    let change = match (w.state, state) {
        (WatchState::Unknown, _) => WatchChange::New,
        (a, b) if a != b => WatchChange::Flip,
        (WatchState::Fail, WatchState::Fail) if w.digest != new_digest => WatchChange::Changed,
        _ => WatchChange::Same,
    };
    let last = tail(&lines, 1)
        .first()
        .map(|l| one_line(l, 160))
        .unwrap_or_default();

    let excerpt = |sess: &mut Session| {
        let Some(spill) = spill else {
            return String::new();
        };
        let view = bound(
            &lines,
            sess.limits.fail_head / 2,
            sess.limits.fail_tail / 2,
            sess.limits.fail_chars / 2,
            Some(spill),
        );
        let mut s = format!("\n{}", view.text.trim_end());
        if sess.features.expand {
            let step = sess.step;
            let block = locate::expand(
                text,
                &sess.ws,
                &mut sess.seen,
                step,
                sess.limits.expand_radius,
                sess.limits.expand_windows,
                sess.limits.expand_chars,
            );
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
        (WatchState::Fail, WatchChange::New) => {
            format!("watch {name}: FAIL ({why}){}", excerpt(sess))
        }
        (WatchState::Fail, WatchChange::Flip) => format!(
            "watch {name}: PASS → FAIL ({why}) — the change you just made broke it{}",
            excerpt(sess)
        ),
        (WatchState::Fail, WatchChange::Changed) => format!(
            "watch {name}: still FAIL ({why}), but the output changed{}",
            excerpt(sess)
        ),
        (WatchState::Fail, WatchChange::Same) => {
            format!("watch {name}: still FAIL, same output as before")
        }
        (WatchState::Unknown, _) => format!("watch {name}: not run ({why})"),
    };

    let item = &mut sess.watches.items[idx];
    item.state = state;
    item.digest = new_digest;
    item.summary = last;
    WatchReport {
        name: w.name,
        cmd: w.cmd,
        state,
        change,
        detail,
    }
}

/// Re-runs every watch that has not seen the current state of the workspace. Returns the reports and
/// the text to append to the turn's last observation (empty when there is nothing to say).
pub async fn run_stale(
    sess: &mut Session,
    cancel: &CancellationToken,
) -> (Vec<WatchReport>, String) {
    let mut reports = Vec::new();
    // A watch that changes files while it runs leaves the ones that ran before it stale: they run
    // once more, against the files as it left them. Two watches that both write on every run would
    // leave each other stale for ever, so there is one such round and no more.
    for _ in 0..2 {
        let stale: Vec<usize> = (0..sess.watches.items.len())
            .filter(|i| sess.watches.items[*i].attempted_at != Some(sess.evidence.epoch))
            .collect();
        if stale.is_empty() {
            break;
        }
        for idx in stale {
            if cancel.is_cancelled() {
                break;
            }
            reports.push(run_one(sess, idx, cancel).await);
        }
    }
    if reports.is_empty() {
        return (Vec::new(), String::new());
    }
    if !cancel.is_cancelled() && sess.watches.all_pass_at(sess.evidence.epoch) {
        let names = sess.watches.names();
        sess.evidence.passed(format!("watch {names}"));
    }
    let mut text: Vec<String> = reports
        .iter()
        .filter(|r| !r.detail.is_empty())
        .map(|r| r.detail.clone())
        .collect();
    let mut quiet: Vec<&str> = reports
        .iter()
        .filter(|r| {
            r.detail.is_empty()
                && sess
                    .watches
                    .items
                    .iter()
                    .any(|w| w.name == r.name && w.epoch == sess.evidence.epoch)
        })
        .map(|r| r.name.as_str())
        .collect();
    // A watch that ran twice in the round is named once.
    let mut named = std::collections::HashSet::new();
    quiet.retain(|name| named.insert(*name));
    if !quiet.is_empty() {
        text.push(format!("watches still passing: {}", quiet.join(", ")));
    }
    (reports, text.join("\n"))
}

/// What `observe` made of a finished run.
#[derive(Default)]
pub struct Observed {
    /// Changes to show in the UI. Not for the model: it has the step's own output.
    pub reports: Vec<WatchReport>,
    /// One line for the model when its test command has just become a watch.
    pub notice: Option<String>,
}

/// Folds a foreground run that left the workspace untouched into the watches. A watch on the same
/// command takes the result as its own instead of running a second time. With `auto_watch`, the
/// model's test command becomes a watch by itself, which is what lets a reply that only edits come
/// back with the test result attached.
pub fn observe(sess: &mut Session, cmd: &str, run: &Completed, observed_epoch: u64) -> Observed {
    let mut out = Observed::default();
    let cmd = without_cd(cmd, &run.cwd);
    let epoch = sess.evidence.epoch;
    let same: Vec<usize> = (0..sess.watches.items.len())
        .filter(|i| sess.watches.items[*i].cmd == cmd && sess.watches.items[*i].cwd == run.cwd)
        .collect();
    for idx in &same {
        let verdict = guard::check_capture(
            sess.watches.items[*idx].expect.as_ref(),
            run.code,
            run.predicate_text.as_deref().unwrap_or(&run.text),
            run.output_complete || run.predicate_text.is_some(),
        );
        let state = if verdict.held {
            WatchState::Pass
        } else {
            WatchState::Fail
        };
        let report = settle(
            sess,
            *idx,
            state,
            &run.text,
            &verdict.violations.join("; "),
            None,
        );
        // As for a run of the watch itself (`run_within`): what the command wrote is its output.
        sess.watches.items[*idx].epoch = epoch;
        sess.watches.items[*idx].attempted_at = Some(epoch);
        if report.change != WatchChange::Same {
            out.reports.push(report);
        }
    }

    // A runner that could not start (126, 127) says nothing about the code. Nor does a throwaway
    // script: one that lives outside the workspace is gone, or failing on purpose, soon after.
    let promote = epoch == observed_epoch
        && same.is_empty()
        && sess.features.auto_watch
        && run.ms <= AUTO_MAX_MS
        && !matches!(run.code, None | Some(126 | 127))
        && test_like(cmd)
        && !throwaway(cmd, &sess.ws.root);
    let previous = sess
        .watches
        .items
        .iter()
        .find(|w| w.auto)
        .map(|w| w.cmd.clone());
    // A watch the model itself named `tests` is the model's, the command the task names stays the
    // one that is watched, and of two runs the broader is kept.
    let owned = sess
        .watches
        .items
        .iter()
        .any(|w| w.name == AUTO_NAME && (!w.auto || w.named));
    if !promote || owned || previous.as_deref().is_some_and(|old| narrows(old, cmd)) {
        return out;
    }
    let idx = sess.watches.set(AUTO_NAME, cmd, run.cwd.clone(), None);
    sess.watches.items[idx].auto = true;
    let state = if run.code == Some(0) {
        WatchState::Pass
    } else {
        WatchState::Fail
    };
    let mut report = settle(sess, idx, state, &run.text, "", None);
    sess.watches.items[idx].epoch = epoch;
    sess.watches.items[idx].attempted_at = Some(epoch);

    let shown = one_line(cmd, 80);
    let notice = match (previous, state) {
        (Some(_), WatchState::Pass) => {
            format!("watch {AUTO_NAME}: PASS · it now follows `{shown}`.")
        }
        (Some(_), _) => format!("watch {AUTO_NAME}: FAIL · it now follows `{shown}`."),
        (None, WatchState::Pass) => format!(
            "watch {AUTO_NAME}: PASS · `{shown}` is now watched: it re-runs by itself after every change and you hear about it when the result changes, so do not run it again."
        ),
        (None, _) => format!(
            "watch {AUTO_NAME}: FAIL · `{shown}` is now watched: it re-runs by itself after every change and you hear about it when the result changes, so do not run it again. Send `finish` in the same reply as your fix: the tests are re-run first, and if they still fail `finish` is refused and you get the new failure instead."
        ),
    };
    report.detail = notice.clone();
    out.reports.push(report);
    out.notice = Some(notice);
    out
}

/// True when `new` is `old` with more arguments: the same run restricted to a part.
fn narrows(old: &str, new: &str) -> bool {
    new.strip_prefix(old)
        .is_some_and(|rest| rest.starts_with(char::is_whitespace))
}

/// A command that writes its own script first (a here-document), or that names a place in the
/// system's temporary directory outside the workspace: scratch work, not the project's tests.
fn throwaway(cmd: &str, root: &Path) -> bool {
    if cmd.contains("<<") {
        return true;
    }
    let tmp = std::env::temp_dir();
    cmd.split(|c: char| c.is_whitespace() || "\"'=;&|()<>".contains(c))
        .map(Path::new)
        .any(|word| {
            word.is_absolute()
                && (word.starts_with(&tmp) || word.starts_with("/tmp"))
                && !word.starts_with(root)
        })
}

/// `cd <the directory it already runs in> && rest` is just `rest`.
fn without_cd<'a>(cmd: &'a str, cwd: &Path) -> &'a str {
    let stripped = cmd
        .trim_start()
        .strip_prefix("cd ")
        .and_then(|rest| rest.split_once("&&"));
    match stripped {
        Some((dir, rest)) if Path::new(dir.trim().trim_matches(['"', '\''])) == cwd => {
            rest.trim_start()
        }
        _ => cmd,
    }
}

/// The test command a task names: the first `backticked` span that is a plain test run. Nothing
/// chained, piped, quoted or redirected qualifies, because it is run before the model has said a word.
pub fn named_in(task: &str) -> Option<&str> {
    task.split('`')
        .skip(1)
        .step_by(2)
        .map(str::trim)
        .find(|span| test_like(span) && !span.contains(|c: char| ";&|<>$(){}\\\"'\n".contains(c)))
}

/// What the model is told after a preflight, below the result.
pub const PREFLIGHT_NOTE: &str = "The harness ran the test command the task names and watches it from now on: it re-runs by itself after every change and you hear about it when the result changes, so do not run it yourself. Send `finish` in the same reply as your fix: the tests are re-run first, and if they still fail `finish` is refused and you get the new failure instead.";

const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(60);

/// Runs the test command the task names before the first model turn and keeps it as the watch. The
/// model then starts from the failing output instead of spending a turn to get it, and its first
/// edit already comes back checked. `None` when the task names no such command or it cannot be run.
pub async fn preflight(
    sess: &mut Session,
    task: &str,
    cancel: &CancellationToken,
) -> Option<WatchReport> {
    if !(sess.features.watches && sess.features.auto_watch && sess.features.preflight) {
        return None;
    }
    let cmd = named_in(task)?;
    let cwd = sess.ws.cwd.clone();
    let idx = sess.watches.set(AUTO_NAME, cmd, cwd, None);
    sess.watches.items[idx].auto = true;
    sess.watches.items[idx].named = true;
    let (report, code) = run_within(sess, idx, PREFLIGHT_TIMEOUT, cancel).await;
    // Too slow to repeat after every change, or a runner that could not start: not a usable watch.
    if matches!(code, None | Some(126 | 127)) {
        sess.watches.remove(AUTO_NAME);
        return None;
    }
    if sess.watches.all_pass_at(sess.evidence.epoch) {
        sess.evidence.passed(format!("watch {AUTO_NAME}"));
    }
    Some(report)
}

const RUNNERS: &[&str] = &[
    "pytest",
    "py.test",
    "tox",
    "nox",
    "nose2",
    "nosetests",
    "jest",
    "vitest",
    "mocha",
    "ava",
    "phpunit",
    "rspec",
    "ctest",
    "bats",
    "behave",
];
/// Tools whose test run is spelled `<tool> test`.
const TEST_SUBCOMMAND: &[&str] = &[
    "go", "deno", "dotnet", "mix", "swift", "flutter", "dart", "stack", "cabal", "lein", "sbt",
    "bazel", "bun", "composer", "just", "task", "rake",
];

fn base(word: &str) -> &str {
    word.rsplit(['/', std::path::MAIN_SEPARATOR])
        .next()
        .unwrap_or(word)
}

/// `run_tests.sh`, `test_calc.py`, `check`: a file whose name says it tests something.
fn test_name(path: &str) -> bool {
    base(path)
        .to_ascii_lowercase()
        .split(['.', '_', '-'])
        .any(|t| {
            matches!(
                t,
                "test"
                    | "tests"
                    | "spec"
                    | "specs"
                    | "check"
                    | "checks"
                    | "verify"
                    | "runtests"
                    | "unittest"
                    | "unittests"
            )
        })
}

/// True when `cmd` is a test run whose exit status is its verdict: the last command on the line is a
/// known test runner or a script named like one. Deliberately narrow, because the result is accepted
/// as verification: anything that could exit 0 without the tests passing is left out.
pub fn test_like(cmd: &str) -> bool {
    if cmd.contains("||") {
        return false;
    }
    let Some(last) = cmd
        .split(['\n', ';'])
        .flat_map(|part| part.split("&&"))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .last()
    else {
        return false;
    };
    if last.ends_with('&') {
        return false;
    }
    // In `tests | tail` the first stage runs the tests, and `exec` reports its failure.
    let stage = last
        .split('|')
        .next()
        .unwrap_or("")
        .trim_matches(['(', ')', '{', '}', ' ']);
    let mut w: Vec<&str> = stage.split_whitespace().collect();

    // Leading `VAR=value` settings and wrappers that only set the stage.
    loop {
        match w.first().copied() {
            Some(x)
                if x.split_once('=').is_some_and(|(k, _)| {
                    !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                }) =>
            {
                w.remove(0);
            }
            Some(
                "env" | "time" | "nice" | "sudo" | "command" | "exec" | "npx" | "bunx" | "xvfb-run"
                | "timeout",
            ) => {
                w.remove(0);
                while w
                    .first()
                    .is_some_and(|x| x.starts_with(|c: char| c == '-' || c.is_ascii_digit()))
                {
                    w.remove(0);
                }
            }
            Some("uv" | "poetry" | "pipenv" | "hatch" | "pdm" | "rye" | "bundle")
                if matches!(w.get(1).copied(), Some("run" | "exec")) =>
            {
                w.drain(..2);
            }
            Some("pnpm" | "yarn") if matches!(w.get(1).copied(), Some("exec" | "dlx")) => {
                w.drain(..2);
            }
            _ => break,
        }
    }
    let Some(first) = w.first().copied() else {
        return false;
    };
    let args: Vec<&str> = w[1..]
        .iter()
        .copied()
        .filter(|a| !a.starts_with('-'))
        .collect();
    let arg = |i: usize| args.get(i).copied().unwrap_or("");
    match base(first) {
        h if RUNNERS.contains(&h) => true,
        h if h.starts_with("python") || h.starts_with("pypy") => {
            match w.iter().position(|a| *a == "-m") {
                Some(i) => matches!(
                    w.get(i + 1).copied(),
                    Some("pytest" | "unittest" | "nose2" | "tox" | "nox" | "doctest")
                ),
                None => {
                    !w.contains(&"-c")
                        && (test_name(arg(0)) || (base(arg(0)) == "manage.py" && arg(1) == "test"))
                }
            }
        }
        "node" => w.contains(&"--test") || test_name(arg(0)),
        "ruby" | "php" | "bash" | "sh" | "zsh" | "dash" | "ksh" => {
            !w.contains(&"-c")
                && (test_name(arg(0)) || (base(arg(0)) == "artisan" && arg(1) == "test"))
        }
        "cargo" => matches!(
            if arg(0).starts_with('+') {
                arg(1)
            } else {
                arg(0)
            },
            "test" | "nextest" | "t"
        ),
        "npm" | "pnpm" | "yarn" => {
            let script = if matches!(arg(0), "run" | "run-script") {
                arg(1)
            } else {
                arg(0)
            };
            script == "t" || script == "test" || script.starts_with("test:")
        }
        "zig" => arg(0) == "test" || (arg(0) == "build" && arg(1) == "test"),
        h if TEST_SUBCOMMAND.contains(&h) => matches!(arg(0), "test" | "spec"),
        "make" | "gmake" | "mvn" | "mvnw" | "gradle" | "gradlew" | "ant" => args
            .iter()
            .any(|a| matches!(*a, "test" | "tests" | "check" | "verify")),
        // A script run directly: `./run_tests.sh`, `bin/test`.
        _ => first.contains(['/', std::path::MAIN_SEPARATOR]) && test_name(first),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::state::{Features, test_session};

    fn observe_unchanged(sess: &mut Session, cmd: &str, run: &Completed) -> Observed {
        observe(sess, cmd, run, sess.evidence.epoch)
    }

    fn add(sess: &mut Session, name: &str, cmd: &str) -> usize {
        let cwd = sess.ws.cwd.clone();
        sess.watches.set(name, cmd, cwd, None)
    }

    #[tokio::test]
    async fn reports_only_changes_and_announces_regressions() {
        let mut s = test_session("watch", Features::sage());
        let cancel = CancellationToken::new();
        std::fs::write(s.ws.root.join("ok"), "").unwrap();
        add(
            &mut s,
            "tests",
            "test -f ok && echo '3 passed in 0.12s' || { echo 'FAILED: ok is missing'; exit 1; }",
        );

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
        assert!(
            s.evidence.fresh(),
            "a passing watch run is evidence for the new state"
        );

        // A change that breaks it is called out as a regression, with the output.
        std::fs::remove_file(s.ws.root.join("ok")).unwrap();
        s.evidence.changed("edit");
        let (reports, text) = run_stale(&mut s, &cancel).await;
        assert_eq!(
            (reports[0].state, reports[0].change),
            (WatchState::Fail, WatchChange::Flip)
        );
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
    async fn watches_a_stopped_run_did_not_get_to_are_not_taken_as_run() {
        let mut s = test_session("watch-stopped", Features::sage());
        std::fs::write(s.ws.root.join("ok"), "").unwrap();
        add(&mut s, "a", "true");
        add(&mut s, "b", "test -f ok");
        run_stale(&mut s, &CancellationToken::new()).await;
        assert!(s.evidence.fresh());

        // A change breaks `b`, and the user stops the run before the watches are run again.
        std::fs::remove_file(s.ws.root.join("ok")).unwrap();
        s.evidence.changed("edit");
        let stopped = CancellationToken::new();
        stopped.cancel();
        let (reports, _) = run_stale(&mut s, &stopped).await;
        assert!(
            reports.is_empty() && !s.evidence.fresh(),
            "nothing ran, so nothing is verified"
        );

        // The next run still owes both of them, and finds the one that broke.
        let (reports, _) = run_stale(&mut s, &CancellationToken::new()).await;
        assert_eq!(reports.len(), 2);
        assert!(!s.evidence.fresh() && s.watches.items[1].state == WatchState::Fail);
    }

    #[tokio::test]
    async fn timing_noise_is_not_a_change_but_a_different_failure_is() {
        let mut s = test_session("watch-digest", Features::sage());
        let cancel = CancellationToken::new();
        std::fs::write(s.ws.root.join("which"), "test_a").unwrap();
        add(
            &mut s,
            "t",
            "echo \"FAILED $(cat which) in 0.$(date +%N | cut -c1-3)s\"; exit 1",
        );
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
        let mut s = test_session("watch-cwd", Features::sage());
        let cancel = CancellationToken::new();
        add(&mut s, "w", "cd / && true");
        run_stale(&mut s, &cancel).await;
        assert_eq!(s.ws.cwd, s.ws.root);

        add(&mut s, "w", "false");
        assert_eq!(s.watches.items.len(), 1);
        assert_eq!(
            s.watches.items[0].state,
            WatchState::Unknown,
            "a redefined watch starts over"
        );
        assert!(s.watches.remove("w"));
        assert!(
            !s.watches.all_pass(),
            "no watches is not the same as all watches passing"
        );
    }

    fn finished(sess: &Session, code: i32, text: &str) -> Completed {
        Completed {
            cwd: sess.ws.cwd.clone(),
            code: Some(code),
            ms: 40,
            text: text.to_string(),
            output_complete: true,
            predicate_text: None,
        }
    }

    #[tokio::test]
    async fn the_models_test_run_becomes_a_watch_and_is_never_run_twice() {
        let mut s = test_session("watch-auto", Features::sage());
        let cancel = CancellationToken::new();
        std::fs::write(
            s.ws.root.join("check.sh"),
            "test -f ok && echo PASS || { echo 'FAIL: ok is missing'; exit 1; }\n",
        )
        .unwrap();

        // Not a test run, a runner that could not start, a slow run: none becomes a watch.
        let run = finished(&s, 0, "a.txt");
        assert!(observe_unchanged(&mut s, "ls", &run).notice.is_none());
        let run = finished(&s, 127, "bash: pytest: command not found");
        assert!(
            observe_unchanged(&mut s, "pytest -q", &run)
                .notice
                .is_none()
        );
        let slow = Completed {
            ms: 90_000,
            ..finished(&s, 1, "FAILED")
        };
        assert!(
            observe_unchanged(&mut s, "pytest -q", &slow)
                .notice
                .is_none()
        );
        assert!(s.watches.items.is_empty());

        // The failing run the model made is the watch's first result.
        let run = finished(&s, 1, "FAIL: ok is missing");
        let seen = observe_unchanged(&mut s, "sh check.sh", &run);
        assert!(
            seen.notice.as_deref().unwrap().starts_with(
                "watch tests: FAIL · `sh check.sh` is now watched: it re-runs by itself"
            ),
            "{:?}",
            seen.notice
        );
        assert_eq!(
            (seen.reports[0].state, seen.reports[0].change),
            (WatchState::Fail, WatchChange::New)
        );
        assert!(s.watches.items[0].auto);
        let (reports, _) = run_stale(&mut s, &cancel).await;
        assert!(reports.is_empty(), "nothing changed since that run");

        // After a change the harness runs it by itself and compares with the run it adopted.
        std::fs::write(s.ws.root.join("ok"), "").unwrap();
        s.evidence.changed("edit");
        let (_, text) = run_stale(&mut s, &cancel).await;
        assert_eq!(text, "watch tests: FAIL → PASS · PASS");

        // The model running the same command itself updates the watch instead of duplicating the run.
        s.evidence.changed("edit");
        let run = finished(&s, 0, "PASS");
        let seen = observe_unchanged(&mut s, "sh check.sh", &run);
        assert!(seen.reports.is_empty() && seen.notice.is_none());
        assert!(s.watches.all_pass_at(s.evidence.epoch));
        let (reports, _) = run_stale(&mut s, &cancel).await;
        assert!(reports.is_empty());

        // A narrower run of it changes nothing; a different test command takes its place.
        assert!(
            observe_unchanged(&mut s, "sh check.sh --only one", &run)
                .notice
                .is_none()
        );
        assert_eq!(s.watches.items[0].cmd, "sh check.sh");
        let run = finished(&s, 1, "1 failed");
        let seen = observe_unchanged(&mut s, "python3 -m unittest", &run);
        assert_eq!(
            seen.notice.as_deref(),
            Some("watch tests: FAIL · it now follows `python3 -m unittest`.")
        );
        assert_eq!(
            (s.watches.items.len(), s.watches.items[0].cmd.as_str()),
            (1, "python3 -m unittest")
        );

        // `cd` into the directory it already runs in is not a different command.
        let run = finished(&s, 0, "OK");
        let wrapped = format!("cd {} && python3 -m unittest", s.ws.cwd.display());
        let seen = observe_unchanged(&mut s, &wrapped, &run);
        assert!(seen.notice.is_none() && s.watches.items[0].state == WatchState::Pass);
    }

    #[tokio::test]
    async fn the_test_command_a_task_names_is_run_and_watched_before_the_first_turn() {
        assert_eq!(
            named_in("`python3 -m unittest` fails here. Fix `pager.py`."),
            Some("python3 -m unittest")
        );
        assert_eq!(
            named_in("Fix `summary.sh` so that `sh check.sh` passes."),
            Some("sh check.sh")
        );
        // Code, files, and anything chained or piped are not a command to run unasked.
        for task in [
            "Create `roman.py` with `to_roman(n)`.",
            "Run `rm -rf build && pytest`.",
            "`pytest | tail -3` shows the failure",
            "no backticks: pytest fails",
            "an unbalanced ` tick",
        ] {
            assert_eq!(named_in(task), None, "{task}");
        }

        let mut s = test_session("watch-preflight", Features::sage());
        let cancel = CancellationToken::new();
        std::fs::write(
            s.ws.root.join("check.sh"),
            "test -f ok && echo PASS || { echo 'FAIL: ok is missing'; exit 1; }\n",
        )
        .unwrap();
        let report = preflight(&mut s, "`sh check.sh` fails. Make it pass.", &cancel)
            .await
            .expect("the task names a test command");
        assert_eq!(
            (report.state, report.change),
            (WatchState::Fail, WatchChange::New)
        );
        assert_eq!(
            report.detail,
            "watch tests: FAIL (expected exit 0, got exit 1)\nFAIL: ok is missing"
        );
        assert!(s.watches.items[0].auto);

        // From here on it behaves like any watch.
        std::fs::write(s.ws.root.join("ok"), "").unwrap();
        s.evidence.changed("edit");
        assert_eq!(
            run_stale(&mut s, &cancel).await.1,
            "watch tests: FAIL → PASS · PASS"
        );

        // It is the check the task asked for: another test run of the model's does not take its place.
        let other = finished(&s, 1, "1 failed");
        let seen = observe_unchanged(&mut s, "python3 -m unittest", &other);
        assert!(seen.notice.is_none() && seen.reports.is_empty());
        assert_eq!(
            (
                s.watches.items.len(),
                s.watches.items[0].cmd.as_str(),
                s.watches.items[0].state
            ),
            (1, "sh check.sh", WatchState::Pass)
        );

        // With no command named, the model's own test run is watched, but not a throwaway one:
        // a script outside the workspace, or one written on the spot.
        let mut s = test_session("watch-throwaway", Features::sage());
        let run = finished(&s, 1, "AssertionError");
        let tmp = std::env::temp_dir().join("sage-scratch-verify_test.py");
        let inside = s.ws.root.join("tests/test_all.py");
        for scratch in [
            format!("python3 {}", tmp.display()),
            "cd /tmp && node --test".to_string(),
            "python3 - <<'EOF' && python3 -m unittest\nimport m\nEOF".to_string(),
        ] {
            assert!(throwaway(&scratch, &s.ws.root), "{scratch}");
            assert!(
                observe_unchanged(&mut s, &scratch, &run).notice.is_none()
                    && s.watches.items.is_empty(),
                "{scratch}"
            );
        }
        assert!(
            !throwaway(&format!("python3 {}", inside.display()), &s.ws.root)
                && !throwaway("npm test 2>&1 | tail -5", &s.ws.root)
        );
        assert!(
            observe_unchanged(&mut s, &format!("python3 {}", inside.display()), &run)
                .notice
                .is_some()
        );

        // A command that cannot start is not kept, and the mechanism can be switched off.
        let mut s = test_session("watch-preflight-missing", Features::sage());
        assert!(
            preflight(&mut s, "`./run_tests.sh` fails.", &cancel)
                .await
                .is_none()
                && s.watches.items.is_empty()
        );
        let mut s = test_session(
            "watch-preflight-off",
            Features::sage().without("preflight").unwrap(),
        );
        assert!(
            preflight(&mut s, "`sh check.sh` fails.", &cancel)
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn a_later_watch_cannot_certify_an_earlier_result_against_its_changes() {
        let mut s = test_session("watch-mutating-batch", Features::sage());
        std::fs::write(s.ws.root.join("data.txt"), "good\n").unwrap();
        add(&mut s, "checks", "grep -qx good data.txt");
        add(&mut s, "mutates", "printf 'bad\\n' > data.txt");
        s.evidence.changed("an edit");
        let (reports, _) = run_stale(&mut s, &CancellationToken::new()).await;
        assert_eq!(reports[0].state, WatchState::Pass);
        assert!(!s.evidence.fresh());
        assert!(!s.watches.all_pass_at(s.evidence.epoch));
    }

    #[tokio::test]
    async fn a_watch_reports_paths_the_way_the_model_gives_them() {
        use crate::anchors::anchor;
        let mut s = test_session("watch-short-paths", Features::sage());
        let cancel = CancellationToken::new();
        let root = s.ws.root.display().to_string();
        std::fs::write(s.ws.root.join("calc.py"), "a = 1\nb = a / 0\n").unwrap();
        std::fs::write(
            s.ws.root.join("check.sh"),
            "echo \"$PWD/calc.py:2: ZeroDivisionError\"; echo \"rootdir: $PWD\"; exit 1\n",
        )
        .unwrap();

        // The absolute path is gone from the report, and the line it pointed at is still found.
        let report = preflight(&mut s, "`sh check.sh` fails.", &cancel)
            .await
            .expect("the task names a test command");
        let wanted = format!(
            "watch tests: FAIL (expected exit 0, got exit 1)\ncalc.py:2: ZeroDivisionError\nrootdir: .\n--- where (current file content; anchors work in edit) ---\ncalc.py\n  {}|a = 1\n> {}|b = a / 0",
            anchor(1, "a = 1"),
            anchor(2, "b = a / 0")
        );
        assert_eq!(report.detail, wanted);

        // On disk the output is as the command printed it; reopened, it reads as it was shown.
        let kept = std::fs::read_to_string(s.spill_path("out:1").unwrap()).unwrap();
        assert_eq!(
            kept,
            format!("{root}/calc.py:2: ZeroDivisionError\nrootdir: {root}\n")
        );
        let reopened = crate::tools::read::run(&mut s, &serde_json::json!({"path": "out:1"}));
        assert_eq!(
            reopened.observation,
            "out:1 · lines 1-2 of 2\n1|calc.py:2: ZeroDivisionError\n2|rootdir: ."
        );

        let mut s = test_session(
            "watch-long-paths",
            Features::sage().without("short_paths").unwrap(),
        );
        let root = s.ws.root.display().to_string();
        std::fs::write(s.ws.root.join("calc.py"), "a = 1\nb = a / 0\n").unwrap();
        add(
            &mut s,
            "t",
            "echo \"$PWD/calc.py:2: ZeroDivisionError\"; exit 1",
        );
        let (_, text) = run_stale(&mut s, &cancel).await;
        assert!(
            text.contains(&format!("\n{root}/calc.py:2: ZeroDivisionError\n--- where"))
                && text.ends_with("|b = a / 0"),
            "{text}"
        );
    }

    #[tokio::test]
    async fn without_auto_watch_only_requested_watches_adopt_a_run() {
        let mut s = test_session(
            "watch-manual",
            Features::sage().without("auto-watch").unwrap(),
        );
        let run = finished(&s, 1, "FAILED");
        assert!(
            observe_unchanged(&mut s, "pytest -q", &run)
                .notice
                .is_none()
                && s.watches.items.is_empty()
        );

        add(&mut s, "t", "pytest -q");
        let seen = observe_unchanged(&mut s, "pytest -q", &run);
        assert!(seen.notice.is_none());
        assert_eq!(
            (seen.reports[0].state, seen.reports[0].change),
            (WatchState::Fail, WatchChange::New)
        );
        assert_eq!(
            seen.reports[0].detail, "watch t: FAIL (expected exit 0, got exit 1)",
            "the model has the output already"
        );
    }

    #[test]
    fn recognises_test_runs_whose_exit_status_is_the_verdict() {
        let yes = [
            "pytest -q",
            "python3 -m unittest discover -s tests -v",
            "python3 -m pytest tests/test_a.py 2>&1 | tail -20",
            "cd pkg && cargo test",
            "npm test",
            "npm run test:unit",
            "npx vitest run",
            "uv run pytest",
            "FOO=1 timeout 60 go test ./...",
            "./run_tests.sh",
            "sh check.sh",
            "python3 test_calc.py",
            "make -j4 check",
            "node --test",
            ".venv/bin/pytest -x",
            "(cd api && ./gradlew test)",
        ];
        for cmd in yes {
            assert!(test_like(cmd), "{cmd}");
        }
        let no = [
            "pytest || true",
            "pytest; echo done",
            "pytest &",
            "echo test",
            "test -f a.txt",
            "cat test_calc.py",
            "python3 calc.py",
            "python3 -c 'import unittest'",
            "grep -rn test .",
            "cargo build",
            "ls tests",
            "python3 conftest.py",
            "git checkout tests",
            "",
        ];
        for cmd in no {
            assert!(!test_like(cmd), "{cmd}");
        }
    }
}
