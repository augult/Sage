//! Model providers. Everything the agent loop needs from an LLM goes through `Provider::chat`.

pub mod mock;
pub mod openai;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Exactly the string the model produced. It is echoed back byte-for-byte on later turns, because
    /// re-serialising it would change the prompt prefix and forfeit the provider's cache.
    pub arguments: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    /// Reasoning text, replayed verbatim: DeepSeek rejects tool loops without it, GLM caches better
    /// with it, and re-reading it from cache is ~200x cheaper than having the model think it again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn system(text: impl Into<String>) -> Self {
        Self { role: Role::System, content: text.into(), reasoning: None, tool_calls: vec![], tool_call_id: None }
    }
    pub fn user(text: impl Into<String>) -> Self {
        Self { role: Role::User, content: text.into(), reasoning: None, tool_calls: vec![], tool_call_id: None }
    }
    pub fn tool(call_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self { role: Role::Tool, content: text.into(), reasoning: None, tool_calls: vec![], tool_call_id: Some(call_id.into()) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Off,
    Low,
    High,
    Max,
}

impl Effort {
    pub fn as_str(&self) -> &'static str {
        match self {
            Effort::Off => "off",
            Effort::Low => "low",
            Effort::High => "high",
            Effort::Max => "max",
        }
    }

    pub fn parse(s: &str) -> Option<Effort> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "disabled" => Some(Effort::Off),
            "low" | "minimal" => Some(Effort::Low),
            "high" | "medium" | "xhigh" => Some(Effort::High),
            "max" | "ultra" => Some(Effort::Max),
            _ => None,
        }
    }

    pub fn next(self) -> Option<Effort> {
        match self {
            Effort::Off => Some(Effort::Low),
            Effort::Low => Some(Effort::High),
            Effort::High => Some(Effort::Max),
            Effort::Max => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// All input tokens, cached or not.
    pub prompt: u64,
    /// The part of `prompt` served from the provider's cache.
    pub cached: u64,
    /// All output tokens, reasoning included.
    pub output: u64,
    /// The part of `output` spent on reasoning.
    pub reasoning: u64,
}

impl Usage {
    pub fn add(&mut self, other: &Usage) {
        self.prompt += other.prompt;
        self.cached += other.cached;
        self.output += other.output;
        self.reasoning += other.reasoning;
    }
}

pub struct ChatRequest<'a> {
    pub messages: &'a [Message],
    /// Prebuilt, constant `tools` array.
    pub tools: &'a Value,
    pub effort: Effort,
    pub max_tokens: u32,
}

#[derive(Clone, Debug, Default)]
pub struct ChatResponse {
    pub content: String,
    pub reasoning: String,
    pub calls: Vec<ToolCall>,
    pub usage: Usage,
    /// False when the provider sent no usage block and the numbers are estimates.
    pub usage_reported: bool,
    pub finish: String,
    pub retries: u32,
}

#[derive(Clone, Debug)]
pub enum Delta {
    Reasoning(String),
    Content(String),
    /// The attempt failed and will be retried; anything streamed so far is void.
    Retry { attempt: u32, wait_ms: u64, reason: String },
}

#[derive(Clone, Debug)]
pub enum ProviderError {
    Cancelled,
    Http { status: u16, message: String, retry_after_secs: Option<u64> },
    Network(String),
    Protocol(String),
    /// The model got stuck repeating itself; the stream was cut to stop paying for it.
    Degenerate,
}

impl ProviderError {
    pub fn retryable(&self) -> bool {
        match self {
            ProviderError::Http { status, .. } => matches!(status, 408 | 409 | 425 | 429 | 500..=599),
            ProviderError::Network(_) | ProviderError::Protocol(_) => true,
            ProviderError::Cancelled | ProviderError::Degenerate => false,
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderError::Cancelled => write!(f, "cancelled"),
            ProviderError::Http { status, message, .. } => write!(f, "provider returned HTTP {status}: {message}"),
            ProviderError::Network(m) => write!(f, "network error: {m}"),
            ProviderError::Protocol(m) => write!(f, "unexpected provider response: {m}"),
            ProviderError::Degenerate => write!(f, "model output degenerated into repetition"),
        }
    }
}

impl std::error::Error for ProviderError {}

pub type DeltaSink<'a> = &'a mut (dyn FnMut(Delta) + Send);

pub enum Provider {
    OpenAi(openai::OpenAiProvider),
    Mock(mock::MockProvider),
}

impl Provider {
    pub async fn chat(&self, req: ChatRequest<'_>, on_delta: DeltaSink<'_>, cancel: &CancellationToken) -> Result<ChatResponse, ProviderError> {
        match self {
            Provider::OpenAi(p) => p.chat(req, on_delta, cancel).await,
            Provider::Mock(p) => p.chat(req, on_delta, cancel).await,
        }
    }

    /// True for the scripted provider: its numbers are fabricated and must be labelled as such.
    pub fn simulated(&self) -> bool {
        matches!(self, Provider::Mock(_))
    }

    /// The serialised `messages` array as it goes on the wire, for the prefix-stability audit.
    pub fn wire_messages(&self, messages: &[Message]) -> String {
        match self {
            Provider::OpenAi(p) => p.wire_messages(messages).to_string(),
            Provider::Mock(_) => serde_json::to_string(messages).unwrap_or_default(),
        }
    }
}

/// True when the tail of `text` is one short unit repeated at least four times. Cheap models do
/// occasionally fall into such loops while thinking, and every looped token is billed as output.
pub fn is_looping(text: &str) -> bool {
    const WINDOW: usize = 1200;
    let bytes = text.as_bytes();
    if bytes.len() < WINDOW {
        return false;
    }
    let w = &bytes[bytes.len() - WINDOW..];
    (8..=WINDOW / 4).any(|period| {
        (period..WINDOW).all(|i| w[i] == w[i - period]) && {
            let mut distinct = [false; 256];
            w[..period].iter().for_each(|b| distinct[*b as usize] = true);
            distinct.iter().filter(|d| **d).count() >= 4
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effort_parses_vendor_aliases() {
        assert_eq!(Effort::parse("medium"), Some(Effort::High));
        assert_eq!(Effort::parse("none"), Some(Effort::Off));
        assert_eq!(Effort::parse("ULTRA"), Some(Effort::Max));
        assert_eq!(Effort::parse("turbo"), None);
        assert_eq!(Effort::Low.next(), Some(Effort::High));
        assert_eq!(Effort::Max.next(), None);
        assert!(Effort::Low < Effort::Max);
    }

    #[test]
    fn detects_repetition_loops_but_not_ordinary_text() {
        let looped = format!("Let me think about this. {}", "I should check the file again. ".repeat(60));
        assert!(is_looping(&looped));

        let prose: String = (0..300).map(|i| format!("Step {i}: consider case {} of the parser. ", i * 7 % 13)).collect();
        assert!(!is_looping(&prose));

        assert!(!is_looping(&"-".repeat(3000)), "a rule of one repeated character is not a thinking loop");
        assert!(!is_looping("short"));
    }

    #[test]
    fn message_serialisation_omits_empty_optionals() {
        let m = Message::user("hi");
        assert_eq!(serde_json::to_string(&m).unwrap(), r#"{"role":"user","content":"hi"}"#);
    }
}
