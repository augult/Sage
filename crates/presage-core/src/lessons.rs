//! Reflex memory: environment lessons learned mechanically, with no model call.
//!
//! When a command fails and, with no file edited in between, a related command (or the same command
//! after some setup step) succeeds, the difference is knowledge about this machine and project, not
//! about the task: "`pytest` is not on PATH here, `uv run pytest` works". Such pairs are stored per
//! project and recalled at the next session's bootstrap and whenever the same error shows up again.

use crate::util::now_ms;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LessonKind {
    /// Use `fix` instead of `failed`.
    Instead,
    /// Run `fix` before `failed`.
    First,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lesson {
    pub sig: String,
    pub failed: String,
    pub fix: String,
    pub kind: LessonKind,
    pub hits: u32,
    pub last_seen: u64,
}

impl Lesson {
    pub fn describe(&self) -> String {
        match self.kind {
            LessonKind::Instead => format!("`{}` fails here ({}) → `{}` works", self.failed, self.sig, self.fix),
            LessonKind::First => format!("`{}` fails here ({}) until you run `{}`", self.failed, self.sig, self.fix),
        }
    }
}

#[derive(Debug)]
struct OpenFailure {
    cmd: String,
    sig: String,
    age: u32,
    setup_between: Vec<String>,
}

#[derive(Debug, Default)]
pub struct LessonBook {
    path: Option<PathBuf>,
    lessons: Vec<Lesson>,
    open: Vec<OpenFailure>,
}

const MAX_OPEN: usize = 4;
const MAX_AGE: u32 = 8;
const MAX_LESSONS: usize = 200;

static ABS_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?:[A-Za-z]:)?(?:/[\w.@+~-]+){2,}/?"#).unwrap());
static HEXISH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b0x[0-9a-fA-F]+\b|\b[0-9a-f]{12,}\b").unwrap());
static DIGITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());
static ERRORISH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)error|not found|no such|cannot|can't|could not|failed|denied|unknown|invalid|missing|unable|exception|unrecognized|not installed|no module|undefined").unwrap()
});

/// A stable fingerprint of what went wrong: the last error-looking line with volatile parts masked.
pub fn signature(output: &str) -> String {
    let lines: Vec<&str> = output.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let pick = lines.iter().rev().take(40).find(|l| ERRORISH.is_match(l)).or(lines.last()).copied().unwrap_or("");
    let s = ABS_PATH.replace_all(pick, "<path>");
    let s = HEXISH.replace_all(&s, "<hex>");
    let s = DIGITS.replace_all(&s, "#");
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    crate::util::truncate_chars(&s, 140)
}

const WRAPPERS: &[&[&str]] = &[
    &["uv", "run"], &["poetry", "run"], &["pipenv", "run"], &["pdm", "run"], &["hatch", "run"], &["bundle", "exec"], &["pnpm", "exec"],
    &["npm", "exec"], &["yarn", "exec"], &["python", "-m"], &["py", "-m"], &["npx"], &["bunx"], &["pnpx"], &["sudo"], &["env"], &["time"],
    &["nohup"], &["command"], &["exec"],
];

const SETUP_PROGRAMS: &[&str] = &[
    "pip", "uv", "npm", "pnpm", "yarn", "bun", "apt", "apt-get", "brew", "cargo", "go", "gem", "bundle", "poetry", "conda", "mamba", "make",
    "cmake", "configure", "export", "source", ".", "chmod", "ln", "mkdir", "docker", "curl", "wget", "tar", "unzip", "rustup", "corepack",
    "virtualenv", "python", "nvm", "dnf", "yum", "apk", "pacman", "git",
];

/// Basename of a command word without a version suffix: `/usr/bin/python3.12` → `python`.
fn bare(word: &str) -> &str {
    let base = word.rsplit('/').next().unwrap_or(word);
    let trimmed = base.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    if trimmed.is_empty() { base } else { trimmed }
}

/// Program name and remaining arguments, with wrappers (`uv run`, `python -m`, `sudo`, `VAR=x`) and
/// version suffixes stripped so related invocations compare equal.
fn program(cmd: &str) -> (String, Vec<String>) {
    let first = cmd.split(['|', ';']).next().unwrap_or(cmd).split("&&").next().unwrap_or(cmd);
    let mut toks: Vec<&str> = first.split_whitespace().collect();
    loop {
        while toks.first().is_some_and(|t| t.contains('=') && !t.starts_with('-') && !t.starts_with('=')) {
            toks.remove(0);
        }
        let wrapper = WRAPPERS.iter().find(|w| toks.len() > w.len() && bare(toks[0]) == w[0] && toks[1..].iter().zip(w[1..].iter()).all(|(a, b)| a == b));
        match wrapper {
            Some(w) => {
                toks.drain(..w.len());
            }
            None => break,
        }
    }
    match toks.first() {
        Some(head) => (bare(head).to_string(), toks[1..].iter().map(|s| s.to_string()).collect()),
        None => (String::new(), vec![]),
    }
}

