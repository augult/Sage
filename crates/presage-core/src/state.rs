//! Per-session mutable state shared by the tools and the script executor.

use crate::economy::Prices;
use crate::lessons::LessonBook;
use crate::tools::jobs::Jobs;
use crate::watches::Watches;
use crate::workspace::{Seen, Workspace, fingerprint};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::path::PathBuf;
use std::time::Duration;

/// Switches for each mechanism, so that any of them can be ablated and measured. `classic()` is a
/// conventional tool loop used as the baseline in A/B runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Features {
    /// Calls in one reply run as a script that halts at the first surprise.
    pub scripts: bool,
    /// `expect` on shell calls; confirmed predictions are answered with a receipt.
    pub receipts: bool,
    /// Source locations named in failing output are attached with anchors.
    pub expand: bool,
    /// `watch` tool and automatic re-runs after changes.
    pub watches: bool,
    /// The model's own test command becomes a watch without being asked. Needs `watches`.
    pub auto_watch: bool,
    /// A test command named in the task is run and watched before the first turn. Needs `auto_watch`.
    pub preflight: bool,
    /// `finish` requires a passing check since the last change.
    pub gate: bool,
    /// Error→fix pairs are learned per project and recalled.
    pub lessons: bool,
    /// Lines carry content-hash anchors usable by `edit`.
    pub anchors: bool,
    /// Unchanged file windows are not printed twice.
    pub dedup: bool,
    /// Edited files are syntax-checked immediately.
    pub post_edit_check: bool,
    /// The first message carries a snapshot of the environment.
    pub bootstrap: bool,
    /// The files the task names are in the first message, already read.
    pub attach: bool,
    /// Command output is shown without the workspace's absolute path.
    pub short_paths: bool,
    /// The first `finish` after a change is answered once with a review of the task instead of being accepted.
    pub review: bool,
    /// A small workspace is listed in full, and as much of it as fits is in the first message, already read. Needs `attach`.
    pub survey: bool,
}

impl Features {
    pub fn presage() -> Self {
        Self { scripts: true, receipts: true, expand: true, watches: true, auto_watch: true, preflight: true, gate: true, lessons: true, anchors: true, dedup: true, post_edit_check: true, bootstrap: true, attach: true, short_paths: true, review: true, survey: true }
    }

    pub fn classic() -> Self {
        Self { scripts: false, receipts: false, expand: false, watches: false, auto_watch: false, preflight: false, gate: false, lessons: false, anchors: false, dedup: false, post_edit_check: false, bootstrap: false, attach: false, short_paths: false, review: false, survey: false }
    }

    /// The mechanisms `without` can switch off.
    pub const NAMES: &'static [&'static str] = &["scripts", "receipts", "expand", "watches", "auto_watch", "preflight", "gate", "lessons", "anchors", "dedup", "post_edit_check", "bootstrap", "attach", "short_paths", "review", "survey"];

    /// Switches one mechanism off by name (`auto_watch` or `auto-watch`), for ablation runs.
    pub fn without(mut self, name: &str) -> Option<Self> {
        let slot = match name.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "scripts" => &mut self.scripts,
            "receipts" => &mut self.receipts,
            "expand" => &mut self.expand,
            "watches" => &mut self.watches,
            "auto_watch" => &mut self.auto_watch,
            "preflight" => &mut self.preflight,
            "gate" => &mut self.gate,
            "lessons" => &mut self.lessons,
            "anchors" => &mut self.anchors,
            "dedup" => &mut self.dedup,
            "post_edit_check" => &mut self.post_edit_check,
            "bootstrap" => &mut self.bootstrap,
            "attach" => &mut self.attach,
            "short_paths" => &mut self.short_paths,
            "review" => &mut self.review,
            "survey" => &mut self.survey,
            _ => return None,
        };
        *slot = false;
        Some(self)
    }
}

/// Output budgets. Sized so a failure is fully legible and a success is never more than a screenful.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Limits {
    pub ok_head: usize,
    pub ok_tail: usize,
    pub ok_chars: usize,
    pub fail_head: usize,
    pub fail_tail: usize,
    pub fail_chars: usize,
    pub read_lines: usize,
    pub read_whole_up_to: usize,
    pub read_chars: usize,
    pub search_matches: usize,
    pub expand_radius: usize,
    pub expand_windows: usize,
    pub expand_chars: usize,
    pub default_timeout_secs: u64,
    pub max_timeout_secs: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            ok_head: 60,
            ok_tail: 60,
            ok_chars: 8_000,
            fail_head: 20,
            fail_tail: 80,
            fail_chars: 10_000,
            read_lines: 250,
            read_whole_up_to: 400,
            read_chars: 20_000,
            search_matches: 60,
            expand_radius: 4,
            expand_windows: 3,
            expand_chars: 2_400,
            default_timeout_secs: 120,
            max_timeout_secs: 3_600,
        }
    }
}

