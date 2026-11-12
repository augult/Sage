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

fn contains(haystack: &str, needle: &str) -> bool {
    if needle.len() > 2 && needle.starts_with('/') && needle.ends_with('/') && let Ok(re) = regex::Regex::new(&needle[1..needle.len() - 1]) {
        return re.is_match(haystack);
    }
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

pub async fn run(sess: &mut Session, args: &Value, cancel: &CancellationToken) -> StepOutput {
    let known = {
        let ids: Vec<String> = sess.jobs.list.iter().map(|j| j.id.clone()).collect();
        if ids.is_empty() { "there are no background jobs".to_string() } else { format!("known jobs: {}", ids.join(", ")) }
    };
    let Some(id) = str_arg(args, "id").map(String::from).or_else(|| u64_arg(args, "id").map(|n| n.to_string())) else {
        return StepOutput::error(format!("`id` is required ({known})"));
    };
    if sess.jobs.get_mut(&id).is_none() {
        return StepOutput::error(format!("no job {id:?} ({known})"));
    }
    let wait = u64_arg(args, "wait").unwrap_or(0).min(MAX_WAIT_SECS);
    let until = str_arg(args, "until").filter(|u| !u.is_empty()).map(String::from);
    let wait = if until.is_some() && wait == 0 { 60 } else { wait };

    let job = sess.jobs.get_mut(&id).expect("checked above");
    if bool_arg(args, "kill") && job.run.exit.borrow().is_none() {
        kill_group(job.run.pid);
        let mut rx = job.run.exit.clone();
        let _ = tokio::time::timeout(Duration::from_secs(3), rx.wait_for(|e| e.is_some())).await;
    }

    let start_offset = job.read_offset;
    let deadline = Instant::now() + Duration::from_secs(wait);
    let mut matched = false;
    loop {
        let exited = job.run.exit.borrow().is_some();
        if let Some(pattern) = &until {
            // Matched against everything so far: text that appeared before the model asked still counts.
            let so_far = shown(&sess.features, &sess.ws, clean(&job.run.buf.lock().unwrap().snapshot().0));
            if contains(&so_far, pattern) {
                matched = true;
                break;
            }
        }
        if exited || Instant::now() >= deadline {
            break;
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(150)) => {}
            _ = cancel.cancelled() => break,
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
        (Some(p), false) => format!(" · {p:?} has not appeared"),
        _ => String::new(),
    };
    let body = if lines.is_empty() { "(no new output)".to_string() } else { view.text.trim_end().to_string() };
    let head = format!("[job {}] {state} · {age}{waited_for}", job.id);
    let failed = exit.is_some_and(|e| e.code != Some(0)) && !bool_arg(args, "kill");
    let missed = until.is_some() && !matched;
    StepOutput {
        status: if failed || missed { StepStatus::Surprise } else { StepStatus::Ok },
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
        let mut s = test_session("job", Features::ecosage());
        let cancel = CancellationToken::new();
        let started = shell::run(&mut s, &json!({"cmd": "echo booting; sleep 3; echo 'ready on :3000'; sleep 30", "background": true}), &cancel).await;
        assert!(started.observation.contains("booting"));

        let out = run(&mut s, &json!({"id": "bg:1", "until": "ready on", "wait": 20}), &cancel).await;
        assert_eq!(out.status, StepStatus::Ok, "{}", out.observation);
        assert!(out.observation.contains("still running") && out.observation.contains("output contains \"ready on\""));
        assert!(out.observation.contains("ready on :3000"));
        assert!(!out.observation.contains("booting"), "already shown at start: {}", out.observation);

        let again = run(&mut s, &json!({"id": "1"}), &cancel).await;
        assert!(again.observation.contains("(no new output)"));

        let killed = run(&mut s, &json!({"id": "bg:1", "kill": true}), &cancel).await;
        assert_eq!(killed.status, StepStatus::Ok);
        assert!(killed.observation.contains("killed by signal") || killed.observation.contains("exited"), "{}", killed.observation);
    }

    #[tokio::test]
    async fn background_output_names_paths_the_way_other_output_does() {
        let mut s = test_session("job-short-paths", Features::ecosage());
        let cancel = CancellationToken::new();
        let root = s.ws.root.display().to_string();
        let started = shell::run(&mut s, &json!({"cmd": "echo \"serving $PWD/public\"; sleep 2; echo \"log: $PWD/server.log\"; sleep 30", "background": true}), &cancel).await;
        assert!(started.observation.ends_with("\nserving public"), "{}", started.observation);

        // The model waits for the text as it would be shown it.
        let out = run(&mut s, &json!({"id": "bg:1", "until": "log: server.log", "wait": 20}), &cancel).await;
        assert_eq!(out.status, StepStatus::Ok, "{}", out.observation);
        assert!(out.observation.ends_with("\nlog: server.log") && !out.observation.contains(&root), "{}", out.observation);
        s.jobs.kill_all();
    }

    #[tokio::test]
    async fn reports_failures_missing_text_and_unknown_ids() {
        let mut s = test_session("job-fail", Features::ecosage());
        let cancel = CancellationToken::new();
        shell::run(&mut s, &json!({"cmd": "sleep 2; echo crash >&2; exit 4", "background": true}), &cancel).await;
        let out = run(&mut s, &json!({"id": "bg:1", "wait": 20}), &cancel).await;
        assert_eq!(out.status, StepStatus::Surprise);
        assert!(out.observation.contains("exited 4") && out.observation.contains("crash"));

        let unknown = run(&mut s, &json!({"id": "bg:9"}), &cancel).await;
        assert_eq!(unknown.status, StepStatus::Error);
        assert!(unknown.observation.contains("known jobs: bg:1"));
    }
}
