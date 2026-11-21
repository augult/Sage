//! Making sense of what the model actually sent.
//!
//! Cheap models produce slightly malformed tool calls at a steady low rate: a trailing comma, a raw
//! newline inside a JSON string, an argument named the way another harness names it, a call written
//! out as text. Failing the turn costs a full round of thinking; fixing what is unambiguous costs
//! nothing. Every repair is counted so its frequency is visible.

use crate::provider::ToolCall;
use crate::tools::Tool;
use serde_json::{Map, Value, json};

#[derive(Clone, Debug)]
pub struct Call {
    pub id: String,
    pub raw_name: String,
    pub tool: Option<Tool>,
    pub args: Value,
    /// Set when the call cannot be executed as sent; reported back to the model as the step's result.
    pub error: Option<String>,
}

/// Escapes raw control characters that appear inside JSON strings.
fn escape_control_in_strings(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    let (mut in_string, mut escaped) = (false, false);
    for c in s.chars() {
        if in_string {
            if escaped {
                escaped = false;
                out.push(c);
            } else {
                match c {
                    '\\' => {
                        escaped = true;
                        out.push(c);
                    }
                    '"' => {
                        in_string = false;
                        out.push(c);
                    }
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                    c => out.push(c),
                }
            }
        } else {
            if c == '"' {
                in_string = true;
            }
            out.push(c);
        }
    }
    out
}

fn strip_trailing_commas(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let (mut in_string, mut escaped) = (false, false);
    for (i, c) in chars.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if *c == '\\' {
                escaped = true;
            } else if *c == '"' {
                in_string = false;
            }
            out.push(*c);
            continue;
        }
        if *c == '"' {
            in_string = true;
        }
        if *c == ','
            && chars[i + 1..]
                .iter()
                .find(|n| !n.is_whitespace())
                .is_some_and(|n| matches!(n, '}' | ']'))
        {
            continue;
        }
        out.push(*c);
    }
    out
}

/// Closes whatever a cut-off JSON document left open.
fn close_truncated(s: &str) -> String {
    let mut stack: Vec<char> = Vec::new();
    let (mut in_string, mut escaped) = (false, false);
    for c in s.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' => {
                stack.pop();
            }
            _ => {}
        }
    }
    // Inside a string that was cut off, a trailing comma is its content.
    let mut out = if in_string {
        s.to_string()
    } else {
        s.trim_end().trim_end_matches(',').to_string()
    };
    if escaped {
        out.pop();
    }
    if in_string {
        out.push('"');
    }
    if out.trim_end().ends_with(':') {
        out.push_str("null");
    }
    while let Some(close) = stack.pop() {
        out.push(close);
    }
    out
}

/// Parses tool-call arguments, repairing the common unambiguous defects. The flag is true when a
/// repair was needed.
pub fn parse_args(raw: &str) -> Result<(Value, bool), String> {
    let s = raw.trim();
    if s.is_empty() {
        return Ok((json!({}), false));
    }
    let first_error = match serde_json::from_str::<Value>(s) {
        Ok(Value::String(inner)) => {
            // Arguments encoded twice: a JSON string that itself contains the JSON object.
            return match serde_json::from_str::<Value>(inner.trim()) {
                Ok(v) if v.is_object() => Ok((v, true)),
                _ => Ok((Value::String(inner), false)),
            };
        }
        Ok(v) => return Ok((v, false)),
        Err(e) => e.to_string(),
    };
    let unfenced = s
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let escaped = escape_control_in_strings(unfenced);
    let decomma = strip_trailing_commas(&escaped);
    let closed = close_truncated(&decomma);
    for candidate in [
        unfenced,
        escaped.as_str(),
        decomma.as_str(),
        closed.as_str(),
    ] {
        if let Ok(v) = serde_json::from_str::<Value>(candidate)
            && v.is_object()
        {
            return Ok((v, true));
        }
    }
    Err(first_error)
}

