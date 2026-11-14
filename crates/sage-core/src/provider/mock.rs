//! A scripted stand-in for a model. Used by tests and by the in-app demo so the harness mechanics can
//! be exercised without a key. Its token counts are fabricated from text lengths and are labelled as
//! simulated everywhere they surface.

use super::{
    ChatRequest, ChatResponse, Delta, DeltaSink, Effort, Message, ProviderError, ToolCall, Usage,
};
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
    /// The request fails with this error instead of being answered.
    pub error: Option<ProviderError>,
}

/// How a request was made, apart from its messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    pub effort: Effort,
    /// How many tools were offered.
    pub tools: usize,
    pub max_tokens: u32,
    /// Characters of reasoning the reply was allowed before its first word or call; 0 for the provider's own cap.
    pub reasoning_cap: usize,
}

impl MockTurn {
    pub fn calls(reasoning: &str, calls: Vec<(&str, Value)>) -> Self {
        Self {
            reasoning: reasoning.into(),
            content: String::new(),
            calls: calls.into_iter().map(|(n, a)| (n.to_string(), a)).collect(),
            finish: None,
            error: None,
        }
    }
    pub fn say(content: &str) -> Self {
        Self {
            reasoning: String::new(),
            content: content.into(),
            calls: vec![],
            finish: None,
            error: None,
        }
    }
    /// A request that fails.
    pub fn fail(error: ProviderError) -> Self {
        Self {
            error: Some(error),
            ..Self::default()
        }
    }
}

/// Produces the reply for one turn. It may inspect the conversation so far, e.g. to compute anchors
/// from files as they currently are.
pub type TurnFn = Box<dyn Fn(&[Message]) -> MockTurn + Send + Sync>;

/// Answers a request that is not a turn of the conversation (it runs beside it, so it has no
/// place in the order of `turns`), or leaves it to them.
pub type AsideFn = Box<dyn Fn(&[Message]) -> Option<MockTurn> + Send + Sync>;

pub struct MockProvider {
    turns: Vec<TurnFn>,
    aside: Option<AsideFn>,
    cursor: AtomicUsize,
    prev_prompt: AtomicU64,
    /// Milliseconds between streamed chunks; 0 in tests.
    pub chunk_delay_ms: u64,
    /// Requests as received, for assertions.
    pub requests: Mutex<Vec<Vec<Message>>>,
    /// How each of them was made.
    pub shapes: Mutex<Vec<Shape>>,
}

impl MockProvider {
    pub fn new(turns: Vec<TurnFn>) -> Self {
        Self {
            turns,
            aside: None,
            cursor: AtomicUsize::new(0),
            prev_prompt: AtomicU64::new(0),
            chunk_delay_ms: 0,
            requests: Mutex::new(Vec::new()),
            shapes: Mutex::new(Vec::new()),
        }
    }

    pub fn scripted(turns: Vec<MockTurn>) -> Self {
        Self::new(
            turns
                .into_iter()
                .map(|t| Box::new(move |_: &[Message]| t.clone()) as TurnFn)
                .collect(),
        )
    }

    pub fn has_aside(&self) -> bool {
        self.aside.is_some()
    }

    /// Requests `aside` answers take no turn of the script.
    pub fn with_aside(mut self, aside: AsideFn) -> Self {
        self.aside = Some(aside);
        self
    }

    pub async fn chat(
        &self,
        req: ChatRequest<'_>,
        on_delta: DeltaSink<'_>,
        cancel: &CancellationToken,
    ) -> Result<ChatResponse, ProviderError> {
        let beside = self.aside.as_ref().and_then(|answer| answer(req.messages));
        let idx = if beside.is_some() {
            usize::MAX
        } else {
            self.cursor.fetch_add(1, Ordering::SeqCst)
        };
        self.requests.lock().unwrap().push(req.messages.to_vec());
        self.shapes.lock().unwrap().push(Shape {
            effort: req.effort,
            tools: req.tools.as_array().map_or(0, Vec::len),
            max_tokens: req.max_tokens,
            reasoning_cap: req.reasoning_cap,
        });
        let turn = match (beside, self.turns.get(idx)) {
            (Some(turn), _) => turn,
            (None, Some(f)) => f(req.messages),
            (None, None) => MockTurn::say("(scripted model has nothing further to say)"),
        };
        if let Some(error) = turn.error {
            // What was thought before the request failed was streamed like any other thought.
            if !turn.reasoning.is_empty() {
                on_delta(Delta::Reasoning(turn.reasoning));
            }
            return Err(error);
        }

        for (text, is_reasoning) in [(&turn.reasoning, true), (&turn.content, false)] {
            let chars: Vec<char> = text.chars().collect();
            for piece in chars.chunks(14) {
                if cancel.is_cancelled() {
                    return Err(ProviderError::Cancelled);
                }
                let s: String = piece.iter().collect();
                on_delta(if is_reasoning {
                    Delta::Reasoning(s)
                } else {
                    Delta::Content(s)
                });
                if self.chunk_delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(self.chunk_delay_ms)).await;
                }
            }
        }

        let calls: Vec<ToolCall> = turn
            .calls
            .iter()
            .enumerate()
            .map(|(i, (name, args))| ToolCall {
                id: format!("mock_{idx}_{i}"),
                name: name.clone(),
                arguments: args.to_string(),
            })
            .collect();

        // Simulate a provider with a perfect prefix cache: everything sent last time is a hit now.
        let prompt = est_tokens(&serde_json::to_string(req.messages).unwrap_or_default())
            + est_tokens(&req.tools.to_string());
        // A request beside the conversation shares no prefix with it.
        let cached = if idx == usize::MAX {
            0
        } else {
            self.prev_prompt.swap(prompt, Ordering::SeqCst).min(prompt)
        };
        let reasoning = est_tokens(&turn.reasoning);
        let output = reasoning
            + est_tokens(&turn.content)
            + calls
                .iter()
                .map(|c| est_tokens(&c.arguments) + 8)
                .sum::<u64>();

        Ok(ChatResponse {
            content: turn.content,
            reasoning: turn.reasoning,
            finish: turn.finish.clone().unwrap_or_else(|| {
                if calls.is_empty() {
                    "stop".into()
                } else {
                    "tool_calls".into()
                }
            }),
            calls,
            usage: Usage {
                prompt,
                cached,
                output,
                reasoning,
            },
            usage_reported: true,
            retries: 0,
        })
    }
}
