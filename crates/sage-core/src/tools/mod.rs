//! The tool surface. Nine tools, deliberately: every schema is re-sent (and re-read) on every turn.

pub mod ask;
pub mod edit;
pub mod job;
pub mod jobs;
pub mod read;
pub mod search;
pub mod shell;
pub mod write;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Shell,
    Read,
    Search,
    Edit,
    Write,
    Watch,
    Job,
    Ask,
    Finish,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Shell => "shell",
            Tool::Read => "read",
            Tool::Search => "search",
            Tool::Edit => "edit",
            Tool::Write => "write",
            Tool::Watch => "watch",
            Tool::Job => "job",
            Tool::Ask => "ask",
            Tool::Finish => "finish",
        }
    }

    /// Canonical names plus the names other harnesses trained these models to reach for.
    pub fn from_name(name: &str) -> Option<Tool> {
        let n = name.trim().to_ascii_lowercase().replace('-', "_");
        let n = n.rsplit(['.', ':']).next().unwrap_or(&n);
        Some(match n {
            "shell" | "bash" | "sh" | "run" | "exec" | "execute" | "terminal" | "run_command"
            | "execute_command" | "run_terminal_cmd" | "run_shell_command" | "execute_commands"
            | "cmd" | "command" => Tool::Shell,
            "read" | "read_file" | "cat" | "view" | "open" | "view_file" | "read_files" | "ls"
            | "list" | "list_dir" | "list_files" => Tool::Read,
            "search" | "grep" | "rg" | "find" | "glob" | "search_files" | "grep_search"
            | "ripgrep" | "codebase_search" | "file_search" | "find_files" | "find_file"
            | "glob_search" => Tool::Search,
            "edit"
            | "edit_file"
            | "str_replace"
            | "replace"
            | "str_replace_editor"
            | "str_replace_based_edit_tool"
            | "search_replace"
            | "apply_patch"
            | "patch"
            | "replace_in_file"
            | "multi_edit"
            | "multiedit" => Tool::Edit,
            "write" | "write_file" | "create" | "create_file" | "write_to_file" | "save" => {
                Tool::Write
            }
            "watch" | "add_watch" => Tool::Watch,
            "job" | "jobs" | "wait" | "bash_output" | "bashoutput" | "job_output" | "kill"
            | "killshell" => Tool::Job,
            "ask"
            | "ask_user"
            | "ask_question"
            | "ask_followup_question"
            | "askuserquestion"
            | "question"
            | "clarify"
            | "request_user_input" => Tool::Ask,
            "finish" | "done" | "complete" | "task_complete" | "submit" | "attempt_completion"
            | "final_answer" | "end" => Tool::Finish,
            _ => return None,
        })
    }

    /// Acts change or verify the world and can halt a script. Probes only look.
    pub fn is_act(self) -> bool {
        matches!(self, Tool::Shell | Tool::Edit | Tool::Write)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    #[default]
    Ok,
    /// Reality contradicted the stated or implicit expectation.
    Surprise,
    /// The call itself was unusable (bad arguments, missing file).
    Error,
    /// Not run because an earlier step in the same reply was a surprise.
    Skipped,
    /// Still running; handed over as a background job.
    Running,
    /// Refused by the user or by policy.
    Denied,
}

/// A foreground command that ran to its end: what a watch needs to take the run as its own result.
#[derive(Clone, Debug, Default)]
pub struct Completed {
    /// Directory the command started in.
    pub cwd: std::path::PathBuf,
    pub code: Option<i32>,
    pub ms: u64,
    /// The bounded display excerpt.
    pub text: String,
    pub output_complete: bool,
    /// The complete cleaned output, loaded only when a predicate needs it.
    pub predicate_text: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ShellMeta {
    pub cmd: String,
    pub ok: bool,
    /// The model predicted the outcome and was right: this counts as evidence.
    pub held_explicit: bool,
    /// Present only while watches are in use.
    pub completed: Option<Completed>,
}

#[derive(Clone, Debug, Default)]
pub struct StepOutput {
    pub status: StepStatus,
    /// Exactly what the model reads.
    pub observation: String,
    /// What the observation shrinks to if old context is ever trimmed.
    pub stub: String,
    pub violations: Vec<String>,
    /// The model made an explicit prediction for this step.
    pub explicit: bool,
    /// Output bytes that exist but were not put in front of the model.
    pub hidden_bytes: u64,
    /// The step itself changed files in the workspace.
    pub changed_files: bool,
    /// What it did to the file it changed, for the UI. The model is not shown this.
    pub change: Option<crate::diff::FileChange>,
    pub shell: Option<ShellMeta>,
}

impl StepOutput {
    pub fn ok(observation: impl Into<String>) -> Self {
        let observation = observation.into();
        Self {
            stub: crate::util::one_line(&observation, 160),
            observation,
            ..Default::default()
        }
    }

    pub fn error(message: impl AsRef<str>) -> Self {
        let observation = format!("[error] {}", message.as_ref());
        Self {
            status: StepStatus::Error,
            stub: crate::util::one_line(&observation, 160),
            observation,
            ..Default::default()
        }
    }
}

pub fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

pub fn bool_arg(args: &Value, key: &str) -> bool {
    match args.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => {
            matches!(s.trim().to_ascii_lowercase().as_str(), "true" | "yes" | "1")
        }
        Some(Value::Number(n)) => n.as_i64().is_some_and(|n| n != 0),
        _ => false,
    }
}

pub fn u64_arg(args: &Value, key: &str) -> Option<u64> {
    match args.get(key)? {
        Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_names_round_trip_and_aliases_resolve() {
        for t in [
            Tool::Shell,
            Tool::Read,
            Tool::Search,
            Tool::Edit,
            Tool::Write,
            Tool::Watch,
            Tool::Job,
            Tool::Ask,
            Tool::Finish,
        ] {
            assert_eq!(Tool::from_name(t.name()), Some(t));
        }
        assert_eq!(Tool::from_name("Bash"), Some(Tool::Shell));
        assert_eq!(Tool::from_name("functions.str_replace"), Some(Tool::Edit));
        assert_eq!(Tool::from_name("read-file"), Some(Tool::Read));
        assert_eq!(Tool::from_name("task_complete"), Some(Tool::Finish));
        assert_eq!(Tool::from_name("AskUserQuestion"), Some(Tool::Ask));
        assert_eq!(Tool::from_name("teleport"), None);
    }

    #[test]
    fn arg_helpers_coerce_what_models_actually_send() {
        let a = json!({"n": "42", "f": 3.0, "b": "true", "z": 0, "s": "x"});
        assert_eq!(u64_arg(&a, "n"), Some(42));
        assert_eq!(u64_arg(&a, "f"), Some(3));
        assert!(bool_arg(&a, "b"));
        assert!(!bool_arg(&a, "z"));
        assert!(!bool_arg(&a, "missing"));
        assert_eq!(str_arg(&a, "s"), Some("x"));
    }
}