/// True for arguments that stop in the middle: valid JSON only once what they left open is closed.
/// From a reply that never said it was finished, that is a call the model did not get to the end of.
pub fn cut_off(raw: &str) -> bool {
    let s = raw.trim();
    if s.is_empty() || serde_json::from_str::<Value>(s).is_ok() {
        return false;
    }
    let mended = strip_trailing_commas(&escape_control_in_strings(
        s.trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim(),
    ));
    serde_json::from_str::<Value>(&mended).is_err()
        && serde_json::from_str::<Value>(&close_truncated(&mended)).is_ok()
}

/// A tool's argument names, each with the names other harnesses give it.
type Aliases = &'static [(&'static str, &'static [&'static str])];

const ALIASES: &[(Tool, Aliases)] = &[
    (
        Tool::Shell,
        &[
            (
                "cmd",
                &["command", "script", "bash", "code", "input", "commands"],
            ),
            ("timeout", &["timeout_secs", "timeout_seconds", "timeout_s"]),
            (
                "background",
                &["run_in_background", "bg", "detach", "is_background"],
            ),
            ("expect", &["expected", "expects", "expectation", "predict"]),
            ("expect_exit", &["exit", "exit_code", "expected_exit"]),
            ("expect_has", &["has", "contains", "expected_output"]),
            ("expect_lacks", &["lacks", "not_contains"]),
        ],
    ),
    (
        Tool::Read,
        &[
            (
                "path",
                &[
                    "file_path",
                    "file",
                    "filename",
                    "filepath",
                    "target_file",
                    "target",
                    "dir",
                    "directory",
                ],
            ),
            ("lines", &["line_range", "view_range", "window"]),
            ("symbol", &["function", "definition", "name"]),
        ],
    ),
    (
        Tool::Search,
        &[
            (
                "pattern",
                &[
                    "query",
                    "regex",
                    "search",
                    "text",
                    "q",
                    "search_term",
                    "needle",
                ],
            ),
            (
                "path",
                &[
                    "dir",
                    "directory",
                    "root",
                    "cwd",
                    "target_directories",
                    "search_path",
                ],
            ),
            (
                "glob",
                &[
                    "include",
                    "file_pattern",
                    "glob_pattern",
                    "files",
                    "file_glob",
                    "include_pattern",
                ],
            ),
            ("context", &["context_lines", "-C", "C"]),
        ],
    ),
    (
        Tool::Edit,
        &[
            (
                "path",
                &["file_path", "file", "filename", "filepath", "target_file"],
            ),
            (
                "old",
                &[
                    "old_string",
                    "old_str",
                    "old_text",
                    "search",
                    "find",
                    "original",
                    "old_content",
                ],
            ),
            (
                "new",
                &[
                    "new_string",
                    "new_str",
                    "new_text",
                    "replace",
                    "replacement",
                    "content",
                    "updated",
                    "new_content",
                    "text",
                ],
            ),
            ("range", &["lines", "anchor", "anchors", "at", "line_range"]),
            ("after", &["insert_after", "insert_line", "after_line"]),
            ("all", &["replace_all", "global"]),
        ],
    ),
    (
        Tool::Write,
        &[
            (
                "path",
                &["file_path", "file", "filename", "filepath", "target_file"],
            ),
            (
                "content",
                &["text", "contents", "file_text", "data", "new", "body"],
            ),
            ("overwrite", &["force", "replace"]),
        ],
    ),
    (
        Tool::Watch,
        &[
            ("name", &["id", "label", "title"]),
            ("cmd", &["command", "check", "script"]),
            ("off", &["remove", "delete", "stop", "unwatch"]),
            ("expect", &["expected", "expects"]),
        ],
    ),
    (
        Tool::Job,
        &[
            (
                "id",
                &["job_id", "job", "bash_id", "shell_id", "task_id", "handle"],
            ),
            ("wait", &["timeout", "seconds", "wait_seconds", "wait_secs"]),
            ("until", &["pattern", "match", "wait_for", "contains"]),
            ("kill", &["stop", "terminate", "cancel"]),
        ],
    ),
    (
        Tool::Finish,
        &[
            (
                "summary",
                &[
                    "message",
                    "result",
                    "answer",
                    "text",
                    "final_answer",
                    "output",
                    "response",
                ],
            ),
            (
                "unverified",
                &["reason", "unverified_reason", "no_check_reason"],
            ),
        ],
    ),
];

