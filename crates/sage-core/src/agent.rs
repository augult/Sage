//! The agent loop: one model reply, one script, repeat until the task is finished or handed back.

use crate::attach;
use crate::bootstrap;
use crate::changes;
use crate::context::Context;
use crate::economy::{peak_multiplier, prices_for};
use crate::events::{CacheVerdict, CallInfo, Channel, EventKind, FinishStatus, GoalState, Sink, Stats};
use crate::examiner;
use crate::goal::{self, Goal, Verdict};
use crate::prompt;
use crate::provider::{ChatRequest, ChatResponse, Delta, Effort, Message, Provider, ProviderError, Role, Usage};
use crate::repair;
use crate::resume::{self, Setup, Snapshot};
use crate::script::{Approvals, Autonomy, Runner, ScriptResult};
use crate::skills::{self, SkillInfo};
use crate::state::{Features, Session};
use crate::tools::shell::{ExecEnd, StateKind, exec};
use crate::util::{now_ms, one_line};
use crate::watches;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// How hard the model is asked to think.
///
/// `Ratchet` starts at a level and only ever goes up, when surprises pile up. One-way on purpose:
/// on some providers the effort setting is part of the cached prompt, so every change can cost a full
/// re-read of the conversation. A ratchet pays that at most twice per session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffortPolicy {
    Fixed(Effort),
    Ratchet(Effort),
}

impl EffortPolicy {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase();
        match s.as_str() {
            "ratchet" | "auto" | "adaptive" => Some(EffortPolicy::Ratchet(Effort::Low)),
            _ => match s.strip_prefix("ratchet-").or_else(|| s.strip_prefix("ratchet:")) {
                Some(start) => Effort::parse(start).map(EffortPolicy::Ratchet),
                None => Effort::parse(&s).map(EffortPolicy::Fixed),
            },
        }
    }

    pub fn start(&self) -> Effort {
        match self {
            EffortPolicy::Fixed(e) | EffortPolicy::Ratchet(e) => *e,
        }
    }

    pub fn label(&self) -> String {
        match self {
            EffortPolicy::Fixed(e) => e.as_str().to_string(),
            EffortPolicy::Ratchet(e) => format!("ratchet-{}", e.as_str()),
        }
    }
}

/// The effort to go on with after a reply that was nothing but reasoning, the `nth` of its run:
/// one level lower. Thinking is switched off only at the second such reply. A model that thinks
/// too long at `low` is on a task that needs thought, and has just been told to act: without any
/// thinking it runs through its budget instead.
fn less_thinking(effort: Effort, nth: u32) -> Option<Effort> {
    effort.lower().filter(|lower| *lower != Effort::Off || nth > 1)
}

#[derive(Clone, Debug)]
pub struct AgentConfig {
    pub features: Features,
    pub effort: EffortPolicy,
    pub model: String,
    /// Model replies allowed per user message.
    pub max_turns: u32,
    /// Output cap per reply, reasoning included: a ceiling on what one runaway reply can cost.
    pub max_tokens: u32,
    pub time_budget: Option<Duration>,
    pub autonomy: Autonomy,
    /// A person is watching and can answer; otherwise the agent must reach `finish` on its own.
    pub interactive: bool,
    /// Prompt size (tokens) above which old observations are trimmed whatever the price.
    pub context_limit: u64,
    /// The system prompt, where the session has its own; otherwise the built-in one for `features`.
    pub system: Option<String>,
    /// Skills the session may switch on by itself, where it routes tasks to skills.
    pub routable: Vec<SkillInfo>,
}

impl AgentConfig {
    pub fn new(model: &str) -> Self {
        Self {
            features: Features::ecosage(),
            effort: EffortPolicy::Ratchet(Effort::Low),
            model: model.to_string(),
            max_turns: 80,
            max_tokens: 32_000,
            time_budget: None,
            autonomy: Autonomy::Auto,
            interactive: false,
            context_limit: 150_000,
            system: None,
            routable: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub status: FinishStatus,
    pub summary: String,
    pub verified: bool,
    pub evidence: String,
}

pub struct Agent {
    provider: Arc<Provider>,
    pub sess: Session,
    pub ctx: Context,
    cfg: AgentConfig,
    sink: Arc<dyn Sink>,
    approvals: Arc<Approvals>,
    pub stats: Stats,
    turn: u32,
    effort: Effort,
    surprise_streak: u32,
    failures: HashMap<u64, u32>,
    edits: HashMap<String, u32>,
    first_message: bool,
    budget_marks: u8,
    /// How many more times the model is reminded that steps can be chained.
    chain_hints: u8,
    /// Replies in a row that carried a single call, and how often that may still be pointed out.
    solo_streak: u32,
    solo_hints: u8,
    /// The `job` tool was in the list sent with the previous request.
    offered_job_tool: bool,
    /// What `retune` changed, to be said in the log once the message it came with is in it.
    announce: Vec<EventKind>,
    /// Files the user attached to the message that comes next, by path.
    attached: Vec<String>,
    /// The session's goal (`/goal`): what a run has to satisfy before it may end.
    goal: Option<Goal>,
    /// What the message that comes next is shown as in the log, where that is not what the model is sent.
    shown: Option<String>,
    /// Where the conversation and the rest of what continuing needs are stored after each reply
    /// (`checkpoint`), with how the session was set up.
    keep: Option<(PathBuf, Setup)>,
    /// Said in front of the next message: the session was brought back after a restart.
    restored: Option<String>,
    /// The examiner's checks: asked for, or written and ready to run.
    exam: ExamState,
    /// What became of them, for the outcome.
    exam_note: Option<String>,
}

/// What a message may change about the session it is sent to. Left out, a thing stays as it is.
#[derive(Default)]
pub struct Retune {
    /// Another model: what to reach it through, its provider's id and its name.
    pub provider: Option<(Provider, String, String)>,
    pub effort: Option<EffortPolicy>,
    pub autonomy: Option<Autonomy>,
    /// A goal to work towards from this message on, in place of the one there was. Clearing it is
    /// `goal_command`.
    pub goal: Option<String>,
}

/// What a run does at the point where it would otherwise end.
enum Ending {
    /// Nothing is left to ask: the run ends with this outcome.
    End(Outcome),
    /// The model is sent back to work with this message.
    Carry(String),
}

/// What the checks before a run ends have used up in this run.
#[derive(Default)]
struct EndChecks {
    /// Goal checks made for this user message.
    goal: u32,
}

const KEEP_RECENT_MESSAGES: usize = 12;
const MIN_DECAY_TOKENS: u64 = 6_000;
/// Reasoning a reply may spend before its first word or call where long thought is allowed: about
/// 24,000 tokens, which still leaves room for the answer under the output cap.
const LONG_THOUGHT_CHARS: usize = 96_000;
const COVERAGE_NOTE: &str = "The checks pass. They show only what they exercise, and the work is held to everything the task and the documents it names say. Before `finish`, go through those texts rule by rule and, for every rule the checks do not exercise, run a case of your own whose expected result you take from the text, not from your code.";
const LAST_REPLY_NOTE: &str = "Your next reply is the last one the budget allows. Start nothing new: call `finish` in it and say in the summary what is done, how it was checked, and what is unfinished or unchecked. It is accepted as it stands.";
/// How long after it was asked for the examiner's script is still waited for at `finish`.
const EXAM_WAIT: Duration = Duration::from_secs(240);
const EXAM_RUN: Duration = Duration::from_secs(120);
/// How often the examiner's cases may hold one message's `finish`.
const EXAM_HOLDS: u8 = 3;
const ROUTE_WAIT: Duration = Duration::from_secs(30);

type SideReply = (Result<ChatResponse, ProviderError>, Duration);

enum ExamState {
    None,
    /// Asked for at `asked`, being written beside the run.
    Pending { reply: tokio::task::JoinHandle<SideReply>, asked: Instant, stop: CancellationToken },
    Ready(Exam),
}

struct Exam {
    cmd: String,
    path: String,
    holds: u8,
    /// Digest of the failing output the worker was last shown.
    shown: Option<u64>,
}

/// One request beside the conversation: no tools, nothing streamed to anyone.
async fn side_request(provider: Arc<Provider>, messages: Vec<Message>, effort: Effort, max_tokens: u32, reasoning_cap: usize, cancel: CancellationToken) -> SideReply {
    let tools = serde_json::json!([]);
    let asked = Instant::now();
    let reply = provider.chat(ChatRequest { messages: &messages, tools: &tools, effort, max_tokens, reasoning_cap }, &mut |_| {}, &cancel).await;
    (reply, asked.elapsed())
}

impl Agent {
    pub fn new(provider: Provider, sess: Session, cfg: AgentConfig, sink: Arc<dyn Sink>, approvals: Arc<Approvals>) -> Self {
        let ctx = Context::new(cfg.system.as_deref().unwrap_or_else(|| prompt::system_prompt(&cfg.features)));
        let effort = cfg.effort.start();
        Self { provider: Arc::new(provider), exam: ExamState::None, exam_note: None, sess, ctx, cfg, sink, approvals, stats: Stats::default(), turn: 0, effort, surprise_streak: 0, failures: HashMap::new(), edits: HashMap::new(), first_message: true, budget_marks: 0, chain_hints: 2, solo_streak: 0, solo_hints: 2, offered_job_tool: false, announce: Vec::new(), attached: Vec::new(), goal: None, shown: None, keep: None, restored: None }
    }

    /// Files the user attached to the next message. They go to the model by path, in front of the
    /// message: it reads the ones it needs with the tool it reads everything else with.
    pub fn attach(&mut self, files: Vec<String>) {
        self.attached = files;
    }

    /// Applies `change` from the next message on. The conversation stays as it is: another model
    /// is sent all of it and has none of it cached, so its first request is a first request
    /// again, not a miss. An effort that changes starts over from where its policy starts.
    pub fn retune(&mut self, change: Retune) {
        let (mut provider, mut model, mut effort, mut autonomy) = (None, None, None, None);
        if let Some((next, id, name)) = change.provider {
            self.provider = Arc::new(next);
            self.sess.prices = prices_for(&name);
            self.cfg.model = name.clone();
            self.ctx.forget_cache();
            (provider, model) = (Some(id), Some(name));
        }
        if let Some(policy) = change.effort.filter(|policy| *policy != self.cfg.effort) {
            self.cfg.effort = policy;
            self.effort = policy.start();
            self.surprise_streak = 0;
            effort = Some(policy.label());
        }
        if let Some(next) = change.autonomy.filter(|next| *next != self.cfg.autonomy) {
            self.cfg.autonomy = next;
            autonomy = Some(if next == Autonomy::Ask { "ask" } else { "auto" }.to_string());
        }
        if provider.is_some() || effort.is_some() || autonomy.is_some() {
            self.announce.push(EventKind::Settings { provider, model, effort, autonomy });
        }
        if let Some(text) = change.goal.map(|text| text.trim().to_string()).filter(|text| !text.is_empty()) {
            self.goal = Some(Goal::new(&text));
            self.announce.push(goal_event(GoalState::Set, text, ""));
        }
    }

    /// The session's goal, if it has one: met or not.
    pub fn goal(&self) -> Option<&Goal> {
        self.goal.as_ref()
    }

    /// What the next message is shown as in the log, where it is not what the model is sent:
    /// `/goal <condition>`, which the model gets worded as a task.
    pub fn shown_as(&mut self, text: String) {
        self.shown = Some(text);
    }

    /// A `/goal` message that is no task: it goes in the log as the user's, is answered from here
    /// and costs no request to the model.
    pub fn goal_command(&mut self, shown: &str, command: goal::Command) {
        self.sink.emit(EventKind::User { text: shown.trim().to_string(), skills: Vec::new(), files: Vec::new() });
        for change in std::mem::take(&mut self.announce) {
            self.sink.emit(change);
        }
        let event = match command {
            goal::Command::Set(text) => {
                self.goal = Some(Goal::new(&text));
                goal_event(GoalState::Set, text.trim().to_string(), "")
            }
            goal::Command::Clear => goal_event(GoalState::Cleared, self.goal.take().map(|g| g.text).unwrap_or_default(), ""),
            goal::Command::Show => match &self.goal {
                Some(g) => goal_event(GoalState::Shown, g.text.clone(), if g.met { "met" } else { "not met yet" }),
                None => goal_event(GoalState::Shown, String::new(), ""),
            },
        };
        self.sink.emit(event);
        self.save_point(false);
    }

    pub fn effort(&self) -> Effort {
        self.effort
    }

    fn note(&self, kind: &str, text: &str) {
        self.sink.emit(EventKind::Note { turn: self.turn, kind: kind.to_string(), text: text.to_string() });
    }

    /// Books a request made beside the conversation like any other: its tokens, cost and time.
    fn count_side_request(&mut self, usage: &Usage, took: Duration) {
        self.stats.side_requests += 1;
        self.stats.prompt_tokens += usage.prompt;
        self.stats.cached_tokens += usage.cached;
        self.stats.output_tokens += usage.output;
        self.stats.reasoning_tokens += usage.reasoning;
        self.stats.model_ms += took.as_millis() as u64;
        if let Some(p) = self.sess.prices {
            self.stats.cost += p.cost(usage) * peak_multiplier(&self.cfg.model, now_ms() / 1000);
            self.stats.priced = true;
        }
    }

    /// Asks which of the skills the session may switch on by itself fit `task`. One short request
    /// without thinking; with no answer in time, none.
    async fn route(&mut self, task: &str, cancel: &CancellationToken) -> Vec<SkillInfo> {
        let known = self.cfg.routable.clone();
        if known.is_empty() {
            return Vec::new();
        }
        let playbooks: Vec<(&str, &str)> = known.iter().map(|s| (s.name.as_str(), s.description.as_str())).collect();
        let request = side_request(self.provider.clone(), examiner::route_brief(task, &playbooks), Effort::Off, 400, 0, cancel.child_token());
        let Ok((Ok(resp), took)) = tokio::time::timeout(ROUTE_WAIT, request).await else {
            self.note("routing", "no skill was chosen: the router did not answer");
            return Vec::new();
        };
        self.count_side_request(&resp.usage, took);
        let names: Vec<&str> = known.iter().map(|s| s.name.as_str()).collect();
        let picked = examiner::routed(&resp.content, &names);
        if !picked.is_empty() {
            self.note("routing", &format!("skill switched on for this task: {}", picked.join(", ")));
        }
        known.iter().filter(|s| picked.contains(&s.name.as_str())).cloned().collect()
    }

    /// Asks the examiner for its script. It is written while the run goes on, with as long to
    /// think as a reply is ever given: reading a specification closely is all it has to do.
    fn start_exam(&mut self, opening: &str, cancel: &CancellationToken) {
        let stop = cancel.child_token();
        let reply = tokio::spawn(side_request(self.provider.clone(), examiner::brief(opening), Effort::High, self.cfg.max_tokens, LONG_THOUGHT_CHARS, stop.clone()));
        self.exam = ExamState::Pending { reply, asked: Instant::now(), stop };
    }

    /// Runs the examiner's script against the work `finish` was just accepted for, waiting for the
    /// script first if it is still being written. Returns what the worker is told where `finish`
    /// is held; `None` lets it stand.
    async fn examine(&mut self, cancel: &CancellationToken) -> Option<String> {
        if let ExamState::Pending { .. } = self.exam {
            let ExamState::Pending { mut reply, asked, stop } = std::mem::replace(&mut self.exam, ExamState::None) else { return None };
            if !reply.is_finished() {
                self.note("examiner", "waiting for the examiner's checks");
            }
            let joined = tokio::select! {
                joined = tokio::time::timeout(EXAM_WAIT.saturating_sub(asked.elapsed()), &mut reply) => joined.ok().and_then(Result::ok),
                _ = cancel.cancelled() => None,
            };
            let Some((answer, took)) = joined else {
                stop.cancel();
                self.note("examiner", "the examiner did not answer in time: finished without its checks");
                return None;
            };
            let resp = match answer {
                Ok(resp) => resp,
                Err(e) => {
                    self.note("examiner", &format!("the examiner's request failed ({e}): finished without its checks"));
                    return None;
                }
            };
            self.count_side_request(&resp.usage, took);
            let Some(script) = examiner::parse(&resp.content) else {
                self.note("examiner", "the examiner wrote no script for this task");
                return None;
            };
            let dir = self.sess.dir.join("exam");
            let path = dir.join(format!("exam_checks.{}", script.ext));
            if std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, &script.body)).is_err() {
                return None;
            }
            let path = path.to_string_lossy().into_owned();
            let cmd = script.command.replace("{file}", &format!("'{}'", path.replace('\'', "'\\''")));
            self.exam = ExamState::Ready(Exam { cmd, path, holds: 0, shown: None });
        }
        let ExamState::Ready(exam) = &self.exam else { return None };
        let (cmd, path, holds, shown) = (exam.cmd.clone(), exam.path.clone(), exam.holds, exam.shown);
        let root = self.sess.ws.root.clone();
        self.sess.evidence.before_command(&self.sess.ws);
        let run = exec(&mut self.sess, &cmd, &root, StateKind::ReadOnly, EXAM_RUN, cancel).await;
        // Whatever the script left behind (caches) is not a change to the work.
        self.sess.evidence.resync(&self.sess.ws);
        let output = match run {
            Ok(ex) if matches!(ex.end, ExecEnd::Exited(_)) => ex.text,
            Ok(ex) => {
                if let ExecEnd::TimedOut(running) = ex.end {
                    crate::tools::jobs::kill_group(running.pid);
                }
                String::new()
            }
            Err(_) => String::new(),
        };
        let Some(tally) = examiner::tally(&output) else {
            self.exam = ExamState::None;
            self.note("examiner", &format!("the examiner's script did not run to its end and is set aside ({path})"));
            return None;
        };
        if tally.failed == 0 {
            self.exam_note = Some(format!("all {} of the examiner's cases pass", tally.passed));
            self.note("examiner", &format!("all {} of the examiner's cases pass", tally.passed));
            return None;
        }
        if holds == 0 && tally.mostly_failing() {
            self.exam = ExamState::None;
            self.note("examiner", &format!("{} of the examiner's {} cases fail against work whose own checks pass: the script is taken to be wrong and set aside ({path})", tally.failed, tally.total()));
            return None;
        }
        let digest = watches::digest(&output);
        if shown == Some(digest) || holds >= EXAM_HOLDS {
            let left = format!("{} of the examiner's {} cases left failing, as the worker ruled", tally.failed, tally.total());
            self.note("examiner", &left);
            self.exam_note = Some(left);
            return None;
        }
        if let ExamState::Ready(exam) = &mut self.exam {
            exam.holds += 1;
            exam.shown = Some(digest);
        }
        self.note("examiner", &format!("finish held: {} of the examiner's {} cases fail", tally.failed, tally.total()));
        Some(examiner::hold(holds + 1, tally, &path, &cmd, &output))
    }