/// Two commands are attempts at the same thing if they run the same program on the same
/// subcommand/target (`pytest -q` ~ `uv run pytest -q`, but `npm test` !~ `npm install`), or run
/// identical arguments through a different program (`npm test` ~ `pnpm test`).
fn related(a: &str, b: &str) -> bool {
    let (pa, aa) = program(a);
    let (pb, ab) = program(b);
    if pa.is_empty() || pb.is_empty() {
        return false;
    }
    let subject = |args: &[String]| args.iter().find(|x| !x.starts_with('-')).cloned();
    if pa == pb { subject(&aa) == subject(&ab) } else { !aa.is_empty() && aa == ab }
}

fn is_setup(cmd: &str) -> bool {
    let (p, _) = program(cmd);
    SETUP_PROGRAMS.contains(&p.as_str()) || cmd.trim_start().starts_with("./")
}

impl LessonBook {
    pub fn in_memory() -> Self {
        Self::default()
    }

    pub fn load(path: PathBuf) -> Self {
        let lessons = std::fs::read_to_string(&path)
            .map(|text| text.lines().filter_map(|l| serde_json::from_str::<Lesson>(l).ok()).collect())
            .unwrap_or_default();
        Self { path: Some(path), lessons, open: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.lessons.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lessons.is_empty()
    }

    /// The most-confirmed lessons, for the bootstrap snapshot.
    pub fn top(&self, n: usize) -> Vec<&Lesson> {
        let mut all: Vec<&Lesson> = self.lessons.iter().collect();
        all.sort_by(|a, b| b.hits.cmp(&a.hits).then(b.last_seen.cmp(&a.last_seen)));
        all.truncate(n);
        all
    }

    /// A failure fixed by editing code says nothing about the environment: drop pending pairings.
    pub fn on_edit(&mut self) {
        self.open.clear();
    }

    /// Feed every finished shell command. Returns a lesson when one was just learned.
    pub fn on_shell(&mut self, cmd: &str, ok: bool, output: &str) -> Option<Lesson> {
        let cmd = cmd.trim();
        if cmd.is_empty() || cmd.len() > 300 {
            return None;
        }
        if !ok {
            for f in &mut self.open {
                f.age += 1;
            }
            self.open.retain(|f| f.age <= MAX_AGE && f.cmd != cmd);
            self.open.push(OpenFailure { cmd: cmd.to_string(), sig: signature(output), age: 0, setup_between: Vec::new() });
            if self.open.len() > MAX_OPEN {
                self.open.remove(0);
            }
            return None;
        }

        let mut learned = None;
        if let Some(pos) = self.open.iter().rposition(|f| f.cmd == cmd) {
            let f = self.open.remove(pos);
            if !f.setup_between.is_empty() {
                learned = Some((f.sig, f.cmd, f.setup_between.join(" && "), LessonKind::First));
            }
        } else if let Some(pos) = self.open.iter().rposition(|f| related(&f.cmd, cmd)) {
            let f = self.open.remove(pos);
            learned = Some((f.sig, f.cmd, cmd.to_string(), LessonKind::Instead));
        } else if is_setup(cmd) {
            for f in &mut self.open {
                if f.setup_between.len() < 3 {
                    f.setup_between.push(cmd.to_string());
                }
            }
        }
        for f in &mut self.open {
            f.age += 1;
        }
        self.open.retain(|f| f.age <= MAX_AGE);

        let (sig, failed, fix, kind) = learned?;
        let lesson = match self.lessons.iter_mut().find(|l| l.failed == failed && l.fix == fix) {
            Some(existing) => {
                existing.hits += 1;
                existing.last_seen = now_ms();
                existing.sig = sig;
                existing.clone()
            }
            None => {
                let l = Lesson { sig, failed, fix, kind, hits: 1, last_seen: now_ms() };
                self.lessons.push(l.clone());
                l
            }
        };
        self.save();
        Some(lesson)
    }

    /// If this failure has been seen and resolved before, say how.
    pub fn hint(&self, cmd: &str, output: &str) -> Option<String> {
        let sig = signature(output);
        let cmd = cmd.trim();
        let l = self
            .lessons
            .iter()
            .filter(|l| l.failed == cmd || (l.sig == sig && related(&l.failed, cmd)))
            .max_by_key(|l| (l.hits, l.last_seen))?;
        Some(match l.kind {
            LessonKind::Instead => format!("lesson from this project ({}×): `{}` worked where `{}` failed like this", l.hits, l.fix, l.failed),
            LessonKind::First => format!("lesson from this project ({}×): this failed until `{}` was run first", l.hits, l.fix),
        })
    }

    fn save(&mut self) {
        if self.lessons.len() > MAX_LESSONS {
            self.lessons.sort_by(|a, b| b.hits.cmp(&a.hits).then(b.last_seen.cmp(&a.last_seen)));
            self.lessons.truncate(MAX_LESSONS);
        }
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let body: String = self.lessons.iter().filter_map(|l| serde_json::to_string(l).ok()).map(|l| l + "\n").collect();
        let _ = std::fs::write(path, body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_masks_paths_numbers_and_picks_the_error_line() {
        let out = "collecting...\n/usr/bin/python3: No module named pytest\n\n";
        assert_eq!(signature(out), "<path>: No module named pytest");
        let a = signature("Error: listen EADDRINUSE: address already in use :::3000\n    at Server.setup (node:net:1897:16)");
        let b = signature("Error: listen EADDRINUSE: address already in use :::8080\n    at Server.setup (node:net:1901:3)");
        assert_eq!(a, b);
    }

    #[test]
    fn program_strips_wrappers_env_and_versions() {
        assert_eq!(program("uv run pytest -q").0, "pytest");
        assert_eq!(program("FOO=1 sudo python3.12 -m pytest tests/ -q"), ("pytest".into(), vec!["tests/".into(), "-q".into()]));
        assert_eq!(program("./node_modules/.bin/jest --ci").0, "jest");
        assert!(related("pytest -q", "python -m pytest -q"));
        assert!(related("npm test", "pnpm test"), "same arguments, different package manager");
        assert!(related("python script.py", "python3 script.py"));
        assert!(related("pytest tests/", "pytest tests/ -p no:cacheprovider"));
        assert!(!related("pytest -q", "ls -la"));
        assert!(!related("npm test", "npm install"), "a different subcommand is a setup step, not a retry");
        assert!(!related("python a.py", "python b.py"));
    }

    #[test]
    fn learns_a_replacement_command() {
        let mut book = LessonBook::in_memory();
        assert!(book.on_shell("pytest -q", false, "bash: pytest: command not found").is_none());
        assert!(book.on_shell("ls", true, "").is_none());
        let l = book.on_shell("uv run pytest -q", true, "3 passed").unwrap();
        assert_eq!((l.kind, l.failed.as_str(), l.fix.as_str()), (LessonKind::Instead, "pytest -q", "uv run pytest -q"));
        assert!(book.hint("pytest -q", "bash: pytest: command not found").unwrap().contains("uv run pytest -q"));
        assert!(book.hint("cargo build", "error: linker `cc` not found").is_none());
    }

    #[test]
    fn learns_a_prerequisite_when_the_same_command_later_passes() {
        let mut book = LessonBook::in_memory();
        book.on_shell("npm test", false, "sh: 1: jest: not found");
        book.on_shell("cat package.json", true, "{}");
        book.on_shell("npm install", true, "added 300 packages");
        let l = book.on_shell("npm test", true, "ok").unwrap();
        assert_eq!((l.kind, l.fix.as_str()), (LessonKind::First, "npm install"));
        assert!(l.describe().contains("until you run `npm install`"));
    }

    #[test]
    fn code_fixes_are_not_environment_lessons() {
        let mut book = LessonBook::in_memory();
        book.on_shell("pytest -q", false, "AssertionError: 1 != 2");
        book.on_edit();
        assert!(book.on_shell("pytest -q", true, "1 passed").is_none());
        assert!(book.is_empty());
    }

    #[test]
    fn repeated_lessons_gain_hits_and_persist() {
        let path = std::env::temp_dir().join(format!("presage-lessons-{}/lessons.jsonl", crate::util::rand_hex(4)));
        let mut book = LessonBook::load(path.clone());
        for _ in 0..2 {
            book.on_shell("python app.py", false, "python: command not found");
            book.on_shell("python3 app.py", true, "");
        }
        let reloaded = LessonBook::load(path);
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded.top(5)[0].hits, 2);
    }

    #[test]
    fn stale_failures_expire() {
        let mut book = LessonBook::in_memory();
        book.on_shell("pytest", false, "command not found");
        for i in 0..10 {
            book.on_shell(&format!("echo {i}"), true, "");
        }
        assert!(book.on_shell("uv run pytest", true, "ok").is_none(), "too long ago to be the same episode");
    }
}
