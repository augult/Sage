//! Mechanical checks on what the task says to produce and what it says to leave alone.
//!
//! A worker whose own tests pass can still fail the task in two plain ways: it finished without
//! writing the file the task asked for (`findings.json`, a new module or script), perhaps having
//! put it in a subfolder or summarized the answer in prose instead, or it changed a file the task
//! forbade touching (a test file, a fixture, a shared module). Both are settled from the task's
//! words and the workspace on disk, with no model request. The second is read off the task's words
//! by pattern and can hold work that is right, and no model of the stored benchmark runs broke such
//! a rule: it is checked only where asked for (`Features::protect`, the "Guard files" setting).

use crate::util::fnv64;
use crate::workspace::{Workspace, derived};
use regex::Regex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const MAX_SNAPSHOT_BYTES: u64 = 8 * 1024 * 1024;
/// Files a walk keeps at most: the protected files of one rule, or the copies of a missing
/// deliverable. Only the files a walk looks for count, so a large project is not cut short.
const MAX_WALK_FILES: usize = 5_000;
/// Entries a walk looks at, at most.
const MAX_WALK_VISITS: usize = 200_000;

const CODE_EXTS: &[&str] = &[
    "py", "js", "mjs", "cjs", "ts", "tsx", "jsx", "rs", "go", "sh", "bash", "rb", "php", "java",
    "c", "cc", "cpp", "h", "hpp",
];

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Protected {
    rel: String,
    len: u64,
    hash: u64,
}

/// What the task fixes mechanically about the workspace: files that must stay as they began, and
/// files that must exist when the run ends.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Contract {
    protected: Vec<Protected>,
    required: Vec<String>,
}

static PROHIBITION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:(?:do\s+not|don't|never)\s+(?:edit|change|modify|touch|alter|delete|remove|overwrite)\b|leave\b[^.;\n]{1,120}?\balone\b|without\s+(?:editing|changing|modifying)\b)").expect("valid regex")
});

static NO_EDIT_TESTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:do\s+not|don't|never)\s+(?:edit|change|modify|touch|alter)\s+(?:the\s+)?(?:existing\s+)?tests\b").expect("valid regex")
});

static NO_EDIT_CODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(?:do\s+not|don't|never)\s+(?:edit|change|modify|touch|alter)\s+any\s+code\b",
    )
    .expect("valid regex")
});

static WRITE_CREATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:write|create)\s+`([^`\s]+)`").expect("valid regex"));

static ADD_IN_FILE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\badd\s+(?:a\s+|an\s+|the\s+)?[a-z0-9 _-]{0,36}?\bin\s+`([^`\s]+)`")
        .expect("valid regex")
});

/// Splits `task` into sentence-sized clauses without cutting inside filenames (`app.conf`, `a.py`).
fn clauses(task: &str) -> Vec<&str> {
    let bytes = task.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        let cut = match bytes[i] {
            b'\n' | b';' => true,
            b'.' | b'!' | b'?' if i + 1 == bytes.len() => true,
            b'.' | b'!' | b'?' if bytes[i + 1].is_ascii_whitespace() => {
                let before = task[start..i].trim_end();
                let abbrev = before.ends_with("e.g") || before.ends_with("i.e");
                !abbrev
            }
            _ => false,
        };
        if cut {
            let piece = task[start..=i].trim();
            if !piece.is_empty() {
                out.push(piece);
            }
            start = i + 1;
        }
        i += 1;
    }
    if start < bytes.len() {
        let piece = task[start..].trim();
        if !piece.is_empty() {
            out.push(piece);
        }
    }
    out
}

fn bare_token(word: &str) -> &str {
    const QUOTES: [char; 7] = ['"', '\'', '“', '”', '‘', '’', '*'];
    let w = word.trim_start_matches(QUOTES).trim_end_matches(|c: char| {
        QUOTES.contains(&c) || matches!(c, '.' | ':' | '!' | '?' | '…' | ',' | ';')
    });
    w.strip_suffix("'s")
        .or_else(|| w.strip_suffix("’s"))
        .unwrap_or(w)
}

