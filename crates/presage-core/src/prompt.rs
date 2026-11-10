//! System prompts and tool schemas.
//!
//! Both are constants: they sit at the very front of every request, so a single changed byte would
//! invalidate the provider's cache for the whole conversation. Anything that varies (date, cwd, what
//! is installed) goes into the first user message instead.

use crate::state::Features;
use serde_json::{Value, json};
use std::sync::LazyLock;

pub const SYSTEM_PRESAGE: &str = "\
You are Presage, a coding agent. You work in a real workspace through tools and carry the task through to the end yourself.

Turns here work differently from other agents:
1. A reply is a script. Every tool call you make in one reply runs, in order. If a shell, edit or write step fails or contradicts your `expect`, the calls after it are skipped and you get the failure in detail. So send the whole next stretch of work in one reply: several reads at once; or edit, then check, then finish.
2. Predict. On `shell`, set `expect` whenever you know what should happen. If you were right you get a one-line receipt instead of the output. If you were wrong you get the output plus the code it points at. Leave `expect` out only when you need to read the output itself.
3. `watch` any check you would otherwise re-run (tests, build, lint). It re-runs by itself after every change and speaks up only when its result changes.
4. `finish` is accepted only if a check has passed since your last change. Chain it: edit, then shell with `expect`, then finish.

Rules:
- Read code before changing it. Lines in read and search output begin with an anchor such as `41kq`; give anchors to `edit` instead of retyping the old text.
- Reproduce exactly any names, paths, commands, formats and output the task specifies.
- Keep changes minimal. Never delete or revert work you did not create.
- After a surprise, find the cause before retrying. Never repeat a failed step unchanged.
- Be brief: no preamble, no recap of tool output, no explanation unless asked.";

pub const SYSTEM_CLASSIC: &str = "\
You are a coding agent. You work in a real workspace through tools and carry the task through to the end yourself.

Use the tools to inspect files, make changes and run commands until the task is done, then call `finish` with a short summary. Verify your work by running the relevant tests or commands before finishing.

Rules:
- Read code before changing it.
- Reproduce exactly any names, paths, commands, formats and output the task specifies.
- Keep changes minimal. Never delete or revert work you did not create.
- Be brief: no preamble, no recap of tool output, no explanation unless asked.";

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({"type": "function", "function": {"name": name, "description": description, "parameters": {"type": "object", "properties": properties, "required": required}}})
}

static TOOLS_PRESAGE: LazyLock<Value> = LazyLock::new(|| {
    let expect = json!({
        "type": "object",
        "description": "Your prediction; the harness checks it.",
        "properties": {
            "exit": {"type": "string", "description": "Expected exit code: \"0\" (default), another code, \"nonzero\" or \"any\"."},
            "has": {"type": "string", "description": "Text (or /regex/) the output must contain."},
            "lacks": {"type": "string", "description": "Text (or /regex/) the output must not contain."}
        }
    });
    json!([
        tool(
            "shell",
            "Run a bash command in the workspace. `cd` and exported variables persist between calls; stdin is closed. Set `expect` when you can predict the outcome: if it holds you get a one-line receipt, otherwise the details, and the remaining calls of this reply are skipped.",
            json!({
                "cmd": {"type": "string"},
                "expect": expect,
                "timeout": {"type": "integer", "description": "Seconds before the command is moved to the background (default 120)."},
                "background": {"type": "boolean", "description": "Start a long-running process (server, watcher) and return at once with a job id."}
            }),
            &["cmd"],
        ),
        tool(
            "read",
            "Read a file (each line starts with an anchor like `41kq` for use in `edit`), list a directory, or reopen saved output (`out:3`). Large files return a window; pass `lines` for another part.",
            json!({
                "path": {"type": "string"},
                "lines": {"type": "string", "description": "Window such as \"120-260\", or \"-80\" for the last 80 lines."},
                "symbol": {"type": "string", "description": "Show only the definition of this function or class."}
            }),
            &["path"],
        ),
        tool(
            "search",
            "Search file contents with a regex (ripgrep rules: smart case, respects .gitignore). Results carry anchors. Give only `glob` to list files.",
            json!({
                "pattern": {"type": "string"},
                "path": {"type": "string", "description": "File or directory to search (default: current directory)."},
                "glob": {"type": "string", "description": "Only files matching this glob, e.g. \"*.py\" or \"src/**/*.ts\"."},
                "context": {"type": "integer", "description": "Lines of context around each match (default 0)."}
            }),
            &[],
        ),
        tool(
            "edit",
            "Change a file. Say where with exactly one of: `range` (anchors from read/search, e.g. \"41kq-47px\"; best for multi-line changes since you do not retype the old text), `old` (exact current text, must be unique), or `after` (anchor to insert after; \"0\" for the top). `new` is the replacement; \"\" deletes.",
            json!({
                "path": {"type": "string"},
                "new": {"type": "string"},
                "range": {"type": "string"},
                "old": {"type": "string"},
                "after": {"type": "string"},
                "all": {"type": "boolean", "description": "With `old`: replace every occurrence."}
            }),
            &["path", "new"],
        ),
        tool(
            "write",
            "Create a file with the given content (parent directories are created). An existing file needs overwrite:true; prefer `edit` for changes.",
            json!({"path": {"type": "string"}, "content": {"type": "string"}, "overwrite": {"type": "boolean"}}),
            &["path", "content"],
        ),
        tool(
            "watch",
            "Register a check command (tests, build, lint). It runs now, then again by itself after every change you make; you hear about it only when its result changes. `finish` requires every watch to pass.",
            json!({
                "name": {"type": "string", "description": "Short label, e.g. \"tests\"."},
                "cmd": {"type": "string"},
                "expect": expect,
                "off": {"type": "boolean", "description": "Remove the watch with this name."}
            }),
            &["name"],
        ),
        tool(
            "job",
            "Check on a background job: wait up to `wait` seconds or until its output contains `until`, then get the new output and its status.",
            json!({
                "id": {"type": "string"},
                "wait": {"type": "integer"},
                "until": {"type": "string", "description": "Text (or /regex/) to wait for."},
                "kill": {"type": "boolean"}
            }),
            &["id"],
        ),
        tool(
            "finish",
            "Declare the task complete. Accepted only if a check has passed since your last change (a shell call whose `expect` held, or passing watches), so chain it after the check in the same reply.",
            json!({
                "summary": {"type": "string", "description": "What was done, in a few lines."},
                "unverified": {"type": "string", "description": "Only when no check is possible: the reason."}
            }),
            &["summary"],
        ),
    ])
});