    fn evidence_text(&self) -> String {
        let ev = &self.sess.evidence;
        match &ev.verified {
            _ if ev.epoch == 0 => "no changes were made".to_string(),
            Some((epoch, by)) if *epoch == ev.epoch => format!("verified by {by}"),
            _ => format!("not checked since {}", ev.last_change),
        }
    }

    /// Trims old observations when the context limit demands it or the price vector says it pays.
    fn maybe_decay(&mut self) {
        let (droppable_bytes, suffix_bytes) = self.ctx.decay_estimate(KEEP_RECENT_MESSAGES);
        let (droppable, suffix) = (droppable_bytes / 4, suffix_bytes / 4);
        if droppable < MIN_DECAY_TOKENS {
            return;
        }
        let over_limit = self.stats.context_tokens > self.cfg.context_limit;
        // With no better information, expect about as many turns again as have already passed.
        let pays = self.sess.prices.is_some_and(|p| p.decay_pays_off(droppable, suffix, self.turn.max(4) as f64));
        if !over_limit && !pays {
            return;
        }
        let d = self.ctx.decay(KEEP_RECENT_MESSAGES);
        // Those windows are gone from the context, so reads must print them again.
        self.sess.seen.clear();
        let reason = if over_limit { "the context limit was reached" } else { "carrying them cost more than re-reading the rest once" };
        self.sink.emit(EventKind::Decay { turn: self.turn, messages: d.messages, dropped_tokens: d.dropped_bytes / 4, reason: reason.to_string() });
    }

    fn stall_notes(&mut self, r: &ScriptResult) -> Vec<String> {
        let mut notes = Vec::new();
        for sig in &r.failed_steps {
            let n = self.failures.entry(*sig).or_insert(0);
            *n += 1;
            if *n == 2 {
                notes.push("The same step has now failed twice in exactly the same way. Do not send it again unchanged: re-read its output and change the approach.".to_string());
            }
        }
        for path in &r.edited {
            let n = self.edits.entry(path.clone()).or_insert(0);
            *n += 1;
            if *n == 6 {
                notes.push(format!("{path} has now been edited 6 times. Step back and state what is actually wrong before changing it again."));
            }
        }
        if self.surprise_streak == 3 {
            notes.push("Three replies in a row hit a surprise. Stop guessing: collect the facts you are missing (read the failing code, print the actual values) before changing anything else.".to_string());
        }
        notes
    }

    /// The reply about to be asked for is the last the budget allows: the turn limit is reached with
    /// it, or the time left would not see another one through. `turns_used` counts those before it.
    fn last_reply(&self, run_started: Instant, turns_used: u32, slowest: Duration) -> bool {
        if turns_used + 1 >= self.cfg.max_turns {
            return true;
        }
        let Some(budget) = self.cfg.time_budget.filter(|_| turns_used > 0) else { return false };
        let elapsed = run_started.elapsed();
        // A reply is expected to take twice the average so far, or as long as the slowest if that is less.
        budget.saturating_sub(elapsed) <= slowest.min(elapsed / turns_used * 2)
    }

    fn budget_notes(&mut self, run_started: Instant, turns_used: u32, slowest: Duration) -> Vec<String> {
        // Said before the last reply, not after it: a run that ends at the limit in the middle of
        // its work leaves nothing behind that says what was done and what was not.
        if self.budget_marks < 3 && self.last_reply(run_started, turns_used, slowest) {
            self.budget_marks = 3;
            return vec![LAST_REPLY_NOTE.to_string()];
        }
        let mut notes = Vec::new();
        // The limit is a safety net there, not something to work towards.
        if self.cfg.features.unhurried {
            return notes;
        }
        let used = match self.cfg.time_budget {
            Some(budget) if !budget.is_zero() => run_started.elapsed().as_secs_f64() / budget.as_secs_f64(),
            _ => 0.0,
        }
        .max(turns_used as f64 / self.cfg.max_turns.max(1) as f64);
        if used >= 0.5 && self.budget_marks == 0 {
            self.budget_marks = 1;
            notes.push("Half of the budget for this task is used.".to_string());
        }
        if used >= 0.8 && self.budget_marks == 1 {
            self.budget_marks = 2;
            notes.push("80% of the budget is used. Wrap up now: make sure the deliverable is in place and checked, then finish.".to_string());
        }
        notes
    }

    fn emit_stats(&self, run_started: Instant, elapsed_before: u64) {
        let mut stats = self.stats.clone();
        stats.elapsed_ms = elapsed_before + run_started.elapsed().as_millis() as u64;
        self.sink.emit(EventKind::Stats { stats });
    }

    /// The first message of a session: the task, after everything the harness can find out before
    /// the model has spent anything (the environment, the files the task names, the result of the
    /// test command it names). With none of that, it is the working directory and the task.
    async fn opening(&mut self, task: &str, cancel: &CancellationToken) -> String {
        let features = self.cfg.features;
        // Before anything of the session's own can be in the workspace. What a check of the work
        // is shown of it is only what changed after this.
        if features.gate || self.goal.is_some() {
            self.sess.baseline = changes::baseline(&self.sess.ws.root).await;
        }
        let mut parts: Vec<String> = Vec::new();
        if features.bootstrap {
            let env = bootstrap::snapshot(&self.sess).await;
            self.sink.emit(EventKind::Env { text: env.clone() });
            parts.push(env);
            // Known from the start, the budget is something to plan with; heard of at half-time, it
            // is only a reason to hurry.
            if features.scripts && !features.unhurried && !self.cfg.interactive {
                let time = self.cfg.time_budget.map(|b| format!(" and {} minutes", (b.as_secs() / 60).max(1))).unwrap_or_default();
                parts.push(format!(
                    "Budget: {} replies{time}. It counts replies, not tool calls: read everything you need in one reply, and send the edits, their check and `finish` together.",
                    self.cfg.max_turns
                ));
            }
        } else {
            // Even the plain loop is told where it works: a model that is not goes looking for the
            // project all over the machine.
            parts.push(format!("cwd: {}", self.sess.ws.root.display()));
        }
        // Before the test command is run: the source lines its failing output points at are then on
        // record as shown wherever they are in an attached file, and are not printed a second time.
        if let Some(files) = attach::attach(&mut self.sess, task) {
            self.note("attach", &files.summary);
            parts.push(files.text);
        }
        // A test command the task names is run before the model has spent anything on it. Only
        // where commands run unasked anyway.
        let preflight = if features.bootstrap && self.cfg.autonomy == Autonomy::Auto { watches::preflight(&mut self.sess, task, cancel).await } else { None };
        if let Some(r) = preflight {
            self.sink.emit(EventKind::Watch { turn: self.turn, name: r.name, cmd: r.cmd, state: r.state, change: r.change, detail: r.detail.clone() });
            parts.push(format!("<tests>\n{}\n</tests>\n{}", r.detail, watches::PREFLIGHT_NOTE));
        }
        parts.push(format!("<task>\n{task}\n</task>"));
        parts.join("\n\n")
    }

    /// Handles one user message: runs model turns until the task is finished, handed back, stopped,
    /// or out of budget.
    pub async fn run(&mut self, text: String, cancel: &CancellationToken) -> Outcome {
        self.run_with(text, &[], cancel).await
    }

