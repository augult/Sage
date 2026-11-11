//! Turn-zero environment snapshot.
//!
//! Agents routinely spend their first turns on `pwd`, `ls`, `which python`. Each of those is a round
//! of deliberation that a few milliseconds of harness time can replace. The snapshot goes into the
//! first user message (never the system prompt, which must stay byte-identical across sessions).

use crate::state::Session;
use crate::util::{date_string, now_ms};
use serde_json::Value;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

const PROBED_TOOLS: &[&str] = &[
    "python3", "python", "pip", "uv", "node", "npm", "pnpm", "yarn", "bun", "deno", "cargo", "rustc", "go", "java", "mvn", "gradle", "gcc", "g++",
    "clang", "make", "cmake", "docker", "git", "rg", "jq", "curl", "wget", "sqlite3", "ruby", "php", "dotnet",
];

const MAX_TOP_LEVEL: usize = 40;
/// A workspace of at most this many files is listed in full.
const MAX_TREE: usize = 60;

async fn sh(script: &str, cwd: &Path) -> Option<String> {
    if cfg!(not(unix)) {
        return None;
    }
    let child = tokio::process::Command::new("bash").arg("-c").arg(script).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true).spawn().ok()?;
    let out = tokio::time::timeout(Duration::from_secs(6), child.wait_with_output()).await.ok()?.ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn manifest_note(root: &Path, name: &str) -> Option<String> {
    match name {
        "package.json" => {
            let v: Value = serde_json::from_str(&std::fs::read_to_string(root.join(name)).ok()?).ok()?;
            let scripts: Vec<&str> = v.get("scripts")?.as_object()?.keys().map(String::as_str).take(12).collect();
            (!scripts.is_empty()).then(|| format!("package.json (scripts: {})", scripts.join(", ")))
        }
        "Makefile" | "makefile" | "GNUmakefile" => {
            let text = std::fs::read_to_string(root.join(name)).ok()?;
            let re = regex::Regex::new(r"(?m)^([A-Za-z0-9][A-Za-z0-9_.-]*):(?:[^=]|$)").ok()?;
            let mut targets: Vec<&str> = re.captures_iter(&text).filter_map(|c| c.get(1).map(|m| m.as_str())).filter(|t| !t.starts_with('.')).collect();
            targets.dedup();
            targets.truncate(12);
            (!targets.is_empty()).then(|| format!("{name} (targets: {})", targets.join(", ")))
        }
        _ => None,
    }
}

fn top_level(root: &Path) -> String {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let Ok(name) = entry.file_name().into_string() else { continue };
        if name == ".git" || name == ".DS_Store" {
            continue;
        }
        if entry.path().is_dir() {
            dirs.push(format!("{name}/"));
        } else {
            files.push(manifest_note(root, &name).unwrap_or(name));
        }
    }
    dirs.sort();
    files.sort();
    let all: Vec<String> = dirs.into_iter().chain(files).collect();
    if all.is_empty() {
        return "(empty directory)".to_string();
    }
    let more = all.len().saturating_sub(MAX_TOP_LEVEL);
    let mut line = all.into_iter().take(MAX_TOP_LEVEL).collect::<Vec<_>>().join("  ");
    if more > 0 {
        line.push_str(&format!("  … +{more} more"));
    }
    line
}

/// Every file of a small workspace with its length, so that the model need not list directories.
fn tree(root: &Path) -> Option<String> {
    let files = crate::workspace::small_tree(root, MAX_TREE)?;
    if files.is_empty() {
        return None;
    }
    let entry = |rel: &std::path::PathBuf| {
        let name = rel.to_string_lossy().into_owned();
        let note = (rel.components().count() == 1).then(|| manifest_note(root, &name)).flatten();
        match (note, crate::attach::measure(&root.join(rel)).and_then(|size| size.lines)) {
            (Some(note), _) => note,
            (None, Some(lines)) => format!("{name} {lines}"),
            (None, None) => name,
        }
    };
    Some(files.iter().map(entry).collect::<Vec<_>>().join("  "))
}

