//! System prompts and tool schemas.
//!
//! Both sit at the very front of every request, so a single changed byte invalidates the provider's
//! cache for the whole conversation. Anything that varies (date, cwd, what is installed) goes into
//! the first user message instead.
//!
//! They are also re-read on every turn, at the full input price whenever the provider's cache
//! misses. So the system prompt explains how turns work once, the tool descriptions say only what
//! each tool does, and a tool is offered only where it is needed.

use crate::skills::SkillInfo;
use crate::state::Features;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::LazyLock;

/// The built-in Sage prompt, as a template. `{chain}`, `{watch}` and `{finish}` are filled in
/// according to whether test runs are watched automatically.
const SYSTEM_SAGE: &str = include_str!("../prompts/system.md");

/// The review step of the built-in template, for a template saved with the `{review}` placeholder.
const SELF_CHECK: &str = "Before `finish`, return to your list from step 1 and tick off every item against what you actually did. Re-read your changes once (`git diff`, or the edited regions) looking for a missed part of the request, a broken neighbour, leftover debugging, a violated constraint.";

/// A project's instruction file is cut to this many characters.
pub const MAX_PROJECT_CHARS: usize = 12_000;
/// Where a project keeps its instructions; the first of these that has any is used.
const PROJECT_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", ".sage/AGENTS.md", ".sage.md"];

pub fn default_template() -> &'static str {
    SYSTEM_SAGE.trim_end()
}

/// A template that lacks a placeholder simply gets nothing put in for it.
fn fill(template: &str, auto_watch: bool, preflight: bool) -> String {
    let (chain, watch, gate) = if auto_watch {
        (
            "read + read + search + shell (run the tests); then edit + edit + shell (try what the tests do not cover)",
            if preflight {
                "Tests are watched for you. The harness runs the test command the task names (or else takes the first one you run) and re-runs it by itself after every change, telling you when its result changes. Do not run it again yourself."
            } else {
                "Tests are watched for you. Once you have run a test command, the harness re-runs it by itself after every change and tells you when its result changes, so do not run it again yourself."
            },
            "Watched tests are re-run first and it is refused if they fail.",
        )
    } else {
        (
            "read + read + read; or edit + edit + shell (run the tests)",
            "`watch` any check you would otherwise re-run (tests, build, lint). It re-runs by itself after every change and speaks up only when its result changes.",
            "Put it right after a check, in the same reply.",
        )
    };
    // The review step is plain text in the built-in template now; a template saved from an older
    // one still has the placeholder for it.
    template
        .trim()
        .replace("{chain}", chain)
        .replace("{watch}", watch)
        .replace("{finish}", gate)
        .replace("{review}", SELF_CHECK)
}

/// A Sage template with its placeholders filled in for the mechanisms that are switched on.
pub fn render(template: &str, features: &Features) -> String {
    let auto_watch = features.watches && features.auto_watch;
    fill(template, auto_watch, features.preflight)
}

/// The project's instruction file, if it has one: its name and its text, cut to `MAX_PROJECT_CHARS`.
pub fn project_instructions(root: &Path) -> Option<(&'static str, String)> {
    PROJECT_FILES.iter().find_map(|name| {
        let text = std::fs::read_to_string(root.join(name)).ok()?;
        let text = text.trim();
        let cut = match text.char_indices().nth(MAX_PROJECT_CHARS) {
            None => text.to_string(),
            Some((idx, _)) => format!("{}\n\n[Cut here: these are the first {MAX_PROJECT_CHARS} of {} characters. Read {name} for the rest.]", text[..idx].trim_end(), text.chars().count()),
        };
        (!text.is_empty()).then_some((*name, cut))
    })
}

/// Said ahead of the user's instructions in every session, and not theirs to edit. Without it a
/// model reads them as part of the message it is answering.
pub const USER_NOTE: &str = "What follows is the user's own standing instructions, written ahead of time for all of their sessions. It is not part of any message in this conversation and it is not a request: apply what bears on the task at hand and disregard the rest.";

/// The same, ahead of a project's instructions file.
pub fn project_note(file: &str) -> String {
    format!(
        "What follows is this project's instructions file ({file}), the same for every session in this folder. It is not part of any message in this conversation and it is not a request: apply what bears on the task at hand and disregard the rest."
    )
}

