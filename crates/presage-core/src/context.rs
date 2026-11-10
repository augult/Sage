//! The conversation as sent to the provider, and the audit that keeps it cache-friendly.
//!
//! History is append-only. Nothing already sent is ever edited in place, because the provider's cache
//! is keyed on the exact prefix and a cache hit is 5–50x cheaper than a miss. The one exception is
//! `decay`, a deliberate, measured rewrite used when carrying old observations costs more than
//! re-buying the suffix (see `economy::Prices::decay_pays_off`).

use crate::events::CacheVerdict;
use crate::provider::{Message, Role, Usage};

pub struct Context {
    pub messages: Vec<Message>,
    /// For tool results: the one-line form they shrink to if decayed.
    stubs: Vec<Option<String>>,
    prev_wire: Option<String>,
    prev_prompt: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decayed {
    pub messages: u32,
    pub dropped_bytes: u64,
}

impl Context {
    pub fn new(system: &str) -> Self {
        Self { messages: vec![Message::system(system)], stubs: vec![None], prev_wire: None, prev_prompt: 0 }
    }

    pub fn push(&mut self, message: Message) {
        self.messages.push(message);
        self.stubs.push(None);
    }

    pub fn push_tool(&mut self, call_id: &str, content: String, stub: String) {
        self.messages.push(Message::tool(call_id, content));
        self.stubs.push(Some(stub));
    }

    /// Appends text to the most recent message (used for harness remarks on the last observation).
    pub fn append_to_last(&mut self, text: &str) {
        if let Some(last) = self.messages.last_mut() {
            last.content.push_str(text);
        }
    }

    /// Call with the serialised messages right before each request. Returns whether everything sent
    /// last time is, byte for byte, a prefix of what is being sent now.
    pub fn check_prefix(&mut self, wire: String) -> bool {
        let stable = match &self.prev_wire {
            // Drop the closing `]` of the previous array before comparing.
            Some(prev) => wire.len() >= prev.len() - 1 && wire.as_bytes()[..prev.len() - 1] == prev.as_bytes()[..prev.len() - 1],
            None => true,
        };
        self.prev_wire = Some(wire);
        stable
    }

    /// Classifies how the provider's cache behaved for the request just answered.
    pub fn audit(&mut self, usage: &Usage, usage_reported: bool, prefix_stable: bool) -> (u64, CacheVerdict) {
        let expected = self.prev_prompt;
        self.prev_prompt = usage.prompt;
        let verdict = if !usage_reported {
            CacheVerdict::Unknown
        } else if expected == 0 {
            CacheVerdict::First
        } else if !prefix_stable {
            CacheVerdict::Bust
        } else if usage.cached * 10 >= expected * 9 {
            CacheVerdict::Hit
        } else if usage.cached * 10 >= expected * 3 {
            CacheVerdict::Partial
        } else {
            CacheVerdict::Miss
        };
        (expected, verdict)
    }

    /// Bytes that decaying everything but the last `keep_recent` messages would drop, and the bytes
    /// from the first affected message to the end (what would have to be re-bought at the miss price).
    pub fn decay_estimate(&self, keep_recent: usize) -> (u64, u64) {
        let limit = self.messages.len().saturating_sub(keep_recent);
        let mut droppable = 0u64;
        let mut first: Option<usize> = None;
        for i in 0..limit {
            if let (Role::Tool, Some(stub)) = (self.messages[i].role, &self.stubs[i]) {
                let len = self.messages[i].content.len();
                if len > stub.len() + 200 {
                    droppable += (len - stub.len()) as u64;
                    first.get_or_insert(i);
                }
            }
        }
        let suffix = first.map(|f| self.messages[f..].iter().map(|m| (m.content.len() + m.reasoning.as_ref().map_or(0, String::len)) as u64).sum()).unwrap_or(0);
        (droppable, suffix)
    }

    /// Shrinks old observations to their stubs. What the model did and thought stays intact; only the
    /// bulky results of long-past steps go.
    pub fn decay(&mut self, keep_recent: usize) -> Decayed {
        let limit = self.messages.len().saturating_sub(keep_recent);
        let mut out = Decayed { messages: 0, dropped_bytes: 0 };
        for i in 0..limit {
            if self.messages[i].role != Role::Tool {
                continue;
            }
            let Some(stub) = self.stubs[i].take() else { continue };
            let len = self.messages[i].content.len();
            if len > stub.len() + 200 {
                out.messages += 1;
                out.dropped_bytes += (len - stub.len()) as u64;
                self.messages[i].content = stub;
            } else {
                self.stubs[i] = Some(stub);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(c: &Context) -> String {
        serde_json::to_string(&c.messages).unwrap()
    }

    #[test]
    fn appending_keeps_the_prefix_and_rewriting_breaks_it() {
        let mut c = Context::new("sys");
        c.push(Message::user("task"));
        assert!(c.check_prefix(wire(&c)));
        c.push_tool("a", "x".repeat(1000), "[stub a]".into());
        c.push_tool("b", "recent".into(), "[stub b]".into());
        assert!(c.check_prefix(wire(&c)), "growth alone never breaks the prefix");

        let d = c.decay(1);
        assert_eq!(d, Decayed { messages: 1, dropped_bytes: 992 });
        assert_eq!(c.messages[2].content, "[stub a]");
        assert_eq!(c.messages[3].content, "recent", "recent results are kept in full");
        assert!(!c.check_prefix(wire(&c)), "a decay is a deliberate cache bust");
        assert!(c.check_prefix(wire(&c)), "and stable again afterwards");
        assert_eq!(c.decay(0).messages, 0, "nothing left worth shrinking");
    }

    #[test]
    fn audit_separates_harness_busts_from_provider_misses() {
        let mut c = Context::new("sys");
        let u = |prompt, cached| Usage { prompt, cached, output: 10, reasoning: 0 };
        assert_eq!(c.audit(&u(1000, 0), true, true), (0, CacheVerdict::First));
        assert_eq!(c.audit(&u(1500, 980), true, true), (1000, CacheVerdict::Hit));
        assert_eq!(c.audit(&u(2000, 700), true, true), (1500, CacheVerdict::Partial));
        assert_eq!(c.audit(&u(2500, 0), true, true), (2000, CacheVerdict::Miss));
        assert_eq!(c.audit(&u(2600, 0), true, false), (2500, CacheVerdict::Bust));
        assert_eq!(c.audit(&u(2700, 0), false, true).1, CacheVerdict::Unknown);
    }

    #[test]
    fn decay_estimate_counts_what_would_go_and_what_must_be_rebought() {
        let mut c = Context::new("sys");
        c.push(Message::user("task"));
        c.push_tool("a", "x".repeat(4000), "[a]".into());
        c.push_tool("b", "y".repeat(100), "[b]".into());
        c.push_tool("c", "z".repeat(3000), "[c]".into());
        let (droppable, suffix) = c.decay_estimate(1);
        assert_eq!(droppable, 3997, "only the big old result; the small one is not worth it and the last is kept");
        assert_eq!(suffix, 4000 + 100 + 3000);
        c.append_to_last("\n--- harness ---\nnote");
        assert!(c.messages.last().unwrap().content.ends_with("note"));
    }
}
