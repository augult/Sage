//! The `search` tool: ripgrep's engine in-process, with results that are directly editable.

use super::{StepOutput, str_arg, u64_arg};
use crate::anchors::push_anchored;
use crate::state::Session;
use crate::workspace::Workspace;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder, Sink, SinkContext, SinkMatch};
use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use serde_json::Value;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

const MAX_PER_FILE: usize = 20;
const MAX_COUNTED: usize = 5_000;
const MAX_LISTED_FILES: usize = 200;
const TIME_BUDGET: Duration = Duration::from_secs(8);
const MAX_COLS: usize = 240;

/// The parts of a comma-separated glob list. A comma inside braces belongs to its glob: `*.{rs,toml}`.
fn glob_parts(list: &str) -> Vec<&str> {
    let (mut parts, mut depth, mut start) = (Vec::new(), 0usize, 0usize);
    for (i, c) in list.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&list[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&list[start..]);
    parts
        .into_iter()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect()
}

fn walker(root: &Path, glob: Option<&str>) -> Result<ignore::Walk, String> {
    let mut b = WalkBuilder::new(root);
    b.hidden(false)
        .follow_links(false)
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(|e| e.file_name() != ".git");
    if let Some(g) = glob {
        let mut ov = OverrideBuilder::new(root);
        for part in glob_parts(g) {
            ov.add(part)
                .map_err(|e| format!("bad glob {part:?}: {e}"))?;
        }
        b.overrides(ov.build().map_err(|e| format!("bad glob {g:?}: {e}"))?);
    }
    Ok(b.build())
}

struct FileHits {
    path: PathBuf,
    lines: Vec<(usize, bool, String)>,
    more: usize,
    partial: bool,
}

#[derive(Default)]
struct Scan {
    hits: Vec<FileHits>,
    shown: usize,
    total: usize,
    files_with_hits: usize,
    partial: bool,
}

struct SearchBudget {
    deadline: Instant,
    cancel: CancellationToken,
}

impl SearchBudget {
    fn expired(&self) -> bool {
        self.cancel.is_cancelled() || Instant::now() >= self.deadline
    }
}

struct BoundedReader<'a> {
    file: std::fs::File,
    budget: &'a SearchBudget,
}

impl Read for BoundedReader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.budget.expired() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "search budget expired",
            ));
        }
        self.file.read(bytes)
    }
}

struct FileSink {
    lines: Vec<(usize, bool, String)>,
    shown: usize,
    count: usize,
    cap: usize,
    context: usize,
    last_hit: usize,
    stop_after_context: bool,
    anchors: bool,
    partial: bool,
}

impl FileSink {
    fn save(&mut self, line: usize, matched: bool, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        let text = text.trim_end_matches(['\n', '\r']);
        let mut rendered = String::new();
        if self.anchors {
            push_anchored(&mut rendered, "", line, text, MAX_COLS);
        } else {
            rendered = format!("{line}: {}\n", crate::util::truncate_chars(text, MAX_COLS));
        }
        self.lines.push((line, matched, rendered));
    }
}

impl Sink for FileSink {
    type Error = io::Error;

    fn matched(&mut self, _: &Searcher, found: &SinkMatch<'_>) -> io::Result<bool> {
        let line = found.line_number().unwrap_or(0) as usize;
        if self.stop_after_context && self.shown == self.cap && line > self.last_hit + self.context
        {
            self.partial = true;
            return Ok(false);
        }
        self.count += 1;
        if self.shown < self.cap {
            self.save(line, true, found.bytes());
            self.shown += 1;
            self.last_hit = line;
        } else if line <= self.last_hit + self.context {
            self.save(line, false, found.bytes());
        }
        self.partial |= self.count >= 2_000
            || (self.stop_after_context && self.shown == self.cap && self.context == 0);
        Ok(!self.partial)
    }

