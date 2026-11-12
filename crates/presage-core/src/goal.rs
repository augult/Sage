//! The session's goal.
//!
//! The user states a condition (`/goal <condition>`, or `--goal` at the start) and the session
//! keeps working until a check agrees that it holds. The check is one request to the session's
//! model outside its conversation, so the reply that says "done" is not the one asked whether it
//! is. It is made at every `finish` the evidence gate lets through (`finish.rs`); a `finish` that
//! falls short of the goal is refused with what is missing.
//!
//! A goal can ask for more than a session gets done, so the check must not be able to hold one
//! for ever: an answer that does not come or cannot be read lets the `finish` through, and so does
//! a model that has come back three times with nothing new. The goal then stays open.

use crate::changes;
use crate::events::{EventKind, GoalState};
use crate::finish::{FinishCx, Refusal};
use crate::provider::ProviderError;
use crate::script::Finished;
use crate::state::Session;
use crate::util::{one_line, truncate_chars};
use crate::watches::WatchState;
use serde::{Deserialize, Serialize};

/// A goal longer than this is refused when it is set.
pub const MAX_GOAL_CHARS: usize = 4_000;
/// How often in a row a refused `finish` may come back with nothing done before it is let through.
const MAX_IDLE: u8 = 3;
const MAX_SUMMARY_CHARS: usize = 4_000;
const MAX_REASON_CHARS: usize = 600;

/// A completion condition, and whether a check has found it met. A goal that is met no longer
/// stands in the way of `finish`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Goal {
    pub text: String,
    #[serde(default)]
    pub met: bool,
    /// Where the session stood when `finish` was last refused for this goal: the workspace epoch
    /// and the number of checks run. A `finish` from the same place has done nothing about it.
    #[serde(skip)]
    refused_at: Option<(u64, u64)>,
    /// How many such `finish` calls have come in a row.
    #[serde(skip)]
    idle: u8,
}

impl Goal {
    pub fn new(text: &str) -> Self {
        Self { text: text.trim().to_string(), ..Self::default() }
    }
}

/// What a message that starts with `/goal` asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// `/goal <condition>`: set the goal and work towards it.
    Set(String),
    /// `/goal`: say what the goal is.
    Show,
    /// `/goal clear`: remove it.
    Clear,
}

/// Reads a message as a `/goal` command. `None` for any other text, `/goalpost` included.
pub fn parse(text: &str) -> Option<Command> {
    let rest = text.trim_start().strip_prefix("/goal")?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let rest = rest.trim();
    Some(match rest {
        "" => Command::Show,
        _ if rest.eq_ignore_ascii_case("clear") => Command::Clear,
        _ => Command::Set(rest.to_string()),
    })
}

/// `goal` as it may be set, or why it may not.
pub fn checked(goal: &str) -> Result<String, String> {
    let goal = goal.trim();
    match goal.chars().count() {
        0 => Err("the goal is empty".into()),
        n if n > MAX_GOAL_CHARS => Err(format!("the goal is {n} characters long; at most {MAX_GOAL_CHARS} are taken")),
        _ => Ok(goal.to_string()),
    }
}

/// The first message of a session and the goal it starts with, from the task as given and the
/// goal stated beside it. A task that is `/goal <condition>` sets the goal, and a goal given with
/// no task is the task, in that same form.
pub fn at_start(task: &str, goal: Option<&str>) -> Result<(String, Option<String>), String> {
    let beside = goal.map(str::trim).filter(|g| !g.is_empty());
    match (parse(task), beside) {
        (Some(Command::Set(_)), Some(_)) => Err("the goal is given twice: in the task as `/goal`, and beside it".into()),
        (Some(Command::Set(goal)), None) => Ok((task.trim().to_string(), Some(checked(&goal)?))),
        (Some(Command::Show | Command::Clear), _) => Err("a new session has no goal yet: state one with `/goal <condition>`".into()),
        (None, Some(goal)) if task.trim().is_empty() => Ok((format!("/goal {goal}"), Some(checked(goal)?))),
        (None, Some(goal)) => Ok((task.to_string(), Some(checked(goal)?))),
        (None, None) => Ok((task.to_string(), None)),
    }
}

