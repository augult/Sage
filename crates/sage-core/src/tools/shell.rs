//! The `shell` tool, and the one place where "how much output does the model get" is decided.
//!
//! Output is sized by surprise. A prediction that held is answered with a receipt (the model already
//! knows what happened). An unpredicted success gets a bounded view. A failure gets the most: the
//! tail-biased output and the source locations it names with edit anchors.

use super::jobs::{
    Exit, Running, StateMode, Waited, drain, kill_group, masked_failure, spawn, wait_exit,
};
use super::{Completed, ShellMeta, StepOutput, StepStatus, bool_arg, str_arg, u64_arg};
use crate::guard::{self, Expect};
use crate::locate;
use crate::state::Session;
use crate::text::{bound, clean, squeeze, tail};
use crate::util::{fmt_duration, one_line};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub enum ExecEnd {
    Exited(Exit),
    /// Still running when the timeout expired; the caller decides whether to keep or kill it.
    TimedOut(Running),
    Cancelled,
    Budget,
}

pub struct Exec {
    pub end: ExecEnd,
    /// Cleaned output (escape codes and redraws removed), as the model is shown it (`Session::shown`).
    pub text: String,
    pub total_bytes: u64,
    pub output_complete: bool,
    drained: bool,
    pub ms: u64,
    pub spill: String,
    /// The reported exit code comes from an earlier stage of a pipeline whose last stage exited 0.
    pub masked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateKind {
    /// Persist `cd` and exports back into the session.
    Session,
    /// See the session's environment, change nothing.
    ReadOnly,
}

/// Runs a command to completion or timeout and captures its cleaned output.
pub async fn exec(
    sess: &mut Session,
    cmd: &str,
    cwd: &Path,
    state: StateKind,
    timeout: Duration,
    cancel: &CancellationToken,
) -> std::io::Result<Exec> {
    let allowed = sess.within_budget(timeout);
    let budget_cut = allowed < timeout;
    if allowed.is_zero() {
        return Ok(Exec {
            end: ExecEnd::Budget,
            text: String::new(),
            total_bytes: 0,
            output_complete: false,
            drained: false,
            ms: 0,
            spill: String::new(),
            masked: false,
        });
    }
    let (spill, spill_path) = sess.next_spill();
    let state_path = sess.shell_state();
    let mode = match state {
        StateKind::Session => StateMode::ReadWrite(&state_path),
        StateKind::ReadOnly => StateMode::ReadOnly(&state_path),
    };
    let mut run = spawn(cmd, cwd, &sess.shell_env, mode, &spill_path)?;
    let waited = wait_exit(&mut run.exit, sess.within_budget(allowed), cancel).await;
    let ms = run.started.elapsed().as_millis() as u64;
    let mut masked = false;
    let mut drained = false;
    let end = match waited {
        Waited::Exited(mut exit) => {
            drained = drain(&mut run.readers).await;
            if state == StateKind::Session {
                adopt_cwd(sess);
            }
            // `tests | tail` exits 0 even when the tests failed. Report the failure, not the tail.
            if let (Some(0), Some(code)) = (exit.code, masked_failure(&spill_path, cmd)) {
                exit.code = Some(code);
                masked = true;
            }
            ExecEnd::Exited(exit)
        }
        Waited::Cancelled => {
            kill_group(run.pid);
            ExecEnd::Cancelled
        }
        Waited::TimedOut if budget_cut || sess.within_budget(timeout).is_zero() => {
            kill_group(run.pid);
            ExecEnd::Budget
        }
        Waited::TimedOut => {
            super::jobs::detach(&spill_path);
            let (raw, _) = run.buf.lock().unwrap().snapshot();
            let total_bytes = run.buf.lock().unwrap().total;
            return Ok(Exec {
                end: ExecEnd::TimedOut(run),
                text: sess.shown(clean(&raw)),
                total_bytes,
                output_complete: false,
                drained: false,
                ms,
                spill,
                masked: false,
            });
        }
    };
    let buf = run.buf.lock().unwrap();
    let (raw, truncated) = buf.snapshot();
    let total_bytes = buf.total;
    // After a `cd` has been followed, so that paths come out relative to where the next call runs.
    Ok(Exec {
        end,
        text: sess.shown(clean(&raw)),
        total_bytes,
        output_complete: drained && !truncated,
        drained,
        ms,
        spill,
        masked,
    })
}

impl Exec {
    /// Recover a complete capture for predicates without expanding the model's excerpt.
    pub async fn predicate_text(&self, sess: &Session, needed: bool) -> Option<String> {
        const MAX_PREDICATE_BYTES: u64 = 16 * 1024 * 1024;
        if !needed
            || self.output_complete
            || !self.drained
            || self.total_bytes > MAX_PREDICATE_BYTES
        {
            return None;
        }
        let path = sess.spill_path(&self.spill)?;
        let ws = sess.ws.clone();
        let shorten = sess.features.short_paths;
        let total = self.total_bytes;
        tokio::task::spawn_blocking(move || {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::fs::File::open(path)
                .ok()?
                .take(total + 1)
                .read_to_end(&mut bytes)
                .ok()?;
            if bytes.len() as u64 != total {
                return None;
            }
            let text = clean(&bytes);
            Some(if shorten {
                ws.shorten(&text).into_owned()
            } else {
                text
            })
        })
        .await
        .ok()
        .flatten()
    }
}

/// Follows a `cd` the command performed, if it left the shell in a directory that still exists.
fn adopt_cwd(sess: &mut Session) {
    let recorded = sess.shell_state().with_extension("cwd");
    if let Ok(text) = std::fs::read_to_string(&recorded) {
        let dir = std::path::PathBuf::from(text.trim());
        if dir.is_absolute() && dir.is_dir() && dir != sess.ws.cwd {
            sess.ws.cwd = dir;
        }
    }
}

/// `cmd` cut at `;`, `&` and `|` outside quotes: the commands of a line, more or less.
fn commands(cmd: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut quote: Option<char> = None;
    for c in cmd.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            None if c == '\'' || c == '"' => quote = Some(c),
            None if matches!(c, ';' | '&' | '|' | '\n') => {
                parts.push(String::new());
                continue;
            }
            _ => {}
        }
        parts.last_mut().unwrap().push(c);
    }
    parts
}