    fn context(&mut self, _: &Searcher, found: &SinkContext<'_>) -> io::Result<bool> {
        let line = found.line_number().unwrap_or(0) as usize;
        if self.shown < self.cap || line <= self.last_hit + self.context {
            self.save(line, false, found.bytes());
        }
        if self.stop_after_context && self.shown == self.cap && line >= self.last_hit + self.context
        {
            self.partial = true;
            return Ok(false);
        }
        Ok(true)
    }
}

fn scan(
    walk: ignore::Walk,
    matcher: &RegexMatcher,
    scope: &Scope,
    context: usize,
    budget: &SearchBudget,
) -> Scan {
    let cap = scope.cap;
    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(b'\x00'))
        .line_number(true)
        .before_context(context)
        .after_context(context)
        .heap_limit(Some(1024 * 1024))
        .build();
    let mut scan = Scan::default();
    for entry in walk {
        if budget.expired() || scan.shown >= cap || scan.total >= MAX_COUNTED {
            scan.partial = true;
            break;
        }
        let Ok(entry) = entry else {
            scan.partial = true;
            continue;
        };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        // The harness's own configuration holds API keys: a search of the folder it is in does
        // not show its lines.
        if scope.holds_keys(entry.path()) {
            continue;
        }
        let room = cap - scan.shown;
        let mut sink = FileSink {
            lines: Vec::new(),
            shown: 0,
            count: 0,
            cap: room.min(MAX_PER_FILE),
            context,
            last_hit: 0,
            stop_after_context: room <= MAX_PER_FILE,
            anchors: scope.anchors,
            partial: false,
        };
        let result = std::fs::File::open(entry.path()).and_then(|file| {
            searcher.search_reader(matcher, BoundedReader { file, budget }, &mut sink)
        });
        let partial = sink.partial || result.is_err();
        scan.partial |= partial;
        if sink.shown == 0 {
            continue;
        }
        scan.total += sink.count;
        scan.shown += sink.shown;
        scan.files_with_hits += 1;
        scan.hits.push(FileHits {
            path: entry.into_path(),
            lines: sink.lines,
            more: sink.count - sink.shown,
            partial,
        });
    }
    scan
}

struct Scope {
    ws: Workspace,
    cap: usize,
    anchors: bool,
    /// The file that holds the API keys, as it really is, where it exists: never searched.
    keys: Option<PathBuf>,
}

impl Scope {
    fn of(sess: &Session) -> Self {
        Self {
            ws: sess.ws.clone(),
            cap: sess.limits.search_matches,
            anchors: sess.features.anchors,
            keys: crate::util::canonical(crate::config::data_dir().join("config.json")).ok(),
        }
    }

    /// Whether `path` is the file with the keys. Only a file of that name is looked at closely.
    fn holds_keys(&self, path: &Path) -> bool {
        self.keys.as_deref().is_some_and(|keys| {
            path.file_name() == keys.file_name()
                && crate::util::canonical(path).is_ok_and(|real| real == keys)
        })
    }
}

/// `run` on a blocking thread: a search of a large tree can take seconds, and the runtime has other
/// sessions to serve meanwhile.
pub async fn run_off_thread(
    sess: &Session,
    args: &Value,
    cancel: &CancellationToken,
) -> StepOutput {
    let (scope, args) = (Scope::of(sess), args.clone());
    let budget = SearchBudget {
        deadline: Instant::now() + sess.within_budget(TIME_BUDGET),
        cancel: cancel.clone(),
    };
    tokio::task::spawn_blocking(move || search(&scope, &args, &budget))
        .await
        .unwrap_or_else(|e| StepOutput::error(format!("the search failed: {e}")))
}

#[cfg(test)]
pub fn run(sess: &Session, args: &Value) -> StepOutput {
    search(
        &Scope::of(sess),
        args,
        &SearchBudget {
            deadline: Instant::now() + TIME_BUDGET,
            cancel: CancellationToken::new(),
        },
    )
}

