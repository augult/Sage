//! Per-session mutable state shared by the tools and the script executor.

use crate::anchors::History;
use crate::changes::Baseline;
use crate::economy::Prices;
use crate::tools::jobs::Jobs;
use crate::watches::{WatchSpec, Watches};
use crate::workspace::{Fingerprint, Seen, Workspace, fingerprint_off_thread};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Switches for each mechanism, so that any of them can be ablated and measured. `classic()` is a
/// conventional tool loop used as the baseline in A/B runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// A small workspace is listed in full, and as much of it as fits is in the first message, already read. Needs `attach`.
    pub survey: bool,
    /// Protected files and named deliverable files in the task are checked mechanically.
    pub contract: bool,
}

/// What a session is run for.
///
/// `Sage` spends as few requests and tokens as it can and gives up a little of the result for
/// it. `Plain` is the loop every other harness runs, one tool call after another with nothing of
/// Sage's around it; users know it as `default`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Sage,
    #[serde(rename = "default")]
    Plain,
}

impl Mode {
    pub const NAMES: &'static [&'static str] = &["sage", "default"];

    /// A mode by its name. `classic` is what `default` was called before.
    pub fn parse(name: &str) -> Option<Mode> {
        match name.trim().to_ascii_lowercase().as_str() {
            "sage" => Some(Mode::Sage),
            "default" | "classic" => Some(Mode::Plain),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Sage => "sage",
            Mode::Plain => "default",
        }
    }

    pub fn features(self) -> Features {
        match self {
            Mode::Sage => Features::sage(),
            Mode::Plain => Features::classic(),
        }
    }
}

impl Features {
    pub fn sage() -> Self {
        Self {
            scripts: true,
            receipts: true,
            expand: true,
            watches: true,
            auto_watch: true,
            preflight: true,
            gate: true,
            anchors: true,
            dedup: true,
            post_edit_check: true,
            bootstrap: true,
            attach: true,
            short_paths: true,
            survey: true,
            contract: true,
        }
    }

    pub fn classic() -> Self {
        Self {
            scripts: false,
            receipts: false,
            expand: false,
            watches: false,
            auto_watch: false,
            preflight: false,
            gate: false,
            anchors: false,
            dedup: false,
            post_edit_check: false,
            bootstrap: false,
            attach: false,
            short_paths: false,
            survey: false,
            contract: false,
        }
    }

    /// The mechanisms `without` can switch off and `with` can switch on.
    pub const NAMES: &'static [&'static str] = &[
        "scripts",
        "receipts",
        "expand",
        "watches",
        "auto_watch",
        "preflight",
        "gate",
        "anchors",
        "dedup",
        "post_edit_check",
        "bootstrap",
        "attach",
        "short_paths",
        "survey",
        "contract",
    ];

    /// Switches one mechanism off by name (`auto_watch` or `auto-watch`), for ablation runs.
    pub fn without(self, name: &str) -> Option<Self> {
        self.set(name, false)
    }

    /// Switches one mechanism on by name, e.g. one of Sage's in the conventional loop.
    pub fn with(self, name: &str) -> Option<Self> {
        self.set(name, true)
    }

    /// Mechanisms that were taken out of Sage with the Prosage mode. A session stored before that
    /// may still name them; it comes back without them.
    pub const RETIRED: &'static [&'static str] = &[
        "long_thought",
        "unhurried",
        "examiner",
        "panel",
        "referee",
        "routing",
    ];

    fn set(mut self, name: &str, on: bool) -> Option<Self> {
        let slot = match name.trim().to_ascii_lowercase().replace('-', "_").as_str() {
            "scripts" => &mut self.scripts,
            "receipts" => &mut self.receipts,
            "expand" => &mut self.expand,
            "watches" => &mut self.watches,
            "auto_watch" => &mut self.auto_watch,
            "preflight" => &mut self.preflight,
            "gate" => &mut self.gate,
            "anchors" => &mut self.anchors,
            "dedup" => &mut self.dedup,
            "post_edit_check" => &mut self.post_edit_check,
            "bootstrap" => &mut self.bootstrap,
            "attach" => &mut self.attach,
            "short_paths" => &mut self.short_paths,
            "survey" => &mut self.survey,
            "contract" => &mut self.contract,
            _ => return None,
        };
        *slot = on;
        Some(self)
    }
}