/// Renames arguments other harnesses taught these models to use, and folds numeric line windows into
/// the one `lines` string this harness takes.
pub fn normalize_args(tool: Tool, args: Value) -> Result<Value, String> {
    let mut map: Map<String, Value> = match args {
        Value::Object(m) => m,
        Value::String(s) => {
            let key = match tool {
                Tool::Shell => "cmd",
                Tool::Read => "path",
                Tool::Search => "pattern",
                Tool::Finish => "summary",
                Tool::Job => "id",
                _ => {
                    return Err(format!(
                        "`{}` needs a JSON object of arguments, got a bare string",
                        tool.name()
                    ));
                }
            };
            let mut m = Map::new();
            m.insert(key.to_string(), Value::String(s));
            m
        }
        Value::Null => Map::new(),
        other => {
            return Err(format!(
                "`{}` needs a JSON object of arguments, got {other}",
                tool.name()
            ));
        }
    };
    if let Some((_, table)) = ALIASES.iter().find(|(t, _)| *t == tool) {
        for (canonical, aliases) in table.iter() {
            if map.contains_key(*canonical) {
                continue;
            }
            if let Some(alias) = aliases.iter().find(|a| map.contains_key(**a)) {
                let v = map.remove(*alias).expect("just checked");
                map.insert(canonical.to_string(), v);
            }
        }
    }
    // A nested `expect` that leaked out in GLM's native call syntax (see `guard::unleak`). Only
    // there: in any other argument, text that holds those marks is text the model means as it is.
    if let Some(value) = map.get_mut("expect")
        && let Some(obj) = value.as_str().and_then(crate::guard::unleak)
    {
        *value = obj;
    }
    let num = |m: &Map<String, Value>, k: &str| {
        m.get(k).and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        })
    };
    // `read {"out": 1}`: the model means the saved output `out:1`.
    if tool == Tool::Read
        && !map.contains_key("path")
        && let Some(n) = map.get("out").and_then(|v| {
            v.as_u64().map(|n| n.to_string()).or_else(|| {
                v.as_str()
                    .map(|s| s.trim().trim_start_matches("out:").to_string())
            })
        })
    {
        map.remove("out");
        map.insert("path".into(), json!(format!("out:{n}")));
    }
    if tool == Tool::Read {
        // `view_range: [400, 420]`, as other harnesses take it.
        if let Some(Value::Array(ends)) = map.get("lines") {
            let ends: Vec<u64> = ends
                .iter()
                .filter_map(|v| {
                    v.as_u64()
                        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
                })
                .collect();
            match ends.as_slice() {
                [from, to] => drop(map.insert("lines".into(), json!(format!("{from}-{to}")))),
                [from] => drop(map.insert("lines".into(), json!(format!("{from}+")))),
                _ => drop(map.remove("lines")),
            }
        }
    }
    if tool == Tool::Read && !map.contains_key("lines") {
        let start = num(&map, "start_line")
            .or_else(|| num(&map, "start"))
            .or_else(|| num(&map, "offset"))
            .or_else(|| num(&map, "from"));
        let end = num(&map, "end_line")
            .or_else(|| num(&map, "end"))
            .or_else(|| num(&map, "to"));
        let limit = num(&map, "limit").or_else(|| num(&map, "max_lines"));
        match (start, end, limit) {
            (Some(s), Some(e), _) => drop(map.insert("lines".into(), json!(format!("{s}-{e}")))),
            (Some(s), None, Some(l)) => drop(map.insert(
                "lines".into(),
                json!(format!("{s}-{}", s.saturating_add(l.saturating_sub(1)))),
            )),
            (Some(s), None, None) => drop(map.insert("lines".into(), json!(format!("{s}+")))),
            (None, _, Some(l)) => drop(map.insert("lines".into(), json!(format!("1-{l}")))),
            _ => {}
        }
    }
    if tool == Tool::Edit {
        // `range: 12` or `range: [12, 14]`: line numbers where an anchor string was meant.
        let number = |v: &Value| {
            v.as_u64()
                .map(|n| n.to_string())
                .or_else(|| v.as_str().map(|s| s.trim().to_string()))
        };
        let range = match map.get("range") {
            Some(Value::Number(n)) => Some(n.to_string()),
            Some(Value::Array(ends)) => match ends
                .iter()
                .map(number)
                .collect::<Option<Vec<_>>>()
                .as_deref()
            {
                Some([from, to]) => Some(format!("{from}-{to}")),
                Some([only]) => Some(only.clone()),
                _ => None,
            },
            _ => None,
        };
        if let Some(range) = range {
            map.insert("range".into(), json!(range));
        }
    }
    if tool == Tool::Shell {
        if let Some(ms) = num(&map, "timeout_ms") {
            map.entry("timeout").or_insert(json!(ms.div_ceil(1000)));
        }
        if let Some(Value::Array(parts)) = map.get("cmd") {
            let parts: Vec<&str> = parts.iter().filter_map(Value::as_str).collect();
            map.insert("cmd".into(), json!(command_line(&parts)));
        }
    }
    Ok(Value::Object(map))
}

