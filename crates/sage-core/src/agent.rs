//! The agent loop: one model reply, one script, repeat until the task is finished or handed back.

use crate::attach;
use crate::bootstrap;
use crate::changes::{self, Baseline};
use crate::context::Context;
use crate::contract::Contract;
use crate::economy::{peak_multiplier, prices_for};
use crate::events::{
    CacheVerdict, CallInfo, Channel, EventKind, FinishStatus, GoalState, Sink, Stats,
};
use crate::goal::{self, Goal, Verdict};
use crate::prompt;
use crate::provider::{ChatRequest, Delta, Effort, Message, Provider, ProviderError, Role, Usage};
use crate::repair;
use crate::resume::{self, Setup, Snapshot};
use crate::script::{Approvals, Autonomy, Runner, ScriptResult};
use crate::skills::{self, SkillInfo};
use crate::state::{Features, Session};
use crate::util::{now_ms, one_line};
use crate::watches;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
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
            _ => match s
                .strip_prefix("ratchet-")
                .or_else(|| s.strip_prefix("ratchet:"))
            {
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
    effort
        .lower()
        .filter(|lower| *lower != Effort::Off || nth > 1)
}

#[derive(Clone, Debug)]
pub struct AgentConfig {
    pub features: Features,
    pub effort: EffortPolicy,
    pub model: String,
    /// Model replies allowed per user message.
    pub max_turns: u32,
    pub max_requests: Option<u32>,
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
}

impl AgentConfig {
    pub(crate) fn set_run_limits(&mut self, max_turns: Option<u32>, time_budget_secs: Option<u64>) {
        if let Some(turns) = max_turns {
            self.max_turns = turns.max(1);
        }
        if let Some(seconds) = time_budget_secs {
            self.time_budget = (seconds > 0).then(|| Duration::from_secs(seconds));
        }
    }