/// Output budgets. Sized so a failure is fully legible and a success is never more than a screenful.
#[derive(Clone, Copy, Debug)]
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
    fingerprint: Option<Fingerprint>,
    /// The workspace is too large to see whole, so a change a command makes may go unseen.
    partial_view: bool,
    /// How often `finish` has been refused for lack of a check since the last change.
    pub refusals: u8,
    /// The epoch when the last user message arrived: only changes since then are the message's own.
    pub message_epoch: u64,
}

/// Said after "verified by …" where the workspace is too large to see every change a command makes.
const PARTIAL_VIEW: &str = " (the workspace is too large to see every change a command makes)";

impl Evidence {
    pub fn fresh(&self) -> bool {
        !self.partial_view
            && (self.epoch == 0
                || self
                    .verified
                    .as_ref()
                    .is_some_and(|(e, _)| *e == self.epoch))
    }

    /// What stands behind a `fresh` state, in words.
    pub fn verified_by(&self) -> String {
        match &self.verified {
            Some((_, by)) if self.epoch > 0 => format!("verified by {by}{}", self.caveat()),
            _ => "no changes were made".to_string(),
        }
    }

    /// What a claim of verification has to add: nothing, unless the last look at the workspace
    /// could not see all of it.
    pub fn caveat(&self) -> &'static str {
        if self.partial_view { PARTIAL_VIEW } else { "" }
    }

    fn look(&mut self, now: Fingerprint) {
        self.partial_view = !now.complete;
        self.fingerprint = Some(now);
    }

    /// A tool changed files itself.
    pub fn changed(&mut self, what: impl Into<String>) {
        self.epoch += 1;
        self.refusals = 0;
        self.last_change = what.into();
        self.fingerprint = None;
    }

    /// Call before running a command whose effect on the workspace is unknown.
    pub async fn before_command(&mut self, ws: &Workspace, deadline: Option<Instant>) {
        if self.fingerprint.is_none() {
            self.refresh(ws, deadline, None).await;
        }
    }

    /// Call after such a command. Returns true if it changed the workspace.
    pub async fn after_command(
        &mut self,
        ws: &Workspace,
        cmd: &str,
        deadline: Option<Instant>,
    ) -> bool {
        self.refresh(ws, deadline, Some(cmd)).await
    }

    async fn refresh(
        &mut self,
        ws: &Workspace,
        deadline: Option<Instant>,
        cmd: Option<&str>,
    ) -> bool {
        let now = match deadline {
            Some(deadline) => {
                tokio::time::timeout_at(deadline.into(), fingerprint_off_thread(&ws.root))
                    .await
                    .ok()
            }
            None => Some(fingerprint_off_thread(&ws.root).await),
        };
        let Some(now) = now else {
            self.changed("the workspace scan exceeded the time budget");
            self.partial_view = true;
            return true;
        };
        let changed = self.fingerprint.is_some_and(|before| before != now);
        self.look(now);
        if changed && let Some(cmd) = cmd {
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

/// What a stored session keeps of its `Session` (see `Session::checkpoint`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SessionState {
    /// Steps taken so far, so that the numbers the model was shown are not given out again.
    pub step: u32,
    /// The last `out:N` handle given out.
    pub spill_seq: u32,
    /// Where the shell was.
    pub cwd: String,
    pub touched: Vec<String>,
    pub baseline: Option<Baseline>,
    pub watches: Vec<WatchSpec>,
    pub dialects: Vec<(String, bool)>,
}

/// The highest `N` of the `out/N.log` files a session directory holds.
fn highest_spill(dir: &std::path::Path) -> u32 {
    std::fs::read_dir(dir.join("out"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            e.file_name()
                .to_str()?
                .strip_suffix(".log")?
                .parse::<u32>()
                .ok()
        })
        .max()
        .unwrap_or(0)
}

pub struct Session {
    pub ws: Workspace,
    pub seen: Seen,
    pub jobs: Jobs,
    pub watches: Watches,
    pub evidence: Evidence,
    /// The user message being worked on.
    pub task: String,
    /// What was already uncommitted when the session began, so the report of changes shows only its own.
    pub baseline: Option<Baseline>,
    /// Files changed through `edit` and `write`, for a report where git cannot say what changed.
    pub touched: Vec<String>,
    pub features: Features,
    pub limits: Limits,
    pub prices: Option<Prices>,
    /// Session directory: event log, spilled outputs, shell state.
    pub dir: PathBuf,
    /// Monotonic step counter across the whole session.
    pub step: u32,
    /// Counts the replies whose calls have been run: an anchor never refers to an edit of its own reply.
    pub reply: u32,
    /// What the tools have changed in each file, so that anchors from before still find their lines.
    pub histories: HashMap<PathBuf, History>,
    /// Files the syntax check cannot judge: whether each already failed it before the tools touched it.
    pub dialects: HashMap<PathBuf, bool>,
    spill_seq: u32,
    pub shell_env: Vec<(String, String)>,
    /// When the time budget of the run in progress runs out, where it has one. Nothing a tool waits
    /// for is waited for past it.
    pub deadline: Option<Instant>,
    /// Per-session scratch directory for `TMPDIR`, outside `dir` so `$TMPDIR/..` never exposes `exam/`.
    scratch: PathBuf,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

impl Session {
    pub fn new(
        ws: Workspace,
        dir: PathBuf,
        features: Features,
        prices: Option<Prices>,
    ) -> std::io::Result<Self> {
        crate::util::private_dir(&dir)?;
        std::fs::create_dir_all(dir.join("out"))?;
        let scratch =
            std::env::temp_dir().join(format!("sage-scratch-{}", crate::util::rand_hex(6)));
        let _ = std::fs::create_dir_all(&scratch);
        let shell_env = non_interactive_env();
        #[cfg(unix)]
        let shell_env = {
            let mut env = shell_env;
            if scratch.is_dir() {
                env.push(("TMPDIR".to_string(), scratch.to_string_lossy().into_owned()));
            }
            env
        };
        Ok(Self {
            ws,
            seen: Seen::default(),
            jobs: Jobs::default(),
            watches: Watches::default(),
            evidence: Evidence::default(),
            task: String::new(),
            baseline: None,
            touched: Vec::new(),
            features,
            limits: Limits::default(),
            prices,
            dir,
            step: 0,
            reply: 0,
            histories: HashMap::new(),
            dialects: HashMap::new(),
            spill_seq: 0,
            shell_env,
            deadline: None,
            scratch,
        })
    }

    /// What a session that is stored keeps of this state. Left out on purpose, so that it starts
    /// empty when the session is brought back: what was read (`seen`, so files are shown again),
    /// the anchors' histories (the model's anchors from before find their lines by content, or
    /// fail and it reads again), background jobs, the evidence (nothing is claimed about changes
    /// the harness has not seen) and the results of the watches.
    pub fn checkpoint(&self) -> SessionState {
        let mut dialects: Vec<(String, bool)> = self
            .dialects
            .iter()
            .map(|(path, already)| (path.to_string_lossy().into_owned(), *already))
            .collect();
        dialects.sort();
        SessionState {
            step: self.step,
            spill_seq: self.spill_seq,
            cwd: self.ws.cwd.to_string_lossy().into_owned(),
            touched: self.touched.clone(),
            baseline: self.baseline.clone(),
            watches: self.watches.specs(),
            dialects,
        }
    }

    /// The inverse of `checkpoint`. The shell's directory comes back only where it is still one
    /// inside the workspace; the step and spill counters carry on so that no step or `out:N` handle
    /// from before is reused.
    pub fn restore(&mut self, state: SessionState) {
        self.step = state.step;
        self.spill_seq = state.spill_seq.max(highest_spill(&self.dir));
        let cwd = PathBuf::from(&state.cwd);
        if cwd.is_dir() && cwd.starts_with(&self.ws.root) {
            self.ws.cwd = cwd;
        }
        self.touched = state.touched;
        self.baseline = state.baseline;
        self.watches.restore(state.watches);
        self.dialects = state
            .dialects
            .into_iter()
            .map(|(path, already)| (PathBuf::from(path), already))
            .collect();
    }

    /// `limit`, or what is left of the run's time budget where that is less: never under a second,
    /// so that whatever was asked for is still started.
    pub fn within_budget(&self, limit: Duration) -> Duration {
        match self.deadline {
            Some(at) => limit.min(at.saturating_duration_since(Instant::now())),
            None => limit,
        }
    }

    /// Allocates the next `out:N` handle and the file behind it.
    pub fn next_spill(&mut self) -> (String, PathBuf) {
        self.spill_seq += 1;
        (
            format!("out:{}", self.spill_seq),
            self.dir.join("out").join(format!("{}.log", self.spill_seq)),
        )
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
    let base = std::env::temp_dir().join(format!("sage-{name}-{}", crate::util::rand_hex(4)));
    let root = base.join("ws");
    std::fs::create_dir_all(&root).unwrap();
    let ws = Workspace::new(&root).unwrap();
    Session::new(ws, base.join("session"), features, None).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn evidence_goes_stale_on_change_and_fresh_on_pass() {
        let mut s = test_session("evidence", Features::sage());
        assert!(s.evidence.fresh(), "nothing changed yet, nothing to verify");

        s.evidence.changed("edit a.py");
        assert!(!s.evidence.fresh());
        s.evidence.passed("pytest -q");
        assert!(s.evidence.fresh());

        // A command that does not touch files leaves the evidence fresh…
        s.evidence.before_command(&s.ws, None).await;
        assert!(!s.evidence.after_command(&s.ws, "git status", None).await);
        assert!(s.evidence.fresh());

        // …one that does makes it stale again.
        s.evidence.before_command(&s.ws, None).await;
        std::fs::write(s.ws.root.join("gen.txt"), "x").unwrap();
        assert!(s.evidence.after_command(&s.ws, "make gen", None).await);
        assert!(!s.evidence.fresh());
        assert_eq!(s.evidence.last_change, "`make gen`");
    }

    #[test]
    fn every_mechanism_can_be_switched_off_by_name() {
        for name in Features::NAMES {
            let without = Features::sage()
                .without(name)
                .unwrap_or_else(|| panic!("{name} is listed but cannot be switched off"));
            assert_ne!(without, Features::sage(), "{name} is on in Sage");
            assert_eq!(
                Features::sage().with(name),
                Some(Features::sage()),
                "{name}"
            );
            assert_eq!(
                Features::sage().with(name).unwrap().without(name),
                Some(without),
                "{name}, on and off again"
            );
            assert_eq!(
                Features::sage().without(&name.replace('_', "-")),
                Some(without),
                "{name}, spelled with dashes"
            );
            assert_eq!(
                Features::classic().without(name),
                Some(Features::classic()),
                "{name} is off in the baseline"
            );
        }
        assert!(
            !Features::sage().without("attach").unwrap().attach
                && !Features::sage()
                    .without("short-paths")
                    .unwrap()
                    .short_paths
        );
        assert_eq!(Features::sage().without("telepathy"), None);
        assert!(Features::sage().with("telepathy").is_none());
    }

    #[test]
    fn command_output_loses_the_workspace_path_only_where_that_is_switched_on() {
        let mut s = test_session("shown", Features::sage());
        let root = s.ws.root.display().to_string();
        let raw = format!("  File \"{root}/pager.py\", line 3\nrootdir: {root}\n");
        assert_eq!(
            s.shown(raw.clone()),
            "  File \"pager.py\", line 3\nrootdir: .\n"
        );
        assert_eq!(s.shown("3 passed".to_string()), "3 passed");

        s.features = Features::sage().without("short_paths").unwrap();
        assert_eq!(s.shown(raw.clone()), raw);
    }

    #[cfg(unix)]
    #[test]
    fn a_session_s_folder_is_closed_to_other_users() {
        use std::os::unix::fs::PermissionsExt;
        let s = test_session("private", Features::sage());
        assert_eq!(
            std::fs::metadata(&s.dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        // One that was open, from before folders were made private, is closed when it is used again.
        std::fs::set_permissions(&s.dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let again = Session::new(s.ws.clone(), s.dir.clone(), Features::sage(), None).unwrap();
        assert_eq!(
            std::fs::metadata(&again.dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn spill_handles_round_trip() {
        let mut s = test_session("spill", Features::sage());
        let (h1, p1) = s.next_spill();
        let (h2, _) = s.next_spill();
        assert_eq!((h1.as_str(), h2.as_str()), ("out:1", "out:2"));
        assert_eq!(s.spill_path("out:1"), Some(p1));
        assert_eq!(s.spill_path("out:x"), None);
    }
}