/// A command given as a list. A shell started with a script (`["bash", "-lc", "make test"]`) is
/// that script; a program and its arguments (`["git", "commit", "-m", "fix it"]`) is one command,
/// quoted as the shell would need it; a list of whole command lines (`["npm ci", "npm test"]`) is
/// each of them in turn.
fn command_line(parts: &[&str]) -> String {
    let Some(first) = parts.first() else {
        return String::new();
    };
    let program = first.rsplit('/').next().unwrap_or(first);
    if matches!(program, "bash" | "sh" | "zsh" | "dash" | "ksh")
        && let Some(at) = parts.iter().position(|part| {
            part.len() > 1 && part.starts_with('-') && !part.starts_with("--") && part.contains('c')
        })
        && let Some(script) = parts.get(at + 1)
    {
        return script.to_string();
    }
    if first.contains(char::is_whitespace) {
        return parts.join(" && ");
    }
    parts
        .iter()
        .map(|part| {
            let plain = !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_-./=:,+@%^".contains(c));
            if plain {
                part.to_string()
            } else {
                format!("'{}'", part.replace('\'', r"'\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Tool names other harnesses give to finding files by name, which `search` does with `glob`.
fn finds_files(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase().replace('-', "_");
    let name = name.rsplit(['.', ':']).next().unwrap_or(&name);
    matches!(
        name,
        "glob" | "glob_search" | "file_search" | "find_files" | "find_file"
    )
}

/// What a file name pattern given to a file-finding tool comes to as a glob: a glob as it is, a
/// bare name or a part of one as any file whose name holds it.
fn as_glob(pattern: &str) -> String {
    let pattern = pattern.trim();
    if pattern.contains(['*', '?', '[', '{']) {
        pattern.to_string()
    } else {
        format!("*{pattern}*")
    }
}

/// Tool calls written into the reply text instead of the structured field.
pub fn calls_in_text(content: &str) -> Vec<ToolCall> {
    fn push(found: &mut Vec<ToolCall>, v: &Value) {
        let name = v
            .get("name")
            .or_else(|| v.get("tool"))
            .or_else(|| v.get("function"))
            .and_then(Value::as_str);
        let args = v
            .get("arguments")
            .or_else(|| v.get("args"))
            .or_else(|| v.get("parameters"))
            .or_else(|| v.get("input"));
        if let (Some(name), Some(args)) = (name, args)
            && Tool::from_name(name).is_some()
        {
            let arguments = match args {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            found.push(ToolCall {
                id: format!("text_{}", found.len()),
                name: name.to_string(),
                arguments,
            });
        }
    }
    let mut found = Vec::new();
    let mut rest = content;
    while let Some(start) = rest.find("<tool_call>") {
        let after = &rest[start + "<tool_call>".len()..];
        let end = after.find("</tool_call>").unwrap_or(after.len());
        if let Ok((v, _)) = parse_args(after[..end].trim()) {
            push(&mut found, &v);
        }
        rest = &after[end..];
    }
    if found.is_empty() {
        let trimmed = content.trim();
        let body = trimmed
            .strip_prefix("```json")
            .or_else(|| trimmed.strip_prefix("```"))
            .map(|b| b.trim_end_matches("```").trim())
            .unwrap_or(trimmed);
        if body.starts_with('{')
            && body.ends_with('}')
            && let Ok(v) = serde_json::from_str::<Value>(body)
        {
            push(&mut found, &v);
        }
    }
    found
}

/// Turns the provider's raw calls into executable ones. Returns the calls and how many needed repair.
pub fn parse_calls(raw: &[ToolCall]) -> (Vec<Call>, u32) {
    let mut repairs = 0u32;
    let calls = raw
        .iter()
        .map(|rc| {
            let tool = Tool::from_name(&rc.name);
            if tool.is_some_and(|t| t.name() != rc.name) {
                repairs += 1;
            }
            let Some(tool) = tool else {
                return Call { id: rc.id.clone(), raw_name: rc.name.clone(), tool: None, args: Value::Null, error: Some(format!("unknown tool `{}`", rc.name)) };
            };
            let (args, repaired) = match parse_args(&rc.arguments) {
                Ok(v) => v,
                Err(e) => {
                    return Call { id: rc.id.clone(), raw_name: rc.name.clone(), tool: Some(tool), args: Value::Null, error: Some(format!("the arguments for `{}` were not valid JSON ({e}); send one JSON object", tool.name())) };
                }
            };
            if repaired {
                repairs += 1;
            }
            // Other harnesses give one editor tool several jobs: its `view` is a read here and its
            // `create` a write. A tool named for killing a job means just that.
            let foreign = rc.name.to_ascii_lowercase();
            let (tool, mut args) = match (tool, args.get("command").and_then(Value::as_str)) {
                (Tool::Edit, Some("view")) => (Tool::Read, args),
                (Tool::Edit, Some("create")) => (Tool::Write, args),
                _ => (tool, args),
            };
            if let (Tool::Job, true, Some(map)) = (tool, foreign.contains("kill"), args.as_object_mut()) {
                map.entry("kill").or_insert(json!(true));
            }
            if tool == Tool::Edit && foreign.contains("patch") && args.get("path").is_none() && args.get("file_path").is_none() {
                return Call { id: rc.id.clone(), raw_name: rc.name.clone(), tool: Some(tool), args: Value::Null, error: Some(format!("`{}` takes no patch here. Use edit with `path`, `new` and one of `range`, `old` or `after`: one call for each change.", rc.name)) };
            }
            let before = args.clone();
            match normalize_args(tool, args) {
                Ok(mut args) => {
                    // `Glob {"pattern": "**/*.py"}`: a pattern for file names, not for contents.
                    if tool == Tool::Search
                        && finds_files(&rc.name)
                        && let Some(map) = args.as_object_mut()
                        && !map.contains_key("glob")
                        && let Some(pattern) = map.remove("pattern")
                    {
                        let glob = pattern.as_str().map(as_glob).unwrap_or_default();
                        map.insert("glob".into(), json!(glob));
                    }
                    if args != before {
                        repairs += 1;
                    }
                    Call { id: rc.id.clone(), raw_name: rc.name.clone(), tool: Some(tool), args, error: None }
                }
                Err(e) => Call { id: rc.id.clone(), raw_name: rc.name.clone(), tool: Some(tool), args: Value::Null, error: Some(e) },
            }
        })
        .collect();
    (calls, repairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_json_passes_untouched() {
        let (v, repaired) = parse_args(r#"{"cmd": "ls -la", "expect": {"exit": "0"}}"#).unwrap();
        assert!(!repaired);
        assert_eq!(v["cmd"], "ls -la");
        assert_eq!(parse_args("  ").unwrap(), (json!({}), false));
    }

    #[test]
    fn repairs_the_usual_defects() {
        let cases = [
            ("{\"cmd\": \"echo a\nb\"}", "echo a\nb"),
            (r#"{"cmd": "ls",}"#, "ls"),
            (
                r#"```json
{"cmd": "ls"}
```"#,
                "ls",
            ),
            (r#""{\"cmd\": \"ls\"}""#, "ls"),
            (r#"{"cmd": "make test"#, "make test"),
            (r#"{"cmd": "x", "expect": {"exit": "0""#, "x"),
        ];
        for (raw, cmd) in cases {
            let (v, repaired) = parse_args(raw).unwrap_or_else(|e| panic!("{raw:?}: {e}"));
            assert!(repaired, "{raw:?}");
            assert_eq!(v["cmd"], cmd, "{raw:?}");
        }
        assert!(parse_args("not json at all").is_err());
        // A comma that a cut-off string ends with is its content, not a trailing comma.
        assert_eq!(
            parse_args(r#"{"cmd": "echo a,"#).unwrap().0["cmd"],
            "echo a,"
        );
        // A comma inside a string is content, not a trailing comma.
        assert_eq!(
            strip_trailing_commas(r#"{"a": "x,}", "b": 1,}"#),
            r#"{"a": "x,}", "b": 1}"#
        );
        // Which of these stopped half-way is known, for the reply that ended without saying why.
        assert!(
            cut_off(r#"{"path": "a.py", "content": "import os\nprint("#)
                && cut_off(r#"{"cmd": "x", "expect": {"exit": "0""#)
        );
        assert!(
            !cut_off(r#"{"cmd": "ls",}"#)
                && !cut_off(r#"{"cmd": "ls"}"#)
                && !cut_off("")
                && !cut_off("not json at all")
        );
    }

    #[test]
    fn a_line_window_given_as_an_array_is_understood() {
        let lines = |args: Value| {
            normalize_args(Tool::Read, args)
                .unwrap()
                .get("lines")
                .cloned()
        };
        assert_eq!(
            lines(json!({"path": "a.py", "view_range": [400, 402]})),
            Some(json!("400-402"))
        );
        assert_eq!(
            lines(json!({"path": "a.py", "lines": ["7"]})),
            Some(json!("7+"))
        );
        assert_eq!(lines(json!({"path": "a.py", "lines": []})), None);
        let range = |args: Value| {
            normalize_args(Tool::Edit, args)
                .unwrap()
                .get("range")
                .cloned()
        };
        assert_eq!(
            range(json!({"path": "a.py", "new": "x", "range": 2})),
            Some(json!("2"))
        );
        assert_eq!(
            range(json!({"path": "a.py", "new": "x", "range": [2, "4kq"]})),
            Some(json!("2-4kq"))
        );
        assert_eq!(
            range(json!({"path": "a.py", "new": "x", "range": "2ab-4kq"})),
            Some(json!("2ab-4kq"))
        );
    }

    #[test]
    fn another_harnesss_tools_are_taken_for_what_they_mean() {
        let call = |name: &str, args: Value| {
            parse_calls(&[ToolCall {
                id: "c".into(),
                name: name.into(),
                arguments: args.to_string(),
            }])
            .0
            .remove(0)
        };
        // One editor tool with a `command`: viewing is reading, creating is writing.
        let view = call(
            "str_replace_editor",
            json!({"command": "view", "path": "a.py", "view_range": [3, 9]}),
        );
        assert_eq!(
            (view.tool, view.args["lines"].as_str()),
            (Some(Tool::Read), Some("3-9"))
        );
        let create = call(
            "str_replace_based_edit_tool",
            json!({"command": "create", "path": "a.py", "file_text": "x = 1\n"}),
        );
        assert_eq!(
            (create.tool, create.args["content"].as_str()),
            (Some(Tool::Write), Some("x = 1\n"))
        );
        let replace = call(
            "str_replace_editor",
            json!({"command": "str_replace", "path": "a.py", "old_str": "1", "new_str": "2"}),
        );
        assert_eq!(
            (
                replace.tool,
                replace.args["old"].as_str(),
                replace.args["new"].as_str()
            ),
            (Some(Tool::Edit), Some("1"), Some("2"))
        );
        // A tool for killing a job kills it; a patch is answered with how to edit here.
        let kill = call("KillShell", json!({"shell_id": "bg:1"}));
        assert_eq!(
            (
                kill.tool,
                kill.args["id"].as_str(),
                kill.args["kill"].as_bool()
            ),
            (Some(Tool::Job), Some("bg:1"), Some(true))
        );
        let patch = call(
            "apply_patch",
            json!({"input": "*** Begin Patch\n*** End Patch"}),
        );
        assert!(
            patch
                .error
                .as_deref()
                .is_some_and(|e| e.contains("takes no patch here")
                    && e.contains("`range`, `old` or `after`")),
            "{:?}",
            patch.error
        );
    }

    #[test]
    fn maps_foreign_argument_names_and_line_windows() {
        let edit = normalize_args(
            Tool::Edit,
            json!({"file_path": "a.py", "old_string": "x", "new_string": "y", "replace_all": true}),
        )
        .unwrap();
        assert_eq!(
            edit,
            json!({"path": "a.py", "old": "x", "new": "y", "all": true})
        );

        let read = normalize_args(
            Tool::Read,
            json!({"file_path": "a.py", "offset": 40, "limit": 20}),
        )
        .unwrap();
        assert_eq!(read["lines"], "40-59");
        let read = normalize_args(
            Tool::Read,
            json!({"path": "a.py", "start_line": "5", "end_line": 9}),
        )
        .unwrap();
        assert_eq!(read["lines"], "5-9");

        let shell =
            normalize_args(Tool::Shell, json!({"command": "ls", "timeout_ms": 90500})).unwrap();
        assert_eq!(
            (&shell["cmd"], &shell["timeout"]),
            (&json!("ls"), &json!(91))
        );
        assert_eq!(
            normalize_args(Tool::Shell, json!("pwd")).unwrap(),
            json!({"cmd": "pwd"})
        );
        assert!(normalize_args(Tool::Edit, json!("x")).is_err());

        // Seen live from GLM-5.3.
        assert_eq!(
            normalize_args(Tool::Read, json!({"out": "1"})).unwrap(),
            json!({"path": "out:1"})
        );
        let leaked = normalize_args(
            Tool::Shell,
            json!({"cmd": "x", "expect": "exit</arg_key><arg_value>nonzero"}),
        )
        .unwrap();
        assert_eq!(leaked["expect"], json!({"exit": "nonzero"}));

        // A canonical name is never overwritten by an alias that happens to be present too.
        let both = normalize_args(
            Tool::Write,
            json!({"path": "a", "content": "real", "text": "other"}),
        )
        .unwrap();
        assert_eq!(both["content"], "real");
    }

    #[test]
    fn parses_calls_counts_repairs_and_reports_what_it_cannot_fix() {
        let raw = [
            ToolCall {
                id: "1".into(),
                name: "bash".into(),
                arguments: r#"{"command": "ls"}"#.into(),
            },
            ToolCall {
                id: "2".into(),
                name: "read".into(),
                arguments: r#"{"path": "a.py"}"#.into(),
            },
            ToolCall {
                id: "3".into(),
                name: "teleport".into(),
                arguments: "{}".into(),
            },
            ToolCall {
                id: "4".into(),
                name: "edit".into(),
                arguments: "{{{".into(),
            },
        ];
        let (calls, repairs) = parse_calls(&raw);
        assert_eq!(calls[0].tool, Some(Tool::Shell));
        assert_eq!(calls[0].args, json!({"cmd": "ls"}));
        assert!(calls[1].error.is_none());
        assert!(
            calls[2]
                .error
                .as_deref()
                .unwrap()
                .contains("unknown tool `teleport`")
        );
        assert!(
            calls[3]
                .error
                .as_deref()
                .unwrap()
                .contains("not valid JSON")
        );
        assert_eq!(
            repairs, 2,
            "the tool alias and the argument alias on call 1"
        );
    }

    #[test]
    fn finds_calls_written_as_text() {
        let text = "I'll check.\n<tool_call>\n{\"name\": \"shell\", \"arguments\": {\"cmd\": \"ls\"}}\n</tool_call>\n<tool_call>{\"name\": \"read\", \"arguments\": \"{\\\"path\\\": \\\"a\\\"}\"}</tool_call>";
        let calls = calls_in_text(text);
        assert_eq!(calls.len(), 2);
        assert_eq!(
            (calls[0].name.as_str(), calls[0].arguments.as_str()),
            ("shell", r#"{"cmd":"ls"}"#)
        );
        assert_eq!(calls[1].arguments, r#"{"path": "a"}"#);

        assert_eq!(
            calls_in_text(
                "```json\n{\"name\": \"finish\", \"arguments\": {\"summary\": \"ok\"}}\n```"
            )
            .len(),
            1
        );
        assert!(
            calls_in_text("Here is some JSON: {\"name\": \"not_a_tool\", \"arguments\": {}}")
                .is_empty()
        );
        assert!(calls_in_text("All done, the tests pass.").is_empty());
    }

    #[test]
    fn a_command_given_as_a_list_is_read_as_the_shell_would_run_it() {
        let cmd =
            |v: Value| normalize_args(Tool::Shell, json!({"command": v})).unwrap()["cmd"].clone();
        assert_eq!(
            cmd(json!(["bash", "-lc", "cat a.txt | wc -l"])),
            "cat a.txt | wc -l"
        );
        assert_eq!(cmd(json!(["/bin/sh", "-c", "make test"])), "make test");
        assert_eq!(
            cmd(json!(["git", "commit", "-m", "fix it's bug"])),
            r"git commit -m 'fix it'\''s bug'"
        );
        assert_eq!(cmd(json!(["npm ci", "npm test"])), "npm ci && npm test");
        assert_eq!(cmd(json!(["ls", "-la", "src/"])), "ls -la src/");
    }

    #[test]
    fn a_tool_that_finds_files_by_name_is_a_glob() {
        let call = |name: &str, args: Value| {
            let raw = [ToolCall {
                id: "c".into(),
                name: name.into(),
                arguments: args.to_string(),
            }];
            parse_calls(&raw).0.remove(0)
        };
        let glob = call("Glob", json!({"pattern": "**/*.rs", "path": "src"}));
        assert_eq!(
            (glob.tool, glob.args),
            (
                Some(Tool::Search),
                json!({"glob": "**/*.rs", "path": "src"})
            )
        );
        assert_eq!(
            call("file_search", json!({"query": "app.py"})).args,
            json!({"glob": "*app.py*"})
        );
        // A search of contents stays one, whatever it is called.
        assert_eq!(
            call("grep", json!({"pattern": "fn main"})).args,
            json!({"pattern": "fn main"})
        );
        assert_eq!(
            call("glob", json!({"pattern": "x", "glob": "*.rs"})).args,
            json!({"pattern": "x", "glob": "*.rs"})
        );
    }
}