    pub fn new(model: &str) -> Self {
        Self {
            features: Features::sage(),
            effort: EffortPolicy::Ratchet(Effort::Low),
            model: model.to_string(),
            max_turns: 80,
            max_requests: None,
            max_tokens: 32_000,
            time_budget: None,
            autonomy: Autonomy::Auto,
            interactive: false,
            context_limit: 150_000,
            system: None,
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
    /// What the workspace held before the session began, while git is still being asked (see
    /// `opening`).
    pending_baseline: Option<tokio::task::JoinHandle<Option<Baseline>>>,
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
    snapshot_warned: bool,
    requests_before: u32,
    journal: Option<resume::Journal>,
    /// Said in front of the next message: the session was brought back after a restart.
    restored: Option<String>,
    /// Files the session's task says not to change, read off it mechanically ("Guard files").
    contract: Contract,
    /// How often `contract` has held this message's `finish`.
    contract_holds: u8,
    /// The next message continues a run of the first task that a restart cut off: `contract`
    /// still applies to it.
    carry_contract: bool,
    /// The current message points at a written specification beyond its tests (`spec_review`).
    spec_task: bool,
}

/// What a message may change about the session it is sent to. Left out, a thing stays as it is.
#[derive(Default)]
pub struct Retune {
    /// Another model: what to reach it through, its provider's id and its name.
    pub provider: Option<(Provider, String, String)>,
    pub effort: Option<EffortPolicy>,
    pub autonomy: Option<Autonomy>,
    pub interactive: Option<bool>,
    pub max_turns: Option<u32>,
    pub time_budget_secs: Option<u64>,
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

/// Sent once when a task with a written specification first calls `finish`.
const SPEC_REVIEW: &str = "Before this ends: the tests cover only part of what the task asks. Review the work against the specification the task names (README, spec, contract, docstring, plan or note), in your next reply and with ONE call: `watch` named `spec`, whose `cmd` is a shell script that checks every rule no test covers (command-line forms and wrong use, errors and exit statuses, edge cases, and every file or entry the task says to change). Make it print PASS or FAIL with the rule for each check and exit non-zero if any fails. The harness runs it: if it passes, your `finish` stands as it is and no further reply is needed. If it fails, fix the work and call `finish` again; the watch re-runs by itself after every change.";
/// The same review where there are no watches: the checks go in one reply, with `finish` last.
const SPEC_REVIEW_PLAIN: &str = "Before this ends: the tests cover only part of what the task asks. Review the work against the specification the task names (README, spec, contract, docstring, plan or note), in your next reply only: one check (shell with expect_exit / expect_has) for every rule no test covers, with `finish` as the last call. The calls stop at the first that fails: then fix that and call `finish` again.";
/// Replies a spec review may take before the model is told to finish.
const SPEC_REVIEW_REPLIES: u32 = 4;

/// Whether a task holds the work to more than its tests: it says they check only part of it, or
/// it points at a written specification (a README, spec, contract, docstring, plan or note).
fn points_past_its_tests(task: &str) -> bool {
    let t = task.to_lowercase();
    const PARTIAL: &[&str] = &[
        "cover only",
        "covers only",
        "checks only",
        "check only",
        "only the library",
        "library only",
        "small part",
        "only some",
        "only a few",
        "not only",
        "does not cover",
        "do not cover",
    ];
    const SPEC: &[&str] = &[
        "readme",
        "spec",
        "contract",
        "docstring",
        "plan",
        "the note",
        "guarantee",
        "requirement",
    ];
    PARTIAL.iter().any(|p| t.contains(p)) || SPEC.iter().any(|p| t.contains(p))
}

/// What the checks before a run ends have used up in this run.
#[derive(Default)]
struct EndChecks {
    /// Goal checks made for this user message.
    goal: u32,
    /// The spec review has held a `finish` of this user message.
    spec: bool,
    /// The `finish` the spec review holds: it stands as soon as a reply's checks all pass.
    held: Option<Outcome>,
    /// Replies since the spec review held the `finish`.
    review_replies: u32,
}

const KEEP_RECENT_MESSAGES: usize = 12;
const MIN_DECAY_TOKENS: u64 = 6_000;
/// Reasoning a reply may spend before its first word or call at the highest effort (whoever asks
/// for `max` is asking for the thought): about 24,000 tokens, which still leaves room for the
/// answer under the output cap.
const LONG_THOUGHT_CHARS: usize = 96_000;
const LAST_REPLY_NOTE: &str = "Your next reply is the last one the budget allows. Start nothing new: call `finish` in it and say in the summary what is done, how it was checked, and what is unfinished or unchecked. It is accepted as it stands.";
/// How often mechanical contract violations may hold one message's `finish`.
const CONTRACT_HOLDS: u8 = 3;
/// Said after the last cut a run allows for reasoning without acting.
const WRAP_UP_NOTE: &str = "Your next reply is the last one this run allows. Act now instead of planning: call `finish` in it and say in the summary what is done, how it was checked, and what is unfinished or unchecked. It is accepted as it stands.";

/// What a request streamed, attempt by attempt, so that what the provider was paid for and never
/// reported (a reply cut off, given up on, or made again after a failure) can be booked by estimate.
#[derive(Default)]
struct Streamed {
    attempts: std::sync::atomic::AtomicU32,
    /// Bytes of reasoning and of text (tool calls included) the attempt in progress has streamed.
    reasoning: AtomicU64,
    content: AtomicU64,
    /// The same of attempts that streamed something and then failed, and how many there were.
    lost_reasoning: AtomicU64,
    lost_content: AtomicU64,
    lost_attempts: AtomicU64,
}

impl Streamed {
    fn seen(&self, reasoning: bool, bytes: usize) {
        let counter = if reasoning {
            &self.reasoning
        } else {
            &self.content
        };
        counter.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    /// The attempt in progress is over without a report: what it streamed is lost.
    fn retried(&self) {
        let reasoning = self.reasoning.swap(0, Ordering::Relaxed);
        let content = self.content.swap(0, Ordering::Relaxed);
        if reasoning + content > 0 {
            self.lost_reasoning.fetch_add(reasoning, Ordering::Relaxed);
            self.lost_content.fetch_add(content, Ordering::Relaxed);
            self.lost_attempts.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// The attempt in progress reasoned at least `bytes`, whatever reached the stream.
    fn at_least(&self, bytes: usize) {
        self.reasoning.fetch_max(bytes as u64, Ordering::Relaxed);
    }
}

/// Waits until `deadline`, or for ever where there is none.
async fn until(deadline: Option<Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await,
        None => std::future::pending().await,
    }
}

impl Agent {
    pub fn new(
        provider: Provider,
        sess: Session,
        cfg: AgentConfig,
        sink: Arc<dyn Sink>,
        approvals: Arc<Approvals>,
    ) -> Self {
        let ctx = Context::new(
            cfg.system
                .as_deref()
                .unwrap_or_else(|| prompt::system_prompt(&cfg.features)),
        );
        let effort = cfg.effort.start();
        Self {
            provider: Arc::new(provider),
            contract: Contract::default(),
            contract_holds: 0,
            carry_contract: false,
            spec_task: false,
            sess,
            ctx,
            cfg,
            sink,
            approvals,
            stats: Stats::default(),
            turn: 0,
            effort,
            surprise_streak: 0,
            failures: HashMap::new(),
            edits: HashMap::new(),
            first_message: true,
            budget_marks: 0,
            chain_hints: 2,
            solo_streak: 0,
            solo_hints: 2,
            pending_baseline: None,
            announce: Vec::new(),
            attached: Vec::new(),
            goal: None,
            shown: None,
            keep: None,
            snapshot_warned: false,
            requests_before: 0,
            journal: None,
            restored: None,
        }
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
        if let Some(interactive) = change.interactive {
            self.cfg.interactive = interactive;
        }
        self.cfg
            .set_run_limits(change.max_turns, change.time_budget_secs);
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
            self.announce.push(EventKind::Settings {
                provider,
                model,
                effort,
                autonomy,
            });
        }
        if let Some(text) = change
            .goal
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
        {
            self.goal = Some(Goal::new(&text));
            self.announce.push(goal_event(GoalState::Set, text, ""));
        }
    }

    /// The session's goal, if it has one: met or not.
    #[cfg(all(test, unix))]
    fn goal(&self) -> Option<&Goal> {
        self.goal.as_ref()
    }

    /// What the next message is shown as in the log, where it is not what the model is sent:
    /// `/goal <condition>`, which the model gets worded as a task.
    pub fn shown_as(&mut self, text: String) {
        self.shown = Some(text);
    }

    /// A `/goal` message that is no task: it goes in the log as the user's, is answered from here
    /// and costs no request to the model.
    pub async fn goal_command(&mut self, shown: &str, command: goal::Command) {
        self.sink.emit(EventKind::User {
            text: shown.trim().to_string(),
            skills: Vec::new(),
            files: Vec::new(),
        });
        for change in std::mem::take(&mut self.announce) {
            self.sink.emit(change);
        }
        let event = match command {
            goal::Command::Set(text) => {
                self.goal = Some(Goal::new(&text));
                goal_event(GoalState::Set, text.trim().to_string(), "")
            }
            goal::Command::Clear => goal_event(
                GoalState::Cleared,
                self.goal.take().map(|g| g.text).unwrap_or_default(),
                "",
            ),
            goal::Command::Show => match &self.goal {
                Some(g) => goal_event(
                    GoalState::Shown,
                    g.text.clone(),
                    if g.met { "met" } else { "not met yet" },
                ),
                None => goal_event(GoalState::Shown, String::new(), ""),
            },
        };
        self.sink.emit(event);
        self.save_point(false).await;
    }

    pub fn effort(&self) -> Effort {
        self.effort
    }

    /// An effort the user fixed is only ever left for the reply that follows one that did
    /// nothing but reason, so that the same thought is not paid for twice. Once the model has
    /// acted, the session is back at its level: `max` stays `max` in every mode.
    fn restore_fixed_effort(&mut self) {
        if let EffortPolicy::Fixed(level) = self.cfg.effort
            && self.effort != level
        {
            self.sink.emit(EventKind::Effort {
                turn: self.turn,
                from: self.effort.as_str().into(),
                to: level.as_str().into(),
                reason: "the level this session is set to".into(),
            });
            self.effort = level;
        }
    }

    fn note(&self, kind: &str, text: &str) {
        self.sink.emit(EventKind::Note {
            turn: self.turn,
            kind: kind.to_string(),
            text: text.to_string(),
        });
    }

    fn remaining_requests(&self) -> Option<u32> {
        self.cfg
            .max_requests
            .map(|max| max.saturating_sub(self.stats.requests - self.requests_before))
    }

    fn evidence_text(&self) -> String {
        let ev = &self.sess.evidence;
        match &ev.verified {
            _ if !ev.caveat().is_empty() => {
                format!("workspace verification is incomplete{}", ev.caveat())
            }
            _ if ev.epoch == 0 => "no changes were made".to_string(),
            Some((epoch, by)) if *epoch == ev.epoch => format!("verified by {by}"),
            _ => format!("not checked since {}", ev.last_change),
        }
    }

    /// Trims old observations when the context limit demands it or the price vector says it pays.
    /// `turns_used` replies of this run have come, and `landing` is the last one it allows.
    fn maybe_decay(&mut self, turns_used: u32, landing: bool, run_started: Instant) {
        let over_limit = self.stats.context_tokens > self.cfg.context_limit;
        // On the last reply a trim saves one reading of what it drops and re-buys all that follows.
        if landing && !over_limit {
            return;
        }
        let (droppable_bytes, suffix_bytes) = self
            .ctx
            .decay_estimate(KEEP_RECENT_MESSAGES, self.provider.replays_reasoning());
        let (droppable, suffix) = (droppable_bytes / 4, suffix_bytes / 4);
        if droppable < MIN_DECAY_TOKENS {
            return;
        }
        let left = self.replies_left(turns_used, run_started);
        let pays = self
            .sess
            .prices
            .is_some_and(|p| p.decay_pays_off(droppable, suffix, left));
        if !over_limit && !pays {
            return;
        }
        let d = self.ctx.decay(KEEP_RECENT_MESSAGES);
        // Those windows are gone from the context, so reads must print them again.
        self.sess.seen.clear();
        let reason = if over_limit {
            "the context limit was reached"
        } else {
            "carrying them cost more than re-reading the rest once"
        };
        self.sink.emit(EventKind::Decay {
            turn: self.turn,
            messages: d.messages,
            dropped_tokens: d.dropped_bytes / 4,
            reason: reason.to_string(),
        });
    }

    /// The replies this run can still be expected to ask for, the next one included: with no better
    /// information about as many again as it has had (at least four), and never more than its turn
    /// limit and the time it has left allow at the pace so far.
    fn replies_left(&self, turns_used: u32, run_started: Instant) -> f64 {
        let mut left = f64::from(
            turns_used
                .max(4)
                .min(self.cfg.max_turns.saturating_sub(turns_used)),
        );
        if let (Some(at), true) = (self.sess.deadline, turns_used > 0) {
            let per_reply = run_started.elapsed().as_secs_f64() / f64::from(turns_used);
            if per_reply > 0.0 {
                left = left
                    .min(at.saturating_duration_since(Instant::now()).as_secs_f64() / per_reply);
            }
        }
        left.max(1.0)
    }

    /// How a run that reached its `what` ("time budget", "turn limit") ends.
    fn out_of_budget(&self, what: &str) -> Outcome {
        Outcome {
            status: FinishStatus::Budget,
            summary: format!("Stopped: the {what} was reached before the task was finished."),
            verified: self.sess.evidence.fresh(),
            evidence: self.evidence_text(),
        }
    }

    /// Books what a request cost beyond the usage the provider reported: attempts that streamed
    /// and were then made again and, where it was `cut` off, the attempt in progress, which ended
    /// before any report. Each is booked as its prompt (`estimate`: its tokens, and of them those
    /// the cache likely held) and the text it streamed, at four bytes a token.
    fn book_unreported(&mut self, streamed: &Streamed, cut: bool, estimate: (u64, u64)) {
        if cut {
            streamed.retried();
        }
        let attempts = streamed.lost_attempts.load(Ordering::Relaxed);
        if attempts == 0 {
            return;
        }
        let reasoning = streamed.lost_reasoning.load(Ordering::Relaxed).div_ceil(4);
        let content = streamed.lost_content.load(Ordering::Relaxed).div_ceil(4);
        let (prompt, cached) = estimate;
        self.count_usage(
            Usage {
                prompt: prompt * attempts,
                cached: cached * attempts,
                output: reasoning + content,
                reasoning,
            },
            0,
        );
    }

    /// Puts what git said the workspace held before the session in place, once it has answered.
    async fn settle_baseline(&mut self) {
        if let Some(task) = self.pending_baseline.take() {
            let mut task = task;
            self.sess.baseline = tokio::select! {
                result = &mut task => result.ok().flatten(),
                _ = until(self.sess.deadline) => { task.abort(); None }
            };
        }
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
        // Replies that changed a file, not edits: several edits of one file in one reply are one
        // piece of work, the way the model is asked to send them.
        let mut edited: Vec<&String> = r.edited.iter().collect();
        edited.sort();
        edited.dedup();
        for path in edited {
            let n = self.edits.entry(path.clone()).or_insert(0);
            *n += 1;
            if *n == 6 {
                notes.push(format!("{path} has now been changed in 6 replies. Step back and state what is actually wrong before changing it again."));
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
        let Some(budget) = self.cfg.time_budget.filter(|_| turns_used > 0) else {
            return false;
        };
        let elapsed = run_started.elapsed();
        // A reply is expected to take twice the average so far, or as long as the slowest if that is less.
        budget.saturating_sub(elapsed) <= slowest.min(elapsed / turns_used * 2)
    }

    fn budget_notes(
        &mut self,
        run_started: Instant,
        turns_used: u32,
        slowest: Duration,
    ) -> Vec<String> {
        // Said before the last reply, not after it: a run that ends at the limit in the middle of
        // its work leaves nothing behind that says what was done and what was not.
        if self.budget_marks < 3 && self.last_reply(run_started, turns_used, slowest) {
            self.budget_marks = 3;
            return vec![LAST_REPLY_NOTE.to_string()];
        }
        let mut notes = Vec::new();
        let used = match self.cfg.time_budget {
            Some(budget) if !budget.is_zero() => {
                run_started.elapsed().as_secs_f64() / budget.as_secs_f64()
            }
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
        // is shown of it is only what changed after this. Git is asked while the rest of the first
        // message is put together and the first reply is coming, and is waited for only before
        // the first command runs (`settle_baseline`).
        if features.gate || self.goal.is_some() {
            let root = self.sess.ws.root.clone();
            self.pending_baseline =
                Some(tokio::spawn(async move { changes::baseline(&root).await }));
        }
        let mut parts: Vec<String> = Vec::new();
        if features.bootstrap {
            let env = bootstrap::snapshot(&self.sess).await;
            self.sink.emit(EventKind::Env { text: env.clone() });
            parts.push(env);
            // Known from the start, the budget is something to plan with; heard of at half-time, it
            // is only a reason to hurry.
            if features.scripts && !self.cfg.interactive {
                let time = self
                    .cfg
                    .time_budget
                    .map(|b| format!(" and {} minutes", (b.as_secs() / 60).max(1)))
                    .unwrap_or_default();
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
        let preflight = if features.bootstrap && self.cfg.autonomy == Autonomy::Auto {
            self.settle_baseline().await;
            let started = Instant::now();
            let report = watches::preflight(&mut self.sess, task, cancel).await;
            self.stats.tool_ms += started.elapsed().as_millis() as u64;
            report
        } else {
            None
        };
        if let Some(r) = preflight {
            self.sink.emit(EventKind::Watch {
                turn: self.turn,
                name: r.name,
                cmd: r.cmd,
                state: r.state,
                change: r.change,
                detail: r.detail.clone(),
            });
            parts.push(format!(
                "<tests>\n{}\n</tests>\n{}",
                r.detail,
                watches::PREFLIGHT_NOTE
            ));
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
    pub async fn run_with(
        &mut self,
        text: String,
        skills: &[SkillInfo],
        cancel: &CancellationToken,
    ) -> Outcome {
        let run_started = Instant::now();
        self.requests_before = self.stats.requests;
        self.sess.deadline = self.cfg.time_budget.map(|budget| run_started + budget);
        let elapsed_before = self.stats.elapsed_ms;
        let features = self.cfg.features;
        let task = text.trim().to_string();
        let files = std::mem::take(&mut self.attached);
        let shown = self.shown.take().unwrap_or_else(|| task.clone());
        self.sink.emit(EventKind::User {
            text: shown,
            skills: skills.iter().map(|s| s.name.clone()).collect(),
            files: files.clone(),
        });
        for change in std::mem::take(&mut self.announce) {
            self.sink.emit(change);
        }
        self.sess.task = task.clone();
        self.restore_fixed_effort();
        self.sess.evidence.message_epoch = self.sess.evidence.epoch;
        let first = std::mem::take(&mut self.first_message);
        // The mechanical contract reads the task the session began with: a later message is not held
        // to it, unless it carries on with that task after a restart cut its run off.
        self.spec_task = points_past_its_tests(&task);
        let carried = std::mem::take(&mut self.carry_contract);
        if !first {
            if !carried {
                self.contract = Contract::default();
            }
            self.contract_holds = 0;
        } else if features.protect {
            self.contract = Contract::capture(&self.sess.ws, &task);
            self.contract_holds = 0;
        }
        let mut composed = if first {
            self.opening(&task, cancel).await
        } else {
            task.clone()
        };
        if !files.is_empty() {
            composed = format!(
                "<attached_files>\n{}\n</attached_files>\nThe user attached these files to this message. Read the ones you need.\n\n{composed}",
                files.join("\n")
            );
        }
        if !skills.is_empty() {
            composed = format!("{}\n\n{composed}", skills::blocks(skills));
        }
        if let Some(note) = self.restored.take() {
            composed = format!("{note}\n\n{composed}");
        }
        self.ctx.push(Message::user(composed));
        self.budget_marks = 0;
        self.save_point(true).await;

        let (mut turns_used, mut empties, mut nudges_left, mut loop_retries, mut overthinking_left) =
            (0u32, 0u32, 1u32, 1u32, 3u32);
        // Replies the provider cut at its length limit that the model is told about and asked again.
        let mut stream_cuts_left = 3u32;
        // Replies of this run that were nothing but reasoning.
        let mut only_reasoned = 0u32;
        let mut slowest = Duration::ZERO;
        let mut end_checks = EndChecks::default();
        // Cut off for thinking without acting as often as that is allowed, the model gets one more
        // reply, the last of the run, to act or to report in (`wrap_up`); once it has come, the run
        // ends (`wrapped`).
        let (mut wrap_up, mut wrapped) = (false, false);
        let tools = prompt::tool_schemas(&features, self.cfg.interactive);
        let tools_tokens = crate::text::est_tokens(&tools.to_string());
        let outcome = loop {
            if cancel.is_cancelled() {
                break Outcome {
                    status: FinishStatus::Stopped,
                    summary: "Stopped by the user.".into(),
                    verified: self.sess.evidence.fresh(),
                    evidence: self.evidence_text(),
                };
            }
            let out_of_time = self.sess.deadline.is_some_and(|at| Instant::now() >= at);
            if turns_used >= self.cfg.max_turns || out_of_time {
                break self.out_of_budget(if out_of_time {
                    "time budget"
                } else {
                    "turn limit"
                });
            }
            if wrapped {
                break Outcome {
                    status: FinishStatus::Budget,
                    summary: "Stopped: the model was cut off three times in a row for reasoning without acting, and the reply it was given after that was the last.".into(),
                    verified: self.sess.evidence.fresh(),
                    evidence: self.evidence_text(),
                };
            }
            if self.remaining_requests() == Some(0) {
                break self.out_of_budget("request limit");
            }
            let landing = wrap_up
                || self.remaining_requests() == Some(1)
                || self.last_reply(run_started, turns_used, slowest);
            self.maybe_decay(turns_used, landing, run_started);
            let turn_started = Instant::now();
            self.turn += 1;
            turns_used += 1;
            self.stats.turns += 1;
            let turn = self.turn;
            self.sink.emit(EventKind::TurnStart {
                turn,
                effort: self.effort.as_str().to_string(),
            });

            let mut wire = self.provider.wire_messages(&self.ctx.messages);
            let compaction_start = self.ctx.revision;
            while crate::text::est_tokens(&wire) + tools_tokens > self.cfg.context_limit
                && self.ctx.compact_oldest()
            {
                wire = self.provider.wire_messages(&self.ctx.messages);
            }
            if self.ctx.revision != compaction_start {
                self.sess.seen.clear();
                self.note(
                    "context",
                    "Dropped older exchanges to fit the request. Re-read any missing evidence.",
                );
            }
            if crate::text::est_tokens(&wire) + tools_tokens > self.cfg.context_limit {
                break Outcome {
                    status: FinishStatus::Incomplete,
                    summary:
                        "The remaining task and recent conversation exceed the context budget."
                            .into(),
                    verified: false,
                    evidence: self.evidence_text(),
                };
            }
            // What this request comes to if it ends before the provider says: its prompt, and of
            // that what the cache held of the request before it.
            let prompt_estimate = crate::text::est_tokens(&wire) + tools_tokens;
            let prefix_stable = self.ctx.check_prefix(wire);
            let estimate = (
                prompt_estimate,
                if prefix_stable {
                    self.ctx.last_prompt().min(prompt_estimate)
                } else {
                    0
                },
            );
            let request = ChatRequest {
                messages: &self.ctx.messages,
                tools,
                effort: self.effort,
                max_tokens: self.cfg.max_tokens,
                reasoning_cap: if self.effort == Effort::Max {
                    LONG_THOUGHT_CHARS
                } else {
                    0
                },
            };
            let sink = self.sink.clone();
            let streamed = Arc::new(Streamed::default());
            let tally = streamed.clone();
            let mut on_delta = move |d: Delta| match d {
                Delta::Attempt => {
                    tally.attempts.fetch_add(1, Ordering::Relaxed);
                }
                Delta::Reasoning(text) => {
                    tally.seen(true, text.len());
                    sink.emit(EventKind::Delta {
                        turn,
                        channel: Channel::Reasoning,
                        text,
                    })
                }
                Delta::Content(text) => {
                    tally.seen(false, text.len());
                    sink.emit(EventKind::Delta {
                        turn,
                        channel: Channel::Content,
                        text,
                    })
                }
                Delta::CallBytes(bytes) => tally.seen(false, bytes),
                Delta::Retry {
                    attempt,
                    wait_ms,
                    reason,
                } => {
                    tally.retried();
                    sink.emit(EventKind::Retry {
                        turn,
                        attempt,
                        wait_ms,
                        reason,
                    })
                }
            };
            let asked = Instant::now();
            let provider = self.provider.clone();
            let deadline = self.sess.deadline;
            // The time budget holds inside a reply too: one still coming when it runs out is cut.
            let reply = tokio::select! {
                reply = provider.chat_with(request, &mut on_delta, cancel, self.ctx.last_wire(), self.remaining_requests()) => Some(reply),
                _ = until(deadline) => None,
            };
            let model_ms = asked.elapsed().as_millis() as u64;
            self.stats.model_ms += model_ms;
            self.stats.requests += streamed.attempts.load(Ordering::Relaxed);
            let Some(reply) = reply else {
                self.book_unreported(&streamed, true, estimate);
                break self.out_of_budget("time budget");
            };
            let resp = match reply {
                Ok(r) => {
                    self.book_unreported(&streamed, false, estimate);
                    r
                }
                Err(ProviderError::Cancelled) => {
                    self.book_unreported(&streamed, true, estimate);
                    continue;
                }
                Err(ProviderError::StreamCut(secs)) if stream_cuts_left > 0 => {
                    self.book_unreported(&streamed, true, estimate);
                    stream_cuts_left -= 1;
                    let msg = format!(
                        "The provider ends any reply that streams longer than about {secs} seconds, and your last one was cut there twice: none of it arrived, so nothing in it ran. Keep each reply well under that: think briefly, make one change or write one file per call, and write a long file in parts (create it, then add to it with edit)."
                    );
                    self.note("stream", &msg);
                    self.ctx.push(Message::user(format!("[harness] {msg}")));
                    continue;
                }
                Err(ProviderError::Degenerate) if loop_retries > 0 => {
                    self.book_unreported(&streamed, true, estimate);
                    loop_retries -= 1;
                    let msg = "Your last reply got stuck repeating itself and was cut off. Do not think it through again: send the next tool call now.";
                    self.note("loop", msg);
                    self.ctx.push(Message::user(format!("[harness] {msg}")));
                    continue;
                }
                Err(ProviderError::Overthinking(chars)) if overthinking_left > 0 => {
                    overthinking_left -= 1;
                    // The stream was cut before the provider reported usage: what was seen is
                    // booked, priced like every other request.
                    streamed.at_least(chars);
                    self.book_unreported(&streamed, true, estimate);
                    let tokens = chars as u64 / 4;
                    let mut msg = format!(
                        "Your last reply was cut off after about {tokens} tokens of thinking with no tool call. Do not work the whole task out in your head: send the first tool calls now and think between steps, with their results in front of you."
                    );
                    self.note("overthinking", &msg);
                    only_reasoned += 1;
                    if let Some(lower) = less_thinking(self.effort, only_reasoned) {
                        self.sink.emit(EventKind::Effort {
                            turn,
                            from: self.effort.as_str().into(),
                            to: lower.as_str().into(),
                            reason: "a reply that only reasoned".into(),
                        });
                        self.effort = lower;
                    }
                    if overthinking_left == 0 {
                        wrap_up = true;
                        self.note("harness", WRAP_UP_NOTE);
                        msg = format!("{msg}\n{WRAP_UP_NOTE}");
                    }
                    self.ctx.push(Message::user(format!("[harness] {msg}")));
                    continue;
                }
                Err(e) => {
                    if let ProviderError::Overthinking(chars) = e {
                        streamed.at_least(chars);
                    }
                    self.book_unreported(&streamed, true, estimate);
                    self.sink.emit(EventKind::Error {
                        message: e.to_string(),
                    });
                    break Outcome {
                        status: FinishStatus::Error,
                        summary: e.to_string(),
                        verified: self.sess.evidence.fresh(),
                        evidence: self.evidence_text(),
                    };
                }
            };
            wrapped = wrap_up;
            // What the workspace held before the session is taken while the first reply is coming,
            // and has to be in before anything of the reply runs.
            self.settle_baseline().await;

            self.stats.retries += resp.retries;
            self.stats.prompt_tokens += resp.usage.prompt;
            self.stats.cached_tokens += resp.usage.cached;
            self.stats.output_tokens += resp.usage.output;
            self.stats.reasoning_tokens += resp.usage.reasoning;
            self.stats.context_tokens = resp.usage.prompt;
            let cost = self
                .sess
                .prices
                .map(|p| p.cost(&resp.usage) * peak_multiplier(&self.cfg.model, now_ms() / 1000));
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
            let cut = resp.finish == "length"
                || (resp.finish == crate::provider::openai::CUT_FINISH
                    && raw_calls.last().is_some_and(|c| {
                        serde_json::from_str::<serde_json::Value>(c.arguments.trim()).is_err()
                    }))
                || (resp.finish.is_empty()
                    && raw_calls
                        .last()
                        .is_some_and(|c| repair::cut_off(&c.arguments)));
            if cut && let Some(last) = calls.last_mut() {
                let why = if resp.finish == "length" {
                    "hit the output limit"
                } else if resp.finish == crate::provider::openai::CUT_FINISH {
                    "was cut off by the provider (it ends replies that stream too long)"
                } else {
                    "broke off"
                };
                last.error = Some(format!(
                    "your reply {why} in the middle of this call, so it was not run. Send it again; split large content into smaller steps."
                ));
            }
            self.stats.repairs += repairs;
            self.sink.emit(EventKind::Model {
                turn,
                content: resp.content.clone(),
                reasoning: resp.reasoning.clone(),
                calls: raw_calls
                    .iter()
                    .map(|c| CallInfo {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        arguments: c.arguments.clone(),
                    })
                    .collect(),
                usage: resp.usage,
                cost,
                finish: resp.finish.clone(),
                ms: model_ms,
                repairs,
            });
            let (expected, verdict) =
                self.ctx
                    .audit(&resp.usage, resp.usage_reported, prefix_stable);
            match verdict {
                CacheVerdict::Miss => self.stats.cache_misses += 1,
                CacheVerdict::Bust => self.stats.cache_busts += 1,
                _ => {}
            }
            self.sink.emit(EventKind::Cache {
                turn,
                prompt: resp.usage.prompt,
                cached: resp.usage.cached,
                expected,
                prefix_stable,
                verdict,
            });

            self.ctx.push(Message {
                role: Role::Assistant,
                content: resp.content.clone(),
                reasoning: (!resp.reasoning.is_empty()).then(|| resp.reasoning.clone()),
                tool_calls: raw_calls,
                tool_call_id: None,
            });
            // The reply is paid for: a run that dies while its calls run keeps it.
            self.save_point(true).await;

            if calls.is_empty() {
                // A reply the output limit cut off is not an answer, whatever text it got to.
                let cut_off = resp.finish == "length";
                if resp.content.trim().is_empty() || cut_off {
                    empties += 1;
                    if empties > 2 {
                        let message = "the model returned neither a tool call nor an answer three times in a row".to_string();
                        self.sink.emit(EventKind::Error {
                            message: message.clone(),
                        });
                        break Outcome {
                            status: FinishStatus::Error,
                            summary: message,
                            verified: self.sess.evidence.fresh(),
                            evidence: self.evidence_text(),
                        };
                    }
                    let msg = if cut_off {
                        // Thinking that fills the whole reply will do so again unless it is given less room.
                        let reasoned = resp.usage.reasoning * 10 >= resp.usage.output * 9;
                        only_reasoned += u32::from(reasoned);
                        if let Some(lower) =
                            less_thinking(self.effort, only_reasoned).filter(|_| reasoned)
                        {
                            self.sink.emit(EventKind::Effort {
                                turn,
                                from: self.effort.as_str().into(),
                                to: lower.as_str().into(),
                                reason: "a reply that only reasoned".into(),
                            });
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
                if !self.cfg.interactive && nudges_left > 0 && !landing {
                    nudges_left -= 1;
                    let msg = match features.gate && !self.sess.evidence.checked() {
                        true => format!("You replied without calling finish, and nothing has been checked since {}. Run a check and call finish, or call finish with `unverified` and the reason.", self.sess.evidence.last_change),
                        false => "You replied without a tool call, and nobody is here to read it or to answer. If the task is not done, carry on with tool calls. If that was your answer, call finish with it as the summary.".to_string(),
                    };
                    self.note("gate", &msg);
                    self.ctx.push(Message::user(format!("[harness] {msg}")));
                    continue;
                }
                let outcome = Outcome {
                    status: FinishStatus::Yielded,
                    summary: resp.content,
                    verified: self.sess.evidence.fresh(),
                    evidence: self.evidence_text(),
                };
                match self
                    .before_ending(outcome, landing, &mut end_checks, cancel)
                    .await
                {
                    Ending::End(outcome) => break outcome,
                    Ending::Carry(message) => {
                        self.send_back(&message, run_started, elapsed_before);
                        continue;
                    }
                }
            }
            empties = 0;
            // The model has acted: a level it was taken off for a reply that only reasoned is
            // back, and such replies are counted afresh.
            overthinking_left = 3;
            self.restore_fixed_effort();

            let result = Runner {
                sess: &mut self.sess,
                sink: self.sink.as_ref(),
                cancel,
                approvals: &self.approvals,
                stats: &mut self.stats,
                turn,
                autonomy: self.cfg.autonomy,
                interactive: self.cfg.interactive,
                landing,
            }
            .run(&calls)
            .await;
            slowest = slowest.max(turn_started.elapsed());
            // Nothing changed and nothing went wrong: what a review that passes looks like.
            let quiet =
                result.edited.is_empty() && !result.surprised && result.failed_steps.is_empty();

            self.surprise_streak = if result.surprised {
                self.surprise_streak + 1
            } else {
                0
            };
            let mut footer = result.footer.clone();
            let mut notes = self.stall_notes(&result);
            // Advice on how to spend replies is for a run that has some left.
            let closing = landing || self.last_reply(run_started, turns_used, slowest);
            // Models are trained to wait for each result. Show, on their own behaviour, that here
            // they need not: twice at most, then it is their call.
            if features.scripts
                && self.chain_hints > 0
                && result.finished.is_none()
                && !result.surprised
                && !closing
            {
                use crate::tools::Tool;
                let tools: Vec<Tool> = calls.iter().filter_map(|c| c.tool).collect();
                let only_changes = !tools.is_empty()
                    && tools.iter().all(|t| matches!(t, Tool::Edit | Tool::Write));
                let checked = self.sess.evidence.epoch > 0 && self.sess.evidence.fresh();
                let hint = if checked && (only_changes || tools.last() == Some(&Tool::Shell)) {
                    Some(
                        "Tip: every check passes, so once every rule in the task and its spec is covered, `finish` is accepted now and could have been in the same reply (it is skipped or refused automatically whenever a check fails).",
                    )
                } else if only_changes && self.sess.watches.items.is_empty() {
                    Some(
                        "Tip: send the check (shell with expect_exit) and `finish` together in your next reply: if the check fails, `finish` is skipped automatically. They can ride along with the edits just the same.",
                    )
                } else {
                    None
                };
                if let Some(h) = hint {
                    self.chain_hints -= 1;
                    notes.push(h.to_string());
                }
            }
            self.solo_streak = if calls.len() == 1 && result.finished.is_none() {
                self.solo_streak + 1
            } else {
                0
            };
            if features.scripts
                && self.solo_hints > 0
                && self.solo_streak >= 4
                && !result.surprised
                && !closing
            {
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
                self.ctx
                    .append_to_last(&format!("\n--- harness ---\n{}", footer.join("\n")));
            }
            self.emit_stats(run_started, elapsed_before);
            self.save_point(true).await;
            // Out of time, a reply that finished or asked still ends the run with what it said;
            // only one that did neither is cut off as out of budget.
            let out_of_time = self
                .sess
                .deadline
                .is_some_and(|deadline| Instant::now() >= deadline);
            if out_of_time && result.asked.is_none() && result.finished.is_none() {
                break self.out_of_budget("time budget");
            }

            // A question ends the run: the answer is the user's next message.
            if let Some(asked) = result.asked {
                self.sink.emit(EventKind::Question {
                    turn,
                    question: asked.question.clone(),
                    choices: asked.choices,
                });
                break Outcome {
                    status: FinishStatus::Yielded,
                    summary: asked.question,
                    verified: self.sess.evidence.fresh(),
                    evidence: self.evidence_text(),
                };
            }
            if result.finished.is_none() && end_checks.held.is_some() {
                end_checks.review_replies += 1;
                if let Some(outcome) = self.review_passed(&calls, quiet, &mut end_checks) {
                    break outcome;
                }
                if end_checks.review_replies == SPEC_REVIEW_REPLIES && !landing {
                    let msg = format!(
                        "The review has taken {SPEC_REVIEW_REPLIES} replies. Finish now: call `finish`, and name in the summary any rule that still fails."
                    );
                    self.note("spec", &msg);
                    self.ctx.push(Message::user(format!("[harness] {msg}")));
                }
            }
            if let Some(done) = result.finished {
                end_checks.held = None;
                let outcome = Outcome {
                    status: FinishStatus::Done,
                    summary: done.summary,
                    verified: done.verified,
                    evidence: done.evidence,
                };
                // With the time gone nothing can follow: the checks are made for the record only.
                match self
                    .before_ending(outcome, landing || out_of_time, &mut end_checks, cancel)
                    .await
                {
                    Ending::End(outcome) => break outcome,
                    Ending::Carry(message) => {
                        self.send_back(&message, run_started, elapsed_before);
                        continue;
                    }
                }
            }
            if let (EffortPolicy::Ratchet(_), true) = (self.cfg.effort, self.surprise_streak >= 3)
                && let Some(next) = self.effort.next()
            {
                self.sink.emit(EventKind::Effort {
                    turn,
                    from: self.effort.as_str().into(),
                    to: next.as_str().into(),
                    reason: "three surprises in a row".into(),
                });
                self.effort = next;
                self.surprise_streak = 0;
            }
        };

        self.stats.elapsed_ms = elapsed_before + run_started.elapsed().as_millis() as u64;
        self.settle_baseline().await;
        self.sess.deadline = None;
        self.save_point(false).await;
        self.sink.emit(EventKind::Stats {
            stats: self.stats.clone(),
        });
        self.sink.emit(EventKind::Finish {
            status: outcome.status,
            summary: outcome.summary.clone(),
            verified: outcome.verified,
            evidence: outcome.evidence.clone(),
        });
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
    async fn before_ending(
        &mut self,
        mut outcome: Outcome,
        landing: bool,
        checks: &mut EndChecks,
        cancel: &CancellationToken,
    ) -> Ending {
        if let Some(message) = self.contract_check(&mut outcome, landing) {
            return Ending::Carry(message);
        }
        if let Some(message) = self.spec_review(&outcome, landing, checks) {
            return Ending::Carry(message);
        }
        if let Some(message) = self.goal_check(&mut outcome, landing, checks, cancel).await {
            return Ending::Carry(message);
        }
        Ending::End(outcome)
    }

    /// "Guard files": the files the task says not to change are checked before the run ends. The
    /// contract is empty unless the session captured one (`Features::protect`) or carries one
    /// over a restart.
    fn contract_check(&mut self, outcome: &mut Outcome, landing: bool) -> Option<String> {
        let ev = &self.sess.evidence;
        let check_due = ev.epoch > ev.message_epoch || outcome.status == FinishStatus::Done;
        if self.contract.is_empty() || !check_due || self.cfg.autonomy != Autonomy::Auto {
            return None;
        }
        let violations = self.contract.violations(&self.sess.ws.root);
        if violations.is_empty() {
            return None;
        }
        let detail = violations.join("; ");
        if landing || self.contract_holds >= CONTRACT_HOLDS {
            outcome.status = FinishStatus::Incomplete;
            outcome.verified = false;
            outcome.evidence = format!("{}; contract unmet: {detail}", outcome.evidence);
            self.note(
                "contract",
                &format!("left unmet after {CONTRACT_HOLDS} holds: {detail}"),
            );
            return None;
        }
        self.contract_holds += 1;
        if let EffortPolicy::Ratchet(_) = self.cfg.effort
            && let Some(next) = self.effort.next()
        {
            self.sink.emit(EventKind::Effort {
                turn: self.turn,
                from: self.effort.as_str().into(),
                to: next.as_str().into(),
                reason: "a file the task said not to change was changed".into(),
            });
            self.effort = next;
        }
        self.note("contract", &format!("finish held: {detail}"));
        Some(Contract::hold_message(&violations))
    }

    /// The spec review: a task that says its tests check only part of what it asks, or that holds the
    /// work to a written specification, has its first `finish` held once. A passing test is all the
    /// gate needs, and a model tends to stop there; what the tests leave out is where it was wrong
    /// (an exit status the README gives, one entry of a manifest a migration note covers).
    fn spec_review(
        &mut self,
        outcome: &Outcome,
        landing: bool,
        checks: &mut EndChecks,
    ) -> Option<String> {
        let ev = &self.sess.evidence;
        if !self.spec_task
            || checks.spec
            || landing
            || outcome.status != FinishStatus::Done
            || ev.epoch <= ev.message_epoch
            || self.cfg.autonomy != Autonomy::Auto
            || !self.cfg.features.spec_review
            || !self.cfg.features.gate
        {
            return None;
        }
        checks.spec = true;
        self.note(
            "spec",
            "finish held once for a review against the specification",
        );
        Some(if self.cfg.features.watches {
            checks.held = Some(outcome.clone());
            SPEC_REVIEW.to_string()
        } else {
            SPEC_REVIEW_PLAIN.to_string()
        })
    }

    /// The held `finish` stands when a reply did nothing but check, every check passed and every
    /// watch, the review's own among them, passes now: asking the model to say `finish` again
    /// would cost a reply and change nothing. A reply that changed files has to call `finish`
    /// itself, since what it says may no longer hold.
    fn review_passed(
        &mut self,
        calls: &[repair::Call],
        quiet: bool,
        checks: &mut EndChecks,
    ) -> Option<Outcome> {
        use crate::tools::Tool;
        let checked = calls
            .iter()
            .any(|c| matches!(c.tool, Some(Tool::Shell | Tool::Watch)));
        let only_looked = calls.iter().all(|c| {
            matches!(
                c.tool,
                Some(Tool::Shell | Tool::Watch | Tool::Read | Tool::Search)
            )
        });
        let watches = &self.sess.watches;
        let review = watches.items.iter().any(|w| w.name == "spec");
        if !checked
            || !only_looked
            || !quiet
            || !review
            || !watches.all_pass_at(self.sess.evidence.epoch)
            || !self.sess.evidence.fresh()
        {
            return None;
        }
        let mut outcome = checks.held.take()?;
        outcome.verified = true;
        outcome.evidence = self.evidence_text();
        outcome.summary = format!(
            "{}\n\nReviewed against the specification: watch `spec` passes.",
            outcome.summary
        );
        self.note("spec", "the review's checks pass: the held finish stands");
        Some(outcome)
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
    async fn goal_check(
        &mut self,
        outcome: &mut Outcome,
        landing: bool,
        checks: &mut EndChecks,
        cancel: &CancellationToken,
    ) -> Option<String> {
        let text = self.goal.as_ref().filter(|goal| !goal.met)?.text.clone();
        let max_attempts = self.remaining_requests();
        if max_attempts == Some(0) {
            let why = "the model request limit was reached";
            self.sink.emit(goal_event(GoalState::Unchecked, text, why));
            outcome.status = FinishStatus::Incomplete;
            outcome.verified = false;
            outcome
                .evidence
                .push_str("; goal could not be checked: the model request limit was reached");
            return None;
        }
        checks.goal += 1;
        self.stats.goal_checks += 1;
        let changes = tokio::select! {
            report = changes::changes(&self.sess) => report,
            _ = until(self.sess.deadline) => None,
        };
        let evidence = if self.cfg.features.gate {
            self.evidence_text()
        } else {
            "not tracked in classic mode".to_string()
        };
        let material = goal::Material {
            goal: &text,
            summary: &outcome.summary,
            evidence: &evidence,
            watches: &self.sess.watches,
            changes: changes.as_deref(),
            epoch: self.sess.evidence.epoch,
        };
        let asked = Instant::now();
        // Not past the time budget, as no reply of the run is.
        let streamed = Streamed::default();
        let mut on_delta = |delta| match delta {
            Delta::Attempt => {
                streamed.attempts.fetch_add(1, Ordering::Relaxed);
            }
            Delta::Reasoning(text) => streamed.seen(true, text.len()),
            Delta::Content(text) => streamed.seen(false, text.len()),
            Delta::CallBytes(bytes) => streamed.seen(false, bytes),
            Delta::Retry { .. } => streamed.retried(),
        };
        let prompt =
            crate::text::est_tokens(&self.provider.wire_messages(&goal::messages(&material)));
        let answer = tokio::select! {
            biased;
            _ = until(self.sess.deadline) => Err("the time budget ran out before it was answered"),
            answer = goal::ask(&self.provider, &material, cancel, &mut on_delta, max_attempts) => Ok(answer),
        };
        self.stats.requests += streamed.attempts.load(Ordering::Relaxed);
        self.book_unreported(&streamed, !matches!(&answer, Ok(Ok(_))), (prompt, 0));
        self.stats.model_ms += asked.elapsed().as_millis() as u64;
        let verdict = match answer {
            Err(late) => Err(late.to_string()),
            Ok(answer) => match answer {
                Err(ProviderError::Cancelled) => {
                    *outcome = Outcome {
                        status: FinishStatus::Stopped,
                        summary: "Stopped by the user.".into(),
                        verified: self.sess.evidence.fresh(),
                        evidence: self.evidence_text(),
                    };
                    return None;
                }
                Err(e) => Err(e.to_string()),
                Ok(answer) => {
                    self.count_usage(answer.usage, answer.retries);
                    answer
                        .verdict
                        .ok_or_else(|| "its answer did not start with MET or NOT MET".to_string())
                }
            },
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
                self.sink
                    .emit(goal_event(GoalState::NotMet, text, &reasons));
                if !landing && checks.goal < goal::MAX_CHECKS {
                    return Some(goal::work_on(&reasons));
                }
                let after = if landing {
                    String::new()
                } else {
                    format!(" after {} checks", checks.goal)
                };
                outcome.status = FinishStatus::Incomplete;
                outcome.verified = false;
                format!("goal not met{after}: {}", one_line(&reasons, 160))
            }
            Err(why) => {
                outcome.status = FinishStatus::Incomplete;
                outcome.verified = false;
                self.sink.emit(goal_event(GoalState::Unchecked, text, &why));
                format!("goal could not be checked: {}", one_line(&why, 160))
            }
        };
        outcome.evidence = format!("{}; {note}", outcome.evidence);
        None
    }

    /// Books tokens and their cost for a request that is no ordinary turn: a goal check, or a
    /// reply cut off before the provider reported its usage.
    fn count_usage(&mut self, usage: Usage, retries: u32) {
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
    EventKind::Goal {
        state,
        text,
        detail: detail.to_string(),
    }
}

/// What a tool call that a restart cut off is answered with.
const INTERRUPTED: &str = "[interrupted by a restart: this call may not have run, or may have run in part. Check the files before relying on it.]";

/// What the model is told in front of the first message of a session that was brought back.
fn restored_note(cut_off: bool, answered: u32) -> String {
    let mut note = "[harness] This session was restored after a restart of the program. Background jobs and shell processes from before are gone, and the files are as they are on disk now, which may differ from what you last saw: read before you rely on what you remember.".to_string();
    if cut_off {
        note.push_str(" The run before this message was cut off by the restart");
        note.push_str(if answered > 0 {
            ": the tool calls it ended on were not finished, and their results say so."
        } else {
            "."
        });
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
    /// again), the turn's streak counters, what is queued for the next message (`announce`,
    /// `attached`, `shown`) and the provider's cache record. The goal is in `meta.json`.
    pub fn checkpoint(&self, in_run: bool, setup: &Setup) -> Snapshot {
        let mut snapshot = self.checkpoint_state(in_run, setup);
        snapshot.messages = self.ctx.messages.clone();
        snapshot.stubs = self.ctx.stubs().to_vec();
        snapshot
    }

    fn checkpoint_state(&self, in_run: bool, setup: &Setup) -> Snapshot {
        Snapshot {
            version: resume::VERSION,
            saved: now_ms(),
            in_run,
            setup: setup.clone(),
            model: self.cfg.model.clone(),
            autonomy: if self.cfg.autonomy == Autonomy::Ask {
                "ask"
            } else {
                "auto"
            }
            .to_string(),
            interactive: self.cfg.interactive,
            max_turns: self.cfg.max_turns,
            max_requests: self.cfg.max_requests,
            time_budget_secs: self.cfg.time_budget.map(|d| d.as_secs()),
            effort_policy: self.cfg.effort.label(),
            effort: self.effort.as_str().to_string(),
            messages: Vec::new(),
            stubs: Vec::new(),
            turn: self.turn,
            stats: self.stats.clone(),
            edits: self
                .edits
                .iter()
                .map(|(path, n)| (path.clone(), *n))
                .collect(),
            chain_hints: self.chain_hints,
            solo_hints: self.solo_hints,
            session: self.sess.checkpoint(),
            contract: self.contract.clone(),
        }
    }

    /// Writes the snapshot, if this agent keeps one. A snapshot that cannot be written costs only
    /// the chance to continue after a restart, never the run.
    /// The file is written and synced on a blocking thread: the runtime goes on serving the other
    /// sessions meanwhile, and this one waits, so that snapshots land in the order they were taken.
    async fn save_point(&mut self, in_run: bool) {
        let Some((path, setup)) = &self.keep else {
            return;
        };
        let path = path.clone();
        let state = self.checkpoint_state(in_run, setup);
        let revision = self.ctx.revision;
        let previous = self.journal.take().filter(|j| j.revision == revision);
        let from = previous.as_ref().map_or(0, |j| j.entries);
        let messages = self.ctx.messages[from..].to_vec();
        let stubs = self.ctx.stubs()[from..].to_vec();
        let result = tokio::task::spawn_blocking(move || {
            resume::write_checkpoint(&path, state, previous, revision, messages, stubs)
        })
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e)));
        if let Err(error) = &result
            && !self.snapshot_warned
        {
            self.sink.emit(EventKind::Error {
                message: format!("Could not save the session snapshot: {error}"),
            });
        }
        self.snapshot_warned = result.is_err();
        self.journal = result.ok();
    }

    /// An agent as the snapshot left it: the conversation, the turn counter and the stats, the
    /// policies the session had, the standing checks, and `goal` from `meta.json`. `cfg` has the
    /// features and the model, which the caller works out; the rest of it is the snapshot's.
    /// Returns the agent and how many tool calls the conversation ended on with no result, which
    /// were answered as interrupted so that a provider accepts it. The first message it gets is told
    /// the session was restored.
    pub fn restore(
        provider: Provider,
        mut sess: Session,
        mut cfg: AgentConfig,
        sink: Arc<dyn Sink>,
        approvals: Arc<Approvals>,
        snap: Snapshot,
        goal: Option<Goal>,
    ) -> (Self, u32) {
        cfg.autonomy = if snap.autonomy == "ask" {
            Autonomy::Ask
        } else {
            Autonomy::Auto
        };
        cfg.interactive = snap.interactive;
        cfg.max_turns = snap.max_turns;
        cfg.max_requests = snap.max_requests;
        cfg.time_budget = snap.time_budget_secs.map(Duration::from_secs);
        if let Some(policy) = EffortPolicy::parse(&snap.effort_policy) {
            cfg.effort = policy;
        }
        sess.restore(snap.session);
        let mut agent = Self::new(provider, sess, cfg, sink, approvals);
        agent.ctx = Context::restored(snap.messages, snap.stubs);
        let answered = agent.ctx.answer_dangling(INTERRUPTED);
        // A step the restart cut off may have changed files after the last check.
        if answered > 0 {
            agent.sess.evidence.changed("a step that a restart cut off");
        }
        // The run of the first task was cut off: continuing it is still that task.
        agent.carry_contract = snap.in_run;
        agent.contract = snap.contract;
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
mod tests;