    /// `run`, with skills switched on from this message onwards. The system prompt cannot change
    /// once it has been sent, so they travel in front of the message.
    pub async fn run_with(&mut self, text: String, skills: &[SkillInfo], cancel: &CancellationToken) -> Outcome {
        let run_started = Instant::now();
        let elapsed_before = self.stats.elapsed_ms;
        let features = self.cfg.features;
        let task = text.trim().to_string();
        let files = std::mem::take(&mut self.attached);
        let shown = self.shown.take().unwrap_or_else(|| task.clone());
        self.sink.emit(EventKind::User { text: shown, skills: skills.iter().map(|s| s.name.clone()).collect(), files: files.clone() });
        for change in std::mem::take(&mut self.announce) {
            self.sink.emit(change);
        }
        self.sess.task = task.clone();
        self.sess.evidence.message_epoch = self.sess.evidence.epoch;
        let first = std::mem::take(&mut self.first_message);
        // The examiner read the task the session began with: a later message is not held to it.
        if !first {
            self.exam = ExamState::None;
            self.exam_note = None;
        }
        // A skill the task calls for is switched on before the first word is sent to the model.
        let beside = self.provider.answers_beside();
        let routed = if first && features.routing && beside { self.route(&task, cancel).await } else { Vec::new() };
        let skills: Vec<SkillInfo> = skills.iter().cloned().chain(routed).collect();
        let skills = skills.as_slice();
        let mut composed = if first { self.opening(&task, cancel).await } else { task };
        // Its script is a command nobody approved, so it is for sessions whose commands run unasked.
        if first && features.examiner && beside && self.cfg.autonomy == Autonomy::Auto {
            self.start_exam(&composed, cancel);
        }
        if !files.is_empty() {
            composed = format!("<attached_files>\n{}\n</attached_files>\nThe user attached these files to this message. Read the ones you need.\n\n{composed}", files.join("\n"));
        }
        if !skills.is_empty() {
            composed = format!("{}\n\n{composed}", skills::blocks(skills));
        }
        if let Some(note) = self.restored.take() {
            composed = format!("{note}\n\n{composed}");
        }
        self.ctx.push(Message::user(composed));
        self.budget_marks = 0;
        self.save_point(true);

        let (mut turns_used, mut empties, mut nudges_left, mut loop_retries, mut overthinking_left) = (0u32, 0u32, 1u32, 1u32, 3u32);
        // Replies of this run that were nothing but reasoning.
        let mut only_reasoned = 0u32;
        let mut slowest = Duration::ZERO;
        let mut end_checks = EndChecks::default();
        let outcome = loop {
            if cancel.is_cancelled() {
                break Outcome { status: FinishStatus::Stopped, summary: "Stopped by the user.".into(), verified: self.sess.evidence.fresh(), evidence: self.evidence_text() };
            }
            let out_of_time = self.cfg.time_budget.is_some_and(|b| run_started.elapsed() >= b);
            if turns_used >= self.cfg.max_turns || out_of_time {
                let what = if out_of_time { "time budget" } else { "turn limit" };
                break Outcome { status: FinishStatus::Budget, summary: format!("Stopped: the {what} was reached before the task was finished."), verified: self.sess.evidence.fresh(), evidence: self.evidence_text() };
            }
            self.maybe_decay();
            let landing = self.last_reply(run_started, turns_used, slowest);
            let turn_started = Instant::now();
            self.turn += 1;
            turns_used += 1;
            self.stats.turns += 1;
            let turn = self.turn;
            self.sink.emit(EventKind::TurnStart { turn, effort: self.effort.as_str().to_string() });

            // A longer tool list changes the very start of the prompt, just as a rewritten message does.
            let jobs = self.sess.jobs.any_started();
            let tools_stable = std::mem::replace(&mut self.offered_job_tool, jobs) == jobs;
            let prefix_stable = self.ctx.check_prefix(self.provider.wire_messages(&self.ctx.messages)) && tools_stable;
            let request = ChatRequest { messages: &self.ctx.messages, tools: prompt::tool_schemas(&features, jobs, self.cfg.interactive), effort: self.effort, max_tokens: self.cfg.max_tokens, reasoning_cap: if features.long_thought { LONG_THOUGHT_CHARS } else { 0 } };
            let sink = self.sink.clone();
            let mut on_delta = move |d: Delta| match d {
                Delta::Reasoning(text) => sink.emit(EventKind::Delta { turn, channel: Channel::Reasoning, text }),
                Delta::Content(text) => sink.emit(EventKind::Delta { turn, channel: Channel::Content, text }),
                Delta::Retry { attempt, wait_ms, reason } => sink.emit(EventKind::Retry { turn, attempt, wait_ms, reason }),
            };
            let asked = Instant::now();
            let reply = self.provider.chat(request, &mut on_delta, cancel).await;
            let model_ms = asked.elapsed().as_millis() as u64;
            self.stats.model_ms += model_ms;
            let resp = match reply {
                Ok(r) => r,
                Err(ProviderError::Cancelled) => continue,
                Err(ProviderError::Degenerate) if loop_retries > 0 => {
                    loop_retries -= 1;
                    let msg = "Your last reply got stuck repeating itself and was cut off. Do not think it through again: send the next tool call now.";
                    self.note("loop", msg);
                    self.ctx.push(Message::user(format!("[harness] {msg}")));
                    continue;
                }
                Err(ProviderError::Overthinking(chars)) if overthinking_left > 0 => {
                    overthinking_left -= 1;
                    // The stream was cut before the provider reported usage: count what was seen.
                    let tokens = chars as u64 / 4;
                    self.stats.output_tokens += tokens;
                    self.stats.reasoning_tokens += tokens;
                    if let Some(p) = self.sess.prices {
                        self.stats.cost += p.cost(&crate::provider::Usage { output: tokens, reasoning: tokens, ..Default::default() });
                    }
                    let msg = format!("Your last reply was cut off after about {tokens} tokens of thinking with no tool call. Do not work the whole task out in your head: send the first tool calls now and think between steps, with their results in front of you.");
                    self.note("overthinking", &msg);
                    only_reasoned += 1;
                    if let Some(lower) = less_thinking(self.effort, only_reasoned) {
                        self.sink.emit(EventKind::Effort { turn, from: self.effort.as_str().into(), to: lower.as_str().into(), reason: "a reply that only reasoned".into() });
                        self.effort = lower;
                    }
                    self.ctx.push(Message::user(format!("[harness] {msg}")));
                    continue;
                }
                Err(e) => {
                    self.sink.emit(EventKind::Error { message: e.to_string() });
                    break Outcome { status: FinishStatus::Error, summary: e.to_string(), verified: self.sess.evidence.fresh(), evidence: self.evidence_text() };
                }
            };

            self.stats.retries += resp.retries;
            self.stats.prompt_tokens += resp.usage.prompt;
            self.stats.cached_tokens += resp.usage.cached;
            self.stats.output_tokens += resp.usage.output;
            self.stats.reasoning_tokens += resp.usage.reasoning;
            self.stats.context_tokens = resp.usage.prompt;
            let cost = self.sess.prices.map(|p| p.cost(&resp.usage) * peak_multiplier(&self.cfg.model, now_ms() / 1000));
            if let Some(c) = cost {
                self.stats.cost += c;
                self.stats.priced = true;
            }

            let mut raw_calls = resp.calls.clone();
            let mut repairs = 0u32;
            if raw_calls.is_empty() {
                raw_calls = repair::calls_in_text(&resp.content);
                repairs += raw_calls.len() as u32;
            }
            let (mut calls, fixed) = repair::parse_calls(&raw_calls);
            repairs += fixed;
            // A call the reply did not get to the end of may be half a command or half a file: never
            // run it. Either the output limit cut it, or the stream ended without a word and left
            // arguments that only parse once closed.
            let cut = resp.finish == "length" || (resp.finish.is_empty() && raw_calls.last().is_some_and(|c| repair::cut_off(&c.arguments)));
            if cut && let Some(last) = calls.last_mut() {
                let why = if resp.finish == "length" { "hit the output limit" } else { "broke off" };
                last.error = Some(format!("your reply {why} in the middle of this call, so it was not run. Send it again; split large content into smaller steps."));
            }
            self.stats.repairs += repairs;
            self.sink.emit(EventKind::Model {
                turn,
                content: resp.content.clone(),
                reasoning: resp.reasoning.clone(),
                calls: raw_calls.iter().map(|c| CallInfo { id: c.id.clone(), name: c.name.clone(), arguments: c.arguments.clone() }).collect(),
                usage: resp.usage,
                cost,
                finish: resp.finish.clone(),
                ms: model_ms,
                repairs,
            });
            let (expected, verdict) = self.ctx.audit(&resp.usage, resp.usage_reported, prefix_stable);
            match verdict {
                CacheVerdict::Miss => self.stats.cache_misses += 1,
                CacheVerdict::Bust => self.stats.cache_busts += 1,
                _ => {}
            }
            self.sink.emit(EventKind::Cache { turn, prompt: resp.usage.prompt, cached: resp.usage.cached, expected, prefix_stable, verdict });

            self.ctx.push(Message {
                role: Role::Assistant,
                content: resp.content.clone(),
                reasoning: (!resp.reasoning.is_empty()).then(|| resp.reasoning.clone()),
                tool_calls: raw_calls,
                tool_call_id: None,
            });
            // The reply is paid for: a run that dies while its calls run keeps it.
            self.save_point(true);

            if calls.is_empty() {
                // A reply the output limit cut off is not an answer, whatever text it got to.
                let cut_off = resp.finish == "length";
                if resp.content.trim().is_empty() || cut_off {
                    empties += 1;
                    if empties > 2 {
                        let message = "the model returned neither a tool call nor an answer three times in a row".to_string();
                        self.sink.emit(EventKind::Error { message: message.clone() });
                        break Outcome { status: FinishStatus::Error, summary: message, verified: self.sess.evidence.fresh(), evidence: self.evidence_text() };
                    }
                    let msg = if cut_off {
                        // Thinking that fills the whole reply will do so again unless it is given less room.
                        let reasoned = resp.usage.reasoning * 10 >= resp.usage.output * 9;
                        only_reasoned += u32::from(reasoned);
                        if let Some(lower) = less_thinking(self.effort, only_reasoned).filter(|_| reasoned) {
                            self.sink.emit(EventKind::Effort { turn, from: self.effort.as_str().into(), to: lower.as_str().into(), reason: "a reply that only reasoned".into() });
                            self.effort = lower;
                        }
                        "Your reply hit the output limit before any tool call or answer. Keep the reasoning short and send the tool calls."
                    } else {
                        "Your reply contained no tool call and no answer. Continue the task: call a tool, or call finish."
                    };
                    self.note("empty", msg);
                    self.ctx.push(Message::user(format!("[harness] {msg}")));
                    continue;
                }
                // Prose only. With nobody watching, there is one reminder: of the gate where changes
                // are unchecked, and otherwise that a plan announced to nobody is not the work. Not
                // on the last reply of the budget: there the prose is all the report there will be.
                if features.gate && !self.cfg.interactive && nudges_left > 0 && !landing {
                    nudges_left -= 1;
                    let msg = match self.sess.evidence.fresh() {
                        false => format!("You replied without calling finish, and nothing has been checked since {}. Run a check and call finish, or call finish with `unverified` and the reason.", self.sess.evidence.last_change),
                        true => "You replied without a tool call, and nobody is here to read it or to answer. If the task is not done, carry on with tool calls. If that was your answer, call finish with it as the summary.".to_string(),
                    };
                    self.note("gate", &msg);
                    self.ctx.push(Message::user(format!("[harness] {msg}")));
                    continue;
                }
                let outcome = Outcome { status: FinishStatus::Yielded, summary: resp.content, verified: self.sess.evidence.fresh(), evidence: self.evidence_text() };
                match self.before_ending(outcome, landing, &mut end_checks, cancel).await {
                    Ending::End(outcome) => break outcome,
                    Ending::Carry(message) => {
                        self.send_back(&message, run_started, elapsed_before);
                        continue;
                    }
                }
            }
            empties = 0;

            let result = Runner { sess: &mut self.sess, sink: self.sink.as_ref(), cancel, approvals: &self.approvals, stats: &mut self.stats, turn, autonomy: self.cfg.autonomy, interactive: self.cfg.interactive, landing }
                .run(&calls)
                .await;
            slowest = slowest.max(turn_started.elapsed());

            self.surprise_streak = if result.surprised { self.surprise_streak + 1 } else { 0 };
            let mut footer = result.footer.clone();
            let mut notes = self.stall_notes(&result);
            // Advice on how to spend replies is for a run that has some left.
            let closing = landing || self.last_reply(run_started, turns_used, slowest);
            // Models are trained to wait for each result. Show, on their own behaviour, that here
            // they need not: twice at most, then it is their call.
            if features.scripts && self.chain_hints > 0 && result.finished.is_none() && !result.surprised && !closing {
                use crate::tools::Tool;
                let tools: Vec<Tool> = calls.iter().filter_map(|c| c.tool).collect();
                let only_changes = !tools.is_empty() && tools.iter().all(|t| matches!(t, Tool::Edit | Tool::Write));
                let checked = self.sess.evidence.epoch > 0 && self.sess.evidence.fresh();
                let hint = if checked && (only_changes || tools.last() == Some(&Tool::Shell)) {
                    Some(if features.unhurried { COVERAGE_NOTE } else { "Tip: every check passes, so `finish` is accepted now and could have been in the same reply. It is skipped or refused automatically whenever a check fails." })
                } else if only_changes && self.sess.watches.items.is_empty() && !features.unhurried {
                    Some("Tip: send the check (shell with expect_exit) and `finish` together in your next reply: if the check fails, `finish` is skipped automatically. They can ride along with the edits just the same.")
                } else {
                    None
                };
                if let Some(h) = hint {
                    // What passing checks leave open is said once.
                    self.chain_hints = if features.unhurried { 0 } else { self.chain_hints - 1 };
                    notes.push(h.to_string());
                }
            }
            self.solo_streak = if calls.len() == 1 && result.finished.is_none() { self.solo_streak + 1 } else { 0 };
            if features.scripts && !features.unhurried && self.solo_hints > 0 && self.solo_streak >= 4 && !result.surprised && !closing {
                self.solo_hints -= 1;
                self.solo_streak = 0;
                notes.push(format!(
                    "Your last 4 replies carried one tool call each, and every reply counts against the budget ({turns_used} of {} used). Send the calls you can already foresee together in one reply: the script stops by itself at the first surprise.",
                    self.cfg.max_turns
                ));
            }
            notes.extend(self.budget_notes(run_started, turns_used, slowest));
            for n in &notes {
                self.note("harness", n);
            }
            footer.extend(notes);
            for (call_id, content, stub) in result.observations {
                self.ctx.push_tool(&call_id, content, stub);
            }
            if !footer.is_empty() {
                self.ctx.append_to_last(&format!("\n--- harness ---\n{}", footer.join("\n")));
            }
            self.emit_stats(run_started, elapsed_before);
            self.save_point(true);

            // A question ends the run: the answer is the user's next message.
            if let Some(asked) = result.asked {
                self.sink.emit(EventKind::Question { turn, question: asked.question.clone(), choices: asked.choices });
                break Outcome { status: FinishStatus::Yielded, summary: asked.question, verified: self.sess.evidence.fresh(), evidence: self.evidence_text() };
            }
            if let Some(done) = result.finished {
                let outcome = Outcome { status: FinishStatus::Done, summary: done.summary, verified: done.verified, evidence: done.evidence };
                match self.before_ending(outcome, landing, &mut end_checks, cancel).await {
                    Ending::End(outcome) => break outcome,
                    Ending::Carry(message) => {
                        self.send_back(&message, run_started, elapsed_before);
                        continue;
                    }
                }
            }
            if let (EffortPolicy::Ratchet(_), true) = (self.cfg.effort, self.surprise_streak >= 3) && let Some(next) = self.effort.next() {
                self.sink.emit(EventKind::Effort { turn, from: self.effort.as_str().into(), to: next.as_str().into(), reason: "three surprises in a row".into() });
                self.effort = next;
                self.surprise_streak = 0;
            }
        };

        // An examiner nobody will wait for any more is not left running.
        if let ExamState::Pending { stop, .. } = &self.exam {
            stop.cancel();
            self.exam = ExamState::None;
        }
        self.stats.elapsed_ms = elapsed_before + run_started.elapsed().as_millis() as u64;
        self.save_point(false);
        self.sink.emit(EventKind::Stats { stats: self.stats.clone() });
        self.sink.emit(EventKind::Finish { status: outcome.status, summary: outcome.summary.clone(), verified: outcome.verified, evidence: outcome.evidence.clone() });
        outcome
    }

    /// Where a run is about to end normally (`finish` accepted, or an answer in prose): the checks
    /// that can still send it back to work, in order. A check that finds the run not done returns
    /// the message to send the model with; one that does not may note something on the outcome.
    /// Not reached by a run that was stopped, ran out of its budget or failed.
    ///
    /// `landing` is the last reply the budget allows: a check can still be made so that its result
    /// is on record, but nothing can follow it.
    ///
    /// Another check is a method like `goal_check`, with what it needs counted in `EndChecks`, and
    /// a call here after the goal's, made only while nothing has sent the run back yet.
    async fn before_ending(&mut self, mut outcome: Outcome, landing: bool, checks: &mut EndChecks, cancel: &CancellationToken) -> Ending {
        if let Some(message) = self.exam_check(&mut outcome, landing, cancel).await {
            return Ending::Carry(message);
        }
        if let Some(message) = self.goal_check(&mut outcome, landing, checks, cancel).await {
            return Ending::Carry(message);
        }
        Ending::End(outcome)
    }

