//! The `job` tool: look at, wait on, or stop a background command.

use super::jobs::kill_group;
use super::{StepOutput, StepStatus, bool_arg, str_arg, u64_arg};
use crate::state::{Session, shown};
use crate::text::{bound, clean, squeeze};
use crate::util::fmt_duration;
use serde_json::Value;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

const MAX_WAIT_SECS: u64 = 900;

enum Pattern {
    Regex(regex::Regex),
    Literal(String),
}

impl Pattern {
    fn new(needle: &str) -> Self {
        needle
            .strip_prefix('/')
            .and_then(|p| p.strip_suffix('/'))
            .filter(|p| !p.is_empty())
            .and_then(|p| regex::Regex::new(p).ok())
            .map_or_else(|| Self::Literal(needle.to_lowercase()), Self::Regex)
    }

    fn matches(&self, text: &str) -> bool {
        match self {
            Self::Regex(regex) => regex.is_match(text),
            Self::Literal(needle) => text.to_lowercase().contains(needle),
        }
    }
}

/// How much of the cleaned output already tried is tried again with what comes next, so that a
/// match that begins in it and ends in the new output is found. About fifty lines.
const OVERLAP: usize = 4 * 1024;
/// How much of an unfinished last line is tried (a prompt or a progress line with no newline).
const OPEN_LINE: usize = 16 * 1024;

/// A job's output as `until` follows it.
#[derive(Default)]
struct Follow {
    /// The spill file; without it, the output kept in memory stands in.
    file: Option<tokio::fs::File>,
    /// Bytes of the output taken in so far.
    read: u64,
    /// Bytes after the last complete line, waiting for the rest of it.
    open: Vec<u8>,
    /// The end of the cleaned complete lines already tried.
    tail: String,
    /// Some of the output was gone before it could be looked at.
    lossy: bool,
}

impl Follow {
    /// Takes in the output up to `total` and says whether `pattern` matches it. Complete lines are
    /// cleaned once each: cleaning works line by line, so the pieces join to what cleaning the
    /// whole would give.
    async fn feed(
        &mut self,
        features: &crate::state::Features,
        ws: &crate::workspace::Workspace,
        job: &super::jobs::Job,
        pattern: &Pattern,
        total: u64,
    ) -> bool {
        use tokio::io::AsyncReadExt;
        let mut new = Vec::new();
        if let Some(file) = &mut self.file {
            let want = total.saturating_sub(self.read);
            let _ = file.take(want).read_to_end(&mut new).await;
            // The collector writes the file before it announces a total, so a short read means the
            // file stopped at its size limit (or failed): what follows is only in memory.
            if (new.len() as u64) < want {
                self.file = None;
            }
        }
        self.read += new.len() as u64;
        if self.file.is_none() {
            let buf = job.run.buf.lock().unwrap();
            let more = buf.since(self.read);
            self.lossy |= (more.len() as u64) < buf.total.saturating_sub(self.read);
            new.extend(more);
            self.read = buf.total;
        }
        self.open.extend_from_slice(&new);
        let lines: Vec<u8> = match self.open.iter().rposition(|b| *b == b'\n') {
            Some(end) => self.open.drain(..=end).collect(),
            None => Vec::new(),
        };
        let mut window = std::mem::take(&mut self.tail);
        window.push_str(&shown(features, ws, clean(&lines)));
        let open = &self.open[self.open.len().saturating_sub(OPEN_LINE)..];
        let matched = if open.is_empty() {
            pattern.matches(&window)
        } else {
            pattern.matches(&format!("{window}{}", shown(features, ws, clean(open))))
        };
        let mut keep = window.len().saturating_sub(OVERLAP);
        while !window.is_char_boundary(keep) {
            keep += 1;
        }
        self.tail = window.split_off(keep);
        matched
    }
}

