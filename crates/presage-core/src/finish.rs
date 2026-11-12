//! The last word on `finish`.
//!
//! The evidence gate (`script.rs`) asks whether anything was checked. A `finish` it lets through can
//! still be refused here, by a check that looks at what the gate cannot: whether the work is what
//! was asked for. Such a check may put a question of its own to the session's model.
//!
//! The checks are a list, run in order; the first that refuses ends the round, and the model goes on
//! working. A check must never be able to trap a session: one that cannot come to a verdict lets the
//! `finish` through and says so. None of them runs on the last reply the budget allows, where
//! nothing could follow a refusal.

use crate::economy::peak_multiplier;
use crate::events::{EventKind, Sink, Stats};
use crate::goal;
use crate::provider::{ChatRequest, Delta, Message, Provider, ProviderError};
use crate::script::Finished;
use crate::state::Session;
use crate::util::now_ms;
use serde_json::{Value, json};
use std::sync::LazyLock;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

/// The checks a `finish` has to pass once the evidence gate has let it through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinishCheck {
    /// The goal the user set, if one is open (`goal.rs`).
    Goal,
}

/// Every check, in the order they run.
pub const FINISH_CHECKS: &[FinishCheck] = &[FinishCheck::Goal];

/// A `finish` that is not accepted after all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    /// Why, in a few words: what the step's row says.
    pub violation: String,
    /// What the model is told.
    pub text: String,
}

/// The session's model, for a question put to it outside the conversation.
#[derive(Clone, Copy)]
pub struct Judge<'a> {
    pub provider: &'a Provider,
    /// The model's name, which its price at this hour goes by.
    pub model: &'a str,
}

/// What a check has to work with: the session, and the means to report and to ask.
pub struct FinishCx<'a> {
    pub sess: &'a mut Session,
    pub sink: &'a dyn Sink,
    pub stats: &'a mut Stats,
    pub cancel: &'a CancellationToken,
    pub turn: u32,
    /// `None` where a script runs with no model behind it: a check that needs one is then not made.
    pub judge: Option<Judge<'a>>,
}

/// Room for a verdict, and for the thinking of a model that cannot be asked to do without.
const ANSWER_TOKENS: u32 = 4_000;

static NO_TOOLS: LazyLock<Value> = LazyLock::new(|| json!([]));

impl FinishCx<'_> {
    /// Puts one question to the session's model and returns its answer. A request of its own: a
    /// fresh context (`system` and `question`, none of the conversation), no tools, and the lowest
    /// effort the provider's path has (`Effort::Off` goes out as whatever switches thinking off
    /// there, or as the least the model takes). It is counted and priced like any other request.
    pub async fn ask(&mut self, system: &str, question: &str) -> Result<String, ProviderError> {
        let Some(judge) = self.judge else { return Err(ProviderError::Protocol("there is no model to ask".into())) };
        let messages = [Message::system(system), Message::user(question)];
        let request = ChatRequest { messages: &messages, tools: &NO_TOOLS, effort: judge.provider.lowest_effort(), max_tokens: ANSWER_TOKENS };
        let (sink, turn) = (self.sink, self.turn);
        // Nothing of the answer is streamed to the timeline: it is not a reply of the conversation.
        let mut on_delta = move |d: Delta| {
            if let Delta::Retry { attempt: 1, reason, .. } = d {
                sink.emit(EventKind::Note { turn, kind: "retry".into(), text: format!("A finish check's request failed and is being tried again: {reason}") });
            }
        };
        let asked = Instant::now();
        let reply = judge.provider.chat(request, &mut on_delta, self.cancel).await;
        self.stats.model_ms += asked.elapsed().as_millis() as u64;
        let resp = reply?;
        self.stats.turns += 1;
        self.stats.checks += 1;
        self.stats.retries += resp.retries;
        self.stats.prompt_tokens += resp.usage.prompt;
        self.stats.cached_tokens += resp.usage.cached;
        self.stats.output_tokens += resp.usage.output;
        self.stats.reasoning_tokens += resp.usage.reasoning;
        if let Some(prices) = self.sess.prices {
            self.stats.cost += prices.cost(&resp.usage) * peak_multiplier(judge.model, now_ms() / 1000);
            self.stats.priced = true;
        }
        Ok(resp.content)
    }
}

/// Runs `FINISH_CHECKS` over a `finish` the evidence gate has accepted. `None`: it stands, as
/// `done` now reads (a check may add to its evidence). `Some`: the first check to refuse it.
pub async fn finish_checks(cx: &mut FinishCx<'_>, done: &mut Finished) -> Option<Refusal> {
    for check in FINISH_CHECKS {
        let refusal = match check {
            FinishCheck::Goal => goal::check(cx, done).await,
        };
        if refusal.is_some() || cx.cancel.is_cancelled() {
            return refusal;
        }
    }
    None
}