/// What exit 1 says of `cmd` where the whole of it is one program that answers with it: no match
/// (`grep`, `rg`), files that differ (`diff`, `cmp`), a condition that is false (`test`). A leading
/// `cd <dir> &&` is allowed. `None` for anything else, whose exit 1 is a failure.
fn answer_of_one(cmd: &str) -> Option<&'static str> {
    let cmd = cmd.trim();
    let cmd = match cmd
        .strip_prefix("cd ")
        .and_then(|rest| rest.split_once("&&"))
    {
        Some((dir, rest)) if commands(dir).len() == 1 => rest,
        _ => cmd,
    };
    let parts = commands(cmd);
    let [only] = parts.as_slice() else {
        return None;
    };
    let word = only.split_whitespace().find(|word| !word.contains('='))?;
    let program = word
        .strip_prefix("/usr/bin/")
        .or_else(|| word.strip_prefix("/bin/"))
        .unwrap_or(word);
    match program {
        "grep" | "egrep" | "fgrep" | "rg" | "ag" | "ack" => Some("no match"),
        "diff" | "cmp" => Some("they differ"),
        // The shell's own `test`, not a project's script of that name (`./test`), whose 1 is a failure.
        "test" | "[" | "[[" if word == "test" || word.starts_with('[') => Some("false"),
        _ => None,
    }
}

