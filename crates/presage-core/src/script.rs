//! A reply is a script.
//!
//! All tool calls of one model reply run in order. The first step that does not go as expected halts
//! the rest, exactly like `set -e`: later steps were written on the assumption that it would work.
//! This lets the model commit to a whole stretch of work (edit, check, finish) in a single round of
//! thinking, safely, because a wrong assumption stops the script instead of compounding.

use crate::events::{EventKind, Sink, Stats};
use crate::guard::Expect;
use crate::repair::Call;
use crate::review;
use crate::state::Session;
use crate::tools::{self, StepOutput, StepStatus, Tool, bool_arg, str_arg};
use crate::util::{fnv64, one_line};
use crate::watches::{self, WatchChange, WatchReport, WatchState};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Autonomy {
    /// Run everything; only catastrophic commands need a human.
    Auto,
    /// Every step that changes something waits for approval.
    Ask,
}

/// Pending approval requests, answered from the UI.
#[derive(Default)]
pub struct Approvals {
    next: AtomicU64,
    pending: Mutex<HashMap<u64, oneshot::Sender<bool>>>,
}

impl Approvals {
    pub fn request(&self) -> (u64, oneshot::Receiver<bool>) {
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        (id, rx)
    }

    pub fn resolve(&self, id: u64, allow: bool) -> bool {
        match self.pending.lock().unwrap().remove(&id) {
            Some(tx) => tx.send(allow).is_ok(),
            None => false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Finished {
    pub summary: String,
    pub verified: bool,
    pub evidence: String,
}

#[derive(Default)]
pub struct ScriptResult {
    /// (call id, text for the model, stub to decay to), in call order.
    pub observations: Vec<(String, String, String)>,
    /// Harness remarks to attach after the last observation.
    pub footer: Vec<String>,
    pub surprised: bool,
    pub finished: Option<Finished>,
    /// Identity of each step that failed, for stall detection.
    pub failed_steps: Vec<u64>,
    pub edited: Vec<String>,
}

pub struct Runner<'a> {
    pub sess: &'a mut Session,
    pub sink: &'a dyn Sink,
    pub cancel: &'a CancellationToken,
    pub approvals: &'a Approvals,
    pub stats: &'a mut Stats,
    pub turn: u32,
    pub autonomy: Autonomy,
    /// Someone is present to answer approval requests.
    pub interactive: bool,
    /// The budget ends with this reply: `finish` is taken as it stands, since nothing can follow it.
    pub landing: bool,
}

fn halts(tool: Option<Tool>, status: StepStatus) -> bool {
    match status {
        StepStatus::Ok | StepStatus::Skipped => false,
        StepStatus::Surprise | StepStatus::Running | StepStatus::Denied => true,
        // A missing file in a batch of reads should not cancel the other reads.
        StepStatus::Error => !matches!(tool, Some(Tool::Read | Tool::Search)),
    }
}

/// Commands that succeed no matter what and so prove nothing. One part that can fail makes the
/// line a check: `cd dir && pytest` is one, `cd dir; echo ok` is not.
fn proves_nothing(cmd: &str) -> bool {
    let mut parts = vec![String::new()];
    let mut quote: Option<char> = None;
    for c in cmd.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '\'' || c == '"' => quote = Some(c),
            None if matches!(c, ';' | '&' | '|' | '\n' | '(' | ')' | '{' | '}') => parts.push(String::new()),
            None => parts.last_mut().unwrap().push(c),
        }
    }
    parts.iter().all(|part| {
        let first = part.split_whitespace().find(|w| !w.contains('=')).unwrap_or("");
        matches!(first, "echo" | "true" | ":" | "printf" | "cd" | "pwd" | "sleep" | "export" | "exit" | "")
    })
}

fn summarize(call: &Call) -> String {
    let a = &call.args;
    let s = |k: &str| str_arg(a, k).unwrap_or("").trim().to_string();
    match call.tool {
        Some(Tool::Shell) => format!("{}{}", one_line(&s("cmd"), 160), if bool_arg(a, "background") { "  &" } else { "" }),
        Some(Tool::Read) => {
            let mut t = s("path");
            if !s("lines").is_empty() {
                t.push_str(&format!(":{}", s("lines")));
            }
            if !s("symbol").is_empty() {
                t.push_str(&format!(" `{}`", s("symbol")));
            }
            t
        }
        Some(Tool::Search) => {
            let mut t = if s("pattern").is_empty() { "files".to_string() } else { format!("/{}/", one_line(&s("pattern"), 80)) };
            if !s("path").is_empty() {
                t.push_str(&format!(" in {}", s("path")));
            }
            if !s("glob").is_empty() {
                t.push_str(&format!(" ({})", s("glob")));
            }
            t
        }
        Some(Tool::Edit) => {
            let place = if !s("range").is_empty() {
                format!(" @{}", s("range"))
            } else if !s("after").is_empty() {
                format!(" after {}", s("after"))
            } else {
                " (text match)".to_string()
            };
            format!("{}{place}", s("path"))
        }
        Some(Tool::Write) => s("path"),
        Some(Tool::Watch) => {
            if bool_arg(a, "off") {
                format!("{} off", s("name"))
            } else {
                format!("{}: {}", s("name"), one_line(&s("cmd"), 120))
            }
        }
        Some(Tool::Job) => {
            let id = s("id");
            if bool_arg(a, "kill") {
                format!("{id} kill")
            } else if !s("until").is_empty() {
                format!("{id} until {:?}", s("until"))
            } else {
                id
            }
        }
        Some(Tool::Finish) => one_line(&s("summary"), 160),
        None => call.raw_name.clone(),
    }
}