/// Candidate path tokens inside a clause: single-word backtick spans and plain words.
fn tokens(clause: &str) -> Vec<&str> {
    let parts: Vec<&str> = clause.split('`').collect();
    let mut out = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        if i % 2 == 1 && i + 1 < parts.len() {
            let span = part.trim();
            if !span.contains(char::is_whitespace) {
                out.push(bare_token(span));
            }
        } else {
            out.extend(
                part.split(|c: char| {
                    c.is_whitespace()
                        || matches!(c, '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | ',' | ';')
                })
                .map(bare_token),
            );
        }
    }
    out.retain(|w| !w.is_empty());
    out
}

fn snapshot_file(root: &Path, path: &Path) -> Option<Protected> {
    let md = std::fs::metadata(path).ok()?;
    if !md.is_file() || md.len() > MAX_SNAPSHOT_BYTES {
        return None;
    }
    let rel = path
        .strip_prefix(root)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    if rel.is_empty() || derived(&rel) {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(Protected {
        rel,
        len: bytes.len() as u64,
        hash: fnv64(&bytes),
    })
}

/// The files under `under` that `keep` takes, by their path from `root` with `/` between parts.
/// The walk goes in path order, so where a limit cuts it, it always cuts at the same files.
fn walk_files(under: &Path, root: &Path, keep: &dyn Fn(&str) -> bool) -> Vec<PathBuf> {
    let walker = ignore::WalkBuilder::new(under)
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .follow_links(false)
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(|e| {
            !e.file_name().to_str().is_some_and(|n| {
                matches!(
                    n,
                    ".git"
                        | "node_modules"
                        | "target"
                        | ".venv"
                        | "venv"
                        | "__pycache__"
                        | ".pytest_cache"
                        | ".mypy_cache"
                        | ".ruff_cache"
                )
            })
        })
        .build();
    let mut out = Vec::new();
    for entry in walker.flatten().take(MAX_WALK_VISITS) {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(root)
            .map(|rel| rel.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        if keep(&rel) {
            out.push(entry.into_path());
            if out.len() >= MAX_WALK_FILES {
                break;
            }
        }
    }
    out.sort();
    out
}

/// Words after a prohibition that end what it covers: what follows them is an exception to it
/// ("except `a.py`", "other than `a.py`", "outside `src`") or a new instruction ("…, just fix
/// `a.py`"). `other` counts only as `other than`, `apart` and `aside` only before `from`.
fn ends_prohibition(word: &str, next: Option<&str>) -> bool {
    let word = word.to_ascii_lowercase();
    let next = next.map(str::to_ascii_lowercase);
    match word.as_str() {
        "except" | "excepting" | "but" | "besides" | "outside" | "unless" | "instead"
        | "rather" | "only" | "just" | "beyond" | "fix" | "implement" | "make" | "add"
        | "write" | "create" | "update" | "refactor" | "solve" | "correct" | "repair"
        | "complete" => true,
        "other" => next.as_deref() == Some("than"),
        "apart" | "aside" => next.as_deref() == Some("from"),
        _ => false,
    }
}

fn is_test_rel(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    if lower
        .split('/')
        .any(|seg| matches!(seg, "test" | "tests" | "__tests__"))
    {
        return true;
    }
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    let stem = name.split('.').next().unwrap_or(name);
    stem.starts_with("test_")
        || stem.ends_with("_test")
        || name.contains(".test.")
        || name.contains(".spec.")
}

fn is_code_rel(rel: &str) -> bool {
    Path::new(rel)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| CODE_EXTS.iter().any(|c| ext.eq_ignore_ascii_case(c)))
}

fn valid_deliverable(ws: &Workspace, raw: &str) -> Option<String> {
    let cleaned = bare_token(raw.trim()).trim_start_matches("./");
    if cleaned.is_empty()
        || cleaned.starts_with('/')
        || cleaned.starts_with('-')
        || cleaned.ends_with('/')
        || cleaned.contains("..")
        || cleaned.contains(|c: char| {
            c.is_whitespace() || matches!(c, '*' | '?' | '[' | ']' | ':' | '\\')
        })
    {
        return None;
    }
    let path = Path::new(cleaned);
    path.extension()
        .and_then(|e| e.to_str())
        .filter(|e| !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric()))?;
    let abs = ws.resolve(cleaned);
    if !abs.starts_with(&ws.root) || abs.is_dir() {
        return None;
    }
    let rel = abs
        .strip_prefix(&ws.root)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    (!rel.is_empty() && !derived(&rel)).then_some(rel)
}

