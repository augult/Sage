//! System prompts and tool schemas.
//!
//! Both sit at the very front of every request, so a single changed byte invalidates the provider's
//! cache for the whole conversation. Anything that varies (date, cwd, what is installed) goes into
//! the first user message instead.
//!
//! They are also re-read on every turn, at the full input price whenever the provider's cache
//! misses. So the system prompt explains how turns work once, the tool descriptions say only what
//! each tool does, and a tool is offered only where it is needed.

use crate::state::Features;
use serde_json::{Value, json};
use std::sync::LazyLock;

/// `{chain}`, `{watch}` and `{finish}` depend on whether test runs are watched automatically.
const SYSTEM_PRESAGE: &str = "\
You are Presage, a coding agent. You work in a real workspace through tools and carry the task through to the end yourself.

Turns here work differently from other agents:
1. A reply is a script. All its tool calls run, in order; a shell, edit or write step that fails or contradicts your prediction skips the calls after it and comes back in detail. So one reply carries the whole next stretch of work, for example: {chain}. Never spend a reply on a single step when the following steps are already clear: a wrong assumption only stops the script.
2. Predict. On `shell`, set `expect_exit`, `expect_has` or `expect_lacks` whenever you know the outcome. If you were right you get a short receipt instead of the output; if you were wrong, the output plus the code it points at. Give no prediction when you need to read the output itself.
3. {watch}
4. `finish` is accepted only if a check has passed since your last change: a shell prediction that held, or passing watches. {finish}

Rules:
- Read code before changing it. Lines in read and search output begin with an anchor such as `41kq`; give anchors to `edit` instead of retyping the old text.
- Reproduce exactly any names, paths, commands, formats and output the task specifies.
- Keep changes minimal. Never delete or revert work you did not create.
- After a surprise, find the cause before retrying. Never repeat a failed step unchanged.
- Be brief: no preamble, no recap of tool output, no explanation unless asked.";

fn presage_prompt(auto_watch: bool) -> String {
    let (chain, watch, finish) = if auto_watch {
        (
            "read + read + shell (run the tests); then edit + edit + finish",
            "Tests are watched for you. Once you have run a test command, the harness re-runs it by itself after every change and tells you when its result changes, so do not run it again yourself.",
            "Once the tests are watched, put `finish` in the same reply as your edits: it is refused if they fail.",
        )
    } else {
        (
            "read + read + read; or edit + edit + shell (run the tests) + finish",
            "`watch` any check you would otherwise re-run (tests, build, lint). It re-runs by itself after every change and speaks up only when its result changes.",
            "Put it in the same reply, right after the check.",
        )
    };
    SYSTEM_PRESAGE.replace("{chain}", chain).replace("{watch}", watch).replace("{finish}", finish)
}

static PRESAGE_AUTO: LazyLock<String> = LazyLock::new(|| presage_prompt(true));
static PRESAGE_MANUAL: LazyLock<String> = LazyLock::new(|| presage_prompt(false));

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

fn presage_tools(watch: bool, job: bool) -> Value {
    let mut tools = vec![
        tool(
            "shell",
            "Run a bash command in the workspace. `cd` and exported variables persist between calls; stdin is closed.",
            json!({
                "cmd": {"type": "string"},
                "expect_exit": {"type": "string", "description": "Predicted exit code: \"0\", another code, \"nonzero\" or \"any\"."},
                "expect_has": {"type": "string", "description": "Text or /regex/ the output will contain."},
                "expect_lacks": {"type": "string", "description": "Text or /regex/ it will not contain."},
                "timeout": {"type": "integer", "description": "Seconds before it moves to the background (default 120)."},
                "background": {"type": "boolean", "description": "Start a server or watcher; returns a job id at once."}
            }),
            &["cmd"],
        ),
        tool(
            "read",
            "Read a file (each line starts with an anchor like `41kq`), list a directory, or reopen saved output (`out:3`). Large files return a window.",
            json!({
                "path": {"type": "string"},
                "lines": {"type": "string", "description": "Such as \"120-260\", or \"-80\" for the last 80 lines."},
                "symbol": {"type": "string", "description": "Only this function or class."}
            }),
            &["path"],
        ),
        tool(
            "search",
            "Regex search of file contents (ripgrep rules: smart case, respects .gitignore). Results carry anchors. `glob` alone lists files.",
            json!({
                "pattern": {"type": "string"},
                "path": {"type": "string"},
                "glob": {"type": "string"},
                "context": {"type": "integer", "description": "Lines around each match."}
            }),
            &[],
        ),
        tool(
            "edit",
            "Change a file. Say where with exactly one of `range` (anchors, e.g. \"41kq-47px\"), `old` (exact current text, must be unique) or `after` (an anchor; \"0\" for the top). `new` is the replacement; \"\" deletes.",
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
            "Create a file (parent directories are created). An existing file needs overwrite:true; prefer `edit` for changes.",
            json!({"path": {"type": "string"}, "content": {"type": "string"}, "overwrite": {"type": "boolean"}}),
            &["path", "content"],
        ),
    ];
    if watch {
        tools.push(tool(
            "watch",
            "Register a check command (tests, build, lint). It runs now and by itself after every change; you hear about it only when its result changes, and `finish` requires it to pass.",
            json!({
                "name": {"type": "string", "description": "Short label, e.g. \"tests\"."},
                "cmd": {"type": "string", "description": "Passes when it exits 0."},
                "off": {"type": "boolean", "description": "Remove the watch with this name."}
            }),
            &["name"],
        ));
    }
    if job {
        tools.push(tool(
            "job",
            "Check on a background job: wait up to `wait` seconds or until its output contains `until`, then get its new output and status.",
            json!({"id": {"type": "string"}, "wait": {"type": "integer"}, "until": {"type": "string"}, "kill": {"type": "boolean"}}),
            &["id"],
        ));
    }
    tools.push(tool(
        "finish",
        "Declare the task complete. Refused unless a check has passed since your last change.",
        json!({
            "summary": {"type": "string", "description": "What was done, in a few lines."},
            "unverified": {"type": "string", "description": "Only when no check is possible: the reason."}
        }),
        &["summary"],
    ));
    Value::Array(tools)
}