pub async fn run(sess: &mut Session, args: &Value, cancel: &CancellationToken) -> StepOutput {
    let known = {
        let ids: Vec<String> = sess.jobs.list.iter().map(|j| j.id.clone()).collect();
        if ids.is_empty() {
            "there are no background jobs".to_string()
        } else {
            format!("known jobs: {}", ids.join(", "))
        }
    };
    let Some(id) = str_arg(args, "id")
        .map(String::from)
        .or_else(|| u64_arg(args, "id").map(|n| n.to_string()))
    else {
        return StepOutput::error(format!("`id` is required ({known})"));
    };
    if sess.jobs.get_mut(&id).is_none() {
        return StepOutput::error(format!("no job {id:?} ({known})"));
    }
    let wait = u64_arg(args, "wait").unwrap_or(0).min(MAX_WAIT_SECS);
    let until = str_arg(args, "until")
        .filter(|u| !u.is_empty())
        .map(String::from);
    let wait = if until.is_some() && wait == 0 {
        60
    } else {
        wait
    };
    // Not past the run's time budget.
    let wait = sess.within_budget(Duration::from_secs(wait));
    let kill_wait = sess.within_budget(Duration::from_secs(3));
    let spill_path = sess
        .jobs
        .get_mut(&id)
        .map(|job| job.spill.clone())
        .and_then(|spill| sess.spill_path(&spill));

    let job = sess.jobs.get_mut(&id).expect("checked above");
    if bool_arg(args, "kill") && job.run.exit.borrow().is_none() {
        kill_group(job.run.pid);
        let mut rx = job.run.exit.clone();
        let _ = tokio::time::timeout(kill_wait, rx.wait_for(|e| e.is_some())).await;
    }

    let start_offset = job.read_offset;
    let deadline = Instant::now() + wait;
    let pattern = until.as_deref().map(Pattern::new);
    // The output is followed as it comes, from its start, and only when the collector announces
    // new bytes: each new piece is read and cleaned once, and the pattern is tried on it together
    // with the end of what came before (`Follow`), so the work grows with the output, not with
    // its square.
    let mut follow = Follow::default();
    if let Some(path) = &spill_path {
        follow.file = tokio::fs::File::open(path).await.ok();
    }
    let mut scanned = None;
    let mut matched = false;
    let mut partial = false;
    let mut output_open = pattern.is_some();
    loop {
        let exited = job.run.exit.borrow().is_some();
        let total = *job.run.output.borrow_and_update();
        if let Some(pattern) = &pattern
            && scanned != Some(total)
        {
            scanned = Some(total);
            matched = follow
                .feed(&sess.features, &sess.ws, job, pattern, total)
                .await;
            partial = follow.lossy;
        }
        if matched || (exited && !output_open) || Instant::now() >= deadline {
            break;
        }
        tokio::select! {
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => break,
            _ = cancel.cancelled() => break,
            updated = job.run.output.changed(), if output_open => {
                output_open = updated.is_ok();
            }
            updated = job.run.exit.changed(), if !exited => {
                if updated.is_err() { break; }
            }
        }
    }

    let exit = *job.run.exit.borrow();
    let (fresh, total) = {
        let buf = job.run.buf.lock().unwrap();
        (buf.since(start_offset), buf.total)
    };
    job.read_offset = total;
    let age = fmt_duration(job.run.started.elapsed().as_millis() as u64);
    let lines = squeeze(&shown(&sess.features, &sess.ws, clean(&fresh)));
    let view = bound(&lines, 10, 60, 6_000, Some(&job.spill));
    let state = match exit {
        Some(e) => match (e.code, e.signal) {
            (Some(c), _) => format!("exited {c}"),
            (None, Some(s)) => format!("killed by signal {s}"),
            _ => "ended".to_string(),
        },
        None => "still running".to_string(),
    };
    let waited_for = match (&until, matched) {
        (Some(p), true) => format!(" · output contains {p:?}"),
        (Some(p), false) if partial => format!(" · {p:?} was not found in the available output"),
        (Some(p), false) => format!(" · {p:?} has not appeared"),
        _ => String::new(),
    };
    let body = if lines.is_empty() {
        "(no new output)".to_string()
    } else {
        view.text.trim_end().to_string()
    };
    let head = format!("[job {}] {state} · {age}{waited_for}", job.id);
    let failed = exit.is_some_and(|e| e.code != Some(0)) && !bool_arg(args, "kill");
    let missed = until.is_some() && !matched;
    StepOutput {
        status: if failed || missed {
            StepStatus::Surprise
        } else {
            StepStatus::Ok
        },
        stub: head.clone(),
        observation: format!("{head}\n{body}"),
        ..Default::default()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::state::{Features, test_session};
    use crate::tools::shell;
    use serde_json::json;

    #[tokio::test]
    async fn waits_until_text_appears_then_reports_only_new_output() {
        let mut s = test_session("job", Features::sage());
        let cancel = CancellationToken::new();
        let started = shell::run(&mut s, &json!({"cmd": "echo booting; sleep 3; echo 'ready on :3000'; sleep 30", "background": true}), &cancel).await;
        assert!(started.observation.contains("booting"));

        let out = run(
            &mut s,
            &json!({"id": "bg:1", "until": "ready on", "wait": 20}),
            &cancel,
        )
        .await;
        assert_eq!(out.status, StepStatus::Ok, "{}", out.observation);
        assert!(
            out.observation.contains("still running")
                && out.observation.contains("output contains \"ready on\"")
        );
        assert!(out.observation.contains("ready on :3000"));
        assert!(
            !out.observation.contains("booting"),
            "already shown at start: {}",
            out.observation
        );

        let again = run(&mut s, &json!({"id": "1"}), &cancel).await;
        assert!(again.observation.contains("(no new output)"));

        let killed = run(&mut s, &json!({"id": "bg:1", "kill": true}), &cancel).await;
        assert_eq!(killed.status, StepStatus::Ok);
        assert!(
            killed.observation.contains("killed by signal")
                || killed.observation.contains("exited"),
            "{}",
            killed.observation
        );
    }

    #[tokio::test]
    async fn background_output_names_paths_the_way_other_output_does() {
        let mut s = test_session("job-short-paths", Features::sage());
        let cancel = CancellationToken::new();
        let root = s.ws.root.display().to_string();
        let started = shell::run(&mut s, &json!({"cmd": "echo \"serving $PWD/public\"; sleep 2; echo \"log: $PWD/server.log\"; sleep 30", "background": true}), &cancel).await;
        assert!(
            started.observation.ends_with("\nserving public"),
            "{}",
            started.observation
        );

        // The model waits for the text as it would be shown it.
        let out = run(
            &mut s,
            &json!({"id": "bg:1", "until": "log: server.log", "wait": 20}),
            &cancel,
        )
        .await;
        assert_eq!(out.status, StepStatus::Ok, "{}", out.observation);
        assert!(
            out.observation.ends_with("\nlog: server.log") && !out.observation.contains(&root),
            "{}",
            out.observation
        );
        s.jobs.kill_all();
    }

    #[tokio::test]
    async fn reports_failures_missing_text_and_unknown_ids() {
        let mut s = test_session("job-fail", Features::sage());
        let cancel = CancellationToken::new();
        shell::run(
            &mut s,
            &json!({"cmd": "sleep 2; echo crash >&2; exit 4", "background": true}),
            &cancel,
        )
        .await;
        let out = run(&mut s, &json!({"id": "bg:1", "wait": 20}), &cancel).await;
        assert_eq!(out.status, StepStatus::Surprise);
        assert!(out.observation.contains("exited 4") && out.observation.contains("crash"));

        let unknown = run(&mut s, &json!({"id": "bg:9"}), &cancel).await;
        assert_eq!(unknown.status, StepStatus::Error);
        assert!(unknown.observation.contains("known jobs: bg:1"));
    }

    #[tokio::test]
    async fn until_finds_text_across_pieces_on_an_open_line_and_long_after_the_start() {
        let mut s = test_session("job-follow", Features::sage());
        let cancel = CancellationToken::new();
        // A long run of output first, then the line waited for, written in two pieces.
        let cmd = "for i in $(seq 1 3000); do echo \"line $i of the warm-up\"; done; printf 'server re'; sleep 0.3; printf 'ady on :3000\\n'; sleep 30";
        shell::run(&mut s, &json!({"cmd": cmd, "background": true}), &cancel).await;
        let out = run(
            &mut s,
            &json!({"id": "bg:1", "until": "ready on :3000", "wait": 20}),
            &cancel,
        )
        .await;
        assert_eq!(out.status, StepStatus::Ok, "{}", out.observation);
        // A prompt with no newline after it is found too, and a regex as well.
        shell::run(
            &mut s,
            &json!({"cmd": "printf 'Password: '; sleep 30", "background": true}),
            &cancel,
        )
        .await;
        let out = run(
            &mut s,
            &json!({"id": "bg:2", "until": "/Pass\\w+:/", "wait": 20}),
            &cancel,
        )
        .await;
        assert_eq!(out.status, StepStatus::Ok, "{}", out.observation);
        s.jobs.kill_all();
    }
}