impl Contract {
    /// Reads `task` against `ws` before the session has changed anything, recording explicitly
    /// requested deliverable paths and, with `protect`, the files it says not to change.
    pub fn capture(ws: &Workspace, task: &str, protect: bool) -> Self {
        let root = &ws.root;
        let mut seen_protected = HashSet::new();
        let mut protected = Vec::new();
        let mut add_protected = |file: &Path| {
            if let Some(p) = snapshot_file(root, file)
                && seen_protected.insert(p.rel.clone())
            {
                protected.push(p);
            }
        };

        // A word of the task as a file or folder of the workspace, other than the workspace itself.
        let inside = |tok: &str| {
            let trimmed = tok.trim_end_matches('/');
            if trimmed.is_empty() {
                return None;
            }
            let real = crate::util::canonical(ws.resolve(trimmed)).ok()?;
            (real.starts_with(root) && real != *root).then_some(real)
        };
        // Files and folders the task names as exceptions to a prohibition: never protected.
        let mut exceptions: Vec<PathBuf> = Vec::new();
        let mut required = Vec::new();
        for clause in clauses(task) {
            let Some(m) = PROHIBITION.find(clause) else {
                Self::collect_required(ws, clause, &mut required);
                continue;
            };
            // A clause may say "Write `x.py`; do not edit `y.py`", or "Improve `a.html` without
            // changing `app.js`": anything ahead of the prohibition still counts for deliverables.
            Self::collect_required(ws, &clause[..m.start()], &mut required);
            if protect {
                // What the prohibition covers ends where an exception or a new instruction begins:
                // "do not edit any file except `a.py`", "don't touch the tests, just fix `a.py`".
                let after = tokens(&clause[m.end()..]);
                let cut = (0..after.len())
                    .find(|&i| ends_prohibition(after[i], after.get(i + 1).copied()))
                    .unwrap_or(after.len());
                let covered = tokens(&clause[m.start()..m.end()])
                    .into_iter()
                    .chain(after[..cut].iter().copied());
                for real in covered.filter_map(inside) {
                    if real.is_file() {
                        add_protected(&real);
                    } else if real.is_dir() {
                        for file in walk_files(&real, root, &|_| true) {
                            add_protected(&file);
                        }
                    }
                }
                exceptions.extend(after[cut..].iter().copied().filter_map(inside));
                let tail = &clause[m.start()..];
                if NO_EDIT_TESTS.is_match(tail) {
                    for file in walk_files(root, root, &is_test_rel) {
                        add_protected(&file);
                    }
                }
                if NO_EDIT_CODE.is_match(tail) {
                    for file in walk_files(root, root, &is_code_rel) {
                        add_protected(&file);
                    }
                }
            }
        }
        protected.retain(|p| {
            let path = root.join(&p.rel);
            !exceptions.iter().any(|e| path.starts_with(e))
        });
        required.retain(|rel| !protected.iter().any(|p| p.rel == *rel));
        Self {
            protected,
            required,
        }
    }