/// Indexed by `watch * 2 + job`.
static TOOLS_PRESAGE: LazyLock<[Value; 4]> = LazyLock::new(|| [presage_tools(false, false), presage_tools(false, true), presage_tools(true, false), presage_tools(true, true)]);

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
    if !features.scripts {
        SYSTEM_CLASSIC
    } else if features.watches && features.auto_watch {
        PRESAGE_AUTO.as_str()
    } else {
        PRESAGE_MANUAL.as_str()
    }
}

/// The tool list for a request. `watch` is offered only where the model has to register its checks
/// itself, and `job` only once the session has a background job (`jobs`). Lengthening the list
/// re-buys the whole prompt once, which is cheaper than carrying an unused schema on every turn of
/// every session. Any tool still runs when called by name, offered or not.
pub fn tool_schemas(features: &Features, jobs: bool) -> &'static Value {
    if !features.scripts {
        return &TOOLS_CLASSIC;
    }
    let watch = features.watches && !features.auto_watch;
    &TOOLS_PRESAGE[usize::from(watch) * 2 + usize::from(jobs)]
}

/// Rough size in tokens of what every request starts with: system prompt plus tool schemas.
pub fn prefix_tokens(features: &Features) -> u64 {
    crate::text::est_tokens(system_prompt(features)) + crate::text::est_tokens(&tool_schemas(features, false).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;

    fn names(features: &Features, jobs: bool) -> Vec<&'static str> {
        tool_schemas(features, jobs).as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap()).collect()
    }

    #[test]
    fn every_advertised_tool_exists_and_the_prefix_stays_small() {
        let manual = Features::presage().without("auto_watch").unwrap();
        for features in [Features::presage(), manual, Features::classic()] {
            for t in tool_schemas(&features, true).as_array().unwrap() {
                let name = t["function"]["name"].as_str().unwrap();
                assert_eq!(Tool::from_name(name).map(|t| t.name()), Some(name));
                assert_eq!(t["function"]["parameters"]["type"], "object");
            }
        }
        // The fixed prefix is paid for on every turn of every session: keep it lean.
        let (presage, classic) = (prefix_tokens(&Features::presage()), prefix_tokens(&Features::classic()));
        assert!(presage < 1_200, "the Presage prefix is ~{presage} tokens");
        assert!(classic < 800, "the classic prefix is ~{classic} tokens");
        assert!(!tool_schemas(&Features::classic(), false).to_string().contains("expect"));
    }

    #[test]
    fn a_tool_is_offered_only_where_it_is_needed() {
        let auto = Features::presage();
        let manual = Features::presage().without("auto_watch").unwrap();
        assert_eq!(names(&auto, false), ["shell", "read", "search", "edit", "write", "finish"]);
        assert_eq!(names(&auto, true), ["shell", "read", "search", "edit", "write", "job", "finish"]);
        assert_eq!(names(&manual, false), ["shell", "read", "search", "edit", "write", "watch", "finish"]);
        assert_eq!(names(&manual, true).len(), 8);
        // The baseline is a fixed list.
        assert_eq!(names(&Features::classic(), false), names(&Features::classic(), true));
        assert_eq!(names(&Features::classic(), false).len(), 7);
    }

    #[test]
    fn the_prompt_describes_the_mechanisms_that_are_switched_on() {
        let auto = system_prompt(&Features::presage());
        let manual = system_prompt(&Features::presage().without("auto_watch").unwrap());
        assert!(auto.contains("Tests are watched for you") && !auto.contains("`watch`"));
        assert!(manual.contains("`watch` any check you would otherwise re-run") && !manual.contains("watched for you"));
        for prompt in [auto, manual] {
            assert!(!prompt.contains('{'), "a placeholder was left in: {prompt}");
        }
        assert_eq!(system_prompt(&Features::presage().without("scripts").unwrap()), SYSTEM_CLASSIC);
    }

    #[test]
    fn schemas_serialise_identically_every_time() {
        let a = tool_schemas(&Features::presage(), false).to_string();
        let b = tool_schemas(&Features::presage(), false).to_string();
        assert_eq!(a, b);
        assert!(a.find("\"shell\"").unwrap() < a.find("\"finish\"").unwrap());
    }
}