/// What the harness knows about whether the current state of the workspace has been checked.
#[derive(Debug, Default)]
pub struct Evidence {
    /// Incremented every time the workspace changes.
    pub epoch: u64,
    /// The epoch at which a check last passed, and which check it was.
    pub verified: Option<(u64, String)>,
    /// Human-readable description of the most recent change.
    pub last_change: String,
    /// Workspace fingerprint as of the last time it was computed.
    fingerprint: Option<u64>,
    /// How often `finish` has been refused for lack of a check since the last change.
    pub refusals: u8,
    /// The review of the task has been asked for since the last user message.
    pub reviewed: bool,
    /// The epoch when the last user message arrived: only changes since then are reviewed.
    pub message_epoch: u64,
}

const FINGERPRINT_BUDGET: Duration = Duration::from_millis(400);

impl Evidence {
    pub fn fresh(&self) -> bool {
        self.epoch == 0 || self.verified.as_ref().is_some_and(|(e, _)| *e == self.epoch)
    }

    /// A tool changed files itself.
    pub fn changed(&mut self, what: impl Into<String>) {
        self.epoch += 1;
        self.refusals = 0;
        self.last_change = what.into();
        self.fingerprint = None;
    }

    /// Call before running a command whose effect on the workspace is unknown.
    pub fn before_command(&mut self, ws: &Workspace) {
        if self.fingerprint.is_none() {
            self.fingerprint = Some(fingerprint(&ws.root, FINGERPRINT_BUDGET));
        }
    }

    /// Call after such a command. Returns true if it changed the workspace.
    pub fn after_command(&mut self, ws: &Workspace, cmd: &str) -> bool {
        let now = fingerprint(&ws.root, FINGERPRINT_BUDGET);
        let changed = self.fingerprint.is_some_and(|before| before != now);
        self.fingerprint = Some(now);
        if changed {
            self.epoch += 1;
            self.refusals = 0;
            self.last_change = format!("`{}`", crate::util::one_line(cmd, 80));
        }
        changed
    }

    /// A check passed against the workspace as it is right now.
    pub fn passed(&mut self, check: impl Into<String>) {
        self.verified = Some((self.epoch, check.into()));
    }
}

pub struct Session {
    pub ws: Workspace,
    pub seen: Seen,
    pub jobs: Jobs,
    pub watches: Watches,
    pub evidence: Evidence,
    /// The user message being worked on, quoted back by the review.
    pub task: String,
    pub lessons: LessonBook,
    pub features: Features,
    pub limits: Limits,
    pub prices: Option<Prices>,
    /// Session directory: event log, spilled outputs, shell state.
    pub dir: PathBuf,
    /// Monotonic step counter across the whole session.
    pub step: u32,
    spill_seq: u32,
    pub shell_env: Vec<(String, String)>,
}

impl Session {
    pub fn new(ws: Workspace, dir: PathBuf, features: Features, lessons: LessonBook, prices: Option<Prices>) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir.join("out"))?;
        Ok(Self {
            ws,
            seen: Seen::default(),
            jobs: Jobs::default(),
            watches: Watches::default(),
            evidence: Evidence::default(),
            task: String::new(),
            lessons,
            features,
            limits: Limits::default(),
            prices,
            dir,
            step: 0,
            spill_seq: 0,
            shell_env: non_interactive_env(),
        })
    }

    /// Allocates the next `out:N` handle and the file behind it.
    pub fn next_spill(&mut self) -> (String, PathBuf) {
        self.spill_seq += 1;
        (format!("out:{}", self.spill_seq), self.dir.join("out").join(format!("{}.log", self.spill_seq)))
    }

    pub fn spill_path(&self, handle: &str) -> Option<PathBuf> {
        let n: u32 = handle.trim().strip_prefix("out:")?.parse().ok()?;
        Some(self.dir.join("out").join(format!("{n}.log")))
    }

    pub fn shell_state(&self) -> PathBuf {
        self.dir.join("shell")
    }

    /// Command output as the model is shown it. See `shown`.
    pub fn shown(&self, text: String) -> String {
        shown(&self.features, &self.ws, text)
    }
}