    fn collect_required(ws: &Workspace, text: &str, required: &mut Vec<String>) {
        for re in [&*WRITE_CREATE, &*ADD_IN_FILE] {
            for cap in re.captures_iter(text) {
                if let Some(rel) = valid_deliverable(ws, &cap[1])
                    && !required.contains(&rel)
                {
                    required.push(rel);
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.protected.is_empty() && self.required.is_empty()
    }

    pub fn has_required(&self) -> bool {
        !self.required.is_empty()
    }

    /// Problems with the workspace as it stands now: protected files touched, or required files
    /// missing, misplaced, empty or malformed. Empty when everything holds.
    pub fn violations(&self, root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        for p in &self.protected {
            let path = root.join(&p.rel);
            match std::fs::read(&path) {
                Err(_) => out.push(format!(
                    "`{}` was deleted, though the task says not to change it",
                    p.rel
                )),
                Ok(bytes) if bytes.len() as u64 != p.len || fnv64(&bytes) != p.hash => {
                    out.push(format!(
                        "`{}` was modified, though the task says not to change it",
                        p.rel
                    ));
                }
                Ok(_) => {}
            }
        }
        let mut walked: Option<Vec<PathBuf>> = None;
        for rel in &self.required {
            let path = root.join(rel);
            let Ok(bytes) = std::fs::read(&path) else {
                let name = Path::new(rel).file_name();
                // Every file of a deliverable's name, wherever it is: one walk for all of them.
                let files = walked.get_or_insert_with(|| {
                    let names: Vec<&std::ffi::OsStr> = self
                        .required
                        .iter()
                        .filter_map(|r| Path::new(r).file_name())
                        .collect();
                    walk_files(root, root, &|path| {
                        Path::new(path)
                            .file_name()
                            .is_some_and(|n| names.contains(&n))
                    })
                });
                let misplaced = name.and_then(|wanted| {
                    files
                        .iter()
                        .filter(|p| p.file_name() == Some(wanted))
                        .filter_map(|p| p.strip_prefix(root).ok())
                        .map(|r| r.to_string_lossy().replace('\\', "/"))
                        .find(|r| r != rel)
                });
                match misplaced {
                    Some(other) => out.push(format!("the task asks for `{rel}`, which does not exist (`{other}` exists instead — write it at `{rel}`)")),
                    None => out.push(format!("the task asks for `{rel}`, which has not been created")),
                }
                continue;
            };
            let text = String::from_utf8_lossy(&bytes);
            if rel.to_ascii_lowercase().ends_with(".json") {
                if let Err(e) = serde_json::from_str::<serde_json::Value>(&text) {
                    out.push(format!("`{rel}` is not valid JSON ({e})"));
                }
            } else if is_code_rel(rel) && text.trim().is_empty() {
                out.push(format!("`{rel}` is empty"));
            }
        }
        out
    }

    /// Formats the hold message sent back to the worker when violations are found.
    pub fn hold_message(violations: &[String]) -> String {
        let items: Vec<String> = violations.iter().map(|v| format!("- {v}")).collect();
        format!(
            "[contract] The work is not ready to finish:\n{}\n\nRestore any file the task told you to leave alone (`git checkout -- <path>` where tracked), put every requested file in place, verify the work, and call `finish` again.",
            items.join("\n")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::rand_hex;

    fn temp_ws(name: &str, files: &[(&str, &str)]) -> Workspace {
        let root = std::env::temp_dir().join(format!("sage-contract-{name}-{}", rand_hex(4)));
        std::fs::create_dir_all(&root).unwrap();
        for (rel, content) in files {
            let p = root.join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(p, content).unwrap();
        }
        Workspace::new(&root).unwrap()
    }

    #[test]
    fn protects_named_files_directories_tests_and_code_without_false_matches() {
        let ws = temp_ws(
            "protect",
            &[
                ("conf/parser.py", "x = 1\n"),
                ("SPEC.md", "# Spec\n"),
                ("examples/app.conf", "[a]\nk = 1\n"),
                ("tests/test_conf.py", "assert True\n"),
                ("tests/sub/test_more.py", "assert True\n"),
            ],
        );
        let prompt = "`conf/parser.py` is incomplete. Complete it to follow `SPEC.md`. Do not edit the files in `tests/`, `SPEC.md` or `examples/app.conf`.";
        let c = Contract::capture(&ws, prompt, true);
        assert!(c.violations(&ws.root).is_empty());

        // Changing conf/parser.py is expected; touching SPEC.md or anything under tests/ is caught.
        std::fs::write(ws.root.join("conf/parser.py"), "x = 2\n").unwrap();
        assert!(c.violations(&ws.root).is_empty());

        std::fs::write(ws.root.join("tests/sub/test_more.py"), "assert False\n").unwrap();
        std::fs::remove_file(ws.root.join("SPEC.md")).unwrap();
        let v = c.violations(&ws.root);
        assert_eq!(v.len(), 2, "{v:?}");
        assert!(v.iter().any(|m| m.contains("`SPEC.md` was deleted")));
        assert!(
            v.iter()
                .any(|m| m.contains("`tests/sub/test_more.py` was modified"))
        );
        let _ = std::fs::remove_dir_all(&ws.root);
    }

    #[test]
    fn audit_and_regression_prompts_require_deliverables_and_protect_existing_files() {
        let ws = temp_ws(
            "audit",
            &[
                ("notesvc/app.py", "def run(): pass\n"),
                ("README.md", "# Notes\n"),
            ],
        );
        let prompt = "Audit `notesvc/` for security vulnerabilities. Write `findings.json` in this folder: a JSON list. Do not change any code.";
        let c = Contract::capture(&ws, prompt, true);

        // Missing findings.json is caught even with zero edits to existing files.
        let v = c.violations(&ws.root);
        assert_eq!(
            v,
            ["the task asks for `findings.json`, which has not been created"]
        );

        // Writing it inside notesvc/ instead of the root names the misplaced copy.
        std::fs::write(ws.root.join("notesvc/findings.json"), "[]\n").unwrap();
        let v = c.violations(&ws.root);
        assert_eq!(
            v,
            [
                "the task asks for `findings.json`, which does not exist (`notesvc/findings.json` exists instead — write it at `findings.json`)"
            ]
        );

        // Invalid JSON is caught; valid JSON passes; editing code is caught.
        std::fs::write(ws.root.join("findings.json"), "{bad").unwrap();
        assert!(c.violations(&ws.root)[0].contains("not valid JSON"));
        std::fs::write(ws.root.join("findings.json"), "[]\n").unwrap();
        assert!(c.violations(&ws.root).is_empty());
        std::fs::write(ws.root.join("notesvc/app.py"), "def run(): return 1\n").unwrap();
        assert_eq!(
            c.violations(&ws.root),
            ["`notesvc/app.py` was modified, though the task says not to change it"]
        );
        let _ = std::fs::remove_dir_all(&ws.root);

        // "Add a regression test in `test_regression.py`. Do not edit the existing tests."
        let ws2 = temp_ws(
            "heisenbug",
            &[("pricing.py", "p = 1\n"), ("test_catalog.py", "assert 1\n")],
        );
        let c2 = Contract::capture(
            &ws2,
            "Fix the bug. Add a regression test in `test_regression.py` that fails now and passes after. Do not edit the existing tests.",
            true,
        );
        assert_eq!(
            c2.violations(&ws2.root),
            ["the task asks for `test_regression.py`, which has not been created"]
        );
        std::fs::write(ws2.root.join("test_regression.py"), "def test_it(): pass\n").unwrap();
        assert!(
            c2.violations(&ws2.root).is_empty(),
            "new test file is required, existing test_catalog.py is protected"
        );
        std::fs::write(ws2.root.join("test_catalog.py"), "assert 2\n").unwrap();
        assert_eq!(
            c2.violations(&ws2.root),
            ["`test_catalog.py` was modified, though the task says not to change it"]
        );
        let _ = std::fs::remove_dir_all(&ws2.root);
    }

    #[test]
    fn without_protect_only_the_files_to_write_are_checked() {
        let ws = temp_ws(
            "unprotected",
            &[("calc.py", "x = 1\n"), ("tests/test_calc.py", "assert 1\n")],
        );
        let task = "Fix `calc.py`. Write `findings.json`. Do not edit the tests or `calc.py`.";
        let c = Contract::capture(&ws, task, false);
        std::fs::write(ws.root.join("tests/test_calc.py"), "assert True\n").unwrap();
        std::fs::write(ws.root.join("calc.py"), "x = 2\n").unwrap();
        assert_eq!(
            c.violations(&ws.root),
            ["the task asks for `findings.json`, which has not been created"]
        );
        std::fs::write(ws.root.join("findings.json"), "[]\n").unwrap();
        assert!(c.violations(&ws.root).is_empty());
        // With it, the same task protects both.
        let c = Contract::capture(&ws, task, true);
        std::fs::write(ws.root.join("calc.py"), "x = 3\n").unwrap();
        assert_eq!(c.violations(&ws.root).len(), 1);
        let _ = std::fs::remove_dir_all(&ws.root);
    }

    #[test]
    fn what_the_task_names_as_an_exception_or_the_work_itself_is_not_protected() {
        let files = [
            ("calc.py", "x = 1\n"),
            ("util.py", "u = 1\n"),
            ("src/calc.py", "y = 1\n"),
            ("tests/test_calc.py", "assert 1\n"),
        ];
        // (task, the file that is the work, a file the task still protects)
        for (task, work, kept) in [
            (
                "Don't touch the tests, just fix `calc.py`.",
                "calc.py",
                Some("tests/test_calc.py"),
            ),
            (
                "Don't touch the tests, fix `calc.py` instead.",
                "calc.py",
                Some("tests/test_calc.py"),
            ),
            (
                "Fix the crash. Do not edit any file except `calc.py`.",
                "calc.py",
                None,
            ),
            (
                "Do not modify anything other than calc.py.",
                "calc.py",
                None,
            ),
            ("Do not change files outside src.", "src/calc.py", None),
            (
                "Do not change any code except `calc.py`.",
                "calc.py",
                Some("util.py"),
            ),
            (
                "Do not edit `util.py` or the tests, but fix `calc.py`.",
                "calc.py",
                Some("util.py"),
            ),
        ] {
            let ws = temp_ws("exception", &files);
            let c = Contract::capture(&ws, task, true);
            std::fs::write(ws.root.join(work), "changed\n").unwrap();
            assert!(
                c.violations(&ws.root).is_empty(),
                "{task:?}: {:?}",
                c.violations(&ws.root)
            );
            if let Some(kept) = kept {
                std::fs::write(ws.root.join(kept), "changed\n").unwrap();
                assert_eq!(
                    c.violations(&ws.root),
                    [format!(
                        "`{kept}` was modified, though the task says not to change it"
                    )],
                    "{task:?}"
                );
            }
            let _ = std::fs::remove_dir_all(&ws.root);
        }
    }

    #[test]
    fn every_test_of_a_large_project_is_protected_and_a_misplaced_file_is_found_anywhere() {
        let mut files: Vec<(String, &str)> = (0..5_000)
            .map(|i| (format!("pkg/m{i:04}.py"), "x\n"))
            .collect();
        files.extend((0..50).map(|i| (format!("pkg/test_m{i}.py"), "assert 1\n")));
        files.push(("zz/deep/findings.json".into(), "[]\n"));
        let refs: Vec<(&str, &str)> = files.iter().map(|(p, b)| (p.as_str(), *b)).collect();
        let ws = temp_ws("large", &refs);
        let c = Contract::capture(
            &ws,
            "Fix `pkg/m0001.py`. Write `findings.json`. Do not edit the tests.",
            true,
        );
        for i in 0..50 {
            std::fs::write(ws.root.join(format!("pkg/test_m{i}.py")), "assert True\n").unwrap();
        }
        let v = c.violations(&ws.root);
        assert_eq!(
            v.iter().filter(|m| m.contains("was modified")).count(),
            50,
            "{v:?}"
        );
        assert!(
            v.iter()
                .any(|m| m.contains("`zz/deep/findings.json` exists instead")),
            "{v:?}"
        );
        let _ = std::fs::remove_dir_all(&ws.root);
    }
}