/// What the model is sent with the message that sets a goal: the goal worded as a task where the
/// message was the `/goal` command, and the message with the goal after it otherwise.
pub fn brief(message: &str, goal: &str) -> String {
    const HOW: &str = "`finish` is accepted once a check of the harness's own agrees that the goal is met. Until then it comes back with what is missing.";
    match parse(message) {
        Some(_) => format!("Work until this goal is met:\n<goal>\n{goal}\n</goal>\n{HOW}"),
        None => format!("{message}\n\nThe user has set a goal for this session as well:\n<goal>\n{goal}\n</goal>\n{HOW}"),
    }
}

/// What the check came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Met,
    /// With what is missing.
    NotMet(String),
}

const JUDGE: &str = "You decide whether a goal that a user set for a coding agent is met. You are given the goal, the agent's closing message, the checks run on the workspace since it last changed with how each came out, and what the session changed. Go by the checks and the changes: a claim in the closing message that neither bears out is not enough. Only where the goal asks for an answer rather than a change is the closing message itself what to judge.
Reply with one line and nothing else: `MET`, or `NOT MET: <what is still missing or unproven, in a sentence or two>`.";

/// What the checker is asked: the goal, the model's `finish` message, the checks run since the
/// workspace last changed, and what the session changed.
fn question(goal: &str, summary: &str, checks: &str, changes: Option<&str>) -> String {
    let summary = if summary.trim().is_empty() { "(none given)".to_string() } else { truncate_chars(summary.trim(), MAX_SUMMARY_CHARS) };
    let changes = changes.unwrap_or("The session has changed nothing in the workspace.");
    format!("<goal>\n{goal}\n</goal>\n<closing_message>\n{summary}\n</closing_message>\n<checks>\n{checks}\n</checks>\n<changes>\n{changes}\n</changes>\nIs the goal met? Reply `MET` or `NOT MET: <what is missing>`.")
}

/// The checks that have seen the workspace as it now is, one to a line, and what the evidence gate
/// made of them.
fn checks_text(sess: &Session, done: &Finished) -> String {
    let ev = &sess.evidence;
    let mut lines: Vec<String> = ev.checks.iter().map(|c| format!("- {}: {}", c.what, c.outcome)).collect();
    for w in sess.watches.items.iter().filter(|w| w.epoch == ev.epoch && w.state != WatchState::Unknown) {
        let state = if w.state == WatchState::Pass { "passing".to_string() } else { format!("failing ({})", w.summary) };
        lines.push(format!("- watch {} (`{}`, re-run by the harness): {state}", w.name, one_line(&w.cmd, 160)));
    }
    let since = if ev.epoch == 0 { "the session began (it has changed nothing)".to_string() } else { format!("the last change to the workspace ({})", ev.last_change) };
    let head = if lines.is_empty() { format!("No check has been run since {since}.") } else { format!("Run since {since}:\n{}", lines.join("\n")) };
    format!("{head}\nThe harness's own gate on `finish`: {}.", done.evidence)
}

/// Reads the checker's answer: `MET`, or `NOT MET: <what is missing>`, on its first line, or on
/// its last where it put something in front. `None` for an answer that is neither.
pub fn verdict(answer: &str) -> Option<Verdict> {
    let lines: Vec<&str> = answer.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let read = |at: usize| -> Option<Verdict> {
        let line = undress(lines.get(at)?);
        let upper = line.to_ascii_uppercase();
        // `METHOD` is not `MET`: the word has to end there.
        let after = |word: &str| upper.strip_prefix(word).is_some_and(|rest| !rest.starts_with(char::is_alphanumeric)).then(|| line[word.len()..].to_string());
        if let Some(rest) = after("NOT MET") {
            let mut reason = rest.trim_start_matches(|c: char| matches!(c, ':' | '-' | '–' | '—' | '.' | ',' | '*') || c.is_whitespace()).to_string();
            for more in &lines[at + 1..] {
                reason.push(' ');
                reason.push_str(more);
            }
            let reason = one_line(&reason, MAX_REASON_CHARS);
            return Some(Verdict::NotMet(if reason.is_empty() { "the check did not say what is missing".into() } else { reason }));
        }
        after("MET").map(|_| Verdict::Met)
    };
    read(0).or_else(|| read(lines.len().checked_sub(1)?))
}

