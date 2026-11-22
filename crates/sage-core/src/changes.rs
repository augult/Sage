//! What the session changed in the workspace, as a report for whoever checks the work.
//!
//! A model asked what it changed spends one reply asking for `git diff` and the next one reading
//! it. The harness can look faster than the model can ask, so a check arrives with what there is
//! to look at: the paths and the diff of the session's own changes, as git sees them.
//!
//! Only the session's own changes are shown. Work that was already uncommitted when the session
//! began is the user's: a model shown it under "what changed" could take it for its own and undo it.
//! `baseline` records that state at the start of the session.

use crate::state::Session;
use crate::text::bound;
use crate::util::{fnv64, truncate_chars};
use crate::workspace::derived;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

const GIT_TIMEOUT: Duration = Duration::from_secs(5);
/// A tree with more uncommitted paths than this at the start is not tracked: too much to tell apart.
const MAX_BASELINE: usize = 200;
const MAX_PATHS: usize = 40;
const DIFF_HEAD: usize = 160;
const DIFF_TAIL: usize = 40;
const DIFF_CHARS: usize = 8_000;

/// The paths that were already uncommitted when the session began, each with a hash of its content
/// then (`None` for one that could not be read, such as a deleted file).
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Baseline {
    dirty: HashMap<String, Option<u64>>,
}

