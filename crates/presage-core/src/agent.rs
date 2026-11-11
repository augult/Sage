//! The agent loop: one model reply, one script, repeat until the task is finished or handed back.

use crate::attach;
use crate::bootstrap;
use crate::context::Context;
use crate::economy::peak_multiplier;
use crate::events::{CacheVerdict, CallInfo, Channel, EventKind, FinishStatus, Sink, Stats};
use crate::prompt;
use crate::provider::{ChatRequest, Delta, Effort, Message, Provider, ProviderError, Role};
use crate::repair;
use crate::script::{Approvals, Autonomy, Runner, ScriptResult};
use crate::skills::{self, SkillInfo};
use crate::state::{Features, Session};
use crate::util::now_ms;
use crate::watches;
use std::collections::HashMap;
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
            "ratchet" | "auto" | "adaptive" => Some(EffortPolicy::Ratchet(Effort::High)),
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
}

impl AgentConfig {
    pub fn new(model: &str) -> Self {
        Self {
            features: Features::presage(),
            effort: EffortPolicy::Ratchet(Effort::High),
            model: model.to_string(),
            max_turns: 80,
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
    provider: Provider,
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
    /// The `job` tool was in the list sent with the previous request.
    offered_job_tool: bool,
}

const KEEP_RECENT_MESSAGES: usize = 12;
const MIN_DECAY_TOKENS: u64 = 6_000;

impl Agent {
    pub fn new(provider: Provider, sess: Session, cfg: AgentConfig, sink: Arc<dyn Sink>, approvals: Arc<Approvals>) -> Self {
        let ctx = Context::new(cfg.system.as_deref().unwrap_or_else(|| prompt::system_prompt(&cfg.features)));
        let effort = cfg.effort.start();
        Self { provider, sess, ctx, cfg, sink, approvals, stats: Stats::default(), turn: 0, effort, surprise_streak: 0, failures: HashMap::new(), edits: HashMap::new(), first_message: true, budget_marks: 0, chain_hints: 2, offered_job_tool: false }
    }

    pub fn effort(&self) -> Effort {
        self.effort
    }

    fn note(&self, kind: &str, text: &str) {
        self.sink.emit(EventKind::Note { turn: self.turn, kind: kind.to_string(), text: text.to_string() });
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

    fn budget_notes(&mut self, run_started: Instant, turns_used: u32) -> Vec<String> {
        let mut notes = Vec::new();
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
    /// test command it names). With none of that, it is the task as given.
    async fn opening(&mut self, task: &str, cancel: &CancellationToken) -> String {
        let features = self.cfg.features;
        let mut parts: Vec<String> = Vec::new();
        if features.bootstrap {
            let env = bootstrap::snapshot(&self.sess).await;
            self.sink.emit(EventKind::Env { text: env.clone() });
            if features.lessons {
                for lesson in self.sess.lessons.top(5) {
                    self.sink.emit(EventKind::Lesson { learned: false, text: lesson.describe() });
                }
            }
            parts.push(env);
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
        if parts.is_empty() {
            return task.to_string();
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
        self.sink.emit(EventKind::User { text: task.clone(), skills: skills.iter().map(|s| s.name.clone()).collect() });
        self.sess.task = task.clone();
        self.sess.evidence.reviewed = false;
        self.sess.evidence.message_epoch = self.sess.evidence.epoch;
        let mut composed = if std::mem::take(&mut self.first_message) { self.opening(&task, cancel).await } else { task };
        if !skills.is_empty() {
            composed = format!("{}\n\n{composed}", skills::blocks(skills));
        }
        self.ctx.push(Message::user(composed));
        self.budget_marks = 0;

        let (mut turns_used, mut empties, mut nudges_left, mut loop_retries) = (0u32, 0u32, 1u32, 1u32);
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
            self.turn += 1;
            turns_used += 1;
            self.stats.turns += 1;
            let turn = self.turn;
            self.sink.emit(EventKind::TurnStart { turn, effort: self.effort.as_str().to_string() });

            // A longer tool list changes the very start of the prompt, just as a rewritten message does.
            let jobs = self.sess.jobs.any_started();
            let tools_stable = std::mem::replace(&mut self.offered_job_tool, jobs) == jobs;
            let prefix_stable = self.ctx.check_prefix(self.provider.wire_messages(&self.ctx.messages)) && tools_stable;
            let request = ChatRequest { messages: &self.ctx.messages, tools: prompt::tool_schemas(&features, jobs), effort: self.effort, max_tokens: self.cfg.max_tokens };
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
            if resp.finish == "length" {
                // A call cut off by the output limit may be a half-written command or file: never run it.
                if let Some(last) = calls.last_mut() {
                    last.error = Some("your reply hit the output limit in the middle of this call, so it was not run. Send it again; split large content into smaller steps.".to_string());
                }
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

            if calls.is_empty() {
                if resp.content.trim().is_empty() {
                    empties += 1;
                    if empties > 2 {
                        let message = "the model returned neither a tool call nor an answer three times in a row".to_string();
                        self.sink.emit(EventKind::Error { message: message.clone() });
                        break Outcome { status: FinishStatus::Error, summary: message, verified: self.sess.evidence.fresh(), evidence: self.evidence_text() };
                    }
                    let msg = if resp.finish == "length" {
                        "Your reply hit the output limit before any tool call or answer. Keep the reasoning short and send the tool calls."
                    } else {
                        "Your reply contained no tool call and no answer. Continue the task: call a tool, or call finish."
                    };
                    self.note("empty", msg);
                    self.ctx.push(Message::user(format!("[harness] {msg}")));
                    continue;
                }
                // Prose only. With nobody watching, unchecked changes get one reminder of the gate.
                if features.gate && !self.cfg.interactive && !self.sess.evidence.fresh() && nudges_left > 0 {
                    nudges_left -= 1;
                    let msg = format!("You replied without calling finish, and nothing has been checked since {}. Run a check and call finish, or call finish with `unverified` and the reason.", self.sess.evidence.last_change);
                    self.note("gate", &msg);
                    self.ctx.push(Message::user(format!("[harness] {msg}")));
                    continue;
                }
                break Outcome { status: FinishStatus::Yielded, summary: resp.content, verified: self.sess.evidence.fresh(), evidence: self.evidence_text() };
            }
            empties = 0;

            let result = Runner { sess: &mut self.sess, sink: self.sink.as_ref(), cancel, approvals: &self.approvals, stats: &mut self.stats, turn, autonomy: self.cfg.autonomy, interactive: self.cfg.interactive }
                .run(&calls)
                .await;

            self.surprise_streak = if result.surprised { self.surprise_streak + 1 } else { 0 };
            let mut footer = result.footer.clone();
            let mut notes = self.stall_notes(&result);
            // Models are trained to wait for each result. Show, on their own behaviour, that here
            // they need not: twice at most, then it is their call.
            if features.scripts && self.chain_hints > 0 && result.finished.is_none() && !result.surprised {
                use crate::tools::Tool;
                let tools: Vec<Tool> = calls.iter().filter_map(|c| c.tool).collect();
                let only_changes = !tools.is_empty() && tools.iter().all(|t| matches!(t, Tool::Edit | Tool::Write));
                let checked = self.sess.evidence.epoch > 0 && self.sess.evidence.fresh();
                let hint = if checked && (only_changes || tools.last() == Some(&Tool::Shell)) {
                    Some("Tip: every check passes, so `finish` is accepted now and could have been in the same reply. It is skipped or refused automatically whenever a check fails.")
                } else if only_changes && self.sess.watches.items.is_empty() {
                    Some("Tip: send the check (shell with expect_exit) and `finish` together in your next reply: if the check fails, `finish` is skipped automatically. They can ride along with the edits just the same.")
                } else {
                    None
                };
                if let Some(h) = hint {
                    self.chain_hints -= 1;
                    notes.push(h.to_string());
                }
            }
            notes.extend(self.budget_notes(run_started, turns_used));
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

            if let Some(done) = result.finished {
                break Outcome { status: FinishStatus::Done, summary: done.summary, verified: done.verified, evidence: done.evidence };
            }
            if let (EffortPolicy::Ratchet(_), true) = (self.cfg.effort, self.surprise_streak >= 3) {
                if let Some(next) = self.effort.next() {
                    self.sink.emit(EventKind::Effort { turn, from: self.effort.as_str().into(), to: next.as_str().into(), reason: "three surprises in a row".into() });
                    self.effort = next;
                    self.surprise_streak = 0;
                }
            }
        };

        self.stats.elapsed_ms = elapsed_before + run_started.elapsed().as_millis() as u64;
        self.sink.emit(EventKind::Stats { stats: self.stats.clone() });
        self.sink.emit(EventKind::Finish { status: outcome.status, summary: outcome.summary.clone(), verified: outcome.verified, evidence: outcome.evidence.clone() });
        outcome
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
        // The review pass has its own test; the others script one reply too few for it.
        let mut cfg = cfg;
        cfg.features.review = cfg.features.review && name == "agent-review";
        let sess = test_session(name, cfg.features);
        std::fs::write(sess.ws.root.join("n.txt"), "alpha\nbeta\ngamma\n").unwrap();
        let sink = Arc::new(MemorySink::default());
        let agent = Agent::new(Provider::Mock(MockProvider::new(turns)), sess, cfg, sink.clone(), Arc::new(Approvals::default()));
        Rig { agent, sink }
    }

    fn fixed(t: MockTurn) -> TurnFn {
        Box::new(move |_| t.clone())
    }

    #[tokio::test]
    async fn the_first_finish_after_a_change_gets_one_review_pass() {
        let change = |old: &str, new: &str| {
            fixed(MockTurn::calls("", vec![("edit", json!({"path": "n.txt", "old": old, "new": new})), ("shell", json!({"cmd": "grep -c A n.txt", "expect_exit": "0"})), ("finish", json!({"summary": "done"}))]))
        };
        let finish = || fixed(MockTurn::calls("", vec![("finish", json!({"summary": "done"}))]));
        let mut r = rig("agent-review", AgentConfig::new("mock"), vec![change("beta", "BETA"), change("gamma", "GAMMA"), finish()]);

        let out = r.agent.run("Upper-case the second line of n.txt.".into(), &CancellationToken::new()).await;
        assert_eq!((out.status, out.verified, r.agent.stats.turns), (FinishStatus::Done, true, 2), "the review is asked for once, even after a further change");
        let review = requests(&r)[1].last().unwrap().content.clone();
        assert!(review.contains("[review] Not accepted yet") && review.contains("<task>\nUpper-case the second line of n.txt.\n</task>"), "{review}");
        assert_eq!(r.agent.stats.surprises, 0, "a review is not a surprise");

        // Nothing changed in answer to the next message, so there is nothing to review.
        let out = r.agent.run("Is it done?".into(), &CancellationToken::new()).await;
        assert_eq!((out.status, r.agent.stats.turns), (FinishStatus::Done, 3));
    }

    fn events(r: &Rig) -> Vec<EventKind> {
        r.sink.events.lock().unwrap().clone()
    }

    fn requests(r: &Rig) -> Vec<Vec<Message>> {
        match &r.agent.provider {
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
        r.agent.provider = Provider::Mock(MockProvider::new(turns));

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
        cfg.max_turns = 2;
        let spin = || fixed(MockTurn::calls("", vec![("shell", json!({"cmd": "true"}))]));
        let mut r = rig("agent-budget", cfg, vec![spin(), spin(), spin()]);
        let out = r.agent.run("x".into(), &CancellationToken::new()).await;
        assert_eq!((out.status, r.agent.stats.turns), (FinishStatus::Budget, 2));
        assert!(events(&r).iter().any(|e| matches!(e, EventKind::Note { text, .. } if text.starts_with("Half of the budget"))));
    }

    #[tokio::test]
    async fn repeated_failures_draw_a_note_and_three_surprises_raise_the_effort() {
        let fail = || fixed(MockTurn::calls("", vec![("shell", json!({"cmd": "grep -q nope n.txt", "expect": {"exit": "0"}}))]));
        let mut r = rig("agent-stall", AgentConfig::new("mock"), vec![fail(), fail(), fail(), fixed(MockTurn::say("giving up"))]);
        assert_eq!(r.agent.effort(), Effort::High);
        r.agent.run("x".into(), &CancellationToken::new()).await;
        assert_eq!(r.agent.effort(), Effort::Max);
        let ev = events(&r);
        assert!(ev.iter().any(|e| matches!(e, EventKind::Effort { from, to, .. } if from == "high" && to == "max")));
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
            fixed(MockTurn::say("Started it and stopped it again.")),
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
        let files = format!("\n</env>\n\n<files>\nn.txt · 3 lines\n{a1}|alpha\n{a2}|beta\n{a3}|gamma\n</files>\nThis is `read` output for the file the task names. It is already read and can be edited directly: do not read it first.\n\n<task>\n{task}\n</task>");
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
        cfg.features = Features::presage().without("attach").unwrap();
        let mut r = rig("agent-no-attach", cfg, vec![read(), fix()]);
        let out = r.agent.run(task.into(), &CancellationToken::new()).await;
        assert_eq!((out.status, r.agent.stats.turns), (FinishStatus::Done, 2));
        let env = events(&r).iter().find_map(|e| if let EventKind::Env { text } = e { Some(text.clone()) } else { None }).unwrap();
        assert_eq!(requests(&r)[0][1].content, format!("{env}\n\n<task>\n{task}\n</task>"));
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
        cfg.features = Features::presage().without("attach").unwrap();
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
        cfg.features = Features::presage().without("short-paths").unwrap();
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
        assert_eq!(reqs[0][1].content, "fix", "no environment snapshot in the baseline");
    }

    #[test]
    fn effort_policy_parses() {
        assert_eq!(EffortPolicy::parse("ratchet"), Some(EffortPolicy::Ratchet(Effort::High)));
        assert_eq!(EffortPolicy::parse("ratchet-low"), Some(EffortPolicy::Ratchet(Effort::Low)));
        assert_eq!(EffortPolicy::parse("MAX"), Some(EffortPolicy::Fixed(Effort::Max)));
        assert_eq!(EffortPolicy::parse("fast"), None);
        assert_eq!(EffortPolicy::Ratchet(Effort::Low).label(), "ratchet-low");
    }
}
