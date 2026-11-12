//! A scripted stand-in for a model. Used by tests and by the in-app demo so the harness mechanics can
//! be exercised without a key. Its token counts are fabricated from text lengths and are labelled as
//! simulated everywhere they surface.

use super::{ChatRequest, ChatResponse, Delta, DeltaSink, Message, ProviderError, ToolCall, Usage};
use crate::text::est_tokens;
use serde_json::Value;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Default)]
pub struct MockTurn {
    pub reasoning: String,
    pub content: String,
    pub calls: Vec<(String, Value)>,
    /// Overrides the finish reason, e.g. "length" to simulate a reply cut off by the output limit.
    pub finish: Option<String>,
}

impl MockTurn {
    pub fn calls(reasoning: &str, calls: Vec<(&str, Value)>) -> Self {
        Self { reasoning: reasoning.into(), content: String::new(), calls: calls.into_iter().map(|(n, a)| (n.to_string(), a)).collect(), finish: None }
    }
    pub fn say(content: &str) -> Self {
        Self { reasoning: String::new(), content: content.into(), calls: vec![], finish: None }
    }
}

/// Produces the reply for one turn. It may inspect the conversation so far, e.g. to compute anchors
/// from files as they currently are.
pub type TurnFn = Box<dyn Fn(&[Message]) -> MockTurn + Send + Sync>;

pub struct MockProvider {
    turns: Vec<TurnFn>,
    cursor: AtomicUsize,
    prev_prompt: AtomicU64,
    /// Milliseconds between streamed chunks; 0 in tests.
    pub chunk_delay_ms: u64,
    /// Requests as received, for assertions.
    pub requests: Mutex<Vec<Vec<Message>>>,
}

impl MockProvider {
    pub fn new(turns: Vec<TurnFn>) -> Self {
        Self { turns, cursor: AtomicUsize::new(0), prev_prompt: AtomicU64::new(0), chunk_delay_ms: 0, requests: Mutex::new(Vec::new()) }
    }

    pub fn scripted(turns: Vec<MockTurn>) -> Self {
        Self::new(turns.into_iter().map(|t| Box::new(move |_: &[Message]| t.clone()) as TurnFn).collect())
    }

    pub async fn chat(&self, req: ChatRequest<'_>, on_delta: DeltaSink<'_>, cancel: &CancellationToken) -> Result<ChatResponse, ProviderError> {
        let idx = self.cursor.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().unwrap().push(req.messages.to_vec());
        let turn = match self.turns.get(idx) {
            Some(f) => f(req.messages),
            None => MockTurn::say("(scripted model has nothing further to say)"),
        };

        for (text, is_reasoning) in [(&turn.reasoning, true), (&turn.content, false)] {
            let chars: Vec<char> = text.chars().collect();
            for piece in chars.chunks(14) {
                if cancel.is_cancelled() {
                    return Err(ProviderError::Cancelled);
                }
                let s: String = piece.iter().collect();
                on_delta(if is_reasoning { Delta::Reasoning(s) } else { Delta::Content(s) });
                if self.chunk_delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(self.chunk_delay_ms)).await;
                }
            }
        }

        let calls: Vec<ToolCall> = turn
            .calls
            .iter()
            .enumerate()
            .map(|(i, (name, args))| ToolCall { id: format!("mock_{idx}_{i}"), name: name.clone(), arguments: args.to_string() })
            .collect();

        // Simulate a provider with a perfect prefix cache: everything sent last time is a hit now.
        let prompt = est_tokens(&serde_json::to_string(req.messages).unwrap_or_default()) + est_tokens(&req.tools.to_string());
        // A request with no tools is a question on the side (a finish check), not a turn of the
        // conversation: it hits nothing and leaves what the conversation has cached as it is.
        let side = req.tools.as_array().is_none_or(|tools| tools.is_empty());
        let cached = if side { 0 } else { self.prev_prompt.swap(prompt, Ordering::SeqCst).min(prompt) };
        let reasoning = est_tokens(&turn.reasoning);
        let output = reasoning + est_tokens(&turn.content) + calls.iter().map(|c| est_tokens(&c.arguments) + 8).sum::<u64>();

        Ok(ChatResponse {
            content: turn.content,
            reasoning: turn.reasoning,
            finish: turn.finish.clone().unwrap_or_else(|| if calls.is_empty() { "stop".into() } else { "tool_calls".into() }),
            calls,
            usage: Usage { prompt, cached, output, reasoning },
            usage_reported: true,
            retries: 0,
        })
    }
}
