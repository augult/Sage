//! The session goal (`/goal`): a condition the user states, and the check that decides at the end
//! of a run whether it holds.
//!
//! The session keeps working until an independent check agrees. The check is one request to the
//! session's own model with a fresh context: it is not shown the conversation, only the goal, what
//! the worker said, what the harness knows about verification and what changed. It cannot run
//! anything, so a claim with nothing behind it does not pass. Where it is asked and what happens
//! with its answer is in `agent.rs`; this module is what is asked and how the answer is read.

use crate::provider::{ChatRequest, Effort, Message, Provider, ProviderError, Usage};
use crate::util::truncate_chars;
use crate::watches::{WatchState, Watches};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio_util::sync::CancellationToken;

/// Checks a user message can cost. When the last one still says no, the run ends and the goal stays.
pub const MAX_CHECKS: u32 = 3;
/// Output cap of a check: a verdict and a few lines. Thinking cannot always be switched off, and
/// has to fit in this too.
pub const MAX_TOKENS: u32 = 1_200;

const MAX_GOAL_CHARS: usize = 2_000;
const MAX_SUMMARY_CHARS: usize = 4_000;
const MAX_REASON_CHARS: usize = 800;

/// A session's goal, as it is kept in its stored meta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Goal {
    pub text: String,
    /// A check found the condition to hold. A goal that is met no longer gates the messages after it.
    #[serde(default)]
    pub met: bool,
}

impl Goal {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.trim().to_string(),
            met: false,
        }
    }
}

/// What a message that starts with `/goal` asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// `/goal <condition>`: work until it holds.
    Set(String),
    /// `/goal`: say what the goal is. Nothing is sent to the model.
    Show,
    /// `/goal clear`: drop it. Nothing is sent to the model.
    Clear,
}

/// The command a message is, if it is one: `/goal` as a word of its own at the start.
pub fn parse_command(text: &str) -> Option<Command> {
    let rest = text.trim().strip_prefix("/goal")?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let argument = rest.trim();
    Some(match argument {
        "" => Command::Show,
        a if a.eq_ignore_ascii_case("clear") => Command::Clear,
        a => Command::Set(a.to_string()),
    })
}

/// What the model is sent for `/goal <condition>`: the goal, as a task.
pub fn task(condition: &str) -> String {
    format!("Work until this holds: {}", condition.trim())
}

/// What the model is told of a goal that comes with a task of its own.
pub fn briefing(condition: &str) -> String {
    format!(
        "The session goal, which an independent check judges when you finish: {}",
        condition.trim()
    )
}

/// What the model is sent when the check says the goal is not met.
pub fn work_on(reasons: &str) -> String {
    let reasons = reasons.trim();
    let why = if reasons.is_empty() {
        "the check gave no reasons".to_string()
    } else {
        reasons.to_string()
    };
    format!(
        "The session goal is not met yet: {why}\nCarry on until it holds, then call `finish` again."
    )
}

/// How the verdict was worded, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Met(String),
    NotMet(String),
}

/// Leading emphasis, quotes, bullets and brackets: what models put in front of a verdict.
fn bare(s: &str) -> &str {
    s.trim_start_matches(|c: char| c.is_whitespace() || "*_#>`\"'-[(\u{2022}".contains(c))
}

/// Reads an answer that starts with `MET` or `NOT MET`, tolerating what models put around it:
/// emphasis, quotes, a label, case, a colon or dash before the reasons. `None` when it starts with
/// neither.
pub fn parse_verdict(answer: &str) -> Option<Verdict> {
    let answer = answer.trim();
    let (first, after) = answer.split_once('\n').unwrap_or((answer, ""));
    let mut first = bare(first);
    for label in ["verdict:", "answer:"] {
        if first
            .get(..label.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(label))
        {
            first = bare(&first[label.len()..]);
        }
    }
    // ASCII case only, so that byte positions in `upper` are those of `first`.
    let upper = first.to_ascii_uppercase();
    let (len, met) = match ["NOT MET", "NOT_MET", "NOT-MET", "NOTMET"]
        .into_iter()
        .find(|word| upper.starts_with(word))
    {
        Some(word) => (word.len(), false),
        None if upper.starts_with("MET") => (3, true),
        None => return None,
    };
    let rest = &first[len..];
    // `METHOD` and `METRICS` are not a verdict.
    if rest.chars().next().is_some_and(char::is_alphanumeric) {
        return None;
    }
    let rest = rest
        .trim_start_matches(|c: char| {
            c.is_whitespace() || "*_`\"'])\u{2013}\u{2014}.:,;-".contains(c)
        })
        .trim_end();
    let reasons = match (rest.is_empty(), after.trim().is_empty()) {
        (_, true) => rest.to_string(),
        (true, false) => after.trim().to_string(),
        (false, false) => format!("{rest}\n{}", after.trim()),
    };
    let reasons = truncate_chars(&reasons, MAX_REASON_CHARS);
    Some(if met {
        Verdict::Met(reasons)
    } else {
        Verdict::NotMet(reasons)
    })
}