impl Runner<'_> {
    pub async fn run(&mut self, calls: &[Call]) -> ScriptResult {
        let scripts = self.sess.features.scripts;
        self.sess.reply += 1;
        let mut res = ScriptResult::default();
        let mut halted: Option<String> = None;
        let mut acted = false;

        for (idx, call) in calls.iter().enumerate() {
            self.sess.step += 1;
            let tool_name = call.tool.map(|t| t.name().to_string()).unwrap_or_else(|| call.raw_name.clone());
            let expect = if self.sess.features.receipts && matches!(call.tool, Some(Tool::Shell | Tool::Watch)) {
                Expect::from_args(&call.args).map(|e| e.describe())
            } else {
                None
            };
            self.sink.emit(EventKind::StepStart { turn: self.turn, idx: idx as u32, call_id: call.id.clone(), tool: tool_name.clone(), summary: summarize(call), expect });

            let started = Instant::now();
            // The closing report of a session that is out of budget is heard even behind a failed
            // step: skipping it would leave no report at all.
            let closing = self.landing && call.tool == Some(Tool::Finish) && call.error.is_none();
            let out = match &halted {
                Some(why) if !closing => {
                    let text = format!("[skipped] not run, because {why}.");
                    StepOutput { status: StepStatus::Skipped, stub: text.clone(), observation: text, ..Default::default() }
                }
                _ => match &call.error {
                    Some(err) => StepOutput::error(err),
                    None => self.execute(call, &mut res, &mut acted).await,
                },
            };
            let ms = started.elapsed().as_millis() as u64;

            self.stats.tool_ms += ms;
            self.stats.hidden_bytes += out.hidden_bytes;
            match out.status {
                StepStatus::Skipped => self.stats.skipped += 1,
                status => {
                    self.stats.steps += 1;
                    if status == StepStatus::Surprise {
                        self.stats.surprises += 1;
                        res.surprised = true;
                    }
                    if out.explicit {
                        self.stats.predictions += 1;
                        if status == StepStatus::Ok {
                            self.stats.predictions_held += 1;
                        }
                    }
                    if matches!(status, StepStatus::Surprise | StepStatus::Error) {
                        res.failed_steps.push(fnv64(format!("{tool_name}\u{0}{}", call.args).as_bytes()));
                    }
                }
            }
            if scripts && halted.is_none() && halts(call.tool, out.status) {
                let how = match out.status {
                    StepStatus::Running => "is still running",
                    StepStatus::Denied => "was not allowed",
                    StepStatus::Error => "could not be carried out",
                    _ => "did not go as expected",
                };
                halted = Some(format!("step {} ({tool_name}) {how}", idx + 1));
            }
            self.sink.emit(EventKind::StepEnd {
                turn: self.turn,
                idx: idx as u32,
                call_id: call.id.clone(),
                tool: tool_name,
                status: out.status,
                observation: out.observation.clone(),
                ms,
                hidden_bytes: out.hidden_bytes,
                violations: out.violations.clone(),
                explicit: out.explicit,
            });
            res.observations.push((call.id.clone(), out.observation, out.stub));
        }

        // Standing expectations: anything the script changed is checked now, without being asked.
        if self.sess.features.watches && acted && res.finished.is_none() && !self.sess.watches.items.is_empty() && !self.cancel.is_cancelled() {
            let (reports, text) = watches::run_stale(self.sess, self.cancel).await;
            if self.emit_watches(&reports) {
                res.surprised = true;
            }
            if !text.is_empty() {
                res.footer.push(text);
            }
        }
        res
    }

    /// Emits watch events; returns true if any watch newly failed.
    fn emit_watches(&mut self, reports: &[WatchReport]) -> bool {
        let mut regressed = false;
        for r in reports {
            if r.state == WatchState::Fail && r.change == WatchChange::Flip {
                regressed = true;
                self.stats.surprises += 1;
            }
            self.sink.emit(EventKind::Watch { turn: self.turn, name: r.name.clone(), cmd: r.cmd.clone(), state: r.state, change: r.change, detail: r.detail.clone() });
        }
        regressed
    }

    async fn approve(&mut self, call: &Call, tool: Tool) -> Result<(), StepOutput> {
        let danger = if tool == Tool::Shell { str_arg(&call.args, "cmd").and_then(tools::shell::danger) } else { None };
        if !tool.is_act() || (self.autonomy == Autonomy::Auto && danger.is_none()) {
            return Ok(());
        }
        let denied = |text: String| StepOutput { status: StepStatus::Denied, stub: text.clone(), observation: text, ..Default::default() };
        if !self.interactive {
            return match danger {
                Some(why) => Err(denied(format!("[denied] refused: this command {why}, and nobody is present to approve it."))),
                None => Ok(()),
            };
        }
        let (id, rx) = self.approvals.request();
        self.sink.emit(EventKind::Approval { id, turn: self.turn, tool: tool.name().to_string(), summary: summarize(call), reason: danger.unwrap_or("this step changes the workspace").to_string() });
        let allow = tokio::select! {
            answer = rx => answer.unwrap_or(false),
            _ = self.cancel.cancelled() => false,
        };
        self.sink.emit(EventKind::ApprovalResolved { id, allow });
        if allow { Ok(()) } else { Err(denied("[denied] the user declined this step. Do not retry it; continue another way or ask what they want.".to_string())) }
    }

    async fn execute(&mut self, call: &Call, res: &mut ScriptResult, acted: &mut bool) -> StepOutput {
        let tool = call.tool.expect("calls without a tool carry an error and never reach execute");
        let args = &call.args;
        if let Err(denied) = self.approve(call, tool).await {
            return denied;
        }
        let epoch_before = self.sess.evidence.epoch;
        let out = match tool {
            Tool::Shell => tools::shell::run(self.sess, args, self.cancel).await,
            Tool::Read => tools::read::run(self.sess, args),
            Tool::Search => tools::search::run(self.sess, args),
            Tool::Edit => tools::edit::run(self.sess, args).await,
            Tool::Write => tools::write::run(self.sess, args).await,
            Tool::Job => tools::job::run(self.sess, args, self.cancel).await,
            Tool::Watch => self.watch(args).await,
            Tool::Finish => self.finish(args, res).await,
        };
        if tool.is_act() {
            *acted = true;
        }
        if out.changed_files {
            let path = str_arg(args, "path").unwrap_or("?").trim().to_string();
            self.sess.evidence.changed(format!("the {} of {path}", tool.name()));
            self.sess.lessons.on_edit();
            if !self.sess.touched.contains(&path) {
                self.sess.touched.push(path.clone());
            }
            res.edited.push(path);
        }
        if let Some(meta) = &out.shell {
            if self.sess.features.lessons {
                if let Some(lesson) = self.sess.lessons.on_shell(&meta.cmd, meta.ok, &meta.output_tail) {
                    self.stats.lessons_learned += 1;
                    self.sink.emit(EventKind::Lesson { learned: true, text: lesson.describe() });
                }
            }
            // Evidence is a prediction of success that came true, made about something that can fail.
            if meta.held_explicit && meta.ok && !proves_nothing(&meta.cmd) {
                self.sess.evidence.passed(format!("`{}`", one_line(&meta.cmd, 100)));
            }
            // A run that left the workspace alone is also the current result of any watch on that
            // command, and the model's own test command becomes a watch here.
            if let (Some(run), true) = (&meta.completed, self.sess.evidence.epoch == epoch_before) {
                let seen = watches::observe(self.sess, &meta.cmd, run);
                for r in &seen.reports {
                    self.sink.emit(EventKind::Watch { turn: self.turn, name: r.name.clone(), cmd: r.cmd.clone(), state: r.state, change: r.change, detail: r.detail.clone() });
                }
                res.footer.extend(seen.notice);
                if self.sess.watches.all_pass_at(self.sess.evidence.epoch) {
                    let names = self.sess.watches.names();
                    self.sess.evidence.passed(format!("watch {names}"));
                }
            }
        }
        out
    }

    async fn watch(&mut self, args: &Value) -> StepOutput {
        if !self.sess.features.watches {
            return StepOutput::error("unknown tool `watch`");
        }
        let name = str_arg(args, "name").map(str::trim).filter(|n| !n.is_empty()).unwrap_or("check").to_string();
        if bool_arg(args, "off") {
            return if self.sess.watches.remove(&name) { StepOutput::ok(format!("[ok] watch {name} removed")) } else { StepOutput::error(format!("there is no watch named {name}")) };
        }
        let Some(cmd) = str_arg(args, "cmd").map(str::trim).filter(|c| !c.is_empty()) else {
            return StepOutput::error("`cmd` is required: the check command to run");
        };
        let expect = if self.sess.features.receipts { Expect::from_args(args) } else { None };
        let cwd = self.sess.ws.cwd.clone();
        // Asking for the check the harness already watches means taking it over under this name.
        self.sess.watches.items.retain(|w| !(w.auto && w.cmd == cmd && w.cwd == cwd));
        let idx = self.sess.watches.set(&name, cmd, cwd, expect);
        let report = watches::run_one(self.sess, idx, self.cancel).await;
        self.sess.watches.items[idx].epoch = self.sess.evidence.epoch;
        if self.sess.watches.all_pass() {
            let names = self.sess.watches.names();
            self.sess.evidence.passed(format!("watch {names}"));
        }
        self.emit_watches(std::slice::from_ref(&report));
        // Registering a check that currently fails is normal (that is often the bug being fixed), so
        // it never halts the script.
        let observation = format!("{}\nIt now re-runs by itself after every change; you will hear about it only when its result changes.", report.detail);
        StepOutput { stub: one_line(&report.detail, 160), observation, ..Default::default() }
    }

    async fn finish(&mut self, args: &Value, res: &mut ScriptResult) -> StepOutput {
        let summary = str_arg(args, "summary").unwrap_or("").trim().to_string();
        if !self.sess.features.gate {
            res.finished = Some(Finished { summary, verified: false, evidence: "not tracked in classic mode".into() });
            return StepOutput::ok("[done]");
        }
        // The gate judges the workspace as it is now, so bring the watches up to date first.
        let unverified = str_arg(args, "unverified").map(str::trim).filter(|r| !r.is_empty());
        let mut failing: Option<(String, bool, String)> = None;
        if self.sess.features.watches && !self.sess.watches.items.is_empty() {
            let (reports, text) = watches::run_stale(self.sess, self.cancel).await;
            self.emit_watches(&reports);
            if !self.sess.watches.all_pass() {
                let not_passing: Vec<&watches::Watch> = self.sess.watches.items.iter().filter(|w| w.state != WatchState::Pass).collect();
                let names = not_passing.iter().map(|w| w.name.as_str()).collect::<Vec<_>>().join(", ");
                // A check the model never asked for must not be able to trap it.
                failing = Some((names, not_passing.iter().all(|w| w.auto), text));
            }
        }
        if self.landing {
            // Nothing can follow this reply, so a refusal or a review would only cost the report.
            // It is accepted, with what is actually known about the work.
            let ev = &self.sess.evidence;
            let (verified, evidence) = match (&failing, unverified) {
                (Some((names, _, _)), _) => (false, format!("unverified: watch {names} was failing when the budget ran out")),
                (None, _) if ev.fresh() => (true, ev.verified_by()),
                (None, Some(reason)) => (false, format!("unverified: {reason}")),
                (None, None) => (false, format!("unverified: nothing was checked after {} before the budget ran out", ev.last_change)),
            };
            res.finished = Some(Finished { summary, verified, evidence });
            return StepOutput::ok(if verified { "[done] accepted" } else { "[done] accepted without verification" });
        }
        if let Some((names, waivable, text)) = failing {
            if let (true, Some(reason)) = (waivable, unverified) {
                res.finished = Some(Finished { summary, verified: false, evidence: format!("unverified: {reason} (watch {names} is failing)") });
                return StepOutput::ok("[done] accepted without verification");
            }
            let head = format!("[SURPRISE] finish refused: watch {names} is not passing.");
            let mut detail = if text.is_empty() { "Its output has not changed since it was last shown.".to_string() } else { text };
            if waivable {
                detail.push_str("\nFix it. If it cannot pass for a reason outside this task, call finish with `unverified` set to that reason.");
            }
            return StepOutput { status: StepStatus::Surprise, violations: vec![format!("watch {names} is failing")], stub: head.clone(), observation: format!("{head}\n{detail}"), ..Default::default() };
        }
        // Passing checks say the visible tests hold, not that the task is done: ask once for a
        // review against the task before accepting work that changed something.
        let ev = &mut self.sess.evidence;
        if self.sess.features.review && !ev.reviewed && ev.epoch > ev.message_epoch && (ev.fresh() || unverified.is_some()) {
            ev.reviewed = true;
            let changes = review::changes(self.sess).await;
            let text = review::text(&self.sess.task, changes.as_deref());
            return StepOutput { stub: "[review] one review pass before finish is accepted".into(), observation: text, ..StepOutput::ok("") };
        }
        let ev = &mut self.sess.evidence;
        if ev.fresh() {
            res.finished = Some(Finished { summary, verified: true, evidence: ev.verified_by() });
            return StepOutput::ok("[done] accepted");
        }
        if let Some(reason) = unverified {
            res.finished = Some(Finished { summary, verified: false, evidence: format!("unverified: {reason}") });
            return StepOutput::ok("[done] accepted without verification");
        }
        // The gate asks for a check; it must not be able to trap a model that cannot produce one.
        ev.refusals += 1;
        if ev.refusals > 2 {
            res.finished = Some(Finished { summary, verified: false, evidence: format!("unverified: no check passed after {}", ev.last_change) });
            return StepOutput::ok("[done] accepted without verification");
        }
        let text = format!(
            "[gate] finish refused: nothing has been checked since {}. Run a check (shell with expect_exit / expect_has) and call finish right after it in the same reply. If no check is possible, call finish again with `unverified` set to the reason.",
            ev.last_change
        );
        StepOutput { status: StepStatus::Surprise, violations: vec!["no check has passed since the last change".into()], stub: text.clone(), observation: text, ..Default::default() }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::events::MemorySink;
    use crate::provider::ToolCall;
    use crate::repair::parse_calls;
    use crate::state::{Features, test_session};
    use serde_json::json;

    fn calls(list: &[(&str, Value)]) -> Vec<Call> {
        let raw: Vec<ToolCall> = list.iter().enumerate().map(|(i, (n, a))| ToolCall { id: format!("c{i}"), name: n.to_string(), arguments: a.to_string() }).collect();
        parse_calls(&raw).0
    }

    async fn run(sess: &mut Session, list: &[(&str, Value)]) -> (ScriptResult, Stats, Vec<EventKind>) {
        run_as(sess, list, false).await
    }

    /// `landing`: the script is the last reply the budget allows.
    async fn run_as(sess: &mut Session, list: &[(&str, Value)], landing: bool) -> (ScriptResult, Stats, Vec<EventKind>) {
        let sink = MemorySink::default();
        let mut stats = Stats::default();
        let cancel = CancellationToken::new();
        let approvals = Approvals::default();
        let res = Runner { sess, sink: &sink, cancel: &cancel, approvals: &approvals, stats: &mut stats, turn: 1, autonomy: Autonomy::Auto, interactive: false, landing }.run(&calls(list)).await;
        let events = sink.events.lock().unwrap().clone();
        (res, stats, events)
    }

    #[tokio::test]
    async fn on_the_last_reply_finish_is_taken_as_it_stands_and_says_what_is_known() {
        let finish = ("finish", json!({"summary": "half done"}));
        // Unchecked, with the review still due and behind a failed step: accepted all the same.
        let mut s = test_session("script-landing", Features::presage());
        std::fs::write(s.ws.root.join("n.txt"), "1\n").unwrap();
        let (res, _, events) = run_as(&mut s, &[("edit", json!({"path": "n.txt", "old": "1", "new": "3"})), ("shell", json!({"cmd": "grep -qx 2 n.txt"})), finish.clone()], true).await;
        assert_eq!(statuses(&events), vec![StepStatus::Ok, StepStatus::Surprise, StepStatus::Ok]);
        let done = res.finished.expect("nothing can follow the last reply");
        assert_eq!((done.verified, done.evidence.as_str(), done.summary.as_str()), (false, "unverified: nothing was checked after the edit of n.txt before the budget ran out", "half done"));

        // Any other step behind a failed one is skipped as always.
        let (res, _, events) = run_as(&mut s, &[("shell", json!({"cmd": "grep -qx 2 n.txt"})), ("write", json!({"path": "b.txt", "content": "x"})), finish.clone()], true).await;
        assert_eq!(statuses(&events), vec![StepStatus::Surprise, StepStatus::Skipped, StepStatus::Ok]);
        assert!(res.finished.is_some() && !s.ws.root.join("b.txt").exists());

        // A failing watch is named; a passing one verifies, with no review in between.
        let mut s = test_session("script-landing-watch", Features::presage());
        std::fs::write(s.ws.root.join("n.txt"), "1\n").unwrap();
        run(&mut s, &[("watch", json!({"name": "is-two", "cmd": "grep -qx 2 n.txt"}))]).await;
        let (res, _, _) = run_as(&mut s, &[("edit", json!({"path": "n.txt", "old": "1", "new": "3"})), finish.clone()], true).await;
        assert_eq!(res.finished.unwrap().evidence, "unverified: watch is-two was failing when the budget ran out");
        let (res, _, _) = run_as(&mut s, &[("edit", json!({"path": "n.txt", "old": "3", "new": "2"})), finish], true).await;
        let done = res.finished.unwrap();
        assert_eq!((done.verified, done.evidence.as_str()), (true, "verified by watch is-two"));
    }

    fn statuses(events: &[EventKind]) -> Vec<StepStatus> {
        events.iter().filter_map(|e| if let EventKind::StepEnd { status, .. } = e { Some(*status) } else { None }).collect()
    }

    #[tokio::test]
    async fn one_reply_can_edit_check_and_finish() {
        let mut s = test_session("script-happy", Features::presage().without("review").unwrap());
        std::fs::write(s.ws.root.join("n.txt"), "1\n").unwrap();
        let (res, stats, events) = run(
            &mut s,
            &[
                ("edit", json!({"path": "n.txt", "old": "1", "new": "2"})),
                ("shell", json!({"cmd": "grep -qx 2 n.txt && echo verified", "expect": {"has": "verified"}})),
                ("finish", json!({"summary": "bumped n to 2"})),
            ],
        )
        .await;
        assert_eq!(statuses(&events), vec![StepStatus::Ok; 3]);
        let done = res.finished.expect("accepted in the same reply");
        assert!(done.verified);
        assert_eq!(done.evidence, "verified by `grep -qx 2 n.txt && echo verified`");
        assert_eq!((stats.steps, stats.predictions, stats.predictions_held, stats.surprises), (3, 1, 1, 0));
    }

    #[tokio::test]
    async fn a_surprise_halts_everything_after_it() {
        let mut s = test_session("script-halt", Features::presage().without("review").unwrap());
        std::fs::write(s.ws.root.join("n.txt"), "1\n").unwrap();
        let (res, stats, events) = run(
            &mut s,
            &[
                ("edit", json!({"path": "n.txt", "old": "1", "new": "2"})),
                ("shell", json!({"cmd": "grep -qx 3 n.txt", "expect": {"exit": "0"}})),
                ("shell", json!({"cmd": "touch should-not-exist"})),
                ("finish", json!({"summary": "done"})),
            ],
        )
        .await;
        assert_eq!(statuses(&events), vec![StepStatus::Ok, StepStatus::Surprise, StepStatus::Skipped, StepStatus::Skipped]);
        assert!(res.finished.is_none() && res.surprised);
        assert!(!s.ws.root.join("should-not-exist").exists());
        assert_eq!(res.observations[2].1, "[skipped] not run, because step 2 (shell) did not go as expected.");
        assert_eq!((stats.steps, stats.skipped, stats.surprises), (2, 2, 1));
        assert_eq!(res.failed_steps.len(), 1);
    }

    #[tokio::test]
    async fn a_failed_read_does_not_cancel_its_neighbours_but_a_failed_edit_does() {
        let mut s = test_session("script-probes", Features::presage().without("review").unwrap());
        std::fs::write(s.ws.root.join("a.txt"), "alpha\n").unwrap();
        let (_, _, events) = run(&mut s, &[("read", json!({"path": "missing.txt"})), ("read", json!({"path": "a.txt"})), ("search", json!({"pattern": "alpha"}))]).await;
        assert_eq!(statuses(&events), vec![StepStatus::Error, StepStatus::Ok, StepStatus::Ok]);

        let (_, _, events) = run(&mut s, &[("edit", json!({"path": "a.txt", "old": "not there", "new": "x"})), ("shell", json!({"cmd": "true"}))]).await;
        assert_eq!(statuses(&events), vec![StepStatus::Error, StepStatus::Skipped]);
    }

    #[tokio::test]
    async fn finish_needs_fresh_evidence_or_a_stated_reason() {
        let mut s = test_session("script-gate", Features::presage().without("review").unwrap());
        std::fs::write(s.ws.root.join("n.txt"), "1\n").unwrap();

        // Nothing changed yet: nothing to verify.
        let (res, _, _) = run(&mut s, &[("finish", json!({"summary": "it is 1"}))]).await;
        assert_eq!(res.finished.unwrap().evidence, "no changes were made");

        let (res, _, events) = run(&mut s, &[("edit", json!({"path": "n.txt", "old": "1", "new": "2"})), ("finish", json!({"summary": "done"}))]).await;
        assert!(res.finished.is_none());
        assert_eq!(statuses(&events), vec![StepStatus::Ok, StepStatus::Surprise]);
        assert!(res.observations[1].1.starts_with("[gate] finish refused: nothing has been checked since the edit of n.txt."), "{}", res.observations[1].1);

        // A command that cannot fail is not a check.
        let (res, _, _) = run(&mut s, &[("shell", json!({"cmd": "echo all good", "expect": {"has": "good"}})), ("finish", json!({"summary": "done"}))]).await;
        assert!(res.finished.is_none());

        // Nor is a run that failed, even if what the model said it would print was printed.
        let (res, _, events) = run(&mut s, &[("write", json!({"path": "log.txt", "content": "x"})), ("shell", json!({"cmd": "sh -c \"echo '5 passing, 1 failing'; exit 1\"", "expect_has": "passing"})), ("finish", json!({"summary": "done"}))]).await;
        assert_eq!(statuses(&events), vec![StepStatus::Ok, StepStatus::Ok, StepStatus::Surprise]);
        assert!(res.finished.is_none());

        // A check behind a `cd` is still a check; a line of commands that cannot fail is not.
        assert!(!proves_nothing("cd sub && FOO=1 python3 -m unittest") && !proves_nothing("echo start; grep -q 2 n.txt"));
        assert!(proves_nothing("cd sub && echo 'done; grep x'") && proves_nothing("  ") && proves_nothing("export A=1; pwd"));

        // A check that passed, then another change: stale again.
        run(&mut s, &[("shell", json!({"cmd": "grep -qx 2 n.txt", "expect": {"exit": "0"}}))]).await;
        assert!(s.evidence.fresh());
        let (res, _, _) = run(&mut s, &[("write", json!({"path": "other.txt", "content": "x"})), ("finish", json!({"summary": "done"}))]).await;
        assert!(res.finished.is_none());

        let (res, _, _) = run(&mut s, &[("finish", json!({"summary": "done", "unverified": "docs only, nothing to run"}))]).await;
        let done = res.finished.unwrap();
        assert!(!done.verified);
        assert_eq!(done.evidence, "unverified: docs only, nothing to run");

        // Refused twice for the same change, the third finish is taken as unverified.
        let (res, _, _) = run(&mut s, &[("write", json!({"path": "third.txt", "content": "x"})), ("finish", json!({"summary": "done"}))]).await;
        assert!(res.finished.is_none());
        let (res, _, _) = run(&mut s, &[("finish", json!({"summary": "done"}))]).await;
        assert!(res.finished.is_none());
        let (res, _, _) = run(&mut s, &[("finish", json!({"summary": "done"}))]).await;
        let done = res.finished.unwrap();
        assert_eq!((done.verified, done.evidence.as_str()), (false, "unverified: no check passed after the write of third.txt"));
    }

    #[tokio::test]
    async fn watches_rerun_after_changes_and_gate_finish() {
        let mut s = test_session("script-watch", Features::presage().without("review").unwrap());
        std::fs::write(s.ws.root.join("n.txt"), "1\n").unwrap();
        let (res, _, events) = run(&mut s, &[("watch", json!({"name": "is-two", "cmd": "grep -qx 2 n.txt && echo ok"})), ("edit", json!({"path": "n.txt", "old": "1", "new": "3"}))]).await;
        // Registering a failing watch does not halt: the edit still ran.
        assert_eq!(statuses(&events), vec![StepStatus::Ok, StepStatus::Ok]);
        assert!(res.observations[0].1.starts_with("watch is-two: FAIL (expected exit 0, got exit 1)"), "{}", res.observations[0].1);
        assert_eq!(res.footer, vec!["watch is-two: still FAIL, same output as before"]);

        let (res, _, _) = run(&mut s, &[("finish", json!({"summary": "done"}))]).await;
        assert!(res.finished.is_none());
        assert!(res.observations[0].1.starts_with("[SURPRISE] finish refused: watch is-two is not passing."));

        // Fixing it flips the watch, which is both reported and accepted as evidence.
        let (res, _, events) = run(&mut s, &[("edit", json!({"path": "n.txt", "old": "3", "new": "2"})), ("finish", json!({"summary": "n is 2"}))]).await;
        let done = res.finished.expect("watch passes now");
        assert_eq!(done.evidence, "verified by watch is-two");
        assert!(events.iter().any(|e| matches!(e, EventKind::Watch { state: WatchState::Pass, change: WatchChange::Flip, .. })));

        // Breaking it again is announced in the footer of that very script.
        let (res, stats, _) = run(&mut s, &[("edit", json!({"path": "n.txt", "old": "2", "new": "9"}))]).await;
        assert!(res.surprised && stats.surprises == 1);
        assert!(res.footer[0].starts_with("watch is-two: PASS → FAIL"), "{:?}", res.footer);
    }

    #[tokio::test]
    async fn environment_lessons_are_learned_from_the_scripts_themselves() {
        let mut s = test_session("script-lesson", Features::presage().without("review").unwrap());
        run(&mut s, &[("shell", json!({"cmd": "definitely-not-installed-tool --version"}))]).await;
        let (_, stats, events) = run(&mut s, &[("shell", json!({"cmd": "true definitely-not-installed-tool --version"}))]).await;
        assert_eq!(stats.lessons_learned, 0, "unrelated command");
        assert!(!events.iter().any(|e| matches!(e, EventKind::Lesson { .. })));

        run(&mut s, &[("shell", json!({"cmd": "python0 -c 'print(1)'"}))]).await;
        let (_, stats, events) = run(&mut s, &[("shell", json!({"cmd": "python3 -c 'print(1)'"}))]).await;
        assert_eq!(stats.lessons_learned, 1);
        assert!(events.iter().any(|e| matches!(e, EventKind::Lesson { learned: true, text } if text.contains("`python3 -c 'print(1)'` works"))));
    }

    #[tokio::test]
    async fn dangerous_commands_are_refused_when_nobody_can_approve() {
        let mut s = test_session("script-danger", Features::presage().without("review").unwrap());
        let (res, _, events) = run(&mut s, &[("shell", json!({"cmd": "rm -rf /"})), ("shell", json!({"cmd": "echo after"}))]).await;
        assert_eq!(statuses(&events), vec![StepStatus::Denied, StepStatus::Skipped]);
        assert!(res.observations[0].1.contains("recursive delete of the filesystem root"));
    }

    #[tokio::test]
    async fn approval_requests_wait_for_an_answer() {
        let mut s = test_session("script-ask", Features::presage().without("review").unwrap());
        let sink = MemorySink::default();
        let mut stats = Stats::default();
        let cancel = CancellationToken::new();
        let approvals = Approvals::default();
        let list = calls(&[("write", json!({"path": "a.txt", "content": "x"})), ("write", json!({"path": "b.txt", "content": "y"}))]);
        let answer = async {
            // Allow the first request, decline the second.
            for (id, allow) in [(1u64, true), (2u64, false)] {
                while !approvals.resolve(id, allow) {
                    tokio::task::yield_now().await;
                }
            }
        };
        let mut runner = Runner { sess: &mut s, sink: &sink, cancel: &cancel, approvals: &approvals, stats: &mut stats, turn: 1, autonomy: Autonomy::Ask, interactive: true, landing: false };
        let (res, _) = tokio::join!(runner.run(&list), answer);
        assert!(s.ws.root.join("a.txt").exists() && !s.ws.root.join("b.txt").exists());
        assert!(res.observations[1].1.starts_with("[denied] the user declined"));
        let events = sink.events.lock().unwrap();
        assert_eq!(events.iter().filter(|e| matches!(e, EventKind::Approval { .. })).count(), 2);
    }

    #[tokio::test]
    async fn classic_mode_runs_every_call_and_accepts_any_finish() {
        let mut s = test_session("script-classic", Features::classic());
        let (res, _, events) = run(&mut s, &[("shell", json!({"cmd": "exit 1"})), ("shell", json!({"cmd": "echo still runs"})), ("finish", json!({"summary": "done"}))]).await;
        assert_eq!(statuses(&events), vec![StepStatus::Error, StepStatus::Ok, StepStatus::Ok]);
        assert!(res.finished.is_some());
    }

    #[tokio::test]
    async fn a_test_run_becomes_a_watch_so_an_edit_alone_comes_back_checked() {
        let mut s = test_session("script-auto", Features::presage().without("review").unwrap());
        std::fs::write(s.ws.root.join("n.txt"), "1\n").unwrap();
        std::fs::write(s.ws.root.join("check.sh"), "grep -qx 2 n.txt && echo PASS || { echo \"FAIL: n is $(cat n.txt)\"; exit 1; }\n").unwrap();
        let watch_events = |events: &[EventKind]| -> Vec<(WatchState, WatchChange)> { events.iter().filter_map(|e| if let EventKind::Watch { state, change, .. } = e { Some((*state, *change)) } else { None }).collect() };

        // Reproducing the failure sets up the watch from that very run: the command is not run twice.
        let (res, _, events) = run(&mut s, &[("shell", json!({"cmd": "sh check.sh", "expect_exit": "nonzero"}))]).await;
        assert_eq!(res.footer.len(), 1);
        assert!(res.footer[0].starts_with("watch tests: FAIL · `sh check.sh` is now watched"), "{:?}", res.footer);
        assert_eq!(watch_events(&events), vec![(WatchState::Fail, WatchChange::New)]);

        // An edit on its own is answered with the test result, and that result is evidence.
        let (res, _, _) = run(&mut s, &[("edit", json!({"path": "n.txt", "old": "1", "new": "2"}))]).await;
        assert_eq!(res.footer, vec!["watch tests: FAIL → PASS · PASS"]);
        assert!(s.evidence.fresh());

        // A model that still runs the tests by hand does not make them run a second time.
        let (res, _, events) = run(&mut s, &[("edit", json!({"path": "n.txt", "old": "2", "new": "9"})), ("shell", json!({"cmd": "sh check.sh", "expect_exit": "nonzero"}))]).await;
        assert!(res.footer.is_empty(), "{:?}", res.footer);
        assert_eq!(watch_events(&events), vec![(WatchState::Fail, WatchChange::Flip)]);
        assert!(!s.evidence.fresh());

        // With the watch in place, `finish` can ride along with the edit: it is judged by the watch.
        let (res, _, _) = run(&mut s, &[("edit", json!({"path": "n.txt", "old": "9", "new": "2"})), ("finish", json!({"summary": "n is 2"}))]).await;
        assert_eq!(res.finished.expect("the watch passes").evidence, "verified by watch tests");
    }

    #[tokio::test]
    async fn a_failing_watch_the_model_never_asked_for_can_be_waived_with_a_reason() {
        let mut s = test_session("script-waive", Features::presage().without("review").unwrap());
        std::fs::write(s.ws.root.join("check.sh"), "echo 'FAIL: the service is unreachable'; exit 1\n").unwrap();
        run(&mut s, &[("shell", json!({"cmd": "sh check.sh"}))]).await;
        run(&mut s, &[("write", json!({"path": "a.txt", "content": "x"}))]).await;

        let (res, _, _) = run(&mut s, &[("finish", json!({"summary": "done"}))]).await;
        assert!(res.finished.is_none());
        assert!(res.observations[0].1.starts_with("[SURPRISE] finish refused: watch tests is not passing."), "{}", res.observations[0].1);
        assert!(res.observations[0].1.ends_with("call finish with `unverified` set to that reason."), "{}", res.observations[0].1);
        let (res, _, _) = run(&mut s, &[("finish", json!({"summary": "done", "unverified": "check.sh needs a service that is not running here"}))]).await;
        let done = res.finished.expect("waived");
        assert!(!done.verified);
        assert_eq!(done.evidence, "unverified: check.sh needs a service that is not running here (watch tests is failing)");

        // Asking for the same check by name takes it over, and a requested watch stays binding.
        let (_, _, _) = run(&mut s, &[("watch", json!({"name": "service", "cmd": "sh check.sh"}))]).await;
        assert_eq!(s.watches.names(), "service");
        let (res, _, _) = run(&mut s, &[("finish", json!({"summary": "done", "unverified": "still unreachable"}))]).await;
        assert!(res.finished.is_none());
        assert!(!res.observations[0].1.contains("unverified"), "{}", res.observations[0].1);
    }

    #[test]
    fn step_summaries_are_short_and_specific() {
        let c = calls(&[
            ("shell", json!({"cmd": "pytest -q\n# second line", "background": true})),
            ("read", json!({"path": "a.py", "lines": "1-9", "symbol": "f"})),
            ("search", json!({"pattern": "todo", "path": "src", "glob": "*.rs"})),
            ("edit", json!({"path": "a.py", "range": "4ab-9cd", "new": ""})),
            ("job", json!({"id": "bg:1", "until": "ready"})),
            ("nope", json!({})),
        ]);
        let got: Vec<String> = c.iter().map(summarize).collect();
        assert_eq!(got, vec!["pytest -q  &", "a.py:1-9 `f`", "/todo/ in src (*.rs)", "a.py @4ab-9cd", "bg:1 until \"ready\"", "nope"]);
    }
}