/// Commands whose damage cannot be undone from inside the workspace. A tripwire, not a sandbox.
pub fn danger(cmd: &str) -> Option<&'static str> {
    let c = cmd.to_ascii_lowercase();
    let c = c.split_whitespace().collect::<Vec<_>>().join(" ");
    // The target is the root or the home directory itself, not something below it.
    let whole = |prefix: &str| {
        c.match_indices(prefix).any(|(i, _)| {
            let rest = &c[i + prefix.len()..];
            rest.is_empty()
                || rest.starts_with([' ', ';', '&', '|', '*'])
                || prefix.ends_with("root")
        })
    };
    let rm_root = [
        "rm -rf /",
        "rm -fr /",
        "rm -rf ~",
        "rm -fr ~",
        "rm -rf $home",
        "rm -rf /*",
        "rm -rf --no-preserve-root",
    ];
    if rm_root.iter().any(|p| whole(p)) {
        return Some("recursive delete of the filesystem root or home directory");
    }
    if c.contains("mkfs")
        || c.contains("dd of=/dev/")
        || c.contains("> /dev/sd")
        || c.contains("> /dev/nvme")
    {
        return Some("writes directly to a disk device");
    }
    if c.contains(":(){") || c.contains(":() {") {
        return Some("fork bomb");
    }
    // The command word, not the same word inside a quoted pattern: `grep 'startup|shutdown'`.
    if commands(&c).iter().any(|part| {
        matches!(
            part.trim().split(' ').next(),
            Some("shutdown" | "reboot" | "halt" | "poweroff")
        )
    }) {
        return Some("shuts the machine down");
    }
    if whole("chmod -r 777 /") {
        return Some("opens up permissions on the whole filesystem");
    }
    None
}

fn exit_text(exit: Exit, masked: bool) -> String {
    match (exit.code, exit.signal) {
        (Some(c), _) if masked => {
            format!("exit {c} (from an earlier command of the pipeline; the last one exited 0)")
        }
        (Some(c), _) => format!("exit {c}"),
        (None, Some(s)) => format!("killed by signal {s}"),
        _ => "no exit status".to_string(),
    }
}