fn search(scope: &Scope, args: &Value, budget: &SearchBudget) -> StepOutput {
    let ws = &scope.ws;
    let pattern = str_arg(args, "pattern").filter(|p| !p.is_empty());
    let glob = str_arg(args, "glob")
        .map(str::trim)
        .filter(|g| !g.is_empty());
    let root = ws.resolve(
        str_arg(args, "path")
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .unwrap_or("."),
    );
    if !root.exists() {
        return StepOutput::error(format!("no such file or directory: {}", ws.display(&root)));
    }
    let walk = match walker(&root, glob) {
        Ok(w) => w,
        Err(e) => return StepOutput::error(e),
    };

    let Some(pattern) = pattern else {
        if glob.is_none() {
            return StepOutput::error(
                "give a `pattern` to search file contents, or a `glob` to list files",
            );
        }
        let mut files: Vec<String> = Vec::new();
        let mut partial = false;
        for entry in walk {
            if budget.expired() || files.len() == MAX_LISTED_FILES {
                partial = true;
                break;
            }
            if let Ok(entry) = entry {
                if entry.file_type().is_some_and(|t| t.is_file()) {
                    files.push(ws.display(entry.path()));
                }
            } else {
                partial = true;
            }
        }
        if files.is_empty() && !partial {
            return StepOutput::ok(format!("[no files match {}]", glob.unwrap_or("")));
        }
        let mut out = files.join("\n");
        out.push_str(&if partial {
            format!(
                "\n[{} files shown; listing incomplete — narrow the glob or path]",
                files.len()
            )
        } else {
            format!("\n[{} files]", files.len())
        });
        return StepOutput::ok(out);
    };

    // Smart case, as in ripgrep. A pattern that is not a valid regex is searched for literally.
    let build = |literal: bool| {
        RegexMatcherBuilder::new()
            .case_smart(true)
            .fixed_strings(literal)
            .line_terminator(Some(b'\n'))
            .build(pattern)
    };
    let (matcher, mut literal) = match build(false) {
        Ok(m) => (m, false),
        Err(_) => match build(true) {
            Ok(m) => (m, true),
            Err(e) => return StepOutput::error(format!("cannot search for {pattern:?}: {e}")),
        },
    };
    let context = u64_arg(args, "context").unwrap_or(0).min(6) as usize;
    let mut found = scan(walk, &matcher, scope, context, budget);
    // Code pasted as a pattern is a valid regex that means something else: `divide(a, b)` is a
    // group, and matches no parenthesis. Before reporting that there is nothing, look for the text.
    let mut why_literal = "not a valid regex";
    if found.hits.is_empty()
        && !literal
        && !found.partial
        && regex::escape(pattern) != pattern
        && let (Ok(m), Ok(walk)) = (build(true), walker(&root, glob))
    {
        let retry = scan(walk, &m, scope, context, budget);
        if !retry.hits.is_empty() {
            (found, literal, why_literal) = (retry, true, "as a regex it matches nothing");
        } else {
            found.partial |= retry.partial;
        }
    }
    let Scan {
        hits,
        shown,
        total,
        files_with_hits,
        partial,
    } = found;

    let label = if literal {
        format!("{pattern:?} (searched literally: {why_literal})")
    } else {
        format!("/{pattern}/")
    };
    if hits.is_empty() {
        let scope = glob.map(|g| format!(" matching {g}")).unwrap_or_default();
        return StepOutput::ok(format!(
            "[no matches for {label} in {}{scope}{}]",
            ws.display(&root),
            if partial {
                " so far; search incomplete"
            } else {
                ""
            }
        ));
    }

    let mut out = String::new();
    for file in &hits {
        out.push_str(&ws.display(&file.path));
        out.push('\n');
        let mut last = 0;
        for (line, matched, text) in &file.lines {
            if context != 0 && last != 0 && *line > last + 1 {
                out.push_str("  …\n");
            }
            let prefix = if context != 0 && *matched { "> " } else { "  " };
            out.push_str(prefix);
            out.push_str(text);
            last = *line;
        }
        if file.more > 0 || file.partial {
            out.push_str(&format!(
                "  … {}{} more matches in this file\n",
                file.more,
                if file.partial { "+" } else { "" }
            ));
        }
    }
    let plural = |n: usize, word: &str| {
        if n == 1 {
            format!("1 {word}")
        } else {
            format!("{n} {word}s")
        }
    };
    let note = if literal {
        format!(" · searched literally: {why_literal}")
    } else {
        String::new()
    };
    out.push_str(&if shown < total || partial {
        format!(
            "[{shown} of {}{} matches shown, in {} — narrow with path, glob or a tighter pattern]",
            total,
            if partial { "+" } else { "" },
            plural(files_with_hits, "file")
        )
    } else {
        format!(
            "[{} in {}{note}]",
            plural(total, "match").replace("matchs", "matches"),
            plural(files_with_hits, "file")
        )
    });
    StepOutput {
        stub: format!(
            "[search {label}: {total} matches in {files_with_hits} files — dropped from context, search again if needed]"
        ),
        ..StepOutput::ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchors::anchor;
    use crate::state::{Features, test_session};
    use crate::tools::StepStatus;
    use serde_json::json;

    fn fixture(features: Features) -> Session {
        let s = test_session("search", features);
        let w = |p: &str, body: &str| {
            let path = s.ws.root.join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        };
        w(
            "src/calc.py",
            "import math\n\ndef divide(a, b):\n    return a / b\n\ndef Divide_all(xs):\n    return [divide(x, 2) for x in xs]\n",
        );
        w(
            "tests/test_calc.py",
            "from src.calc import divide\n\ndef test_divide():\n    assert divide(4, 2) == 2\n",
        );
        w("node_modules/pkg/index.js", "function divide() {}\n");
        w(".gitignore", "node_modules/\n");
        w(
            ".github/workflows/ci.yml",
            "run: pytest  # divide and conquer\n",
        );
        w("data.bin", "divide\0binary");
        std::fs::create_dir_all(s.ws.root.join(".git")).unwrap();
        std::fs::write(s.ws.root.join(".git/config"), "divide\n").unwrap();
        s
    }

    #[test]
    fn finds_matches_with_anchors_respecting_gitignore_but_not_hiding_dotfiles() {
        let s = fixture(Features::sage());
        let out = run(&s, &json!({"pattern": "divide"}));
        let o = &out.observation;
        assert!(
            o.contains(&format!(
                "src/calc.py\n  {}|def divide(a, b):\n",
                anchor(3, "def divide(a, b):")
            )),
            "{o}"
        );
        assert!(o.contains("tests/test_calc.py\n"));
        assert!(
            o.contains(".github/workflows/ci.yml\n"),
            "hidden config dirs are searched: {o}"
        );
        assert!(
            !o.contains("node_modules") && !o.contains(".git/config") && !o.contains("data.bin"),
            "{o}"
        );
        // Smart case: a lowercase pattern also finds `Divide_all`.
        assert!(o.contains("Divide_all"));
        assert!(o.ends_with("[7 matches in 3 files]"), "{o}");
    }

    #[test]
    fn code_pasted_as_a_pattern_is_found_and_brace_globs_work() {
        let s = fixture(Features::sage());
        // As a regex this is a group that matches `dividea, b`: nothing. The text is there.
        let out = run(&s, &json!({"pattern": "divide(a, b)"}));
        assert!(
            out.observation.contains("|def divide(a, b):")
                && out.observation.ends_with(
                    "[1 match in 1 file · searched literally: as a regex it matches nothing]"
                ),
            "{}",
            out.observation
        );
        // A regex that matches is left a regex, and one that finds nothing either way says so plainly.
        assert!(
            run(&s, &json!({"pattern": "divide\\(a, \\w\\)"}))
                .observation
                .ends_with("[1 match in 1 file]")
        );
        assert_eq!(
            run(&s, &json!({"pattern": "multiply(a, b)"})).observation,
            "[no matches for /multiply(a, b)/ in .]"
        );

        let out = run(&s, &json!({"glob": "**/*.{py,yml}"}));
        assert!(
            out.observation.ends_with("[3 files]") && out.observation.contains("ci.yml"),
            "{}",
            out.observation
        );
        let out = run(&s, &json!({"glob": "*.{py,yml}, *.bin"}));
        assert!(
            out.observation.ends_with("[4 files]"),
            "{}",
            out.observation
        );
        assert_eq!(glob_parts("a.{x,y}, b ,,c"), ["a.{x,y}", "b", "c"]);
    }

    #[test]
    fn smart_case_glob_path_and_context() {
        let s = fixture(Features::sage());
        let out = run(&s, &json!({"pattern": "Divide", "glob": "*.py"}));
        assert!(
            out.observation.ends_with("[1 match in 1 file]"),
            "{}",
            out.observation
        );

        let out = run(
            &s,
            &json!({"pattern": "return a / b", "path": "src", "context": 1}),
        );
        let o = &out.observation;
        assert!(
            o.contains(&format!(
                "  {}|def divide(a, b):\n> {}|    return a / b\n",
                anchor(3, "def divide(a, b):"),
                anchor(4, "    return a / b")
            )),
            "{o}"
        );

        let none = run(&s, &json!({"pattern": "zzz_not_here", "glob": "*.py"}));
        assert_eq!(none.status, StepStatus::Ok);
        assert_eq!(
            none.observation,
            "[no matches for /zzz_not_here/ in . matching *.py]"
        );
    }

    #[test]
    fn invalid_regex_falls_back_to_literal_and_glob_alone_lists_files() {
        let s = fixture(Features::sage());
        let out = run(&s, &json!({"pattern": "divide(4, 2"}));
        assert!(
            out.observation.contains("searched literally")
                && out.observation.contains("test_calc.py"),
            "{}",
            out.observation
        );

        let files = run(&s, &json!({"glob": "**/*.py"}));
        assert_eq!(
            files.observation,
            "src/calc.py\ntests/test_calc.py\n[2 files]"
        );
        assert_eq!(run(&s, &json!({})).status, StepStatus::Error);
        assert_eq!(
            run(&s, &json!({"pattern": "x", "path": "missing"})).status,
            StepStatus::Error
        );
    }

    #[test]
    fn caps_output_and_says_how_much_is_missing() {
        let s = test_session("search-cap", Features::sage());
        for f in 0..5 {
            let body: String = (0..40).map(|i| format!("needle {i}\n")).collect();
            std::fs::write(s.ws.root.join(format!("f{f}.txt")), body).unwrap();
        }
        let out = run(&s, &json!({"pattern": "needle"}));
        assert!(out.observation.contains("… 20 more matches in this file"));
        assert!(out.observation.ends_with("[60 of 100+ matches shown, in 3 files — narrow with path, glob or a tighter pattern]"), "{}", out.observation);
        assert_eq!(out.observation.matches("|needle").count(), 60);
    }

    #[test]
    fn the_file_with_the_keys_is_never_searched() {
        let s = fixture(Features::sage());
        let home = s.ws.root.join("data");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join("config.json"), "{\"key\": \"sk-secret\"}\n").unwrap();
        std::fs::write(home.join("notes.txt"), "sk-not-a-secret\n").unwrap();
        let mut scope = Scope::of(&s);
        scope.keys = crate::util::canonical(home.join("config.json")).ok();
        let budget = SearchBudget {
            deadline: Instant::now() + TIME_BUDGET,
            cancel: CancellationToken::new(),
        };
        let out = search(&scope, &json!({"pattern": "sk-", "path": "data"}), &budget);
        assert!(
            out.observation.contains("sk-not-a-secret") && !out.observation.contains("sk-secret\""),
            "{}",
            out.observation
        );
    }
}