/// Builds the `<env>` block for the first message of a session.
pub async fn snapshot(sess: &Session) -> String {
    let root = &sess.ws.root;
    let probe = format!(
        "for t in {}; do command -v $t >/dev/null 2>&1 && echo \"+$t\" || echo \"-$t\"; done; echo \"py=$(python3 --version 2>&1 | head -1)\"; echo \"node=$(node --version 2>&1 | head -1)\"; echo \"branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)\"; echo \"dirty=$(git status --porcelain 2>/dev/null | wc -l | tr -d ' ')\"",
        PROBED_TOOLS.join(" ")
    );
    let probed = sh(&probe, root).await.unwrap_or_default();
    let (mut present, mut absent) = (Vec::new(), Vec::new());
    let (mut py, mut node, mut branch, mut dirty) = (String::new(), String::new(), String::new(), String::new());
    for line in probed.lines() {
        if let Some(t) = line.strip_prefix('+') {
            present.push(t.to_string());
        } else if let Some(t) = line.strip_prefix('-') {
            absent.push(t);
        } else if let Some(v) = line.strip_prefix("py=") {
            py = v.trim().trim_start_matches("Python ").to_string();
        } else if let Some(v) = line.strip_prefix("node=") {
            node = v.trim().trim_start_matches('v').to_string();
        } else if let Some(v) = line.strip_prefix("branch=") {
            branch = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("dirty=") {
            dirty = v.trim().to_string();
        }
    }
    let looks_like_version = |v: &str| v.chars().next().is_some_and(|c| c.is_ascii_digit());
    for t in &mut present {
        if t == "python3" && looks_like_version(&py) {
            *t = format!("python3 {py}");
        } else if t == "node" && looks_like_version(&node) {
            *t = format!("node {node}");
        }
    }

    let mut out = String::from("<env>\n");
    let git = if branch.is_empty() {
        " (not a git repository)".to_string()
    } else if dirty == "0" || dirty.is_empty() {
        format!(" (git: {branch}, clean)")
    } else {
        format!(" (git: {branch}, {dirty} uncommitted changes)")
    };
    // Without this, models `cd` to the absolute path and spell it out in every call. Where command
    // output is cut the same way, say so: a relative path in it is not what the program printed.
    let output = if sess.features.short_paths { ", as command output does" } else { "" };
    out.push_str(&format!("cwd: {}{git}; every tool already runs here, so give paths relative to it{output}\n", root.display()));
    out.push_str(&format!("os: {} {} · date: {}\n", std::env::consts::OS, std::env::consts::ARCH, date_string(now_ms() / 1000)));
    if !present.is_empty() {
        out.push_str(&format!("installed: {}\n", present.join(", ")));
    }
    // "Not installed" is the single most common cause of failed commands; say it before it bites.
    let missing: Vec<&str> = absent.into_iter().filter(|t| !matches!(*t, "python" | "pip" | "deno" | "bun" | "yarn" | "mvn" | "gradle" | "clang" | "php" | "ruby" | "dotnet" | "wget")).collect();
    if !missing.is_empty() && !present.is_empty() {
        out.push_str(&format!("not installed: {}\n", missing.join(", ")));
    }
    match sess.features.survey.then(|| tree(root)).flatten() {
        Some(tree) => out.push_str(&format!("files, with line counts: {tree}\n")),
        None => out.push_str(&format!("files: {}\n", top_level(root))),
    }
    if sess.features.lessons {
        for lesson in sess.lessons.top(5) {
            out.push_str(&format!("known here: {}\n", lesson.describe()));
        }
    }
    out.push_str("</env>");
    out
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::state::{Features, test_session};

    #[tokio::test]
    async fn snapshot_names_the_place_the_tools_the_files_and_the_lessons() {
        let mut s = test_session("bootstrap", Features::presage().without("survey").unwrap());
        std::fs::create_dir_all(s.ws.root.join("src")).unwrap();
        std::fs::write(s.ws.root.join("package.json"), r#"{"scripts": {"test": "node --test", "build": "tsc"}}"#).unwrap();
        std::fs::write(s.ws.root.join("Makefile"), "CC=gcc\nall: build\n\nbuild:\n\tcc x.c\n.PHONY: all\ntest: build\n\t./t\n").unwrap();
        std::fs::write(s.ws.root.join("AGENTS.md"), "Always run `make test` before finishing.\n").unwrap();
        s.lessons.on_shell("pytest -q", false, "bash: pytest: command not found");
        s.lessons.on_shell("uv run pytest -q", true, "ok");

        let env = snapshot(&s).await;
        assert!(env.starts_with("<env>\ncwd: "), "{env}");
        assert!(env.contains("(not a git repository); every tool already runs here, so give paths relative to it, as command output does\n"), "{env}");
        assert!(env.contains(&format!("os: {} {} · date: 20", std::env::consts::OS, std::env::consts::ARCH)));
        assert!(env.contains("installed: ") && env.contains("git"), "{env}");
        assert!(env.contains("files: src/  AGENTS.md  Makefile (targets: all, build, test)  package.json (scripts: test, build)"), "{env}");
        assert!(env.contains("known here: `pytest -q` fails here (bash: pytest: command not found) → `uv run pytest -q` works"), "{env}");
        // Project instructions are part of the system prompt, so they are not said a second time here.
        assert!(env.ends_with("</env>") && !env.contains("make test"), "{env}");

        // Where command output keeps its absolute paths, nothing is said about it.
        s.features = Features::presage().without("survey").unwrap().without("short_paths").unwrap();
        assert!(snapshot(&s).await.contains("(not a git repository); every tool already runs here, so give paths relative to it\n"));
    }
}