const SYSTEM: &str = "\
You are the independent check of a coding session. The user stated a goal: a condition that has to hold before the work counts as done. Decide from the evidence below whether it holds now.

You cannot run anything. Do not take the worker's own claim of success as proof: look at what was actually checked and what the changes actually show. A condition that only a check nobody ran could establish is not met.

Answer with `MET` or `NOT MET` alone on the first line. Then say why in at most five short lines. For NOT MET, name what is still missing or unproven, concretely, so that the worker can act on it.";

/// What the check is shown.
pub struct Material<'a> {
    pub goal: &'a str,
    /// What the worker said when it finished: its `finish` summary, or its prose answer.
    pub summary: &'a str,
    /// What the harness knows about whether the workspace was checked after its last change.
    pub evidence: &'a str,
    pub watches: &'a Watches,
    /// The report from `changes::changes`.
    pub changes: Option<&'a str>,
    /// The workspace's epoch, to tell a watch that has seen the latest change from one that has not.
    pub epoch: u64,
}

/// The state of the watches, one line each.
fn watch_lines(watches: &Watches, epoch: u64) -> String {
    if watches.items.is_empty() {
        return "none".to_string();
    }
    let lines: Vec<String> = watches
        .items
        .iter()
        .map(|w| {
            let state = match w.state {
                WatchState::Pass if w.epoch == epoch => "passing",
                WatchState::Pass => "passed before the last change",
                WatchState::Fail => "failing",
                WatchState::Unknown => "no result yet",
            };
            format!("- {}: `{}` {state}", w.name, w.cmd)
        })
        .collect();
    lines.join("\n")
}

/// The check's whole request: its instructions and what it is shown. Nothing of the session's own
/// conversation is in it.
pub fn messages(m: &Material) -> Vec<Message> {
    let changes = m.changes.unwrap_or("none recorded");
    let user = format!(
        "<goal>\n{}\n</goal>\n<worker_summary>\n{}\n</worker_summary>\n<verification>\n{}\nWatches:\n{}\n</verification>\n<changes>\n{changes}\n</changes>",
        truncate_chars(m.goal.trim(), MAX_GOAL_CHARS),
        truncate_chars(m.summary.trim(), MAX_SUMMARY_CHARS),
        m.evidence,
        watch_lines(m.watches, m.epoch),
    );
    vec![Message::system(SYSTEM), Message::user(user)]
}

/// What a check came back with.
pub struct Answer {
    /// `None` when the answer did not start with `MET` or `NOT MET`.
    pub verdict: Option<Verdict>,
    pub usage: Usage,
    pub retries: u32,
}