static TOOLS_CLASSIC: LazyLock<Value> = LazyLock::new(|| {
    json!([
        tool(
            "shell",
            "Run a bash command in the workspace and return its output. `cd` and exported variables persist between calls; stdin is closed.",
            json!({
                "cmd": {"type": "string"},
                "timeout": {"type": "integer", "description": "Seconds before the command is moved to the background (default 120)."},
                "background": {"type": "boolean", "description": "Start a long-running process and return at once with a job id."}
            }),
            &["cmd"],
        ),
        tool(
            "read",
            "Read a file with line numbers, or list a directory. Large files return a window; pass `lines` for another part.",
            json!({"path": {"type": "string"}, "lines": {"type": "string", "description": "Window such as \"120-260\", or \"-80\" for the last 80 lines."}}),
            &["path"],
        ),
        tool(
            "search",
            "Search file contents with a regex (ripgrep rules: smart case, respects .gitignore). Give only `glob` to list files.",
            json!({
                "pattern": {"type": "string"},
                "path": {"type": "string"},
                "glob": {"type": "string"},
                "context": {"type": "integer"}
            }),
            &[],
        ),
        tool(
            "edit",
            "Replace `old` (exact current text, must be unique unless all:true) with `new` in a file.",
            json!({"path": {"type": "string"}, "old": {"type": "string"}, "new": {"type": "string"}, "all": {"type": "boolean"}}),
            &["path", "old", "new"],
        ),
        tool(
            "write",
            "Create a file with the given content. An existing file needs overwrite:true.",
            json!({"path": {"type": "string"}, "content": {"type": "string"}, "overwrite": {"type": "boolean"}}),
            &["path", "content"],
        ),
        tool(
            "job",
            "Check on a background job: wait up to `wait` seconds or until its output contains `until`, then get the new output and its status.",
            json!({"id": {"type": "string"}, "wait": {"type": "integer"}, "until": {"type": "string"}, "kill": {"type": "boolean"}}),
            &["id"],
        ),
        tool("finish", "Declare the task complete.", json!({"summary": {"type": "string", "description": "What was done, in a few lines."}}), &["summary"]),
    ])
});

pub fn system_prompt(features: &Features) -> &'static str {
    if features.scripts { SYSTEM_PRESAGE } else { SYSTEM_CLASSIC }
}

pub fn tool_schemas(features: &Features) -> &'static Value {
    if features.scripts { &TOOLS_PRESAGE } else { &TOOLS_CLASSIC }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::est_tokens;
    use crate::tools::Tool;

    #[test]
    fn every_advertised_tool_exists_and_the_prefix_stays_small() {
        for features in [Features::presage(), Features::classic()] {
            let tools = tool_schemas(&features).as_array().unwrap();
            for t in tools {
                let name = t["function"]["name"].as_str().unwrap();
                assert_eq!(Tool::from_name(name).map(|t| t.name()), Some(name));
                assert_eq!(t["function"]["parameters"]["type"], "object");
            }
            // The fixed prefix is paid for on every turn of every session: keep it lean.
            let prefix = est_tokens(system_prompt(&features)) + est_tokens(&tool_schemas(&features).to_string());
            assert!(prefix < 2_000, "prefix is ~{prefix} tokens");
        }
        assert_eq!(tool_schemas(&Features::presage()).as_array().unwrap().len(), 8);
        assert!(!tool_schemas(&Features::classic()).to_string().contains("expect"));
    }

    #[test]
    fn schemas_serialise_identically_every_time() {
        let a = tool_schemas(&Features::presage()).to_string();
        let b = tool_schemas(&Features::presage()).to_string();
        assert_eq!(a, b);
        assert!(a.find("\"shell\"").unwrap() < a.find("\"finish\"").unwrap());
    }
}