pub async fn run(sess: &mut Session, args: &Value, cancel: &CancellationToken) -> StepOutput {
    let Some(cmd) = str_arg(args, "cmd")
        .map(str::trim)
        .filter(|c| !c.is_empty())
    else {
        return StepOutput::error("`cmd` is required: the bash command to run");
    };
    let cmd = cmd.to_string();
    let expect = if sess.features.receipts {
        Expect::from_args(args)
    } else {
        None
    };
    // More seconds than a command is ever given is a number of milliseconds, as other harnesses take it.
    let timeout = u64_arg(args, "timeout").map(|t| {
        if t > sess.limits.max_timeout_secs {
            t.div_ceil(1000)
        } else {
            t
        }
    });
    let timeout = timeout
        .unwrap_or(sess.limits.default_timeout_secs)
        .clamp(1, sess.limits.max_timeout_secs);
    let cwd = sess.ws.cwd.clone();

    if bool_arg(args, "background") {
        return start_background(sess, &cmd, &cwd, cancel).await;
    }

    sess.evidence.before_command(&sess.ws, sess.deadline).await;
    let ex = match exec(
        sess,
        &cmd,
        &cwd,
        StateKind::Session,
        Duration::from_secs(timeout),
        cancel,
    )
    .await
    {
        Ok(ex) => ex,
        Err(e) => return StepOutput::error(format!("could not start the shell: {e}")),
    };
    if matches!(ex.end, ExecEnd::Budget) {
        sess.evidence
            .changed("a command stopped at the time budget");
    } else {
        sess.evidence
            .after_command(&sess.ws, &cmd, sess.deadline)
            .await;
    }
    let lines = squeeze(&ex.text);
    let shown_all = ex.text.len() as u64;

    let needs_output = expect.as_ref().is_some_and(Expect::has_output_predicates)
        || sess.watches.items.iter().any(|watch| {
            watch
                .expect
                .as_ref()
                .is_some_and(Expect::has_output_predicates)
        });
    let predicate_text = ex.predicate_text(sess, needs_output).await;
    match ex.end {
        ExecEnd::Budget => StepOutput::error("the run's time budget expired"),
        ExecEnd::Cancelled => StepOutput {
            status: StepStatus::Error,
            observation: "[cancelled] stopped by the user".into(),
            stub: "[cancelled]".into(),
            ..Default::default()
        },
        ExecEnd::TimedOut(running) => {
            let id = sess.jobs.add(&ex.spill, running, ex.total_bytes);
            let view = bound(&lines, 5, 25, 3_000, Some(&ex.spill));
            let observation = format!(
                "[running] no exit after {timeout}s, so it now runs in the background as {id}. Use job {{\"id\":\"{id}\",\"wait\":…}} to wait for it or {{\"id\":\"{id}\",\"kill\":true}} to stop it. Output so far:\n{}",
                if lines.is_empty() {
                    "(none)\n".to_string()
                } else {
                    view.text
                }
            );
            StepOutput {
                status: StepStatus::Running,
                stub: format!("[running] moved to background as {id}"),
                observation,
                violations: vec![format!("did not finish within {timeout}s")],
                explicit: expect.is_some(),
                shell: Some(ShellMeta {
                    cmd,
                    ok: false,
                    held_explicit: false,
                    completed: None,
                }),
                ..Default::default()
            }
        }
        ExecEnd::Exited(exit) => {
            let verdict = guard::check_capture(
                expect.as_ref(),
                exit.code,
                predicate_text.as_deref().unwrap_or(&ex.text),
                ex.output_complete || predicate_text.is_some(),
            );
            let ok = exit.code == Some(0);
            let dur = fmt_duration(ex.ms);
            // Unpredicted, `grep` that finds nothing or `diff` of files that differ has answered,
            // not failed: what follows it in the reply still runs.
            let answer = (sess.features.receipts && !verdict.explicit && exit.code == Some(1))
                .then(|| answer_of_one(&cmd))
                .flatten();
            let held = verdict.held || answer.is_some();
            let mut out = StepOutput {
                explicit: verdict.explicit,
                violations: if answer.is_some() {
                    Vec::new()
                } else {
                    verdict.violations.clone()
                },
                shell: Some(ShellMeta {
                    cmd: cmd.clone(),
                    ok,
                    held_explicit: verdict.explicit && verdict.held,
                    completed: None,
                }),
                ..Default::default()
            };

            if !sess.features.receipts {
                // Conventional rendering: status line plus a bounded view, success or not.
                let view = bound(
                    &lines,
                    sess.limits.ok_head,
                    sess.limits.ok_tail,
                    sess.limits.ok_chars,
                    Some(&ex.spill),
                );
                out.status = if ok {
                    StepStatus::Ok
                } else {
                    StepStatus::Error
                };
                out.observation = format!(
                    "{} · {dur}\n{}",
                    exit_text(exit, ex.masked),
                    if lines.is_empty() {
                        "(no output)\n".into()
                    } else {
                        view.text
                    }
                );
                out.stub = format!("{} ({})", exit_text(exit, ex.masked), ex.spill);
                out.hidden_bytes = shown_all.saturating_sub(out.observation.len() as u64);
                return out;
            }

            if held {
                let head = format!(
                    "[ok] {}{} · {dur}",
                    exit_text(exit, ex.masked),
                    answer.map(|a| format!(" ({a})")).unwrap_or_default()
                );
                // Hiding a short output saves little and costs a turn if the model wanted it after all.
                let small = lines.len() <= 30 && ex.text.len() <= 2_000;
                out.observation = if lines.is_empty() {
                    format!("{head} · no output")
                } else if verdict.explicit && !small && ok {
                    // Only a predicted *success* is reduced to a receipt. A predicted failure (the
                    // model reproducing a bug) is something it still has to read.
                    let last = tail(&lines, 5)
                        .iter()
                        .map(|l| one_line(l, 200))
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!(
                        "{head} · {} lines not shown, your prediction held (read path \"{}\" shows them)\n…\n{last}",
                        lines.len(),
                        ex.spill
                    )
                } else {
                    let view = bound(
                        &lines,
                        sess.limits.ok_head,
                        sess.limits.ok_tail,
                        sess.limits.ok_chars,
                        Some(&ex.spill),
                    );
                    format!("{head}\n{}", view.text.trim_end())
                };
                out.stub = format!("{head} ({})", ex.spill);
            } else {
                let head = if verdict.explicit {
                    format!("[SURPRISE] {} · {dur}", verdict.violations.join("; "))
                } else {
                    format!("[FAILED] {} · {dur}", exit_text(exit, ex.masked))
                };
                let view = bound(
                    &lines,
                    sess.limits.fail_head,
                    sess.limits.fail_tail,
                    sess.limits.fail_chars,
                    Some(&ex.spill),
                );
                let mut obs = format!(
                    "{head}\n{}",
                    if lines.is_empty() {
                        "(no output)".into()
                    } else {
                        view.text.trim_end().to_string()
                    }
                );
                if sess.features.expand {
                    let budget = sess
                        .prices
                        .map(|p| p.speculation_budget(0.5, 1500.0) as usize * 4)
                        .unwrap_or(usize::MAX)
                        .min(sess.limits.expand_chars);
                    let step = sess.step;
                    let block = locate::expand(
                        &ex.text,
                        &sess.ws,
                        &mut sess.seen,
                        step,
                        sess.limits.expand_radius,
                        sess.limits.expand_windows,
                        budget,
                    );
                    if !block.is_empty() {
                        obs.push_str(
                            "\n--- where (current file content; anchors work in edit) ---\n",
                        );
                        obs.push_str(block.trim_end());
                    }
                }
                out.status = StepStatus::Surprise;
                out.stub = format!("{head} ({})", ex.spill);
                out.observation = obs;
            }
            out.hidden_bytes = shown_all.saturating_sub(out.observation.len() as u64);
            if let (true, Some(meta)) = (sess.features.watches, out.shell.as_mut()) {
                meta.completed = Some(Completed {
                    cwd,
                    code: exit.code,
                    ms: ex.ms,
                    text: ex.text,
                    output_complete: ex.output_complete,
                    predicate_text,
                });
            }
            out
        }
    }
}