/// Command output as the model is shown it: with `short_paths`, paths under the directory the tools
/// run in are relative to it, like the paths the model gives. A long absolute prefix repeated on
/// every line of a traceback is bought once and then re-read on every later turn. What is kept on
/// disk (`out:N`) stays as the command printed it.
///
/// Takes the two fields rather than the session, so that it can be called while another part of
/// the session is borrowed.
pub fn shown(features: &Features, ws: &Workspace, text: String) -> String {
    if !features.short_paths {
        return text;
    }
    let short = match ws.shorten(&text) {
        Cow::Owned(short) => Some(short),
        Cow::Borrowed(_) => None,
    };
    short.unwrap_or(text)
}

/// Environment that keeps tools from waiting on a human or painting the output.
fn non_interactive_env() -> Vec<(String, String)> {
    [
        ("TERM", "dumb"),
        ("NO_COLOR", "1"),
        ("CLICOLOR", "0"),
        ("FORCE_COLOR", "0"),
        ("CARGO_TERM_COLOR", "never"),
        ("PAGER", "cat"),
        ("GIT_PAGER", "cat"),
        ("MANPAGER", "cat"),
        ("GIT_TERMINAL_PROMPT", "0"),
        ("GIT_EDITOR", "true"),
        ("EDITOR", "true"),
        ("DEBIAN_FRONTEND", "noninteractive"),
        ("PIP_DISABLE_PIP_VERSION_CHECK", "1"),
        ("PIP_NO_INPUT", "1"),
        ("PYTHONUNBUFFERED", "1"),
        ("NPM_CONFIG_UPDATE_NOTIFIER", "false"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

#[cfg(test)]
pub(crate) fn test_session(name: &str, features: Features) -> Session {
    let base = std::env::temp_dir().join(format!("presage-{name}-{}", crate::util::rand_hex(4)));
    let root = base.join("ws");
    std::fs::create_dir_all(&root).unwrap();
    let ws = Workspace::new(&root).unwrap();
    Session::new(ws, base.join("session"), features, LessonBook::in_memory(), None).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_goes_stale_on_change_and_fresh_on_pass() {
        let mut s = test_session("evidence", Features::presage());
        assert!(s.evidence.fresh(), "nothing changed yet, nothing to verify");

        s.evidence.changed("edit a.py");
        assert!(!s.evidence.fresh());
        s.evidence.passed("pytest -q");
        assert!(s.evidence.fresh());

        // A command that does not touch files leaves the evidence fresh…
        s.evidence.before_command(&s.ws);
        assert!(!s.evidence.after_command(&s.ws, "git status"));
        assert!(s.evidence.fresh());

        // …one that does makes it stale again.
        s.evidence.before_command(&s.ws);
        std::fs::write(s.ws.root.join("gen.txt"), "x").unwrap();
        assert!(s.evidence.after_command(&s.ws, "make gen"));
        assert!(!s.evidence.fresh());
        assert_eq!(s.evidence.last_change, "`make gen`");
    }

    #[test]
    fn every_mechanism_can_be_switched_off_by_name() {
        for name in Features::NAMES {
            let without = Features::presage().without(name).unwrap_or_else(|| panic!("{name} is listed but cannot be switched off"));
            assert_ne!(without, Features::presage(), "{name}");
            assert_eq!(Features::presage().without(&name.replace('_', "-")), Some(without), "{name}, spelled with dashes");
            assert_eq!(Features::classic().without(name), Some(Features::classic()), "{name} is off in the baseline");
        }
        assert!(!Features::presage().without("attach").unwrap().attach && !Features::presage().without("short-paths").unwrap().short_paths);
        assert_eq!(Features::presage().without("telepathy"), None);
    }

    #[test]
    fn command_output_loses_the_workspace_path_only_where_that_is_switched_on() {
        let mut s = test_session("shown", Features::presage());
        let root = s.ws.root.display().to_string();
        let raw = format!("  File \"{root}/pager.py\", line 3\nrootdir: {root}\n");
        assert_eq!(s.shown(raw.clone()), "  File \"pager.py\", line 3\nrootdir: .\n");
        assert_eq!(s.shown("3 passed".to_string()), "3 passed");

        s.features = Features::presage().without("short_paths").unwrap();
        assert_eq!(s.shown(raw.clone()), raw);
    }

    #[test]
    fn spill_handles_round_trip() {
        let mut s = test_session("spill", Features::presage());
        let (h1, p1) = s.next_spill();
        let (h2, _) = s.next_spill();
        assert_eq!((h1.as_str(), h2.as_str()), ("out:1", "out:2"));
        assert_eq!(s.spill_path("out:1"), Some(p1));
        assert_eq!(s.spill_path("out:x"), None);
    }
}
