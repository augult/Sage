//! A reply is a script.
//!
//! All tool calls of one model reply run in order. The first step that does not go as expected halts
//! the rest, exactly like `set -e`: later steps were written on the assumption that it would work.
//! This lets the model commit to a whole stretch of work (edit, check, finish) in a single round of
//! thinking, safely, because a wrong assumption stops the script instead of compounding.

use crate::events::{EventKind, Sink, Stats};
use crate::guard::Expect;
use crate::repair::Call;
use crate::state::Session;
use crate::tools::{self, StepOutput, StepStatus, Tool, bool_arg, str_arg};
use crate::util::{fnv64, one_line};
use crate::watches::{self, WatchChange, WatchReport, WatchState};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Autonomy {
    /// Run everything; only catastrophic commands need a human.
    Auto,
    /// Every step that changes something waits for approval.
    Ask,
}

/// Pending approval requests, answered from the UI.
#[derive(Default)]
pub struct Approvals {
    next: AtomicU64,
    pending: Mutex<HashMap<u64, oneshot::Sender<bool>>>,
}

impl Approvals {
    pub fn request(&self) -> (u64, oneshot::Receiver<bool>) {
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        (id, rx)
    }

    pub fn resolve(&self, id: u64, allow: bool) -> bool {
        match self.pending.lock().unwrap().remove(&id) {
            Some(tx) => tx.send(allow).is_ok(),
            None => false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Finished {
    pub summary: String,
    pub verified: bool,
    pub evidence: String,
}

#[derive(Default)]
pub struct ScriptResult {
    /// (call id, text for the model, stub to decay to), in call order.
    pub observations: Vec<(String, String, String)>,
    /// Harness remarks to attach after the last observation.
    pub footer: Vec<String>,
    pub surprised: bool,
    pub finished: Option<Finished>,
    /// The model asked the user something: the run ends and waits for the answer.
    pub asked: Option<tools::ask::Question>,
    /// Identity of each step that failed, for stall detection.
    pub failed_steps: Vec<u64>,
    pub edited: Vec<String>,
}

pub struct Runner<'a> {
    pub sess: &'a mut Session,
    pub sink: &'a dyn Sink,
    pub cancel: &'a CancellationToken,
    pub approvals: &'a Approvals,
    pub stats: &'a mut Stats,
    pub turn: u32,
    pub autonomy: Autonomy,
    /// Someone is present to answer approval requests.
    pub interactive: bool,
    /// The budget ends with this reply: `finish` is taken as it stands, since nothing can follow it.
    pub landing: bool,
}

fn halts(tool: Option<Tool>, status: StepStatus) -> bool {
    match status {
        StepStatus::Ok | StepStatus::Skipped => false,
        StepStatus::Surprise | StepStatus::Running | StepStatus::Denied => true,
        // A missing file in a batch of reads should not cancel the other reads.
        StepStatus::Error => !matches!(tool, Some(Tool::Read | Tool::Search)),
    }
}

/// Commands whose exit status cannot be a failure, and so prove nothing: what decides the exit
/// status of the line cannot fail. `cd dir && pytest` is a check; `cd dir; echo ok`,
/// `pytest; echo done` (the status is `echo`'s), `pytest || true` and `echo $(pytest)` are not.
fn proves_nothing(cmd: &str) -> bool {
    !can_fail(cmd)
}

/// How two parts of a command line are joined.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Join {
    /// `;`, a newline or `&`: the status is the later part's.
    List,
    And,
    Or,
    Pipe,
}

/// The parts of `cmd` at its top level, each with what joins it to the next: separators inside
/// quotes, `$(…)`, backticks and `(…)` / `{…}` groups belong to their part.
fn top_level(cmd: &str) -> Vec<(String, Option<Join>)> {
    let chars: Vec<char> = cmd.chars().collect();
    let mut parts = Vec::new();
    let mut part = String::new();
    let mut quote: Option<char> = None;
    let mut depth = 0usize;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if let Some(q) = quote {
            part.push(c);
            if c == '\\' && q != '\'' {
                if let Some(n) = next {
                    part.push(n);
                    i += 1;
                }
            } else if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        let join = match c {
            ';' | '\n' if depth == 0 => Some(Join::List),
            '&' if depth == 0 && next == Some('&') => Some(Join::And),
            '|' if depth == 0 && next == Some('|') => Some(Join::Or),
            // `2>&1`, `>&2` and `&>file` are redirections, not a job sent to the background.
            '&' if depth == 0 && i > 0 && matches!(chars[i - 1], '>' | '<') => None,
            '&' if depth == 0 && next == Some('>') => None,
            '&' if depth == 0 => Some(Join::List),
            '|' if depth == 0 => Some(Join::Pipe),
            _ => None,
        };
        if let Some(join) = join {
            parts.push((std::mem::take(&mut part), Some(join)));
            if matches!(join, Join::And | Join::Or) || (join == Join::Pipe && next == Some('&')) {
                i += 1;
            }
            i += 1;
            continue;
        }
        match c {
            '\\' => {
                part.push(c);
                if let Some(n) = next {
                    part.push(n);
                    i += 1;
                }
            }
            '\'' | '"' | '`' => {
                quote = Some(c);
                part.push(c);
            }
            '(' | '{' => {
                depth += 1;
                part.push(c);
            }
            ')' | '}' => {
                depth = depth.saturating_sub(1);
                part.push(c);
            }
            _ => part.push(c),
        }
        i += 1;
    }
    parts.push((part, None));
    parts
}

/// Whether the exit status of `cmd` can be a failure.
fn can_fail(cmd: &str) -> bool {
    // The lists of the line: what `;`, newlines and `&` separate, each an and-or chain of pipelines.
    let mut lists: Vec<Vec<(String, Option<Join>)>> = vec![Vec::new()];
    for (part, join) in top_level(cmd) {
        let ends = join == Some(Join::List);
        lists
            .last_mut()
            .expect("there is always a list")
            .push((part, join.filter(|j| *j != Join::List)));
        if ends {
            lists.push(Vec::new());
        }
    }
    lists.retain(|list| list.iter().any(|(part, _)| !part.trim().is_empty()));
    // Under `set -e` any list that fails ends the line with its status.
    if lists.iter().any(|list| sets_errexit(list)) {
        return lists.iter().any(|list| list_can_fail(list));
    }
    // Otherwise the status is the last list's. A bare `exit` passes on the status before it.
    while let Some(last) = lists.pop() {
        if let [(part, _)] = last.as_slice()
            && part.split_whitespace().collect::<Vec<_>>() == ["exit"]
        {
            continue;
        }
        return list_can_fail(&last);
    }
    false
}

fn sets_errexit(list: &[(String, Option<Join>)]) -> bool {
    list.iter().any(|(part, _)| {
        let words: Vec<&str> = part.split_whitespace().collect();
        let Some((&"set", flags)) = words.split_first() else {
            return false;
        };
        let short = flags
            .iter()
            .any(|w| w.starts_with('-') && !w.starts_with("--") && w.contains('e'));
        short || flags.windows(2).any(|w| w == ["-o", "errexit"])
    })
}

/// An and-or chain: `a && b` fails if either can, `a || b` only if both can.
fn list_can_fail(list: &[(String, Option<Join>)]) -> bool {
    let mut pipelines: Vec<(Vec<&str>, Option<Join>)> = vec![(Vec::new(), None)];
    for (part, join) in list {
        pipelines
            .last_mut()
            .expect("never empty")
            .0
            .push(part.as_str());
        if matches!(join, Some(Join::And | Join::Or)) {
            pipelines.push((Vec::new(), *join));
        }
    }
    let mut chain = pipelines.into_iter();
    let (first, _) = chain.next().expect("never empty");
    // A failure in any stage of a pipeline is reported (see `jobs::masked_failure`).
    let fails = |stages: &[&str]| stages.iter().any(|stage| stage_can_fail(stage));
    let mut can = fails(&first);
    for (stages, join) in chain {
        can = match join {
            Some(Join::Or) => can && fails(&stages),
            _ => can || fails(&stages),
        };
    }
    can
}

fn stage_can_fail(stage: &str) -> bool {
    let stage = stage.trim();
    if let Some(inner) = stage
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .or_else(|| stage.strip_prefix('{').and_then(|s| s.strip_suffix('}')))
    {
        return can_fail(inner);
    }
    let mut words = stage.split_whitespace().skip_while(|w| {
        w.split_once('=').is_some_and(|(k, _)| {
            !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
    });
    match words.next() {
        None => false,
        Some("exit") => !matches!(words.next(), None | Some("0")),
        Some(word) => !matches!(
            word,
            "echo" | "true" | ":" | "printf" | "cd" | "pwd" | "sleep" | "export" | "set" | "unset"
        ),
    }
}

/// Whether `cmd` only inspects files or git state (`cat`, `ls`, `git status`, `git diff`,
/// `rg ... || true`) without running a behavioural check of the project's code.
fn inspection_only(cmd: &str) -> bool {
    let mut lists: Vec<Vec<(String, Option<Join>)>> = vec![Vec::new()];
    for (part, join) in top_level(cmd) {
        let ends = join == Some(Join::List);
        lists
            .last_mut()
            .expect("there is always a list")
            .push((part, join.filter(|j| *j != Join::List)));
        if ends {
            lists.push(Vec::new());
        }
    }
    lists.retain(|list| list.iter().any(|(part, _)| !part.trim().is_empty()));
    !lists.is_empty() && lists.iter().all(|list| list_is_inspection(list))
}

fn list_is_inspection(list: &[(String, Option<Join>)]) -> bool {
    let mut pipelines: Vec<(Vec<&str>, Option<Join>)> = vec![(Vec::new(), None)];
    for (part, join) in list {
        pipelines
            .last_mut()
            .expect("never empty")
            .0
            .push(part.as_str());
        if matches!(join, Some(Join::And | Join::Or)) {
            pipelines.push((Vec::new(), *join));
        }
    }
    let mut chain = pipelines.into_iter();
    let (first, _) = chain.next().expect("never empty");
    let passive = |stages: &[&str]| stages.iter().all(|stage| stage_is_inspection(stage));
    let mut insp = passive(&first);
    for (stages, join) in chain {
        insp = match join {
            // `rg ... || true` cannot fail and only inspects files.
            Some(Join::Or) => insp || passive(&stages),
            _ => insp && passive(&stages),
        };
    }
    insp
}

fn stage_is_inspection(stage: &str) -> bool {
    let stage = stage.trim();
    if let Some(inner) = stage
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .or_else(|| stage.strip_prefix('{').and_then(|s| s.strip_suffix('}')))
    {
        return inspection_only(inner);
    }
    if !stage_can_fail(stage) {
        return true;
    }
    let mut words = stage.split_whitespace().skip_while(|w| {
        w.split_once('=').is_some_and(|(k, _)| {
            !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
    });
    let Some(word) = words.next() else {
        return true;
    };
    let prog = word.rsplit('/').next().unwrap_or(word);
    match prog {
        "cat" | "head" | "tail" | "ls" | "wc" | "stat" | "file" | "nl" | "sort" | "uniq"
        | "cut" | "tr" | "realpath" | "dirname" | "basename" | "which" | "type" => true,
        "git" => matches!(
            words.find(|w| !w.starts_with('-')),
            Some("status" | "diff" | "log" | "show" | "branch" | "rev-parse")
        ),
        _ => false,
    }
}

/// Whether `cmd` only loads or compiles the code (`python3 -c "import calc"`, `cargo build`,
/// `go vet`, `node --check app.js`) and runs none of it. That the code loads says nothing about the
/// rules a spec sets, so such a run never stands for checking them. Parts that cannot fail (`cd`,
/// `echo`) are passed over.
fn loads_only(cmd: &str) -> bool {
    let mut loads = false;
    for (part, _) in top_level(cmd) {
        let stage = part.trim();
        if stage.is_empty() || !stage_can_fail(stage) {
            continue;
        }
        if !stage_loads_only(stage) {
            return false;
        }
        loads = true;
    }
    loads
}

fn stage_loads_only(stage: &str) -> bool {
    let words: Vec<&str> = stage
        .split_whitespace()
        .skip_while(|w| {
            w.split_once('=').is_some_and(|(k, _)| {
                !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            }) || matches!(*w, "env" | "time" | "npx" | "command")
        })
        .collect();
    let Some(first) = words.first() else {
        return false;
    };
    let program = first.rsplit('/').next().unwrap_or(first);
    // The subcommand: the first word after the program that is no flag or toolchain (`+nightly`).
    let sub = words[1..]
        .iter()
        .find(|w| !w.starts_with('-') && !w.starts_with('+'))
        .copied()
        .unwrap_or("");
    if program.starts_with("python") || program.starts_with("pypy") {
        if let Some(i) = words.iter().position(|w| *w == "-m") {
            return matches!(words.get(i + 1).copied(), Some("py_compile" | "compileall"));
        }
        return stage
            .split_once(" -c ")
            .is_some_and(|(_, code)| imports_only(unquoted(code.trim())));
    }
    match program {
        "cargo" => matches!(sub, "build" | "b" | "check" | "c"),
        "go" => matches!(sub, "build" | "vet"),
        "tsc" => true,
        "node" => words.contains(&"--check") || words.contains(&"-c"),
        "bash" | "sh" | "zsh" => words.contains(&"-n"),
        "ruby" => words.contains(&"-c"),
        "php" => words.contains(&"-l"),
        _ => false,
    }
}

/// The text inside the quotes `code` starts with, or all of it when it is not quoted.
fn unquoted(code: &str) -> &str {
    match code.chars().next() {
        Some(q @ ('"' | '\'')) => code[1..].rfind(q).map_or(&code[1..], |end| &code[1..=end]),
        _ => code,
    }
}

/// Python that only imports, perhaps saying so: `import calc`, `from calc import f; print("ok")`.
fn imports_only(code: &str) -> bool {
    let literal = |s: &str| {
        let s = s.trim();
        s.len() >= 2
            && matches!(s.as_bytes()[0], b'"' | b'\'')
            && s.as_bytes()[s.len() - 1] == s.as_bytes()[0]
            && !s[1..s.len() - 1].contains(['"', '\''])
    };
    let statements: Vec<&str> = code
        .split(['\n', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    !statements.is_empty()
        && statements.iter().all(|s| {
            s.starts_with("import ")
                || (s.starts_with("from ") && s.contains(" import "))
                || *s == "pass"
                || s.strip_prefix("print(")
                    .and_then(|rest| rest.strip_suffix(')'))
                    .is_some_and(literal)
        })
}

/// Whether a command or a path names the file the harness keeps API keys in: `config.json` in its
/// data folder, wherever that is, or in the usual `~/.sage`.
fn names_key_file(text: &str) -> bool {
    let own = crate::config::data_dir().join("config.json");
    text.contains(".sage/config.json")
        || text.contains(".sage\\config.json")
        || text.contains(own.to_string_lossy().as_ref())
}

/// Whether a command names the file with the keys, or reaches it through the data folder: the
/// folder itself (`grep -r key ~/.sage`, `cd ~/.sage && cat config.json`), a pattern in it
/// (`cat ~/.sage/conf*`) or anything there whose name starts like the file's. A subfolder such as
/// `~/.sage/sessions` is not refused. A command can always reach a file some other way: this
/// catches the usual spellings, it is no sandbox.
fn shell_reaches_keys(cmd: &str) -> bool {
    if reaches_keys_as_written(cmd) {
        return true;
    }
    // A path spelled with `.` or `..` leads there all the same: `~/.sage/sessions/../config.json`.
    cmd.split(|c: char| c.is_whitespace() || "'\"`;&|()<>=".contains(c))
        .filter_map(from_home)
        .any(|path| reaches_keys_as_written(&crate::workspace::normalize(&path).to_string_lossy()))
}

/// `word` as an absolute path, with a leading `~`, `$HOME` or `${HOME}` put in its place.
fn from_home(word: &str) -> Option<std::path::PathBuf> {
    for home in ["~", "$HOME", "${HOME}"] {
        if let Some(rest) = word
            .strip_prefix(home)
            .filter(|rest| rest.is_empty() || rest.starts_with(['/', '\\']))
        {
            return Some(crate::util::home_dir().join(rest.trim_start_matches(['/', '\\'])));
        }
    }
    std::path::Path::new(word)
        .is_absolute()
        .then(|| std::path::PathBuf::from(word))
}

/// `shell_reaches_keys`, for the command exactly as it is spelled.
fn reaches_keys_as_written(cmd: &str) -> bool {
    if names_key_file(cmd) {
        return true;
    }
    let dir = crate::config::data_dir();
    let mut spellings = vec![dir.to_string_lossy().into_owned()];
    if dir == crate::util::home_dir().join(".sage") {
        spellings.extend(["~/.sage", "$HOME/.sage", "${HOME}/.sage"].map(String::from));
    }
    spellings.iter().any(|spelling| {
        cmd.match_indices(spelling.as_str()).any(|(at, _)| {
            let rest: String = cmd[at + spelling.len()..]
                .chars()
                .take_while(|c| {
                    !c.is_whitespace() && !matches!(c, ';' | '|' | '&' | ')' | '\'' | '"')
                })
                .collect();
            if rest.is_empty() {
                return true;
            }
            // `~/.sage-old` is another folder.
            let Some(inside) = rest.strip_prefix(['/', '\\']) else {
                return false;
            };
            let inside = inside.trim_start_matches(['/', '\\']);
            inside.is_empty()
                || inside.contains(['*', '?', '[', '{'])
                || inside.to_ascii_lowercase().starts_with("conf")
        })
    })
}

fn summarize(call: &Call) -> String {
    let a = &call.args;
    let s = |k: &str| str_arg(a, k).unwrap_or("").trim().to_string();
    match call.tool {
        Some(Tool::Shell) => format!(
            "{}{}",
            one_line(&s("cmd"), 160),
            if bool_arg(a, "background") { "  &" } else { "" }
        ),
        Some(Tool::Read) => {
            let mut t = s("path");
            if !s("lines").is_empty() {
                t.push_str(&format!(":{}", s("lines")));
            }
            if !s("symbol").is_empty() {
                t.push_str(&format!(" `{}`", s("symbol")));
            }
            t
        }
        Some(Tool::Search) => {
            let mut t = if s("pattern").is_empty() {
                "files".to_string()
            } else {
                format!("/{}/", one_line(&s("pattern"), 80))
            };
            if !s("path").is_empty() {
                t.push_str(&format!(" in {}", s("path")));
            }
            if !s("glob").is_empty() {
                t.push_str(&format!(" ({})", s("glob")));
            }
            t
        }
        Some(Tool::Edit) => {
            let place = if !s("range").is_empty() {
                format!(" @{}", s("range"))
            } else if !s("after").is_empty() {
                format!(" after {}", s("after"))
            } else {
                " (text match)".to_string()
            };
            format!("{}{place}", s("path"))
        }
        Some(Tool::Write) => s("path"),
        Some(Tool::Watch) => {
            if bool_arg(a, "off") {
                format!("{} off", s("name"))
            } else {
                format!("{}: {}", s("name"), one_line(&s("cmd"), 120))
            }
        }
        Some(Tool::Job) => {
            let id = s("id");
            if bool_arg(a, "kill") {
                format!("{id} kill")
            } else if !s("until").is_empty() {
                format!("{id} until {:?}", s("until"))
            } else {
                id
            }
        }
        Some(Tool::Ask) => one_line(&s("question"), 160),
        Some(Tool::Finish) => one_line(&s("summary"), 160),
        None => call.raw_name.clone(),
    }
}

impl Runner<'_> {
    pub async fn run(&mut self, calls: &[Call]) -> ScriptResult {
        let scripts = self.sess.features.scripts;
        self.sess.reply += 1;
        let mut res = ScriptResult::default();
        let mut halted: Option<String> = None;
        let mut acted = false;

        for (idx, call) in calls.iter().enumerate() {
            self.sess.step += 1;
            let tool_name = call
                .tool
                .map(|t| t.name().to_string())
                .unwrap_or_else(|| call.raw_name.clone());
            let expect = if self.sess.features.receipts
                && matches!(call.tool, Some(Tool::Shell | Tool::Watch))
            {
                Expect::from_args(&call.args).map(|e| e.describe())
            } else {
                None
            };
            self.sink.emit(EventKind::StepStart {
                turn: self.turn,
                idx: idx as u32,
                call_id: call.id.clone(),
                tool: tool_name.clone(),
                summary: summarize(call),
                expect,
            });

            let started = Instant::now();
            // The closing report of a session that is out of budget is heard even behind a failed
            // step: skipping it would leave no report at all.
            let closing = self.landing && call.tool == Some(Tool::Finish) && call.error.is_none();
            // Whatever the mode: nothing runs once the user has stopped the run, and nothing after
            // an accepted `finish`, which judged the workspace as it was then.
            let stop = if self.cancel.is_cancelled() {
                Some("the user stopped the run")
            } else if res.finished.is_some() {
                Some("the task was already finished in this reply")
            } else {
                None
            };
            let skip = match (stop, &halted) {
                (Some(why), _) => Some(why),
                (None, Some(why)) if !closing => Some(why.as_str()),
                _ => None,
            };
            let out = match skip {
                Some(why) => {
                    let text = format!("[skipped] not run, because {why}.");
                    StepOutput {
                        status: StepStatus::Skipped,
                        stub: text.clone(),
                        observation: text,
                        ..Default::default()
                    }
                }
                None => match (&call.error, call.tool) {
                    (Some(err), None) => {
                        StepOutput::error(format!("{err}; the tools are {}", self.tool_names()))
                    }
                    (Some(err), Some(_)) => StepOutput::error(err),
                    (None, _) => self.execute(call, &mut res, &mut acted).await,
                },
            };
            let ms = started.elapsed().as_millis() as u64;

            self.stats.tool_ms += ms;
            self.stats.hidden_bytes += out.hidden_bytes;
            match out.status {
                StepStatus::Skipped => self.stats.skipped += 1,
                status => {
                    self.stats.steps += 1;
                    if status == StepStatus::Surprise {
                        self.stats.surprises += 1;
                        res.surprised = true;
                    }
                    if out.explicit {
                        self.stats.predictions += 1;
                        if status == StepStatus::Ok {
                            self.stats.predictions_held += 1;
                        }
                    }
                    if matches!(status, StepStatus::Surprise | StepStatus::Error) {
                        res.failed_steps
                            .push(fnv64(format!("{tool_name}\u{0}{}", call.args).as_bytes()));
                    }
                }
            }
            // Nothing after a question is run: the answer may change it.
            if halted.is_none() && res.asked.is_some() {
                halted = Some("the user was asked a question; wait for the answer".to_string());
            }
            if scripts && halted.is_none() && halts(call.tool, out.status) {
                let how = match out.status {
                    StepStatus::Running => "is still running",
                    StepStatus::Denied => "was not allowed",
                    StepStatus::Error => "could not be carried out",
                    _ => "did not go as expected",
                };
                halted = Some(format!("step {} ({tool_name}) {how}", idx + 1));
            }
            self.sink.emit(EventKind::StepEnd {
                turn: self.turn,
                idx: idx as u32,
                call_id: call.id.clone(),
                tool: tool_name,
                status: out.status,
                observation: out.observation.clone(),
                ms,
                hidden_bytes: out.hidden_bytes,
                violations: out.violations.clone(),
                explicit: out.explicit,
                change: out.change.clone(),
            });
            res.observations
                .push((call.id.clone(), out.observation, out.stub));
        }

        // Standing expectations: anything the script changed is checked now, without being asked.
        if self.sess.features.watches
            && acted
            && res.finished.is_none()
            && !self.sess.watches.items.is_empty()
            && !self.cancel.is_cancelled()
        {
            let started = Instant::now();
            let (reports, text) = watches::run_stale(self.sess, self.cancel).await;
            self.stats.tool_ms += started.elapsed().as_millis() as u64;
            if self.emit_watches(&reports) {
                res.surprised = true;
            }
            if !text.is_empty() {
                res.footer.push(text);
            }
        }
        res
    }

    /// The tools this session can run, for a model that called one that does not exist.
    fn tool_names(&self) -> String {
        let mut names = vec!["shell", "read", "search", "edit", "write"];
        // As the tool schemas have it (`prompt::tool_schemas`): with auto-watch, the harness makes
        // the watches itself and `watch` is not offered.
        if self.sess.features.watches && !self.sess.features.auto_watch {
            names.push("watch");
        }
        names.push("job");
        if self.interactive {
            names.push("ask");
        }
        names.push("finish");
        names.join(", ")
    }

    /// Emits watch events; returns true if any watch newly failed.
    fn emit_watches(&mut self, reports: &[WatchReport]) -> bool {
        let mut regressed = false;
        for r in reports {
            if r.state == WatchState::Fail && r.change == WatchChange::Flip {
                regressed = true;
                self.stats.surprises += 1;
            }
            self.sink.emit(EventKind::Watch {
                turn: self.turn,
                name: r.name.clone(),
                cmd: r.cmd.clone(),
                state: r.state,
                change: r.change,
                detail: r.detail.clone(),
            });
        }
        regressed
    }

    /// Asks for approval where the step needs it. Returns what the model is told if it was refused.
    async fn refusal(&mut self, call: &Call, tool: Tool) -> Option<StepOutput> {
        // Whatever the autonomy: what a tool prints goes to the provider with the next request.
        // A path is judged by the file it reaches (`./`, `..` and links included); a command by
        // what it says, which is a tripwire for the usual spellings and no more. `search` of a
        // folder that holds the file skips the file itself (`tools::search`).
        // A watch runs its command now and again after every change: it is held to what a shell
        // command is held to. One that only takes a watch away runs nothing.
        let cmd = match tool {
            Tool::Shell => str_arg(&call.args, "cmd"),
            Tool::Watch if !bool_arg(&call.args, "off") => str_arg(&call.args, "cmd"),
            _ => None,
        };
        let keys = match tool {
            Tool::Shell | Tool::Watch => cmd.is_some_and(shell_reaches_keys),
            Tool::Read | Tool::Search | Tool::Edit | Tool::Write => str_arg(&call.args, "path")
                .is_some_and(|path| {
                    names_key_file(path) || crate::config::holds_keys(&self.sess.ws.resolve(path))
                }),
            _ => false,
        };
        if keys {
            let text = "[denied] refused: that is the harness's own configuration, which holds API keys. Nothing in it bears on the task.".to_string();
            return Some(StepOutput {
                status: StepStatus::Denied,
                stub: text.clone(),
                observation: text,
                ..Default::default()
            });
        }
        if matches!(tool, Tool::Edit | Tool::Write)
            && let Some(path) = str_arg(&call.args, "path")
        {
            let target = self.sess.ws.resolve(path);
            let real = crate::util::canonical(&target).unwrap_or(target);
            if real.starts_with(&self.sess.dir) && !self.sess.ws.root.starts_with(&self.sess.dir) {
                let text = "[denied] refused: that file belongs to the harness's own session record; do not edit it.".to_string();
                return Some(StepOutput {
                    status: StepStatus::Denied,
                    stub: text.clone(),
                    observation: text,
                    ..Default::default()
                });
            }
        }
        let danger = cmd.and_then(tools::shell::danger);
        let runs = tool.is_act() || cmd.is_some();
        if !runs || (self.autonomy == Autonomy::Auto && danger.is_none()) {
            return None;
        }
        let denied = |text: String| StepOutput {
            status: StepStatus::Denied,
            stub: text.clone(),
            observation: text,
            ..Default::default()
        };
        if !self.interactive {
            let reason = danger.map_or_else(
                || "this step requires approval".to_string(),
                |why| format!("this command {why}"),
            );
            return Some(denied(format!(
                "[denied] refused: {reason}, and nobody is present to approve it."
            )));
        }
        let (id, rx) = self.approvals.request();
        self.sink.emit(EventKind::Approval {
            id,
            turn: self.turn,
            tool: tool.name().to_string(),
            summary: summarize(call),
            reason: danger
                .unwrap_or(if tool == Tool::Watch {
                    "this step runs a command, and again after every change"
                } else {
                    "this step changes the workspace"
                })
                .to_string(),
        });
        let allow = tokio::select! {
            answer = rx => answer.unwrap_or(false),
            _ = self.cancel.cancelled() => false,
        };
        self.sink.emit(EventKind::ApprovalResolved { id, allow });
        (!allow).then(|| denied("[denied] the user declined this step. Do not retry it; continue another way or ask what they want.".to_string()))
    }

    async fn execute(
        &mut self,
        call: &Call,
        res: &mut ScriptResult,
        acted: &mut bool,
    ) -> StepOutput {
        let tool = call
            .tool
            .expect("calls without a tool carry an error and never reach execute");
        let args = &call.args;
        if let Some(denied) = self.refusal(call, tool).await {
            return denied;
        }
        let epoch_before = self.sess.evidence.epoch;
        let out = match tool {
            Tool::Shell => tools::shell::run(self.sess, args, self.cancel).await,
            Tool::Read => tools::read::run_spill_off_thread(self.sess, args)
                .await
                .unwrap_or_else(|| tools::read::run(self.sess, args)),
            Tool::Search => tools::search::run_off_thread(self.sess, args, self.cancel).await,
            Tool::Edit => tools::edit::run(self.sess, args).await,
            Tool::Write => tools::write::run(self.sess, args).await,
            Tool::Job => tools::job::run(self.sess, args, self.cancel).await,
            Tool::Watch => self.watch(args).await,
            Tool::Ask => self.ask(args, res),
            Tool::Finish => self.finish(args, res).await,
        };
        if tool.is_act() {
            *acted = true;
        }
        if out.changed_files {
            let path = str_arg(args, "path").unwrap_or("?").trim().to_string();
            self.sess
                .evidence
                .changed(format!("the {} of {path}", tool.name()));
            if !self.sess.touched.contains(&path) {
                self.sess.touched.push(path.clone());
            }
            res.edited.push(path);
        }
        if let Some(meta) = &out.shell {
            let empty_test_run = watches::test_like(&meta.cmd)
                && meta
                    .completed
                    .as_ref()
                    .is_some_and(|r| watches::ran_no_tests(&r.text));
            // Evidence is a prediction of success that came true, made about something that can fail.
            if meta.held_explicit && meta.ok && !proves_nothing(&meta.cmd) && !empty_test_run {
                self.sess
                    .evidence
                    .passed(format!("`{}`", one_line(&meta.cmd, 100)));
            }
            if meta.held_explicit
                && meta.ok
                && !proves_nothing(&meta.cmd)
                && !inspection_only(&meta.cmd)
                && !loads_only(&meta.cmd)
                && !empty_test_run
                && self.sess.evidence.epoch == epoch_before
                && !self.sess.watches.is_auto_cmd(&meta.cmd, &self.sess.ws.cwd)
            {
                self.sess.evidence.passed_beyond_watch();
            }
            // An explicit run satisfies a standing check only for the state it observed. It still
            // counts as an attempt if it changed the workspace.
            if let Some(run) = &meta.completed {
                let seen = watches::observe(self.sess, &meta.cmd, run, epoch_before);
                for r in &seen.reports {
                    self.sink.emit(EventKind::Watch {
                        turn: self.turn,
                        name: r.name.clone(),
                        cmd: r.cmd.clone(),
                        state: r.state,
                        change: r.change,
                        detail: r.detail.clone(),
                    });
                }
                res.footer.extend(seen.notice);
                if self.sess.watches.all_pass_at(self.sess.evidence.epoch) {
                    let names = self.sess.watches.names();
                    self.sess.evidence.passed(format!("watch {names}"));
                    if self
                        .sess
                        .touched
                        .iter()
                        .any(|p| crate::state::is_test_path(p))
                    {
                        self.sess.evidence.passed_beyond_watch();
                    }
                }
            }
        }
        out
    }

    async fn watch(&mut self, args: &Value) -> StepOutput {
        if !self.sess.features.watches {
            return StepOutput::error("unknown tool `watch`");
        }
        let name = str_arg(args, "name")
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .unwrap_or("check")
            .to_string();
        if bool_arg(args, "off") {
            return if self.sess.watches.remove(&name) {
                StepOutput::ok(format!("[ok] watch {name} removed"))
            } else {
                StepOutput::error(format!("there is no watch named {name}"))
            };
        }
        let Some(cmd) = str_arg(args, "cmd")
            .map(str::trim)
            .filter(|c| !c.is_empty())
        else {
            return StepOutput::error("`cmd` is required: the check command to run");
        };
        let expect = if self.sess.features.receipts {
            Expect::from_args(args)
        } else {
            None
        };
        let cwd = self.sess.ws.cwd.clone();
        // Asking for the check the harness already watches means taking it over under this name.
        self.sess
            .watches
            .items
            .retain(|w| !(w.auto && w.cmd == cmd && w.cwd == cwd));
        let idx = self.sess.watches.set(&name, cmd, cwd, expect);
        let report = watches::run_one(self.sess, idx, self.cancel).await;
        if self.sess.watches.all_pass_at(self.sess.evidence.epoch) {
            let names = self.sess.watches.names();
            self.sess.evidence.passed(format!("watch {names}"));
            self.sess.evidence.passed_beyond_watch();
        }
        self.emit_watches(std::slice::from_ref(&report));
        // Registering a check that currently fails is normal (that is often the bug being fixed), so
        // it never halts the script.
        let observation = format!(
            "{}\nIt now re-runs by itself after every change; you will hear about it only when its result changes.",
            report.detail
        );
        StepOutput {
            stub: one_line(&report.detail, 160),
            observation,
            ..Default::default()
        }
    }

    /// Puts a question to the user. With nobody there to answer, the model is told to decide.
    fn ask(&mut self, args: &Value, res: &mut ScriptResult) -> StepOutput {
        if !self.interactive {
            return StepOutput::error(
                "nobody is present to answer questions in this session. Make the most reasonable choice yourself, carry on, and say in `finish` what you assumed.",
            );
        }
        match tools::ask::parse(args) {
            Ok(question) => {
                res.asked = Some(question);
                StepOutput::ok(
                    "Asked. The user's answer comes as their next message: one of your options, or their own words.",
                )
            }
            Err(e) => StepOutput::error(e),
        }
    }

    async fn finish(&mut self, args: &Value, res: &mut ScriptResult) -> StepOutput {
        let summary = str_arg(args, "summary").unwrap_or("").trim().to_string();
        if !self.sess.features.gate {
            res.finished = Some(Finished {
                summary,
                verified: false,
                evidence: "not tracked in classic mode".into(),
            });
            return StepOutput::ok("[done]");
        }
        // The gate judges the workspace as it is now, so bring the watches up to date first.
        // A background job may have changed files since the last check, with no command after it
        // to notice.
        if self.sess.jobs.any_started() {
            self.sess
                .evidence
                .after_command(&self.sess.ws, "a background job", self.sess.deadline)
                .await;
        }
        let unverified = str_arg(args, "unverified")
            .map(str::trim)
            .filter(|r| !r.is_empty());
        // Watches that do not pass against the workspace as it is: their names, whether `unverified`
        // gets past them, whether any of them fails (rather than having passed against an earlier
        // state), and what their last runs said.
        let mut failing: Option<(String, bool, bool, String)> = None;
        if self.sess.features.watches && !self.sess.watches.items.is_empty() {
            let (reports, text) = watches::run_stale(self.sess, self.cancel).await;
            self.emit_watches(&reports);
            if !self.sess.watches.all_pass_at(self.sess.evidence.epoch) {
                let not_passing: Vec<&watches::Watch> = self
                    .sess
                    .watches
                    .items
                    .iter()
                    .filter(|w| w.state != WatchState::Pass || w.epoch != self.sess.evidence.epoch)
                    .collect();
                let names = not_passing
                    .iter()
                    .map(|w| w.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let fails = not_passing.iter().any(|w| w.state == WatchState::Fail);
                // A check the model never asked for must not be able to trap it, and nor may one
                // that is not failing but could not be brought up to date (two watches that write
                // files on every run leave each other behind).
                let waivable = not_passing
                    .iter()
                    .all(|w| w.auto || w.state != WatchState::Fail);
                failing = Some((names, waivable, fails, text));
            }
        }
        if self.landing {
            // Nothing can follow this reply, so a refusal would only cost the report.
            // It is accepted, with what is actually known about the work.
            let ev = &self.sess.evidence;
            let (verified, evidence) = match (&failing, unverified) {
                (Some((names, _, fails, _)), _) => (
                    false,
                    format!(
                        "unverified: watch {names} {} when the budget ran out",
                        if *fails {
                            "was failing"
                        } else {
                            "had no passing result for the final files"
                        }
                    ),
                ),
                (None, _) if ev.fresh() => (true, ev.verified_by()),
                (None, Some(reason)) => (false, format!("unverified: {reason}")),
                (None, None) => (
                    false,
                    format!(
                        "unverified: nothing was checked after {} before the budget ran out",
                        ev.last_change
                    ),
                ),
            };
            res.finished = Some(Finished {
                summary,
                verified,
                evidence,
            });
            return StepOutput::ok(if verified {
                "[done] accepted"
            } else {
                "[done] accepted without verification"
            });
        }
        if let Some((names, waivable, fails, text)) = failing {
            if let (true, Some(reason)) = (waivable, unverified) {
                let state = if fails {
                    "is failing"
                } else {
                    "has no passing result for the final files"
                };
                res.finished = Some(Finished {
                    summary,
                    verified: false,
                    evidence: format!("unverified: {reason} (watch {names} {state})"),
                });
                return StepOutput::ok("[done] accepted without verification");
            }
            let why = if fails {
                "is not passing"
            } else {
                "has no current passing result"
            };
            let head = format!("[SURPRISE] finish refused: watch {names} {why}.");
            let mut detail = if text.is_empty() {
                if fails {
                    "Its output has not changed since it was last shown.".to_string()
                } else {
                    "It passed, but files changed after it ran, so its result is for an earlier state of the files.".to_string()
                }
            } else {
                text
            };
            if waivable && fails {
                detail.push_str("\nFix it. If it cannot pass for a reason outside this task, call finish with `unverified` set to that reason.");
            } else if waivable {
                detail.push_str(
                    "\nCall finish with `unverified` set to the reason it cannot be checked.",
                );
            }
            return StepOutput {
                status: StepStatus::Surprise,
                violations: vec![format!("watch {names} is failing")],
                stub: head.clone(),
                observation: format!("{head}\n{detail}"),
                ..Default::default()
            };
        }
        if !self.sess.evidence.reviewed
            && self.sess.evidence.epoch > self.sess.evidence.message_epoch
            && (self.sess.evidence.fresh() || unverified.is_some())
            && !self.sess.evidence.verified_beyond_watch()
            && crate::state::task_has_spec(&self.sess.task, &self.sess.ws.root)
        {
            self.sess.evidence.reviewed = true;
            let diff = crate::changes::changes(self.sess).await;
            let text = crate::changes::review_text(&self.sess.task, diff.as_deref());
            return StepOutput {
                stub: "[review] one spec verification pass before finish is accepted".into(),
                observation: text,
                ..StepOutput::ok("")
            };
        }
        let ev = &mut self.sess.evidence;
        if ev.fresh() {
            res.finished = Some(Finished {
                summary,
                verified: true,
                evidence: ev.verified_by(),
            });
            return StepOutput::ok("[done] accepted");
        }
        if let Some(reason) = unverified {
            res.finished = Some(Finished {
                summary,
                verified: false,
                evidence: format!("unverified: {reason}"),
            });
            return StepOutput::ok("[done] accepted without verification");
        }
        // The gate asks for a check; it must not be able to trap a model that cannot produce one.
        ev.refusals += 1;
        if ev.refusals > 2 {
            res.finished = Some(Finished {
                summary,
                verified: false,
                evidence: format!("unverified: no check passed after {}", ev.last_change),
            });
            return StepOutput::ok("[done] accepted without verification");
        }
        let text = format!(
            "[gate] finish refused: nothing has been checked since {}. Run a check (shell with expect_exit / expect_has) and call finish right after it in the same reply. If no check is possible, call finish again with `unverified` set to the reason.",
            ev.last_change
        );
        StepOutput {
            status: StepStatus::Surprise,
            violations: vec!["no check has passed since the last change".into()],
            stub: text.clone(),
            observation: text,
            ..Default::default()
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::events::MemorySink;
    use crate::provider::ToolCall;
    use crate::repair::parse_calls;
    use crate::state::{Features, test_session};
    use serde_json::json;

    fn calls(list: &[(&str, Value)]) -> Vec<Call> {
        let raw: Vec<ToolCall> = list
            .iter()
            .enumerate()
            .map(|(i, (n, a))| ToolCall {
                id: format!("c{i}"),
                name: n.to_string(),
                arguments: a.to_string(),
            })
            .collect();
        parse_calls(&raw).0
    }

    async fn run(
        sess: &mut Session,
        list: &[(&str, Value)],
    ) -> (ScriptResult, Stats, Vec<EventKind>) {
        run_as(sess, list, false).await
    }

    /// `landing`: the script is the last reply the budget allows.
    async fn run_as(
        sess: &mut Session,
        list: &[(&str, Value)],
        landing: bool,
    ) -> (ScriptResult, Stats, Vec<EventKind>) {
        let sink = MemorySink::default();
        let mut stats = Stats::default();
        let cancel = CancellationToken::new();
        let approvals = Approvals::default();
        let res = Runner {
            sess,
            sink: &sink,
            cancel: &cancel,
            approvals: &approvals,
            stats: &mut stats,
            turn: 1,
            autonomy: Autonomy::Auto,
            interactive: false,
            landing,
        }
        .run(&calls(list))
        .await;
        let events = sink.events.lock().unwrap().clone();
        (res, stats, events)
    }

    #[tokio::test]
    async fn the_file_that_holds_the_keys_is_not_shown_to_the_model() {
        let mut s = test_session("script-keys", Features::sage());
        let own = crate::config::data_dir().join("config.json");
        let list: Vec<(&str, Value)> = vec![(
            "shell",
            json!({"cmd": "cat ~/.sage/config.json 2>/dev/null | head"}),
        )];
        let (res, _, events) = run(&mut s, &list).await;
        assert_eq!(statuses(&events), vec![StepStatus::Denied]);
        assert!(
            res.observations[0]
                .1
                .starts_with("[denied] refused: that is the harness's own configuration"),
            "{}",
            res.observations[0].1
        );
        // By its real place too, and through `read`; a project's own config.json is nobody's secret.
        let (_, _, events) = run(&mut s, &[("read", json!({"path": own.to_string_lossy()}))]).await;
        assert_eq!(statuses(&events), vec![StepStatus::Denied]);
        let (_, _, events) = run(
            &mut s,
            &[("shell", json!({"cmd": "cat config.json 2>/dev/null; true"}))],
        )
        .await;
        assert_eq!(statuses(&events), vec![StepStatus::Ok]);
    }

    #[tokio::test]
    async fn on_the_last_reply_finish_is_taken_as_it_stands_and_says_what_is_known() {
        let finish = ("finish", json!({"summary": "half done"}));
        // Unchecked and behind a failed step: accepted all the same.
        let mut s = test_session("script-landing", Features::sage());
        std::fs::write(s.ws.root.join("n.txt"), "1\n").unwrap();
        let (res, _, events) = run_as(
            &mut s,
            &[
                ("edit", json!({"path": "n.txt", "old": "1", "new": "3"})),
                (
                    "shell",
                    json!({"cmd": "grep -qx 2 n.txt", "expect_exit": "0"}),
                ),
                finish.clone(),
            ],
            true,
        )
        .await;
        assert_eq!(
            statuses(&events),
            vec![StepStatus::Ok, StepStatus::Surprise, StepStatus::Ok]
        );
        let done = res.finished.expect("nothing can follow the last reply");
        assert_eq!(
            (done.verified, done.evidence.as_str(), done.summary.as_str()),
            (
                false,
                "unverified: nothing was checked after the edit of n.txt before the budget ran out",
                "half done"
            )
        );

        // Any other step behind a failed one is skipped as always.
        let (res, _, events) = run_as(
            &mut s,
            &[
                (
                    "shell",
                    json!({"cmd": "grep -qx 2 n.txt", "expect_exit": "0"}),
                ),
                ("write", json!({"path": "b.txt", "content": "x"})),
                finish.clone(),
            ],
            true,
        )
        .await;
        assert_eq!(
            statuses(&events),
            vec![StepStatus::Surprise, StepStatus::Skipped, StepStatus::Ok]
        );
        assert!(res.finished.is_some() && !s.ws.root.join("b.txt").exists());

        // A failing watch is named; a passing one verifies.
        let mut s = test_session("script-landing-watch", Features::sage());
        std::fs::write(s.ws.root.join("n.txt"), "1\n").unwrap();
        run(
            &mut s,
            &[(
                "watch",
                json!({"name": "is-two", "cmd": "grep -qx 2 n.txt"}),
            )],
        )
        .await;
        let (res, _, _) = run_as(
            &mut s,
            &[
                ("edit", json!({"path": "n.txt", "old": "1", "new": "3"})),
                finish.clone(),
            ],
            true,
        )
        .await;
        assert_eq!(
            res.finished.unwrap().evidence,
            "unverified: watch is-two was failing when the budget ran out"
        );
        let (res, _, _) = run_as(
            &mut s,
            &[
                ("edit", json!({"path": "n.txt", "old": "3", "new": "2"})),
                finish,
            ],
            true,
        )
        .await;
        let done = res.finished.unwrap();
        assert_eq!(
            (done.verified, done.evidence.as_str()),
            (true, "verified by watch is-two")
        );
    }

    #[tokio::test]
    async fn nothing_after_an_accepted_finish_is_run() {
        let mut s = test_session("script-after-finish", Features::sage());
        std::fs::write(s.ws.root.join("n.txt"), "1\n").unwrap();
        let (res, _, events) = run(
            &mut s,
            &[
                (
                    "shell",
                    json!({"cmd": "grep -qx 1 n.txt", "expect_exit": "0"}),
                ),
                ("finish", json!({"summary": "done"})),
                ("write", json!({"path": "late.txt", "content": "x"})),
            ],
        )
        .await;
        assert_eq!(
            statuses(&events),
            vec![StepStatus::Ok, StepStatus::Ok, StepStatus::Skipped]
        );
        assert!(res.finished.unwrap().verified);
        assert!(!s.ws.root.join("late.txt").exists());
    }

    #[tokio::test]
    async fn nothing_is_run_once_the_user_has_stopped_the_run() {
        for features in [Features::sage(), Features::classic()] {
            let mut s = test_session("script-stopped", features);
            let sink = MemorySink::default();
            let mut stats = Stats::default();
            let cancel = CancellationToken::new();
            cancel.cancel();
            let approvals = Approvals::default();
            Runner {
                sess: &mut s,
                sink: &sink,
                cancel: &cancel,
                approvals: &approvals,
                stats: &mut stats,
                turn: 1,
                autonomy: Autonomy::Auto,
                interactive: false,
                landing: false,
            }
            .run(&calls(&[
                ("write", json!({"path": "a.txt", "content": "x"})),
                ("edit", json!({"path": "a.txt", "old": "x", "new": "y"})),
            ]))
            .await;
            let events = sink.events.lock().unwrap().clone();
            assert_eq!(
                statuses(&events),
                vec![StepStatus::Skipped, StepStatus::Skipped]
            );
            assert!(!s.ws.root.join("a.txt").exists());
        }
    }

    #[tokio::test]
    async fn a_background_command_that_changes_files_makes_the_last_check_stale() {
        let mut s = test_session("script-background-change", Features::sage());
        std::fs::write(s.ws.root.join("n.txt"), "1\n").unwrap();
        run(
            &mut s,
            &[(
                "shell",
                json!({"cmd": "grep -qx 1 n.txt", "expect_exit": "0"}),
            )],
        )
        .await;
        assert!(s.evidence.fresh());
        let (res, _, _) = run(
            &mut s,
            &[
                (
                    "shell",
                    json!({"cmd": "echo 2 > n.txt", "background": true}),
                ),
                ("finish", json!({"summary": "done"})),
            ],
        )
        .await;
        assert!(
            res.finished.is_none(),
            "the check is stale: n.txt changed after it"
        );
    }

    #[tokio::test]
    async fn an_unknown_tool_is_answered_with_the_tools_there_are() {
        let unknown = |features: Features| async move {
            let mut s = test_session("script-unknown-tool", features);
            let (res, _, _) = run(&mut s, &[("frobnicate", json!({}))]).await;
            res.observations[0].1.clone()
        };
        let classic = unknown(Features::classic()).await;
        assert!(
            !classic.contains("watch") && classic.contains("finish"),
            "{classic}"
        );
        let sage = unknown(Features::sage()).await;
        assert!(sage.contains("shell") && sage.contains("job"), "{sage}");
        assert!(!sage.contains("ask"), "nobody is present to answer: {sage}");
        // `watch` is named only where the tool schemas offer it.
        assert!(!sage.contains("watch"), "{sage}");
        let manual = unknown(Features::sage().without("auto_watch").unwrap()).await;
        assert!(manual.contains("watch"), "{manual}");
    }

    #[tokio::test]
    async fn a_watch_is_held_to_what_a_shell_command_is_held_to() {
        // The key file: refused, not shown.
        let mut s = test_session("script-watch-keys", Features::sage());
        let key = crate::config::data_dir().join("config.json");
        let (res, _, events) = run(
            &mut s,
            &[(
                "watch",
                json!({"name": "k", "cmd": format!("cat {}", key.display())}),
            )],
        )
        .await;
        assert_eq!(statuses(&events), vec![StepStatus::Denied]);
        assert!(res.observations[0].1.contains("holds API keys"));
        assert!(s.watches.items.is_empty(), "a refused watch is not kept");

        // A command the tripwire catches: refused with nobody to approve it, as through `shell`.
        let (res, _, _) = run(
            &mut s,
            &[(
                "watch",
                json!({"name": "d", "cmd": "echo mkfs > via_watch.txt"}),
            )],
        )
        .await;
        assert!(
            res.observations[0]
                .1
                .starts_with("[denied] refused: this command writes directly to a disk device")
        );
        assert!(!s.ws.root.join("via_watch.txt").exists());

        // Asked first where every step that runs something is, and not run when declined.
        let sink = MemorySink::default();
        let mut stats = Stats::default();
        let cancel = CancellationToken::new();
        let approvals = Approvals::default();
        let list = calls(&[("watch", json!({"name": "w", "cmd": "touch made_by_watch"}))]);
        let mut runner = Runner {
            sess: &mut s,
            sink: &sink,
            cancel: &cancel,
            approvals: &approvals,
            stats: &mut stats,
            turn: 1,
            autonomy: Autonomy::Ask,
            interactive: true,
            landing: false,
        };
        let decline = async {
            while !approvals.resolve(1, false) {
                tokio::task::yield_now().await;
            }
        };
        let (res, _) = tokio::join!(runner.run(&list), decline);
        assert!(
            res.observations[0]
                .1
                .starts_with("[denied] the user declined")
        );
        assert!(!s.ws.root.join("made_by_watch").exists());
        assert!(sink.events.lock().unwrap().iter().any(|e| matches!(
            e,
            EventKind::Approval { tool, .. } if tool == "watch"
        )));

        // Taking a watch away runs nothing and needs no approval.
        s.watches.set("w", "true", s.ws.cwd.clone(), None);
        let sink = MemorySink::default();
        let list = calls(&[("watch", json!({"name": "w", "off": true}))]);
        let res = Runner {
            sess: &mut s,
            sink: &sink,
            cancel: &cancel,
            approvals: &approvals,
            stats: &mut stats,
            turn: 2,
            autonomy: Autonomy::Ask,
            interactive: true,
            landing: false,
        }
        .run(&list)
        .await;
        assert!(res.observations[0].1.starts_with("[ok] watch w removed"));
        assert!(
            !sink
                .events
                .lock()
                .unwrap()
                .iter()
                .any(|e| matches!(e, EventKind::Approval { .. }))
        );
    }

    fn statuses(events: &[EventKind]) -> Vec<StepStatus> {
        events
            .iter()
            .filter_map(|e| {
                if let EventKind::StepEnd { status, .. } = e {
                    Some(*status)
                } else {
                    None
                }
            })
            .collect()
    }

    #[tokio::test]
    async fn one_reply_can_edit_check_and_finish() {
        let mut s = test_session("script-happy", Features::sage());
        std::fs::write(s.ws.root.join("n.txt"), "1\n").unwrap();
        let (res, stats, events) = run(
            &mut s,
            &[
                ("edit", json!({"path": "n.txt", "old": "1", "new": "2"})),
                ("shell", json!({"cmd": "grep -qx 2 n.txt && echo verified", "expect": {"has": "verified"}})),
                ("finish", json!({"summary": "bumped n to 2"})),
            ],
        )
        .await;
        assert_eq!(statuses(&events), vec![StepStatus::Ok; 3]);
        let done = res.finished.expect("accepted in the same reply");
        assert!(done.verified);
        assert_eq!(
            done.evidence,
            "verified by `grep -qx 2 n.txt && echo verified`"
        );
        assert_eq!(
            (
                stats.steps,
                stats.predictions,
                stats.predictions_held,
                stats.surprises
            ),
            (3, 1, 1, 0)
        );
    }

    #[tokio::test]
    async fn a_surprise_halts_everything_after_it() {
        let mut s = test_session("script-halt", Features::sage());
        std::fs::write(s.ws.root.join("n.txt"), "1\n").unwrap();
        let (res, stats, events) = run(
            &mut s,
            &[
                ("edit", json!({"path": "n.txt", "old": "1", "new": "2"})),
                (
                    "shell",
                    json!({"cmd": "grep -qx 3 n.txt", "expect": {"exit": "0"}}),
                ),
                ("shell", json!({"cmd": "touch should-not-exist"})),
                ("finish", json!({"summary": "done"})),
            ],
        )
        .await;
        assert_eq!(
            statuses(&events),
            vec![
                StepStatus::Ok,
                StepStatus::Surprise,
                StepStatus::Skipped,
                StepStatus::Skipped
            ]
        );
        assert!(res.finished.is_none() && res.surprised);
        assert!(!s.ws.root.join("should-not-exist").exists());
        assert_eq!(
            res.observations[2].1,
            "[skipped] not run, because step 2 (shell) did not go as expected."
        );
        assert_eq!((stats.steps, stats.skipped, stats.surprises), (2, 2, 1));
        assert_eq!(res.failed_steps.len(), 1);
    }

    #[tokio::test]
    async fn a_failed_read_does_not_cancel_its_neighbours_but_a_failed_edit_does() {
        let mut s = test_session("script-probes", Features::sage());
        std::fs::write(s.ws.root.join("a.txt"), "alpha\n").unwrap();
        let (_, _, events) = run(
            &mut s,
            &[
                ("read", json!({"path": "missing.txt"})),
                ("read", json!({"path": "a.txt"})),
                ("search", json!({"pattern": "alpha"})),
            ],
        )
        .await;
        assert_eq!(
            statuses(&events),
            vec![StepStatus::Error, StepStatus::Ok, StepStatus::Ok]
        );

        let (_, _, events) = run(
            &mut s,
            &[
                (
                    "edit",
                    json!({"path": "a.txt", "old": "not there", "new": "x"}),
                ),
                ("shell", json!({"cmd": "true"})),
            ],
        )
        .await;
        assert_eq!(
            statuses(&events),
            vec![StepStatus::Error, StepStatus::Skipped]
        );
    }

    #[tokio::test]
    async fn finish_needs_fresh_evidence_or_a_stated_reason() {
        let mut s = test_session("script-gate", Features::sage());
        std::fs::write(s.ws.root.join("n.txt"), "1\n").unwrap();

        // Nothing changed yet: nothing to verify.
        let (res, _, _) = run(&mut s, &[("finish", json!({"summary": "it is 1"}))]).await;
        assert_eq!(res.finished.unwrap().evidence, "no changes were made");

        let (res, _, events) = run(
            &mut s,
            &[
                ("edit", json!({"path": "n.txt", "old": "1", "new": "2"})),
                ("finish", json!({"summary": "done"})),
            ],
        )
        .await;
        assert!(res.finished.is_none());
        assert_eq!(
            statuses(&events),
            vec![StepStatus::Ok, StepStatus::Surprise]
        );
        assert!(
            res.observations[1].1.starts_with(
                "[gate] finish refused: nothing has been checked since the edit of n.txt."
            ),
            "{}",
            res.observations[1].1
        );

        // A command that cannot fail is not a check.
        let (res, _, _) = run(
            &mut s,
            &[
                (
                    "shell",
                    json!({"cmd": "echo all good", "expect": {"has": "good"}}),
                ),
                ("finish", json!({"summary": "done"})),
            ],
        )
        .await;
        assert!(res.finished.is_none());

        // Nor is a run that failed, even if what the model said it would print was printed.
        let (res, _, events) = run(&mut s, &[("write", json!({"path": "log.txt", "content": "x"})), ("shell", json!({"cmd": "sh -c \"echo '5 passing, 1 failing'; exit 1\"", "expect_has": "passing"})), ("finish", json!({"summary": "done"}))]).await;
        assert_eq!(
            statuses(&events),
            vec![StepStatus::Ok, StepStatus::Ok, StepStatus::Surprise]
        );
        assert!(res.finished.is_none());

        // A check behind a `cd` is still a check; a line of commands that cannot fail is not.
        assert!(
            !proves_nothing("cd sub && FOO=1 python3 -m unittest")
                && !proves_nothing("echo start; grep -q 2 n.txt")
        );
        assert!(
            proves_nothing("cd sub && echo 'done; grep x'")
                && proves_nothing("  ")
                && proves_nothing("export A=1; pwd")
        );

        // A check that passed, then another change: stale again.
        run(
            &mut s,
            &[(
                "shell",
                json!({"cmd": "grep -qx 2 n.txt", "expect": {"exit": "0"}}),
            )],
        )
        .await;
        assert!(s.evidence.fresh());
        let (res, _, _) = run(
            &mut s,
            &[
                ("write", json!({"path": "other.txt", "content": "x"})),
                ("finish", json!({"summary": "done"})),
            ],
        )
        .await;
        assert!(res.finished.is_none());

        let (res, _, _) = run(
            &mut s,
            &[(
                "finish",
                json!({"summary": "done", "unverified": "docs only, nothing to run"}),
            )],
        )
        .await;
        let done = res.finished.unwrap();
        assert!(!done.verified);
        assert_eq!(done.evidence, "unverified: docs only, nothing to run");

        // Refused twice for the same change, the third finish is taken as unverified.
        let (res, _, _) = run(
            &mut s,
            &[
                ("write", json!({"path": "third.txt", "content": "x"})),
                ("finish", json!({"summary": "done"})),
            ],
        )
        .await;
        assert!(res.finished.is_none());
        let (res, _, _) = run(&mut s, &[("finish", json!({"summary": "done"}))]).await;
        assert!(res.finished.is_none());
        let (res, _, _) = run(&mut s, &[("finish", json!({"summary": "done"}))]).await;
        let done = res.finished.unwrap();
        assert_eq!(
            (done.verified, done.evidence.as_str()),
            (
                false,
                "unverified: no check passed after the write of third.txt"
            )
        );
    }

    #[tokio::test]
    async fn a_test_run_that_writes_ignored_build_output_is_watched_and_not_run_twice() {
        let mut s = test_session("script-build-output", Features::sage());
        let root = s.ws.root.clone();
        let git = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["init", "-q"])
            .status()
            .unwrap();
        assert!(git.success());
        std::fs::write(root.join(".gitignore"), "build/\n").unwrap();
        std::fs::write(root.join("a.py"), "x = 1\n").unwrap();
        let counter = s.dir.join("runs");
        std::fs::write(
            root.join("run_tests.sh"),
            format!(
                "mkdir -p build && date > build/report.txt && echo run >> '{}'\necho '3 passed'\n",
                counter.display()
            ),
        )
        .unwrap();
        let runs = || {
            std::fs::read_to_string(&counter)
                .map(|t| t.lines().count())
                .unwrap_or(0)
        };
        let tests = (
            "shell",
            json!({"cmd": "bash run_tests.sh", "expect_exit": "0"}),
        );
        // The build output is ignored, so the run left the workspace as it was: it becomes the watch.
        run(&mut s, std::slice::from_ref(&tests)).await;
        assert_eq!(
            (s.evidence.epoch, s.watches.names(), runs()),
            (0, "tests".to_string(), 1)
        );
        // After an edit, the model's own run is the watch's result: the tests are not run again.
        run(
            &mut s,
            &[
                (
                    "edit",
                    json!({"path": "a.py", "old": "x = 1", "new": "x = 2"}),
                ),
                tests,
            ],
        )
        .await;
        assert_eq!((s.evidence.epoch, runs()), (1, 2));
        assert!(s.evidence.fresh());
    }

    #[tokio::test]
    async fn watches_rerun_after_changes_and_gate_finish() {
        let mut s = test_session("script-watch", Features::sage());
        std::fs::write(s.ws.root.join("n.txt"), "1\n").unwrap();
        let (res, _, events) = run(
            &mut s,
            &[
                (
                    "watch",
                    json!({"name": "is-two", "cmd": "grep -qx 2 n.txt && echo ok"}),
                ),
                ("edit", json!({"path": "n.txt", "old": "1", "new": "3"})),
            ],
        )
        .await;
        // Registering a failing watch does not halt: the edit still ran.
        assert_eq!(statuses(&events), vec![StepStatus::Ok, StepStatus::Ok]);
        assert!(
            res.observations[0]
                .1
                .starts_with("watch is-two: FAIL (expected exit 0, got exit 1)"),
            "{}",
            res.observations[0].1
        );
        assert_eq!(
            res.footer,
            vec!["watch is-two: still FAIL, same output as before"]
        );

        let (res, _, _) = run(&mut s, &[("finish", json!({"summary": "done"}))]).await;
        assert!(res.finished.is_none());
        assert!(
            res.observations[0]
                .1
                .starts_with("[SURPRISE] finish refused: watch is-two is not passing.")
        );

        // Fixing it flips the watch, which is both reported and accepted as evidence.
        let (res, _, events) = run(
            &mut s,
            &[
                ("edit", json!({"path": "n.txt", "old": "3", "new": "2"})),
                ("finish", json!({"summary": "n is 2"})),
            ],
        )
        .await;
        let done = res.finished.expect("watch passes now");
        assert_eq!(done.evidence, "verified by watch is-two");
        assert!(events.iter().any(|e| matches!(
            e,
            EventKind::Watch {
                state: WatchState::Pass,
                change: WatchChange::Flip,
                ..
            }
        )));

        // Breaking it again is announced in the footer of that very script.
        let (res, stats, _) = run(
            &mut s,
            &[("edit", json!({"path": "n.txt", "old": "2", "new": "9"}))],
        )
        .await;
        assert!(res.surprised && stats.surprises == 1);
        assert!(
            res.footer[0].starts_with("watch is-two: PASS → FAIL"),
            "{:?}",
            res.footer
        );
    }

    #[tokio::test]
    async fn dangerous_commands_are_refused_when_nobody_can_approve() {
        let mut s = test_session("script-danger", Features::sage());
        let (res, _, events) = run(
            &mut s,
            &[
                ("shell", json!({"cmd": "rm -rf /"})),
                ("shell", json!({"cmd": "echo after"})),
            ],
        )
        .await;
        assert_eq!(
            statuses(&events),
            vec![StepStatus::Denied, StepStatus::Skipped]
        );
        assert!(
            res.observations[0]
                .1
                .contains("recursive delete of the filesystem root")
        );
    }

    #[tokio::test]
    async fn approval_requests_wait_for_an_answer() {
        let mut s = test_session("script-ask", Features::sage());
        let sink = MemorySink::default();
        let mut stats = Stats::default();
        let cancel = CancellationToken::new();
        let approvals = Approvals::default();
        let list = calls(&[
            ("write", json!({"path": "a.txt", "content": "x"})),
            ("write", json!({"path": "b.txt", "content": "y"})),
        ]);
        let answer = async {
            // Allow the first request, decline the second.
            for (id, allow) in [(1u64, true), (2u64, false)] {
                while !approvals.resolve(id, allow) {
                    tokio::task::yield_now().await;
                }
            }
        };
        let mut runner = Runner {
            sess: &mut s,
            sink: &sink,
            cancel: &cancel,
            approvals: &approvals,
            stats: &mut stats,
            turn: 1,
            autonomy: Autonomy::Ask,
            interactive: true,
            landing: false,
        };
        let (res, _) = tokio::join!(runner.run(&list), answer);
        assert!(s.ws.root.join("a.txt").exists() && !s.ws.root.join("b.txt").exists());
        assert!(
            res.observations[1]
                .1
                .starts_with("[denied] the user declined")
        );
        let events = sink.events.lock().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, EventKind::Approval { .. }))
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn classic_mode_runs_every_call_and_accepts_any_finish() {
        let mut s = test_session("script-classic", Features::classic());
        let (res, _, events) = run(
            &mut s,
            &[
                ("shell", json!({"cmd": "exit 1"})),
                ("shell", json!({"cmd": "echo still runs"})),
                ("finish", json!({"summary": "done"})),
            ],
        )
        .await;
        assert_eq!(
            statuses(&events),
            vec![StepStatus::Error, StepStatus::Ok, StepStatus::Ok]
        );
        assert!(res.finished.is_some());
    }

    #[tokio::test]
    async fn a_test_run_becomes_a_watch_so_an_edit_alone_comes_back_checked() {
        let mut s = test_session("script-auto", Features::sage());
        std::fs::write(s.ws.root.join("n.txt"), "1\n").unwrap();
        std::fs::write(
            s.ws.root.join("check.sh"),
            "grep -qx 2 n.txt && echo PASS || { echo \"FAIL: n is $(cat n.txt)\"; exit 1; }\n",
        )
        .unwrap();
        let watch_events = |events: &[EventKind]| -> Vec<(WatchState, WatchChange)> {
            events
                .iter()
                .filter_map(|e| {
                    if let EventKind::Watch { state, change, .. } = e {
                        Some((*state, *change))
                    } else {
                        None
                    }
                })
                .collect()
        };

        // Reproducing the failure sets up the watch from that very run: the command is not run twice.
        let (res, _, events) = run(
            &mut s,
            &[(
                "shell",
                json!({"cmd": "sh check.sh", "expect_exit": "nonzero"}),
            )],
        )
        .await;
        assert_eq!(res.footer.len(), 1);
        assert!(
            res.footer[0].starts_with("watch tests: FAIL · `sh check.sh` is now watched"),
            "{:?}",
            res.footer
        );
        assert_eq!(
            watch_events(&events),
            vec![(WatchState::Fail, WatchChange::New)]
        );

        // An edit on its own is answered with the test result, and that result is evidence.
        let (res, _, _) = run(
            &mut s,
            &[("edit", json!({"path": "n.txt", "old": "1", "new": "2"}))],
        )
        .await;
        assert_eq!(res.footer, vec!["watch tests: FAIL → PASS · PASS"]);
        assert!(s.evidence.fresh());

        // A model that still runs the tests by hand does not make them run a second time.
        let (res, _, events) = run(
            &mut s,
            &[
                ("edit", json!({"path": "n.txt", "old": "2", "new": "9"})),
                (
                    "shell",
                    json!({"cmd": "sh check.sh", "expect_exit": "nonzero"}),
                ),
            ],
        )
        .await;
        assert!(res.footer.is_empty(), "{:?}", res.footer);
        assert_eq!(
            watch_events(&events),
            vec![(WatchState::Fail, WatchChange::Flip)]
        );
        assert!(!s.evidence.fresh());

        // With the watch in place, `finish` can ride along with the edit: it is judged by the watch.
        let (res, _, _) = run(
            &mut s,
            &[
                ("edit", json!({"path": "n.txt", "old": "9", "new": "2"})),
                ("finish", json!({"summary": "n is 2"})),
            ],
        )
        .await;
        assert_eq!(
            res.finished.expect("the watch passes").evidence,
            "verified by watch tests"
        );
    }

    #[tokio::test]
    async fn a_failing_watch_the_model_never_asked_for_can_be_waived_with_a_reason() {
        let mut s = test_session("script-waive", Features::sage());
        std::fs::write(
            s.ws.root.join("check.sh"),
            "echo 'FAIL: the service is unreachable'; exit 1\n",
        )
        .unwrap();
        run(&mut s, &[("shell", json!({"cmd": "sh check.sh"}))]).await;
        run(
            &mut s,
            &[("write", json!({"path": "a.txt", "content": "x"}))],
        )
        .await;

        let (res, _, _) = run(&mut s, &[("finish", json!({"summary": "done"}))]).await;
        assert!(res.finished.is_none());
        assert!(
            res.observations[0]
                .1
                .starts_with("[SURPRISE] finish refused: watch tests is not passing."),
            "{}",
            res.observations[0].1
        );
        assert!(
            res.observations[0]
                .1
                .ends_with("call finish with `unverified` set to that reason."),
            "{}",
            res.observations[0].1
        );
        let (res, _, _) = run(&mut s, &[("finish", json!({"summary": "done", "unverified": "check.sh needs a service that is not running here"}))]).await;
        let done = res.finished.expect("waived");
        assert!(!done.verified);
        assert_eq!(
            done.evidence,
            "unverified: check.sh needs a service that is not running here (watch tests is failing)"
        );

        // Asking for the same check by name takes it over, and a requested watch stays binding.
        let (_, _, _) = run(
            &mut s,
            &[("watch", json!({"name": "service", "cmd": "sh check.sh"}))],
        )
        .await;
        assert_eq!(s.watches.names(), "service");
        let (res, _, _) = run(
            &mut s,
            &[(
                "finish",
                json!({"summary": "done", "unverified": "still unreachable"}),
            )],
        )
        .await;
        assert!(res.finished.is_none());
        assert!(
            !res.observations[0].1.contains("unverified"),
            "{}",
            res.observations[0].1
        );
    }

    #[test]
    fn step_summaries_are_short_and_specific() {
        let c = calls(&[
            (
                "shell",
                json!({"cmd": "pytest -q\n# second line", "background": true}),
            ),
            (
                "read",
                json!({"path": "a.py", "lines": "1-9", "symbol": "f"}),
            ),
            (
                "search",
                json!({"pattern": "todo", "path": "src", "glob": "*.rs"}),
            ),
            (
                "edit",
                json!({"path": "a.py", "range": "4ab-9cd", "new": ""}),
            ),
            ("job", json!({"id": "bg:1", "until": "ready"})),
            ("nope", json!({})),
        ]);
        let got: Vec<String> = c.iter().map(summarize).collect();
        assert_eq!(
            got,
            vec![
                "pytest -q  &",
                "a.py:1-9 `f`",
                "/todo/ in src (*.rs)",
                "a.py @4ab-9cd",
                "bg:1 until \"ready\"",
                "nope"
            ]
        );
    }

    #[tokio::test]
    async fn a_watch_that_writes_a_file_as_it_runs_still_verifies_and_never_traps_finish() {
        // The model's own watch writes a log on every run: its result is for the files as it left
        // them, so `finish` is accepted as verified.
        let mut s = test_session("script-writing-watch", Features::sage());
        std::fs::write(s.ws.root.join("n.txt"), "1\n").unwrap();
        run(
            &mut s,
            &[(
                "watch",
                json!({"name": "check", "cmd": "grep -q 1 n.txt && date +%s%N > last-run.log"}),
            )],
        )
        .await;
        let (res, _, _) = run(&mut s, &[("finish", json!({"summary": "done"}))]).await;
        let done = res.finished.expect("a passing watch lets finish through");
        assert!(done.verified, "{}", done.evidence);

        // The test command the task names writes a file too: a fix sent with `finish` is checked
        // and accepted in the same reply.
        let mut s = test_session("script-writing-preflight", Features::sage());
        let root = s.ws.root.clone();
        std::fs::write(root.join("n.txt"), "1\n").unwrap();
        std::fs::write(
            root.join("test.sh"),
            "echo ran > test-output.log\ngrep -qx 2 n.txt\n",
        )
        .unwrap();
        let cancel = CancellationToken::new();
        let pre = watches::preflight(&mut s, "Make `bash test.sh` pass.", &cancel)
            .await
            .expect("the task names a test command");
        assert_eq!(pre.state, WatchState::Fail);
        let (res, _, events) = run(
            &mut s,
            &[
                ("edit", json!({"path": "n.txt", "old": "1", "new": "2"})),
                ("finish", json!({"summary": "fixed"})),
            ],
        )
        .await;
        assert_eq!(statuses(&events), vec![StepStatus::Ok, StepStatus::Ok]);
        let done = res.finished.expect("the fix passes the named test");
        assert_eq!(
            (done.verified, done.evidence.as_str()),
            (true, "verified by watch tests")
        );
    }

    #[tokio::test]
    async fn two_watches_that_write_on_every_run_can_be_finished_past_with_a_reason() {
        // Each leaves the other behind, so neither result is for the final files. Neither fails:
        // `unverified` gets past them, and the evidence does not call them failing.
        let mut s = test_session("script-two-writers", Features::sage());
        for name in ["a", "b"] {
            run(
                &mut s,
                &[(
                    "watch",
                    json!({"name": name, "cmd": format!("date +%s%N > {name}.log")}),
                )],
            )
            .await;
        }
        let (res, _, _) = run(&mut s, &[("finish", json!({"summary": "done"}))]).await;
        assert!(res.finished.is_none());
        let refused = &res.observations[0].1;
        assert!(
            refused.contains("has no current passing result") && refused.contains("`unverified`"),
            "{refused}"
        );
        let (res, _, _) = run(
            &mut s,
            &[(
                "finish",
                json!({"summary": "done", "unverified": "both checks write logs"}),
            )],
        )
        .await;
        let done = res
            .finished
            .expect("a watch that is not failing cannot trap finish");
        assert!(!done.verified);
        assert!(
            done.evidence
                .contains("has no passing result for the final files")
                && !done.evidence.contains("failing"),
            "{}",
            done.evidence
        );
    }

    #[tokio::test]
    async fn a_check_whose_status_is_not_its_own_is_no_evidence() {
        // `grep` fails, but the line exits with `echo`'s 0.
        let mut s = test_session("script-echo-after", Features::sage());
        std::fs::write(s.ws.root.join("n.txt"), "1\n").unwrap();
        let (res, _, _) = run(
            &mut s,
            &[
                ("edit", json!({"path": "n.txt", "old": "1", "new": "3"})),
                (
                    "shell",
                    json!({"cmd": "grep -qx 2 n.txt; echo checked", "expect_exit": "0"}),
                ),
                ("finish", json!({"summary": "done"})),
            ],
        )
        .await;
        assert!(res.finished.is_none(), "the gate wants a real check");
        assert!(res.observations[2].1.starts_with("[gate] finish refused"));

        for nothing in [
            "pytest -q; echo done",
            "pytest -q || true",
            "echo $(bash -c 'exit 3')",
            "echo \"$(pytest)\"",
            "cd sub && echo 'done; grep x'",
            "export A=1; pwd",
            "  ",
        ] {
            assert!(proves_nothing(nothing), "{nothing}");
        }
        for check in [
            "cd sub && FOO=1 python3 -m unittest",
            "echo start; grep -q 2 n.txt",
            "pytest -q 2>&1 | tail -5",
            "(cd sub && pytest)",
            "{ make; make test; }",
            "set -e; pytest; echo done",
            "set -euo pipefail\npytest\necho done",
            "python app.py & sleep 1; curl -sf localhost:8000/health",
            "pytest || pytest --lf",
            "pytest; exit $?",
            "pytest; exit",
            "! grep -q TODO src.py",
        ] {
            assert!(!proves_nothing(check), "{check}");
        }
    }

    #[tokio::test]
    async fn the_key_file_is_refused_however_it_is_spelled() {
        let mut s = test_session("script-key-spellings", Features::sage());
        let dir = crate::config::data_dir();
        let d = dir.display().to_string();
        for (tool, args) in [
            ("read", json!({"path": format!("{d}/./config.json")})),
            (
                "read",
                json!({"path": format!("{d}/sessions/../config.json")}),
            ),
            (
                "edit",
                json!({"path": format!("{d}/config.json"), "old": "a", "new": "b"}),
            ),
            (
                "write",
                json!({"path": format!("{d}/config.json"), "content": "{}"}),
            ),
            ("shell", json!({"cmd": format!("cat {d}/conf*.json")})),
            ("shell", json!({"cmd": format!("grep -r key {d}")})),
            (
                "shell",
                json!({"cmd": format!("cd {d} && cat config.json")}),
            ),
            (
                "shell",
                json!({"cmd": format!("cat {d}/sessions/../config.json")}),
            ),
            (
                "shell",
                json!({"cmd": format!("cat '{d}/sessions/./../conf.json' {d}/x/../conf*")}),
            ),
        ]
        .into_iter()
        .chain(
            (dir == crate::util::home_dir().join(".sage"))
                .then(|| ("shell", json!({"cmd": "grep -rn sk- ~/.sage/"}))),
        ) {
            let (res, _, events) = run(&mut s, &[(tool, args.clone())]).await;
            assert_eq!(statuses(&events), vec![StepStatus::Denied], "{tool} {args}");
            assert!(res.observations[0].1.contains("holds API keys"));
        }
        // What else is in the data folder, and a folder that only starts like it, are not refused.
        for cmd in [
            format!("ls {d}/sessions 2>/dev/null; true"),
            format!("ls {d}-old 2>/dev/null; true"),
        ] {
            let (_, _, events) = run(&mut s, &[("shell", json!({"cmd": cmd}))]).await;
            assert_eq!(statuses(&events), vec![StepStatus::Ok], "{cmd}");
        }
    }

    #[tokio::test]
    async fn spec_tasks_require_one_verification_beyond_the_starter_watch_or_accept_a_chained_check()
     {
        let mut s = test_session("script-spec-review", Features::sage());
        s.task = "Implement README.md; the existing test covers only the basics.".into();
        std::fs::write(s.ws.root.join("README.md"), "# Spec\nRule 1 and Rule 2.\n").unwrap();
        std::fs::write(s.ws.root.join("n.txt"), "1\n").unwrap();
        let idx = s
            .watches
            .set("tests", "grep -qx 2 n.txt", s.ws.cwd.clone(), None);
        s.watches.items[idx].auto = true;
        s.watches.items[idx].named = true;

        // First finish when only the auto-watch ran triggers one [review] pass.
        let (res, _, _) = run(
            &mut s,
            &[
                (
                    "write",
                    json!({"path": "n.txt", "content": "2\n", "overwrite": true}),
                ),
                ("finish", json!({"summary": "updated n.txt"})),
            ],
        )
        .await;
        assert!(res.finished.is_none());
        assert!(
            res.observations[1]
                .1
                .starts_with("[review] Not accepted yet"),
            "{}",
            res.observations[1].1
        );

        // Next reply chains an in-memory shell check with finish and completes in one reply.
        let (res, _, _) = run(
            &mut s,
            &[
                (
                    "shell",
                    json!({"cmd": "test \"$(cat n.txt)\" = 2", "expect_exit": "0"}),
                ),
                (
                    "finish",
                    json!({"summary": "verified spec rule and finished"}),
                ),
            ],
        )
        .await;
        assert!(res.finished.is_some_and(|f| f.verified));

        // If the model chains its own check right before finish on the first try, 0 review turns are needed.
        let mut s2 = test_session("script-spec-chained", Features::sage());
        std::fs::write(s2.ws.root.join("SPEC.md"), "# Spec\n").unwrap();
        std::fs::write(s2.ws.root.join("n.txt"), "1\n").unwrap();
        let idx = s2
            .watches
            .set("tests", "grep -qx 2 n.txt", s2.ws.cwd.clone(), None);
        s2.watches.items[idx].auto = true;
        let (res2, _, _) = run(
            &mut s2,
            &[
                (
                    "write",
                    json!({"path": "n.txt", "content": "2\n", "overwrite": true}),
                ),
                (
                    "shell",
                    json!({"cmd": "test \"$(cat n.txt)\" = 2", "expect_exit": "0"}),
                ),
                ("finish", json!({"summary": "done in one reply"})),
            ],
        )
        .await;
        assert!(res2.finished.is_some_and(|f| f.verified));
        assert!(!s2.evidence.reviewed);

        // A multi-line command where an earlier python script fails with SyntaxError before the
        // watched test command exits 0 halts the script and does not count as beyond-watch verification.
        let mut s3 = test_session("script-spec-masked-syntax", Features::sage());
        s3.task = "Implement README.md; the existing test covers only the basics.".into();
        std::fs::write(s3.ws.root.join("README.md"), "# Spec\n").unwrap();
        std::fs::write(s3.ws.root.join("n.txt"), "2\n").unwrap();
        let idx = s3
            .watches
            .set("tests", "grep -qx 2 n.txt", s3.ws.cwd.clone(), None);
        s3.watches.items[idx].auto = true;
        let (res3, _, _) = run(
            &mut s3,
            &[
                (
                    "shell",
                    json!({"cmd": "python3 -c '('\ngrep -qx 2 n.txt", "expect_exit": "0"}),
                ),
                ("finish", json!({"summary": "should be skipped"})),
            ],
        )
        .await;
        assert!(res3.surprised);
        assert!(res3.finished.is_none());

        // Passive inspection commands (`cat`, `git status`, `rg ... || true`) do not satisfy
        // beyond-watch spec verification.
        let mut s4 = test_session("script-spec-inspection-only", Features::sage());
        s4.task = "Implement README.md; the existing test covers only the basics.".into();
        std::fs::write(s4.ws.root.join("README.md"), "# Spec\n").unwrap();
        std::fs::write(s4.ws.root.join("n.txt"), "1\n").unwrap();
        let idx = s4
            .watches
            .set("tests", "grep -qx 2 n.txt", s4.ws.cwd.clone(), None);
        s4.watches.items[idx].auto = true;
        let (res4, _, _) = run(
            &mut s4,
            &[
                (
                    "write",
                    json!({"path": "n.txt", "content": "2\n", "overwrite": true}),
                ),
                (
                    "shell",
                    json!({"cmd": "rg -n 'old' . || true; cat n.txt; git status --short || true", "expect_exit": "0"}),
                ),
                ("finish", json!({"summary": "only inspected files"})),
            ],
        )
        .await;
        assert!(res4.finished.is_none());
        assert!(
            res4.observations[2]
                .1
                .starts_with("[review] Not accepted yet")
        );

        // Nor does a check that only loads the code: it says nothing about the spec's rules.
        let mut s5 = test_session("script-spec-import-only", Features::sage());
        s5.task = "Implement the exercise described in README.md.".into();
        std::fs::write(s5.ws.root.join("README.md"), "# Leap\nRules.\n").unwrap();
        let (res5, _, _) = run(
            &mut s5,
            &[
                (
                    "write",
                    json!({"path": "leap.py", "content": "def is_leap(y):\n    return y % 4 == 0\n"}),
                ),
                (
                    "shell",
                    json!({"cmd": "python3 -c \"import leap\"", "expect_exit": "0"}),
                ),
                ("finish", json!({"summary": "implemented"})),
            ],
        )
        .await;
        assert!(res5.finished.is_none());
        assert!(
            res5.observations[2]
                .1
                .starts_with("[review] Not accepted yet"),
            "{}",
            res5.observations[2].1
        );
    }

    #[test]
    fn a_command_that_only_loads_the_code_is_told_from_one_that_runs_it() {
        for cmd in [
            "python3 -c \"import leap\"",
            "python3 -c 'from leap import is_leap; print(\"ok\")'",
            "cd src && python -m py_compile leap.py && echo compiled",
            "cargo build --release",
            "cargo +nightly check",
            "go vet ./...",
            "npx tsc --noEmit",
            "node --check app.js",
            "bash -n run.sh",
        ] {
            assert!(loads_only(cmd), "{cmd}");
        }
        for cmd in [
            "python3 -c \"import leap; print(leap.is_leap(1900))\"",
            "python3 -c \"import leap\" && python3 test_leap.py",
            "python3 leap.py",
            "python3 -m pytest -q",
            "cargo test",
            "go test ./...",
            "node --test",
            "echo done",
        ] {
            assert!(!loads_only(cmd), "{cmd}");
        }
    }
}