/// The system prompt of a session: `base`, then the user's instructions, the project's, and the
/// active skills, each under its own heading. It is the cached prefix of every request, so it is
/// built once, when the session starts.
pub fn compose(
    base: &str,
    global: &str,
    project: Option<(&str, &str)>,
    skills: &[SkillInfo],
) -> String {
    let mut out = base.trim().to_string();
    let mut section = |heading: String, note: Option<&str>, text: &str| {
        if !text.trim().is_empty() {
            let note = note.map(|n| format!("{n}\n\n")).unwrap_or_default();
            out.push_str(&format!("\n\n# {heading}\n\n{note}{}", text.trim()));
        }
    };
    section("User instructions".into(), Some(USER_NOTE), global);
    if let Some((file, text)) = project {
        section(
            format!("Project instructions ({file})"),
            Some(&project_note(file)),
            text,
        );
    }
    for skill in skills {
        section(format!("Skill: {}", skill.title), None, &skill.body);
    }
    out
}

/// The built-in prompt for each combination of mechanisms it describes, indexed by `watching`: 0
/// for watches the model registers itself, 1 for automatic ones and 2 for automatic ones with a
/// preflight run.
static SAGE: LazyLock<[String; 3]> =
    LazyLock::new(|| std::array::from_fn(|i| fill(default_template(), i > 0, i == 2)));

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

/// `ask`, for a session someone is present in. Its description carries the rule of when to ask, so
/// that a system prompt of the user's own still has it.
fn ask_tool() -> Value {
    tool(
        "ask",
        "Ask the user when the request can be read in ways that lead to different work and neither the task nor the workspace settles it. Above all: asked to build something new with no language or stack named (and none in the workspace), ask which, e.g. Rust, C++, Python, TypeScript. Do not ask what you can look up or reasonably decide yourself. The run pauses; the answer is the user's next message.",
        json!({
            "question": {"type": "string", "description": "One short question."},
            "options": {
                "type": "array",
                "description": "2-5 likely answers, the one you would pick first. Leave out for an open question; the user can always answer in their own words.",
                "items": {
                    "type": "object",
                    "properties": {
                        "label": {"type": "string"},
                        "hint": {"type": "string", "description": "When it is the best pick, e.g. \"Recommended for performance\", \"Recommended to get it done quickly\"."}
                    },
                    "required": ["label"]
                }
            }
        }),
        &["question"],
    )
}

fn sage_tools(watch: bool, job: bool, ask: bool) -> Value {
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
            "Create a file (parent directories are created). An existing file you have not been shown in full needs overwrite:true; prefer `edit` for changes.",
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
    if ask {
        tools.push(ask_tool());
    }
    tools.push(tool(
        "finish",
        "Declare the task complete. Watched tests are re-run first; it is refused unless a check has passed since your last change.",
        json!({
            "summary": {"type": "string", "description": "What was done, in a few lines."},
            "unverified": {"type": "string", "description": "Only when no check is possible: the reason."}
        }),
        &["summary"],
    ));
    Value::Array(tools)
}

/// Indexed by `watch * 4 + job * 2 + ask`.
static TOOLS_SAGE: LazyLock<[Value; 8]> =
    LazyLock::new(|| std::array::from_fn(|i| sage_tools(i & 4 != 0, i & 2 != 0, i & 1 != 0)));

/// Without `ask`, and with it.
static TOOLS_CLASSIC: LazyLock<[Value; 2]> =
    LazyLock::new(|| [classic_tools(false), classic_tools(true)]);

fn classic_tools(ask: bool) -> Value {
    let mut tools = json!([
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
        tool(
            "finish",
            "Declare the task complete.",
            json!({"summary": {"type": "string", "description": "What was done, in a few lines."}}),
            &["summary"]
        ),
    ]);
    if ask {
        let list = tools.as_array_mut().expect("a list");
        list.insert(list.len() - 1, ask_tool());
    }
    tools
}

/// The built-in system prompt, with nothing of the user's or the project's in it.
pub fn system_prompt(features: &Features) -> &'static str {
    if !features.scripts {
        return SYSTEM_CLASSIC;
    }
    let watching = match (features.watches && features.auto_watch, features.preflight) {
        (false, _) => 0,
        (true, false) => 1,
        (true, true) => 2,
    };
    SAGE[watching].as_str()
}

/// The tool list for a request. `watch` is offered only where the model has to register its checks
/// itself, and `job` only once the session has a background job (`jobs`). Lengthening the list
/// re-buys the whole prompt once, which is cheaper than carrying an unused schema on every turn of
/// every session. Any tool still runs when called by name, offered or not. `ask` is offered only
/// where someone is present to answer (`interactive`).
pub fn tool_schemas(features: &Features, jobs: bool, interactive: bool) -> &'static Value {
    if !features.scripts {
        return &TOOLS_CLASSIC[usize::from(interactive)];
    }
    let watch = features.watches && !features.auto_watch;
    &TOOLS_SAGE[usize::from(watch) * 4 + usize::from(jobs) * 2 + usize::from(interactive)]
}