/// Makes the request: no tools, the least thinking the provider allows, a small output cap.
pub(crate) async fn ask(
    provider: &Provider,
    m: &Material<'_>,
    cancel: &CancellationToken,
    on_delta: crate::provider::DeltaSink<'_>,
    max_attempts: Option<u32>,
) -> Result<Answer, ProviderError> {
    let messages = messages(m);
    let tools = json!([]);
    let request = ChatRequest {
        messages: &messages,
        tools: &tools,
        effort: Effort::Off,
        max_tokens: MAX_TOKENS,
        reasoning_cap: 0,
    };
    let reply = provider
        .chat_with(request, on_delta, cancel, None, max_attempts)
        .await?;
    Ok(Answer {
        verdict: parse_verdict(&reply.content),
        usage: reply.usage,
        retries: reply.retries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_is_a_goal_command_when_it_starts_with_the_word() {
        assert_eq!(
            parse_command("/goal all tests pass"),
            Some(Command::Set("all tests pass".into()))
        );
        assert_eq!(
            parse_command("  /goal\n  the build is green and docs updated \n"),
            Some(Command::Set("the build is green and docs updated".into()))
        );
        assert_eq!(parse_command("/goal"), Some(Command::Show));
        assert_eq!(parse_command(" /goal  "), Some(Command::Show));
        assert_eq!(parse_command("/goal clear"), Some(Command::Clear));
        assert_eq!(parse_command("/GOAL"), None, "commands are lower case");
        assert_eq!(parse_command("/goals are good"), None);
        assert_eq!(parse_command("please /goal x"), None);
        assert_eq!(
            parse_command("/goal clear the cache and report"),
            Some(Command::Set("clear the cache and report".into()))
        );
        assert_eq!(task(" tests pass "), "Work until this holds: tests pass");
    }

    #[test]
    fn the_verdict_is_read_from_the_first_line_whatever_is_around_it() {
        let met = |reasons: &str| Some(Verdict::Met(reasons.to_string()));
        let not = |reasons: &str| Some(Verdict::NotMet(reasons.to_string()));
        assert_eq!(
            parse_verdict("MET\nThe suite passed after the last edit."),
            met("The suite passed after the last edit.")
        );
        assert_eq!(
            parse_verdict("NOT MET\n- no test was run\n- docs untouched"),
            not("- no test was run\n- docs untouched")
        );
        assert_eq!(
            parse_verdict("**NOT MET**\nnothing ran"),
            not("nothing ran")
        );
        assert_eq!(parse_verdict("  not met: nothing ran"), not("nothing ran"));
        assert_eq!(parse_verdict("Not met."), not(""));
        assert_eq!(
            parse_verdict("NOT_MET - the tests were not run"),
            not("the tests were not run")
        );
        assert_eq!(parse_verdict("`MET`"), met(""));
        assert_eq!(
            parse_verdict("Verdict: MET — all green\nmore"),
            met("all green\nmore")
        );
        assert_eq!(parse_verdict("## MET\n\nok"), met("ok"));
        assert_eq!(parse_verdict("met"), met(""));
        assert_eq!(parse_verdict("\n\nMET: yes"), met("yes"));
        assert_eq!(parse_verdict("METHOD: none"), None);
        assert_eq!(parse_verdict("The goal appears to be met."), None);
        assert_eq!(parse_verdict(""), None);
        assert_eq!(
            parse_verdict("(scripted model has nothing further to say)"),
            None
        );
        let long = format!("NOT MET\n{}", "x".repeat(2_000));
        assert!(
            matches!(parse_verdict(&long), Some(Verdict::NotMet(r)) if r.chars().count() < 900)
        );
    }

    #[test]
    fn the_check_is_shown_the_evidence_and_not_the_conversation() {
        let mut watches = Watches::default();
        watches.set("tests", "cargo test", std::path::PathBuf::from("."), None);
        let m = Material {
            goal: "all tests pass",
            summary: "Fixed it.",
            evidence: "verified by watch tests",
            watches: &watches,
            changes: Some(" M a.rs"),
            epoch: 0,
        };
        let sent = messages(&m);
        assert_eq!(sent.len(), 2);
        assert!(sent[0].content.contains("`MET` or `NOT MET`"));
        for part in [
            "<goal>\nall tests pass\n</goal>",
            "<worker_summary>\nFixed it.\n</worker_summary>",
            "verified by watch tests\nWatches:\n- tests: `cargo test` no result yet",
            "<changes>\n M a.rs\n</changes>",
        ] {
            assert!(
                sent[1].content.contains(part),
                "{part} is missing from {}",
                sent[1].content
            );
        }
        let bare = messages(&Material { changes: None, ..m });
        assert!(
            bare[1]
                .content
                .contains("<changes>\nnone recorded\n</changes>")
        );
        assert!(
            work_on("tests not run").starts_with("The session goal is not met yet: tests not run")
                && work_on(" ").contains("no reasons")
        );
    }
}