/// A line without the emphasis or the quotes a model wrapped it in.
fn undress(line: &str) -> &str {
    let line = line.trim_matches(|c: char| matches!(c, '*' | '#') || c.is_whitespace());
    match ['`', '"', '\''].iter().find(|q| line.len() >= 2 && line.starts_with(**q) && line.ends_with(**q)) {
        Some(_) => line[1..line.len() - 1].trim(),
        None => line,
    }
}

/// The goal's say on a `finish` the evidence gate has let through. Nothing to say without an open
/// goal. With one, the checker is asked: `MET` marks the goal met, `NOT MET` refuses the `finish`
/// with what is missing, and anything else lets it through unchecked.
pub(crate) async fn check(cx: &mut FinishCx<'_>, done: &mut Finished) -> Option<Refusal> {
    let here = (cx.sess.evidence.epoch, cx.sess.evidence.checks_run);
    let goal = cx.sess.goal.as_mut().filter(|g| !g.met)?;
    let text = goal.text.clone();
    let report = |cx: &FinishCx<'_>, state: GoalState, reason: &str| cx.sink.emit(EventKind::Goal { goal: text.clone(), state, met: state == GoalState::Met, reason: reason.to_string() });

    // The gate's own pattern: refused again and again with nothing done in between, the model has
    // no more to give. The user gets the turn back, and the goal stays open.
    goal.idle = if goal.refused_at == Some(here) { goal.idle + 1 } else { 0 };
    if goal.idle >= MAX_IDLE {
        (goal.idle, goal.refused_at) = (0, None);
        report(cx, GoalState::Open, "finish came back three times in a row with no change to the workspace and no check run");
        done.evidence.push_str("; the goal is still open");
        return None;
    }

    let changed = changes::changes(cx.sess).await;
    let asked = question(&text, &done.summary, &checks_text(cx.sess, done), changed.as_deref());
    let unchecked = match cx.ask(JUDGE, &asked).await {
        // A stop is not a verdict. The run ends where it is.
        Err(ProviderError::Cancelled) => return None,
        Err(e) => format!("the check's request failed: {e}"),
        Ok(answer) => match verdict(&answer) {
            Some(Verdict::Met) => {
                if let Some(goal) = cx.sess.goal.as_mut() {
                    (goal.met, goal.idle, goal.refused_at) = (true, 0, None);
                }
                report(cx, GoalState::Met, "");
                done.evidence.push_str("; goal met");
                return None;
            }
            Some(Verdict::NotMet(reason)) => {
                if let Some(goal) = cx.sess.goal.as_mut() {
                    goal.refused_at = Some(here);
                }
                report(cx, GoalState::NotMet, &reason);
                let text = format!("[goal] finish refused. The goal is not met yet: {reason}\nKeep working towards it, and call finish again when it is met.");
                return Some(Refusal { violation: "the goal is not met".into(), text });
            }
            None => format!("the check answered neither `MET` nor `NOT MET`: {}", one_line(&answer, 160)),
        },
    };
    // Whatever kept the check from a verdict is not the model's doing, and must not hold it here.
    if let Some(goal) = cx.sess.goal.as_mut() {
        (goal.idle, goal.refused_at) = (0, None);
    }
    report(cx, GoalState::Unchecked, &unchecked);
    done.evidence.push_str("; the goal was not checked");
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_that_starts_with_goal_is_a_command() {
        assert_eq!(parse("/goal every test in tests/ passes"), Some(Command::Set("every test in tests/ passes".into())));
        assert_eq!(parse("  /goal\tthe build is green\nand lint is clean \n"), Some(Command::Set("the build is green\nand lint is clean".into())));
        assert_eq!((parse("/goal"), parse(" /goal  \n")), (Some(Command::Show), Some(Command::Show)));
        assert_eq!((parse("/goal clear"), parse("/goal  CLEAR ")), (Some(Command::Clear), Some(Command::Clear)));
        // Clearing is the one word: anything longer is a condition.
        assert_eq!(parse("/goal clear the backlog"), Some(Command::Set("clear the backlog".into())));
        for other in ["/goalpost moved", "goal: pass", "Fix it. /goal x", "/go", ""] {
            assert_eq!(parse(other), None, "{other:?}");
        }
    }

    #[test]
    fn a_session_starts_with_the_goal_in_its_task_or_beside_it() {
        assert_eq!(at_start("Fix the pager.", None), Ok(("Fix the pager.".into(), None)));
        assert_eq!(at_start("Fix the pager.", Some(" tests pass ")), Ok(("Fix the pager.".into(), Some("tests pass".into()))));
        // With no task, the goal is the task; so is a task that is the command.
        assert_eq!(at_start(" ", Some("tests pass")), Ok(("/goal tests pass".into(), Some("tests pass".into()))));
        assert_eq!(at_start("/goal tests pass\n", Some(" ")), Ok(("/goal tests pass".into(), Some("tests pass".into()))));
        assert!(at_start("/goal a", Some("b")).unwrap_err().contains("given twice"));
        assert!(at_start("/goal", None).unwrap_err().contains("no goal yet") && at_start("/goal clear", Some("x")).unwrap_err().contains("no goal yet"));
        assert!(at_start("x", Some(&"g".repeat(MAX_GOAL_CHARS + 1))).unwrap_err().contains("at most 4000"));
        assert_eq!(checked(&"é".repeat(MAX_GOAL_CHARS)).map(|g| g.chars().count()), Ok(MAX_GOAL_CHARS));
    }

    #[test]
    fn the_goal_goes_to_the_model_worded_as_a_task() {
        let alone = brief("/goal tests pass", "tests pass");
        assert!(alone.starts_with("Work until this goal is met:\n<goal>\ntests pass\n</goal>\n") && !alone.contains("/goal tests"), "{alone}");
        let beside = brief("Fix the pager.", "tests pass");
        assert!(beside.starts_with("Fix the pager.\n\nThe user has set a goal for this session as well:\n<goal>\ntests pass\n</goal>\n"), "{beside}");
        assert!(alone.ends_with("comes back with what is missing.") && beside.ends_with("comes back with what is missing."));
    }

    #[test]
    fn the_checkers_answer_is_read_strictly() {
        let not_met = |reason: &str| Some(Verdict::NotMet(reason.into()));
        assert_eq!(verdict("MET"), Some(Verdict::Met));
        assert_eq!((verdict(" met.\n"), verdict("**MET**"), verdict("`MET`")), (Some(Verdict::Met), Some(Verdict::Met), Some(Verdict::Met)));
        assert_eq!(verdict("NOT MET: two tests still fail"), not_met("two tests still fail"));
        assert_eq!(verdict("not met - `lint` was never run\nRun it."), not_met("`lint` was never run Run it."));
        assert_eq!(verdict("NOT MET"), not_met("the check did not say what is missing"));
        // A verdict after some words of its own is taken from the last line.
        assert_eq!(verdict("The diff touches only the README.\n\nNOT MET: no test was added"), not_met("no test was added"));
        assert_eq!(verdict("Looking at the checks.\nMET"), Some(Verdict::Met));
        for unreadable in ["", "  \n", "Yes, it looks done.", "METHOD missing", "The goal is met.", "NOT METRIC", "unmet"] {
            assert_eq!(verdict(unreadable), None, "{unreadable:?}");
        }
        assert_eq!(verdict(&format!("NOT MET: {}", "x".repeat(900))).map(|v| matches!(v, Verdict::NotMet(r) if r.chars().count() == MAX_REASON_CHARS + 1)), Some(true));
    }

    #[test]
    fn the_checker_is_asked_with_the_four_things_it_judges_by() {
        let asked = question("tests pass", " All green. ", "Run since the last change:\n- `pytest -q`: exit 0", Some(" M a.py"));
        assert_eq!(asked, "<goal>\ntests pass\n</goal>\n<closing_message>\nAll green.\n</closing_message>\n<checks>\nRun since the last change:\n- `pytest -q`: exit 0\n</checks>\n<changes>\n M a.py\n</changes>\nIs the goal met? Reply `MET` or `NOT MET: <what is missing>`.");
        let bare = question("tests pass", "", "No check has been run.", None);
        assert!(bare.contains("<closing_message>\n(none given)\n</closing_message>") && bare.contains("<changes>\nThe session has changed nothing in the workspace.\n</changes>"), "{bare}");
        assert!(JUDGE.contains("`MET`, or `NOT MET: <what is still missing"));
    }
}