async fn start_background(
    sess: &mut Session,
    cmd: &str,
    cwd: &Path,
    cancel: &CancellationToken,
) -> StepOutput {
    let (spill, spill_path) = sess.next_spill();
    let state_path = sess.shell_state();
    let mut running = match spawn(
        cmd,
        cwd,
        &sess.shell_env,
        StateMode::ReadOnly(&state_path),
        &spill_path,
    ) {
        Ok(r) => r,
        Err(e) => return StepOutput::error(format!("could not start the shell: {e}")),
    };
    // Give it a moment: a server that dies on startup should be reported now, not three turns later.
    let early = wait_exit(
        &mut running.exit,
        sess.within_budget(Duration::from_millis(100)),
        cancel,
    )
    .await;
    if matches!(early, Waited::Exited(_)) {
        drain(&mut running.readers).await;
    }
    let (raw, _) = running.buf.lock().unwrap().snapshot();
    let total = running.buf.lock().unwrap().total;
    let lines = squeeze(&sess.shown(clean(&raw)));
    let view = bound(&lines, 10, 20, 2_500, Some(&spill));
    let shown = if lines.is_empty() {
        "(no output yet)".to_string()
    } else {
        view.text.trim_end().to_string()
    };
    match early {
        Waited::Exited(exit) => {
            let ok = exit.code == Some(0);
            let observation = format!(
                "[{}] the background command ended immediately: {}\n{shown}",
                if ok { "ok" } else { "FAILED" },
                exit_text(exit, false)
            );
            StepOutput {
                status: if ok {
                    StepStatus::Ok
                } else {
                    StepStatus::Surprise
                },
                stub: one_line(&observation, 160),
                observation,
                shell: Some(ShellMeta {
                    cmd: cmd.to_string(),
                    ok,
                    held_explicit: false,
                    completed: None,
                }),
                ..Default::default()
            }
        }
        Waited::Cancelled => {
            kill_group(running.pid);
            StepOutput {
                status: StepStatus::Error,
                observation: "[cancelled] stopped by the user".into(),
                stub: "[cancelled]".into(),
                ..Default::default()
            }
        }
        Waited::TimedOut => {
            let pid = running
                .pid
                .map(|p| format!(" (pid {p})"))
                .unwrap_or_default();
            let id = sess.jobs.add(&spill, running, total);
            let observation = format!(
                "[started] {id}{pid} keeps running in the background. Use job {{\"id\":\"{id}\",\"until\":\"text\"}} to wait for output, {{\"id\":\"{id}\",\"kill\":true}} to stop it.\n{shown}"
            );
            StepOutput {
                stub: format!("[started] {id}"),
                observation,
                ..Default::default()
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::state::{Features, test_session};
    use serde_json::json;

    async fn sh(sess: &mut Session, args: Value) -> StepOutput {
        run(sess, &args, &CancellationToken::new()).await
    }

    #[tokio::test]
    async fn held_prediction_yields_a_receipt_not_the_output() {
        let mut s = test_session("receipt", Features::sage());
        let out = sh(&mut s, json!({"cmd": "for i in $(seq 1 200); do echo \"test_$i passed\"; done; echo '200 passed in 1.2s'", "expect": {"exit": "0", "has": "passed"}})).await;
        assert_eq!(out.status, StepStatus::Ok);
        assert!(out.explicit);
        assert!(
            out.observation.starts_with("[ok] exit 0 · "),
            "{}",
            out.observation
        );
        assert!(out.observation.contains(
            "201 lines not shown, your prediction held (read path \"out:1\" shows them)"
        ));
        assert!(out.observation.ends_with("200 passed in 1.2s"));
        assert!(
            out.observation.len() < 300,
            "a receipt, not the log: {}",
            out.observation.len()
        );
        assert!(out.hidden_bytes > 2_000);
        assert!(out.shell.as_ref().unwrap().held_explicit);
        // The full output is still one read away.
        let spilled = std::fs::read_to_string(s.spill_path("out:1").unwrap()).unwrap();
        assert_eq!(spilled.lines().count(), 201);
    }

    #[tokio::test]
    async fn unpredicted_success_shows_bounded_output_and_small_output_is_never_hidden() {
        let mut s = test_session("implicit", Features::sage());
        let out = sh(&mut s, json!({"cmd": "seq 1 500"})).await;
        assert_eq!(out.status, StepStatus::Ok);
        assert!(!out.explicit);
        assert!(
            out.observation.contains("\n1\n2\n")
                && out
                    .observation
                    .contains("lines hidden · read path \"out:1\"")
        );
        assert!(out.observation.trim_end().ends_with("500"));

        let small = sh(
            &mut s,
            json!({"cmd": "echo hello", "expect": {"has": "hello"}}),
        )
        .await;
        assert_eq!(small.observation.lines().nth(1), Some("hello"));
    }

    #[tokio::test]
    async fn violated_prediction_is_a_surprise_with_detail_and_code_context() {
        let mut s = test_session("surprise", Features::sage());
        let body: String = (1..=30).map(|i| format!("line_{i} = {i}\n")).collect();
        std::fs::write(s.ws.root.join("calc.py"), body).unwrap();
        let out = sh(&mut s, json!({"cmd": "echo 'Traceback (most recent call last):'; echo '  File \"calc.py\", line 12, in f'; echo 'ZeroDivisionError'; exit 1", "expect": {"exit": "0"}})).await;
        assert_eq!(out.status, StepStatus::Surprise);
        assert!(
            out.observation
                .starts_with("[SURPRISE] expected exit 0, got exit 1 · "),
            "{}",
            out.observation
        );
        assert!(out.observation.contains("ZeroDivisionError"));
        assert!(
            out.observation
                .contains("--- where (current file content; anchors work in edit) ---\ncalc.py\n")
        );
        assert!(out.observation.contains(&format!(
            "> {}|line_12 = 12",
            crate::anchors::anchor(12, "line_12 = 12")
        )));
        assert_eq!(out.violations, vec!["expected exit 0, got exit 1"]);
        assert!(!out.shell.unwrap().ok);
    }

    #[tokio::test]
    async fn unpredicted_failure_is_marked_failed_and_expected_failure_is_ok() {
        let mut s = test_session("failed", Features::sage());
        let out = sh(&mut s, json!({"cmd": "echo boom >&2; exit 2"})).await;
        assert_eq!(out.status, StepStatus::Surprise);
        assert!(out.observation.starts_with("[FAILED] exit 2 · "));

        // Reproducing a bug: the model says the test should fail, and it does.
        let out = sh(&mut s, json!({"cmd": "echo 'AssertionError'; exit 1", "expect": {"exit": "nonzero", "has": "AssertionError"}})).await;
        assert_eq!(out.status, StepStatus::Ok);
        assert!(out.observation.starts_with("[ok] exit 1 · "));
        assert!(
            !out.shell.unwrap().ok,
            "exit 1 is not a passing check even though the prediction held"
        );
    }

    #[tokio::test]
    async fn a_search_that_finds_nothing_has_answered_not_failed() {
        let mut s = test_session("answers", Features::sage());
        std::fs::write(s.ws.root.join("a.py"), "x = 1\n").unwrap();
        std::fs::write(s.ws.root.join("b.py"), "x = 2\n").unwrap();
        for (cmd, word) in [
            ("grep -n old_name a.py", "no match"),
            ("cd . && grep -q old_name a.py", "no match"),
            ("diff a.py b.py", "they differ"),
            ("test -f missing.txt", "false"),
        ] {
            let out = sh(&mut s, json!({ "cmd": cmd })).await;
            assert_eq!(out.status, StepStatus::Ok, "{cmd}");
            assert!(
                out.observation
                    .starts_with(&format!("[ok] exit 1 ({word}) · ")),
                "{cmd}: {}",
                out.observation
            );
            assert!(out.violations.is_empty() && !out.shell.unwrap().ok, "{cmd}");
        }
        // Anything else that exits non-zero has failed: an error of grep itself (2), a line of
        // several commands, or a project's own script named `test`.
        std::fs::write(s.ws.root.join("test"), "exit 1\n").unwrap();
        for cmd in [
            "grep -n x no-such-file",
            "grep -c old_name a.py; exit 1",
            "grep -q x a.py && grep -q old_name a.py",
            "sh ./test",
            "false",
        ] {
            let out = sh(&mut s, json!({ "cmd": cmd })).await;
            assert_eq!(out.status, StepStatus::Surprise, "{cmd}");
        }
        assert_eq!(answer_of_one("./test"), None);
        // A prediction is checked as it was made.
        let out = sh(
            &mut s,
            json!({"cmd": "grep -n old_name a.py", "expect_exit": "0"}),
        )
        .await;
        assert_eq!(out.status, StepStatus::Surprise);
    }

    #[tokio::test]
    async fn a_test_run_piped_through_tail_cannot_pass_as_success() {
        let mut s = test_session("masked", Features::sage());
        let out = sh(
            &mut s,
            json!({"cmd": "sh -c 'echo FAILED 2 tests; exit 1' | tail -5", "expect_exit": "0"}),
        )
        .await;
        assert_eq!(out.status, StepStatus::Surprise);
        assert!(
            out.observation.contains("expected exit 0, got exit 1"),
            "{}",
            out.observation
        );
        assert!(!out.shell.unwrap().held_explicit);

        // The same shape, predicted correctly: the model said it would fail.
        let out = sh(&mut s, json!({"cmd": "sh -c 'echo FAILED; exit 1' | tail -5", "expect_exit": "nonzero", "expect_has": "FAILED"})).await;
        assert_eq!(out.status, StepStatus::Ok);
        assert!(
            out.observation.starts_with(
                "[ok] exit 1 (from an earlier command of the pipeline; the last one exited 0)"
            ),
            "{}",
            out.observation
        );
    }

    #[tokio::test]
    async fn cwd_and_exports_persist_between_calls() {
        let mut s = test_session("cwd", Features::sage());
        std::fs::create_dir_all(s.ws.root.join("pkg")).unwrap();
        sh(&mut s, json!({"cmd": "cd pkg && export STAGE=two"})).await;
        assert!(s.ws.cwd.ends_with("pkg"));
        let out = sh(
            &mut s,
            json!({"cmd": "echo \"$STAGE in $(basename \"$PWD\")\""}),
        )
        .await;
        assert!(
            out.observation.contains("two in pkg"),
            "{}",
            out.observation
        );
    }

    #[tokio::test]
    async fn a_command_handed_over_as_a_job_does_not_take_the_shell_state_back_with_it() {
        let mut s = test_session("detached", Features::sage());
        let out = sh(&mut s, json!({"cmd": "sleep 2", "timeout": 1})).await;
        assert_eq!(out.status, StepStatus::Running);
        sh(&mut s, json!({"cmd": "export STAGE=later"})).await;
        // The job ends now, with the environment it started in.
        tokio::time::sleep(Duration::from_millis(1800)).await;
        let out = sh(&mut s, json!({"cmd": "echo \"stage=[$STAGE]\""})).await;
        assert!(
            out.observation.contains("stage=[later]"),
            "{}",
            out.observation
        );

        // A syntax error is reported on the line the model wrote it on.
        let out = sh(&mut s, json!({"cmd": "echo one\necho two\nif then"})).await;
        assert!(
            out.observation.contains("line 3: syntax error"),
            "{}",
            out.observation
        );
        s.jobs.kill_all();
    }

    #[tokio::test]
    async fn timeout_hands_the_command_over_as_a_job_instead_of_killing_it() {
        let mut s = test_session("timeout", Features::sage());
        let out = sh(
            &mut s,
            json!({"cmd": "echo warming up; sleep 20; echo done", "timeout": 1}),
        )
        .await;
        assert_eq!(out.status, StepStatus::Running);
        assert!(out.observation.contains("runs in the background as bg:1"));
        assert!(out.observation.contains("warming up"));
        assert_eq!(s.jobs.running().len(), 1);
        s.jobs.kill_all();
    }

    #[tokio::test]
    async fn a_foreground_command_is_killed_when_the_run_budget_expires() {
        let mut s = test_session("shell-deadline", Features::sage());
        s.deadline = Some(std::time::Instant::now() + Duration::from_millis(100));
        let result = sh(&mut s, json!({"cmd": "sleep 0.4; echo late > late.txt"})).await;
        assert_eq!(result.status, StepStatus::Error);
        assert!(result.observation.contains("time budget"));
        tokio::time::sleep(Duration::from_millis(450)).await;
        assert!(!s.ws.root.join("late.txt").exists());
    }

    #[tokio::test]
    async fn an_output_predicate_sees_the_middle_of_the_capture() {
        let mut s = test_session("shell-complete-output", Features::sage());
        let result = sh(
            &mut s,
            json!({
                "cmd": "yes row | head -n 85000; echo MIDDLE_MARKER; yes row | head -n 85000",
                "expect_exit": 0,
                "expect_lacks": "MIDDLE_MARKER"
            }),
        )
        .await;
        assert_eq!(result.status, StepStatus::Surprise);
        assert!(result.observation.contains("MIDDLE_MARKER"));
    }

    #[tokio::test]
    async fn background_start_reports_immediate_crashes() {
        let mut s = test_session("bg", Features::sage());
        let out = sh(
            &mut s,
            json!({"cmd": "echo 'port in use' >&2; exit 1", "background": true}),
        )
        .await;
        assert_eq!(out.status, StepStatus::Surprise);
        assert!(
            out.observation.contains("ended immediately: exit 1")
                && out.observation.contains("port in use")
        );

        let out = sh(
            &mut s,
            json!({"cmd": "echo listening; sleep 20", "background": true}),
        )
        .await;
        assert_eq!(out.status, StepStatus::Ok);
        assert!(out.observation.starts_with("[started] bg:1"));
        assert!(out.observation.contains("listening"));
        s.jobs.kill_all();
    }

    #[tokio::test]
    async fn classic_mode_always_shows_output_and_ignores_expect() {
        let mut s = test_session("classic", Features::classic());
        let out = sh(
            &mut s,
            json!({"cmd": "seq 1 30; exit 1", "expect": {"exit": "0"}}),
        )
        .await;
        assert_eq!(out.status, StepStatus::Error);
        assert!(!out.explicit);
        assert!(out.observation.starts_with("exit 1 · "));
        assert!(out.observation.contains("\n30\n"));
    }

    #[test]
    fn danger_tripwire_catches_the_catastrophic_and_spares_the_ordinary() {
        assert!(danger("rm -rf /").is_some());
        assert!(danger("sudo rm -rf / --no-preserve-root").is_some());
        assert!(danger("cd /tmp && rm -rf ~").is_some());
        assert!(danger("mkfs.ext4 /dev/sda1").is_some());
        assert!(danger("echo hi; shutdown -h now").is_some());
        assert!(danger("rm -rf /tmp/build").is_none());
        assert!(danger("rm -rf ./node_modules").is_none());
        assert!(danger("rm -rf ~/project/dist").is_none());
        assert!(danger("git push --force").is_none());
        // A word in a quoted pattern is not a command, and a directory below the root is not the root.
        assert!(danger("grep -E 'startup|shutdown|reload' log.txt").is_none());
        assert!(
            danger("chmod -R 777 /workspace/out").is_none() && danger("chmod -R 777 /").is_some()
        );
    }
}