/// `git` in the workspace root, looking only: no pager, no colour, no lock taken on the index.
async fn git(root: &Path, args: &[&str]) -> Option<String> {
    let child = tokio::process::Command::new("git")
        .arg("-C")
        .arg(root)
        // The user's own settings must not change what is read here: `color.status` and
        // `color.diff` outrank `color.ui`, and `status.branch` adds a line that is no path.
        .args([
            "--no-pager",
            "-c",
            "core.quotepath=off",
            "-c",
            "color.ui=never",
            "-c",
            "color.status=never",
            "-c",
            "color.diff=never",
            "-c",
            "status.branch=false",
            "-c",
            "status.relativePaths=true",
        ])
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;
    let out = tokio::time::timeout(GIT_TIMEOUT, child.wait_with_output())
        .await
        .ok()?
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Uncommitted paths below `root`, as `git status --short` lines with paths relative to it. What a
/// test run leaves behind (caches, installed dependencies) is nobody's change and is left out.
async fn status(root: &Path) -> Option<Vec<String>> {
    let out = git(
        root,
        &["status", "--short", "--untracked-files=all", "--", "."],
    )
    .await?;
    Some(
        out.lines()
            .filter(|l| l.len() > 3 && !path_of(l).is_some_and(derived))
            .map(String::from)
            .collect(),
    )
}

/// The path a status line is about (the new name, for a rename). `None` for one git had to quote:
/// such a name cannot be handed back to git as it is printed.
fn path_of(line: &str) -> Option<&str> {
    let path = line.get(3..)?;
    let path = path.rsplit_once(" -> ").map_or(path, |(_, new)| new);
    (!path.starts_with('"')).then_some(path)
}

/// Identifies a file's content; for a very large file, its size and time of change stand in.
fn content_hash(root: &Path, path: &str) -> Option<u64> {
    const HASH_UP_TO: u64 = 4 * 1024 * 1024;
    let file = root.join(path);
    let md = std::fs::metadata(&file).ok()?;
    // What is no file (a link to a device, a pipe) has no content to compare, and may have no end.
    if !md.is_file() {
        return None;
    }
    if md.len() > HASH_UP_TO {
        let changed = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        return Some(fnv64(format!("{}:{changed}", md.len()).as_bytes()));
    }
    std::fs::read(&file).ok().map(|bytes| fnv64(&bytes))
}

/// What is uncommitted in the workspace right now, to be told apart later from what the session
/// does. `None` where git cannot say (no repository, no git) or the tree is too dirty to track.
pub async fn baseline(root: &Path) -> Option<Baseline> {
    let lines = status(root).await?;
    if lines.len() > MAX_BASELINE {
        return None;
    }
    let mut dirty = HashMap::new();
    for line in &lines {
        // A name that cannot be read back makes every later comparison a guess.
        let path = path_of(line)?;
        dirty.insert(path.to_string(), content_hash(root, path));
    }
    Some(Baseline { dirty })
}

/// What the session changed in the workspace: the paths, then the diff of the tracked ones. Falls
/// back to the files changed through `edit` and `write` where git cannot tell. `None` when there is
/// nothing to show.
pub async fn changes(sess: &Session) -> Option<String> {
    match git_changes(sess).await {
        Some(report) => Some(report),
        None if sess.touched.is_empty() => None,
        None => Some(format!(
            "Files you changed with edit or write: {}",
            sess.touched.join(", ")
        )),
    }
}

async fn git_changes(sess: &Session) -> Option<String> {
    let root = &sess.ws.root;
    let base = sess.baseline.as_ref()?;
    let lines = status(root).await?;
    let (mut listed, mut untouched, mut shared) = (Vec::new(), Vec::new(), Vec::new());
    for line in &lines {
        let path = path_of(line);
        match path.and_then(|p| base.dirty.get(p).map(|before| (p, before))) {
            // Uncommitted before the session and the same now: not the session's doing.
            Some((p, before)) if *before == content_hash(root, p) => untouched.push(p),
            Some((p, _)) => {
                shared.push(p);
                listed.push(line.as_str());
            }
            None => listed.push(line.as_str()),
        }
    }
    if listed.is_empty() {
        return None;
    }
    let mut out = String::from("Paths (`git status --short`):\n");
    for line in listed.iter().take(MAX_PATHS) {
        out.push_str(line);
        out.push('\n');
    }
    if listed.len() > MAX_PATHS {
        out.push_str(&format!("… {} more\n", listed.len() - MAX_PATHS));
    }
    if !shared.is_empty() {
        out.push_str(&format!("Already uncommitted before this session, so the diff shows earlier work too (leave that as it is): {}\n", shared.join(", ")));
    }
    if sess.ws.cwd != *root {
        out.push_str(&format!("Paths are relative to {}.\n", root.display()));
    }

    let exclude: Vec<String> = untouched
        .iter()
        .map(|p| format!(":(exclude,literal){p}"))
        .collect();
    let mut args = vec!["diff", "HEAD", "--no-ext-diff", "--relative", "--", "."];
    args.extend(exclude.iter().map(String::as_str));
    // Where there is no commit yet to compare with, the paths have to do.
    if let Some(diff) = git(root, &args).await.filter(|d| !d.trim().is_empty()) {
        let diff_lines: Vec<String> = diff.lines().map(String::from).collect();
        let view = bound(&diff_lines, DIFF_HEAD, DIFF_TAIL, DIFF_CHARS, None);
        out.push_str("Diff of the tracked files (`git diff HEAD`):\n");
        out.push_str(view.text.trim_end());
        if view.hidden_lines > 0 {
            out.push_str("\nRun `git diff HEAD` for the hidden part.");
        }
    }
    Some(out.trim_end().to_string())
}

/// One-pass spec and diff verification prompt when `finish` is called after only the starter test
/// watch has run on a spec-driven task.
pub fn review_text(task: &str, changes: Option<&str>) -> String {
    let block = match changes {
        Some(changes) => format!("<changes>\n{changes}\n</changes>\n"),
        None => String::new(),
    };
    format!(
        "[review] Not accepted yet: only the starter test watch has run, and passing the visible starter tests does not prove every requirement in the task and spec holds.\n<task>\n{}\n</task>\n{block}Complete verification in ONE reply by chaining your check and `finish`:\n1. Audit `<changes>` against `<task>` and the spec or README sentence by sentence: check every rule, error condition and its attributes (such as `.line` on the 2nd item of a list `x[] = 1\\nx[] = ${{bad}}`, and empty leading continuation pieces `k = \\\\\\n b` adding no space), boundary or EOF case, and every CLI flag, caller, config or manifest file.\n2. In your next reply, chain any needed `edit` with ONE self-contained `shell` command (e.g. `python3 -c \"...\"`, `node -e \"...\"`, or `sh -c \"...\"` with `expect_exit: \"0\"`, leaving no scratch files) that tests the rules and edge cases missing from the starter tests, followed immediately by `finish` in the same reply.",
        truncate_chars(task, 6000)
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::state::{Features, test_session};

    fn sh(root: &Path, script: &str) {
        let ok = std::process::Command::new("bash")
            .arg("-c")
            .arg(script)
            .current_dir(root)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "{script}");
    }

    const COMMIT: &str =
        "git -c user.name=t -c user.email=t@localhost -c commit.gpgsign=false commit -q -m base";

    #[tokio::test]
    async fn only_what_the_session_changed_is_shown() {
        let mut sess = test_session("changes-git", Features::sage());
        let root = sess.ws.root.clone();
        sh(
            &root,
            &format!(
                "printf 'a\\nb\\n' > code.py; printf 'x\\n' > wip.py; printf 'k\\n' > both.py; git init -q && git add -A && {COMMIT}"
            ),
        );
        // The user's own work in progress, there before the session.
        sh(
            &root,
            "printf 'x\\nmine\\n' > wip.py; printf 'k\\nmine\\n' > both.py; printf 'n\\n' > notes.txt",
        );
        sess.baseline = baseline(&root).await;
        assert!(sess.baseline.is_some());
        assert_eq!(changes(&sess).await, None, "nothing yet");

        sh(
            &root,
            "printf 'a\\nB\\n' > code.py; printf 'k\\nmine\\ntoo\\n' > both.py; printf 'tmp\\n' > scratch.py",
        );
        let report = changes(&sess).await.unwrap();
        assert!(
            report.starts_with("Paths (`git status --short`):\n"),
            "{report}"
        );
        assert!(
            report.contains(" M code.py\n")
                && report.contains("?? scratch.py\n")
                && report.contains(" M both.py\n"),
            "{report}"
        );
        assert!(
            report.contains("-b\n+B") && report.contains("+too"),
            "{report}"
        );
        assert!(
            report.contains("leave that as it is): both.py\n"),
            "{report}"
        );
        // What the user had lying about is neither listed nor in the diff.
        assert!(
            !report.contains("wip.py") && !report.contains("notes.txt"),
            "{report}"
        );
    }

    #[tokio::test]
    async fn without_git_the_edited_files_are_named() {
        let mut sess = test_session("changes-plain", Features::sage());
        sess.baseline = baseline(&sess.ws.root).await;
        assert!(sess.baseline.is_none(), "not a repository");
        assert_eq!(changes(&sess).await, None);
        sess.touched = vec!["a.py".into(), "b.py".into()];
        assert_eq!(
            changes(&sess).await.as_deref(),
            Some("Files you changed with edit or write: a.py, b.py")
        );
    }
}
