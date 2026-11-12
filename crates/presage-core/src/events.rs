//! The event stream: the single source of truth for the UI, the CLI and the on-disk session log.

use crate::provider::Usage;
use crate::tools::StepStatus;
use crate::watches::{WatchChange, WatchState};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    /// Unix milliseconds.
    pub ts: u64,
    #[serde(flatten)]
    pub kind: EventKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Reasoning,
    Content,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FinishStatus {
    /// `finish` was accepted.
    Done,
    /// The model replied in prose and handed the turn back to the user.
    Yielded,
    /// The user stopped the run.
    Stopped,
    /// A turn or time budget ran out.
    Budget,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheVerdict {
    /// First request of the session: nothing could have been cached.
    First,
    /// The provider served (nearly) the whole previous prompt from cache.
    Hit,
    Partial,
    /// The prefix was byte-identical but the provider reported no cache hit. Not a harness fault.
    Miss,
    /// The harness itself changed what was already sent: old observations were trimmed, or a tool
    /// was added to the list.
    Bust,
    /// The provider reported no usage.
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallInfo {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// What a `Goal` event records.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalState {
    /// A goal was set: with the start of the session, or by `/goal <condition>`.
    Set,
    /// It was removed by `/goal clear`.
    Cleared,
    /// It was asked about with `/goal`.
    Status,
    /// The check at a `finish` found it met.
    Met,
    /// The check found it unmet and the `finish` was refused; `reason` says what is missing.
    NotMet,
    /// The check could not be made or read and the `finish` was accepted without it; `reason` says why.
    Unchecked,
    /// The `finish` was accepted with the goal still open, after refusals that changed nothing.
    Open,
}

/// A field a stored log lacks (or has, from a mechanism since retired) does not keep it from loading.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stats {
    /// Model requests answered: the replies of the conversation, and the requests that finish
    /// checks make on the side.
    pub turns: u32,
    /// How many of `turns` were such requests on the side.
    pub checks: u32,
    pub steps: u32,
    pub skipped: u32,
    pub surprises: u32,
    /// Steps where the model made an explicit prediction, and how many of those held.
    pub predictions: u32,
    pub predictions_held: u32,
    pub prompt_tokens: u64,
    pub cached_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    /// Size of the most recent prompt: how much context the session is carrying.
    pub context_tokens: u64,
    /// USD at list prices; meaningful only when `priced`.
    pub cost: f64,
    pub priced: bool,
    /// Tool output that exists on disk but was never put in front of the model.
    pub hidden_bytes: u64,
    pub repairs: u32,
    pub retries: u32,
    pub cache_misses: u32,
    pub cache_busts: u32,
    pub elapsed_ms: u64,
    pub model_ms: u64,
    pub tool_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventKind {
    SessionStart {
        id: String,
        title: String,
        cwd: String,
        provider: String,
        model: String,
        effort: String,
        /// `presage` or `classic` (the conventional baseline loop).
        mode: String,
        autonomy: String,
        /// True when a scripted stand-in is playing the model; all numbers are then fabricated.
        simulated: bool,
        /// Skills the session starts with, by name.
        #[serde(default)]
        skills: Vec<String>,
    },
    User {
        text: String,
        /// Skills switched on with this message.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        skills: Vec<String>,
        /// Files attached to this message, by path.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<String>,
    },
    /// What changed about the session with the message before this event, as `SessionStart`
    /// names it. Only what changed is there.
    Settings {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        autonomy: Option<String>,
    },
    /// Environment snapshot that was prepended to the first user message.
    Env {
        text: String,
    },
    TurnStart {
        turn: u32,
        effort: String,
    },
    /// Streaming text. Transient: broadcast live, never stored.
    Delta {
        turn: u32,
        channel: Channel,
        text: String,
    },
    /// The provider call failed and is being retried; streamed text for this turn is void.
    Retry {
        turn: u32,
        attempt: u32,
        wait_ms: u64,
        reason: String,
    },
    Model {
        turn: u32,
        content: String,
        reasoning: String,
        calls: Vec<CallInfo>,
        usage: Usage,
        cost: Option<f64>,
        finish: String,
        ms: u64,
        repairs: u32,
    },
    Cache {
        turn: u32,
        prompt: u64,
        cached: u64,
        /// Tokens that should have been served from cache: the whole previous prompt.
        expected: u64,
        prefix_stable: bool,
        verdict: CacheVerdict,
    },
    StepStart {
        turn: u32,
        idx: u32,
        call_id: String,
        tool: String,
        summary: String,
        /// The model's prediction, rendered, if it made one.
        expect: Option<String>,
    },
    StepEnd {
        turn: u32,
        idx: u32,
        call_id: String,
        tool: String,
        status: StepStatus,
        /// Exactly the text the model received for this step.
        observation: String,
        ms: u64,
        hidden_bytes: u64,
        violations: Vec<String>,
        explicit: bool,
        /// What an `edit` or a `write` did to its file, for a before/after view. The model is
        /// not shown this.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        change: Option<crate::diff::FileChange>,
    },
    Watch {
        turn: u32,
        name: String,
        cmd: String,
        state: WatchState,
        change: WatchChange,
        detail: String,
    },
    /// Something the harness told the model (nudges, budget warnings).
    Note {
        turn: u32,
        kind: String,
        text: String,
    },
    /// The session's goal: set, asked about, cleared, or checked at a `finish`.
    Goal {
        /// The completion condition. Empty where there is none to report.
        goal: String,
        state: GoalState,
        /// Whether the goal is met as of this event.
        met: bool,
        /// What is missing, or why the goal was not checked. Empty otherwise.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        reason: String,
    },
    Approval {
        id: u64,
        turn: u32,
        tool: String,
        summary: String,
        reason: String,
    },
    ApprovalResolved {
        id: u64,
        allow: bool,
    },
    Effort {
        turn: u32,
        from: String,
        to: String,
        reason: String,
    },
    Decay {
        turn: u32,
        messages: u32,
        dropped_tokens: u64,
        reason: String,
    },
    Stats {
        stats: Stats,
    },
    Finish {
        status: FinishStatus,
        summary: String,
        verified: bool,
        evidence: String,
    },
    Error {
        message: String,
    },
}

impl EventKind {
    pub fn transient(&self) -> bool {
        matches!(self, EventKind::Delta { .. })
    }
}

/// Where events go. Implemented by the session store; tests collect into a vector.
pub trait Sink: Send + Sync {
    fn emit(&self, kind: EventKind);
}

#[derive(Default)]
#[cfg(test)]
pub struct MemorySink {
    pub events: std::sync::Mutex<Vec<EventKind>>,
}

#[cfg(test)]
impl Sink for MemorySink {
    fn emit(&self, kind: EventKind) {
        self.events.lock().unwrap().push(kind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_serialise_flat_with_a_type_tag() {
        let e = Event { seq: 7, ts: 1, kind: EventKind::StepEnd { turn: 2, idx: 0, call_id: "c".into(), tool: "shell".into(), status: StepStatus::Surprise, observation: "x".into(), ms: 5, hidden_bytes: 0, violations: vec![], explicit: true, change: None } };
        let v: serde_json::Value = serde_json::to_value(&e).unwrap();
        assert_eq!(v["type"], "step_end");
        assert_eq!(v["seq"], 7);
        assert_eq!(v["status"], "surprise");
        let back: Event = serde_json::from_value(v).unwrap();
        assert!(matches!(back.kind, EventKind::StepEnd { turn: 2, .. }));

        let s = serde_json::to_value(EventKind::Stats { stats: Stats::default() }).unwrap();
        assert_eq!(s["type"], "stats");
        assert_eq!(s["stats"]["turns"], 0);
        assert!(EventKind::Delta { turn: 1, channel: Channel::Reasoning, text: "t".into() }.transient());

        let g = serde_json::to_value(EventKind::Goal { goal: "all tests pass".into(), state: GoalState::NotMet, met: false, reason: "two fail".into() }).unwrap();
        assert_eq!(g, serde_json::json!({"type": "goal", "goal": "all tests pass", "state": "not_met", "met": false, "reason": "two fail"}));
    }

    #[test]
    fn what_a_retired_mechanism_wrote_is_not_an_event_and_its_numbers_are_passed_over() {
        // The memory feature's event has no type any more: whoever reads a log skips the line.
        assert!(serde_json::from_str::<Event>(r#"{"seq":4,"ts":1,"type":"lesson","learned":true,"text":"`pytest` fails here"}"#).is_err());
        assert!(serde_json::from_str::<Event>(r#"{"seq":5,"ts":1,"type":"review","text":"one review pass"}"#).is_err());
        // Its counter in stored statistics is ignored, and a counter the log predates is zero.
        let old: Event = serde_json::from_str(r#"{"seq":6,"ts":1,"type":"stats","stats":{"turns":3,"lessons_learned":2,"cost":0.5}}"#).unwrap();
        assert!(matches!(old.kind, EventKind::Stats { stats } if stats.turns == 3 && stats.cost == 0.5 && stats.steps == 0));
    }
}