    /// The examiner's check on a run about to end: its script is run against the work. Returns
    /// what the worker is told where cases fail that it has not ruled on yet; otherwise says on
    /// the outcome how the examination went. Only where the message changed something: an answer
    /// to a question is nobody's work to examine. Not on the last reply: nothing could follow.
    async fn exam_check(&mut self, outcome: &mut Outcome, landing: bool, cancel: &CancellationToken) -> Option<String> {
        let ev = &self.sess.evidence;
        let worked = ev.epoch > ev.message_epoch;
        if !self.cfg.features.examiner || landing || !worked || self.cfg.autonomy != Autonomy::Auto {
            return None;
        }
        let hold = self.examine(cancel).await;
        if hold.is_none() && let Some(note) = &self.exam_note {
            outcome.evidence = format!("{}; {note}", outcome.evidence);
        }
        // Work its author took for finished has a gap its own checks did not show: the next
        // stretch is worth more thought than the last.
        if hold.is_some() && let EffortPolicy::Ratchet(_) = self.cfg.effort && let Some(next) = self.effort.next() {
            self.sink.emit(EventKind::Effort { turn: self.turn, from: self.effort.as_str().into(), to: next.as_str().into(), reason: "the examiner's cases found a gap".into() });
            self.effort = next;
        }
        hold
    }

    /// Sends the model back to work with `message`, as the harness speaking.
    fn send_back(&mut self, message: &str, run_started: Instant, elapsed_before: u64) {
        self.ctx.push(Message::user(format!("[harness] {message}")));
        self.emit_stats(run_started, elapsed_before);
    }

    /// The goal's check, while the session has a goal that is not met: one request to the same
    /// model with a fresh context (see `goal`). Returns the message to send the model back with
    /// when the goal is not met and another reply can follow; otherwise says how it went on the
    /// outcome. A check that cannot be made never fails the run.
    async fn goal_check(&mut self, outcome: &mut Outcome, landing: bool, checks: &mut EndChecks, cancel: &CancellationToken) -> Option<String> {
        let text = self.goal.as_ref().filter(|goal| !goal.met)?.text.clone();
        checks.goal += 1;
        self.stats.goal_checks += 1;
        let changes = changes::changes(&self.sess).await;
        let evidence = if self.cfg.features.gate { self.evidence_text() } else { "not tracked in classic mode".to_string() };
        let material = goal::Material { goal: &text, summary: &outcome.summary, evidence: &evidence, watches: &self.sess.watches, changes: changes.as_deref(), epoch: self.sess.evidence.epoch };
        let asked = Instant::now();
        let answer = goal::ask(&self.provider, &material, cancel).await;
        self.stats.model_ms += asked.elapsed().as_millis() as u64;
        let verdict = match answer {
            Err(ProviderError::Cancelled) => {
                *outcome = Outcome { status: FinishStatus::Stopped, summary: "Stopped by the user.".into(), verified: self.sess.evidence.fresh(), evidence: self.evidence_text() };
                return None;
            }
            Err(e) => Err(e.to_string()),
            Ok(answer) => {
                self.count_check(answer.usage, answer.retries);
                answer.verdict.ok_or_else(|| "its answer did not start with MET or NOT MET".to_string())
            }
        };
        let note = match verdict {
            Ok(Verdict::Met(reasons)) => {
                if let Some(goal) = self.goal.as_mut() {
                    goal.met = true;
                }
                self.sink.emit(goal_event(GoalState::Met, text, &reasons));
                "goal met".to_string()
            }
            Ok(Verdict::NotMet(reasons)) => {
                self.sink.emit(goal_event(GoalState::NotMet, text, &reasons));
                if !landing && checks.goal < goal::MAX_CHECKS {
                    return Some(goal::work_on(&reasons));
                }
                let after = if landing { String::new() } else { format!(" after {} checks", checks.goal) };
                format!("goal not met{after}: {}", one_line(&reasons, 160))
            }
            Err(why) => {
                self.sink.emit(goal_event(GoalState::Unchecked, text, &why));
                format!("goal could not be checked: {}", one_line(&why, 160))
            }
        };
        outcome.evidence = format!("{}; {note}", outcome.evidence);
        None
    }

    /// A check's request is paid for like any other, though it is no turn of the session.
    fn count_check(&mut self, usage: Usage, retries: u32) {
        self.stats.retries += retries;
        self.stats.prompt_tokens += usage.prompt;
        self.stats.cached_tokens += usage.cached;
        self.stats.output_tokens += usage.output;
        self.stats.reasoning_tokens += usage.reasoning;
        if let Some(p) = self.sess.prices {
            self.stats.cost += p.cost(&usage) * peak_multiplier(&self.cfg.model, now_ms() / 1000);
            self.stats.priced = true;
        }
    }
}

fn goal_event(state: GoalState, text: String, detail: &str) -> EventKind {
    EventKind::Goal { state, text, detail: detail.to_string() }
}

/// What a tool call that a restart cut off is answered with.
const INTERRUPTED: &str = "[interrupted by a restart: this call may not have run, or may have run in part. Check the files before relying on it.]";

/// What the model is told in front of the first message of a session that was brought back.
fn restored_note(cut_off: bool, answered: u32) -> String {
    let mut note = "[harness] This session was restored after a restart of the program. Background jobs and shell processes from before are gone, and the files are as they are on disk now, which may differ from what you last saw: read before you rely on what you remember.".to_string();
    if cut_off {
        note.push_str(" The run before this message was cut off by the restart");
        note.push_str(if answered > 0 { ": the tool calls it ended on were not finished, and their results say so." } else { "." });
    }
    note
}

/// Keeping a session so that it can be continued after a restart.
impl Agent {
    /// Stores a snapshot of the session at `path` after every reply and at the end of every run.
    pub fn keep_snapshots(&mut self, path: PathBuf, setup: Setup) {
        self.keep = Some((path, setup));
    }

    /// What continuing this session needs that the workspace, `meta.json` and the log do not hold.
    /// `in_run`: a run is in progress, so that a snapshot that is the last one tells the run was cut off.
    /// Left out on purpose: the read-dedup state and the anchors' histories (files are shown
    /// again), the evidence, the turn's streak counters, what is queued for the next message
    /// (`announce`, `attached`, `shown`) and the provider's cache record. The goal is in `meta.json`.
    pub fn checkpoint(&self, in_run: bool, setup: &Setup) -> Snapshot {
        Snapshot {
            version: resume::VERSION,
            saved: now_ms(),
            in_run,
            setup: setup.clone(),
            model: self.cfg.model.clone(),
            autonomy: if self.cfg.autonomy == Autonomy::Ask { "ask" } else { "auto" }.to_string(),
            interactive: self.cfg.interactive,
            max_turns: self.cfg.max_turns,
            time_budget_secs: self.cfg.time_budget.map(|d| d.as_secs()),
            effort_policy: self.cfg.effort.label(),
            effort: self.effort.as_str().to_string(),
            messages: self.ctx.messages.clone(),
            stubs: self.ctx.stubs().to_vec(),
            turn: self.turn,
            stats: self.stats.clone(),
            edits: self.edits.iter().map(|(path, n)| (path.clone(), *n)).collect(),
            chain_hints: self.chain_hints,
            solo_hints: self.solo_hints,
            session: self.sess.checkpoint(),
        }
    }

    /// Writes the snapshot, if this agent keeps one. A snapshot that cannot be written costs only
    /// the chance to continue after a restart, never the run.
    fn save_point(&self, in_run: bool) {
        let Some((path, setup)) = &self.keep else { return };
        if let Ok(body) = serde_json::to_vec(&self.checkpoint(in_run, setup)) {
            let _ = resume::write_atomic(path, &body);
        }
    }