/// Rough size in tokens of what every request starts with: system prompt plus tool schemas.
pub fn prefix_tokens(features: &Features) -> u64 {
    crate::text::est_tokens(system_prompt(features))
        + crate::text::est_tokens(&tool_schemas(features, false, false).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;

    fn names(features: &Features, jobs: bool) -> Vec<&'static str> {
        tool_schemas(features, jobs, false)
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn every_advertised_tool_exists_and_the_prefix_stays_small() {
        let manual = Features::ecosage().without("auto_watch").unwrap();
        for features in [Features::ecosage(), manual, Features::classic()] {
            for t in tool_schemas(&features, true, true).as_array().unwrap() {
                let name = t["function"]["name"].as_str().unwrap();
                assert_eq!(Tool::from_name(name).map(|t| t.name()), Some(name));
                assert_eq!(t["function"]["parameters"]["type"], "object");
            }
        }
        // The fixed prefix is paid for on every turn of every session: keep it lean.
        let (ecosage, classic) = (
            prefix_tokens(&Features::ecosage()),
            prefix_tokens(&Features::classic()),
        );
        assert!(ecosage < 2_300, "the Ecosage prefix is ~{ecosage} tokens");
        assert!(classic < 800, "the classic prefix is ~{classic} tokens");
        assert!(
            !tool_schemas(&Features::classic(), false, false)
                .to_string()
                .contains("expect")
        );
    }

    #[test]
    fn a_tool_is_offered_only_where_it_is_needed() {
        let auto = Features::ecosage();
        let manual = Features::ecosage().without("auto_watch").unwrap();
        assert_eq!(
            names(&auto, false),
            ["shell", "read", "search", "edit", "write", "finish"]
        );
        assert_eq!(
            names(&auto, true),
            ["shell", "read", "search", "edit", "write", "job", "finish"]
        );
        assert_eq!(
            names(&manual, false),
            [
                "shell", "read", "search", "edit", "write", "watch", "finish"
            ]
        );
        assert_eq!(names(&manual, true).len(), 8);
        // The baseline is a fixed list.
        assert_eq!(
            names(&Features::classic(), false),
            names(&Features::classic(), true)
        );
        assert_eq!(names(&Features::classic(), false).len(), 7);
        // `ask` only where someone can answer, just before `finish`.
        let asking = |features: &Features| {
            tool_schemas(features, false, true)
                .as_array()
                .unwrap()
                .iter()
                .map(|t| t["function"]["name"].as_str().unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            asking(&auto),
            ["shell", "read", "search", "edit", "write", "ask", "finish"]
        );
        assert_eq!(asking(&Features::classic())[6..], ["ask", "finish"]);
    }

    #[test]
    fn the_prompt_describes_the_mechanisms_that_are_switched_on() {
        let auto = system_prompt(&Features::ecosage());
        let promoted = system_prompt(&Features::ecosage().without("preflight").unwrap());
        let manual = system_prompt(&Features::ecosage().without("auto_watch").unwrap());
        assert!(
            auto.contains("Tests are watched for you")
                && auto.contains("the task names")
                && !auto.contains("`watch`")
        );
        assert!(
            promoted.contains("Tests are watched for you") && !promoted.contains("the task names")
        );
        assert!(
            manual.contains("`watch` any check you would otherwise re-run")
                && !manual.contains("watched for you")
        );
        for prompt in [auto, promoted, manual] {
            assert!(!prompt.contains('{'), "a placeholder was left in: {prompt}");
        }
        assert!(auto.contains("Re-read your changes once (`git diff`"));
        assert_eq!(
            system_prompt(&Features::ecosage().without("scripts").unwrap()),
            SYSTEM_CLASSIC
        );
    }

    #[test]
    fn a_template_is_filled_in_where_it_has_placeholders() {
        let auto = Features::ecosage();
        let manual = Features::ecosage().without("auto_watch").unwrap();
        for placeholder in ["{chain}", "{watch}", "{finish}"] {
            assert!(
                default_template().contains(placeholder),
                "the built-in template lost {placeholder}"
            );
        }
        for features in [
            auto,
            manual,
            Features::ecosage().without("preflight").unwrap(),
        ] {
            assert_eq!(
                render(default_template(), &features),
                system_prompt(&features)
            );
        }
        // A template saved while the review step was a placeholder still gets that step, as the built-in one has it.
        assert!(default_template().contains(SELF_CHECK));
        assert_eq!(
            render("Step: {review}", &auto),
            format!("Step: {SELF_CHECK}")
        );
        assert_eq!(render("Be terse.\n", &auto), "Be terse.");
        assert_eq!(
            render("Tests: {watch} {nothing}", &manual),
            "Tests: `watch` any check you would otherwise re-run (tests, build, lint). It re-runs by itself after every change and speaks up only when its result changes. {nothing}"
        );
    }

    #[test]
    fn the_parts_of_a_system_prompt_come_in_a_fixed_order() {
        let skill = |name: &str, title: &str, body: &str| SkillInfo {
            name: name.into(),
            title: title.into(),
            description: String::new(),
            source: crate::skills::SkillSource::Builtin,
            overrides_builtin: false,
            body: body.into(),
        };
        let skills = [
            skill("security", "Security audit", "Look for injection."),
            skill("ui", "UI work", "Check it in the app."),
        ];
        let full = compose(
            "Base.\n",
            "Use tabs.\n",
            Some(("AGENTS.md", "Run make test.")),
            &skills,
        );
        let (user, project) = (USER_NOTE, project_note("AGENTS.md"));
        assert_eq!(
            full,
            format!(
                "Base.\n\n# User instructions\n\n{user}\n\nUse tabs.\n\n# Project instructions (AGENTS.md)\n\n{project}\n\nRun make test.\n\n# Skill: Security audit\n\nLook for injection.\n\n# Skill: UI work\n\nCheck it in the app."
            )
        );
        // What is said ahead of the instructions is the harness's, not text from either file.
        assert!(
            user.contains("not part of any message")
                && project.contains("AGENTS.md")
                && !full.contains("Use tabs.\n\nWhat follows")
        );
        // The same parts give the same bytes: this is the prefix the provider caches.
        assert_eq!(
            full,
            compose(
                "Base.\n",
                "Use tabs.\n",
                Some(("AGENTS.md", "Run make test.")),
                &skills
            )
        );
        assert_eq!(compose("Base.", " \n", None, &[]), "Base.");
        assert_eq!(
            compose("Base.", "", Some(("CLAUDE.md", "Hi.")), &skills[1..]),
            format!(
                "Base.\n\n# Project instructions (CLAUDE.md)\n\n{}\n\nHi.\n\n# Skill: UI work\n\nCheck it in the app.",
                project_note("CLAUDE.md")
            )
        );
    }

    #[test]
    fn project_instructions_come_from_the_first_file_that_has_any_and_are_capped() {
        let root = std::env::temp_dir().join(format!("sage-prompt-{}", crate::util::rand_hex(4)));
        std::fs::create_dir_all(root.join(".sage")).unwrap();
        assert_eq!(project_instructions(&root), None);
        std::fs::write(root.join(".sage/AGENTS.md"), "deep\n").unwrap();
        assert_eq!(
            project_instructions(&root),
            Some((".sage/AGENTS.md", "deep".to_string()))
        );
        std::fs::write(root.join("CLAUDE.md"), "claude").unwrap();
        std::fs::write(root.join("AGENTS.md"), " \n").unwrap();
        assert_eq!(
            project_instructions(&root),
            Some(("CLAUDE.md", "claude".to_string())),
            "an empty file is passed over"
        );

        // The cap counts characters, not bytes.
        std::fs::write(root.join("AGENTS.md"), "é".repeat(MAX_PROJECT_CHARS)).unwrap();
        assert_eq!(
            project_instructions(&root),
            Some(("AGENTS.md", "é".repeat(MAX_PROJECT_CHARS)))
        );
        std::fs::write(
            root.join("AGENTS.md"),
            format!("{}{}", "é".repeat(MAX_PROJECT_CHARS), "x".repeat(8_000)),
        )
        .unwrap();
        let (file, text) = project_instructions(&root).unwrap();
        assert_eq!(file, "AGENTS.md");
        assert_eq!(
            text,
            format!(
                "{}\n\n[Cut here: these are the first 12000 of 20000 characters. Read AGENTS.md for the rest.]",
                "é".repeat(MAX_PROJECT_CHARS)
            )
        );
    }

    #[test]
    fn schemas_serialise_identically_every_time() {
        let a = tool_schemas(&Features::ecosage(), false, false).to_string();
        let b = tool_schemas(&Features::ecosage(), false, false).to_string();
        assert_eq!(a, b);
        assert!(a.find("\"shell\"").unwrap() < a.find("\"finish\"").unwrap());
    }
}