    /// An agent as the snapshot left it: the conversation, the turn counter and the stats, the
    /// policies the session had, the standing checks, and `goal` from `meta.json`. `cfg` has the
    /// features and the model, which the caller works out; the rest of it is the snapshot's.
    /// Returns the agent and how many tool calls the conversation ended on with no result, which
    /// were answered as interrupted so that a provider accepts it. The first message it gets is told
    /// the session was restored.
    pub fn restore(provider: Provider, mut sess: Session, mut cfg: AgentConfig, sink: Arc<dyn Sink>, approvals: Arc<Approvals>, snap: Snapshot, goal: Option<Goal>) -> (Self, u32) {
        cfg.autonomy = if snap.autonomy == "ask" { Autonomy::Ask } else { Autonomy::Auto };
        cfg.interactive = snap.interactive;
        cfg.max_turns = snap.max_turns;
        cfg.time_budget = snap.time_budget_secs.map(Duration::from_secs);
        if let Some(policy) = EffortPolicy::parse(&snap.effort_policy) {
            cfg.effort = policy;
        }
        sess.restore(snap.session);
        let mut agent = Self::new(provider, sess, cfg, sink, approvals);
        agent.ctx = Context::restored(snap.messages, snap.stubs);
        let answered = agent.ctx.answer_dangling(INTERRUPTED);
        agent.turn = snap.turn;
        agent.effort = Effort::parse(&snap.effort).unwrap_or(agent.effort);
        agent.stats = snap.stats;
        agent.edits = snap.edits.into_iter().collect();
        agent.chain_hints = snap.chain_hints;
        agent.solo_hints = snap.solo_hints;
        agent.first_message = false;
        agent.goal = goal;
        agent.restored = Some(restored_note(snap.in_run, answered));
        (agent, answered)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::anchors::anchor;
    use crate::events::MemorySink;
    use crate::provider::mock::{MockProvider, MockTurn, TurnFn};
    use crate::state::test_session;
    use crate::tools::StepStatus;
    use serde_json::json;

    struct Rig {
        agent: Agent,
        sink: Arc<MemorySink>,
    }

    fn rig(name: &str, cfg: AgentConfig, turns: Vec<TurnFn>) -> Rig {
        // The survey: these workspaces are small, and the tests say what the first message holds.
        let mut cfg = cfg;
        cfg.features.survey = cfg.features.survey && name == "agent-survey";
        let sess = test_session(name, cfg.features);
        std::fs::write(sess.ws.root.join("n.txt"), "alpha\nbeta\ngamma\n").unwrap();
        let sink = Arc::new(MemorySink::default());
        let agent = Agent::new(Provider::Mock(MockProvider::new(turns)), sess, cfg, sink.clone(), Arc::new(Approvals::default()));
        Rig { agent, sink }
    }

    fn fixed(t: MockTurn) -> TurnFn {
        Box::new(move |_| t.clone())
    }

    fn events(r: &Rig) -> Vec<EventKind> {
        r.sink.events.lock().unwrap().clone()
    }

    #[tokio::test]
    async fn the_model_the_effort_and_the_autonomy_can_change_between_two_messages() {
        let mut cfg = AgentConfig::new("first");
        cfg.interactive = true;
        let mut r = rig("agent-retune", cfg, vec![fixed(MockTurn::say("from the first"))]);
        let cancel = CancellationToken::new();
        assert_eq!(r.agent.run("One.".into(), &cancel).await.summary, "from the first");

        // What is already so is not a change.
        r.agent.retune(Retune::default());
        r.agent.retune(Retune { effort: Some(EffortPolicy::Ratchet(Effort::Low)), autonomy: Some(Autonomy::Auto), ..Default::default() });
        let second = Provider::Mock(MockProvider::new(vec![fixed(MockTurn::say("from the second"))]));
        r.agent.retune(Retune { provider: Some((second, "other".into(), "second".into())), effort: Some(EffortPolicy::Fixed(Effort::Max)), autonomy: Some(Autonomy::Ask), ..Default::default() });
        assert_eq!(r.agent.run("Two.".into(), &cancel).await.summary, "from the second");

        // The second model is sent the whole conversation, and none of it is expected to be cached there.
        let sent = requests(&r);
        assert_eq!(sent.len(), 1);
        assert!(sent[0][1].content.contains("One.") && sent[0][2].content == "from the first" && sent[0].last().unwrap().content == "Two.", "{:?}", sent[0]);
        let ev = events(&r);
        let verdicts: Vec<CacheVerdict> = ev.iter().filter_map(|e| if let EventKind::Cache { verdict, .. } = e { Some(*verdict) } else { None }).collect();
        assert_eq!(verdicts, vec![CacheVerdict::First, CacheVerdict::First]);
        assert_eq!(r.agent.stats.cache_misses + r.agent.stats.cache_busts, 0);

        // What changed is said once, after the message it came with, and holds from its first turn.
        let at = ev.iter().position(|e| matches!(e, EventKind::User { text, .. } if text == "Two.")).unwrap();
        assert!(matches!(&ev[at + 1], EventKind::Settings { provider: Some(p), model: Some(m), effort: Some(e), autonomy: Some(a) } if p == "other" && m == "second" && e == "max" && a == "ask"), "{:?}", ev[at + 1]);
        assert_eq!(ev.iter().filter(|e| matches!(e, EventKind::Settings { .. })).count(), 1);
        assert!(matches!(ev.iter().rfind(|e| matches!(e, EventKind::TurnStart { .. })), Some(EventKind::TurnStart { effort, .. }) if effort == "max"));

        // Files attached to a message go with that message alone, by path and in front of it.
        r.agent.retune(Retune { provider: Some((Provider::Mock(MockProvider::new(vec![fixed(MockTurn::say("read")), fixed(MockTurn::say("again"))])), "other".into(), "third".into())), ..Default::default() });
        r.agent.attach(vec!["/tmp/notes.txt".into()]);
        r.agent.run("Three.".into(), &cancel).await;
        r.agent.run("Four.".into(), &cancel).await;
        let sent = requests(&r);
        assert!(sent[0].last().unwrap().content.starts_with("<attached_files>\n/tmp/notes.txt\n</attached_files>") && sent[0].last().unwrap().content.ends_with("Three."), "{:?}", sent[0].last());
        assert_eq!(sent[1].last().unwrap().content, "Four.");
        let attached: Vec<Vec<String>> = events(&r).iter().filter_map(|e| if let EventKind::User { files, .. } = e { Some(files.clone()) } else { None }).collect();
        assert_eq!(attached, vec![vec![], vec![], vec!["/tmp/notes.txt".to_string()], vec![]]);
    }

    fn requests(r: &Rig) -> Vec<Vec<Message>> {
        match &*r.agent.provider {
            Provider::Mock(m) => m.requests.lock().unwrap().clone(),
            _ => unreachable!(),
        }
    }

    #[tokio::test]
    async fn a_fix_takes_two_replies_and_the_prefix_stays_cached() {
        let cfg = AgentConfig::new("mock");
        let mut r = rig("agent-happy", cfg, vec![]);
        let root = r.agent.sess.ws.root.clone();
        let turns: Vec<TurnFn> = vec![
            fixed(MockTurn::calls("look first", vec![("read", json!({"path": "n.txt"})), ("shell", json!({"cmd": "grep -qx BETA n.txt", "expect": {"exit": "nonzero"}}))])),
            Box::new(move |_| {
                let text = std::fs::read_to_string(root.join("n.txt")).unwrap();
                let line = text.lines().nth(1).unwrap();
                MockTurn::calls(
                    "fix, check, finish",
                    vec![
                        ("edit", json!({"path": "n.txt", "range": anchor(2, line), "new": "BETA"})),
                        ("shell", json!({"cmd": "grep -qx BETA n.txt && seq 1 40", "expect": {"exit": "0", "has": "40"}})),
                        ("finish", json!({"summary": "upper-cased beta"})),
                    ],
                )
            }),
        ];
        r.agent.provider = Arc::new(Provider::Mock(MockProvider::new(turns)));

        let out = r.agent.run("Upper-case the second line of n.txt.".into(), &CancellationToken::new()).await;
        assert_eq!(out.status, FinishStatus::Done);
        assert!(out.verified);
        assert_eq!(out.evidence, "verified by `grep -qx BETA n.txt && seq 1 40`");
        assert_eq!(std::fs::read_to_string(r.agent.sess.ws.root.join("n.txt")).unwrap(), "alpha\nBETA\ngamma\n");
        assert_eq!((r.agent.stats.turns, r.agent.stats.steps, r.agent.stats.surprises, r.agent.stats.predictions_held), (2, 5, 0, 2));

        let reqs = requests(&r);
        let first_user = &reqs[0][1].content;
        assert!(first_user.starts_with("<env>\ncwd: ") && first_user.ends_with("<task>\nUpper-case the second line of n.txt.\n</task>"), "{first_user}");
        // The second request is the first plus appended messages, nothing rewritten.
        assert_eq!(&reqs[1][..reqs[0].len()], &reqs[0][..]);
        // Reasoning is replayed verbatim; the confirmed check came back as a receipt, not 40 lines.
        assert_eq!(reqs[1][2].reasoning.as_deref(), Some("look first"));
        let last_tools: Vec<&Message> = r.agent.ctx.messages.iter().filter(|m| m.role == Role::Tool).collect();
        assert!(last_tools[3].content.contains("40 lines not shown, your prediction held"), "{}", last_tools[3].content);

        let ev = events(&r);
        let verdicts: Vec<(bool, CacheVerdict)> = ev.iter().filter_map(|e| if let EventKind::Cache { prefix_stable, verdict, .. } = e { Some((*prefix_stable, *verdict)) } else { None }).collect();
        assert_eq!(verdicts, vec![(true, CacheVerdict::First), (true, CacheVerdict::Hit)]);
        assert!(matches!(ev.last(), Some(EventKind::Finish { status: FinishStatus::Done, verified: true, .. })));
        assert!(ev.iter().any(|e| matches!(e, EventKind::Env { .. })));
        assert!(ev.iter().any(|e| matches!(e, EventKind::Delta { channel: Channel::Reasoning, .. })));
    }

    #[tokio::test]
    async fn a_wrong_fix_stops_the_script_and_the_next_reply_recovers() {
        let turns = vec![
            fixed(MockTurn::calls("", vec![("edit", json!({"path": "n.txt", "old": "beta", "new": "Beta"})), ("shell", json!({"cmd": "grep -qx BETA n.txt", "expect": {"exit": "0"}})), ("finish", json!({"summary": "done"}))])),
            fixed(MockTurn::calls("", vec![("edit", json!({"path": "n.txt", "old": "Beta", "new": "BETA"})), ("shell", json!({"cmd": "grep -qx BETA n.txt", "expect": {"exit": "0"}})), ("finish", json!({"summary": "done properly"}))])),
        ];
        let mut r = rig("agent-recover", AgentConfig::new("mock"), turns);
        let out = r.agent.run("fix".into(), &CancellationToken::new()).await;
        assert_eq!((out.status, out.summary.as_str()), (FinishStatus::Done, "done properly"));
        assert_eq!((r.agent.stats.turns, r.agent.stats.surprises, r.agent.stats.skipped), (2, 1, 1));
        let statuses: Vec<StepStatus> = events(&r).iter().filter_map(|e| if let EventKind::StepEnd { status, .. } = e { Some(*status) } else { None }).collect();
        assert_eq!(statuses, vec![StepStatus::Ok, StepStatus::Surprise, StepStatus::Skipped, StepStatus::Ok, StepStatus::Ok, StepStatus::Ok]);
    }

    #[tokio::test]
    async fn a_question_ends_the_run_and_the_answer_is_the_next_message() {
        let ask = json!({"question": "Which language?", "options": [{"label": "Rust", "hint": "Recommended for performance"}, {"label": "Python", "hint": "Recommended to get it done quickly"}]});
        let turns = vec![
            // What comes after the question in the same reply is not run.
            fixed(MockTurn::calls("", vec![("ask", ask.clone()), ("write", json!({"path": "main.rs", "content": "fn main() {}\n"}))])),
            fixed(MockTurn::calls("", vec![("write", json!({"path": "main.py", "content": "print(1)\n"})), ("finish", json!({"summary": "done", "unverified": "a stub"}))])),
        ];
        let mut cfg = AgentConfig::new("mock");
        cfg.interactive = true;
        let mut r = rig("agent-ask", cfg, turns);
        let out = r.agent.run("make me a tool".into(), &CancellationToken::new()).await;
        assert_eq!((out.status, out.summary.as_str(), r.agent.stats.turns), (FinishStatus::Yielded, "Which language?", 1));
        let asked = events(&r).into_iter().find_map(|e| match e {
            EventKind::Question { question, choices, .. } => Some((question, choices)),
            _ => None,
        });
        let (question, choices) = asked.expect("the question is put to the user");
        assert_eq!((question.as_str(), choices.len(), choices[0].hint.as_str()), ("Which language?", 2, "Recommended for performance"));
        assert!(!r.agent.sess.ws.root.join("main.rs").exists());
        let out = r.agent.run("Python".into(), &CancellationToken::new()).await;
        assert_eq!(out.status, FinishStatus::Done);

        // With nobody there, the model is told to decide, and the run goes on.
        let turns = vec![fixed(MockTurn::calls("", vec![("ask", ask)])), finish()];
        let mut r = rig("agent-ask-alone", AgentConfig::new("mock"), turns);
        let out = r.agent.run("make me a tool".into(), &CancellationToken::new()).await;
        assert_eq!(out.status, FinishStatus::Done);
        assert!(!events(&r).iter().any(|e| matches!(e, EventKind::Question { .. })));
    }

    #[tokio::test]
    async fn prose_with_unchecked_changes_gets_one_reminder_then_is_accepted_as_unverified() {
        let turns = vec![
            fixed(MockTurn::calls("", vec![("edit", json!({"path": "n.txt", "old": "beta", "new": "BETA"}))])),
            fixed(MockTurn::say("All done.")),
            fixed(MockTurn::say("Really done.")),
        ];
        let mut r = rig("agent-nudge", AgentConfig::new("mock"), turns);
        let out = r.agent.run("fix".into(), &CancellationToken::new()).await;
        assert_eq!((out.status, out.verified), (FinishStatus::Yielded, false));
        assert_eq!(out.evidence, "not checked since the edit of n.txt");
        let nudges: Vec<&Message> = r.agent.ctx.messages.iter().filter(|m| m.role == Role::User && m.content.starts_with("[harness] You replied without calling finish")).collect();
        assert_eq!(nudges.len(), 1);
        // The lone edit also drew the reminder that the check and finish could have ridden along.
        let tool = r.agent.ctx.messages.iter().find(|m| m.role == Role::Tool).unwrap();
        assert!(tool.content.contains("--- harness ---\nTip: send the check"), "{}", tool.content);

        // With a person watching, prose simply hands the turn back.
        let mut cfg = AgentConfig::new("mock");
        cfg.interactive = true;
        let mut r = rig("agent-interactive", cfg, vec![fixed(MockTurn::calls("", vec![("edit", json!({"path": "n.txt", "old": "beta", "new": "B"}))])), fixed(MockTurn::say("Changed it. Want tests too?"))]);
        let out = r.agent.run("fix".into(), &CancellationToken::new()).await;
        assert_eq!((out.status, r.agent.stats.turns), (FinishStatus::Yielded, 2));
    }

    #[tokio::test]
    async fn unattended_a_plan_in_prose_or_a_reply_cut_off_is_not_taken_for_the_answer() {
        let plan = MockTurn::say("I'll start by reading the file.");
        let cut = MockTurn { finish: Some("length".into()), ..MockTurn::say("First I will") };
        let finish = MockTurn::calls("", vec![("finish", json!({"summary": "three lines"}))]);
        let mut r = rig("agent-plan", AgentConfig::new("mock"), vec![fixed(plan), fixed(cut), fixed(finish)]);
        let out = r.agent.run("How many lines has n.txt?".into(), &CancellationToken::new()).await;
        assert_eq!((out.status, out.summary.as_str(), r.agent.stats.turns), (FinishStatus::Done, "three lines", 3));
        let said: Vec<&str> = r.agent.ctx.messages.iter().filter(|m| m.role == Role::User).map(|m| m.content.as_str()).collect();
        assert!(said[1].starts_with("[harness] You replied without a tool call, and nobody is here") && said[2].starts_with("[harness] Your reply hit the output limit"), "{said:?}");
    }

    #[tokio::test]
    async fn a_call_cut_off_by_the_output_limit_is_never_executed() {
        let mut cut = MockTurn::calls("", vec![("shell", json!({"cmd": "echo first"})), ("write", json!({"path": "big.txt", "content": "half a fi"}))]);
        cut.finish = Some("length".into());
        let mut r = rig("agent-length", AgentConfig::new("mock"), vec![fixed(cut), fixed(MockTurn::say("ok"))]);
        r.agent.run("write".into(), &CancellationToken::new()).await;
        assert!(!r.agent.sess.ws.root.join("big.txt").exists());
        let tools: Vec<&Message> = r.agent.ctx.messages.iter().filter(|m| m.role == Role::Tool).collect();
        assert!(tools[0].content.contains("first"));
        assert!(tools[1].content.contains("hit the output limit in the middle of this call"));
    }

    #[tokio::test]
    async fn stops_on_cancel_and_on_the_turn_limit() {
        let mut r = rig("agent-stop", AgentConfig::new("mock"), vec![]);
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert_eq!(r.agent.run("x".into(), &cancel).await.status, FinishStatus::Stopped);

        let mut cfg = AgentConfig::new("mock");
        cfg.max_turns = 4;
        let spin = || fixed(MockTurn::calls("", vec![("shell", json!({"cmd": "true"}))]));
        let mut r = rig("agent-budget", cfg, vec![spin(), spin(), spin(), spin(), spin()]);
        let out = r.agent.run("x".into(), &CancellationToken::new()).await;
        assert_eq!((out.status, r.agent.stats.turns), (FinishStatus::Budget, 4));
        let notes: Vec<String> = events(&r).iter().filter_map(|e| if let EventKind::Note { kind, text, .. } = e { (kind == "harness").then(|| text.clone()) } else { None }).collect();
        assert_eq!(notes, ["Half of the budget for this task is used.", LAST_REPLY_NOTE], "the last reply is announced once, before it");
        assert!(requests(&r)[3].last().unwrap().content.ends_with(&format!("\n--- harness ---\n{LAST_REPLY_NOTE}")));
    }

    #[tokio::test]
    async fn the_last_reply_of_the_budget_is_the_report_whatever_it_holds() {
        let edit = || fixed(MockTurn::calls("", vec![("edit", json!({"path": "n.txt", "old": "beta", "new": "BETA"}))]));
        let two_replies = || {
            let mut cfg = AgentConfig::new("mock");
            cfg.max_turns = 2;
            cfg
        };

        // A check that fails in front of it: `finish` is heard all the same.
        let last = fixed(MockTurn::calls("", vec![("shell", json!({"cmd": "grep -qx GAMMA n.txt", "expect_exit": "0"})), ("finish", json!({"summary": "beta is done, gamma is not"}))]));
        let mut r = rig("agent-landing-failed", two_replies(), vec![edit(), last]);
        let out = r.agent.run("Upper-case lines two and three of n.txt.".into(), &CancellationToken::new()).await;
        assert_eq!((out.status, out.verified, out.summary.as_str()), (FinishStatus::Done, false, "beta is done, gamma is not"));
        assert_eq!(out.evidence, "unverified: nothing was checked after the edit of n.txt before the budget ran out");

        // Checked work is accepted as verified.
        let last = fixed(MockTurn::calls("", vec![("shell", json!({"cmd": "grep -qx BETA n.txt", "expect_exit": "0"})), ("finish", json!({"summary": "done"}))]));
        let mut r = rig("agent-landed", two_replies(), vec![edit(), last]);
        let out = r.agent.run("Upper-case the second line of n.txt.".into(), &CancellationToken::new()).await;
        assert_eq!((out.status, out.verified, out.evidence.as_str()), (FinishStatus::Done, true, "verified by `grep -qx BETA n.txt`"));

        // Prose is not answered with a reminder that there is no reply left to act on.
        let mut r = rig("agent-landing-prose", two_replies(), vec![edit(), fixed(MockTurn::say("Line two is done; line three is not."))]);
        let out = r.agent.run("Upper-case lines two and three of n.txt.".into(), &CancellationToken::new()).await;
        assert_eq!((out.status, out.verified, out.summary.as_str()), (FinishStatus::Yielded, false, "Line two is done; line three is not."));
    }

    #[tokio::test]
    async fn repeated_failures_draw_a_note_and_three_surprises_raise_the_effort() {
        let fail = || fixed(MockTurn::calls("", vec![("shell", json!({"cmd": "grep -q nope n.txt", "expect": {"exit": "0"}}))]));
        let mut r = rig("agent-stall", AgentConfig::new("mock"), vec![fail(), fail(), fail(), fixed(MockTurn::say("giving up"))]);
        assert_eq!(r.agent.effort(), Effort::Low);
        r.agent.run("x".into(), &CancellationToken::new()).await;
        assert_eq!(r.agent.effort(), Effort::High);
        let ev = events(&r);
        assert!(ev.iter().any(|e| matches!(e, EventKind::Effort { from, to, .. } if from == "low" && to == "high")));
        let notes: Vec<&str> = ev.iter().filter_map(|e| if let EventKind::Note { text, .. } = e { Some(text.as_str()) } else { None }).collect();
        assert!(notes.iter().any(|n| n.starts_with("The same step has now failed twice")), "{notes:?}");
        assert!(notes.iter().any(|n| n.starts_with("Three replies in a row")));
        // The note reaches the model as a footer on the observation, not as a rewritten message.
        let tools: Vec<&Message> = r.agent.ctx.messages.iter().filter(|m| m.role == Role::Tool).collect();
        assert!(tools[1].content.contains("\n--- harness ---\nThe same step has now failed twice"), "{}", tools[1].content);
    }

    #[tokio::test]
    async fn the_job_tool_is_offered_once_a_job_exists_at_the_price_of_one_cache_restart() {
        let turns = vec![
            fixed(MockTurn::calls("", vec![("shell", json!({"cmd": "sleep 30", "background": true}))])),
            fixed(MockTurn::calls("", vec![("job", json!({"id": "bg:1", "kill": true}))])),
            fixed(MockTurn::calls("", vec![("finish", json!({"summary": "Started it and stopped it again."}))])),
        ];
        let mut r = rig("agent-jobs", AgentConfig::new("mock"), turns);
        r.agent.run("start the server, then stop it".into(), &CancellationToken::new()).await;
        let cache: Vec<(bool, CacheVerdict)> = events(&r).iter().filter_map(|e| if let EventKind::Cache { prefix_stable, verdict, .. } = e { Some((*prefix_stable, *verdict)) } else { None }).collect();
        assert_eq!(cache, vec![(true, CacheVerdict::First), (false, CacheVerdict::Bust), (true, CacheVerdict::Hit)]);
        assert_eq!(r.agent.stats.cache_busts, 1);
    }

    #[tokio::test]
    async fn the_test_command_the_task_names_is_checked_before_the_first_turn() {
        let turns = || {
            vec![
                fixed(MockTurn::calls("", vec![("edit", json!({"path": "n.txt", "old": "beta", "new": "BETA"}))])),
                fixed(MockTurn::calls("", vec![("finish", json!({"summary": "upper-cased beta"}))])),
            ]
        };
        let check = "grep -qx BETA n.txt && echo PASS || { echo 'FAIL: no BETA line'; exit 1; }\n";
        let task = "`sh check.sh` fails. Upper-case the second line of n.txt.";

        let mut r = rig("agent-preflight", AgentConfig::new("mock"), turns());
        std::fs::write(r.agent.sess.ws.root.join("check.sh"), check).unwrap();
        let out = r.agent.run(task.into(), &CancellationToken::new()).await;
        assert_eq!((out.status, out.verified, out.evidence.as_str()), (FinishStatus::Done, true, "verified by watch tests"));
        assert_eq!(r.agent.stats.turns, 2, "no turn was spent on running the check");
        // The model starts from the failing output, and its edit alone comes back with the verdict.
        let first = &requests(&r)[0][1].content;
        assert!(first.contains("\n\n<tests>\nwatch tests: FAIL (expected exit 0, got exit 1)\nFAIL: no BETA line\n</tests>\nThe harness ran the test command"), "{first}");
        assert!(first.starts_with("<env>\ncwd: ") && first.ends_with("</task>"));
        let tool = r.agent.ctx.messages.iter().find(|m| m.role == Role::Tool).unwrap();
        assert!(tool.content.contains("--- harness ---\nwatch tests: FAIL → PASS · PASS\nTip: every check passes"), "{}", tool.content);

        // Where every command waits for approval, none is run unasked.
        let mut cfg = AgentConfig::new("mock");
        cfg.autonomy = Autonomy::Ask;
        let mut r = rig("agent-preflight-ask", cfg, turns());
        std::fs::write(r.agent.sess.ws.root.join("check.sh"), check).unwrap();
        r.agent.run(task.into(), &CancellationToken::new()).await;
        assert!(!requests(&r)[0][1].content.contains("<tests>"));
    }

    #[tokio::test]
    async fn the_files_the_task_names_are_in_the_first_message_and_can_be_edited_at_once() {
        let (a1, a2, a3) = (anchor(1, "alpha"), anchor(2, "beta"), anchor(3, "gamma"));
        let task = "Upper-case the second line of `n.txt`.";
        let fix = || fixed(MockTurn::calls("", vec![("edit", json!({"path": "n.txt", "range": anchor(2, "beta"), "new": "BETA"})), ("shell", json!({"cmd": "grep -qx BETA n.txt", "expect_exit": "0"})), ("finish", json!({"summary": "upper-cased beta"}))]));
        let read = || fixed(MockTurn::calls("", vec![("read", json!({"path": "n.txt"}))]));
        let first_result = |r: &Rig| r.agent.ctx.messages.iter().find(|m| m.role == Role::Tool).unwrap().content.clone();

        let mut r = rig("agent-attach", AgentConfig::new("mock"), vec![fix()]);
        let out = r.agent.run(task.into(), &CancellationToken::new()).await;
        assert_eq!((out.status, out.verified, r.agent.stats.turns), (FinishStatus::Done, true, 1), "nothing had to be read first");
        assert_eq!(std::fs::read_to_string(r.agent.sess.ws.root.join("n.txt")).unwrap(), "alpha\nBETA\ngamma\n");
        // The file is there as `read` prints it, between the environment and the task.
        let first = &requests(&r)[0][1].content;
        let files = format!("\n</env>\n\nBudget: 80 replies. It counts replies, not tool calls: read everything you need in one reply, and send the edits, their check and `finish` together.\n\n<files>\nn.txt · 3 lines\n{a1}|alpha\n{a2}|beta\n{a3}|gamma\n</files>\nThis is `read` output for the file the task names. It is already read and can be edited directly: do not read it first.\n\n<task>\n{task}\n</task>");
        assert!(first.starts_with("<env>\ncwd: ") && first.ends_with(&files), "{first}");
        let ev = events(&r);
        assert!(ev.iter().any(|e| matches!(e, EventKind::Note { turn: 0, kind, text } if kind == "attach" && text == "attached n.txt (3 lines)")));
        let tools: Vec<String> = ev.iter().filter_map(|e| if let EventKind::StepEnd { tool, .. } = e { Some(tool.clone()) } else { None }).collect();
        assert_eq!(tools, ["edit", "shell", "finish"]);

        // A model that asks for the file all the same is told that it has it.
        let mut r = rig("agent-attach-reread", AgentConfig::new("mock"), vec![read(), fixed(MockTurn::say("ok"))]);
        r.agent.run(task.into(), &CancellationToken::new()).await;
        assert!(first_result(&r).starts_with("[unchanged] n.txt lines 1-3 are already in your context (shown in the first message)"), "{}", first_result(&r));

        // Switched off, the first message is what it used to be and the file has to be asked for.
        let mut cfg = AgentConfig::new("mock");
        cfg.features = Features::ecosage().without("attach").unwrap();
        let mut r = rig("agent-no-attach", cfg, vec![read(), fix()]);
        let out = r.agent.run(task.into(), &CancellationToken::new()).await;
        assert_eq!((out.status, r.agent.stats.turns), (FinishStatus::Done, 2));
        let env = events(&r).iter().find_map(|e| if let EventKind::Env { text } = e { Some(text.clone()) } else { None }).unwrap();
        assert_eq!(requests(&r)[0][1].content, format!("{env}\n\nBudget: 80 replies. It counts replies, not tool calls: read everything you need in one reply, and send the edits, their check and `finish` together.\n\n<task>\n{task}\n</task>"));
        assert!(first_result(&r).starts_with("n.txt · 3 lines\n"), "{}", first_result(&r));
        assert!(!events(&r).iter().any(|e| matches!(e, EventKind::Note { kind, .. } if kind == "attach")));
    }

    #[tokio::test]
    async fn an_attached_file_and_the_failing_output_do_not_repeat_each_other() {
        let turns = || vec![fixed(MockTurn::calls("", vec![("edit", json!({"path": "n.txt", "old": "beta", "new": "BETA"})), ("finish", json!({"summary": "upper-cased beta"}))]))];
        // The check points at the line that is wrong, and at one in a file the task does not name.
        let check = "grep -qx BETA n.txt && echo PASS || { echo 'n.txt:2: expected BETA'; echo 'see rules.txt:1'; exit 1; }\n";
        let task = "`sh check.sh` fails. Upper-case the second line of `n.txt`.";
        let beta = format!("{}|beta", anchor(2, "beta"));
        let prepare = |r: &Rig| {
            std::fs::write(r.agent.sess.ws.root.join("check.sh"), check).unwrap();
            std::fs::write(r.agent.sess.ws.root.join("rules.txt"), "second line in capitals\n").unwrap();
        };

        let mut r = rig("agent-attach-preflight", AgentConfig::new("mock"), turns());
        prepare(&r);
        let out = r.agent.run(task.into(), &CancellationToken::new()).await;
        assert_eq!((out.status, out.evidence.as_str(), r.agent.stats.turns), (FinishStatus::Done, "verified by watch tests", 1), "the fix and `finish` in the first reply");
        // The named file comes whole; of what the output points at, only the other file is added.
        let first = &requests(&r)[0][1].content;
        let (files, tests) = (first.find("\n\n<files>\nn.txt · 3 lines\n").expect("attached"), first.find("\n\n<tests>\nwatch tests: FAIL (expected exit 0, got exit 1)\nn.txt:2: expected BETA\n").expect("checked"));
        assert!(files < tests && first.ends_with("</task>"), "{first}");
        assert_eq!(first.matches(&beta).count(), 1, "{first}");
        assert!(first.contains(&format!("\n--- where (current file content; anchors work in edit) ---\nrules.txt\n> {}|second line in capitals\n</tests>\n", anchor(1, "second line in capitals"))), "{first}");

        // Without the attachment the lines come once as well, with the failing output.
        let mut cfg = AgentConfig::new("mock");
        cfg.features = Features::ecosage().without("attach").unwrap();
        let mut r = rig("agent-preflight-alone", cfg, turns());
        prepare(&r);
        r.agent.run(task.into(), &CancellationToken::new()).await;
        let first = &requests(&r)[0][1].content;
        assert!(!first.contains("<files>") && first.contains(&format!("\nn.txt\n  {}|alpha\n> {beta}\n", anchor(1, "alpha"))), "{first}");
        assert_eq!(first.matches(&beta).count(), 1, "{first}");
    }

    #[tokio::test]
    async fn a_traceback_reaches_the_model_without_the_workspace_path_and_with_its_source_lines() {
        let trace = "echo 'Traceback (most recent call last):'; echo \"  File \\\"$PWD/calc.py\\\", line 12, in f\"; echo ZeroDivisionError; exit 1";
        let turns = || vec![fixed(MockTurn::calls("", vec![("shell", json!({"cmd": trace, "expect_exit": "0"}))])), fixed(MockTurn::say("I see."))];
        let body: String = (1..=30).map(|i| format!("line_{i} = {i}\n")).collect();
        let target = format!("\n> {}|line_12 = 12\n", anchor(12, "line_12 = 12"));
        // What the model was sent for the step, in the request that followed it.
        let observed = |r: &Rig| requests(r)[1].last().unwrap().content.clone();

        let mut r = rig("agent-short-paths", AgentConfig::new("mock"), turns());
        let root = r.agent.sess.ws.root.display().to_string();
        std::fs::write(r.agent.sess.ws.root.join("calc.py"), &body).unwrap();
        r.agent.run("Run it.".into(), &CancellationToken::new()).await;
        let seen = observed(&r);
        assert!(seen.starts_with("[SURPRISE] expected exit 0, got exit 1 · "), "{seen}");
        assert!(seen.contains("\nTraceback (most recent call last):\n  File \"calc.py\", line 12, in f\nZeroDivisionError\n--- where (current file content; anchors work in edit) ---\ncalc.py\n") && seen.contains(&target), "{seen}");
        assert!(!seen.contains(&root), "{seen}");
        // What is kept on disk is what the command printed.
        let kept = std::fs::read_to_string(r.agent.sess.spill_path("out:1").unwrap()).unwrap();
        assert!(kept.contains(&format!("  File \"{root}/calc.py\", line 12, in f")), "{kept}");

        // Switched off, the path is there in full, and the lines are found all the same.
        let mut cfg = AgentConfig::new("mock");
        cfg.features = Features::ecosage().without("short-paths").unwrap();
        let mut r = rig("agent-long-paths", cfg, turns());
        let root = r.agent.sess.ws.root.display().to_string();
        std::fs::write(r.agent.sess.ws.root.join("calc.py"), &body).unwrap();
        r.agent.run("Run it.".into(), &CancellationToken::new()).await;
        let seen = observed(&r);
        assert!(seen.contains(&format!("\n  File \"{root}/calc.py\", line 12, in f\n")) && seen.contains(&target), "{seen}");
    }

    #[tokio::test]
    async fn classic_mode_is_a_plain_loop() {
        let mut cfg = AgentConfig::new("mock");
        cfg.features = Features::classic();
        let turns = vec![fixed(MockTurn::calls("", vec![("edit", json!({"path": "n.txt", "old": "beta", "new": "BETA"})), ("finish", json!({"summary": "done"}))]))];
        let mut r = rig("agent-classic", cfg, turns);
        let out = r.agent.run("fix".into(), &CancellationToken::new()).await;
        assert_eq!((out.status, out.verified), (FinishStatus::Done, false));
        let reqs = requests(&r);
        assert_eq!(reqs[0][0].content, prompt::SYSTEM_CLASSIC);
        let root = r.agent.sess.ws.root.display().to_string();
        assert_eq!(reqs[0][1].content, format!("cwd: {root}\n\n<task>\nfix\n</task>"), "no environment snapshot in the baseline, only where it works");
    }

    #[test]
    fn effort_policy_parses() {
        assert_eq!(EffortPolicy::parse("ratchet"), Some(EffortPolicy::Ratchet(Effort::Low)));
        assert_eq!(EffortPolicy::parse("ratchet-high"), Some(EffortPolicy::Ratchet(Effort::High)));
        assert_eq!(EffortPolicy::parse("MAX"), Some(EffortPolicy::Fixed(Effort::Max)));
        assert_eq!(EffortPolicy::parse("fast"), None);
        assert_eq!(EffortPolicy::Ratchet(Effort::Low).label(), "ratchet-low");
    }

    #[test]
    fn a_reply_that_only_reasoned_costs_a_level_but_thinking_is_not_switched_off_at_the_first() {
        assert_eq!((less_thinking(Effort::Max, 1), less_thinking(Effort::High, 1), less_thinking(Effort::High, 2)), (Some(Effort::High), Some(Effort::Low), Some(Effort::Low)));
        // Where the ratchet starts, the first one changes nothing; the second switches thinking off.
        assert_eq!((less_thinking(Effort::Low, 1), less_thinking(Effort::Low, 2), less_thinking(Effort::Off, 3)), (None, Some(Effort::Off), None));
    }

    fn shapes(r: &Rig) -> Vec<crate::provider::mock::Shape> {
        match &*r.agent.provider {
            Provider::Mock(m) => m.shapes.lock().unwrap().clone(),
            _ => unreachable!(),
        }
    }

    /// A reply that edits a line of n.txt, checks it and finishes.
    fn upper(old: &str, new: &str) -> TurnFn {
        fixed(MockTurn::calls("", vec![("edit", json!({"path": "n.txt", "old": old, "new": new})), ("shell", json!({"cmd": format!("grep -qx {new} n.txt"), "expect_exit": "0"})), ("finish", json!({"summary": format!("{new} done")}))]))
    }

    fn finish() -> TurnFn {
        fixed(MockTurn::calls("", vec![("finish", json!({"summary": "done"}))]))
    }

    fn with_goal(r: &mut Rig, text: &str) {
        r.agent.retune(Retune { goal: Some(text.into()), ..Default::default() });
    }

    fn goal_events(r: &Rig) -> Vec<(GoalState, String, String)> {
        events(r).into_iter().filter_map(|e| if let EventKind::Goal { state, text, detail } = e { Some((state, text, detail)) } else { None }).collect()
    }

    const TASK: &str = "Upper-case lines two and three of n.txt.";

    #[tokio::test]
    async fn a_goal_that_is_not_met_sends_the_model_back_until_a_check_says_it_is() {
        let turns = vec![upper("beta", "BETA"), fixed(MockTurn::say("NOT MET\nGamma is still lower case.")), upper("gamma", "GAMMA"), fixed(MockTurn::say("**MET**\nBoth lines are upper case and checked."))];
        let mut r = rig("agent-goal", AgentConfig::new("mock"), turns);
        with_goal(&mut r, "lines two and three of n.txt are upper case");
        let out = r.agent.run(TASK.into(), &CancellationToken::new()).await;
        assert_eq!((out.status, out.verified, r.agent.stats.turns, r.agent.stats.goal_checks), (FinishStatus::Done, true, 2, 2));
        assert_eq!(out.summary, "GAMMA done");
        assert!(out.evidence.ends_with("; goal met"), "{}", out.evidence);
        assert_eq!(r.agent.goal(), Some(&Goal { text: "lines two and three of n.txt are upper case".into(), met: true }));

        let sent = requests(&r);
        assert_eq!(sent.len(), 4, "two replies of the model and two checks");
        // The check is shown the goal, what the model said and what the harness knows; not the conversation.
        assert_eq!(sent[1].len(), 2);
        let shown = &sent[1][1].content;
        assert!(shown.contains("<goal>\nlines two and three of n.txt are upper case\n</goal>") && shown.contains("<worker_summary>\nBETA done\n</worker_summary>"), "{shown}");
        assert!(shown.contains("verified by `grep -qx BETA n.txt`") && shown.contains("Files you changed with edit or write: n.txt"), "{shown}");
        assert!(!shown.contains("<env>") && !shown.contains(TASK), "{shown}");
        // The model is sent back with the reasons, as the harness speaking.
        assert_eq!(sent[2].last().unwrap().content, "[harness] The session goal is not met yet: Gamma is still lower case.\nCarry on until it holds, then call `finish` again.");
        // No tools, the least thinking and a small cap for the check; the work has its tools and its effort.
        let shapes = shapes(&r);
        assert_eq!((shapes[1].effort, shapes[1].tools, shapes[1].max_tokens), (Effort::Off, 0, goal::MAX_TOKENS));
        assert!(shapes[0].tools > 0 && shapes[0].effort == Effort::Low && shapes[2].tools > 0);

        let goals = goal_events(&r);
        assert_eq!(goals.iter().map(|(state, ..)| *state).collect::<Vec<_>>(), [GoalState::Set, GoalState::NotMet, GoalState::Met]);
        assert_eq!(goals[1].2, "Gamma is still lower case.");
        // What the checks cost is in the session's figures.
        let replied: u64 = events(&r).iter().filter_map(|e| if let EventKind::Model { usage, .. } = e { Some(usage.output) } else { None }).sum();
        assert!(r.agent.stats.output_tokens > replied, "{} against {replied}", r.agent.stats.output_tokens);

        // A goal that is met no longer gates what comes after it.
        r.agent.provider = Arc::new(Provider::Mock(MockProvider::new(vec![finish()])));
        let out = r.agent.run("Is it done?".into(), &CancellationToken::new()).await;
        assert_eq!((out.status, r.agent.stats.goal_checks, requests(&r).len()), (FinishStatus::Done, 2, 1));
    }

    #[tokio::test]
    async fn without_a_goal_nothing_is_asked_at_the_end() {
        let mut r = rig("agent-no-goal", AgentConfig::new("mock"), vec![upper("beta", "BETA"), fixed(MockTurn::say("NOT MET"))]);
        let out = r.agent.run(TASK.into(), &CancellationToken::new()).await;
        assert_eq!((out.status, requests(&r).len(), r.agent.stats.goal_checks), (FinishStatus::Done, 1, 0));
        assert!(goal_events(&r).is_empty() && !out.evidence.contains("goal"));
    }

    #[tokio::test]
    async fn the_goal_is_checked_three_times_at_most_per_message_and_then_stays_unmet() {
        let no = || fixed(MockTurn::say("NOT MET\nStill nothing about gamma."));
        let turns = vec![upper("beta", "BETA"), no(), finish(), no(), finish(), no(), finish(), fixed(MockTurn::say("MET\nNow it is."))];
        let mut r = rig("agent-goal-cap", AgentConfig::new("mock"), turns);
        with_goal(&mut r, "gamma is upper case too");
        let out = r.agent.run(TASK.into(), &CancellationToken::new()).await;
        // The run ends as any other would, and says that the goal did not hold.
        assert_eq!((out.status, r.agent.stats.turns, r.agent.stats.goal_checks, requests(&r).len()), (FinishStatus::Done, 3, 3, 6));
        assert!(out.evidence.ends_with("; goal not met after 3 checks: Still nothing about gamma."), "{}", out.evidence);
        assert_eq!(r.agent.goal().map(|g| g.met), Some(false));
        assert_eq!(goal_events(&r).iter().filter(|(state, ..)| *state == GoalState::NotMet).count(), 3);
        // The goal stays: the next message is checked, with three checks of its own.
        let out = r.agent.run("Go on.".into(), &CancellationToken::new()).await;
        assert!(out.evidence.ends_with("; goal met") && r.agent.goal().is_some_and(|g| g.met), "{}", out.evidence);
        assert_eq!(r.agent.stats.goal_checks, 4);
    }

    #[tokio::test]
    async fn a_check_that_fails_or_cannot_be_read_does_not_fail_the_run() {
        for (name, answer) in [("agent-goal-fails", MockTurn::fail(ProviderError::Network("connection reset".into()))), ("agent-goal-unreadable", MockTurn::say("Looks fine to me."))] {
            let mut r = rig(name, AgentConfig::new("mock"), vec![upper("beta", "BETA"), fixed(answer)]);
            with_goal(&mut r, "beta is upper case");
            let out = r.agent.run(TASK.into(), &CancellationToken::new()).await;
            assert_eq!((out.status, out.verified, r.agent.stats.turns, requests(&r).len()), (FinishStatus::Done, true, 1, 2), "{name}");
            assert!(out.evidence.contains("; goal could not be checked: "), "{name}: {}", out.evidence);
            assert_eq!(r.agent.goal().map(|g| g.met), Some(false), "{name}: the goal stays");
            assert!(matches!(goal_events(&r).last(), Some((GoalState::Unchecked, text, why)) if text == "beta is upper case" && !why.is_empty()), "{name}");
            assert!(matches!(events(&r).last(), Some(EventKind::Finish { status: FinishStatus::Done, .. })));
        }
    }

    #[tokio::test]
    async fn stopping_during_the_check_stops_the_run() {
        let cancel = CancellationToken::new();
        let stop = {
            let cancel = cancel.clone();
            Box::new(move |_: &[Message]| {
                cancel.cancel();
                MockTurn::say("MET")
            }) as TurnFn
        };
        let mut r = rig("agent-goal-stop", AgentConfig::new("mock"), vec![upper("beta", "BETA"), stop]);
        with_goal(&mut r, "beta is upper case");
        let out = r.agent.run(TASK.into(), &cancel).await;
        assert_eq!((out.status, out.summary.as_str()), (FinishStatus::Stopped, "Stopped by the user."));
        assert_eq!(r.agent.goal().map(|g| g.met), Some(false));
        assert!(!goal_events(&r).iter().any(|(state, ..)| *state == GoalState::Met));
    }

    #[tokio::test]
    async fn on_the_last_reply_the_check_is_made_and_recorded_but_nothing_can_follow_it() {
        let edit = || fixed(MockTurn::calls("", vec![("edit", json!({"path": "n.txt", "old": "beta", "new": "BETA"}))]));
        let last = || fixed(MockTurn::calls("", vec![("shell", json!({"cmd": "grep -qx BETA n.txt", "expect_exit": "0"})), ("finish", json!({"summary": "beta is done, gamma is not"}))]));
        let two_replies = || AgentConfig { max_turns: 2, ..AgentConfig::new("mock") };

        let mut r = rig("agent-goal-landing", two_replies(), vec![edit(), last(), fixed(MockTurn::say("NOT MET\nGamma was not done."))]);
        with_goal(&mut r, "lines two and three are upper case");
        let out = r.agent.run(TASK.into(), &CancellationToken::new()).await;
        assert_eq!((out.status, r.agent.stats.turns, requests(&r).len()), (FinishStatus::Done, 2, 3));
        assert!(out.evidence.ends_with("; goal not met: Gamma was not done."), "{}", out.evidence);
        assert_eq!(r.agent.goal().map(|g| g.met), Some(false));
        assert!(matches!(goal_events(&r).last(), Some((GoalState::NotMet, ..))));

        let mut r = rig("agent-goal-landed", two_replies(), vec![edit(), last(), fixed(MockTurn::say("MET\nThe goal as stated holds."))]);
        with_goal(&mut r, "line two is upper case");
        r.agent.run(TASK.into(), &CancellationToken::new()).await;
        assert_eq!(r.agent.goal().map(|g| g.met), Some(true));
    }

    #[tokio::test]
    async fn an_answer_in_prose_is_checked_against_the_goal_too() {
        let mut cfg = AgentConfig::new("mock");
        cfg.interactive = true;
        let turns = vec![fixed(MockTurn::say("I think that is it.")), fixed(MockTurn::say("NOT MET\nNothing was run.")), fixed(MockTurn::say("Ran it: all lines are right.")), fixed(MockTurn::say("MET"))];
        let mut r = rig("agent-goal-prose", cfg, turns);
        with_goal(&mut r, "n.txt has three lines");
        let out = r.agent.run("Count the lines of n.txt.".into(), &CancellationToken::new()).await;
        assert_eq!((out.status, out.summary.as_str(), r.agent.stats.turns), (FinishStatus::Yielded, "Ran it: all lines are right.", 2));
        assert!(r.agent.goal().is_some_and(|g| g.met));
        assert!(requests(&r)[2].last().unwrap().content.starts_with("[harness] The session goal is not met yet: Nothing was run."));
        assert!(requests(&r)[1][1].content.contains("<worker_summary>\nI think that is it.\n</worker_summary>"));
    }

    #[tokio::test]
    async fn goal_commands_are_answered_by_the_session_and_cost_no_request() {
        let mut r = rig("agent-goal-command", AgentConfig::new("mock"), vec![]);
        r.agent.goal_command("/goal", goal::Command::Show);
        r.agent.goal_command("/goal tests pass", goal::Command::Set("tests pass".into()));
        r.agent.goal_command("/goal", goal::Command::Show);
        r.agent.goal_command("/goal clear", goal::Command::Clear);
        r.agent.goal_command("/goal clear", goal::Command::Clear);
        assert!(requests(&r).is_empty() && r.agent.goal().is_none());
        let log: Vec<String> = events(&r)
            .iter()
            .map(|e| match e {
                EventKind::User { text, .. } => format!("user {text}"),
                EventKind::Goal { state, text, detail } => format!("{state:?} {text:?} {detail:?}"),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            log,
            [
                "user /goal",
                "Shown \"\" \"\"",
                "user /goal tests pass",
                "Set \"tests pass\" \"\"",
                "user /goal",
                "Shown \"tests pass\" \"not met yet\"",
                "user /goal clear",
                "Cleared \"tests pass\" \"\"",
                "user /goal clear",
                "Cleared \"\" \"\"",
            ]
        );
    }

    fn git(r: &Rig, script: &str) {
        assert!(std::process::Command::new("bash").arg("-c").arg(script).current_dir(&r.agent.sess.ws.root).output().unwrap().status.success(), "{script}");
    }

    const COMMIT: &str = "git -c user.name=t -c user.email=t@localhost -c commit.gpgsign=false commit -q -m base";

    #[tokio::test]
    async fn in_a_repository_the_check_is_shown_the_diff_of_the_session_and_not_the_work_that_was_there_before() {
        let edit = fixed(MockTurn::calls("", vec![("edit", json!({"path": "n.txt", "old": "beta", "new": "BETA"})), ("write", json!({"path": "scratch.txt", "content": "tmp\n"})), ("shell", json!({"cmd": "grep -qx BETA n.txt", "expect_exit": "0"})), ("finish", json!({"summary": "done"}))]));
        let mut r = rig("agent-goal-git", AgentConfig::new("mock"), vec![edit, fixed(MockTurn::say("MET"))]);
        git(&r, &format!("printf 'mine\\n' > wip.txt && git init -q && git add n.txt && {COMMIT}"));
        with_goal(&mut r, "n.txt says BETA");
        r.agent.run(TASK.into(), &CancellationToken::new()).await;
        let shown = requests(&r)[1][1].content.clone();
        // Its own two files and the diff of the tracked one; not the file that lay there before.
        assert!(shown.contains("<changes>\nPaths (`git status --short`):\n M n.txt\n?? scratch.txt\nDiff of the tracked files (`git diff HEAD`):\n"), "{shown}");
        assert!(shown.contains("\n-beta\n+BETA\n") && !shown.contains("wip.txt"), "{shown}");
    }

    /// Whether the session had a baseline of the uncommitted work once it had run.
    async fn baseline_taken(name: &str, features: Features, goal: Option<&str>) -> bool {
        let mut r = rig(name, AgentConfig { features, interactive: true, ..AgentConfig::new("mock") }, vec![fixed(MockTurn::say("fine")), fixed(MockTurn::say("MET"))]);
        git(&r, &format!("git init -q && git add n.txt && {COMMIT}"));
        if let Some(goal) = goal {
            with_goal(&mut r, goal);
        }
        r.agent.run("Look at n.txt.".into(), &CancellationToken::new()).await;
        r.agent.sess.baseline.is_some()
    }

    #[tokio::test]
    async fn the_baseline_of_the_uncommitted_work_is_taken_where_the_mode_tracks_changes_or_there_is_a_goal() {
        assert!(baseline_taken("agent-baseline", Features::ecosage(), None).await, "taken whenever the mode tracks changes");
        assert!(!baseline_taken("agent-baseline-classic", Features::classic(), None).await, "the plain loop tracks nothing");
        assert!(baseline_taken("agent-baseline-goal", Features::classic(), Some("n.txt is read")).await, "unless there is a goal to check");
    }

    #[tokio::test]
    async fn a_goal_is_checked_in_the_classic_loop_too() {
        let cfg = AgentConfig { features: Features::classic(), ..AgentConfig::new("mock") };
        let edit = fixed(MockTurn::calls("", vec![("edit", json!({"path": "n.txt", "old": "beta", "new": "BETA"})), ("finish", json!({"summary": "done"}))]));
        let mut r = rig("agent-goal-classic", cfg, vec![edit, fixed(MockTurn::say("NOT MET\nNothing was run.")), finish(), fixed(MockTurn::say("MET"))]);
        with_goal(&mut r, "n.txt says BETA");
        let out = r.agent.run("fix".into(), &CancellationToken::new()).await;
        assert_eq!((out.status, r.agent.stats.goal_checks, requests(&r).len()), (FinishStatus::Done, 2, 4));
        assert!(requests(&r)[1][1].content.contains("not tracked in classic mode") && out.evidence.ends_with("; goal met"), "{}", out.evidence);
    }

    /// Three cases about n.txt, the third of which wants line two in capitals.
    const EXAM: &str = "COMMAND: sh {file}\n```sh\np=0; f=0\ncheck() { if eval \"$2\"; then p=$((p+1)); else f=$((f+1)); echo \"case $1 failed\"; fi; }\ncheck line1_alpha 'sed -n 1p n.txt | grep -qx alpha'\ncheck line3_gamma 'sed -n 3p n.txt | grep -qx gamma'\ncheck line2_capitals 'sed -n 2p n.txt | grep -qx BETA'\necho \"EXAM: $p passed, $f failed\"\n[ \"$f\" -eq 0 ]\n```";

    /// A rig whose model also plays the examiner (answering `exam`) and the skill router (`route`).
    fn prosage_rig(name: &str, with: &[&str], turns: Vec<TurnFn>, exam: &'static str, route: &'static str) -> Rig {
        let mut cfg = AgentConfig::new("mock");
        cfg.features = with.iter().fold(Features::ecosage(), |f, name| f.with(name).unwrap());
        let mut r = rig(name, cfg, vec![]);
        let aside: crate::provider::mock::AsideFn = Box::new(move |messages| match messages.first().map(|m| m.content.as_str()) {
            Some(examiner::SYSTEM) => Some(MockTurn::say(exam)),
            Some(examiner::ROUTER_SYSTEM) => Some(MockTurn::say(route)),
            _ => None,
        });
        r.agent.provider = Arc::new(Provider::Mock(MockProvider::new(turns).with_aside(aside)));
        r
    }

    fn notes(r: &Rig, kind: &str) -> Vec<String> {
        events(r).into_iter().filter_map(|e| if let EventKind::Note { kind: k, text, .. } = e { (k == kind).then_some(text) } else { None }).collect()
    }

    /// The requests of the conversation itself, without the ones made beside it.
    fn turns_sent(r: &Rig) -> Vec<Vec<Message>> {
        requests(r).into_iter().filter(|m| m[0].content != examiner::SYSTEM && m[0].content != examiner::ROUTER_SYSTEM).collect()
    }

    #[tokio::test]
    async fn the_examiner_holds_a_finish_until_its_cases_pass() {
        let mut r = prosage_rig("agent-exam", &["examiner"], vec![upper("beta", "Beta"), upper("Beta", "BETA")], EXAM, "NONE");
        let out = r.agent.run("Put line two of n.txt in capitals.".into(), &CancellationToken::new()).await;
        assert_eq!((out.status, out.verified, r.agent.stats.turns, r.agent.stats.side_requests), (FinishStatus::Done, true, 2, 1));
        assert!(out.evidence.ends_with("; all 3 of the examiner's cases pass"), "{}", out.evidence);
        assert_eq!(std::fs::read_to_string(r.agent.sess.ws.root.join("n.txt")).unwrap(), "alpha\nBETA\ngamma\n");

        // The examiner was shown what the worker was shown at the start, and none of the work.
        let exam = requests(&r).into_iter().find(|m| m[0].content == examiner::SYSTEM).unwrap();
        assert!(exam[1].content.contains("<task>\nPut line two of n.txt in capitals.\n</task>") && exam[1].content.ends_with("Write the check script for this task now."), "{}", exam[1].content);
        // The worker is sent back with the failing case and the rule for settling it.
        let sent = turns_sent(&r);
        let held = &sent[1].last().unwrap().content;
        assert!(held.starts_with("[harness] [examiner] The work is not accepted yet.") && held.contains("2 pass, 1 fail") && held.contains("case line2_capitals failed") && held.contains("Settle each one by the text it cites"), "{held}");
        assert_eq!(notes(&r, "examiner"), ["finish held: 1 of the examiner's 3 cases fail", "all 3 of the examiner's cases pass"]);
        // A gap the worker's own checks did not show is a reason to think harder about the rest.
        assert_eq!(r.agent.effort(), Effort::High);
        assert!(events(&r).iter().any(|e| matches!(e, EventKind::Effort { from, to, reason, .. } if from == "low" && to == "high" && reason.contains("examiner"))));
        // The script is kept with the session, outside the workspace.
        assert!(r.agent.sess.dir.join("exam/exam_checks.sh").exists() && !r.agent.sess.ws.root.join("exam").exists());
    }

    #[tokio::test]
    async fn an_answer_in_prose_is_examined_only_after_changes() {
        let edit = fixed(MockTurn::calls("", vec![("edit", json!({"path": "n.txt", "old": "beta", "new": "Beta"}))]));
        let mut cfg = AgentConfig::new("mock");
        cfg.interactive = true;
        let turns = vec![edit, fixed(MockTurn::say("Line two is capitalised.")), upper("Beta", "BETA")];
        let mut r = prosage_rig("agent-exam-prose", &["examiner"], turns, EXAM, "NONE");
        r.agent.cfg.interactive = cfg.interactive;
        let out = r.agent.run("Put line two of n.txt in capitals.".into(), &CancellationToken::new()).await;
        assert_eq!((out.status, r.agent.stats.turns), (FinishStatus::Done, 3));
        assert!(turns_sent(&r)[2].last().unwrap().content.contains("case line2_capitals failed"));

        // A question answered without touching anything is nobody's work to examine.
        let mut r = prosage_rig("agent-exam-question", &["examiner"], vec![fixed(MockTurn::say("It has three lines."))], EXAM, "NONE");
        r.agent.cfg.interactive = true;
        let out = r.agent.run("How long is n.txt?".into(), &CancellationToken::new()).await;
        assert_eq!((out.status, out.summary.as_str(), r.agent.stats.turns), (FinishStatus::Yielded, "It has three lines.", 1));
        assert!(notes(&r, "examiner").is_empty());
    }

    #[tokio::test]
    async fn a_case_the_worker_rules_wrong_does_not_hold_the_finish_twice() {
        let mut r = prosage_rig("agent-exam-ruling", &["examiner"], vec![upper("beta", "Beta"), finish()], EXAM, "NONE");
        let out = r.agent.run("Capitalise line two of n.txt.".into(), &CancellationToken::new()).await;
        assert_eq!((out.status, out.verified, r.agent.stats.turns), (FinishStatus::Done, true, 2));
        assert!(out.evidence.ends_with("; 1 of the examiner's 3 cases left failing, as the worker ruled"), "{}", out.evidence);
        assert_eq!(std::fs::read_to_string(r.agent.sess.ws.root.join("n.txt")).unwrap(), "alpha\nBeta\ngamma\n");
    }

    #[tokio::test]
    async fn an_examiner_with_nothing_usable_never_stands_in_the_way() {
        // Nothing a script could check; a script that does not run to its end; one that fails nearly every case.
        let broken = "COMMAND: sh {file}\n```sh\necho 'cannot import the module'; exit 2\n```";
        let hostile = "COMMAND: sh {file}\n```sh\necho 'EXAM: 1 passed, 4 failed'; exit 1\n```";
        for (name, exam, said) in [("none", "NONE", "wrote no script"), ("broken", broken, "did not run to its end"), ("hostile", hostile, "taken to be wrong")] {
            let mut r = prosage_rig(&format!("agent-exam-{name}"), &["examiner"], vec![upper("beta", "BETA")], exam, "NONE");
            let out = r.agent.run("Put line two of n.txt in capitals.".into(), &CancellationToken::new()).await;
            assert_eq!((out.status, out.verified, r.agent.stats.turns, out.evidence.as_str()), (FinishStatus::Done, true, 1, "verified by `grep -qx BETA n.txt`"), "{name}");
            assert!(notes(&r, "examiner").iter().any(|n| n.contains(said)), "{name}: {:?}", notes(&r, "examiner"));
        }
        // Without the mechanism nobody is asked.
        let mut r = prosage_rig("agent-exam-off", &[], vec![upper("beta", "BETA")], EXAM, "NONE");
        r.agent.run("Put line two of n.txt in capitals.".into(), &CancellationToken::new()).await;
        assert_eq!((requests(&r).len(), r.agent.stats.side_requests), (1, 0));
        // Nor where every step waits for the user's word: the script would be a command run unasked.
        let mut r = prosage_rig("agent-exam-ask", &["examiner"], vec![finish()], EXAM, "NONE");
        r.agent.retune(Retune { autonomy: Some(Autonomy::Ask), ..Default::default() });
        r.agent.run("How long is n.txt?".into(), &CancellationToken::new()).await;
        assert_eq!((requests(&r).len(), r.agent.stats.side_requests), (1, 0));
    }

    #[tokio::test]
    async fn a_skill_that_fits_the_task_is_switched_on_unasked() {
        let skill = |name: &str, about: &str| skills::parse(name, &format!("---\nname: {name}\ntitle: The {name} playbook\ndescription: {about}\n---\nBody of {name}."), skills::SkillSource::Builtin).unwrap();
        let mut r = prosage_rig("agent-route", &["routing"], vec![upper("beta", "BETA")], "NONE", "ui");
        r.agent.cfg.routable = vec![skill("ui", "Build or polish a UI."), skill("debug", "Find the cause of a bug.")];
        r.agent.run("Make the page look consistent.".into(), &CancellationToken::new()).await;
        let asked = requests(&r).into_iter().find(|m| m[0].content == examiner::ROUTER_SYSTEM).unwrap();
        assert!(asked[1].content.contains("- ui: Build or polish a UI.\n- debug: Find the cause of a bug.") && asked[1].content.ends_with("Task:\nMake the page look consistent."), "{}", asked[1].content);
        let first = &turns_sent(&r)[0][1].content;
        assert!(first.contains("Body of ui.") && !first.contains("Body of debug."), "{first}");
        assert_eq!(notes(&r, "routing"), ["skill switched on for this task: ui"]);

        // An answer that names none switches none on, and so does a session with nothing to choose from.
        let mut r = prosage_rig("agent-route-none", &["routing"], vec![upper("beta", "BETA")], "NONE", "NONE");
        r.agent.cfg.routable = vec![skill("ui", "Build or polish a UI.")];
        r.agent.run("Fix the off-by-one in pager.".into(), &CancellationToken::new()).await;
        assert!(!turns_sent(&r)[0][1].content.contains("Body of ui.") && notes(&r, "routing").is_empty());
        let mut r = prosage_rig("agent-route-empty", &["routing"], vec![upper("beta", "BETA")], "NONE", "ui");
        r.agent.run("Fix it.".into(), &CancellationToken::new()).await;
        assert_eq!(requests(&r).len(), 1);
    }

    #[tokio::test]
    async fn an_unhurried_run_hears_of_no_budget_and_of_what_passing_checks_leave_open() {
        let edit_and_check = || fixed(MockTurn::calls("", vec![("edit", json!({"path": "n.txt", "old": "beta", "new": "BETA"})), ("shell", json!({"cmd": "grep -qx BETA n.txt", "expect_exit": "0"}))]));
        let mut r = prosage_rig("agent-unhurried", &["unhurried"], vec![edit_and_check(), finish()], "NONE", "NONE");
        r.agent.run("Put line two of n.txt in capitals.".into(), &CancellationToken::new()).await;
        let sent = turns_sent(&r);
        assert!(!sent[0][1].content.contains("Budget:"), "{}", sent[0][1].content);
        let after_check = &sent[1].last().unwrap().content;
        assert!(after_check.contains("The checks pass. They show only what they exercise") && !after_check.contains("`finish` is accepted now"), "{after_check}");

        let mut r = prosage_rig("agent-hurried", &[], vec![edit_and_check(), finish()], "NONE", "NONE");
        r.agent.run("Put line two of n.txt in capitals.".into(), &CancellationToken::new()).await;
        let sent = turns_sent(&r);
        assert!(sent[0][1].content.contains("Budget: 80 replies") && sent[1].last().unwrap().content.contains("`finish` is accepted now"));
    }
}
