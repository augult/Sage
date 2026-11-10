//! The `search` tool: ripgrep's engine in-process, with results that are directly editable.

use super::{StepOutput, str_arg, u64_arg};
use crate::anchors::push_anchored;
use crate::state::Session;
use grep_regex::RegexMatcherBuilder;
use grep_searcher::sinks::UTF8;
use grep_searcher::{BinaryDetection, SearcherBuilder};
use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const MAX_PER_FILE: usize = 20;
const MAX_COUNTED: usize = 5_000;
const MAX_LISTED_FILES: usize = 200;
const TIME_BUDGET: Duration = Duration::from_secs(8);
const MAX_COLS: usize = 240;

fn walker(root: &Path, glob: Option<&str>) -> Result<ignore::Walk, String> {
    let mut b = WalkBuilder::new(root);
    b.hidden(false).follow_links(false).sort_by_file_name(|a, b| a.cmp(b)).filter_entry(|e| e.file_name() != ".git");
    if let Some(g) = glob {
        let mut ov = OverrideBuilder::new(root);
        for part in g.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            ov.add(part).map_err(|e| format!("bad glob {part:?}: {e}"))?;
        }
        b.overrides(ov.build().map_err(|e| format!("bad glob {g:?}: {e}"))?);
    }
    Ok(b.build())
}

struct FileHits {
    path: PathBuf,
    lines: Vec<(usize, String)>,
    more: usize,
}

pub fn run(sess: &mut Session, args: &Value) -> StepOutput {
    let pattern = str_arg(args, "pattern").filter(|p| !p.is_empty());
    let glob = str_arg(args, "glob").map(str::trim).filter(|g| !g.is_empty());
    let root = sess.ws.resolve(str_arg(args, "path").map(str::trim).filter(|p| !p.is_empty()).unwrap_or("."));
    if !root.exists() {
        return StepOutput::error(format!("no such file or directory: {}", sess.ws.display(&root)));
    }
    let walk = match walker(&root, glob) {
        Ok(w) => w,
        Err(e) => return StepOutput::error(e),
    };
    let started = Instant::now();

    let Some(pattern) = pattern else {
        if glob.is_none() {
            return StepOutput::error("give a `pattern` to search file contents, or a `glob` to list files");
        }
        let mut files: Vec<String> = Vec::new();
        let mut total = 0usize;
        for entry in walk.flatten() {
            if entry.file_type().is_some_and(|t| t.is_file()) {
                total += 1;
                if files.len() < MAX_LISTED_FILES {
                    files.push(sess.ws.display(entry.path()));
                }
            }
            if started.elapsed() > TIME_BUDGET {
                break;
            }
        }
        if files.is_empty() {
            return StepOutput::ok(format!("[no files match {}]", glob.unwrap_or("")));
        }
        let mut out = files.join("\n");
        out.push_str(&if total > files.len() { format!("\n[{} of {total} files shown — narrow the glob or path]", files.len()) } else { format!("\n[{total} files]") });
        return StepOutput::ok(out);
    };

    // Smart case, as in ripgrep. A pattern that is not a valid regex is searched for literally.
    let build = |literal: bool| RegexMatcherBuilder::new().case_smart(true).fixed_strings(literal).line_terminator(Some(b'\n')).build(pattern);
    let (matcher, literal) = match build(false) {
        Ok(m) => (m, false),
        Err(_) => match build(true) {
            Ok(m) => (m, true),
            Err(e) => return StepOutput::error(format!("cannot search for {pattern:?}: {e}")),
        },
    };
    let mut searcher = SearcherBuilder::new().binary_detection(BinaryDetection::quit(b'\x00')).line_number(true).build();
    let cap = sess.limits.search_matches;
    let mut hits: Vec<FileHits> = Vec::new();
    let (mut shown, mut total, mut files_with_hits, mut timed_out) = (0usize, 0usize, 0usize, false);

    for entry in walk.flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if started.elapsed() > TIME_BUDGET {
            timed_out = true;
            break;
        }
        if total >= MAX_COUNTED {
            break;
        }
        let mut found: Vec<(usize, String)> = Vec::new();
        let mut count = 0usize;
        let _ = searcher.search_path(
            &matcher,
            entry.path(),
            UTF8(|lnum, line| {
                count += 1;
                if found.len() < MAX_PER_FILE {
                    found.push((lnum as usize, line.trim_end_matches(['\n', '\r']).to_string()));
                }
                Ok(count < 2_000)
            }),
        );
        if count == 0 {
            continue;
        }
        files_with_hits += 1;
        total += count;
        let room = cap.saturating_sub(shown);
        if room > 0 {
            found.truncate(room);
            shown += found.len();
            let more = count - found.len();
            hits.push(FileHits { path: entry.path().to_path_buf(), lines: found, more });
        }
    }

    let label = if literal { format!("{pattern:?} (searched literally: not a valid regex)") } else { format!("/{pattern}/") };
    if hits.is_empty() {
        let scope = glob.map(|g| format!(" matching {g}")).unwrap_or_default();
        return StepOutput::ok(format!("[no matches for {label} in {}{scope}]", sess.ws.display(&root)));
    }

    let context = u64_arg(args, "context").unwrap_or(0).min(6) as usize;
    let anchors = sess.features.anchors;
    let mut out = String::new();
    for file in &hits {
        out.push_str(&sess.ws.display(&file.path));
        out.push('\n');
        if context == 0 {
            for (ln, text) in &file.lines {
                if anchors {
                    push_anchored(&mut out, "  ", *ln, text, MAX_COLS);
                } else {
                    out.push_str(&format!("  {ln}: {}\n", crate::util::truncate_chars(text, MAX_COLS)));
                }
            }
        } else if let Ok(content) = std::fs::read_to_string(&file.path) {
            let all: Vec<&str> = content.lines().collect();
            let mut last_printed = 0usize;
            for (ln, _) in &file.lines {
                let from = ln.saturating_sub(context).max(last_printed + 1).max(1);
                let to = (ln + context).min(all.len());
                if last_printed > 0 && from > last_printed + 1 {
                    out.push_str("  …\n");
                }
                for l in from..=to {
                    let prefix = if file.lines.iter().any(|(m, _)| *m == l) { "> " } else { "  " };
                    if anchors {
                        push_anchored(&mut out, prefix, l, all[l - 1], MAX_COLS);
                    } else {
                        out.push_str(&format!("{prefix}{l}: {}\n", crate::util::truncate_chars(all[l - 1], MAX_COLS)));
                    }
                }
                last_printed = last_printed.max(to);
            }
        }
        if file.more > 0 {
            out.push_str(&format!("  … {} more matches in this file\n", file.more));
        }
    }
    let plural = |n: usize, word: &str| if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") };
    let note = if literal { " · searched literally: not a valid regex" } else { "" };
    out.push_str(&if shown < total || timed_out {
        format!(
            "[{shown} of {}{} matches shown, in {} — narrow with path, glob or a tighter pattern]",
            total,
            if total >= MAX_COUNTED || timed_out { "+" } else { "" },
            plural(files_with_hits, "file")
        )
    } else {
        format!("[{} in {}{note}]", plural(total, "match").replace("matchs", "matches"), plural(files_with_hits, "file"))
    });
    StepOutput { stub: format!("[search {label}: {total} matches in {files_with_hits} files — dropped from context, search again if needed]"), ..StepOutput::ok(out) }
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
        w("src/calc.py", "import math\n\ndef divide(a, b):\n    return a / b\n\ndef Divide_all(xs):\n    return [divide(x, 2) for x in xs]\n");
        w("tests/test_calc.py", "from src.calc import divide\n\ndef test_divide():\n    assert divide(4, 2) == 2\n");
        w("node_modules/pkg/index.js", "function divide() {}\n");
        w(".gitignore", "node_modules/\n");
        w(".github/workflows/ci.yml", "run: pytest  # divide and conquer\n");
        w("data.bin", "divide\0binary");
        std::fs::create_dir_all(s.ws.root.join(".git")).unwrap();
        std::fs::write(s.ws.root.join(".git/config"), "divide\n").unwrap();
        s
    }

    #[test]
    fn finds_matches_with_anchors_respecting_gitignore_but_not_hiding_dotfiles() {
        let mut s = fixture(Features::presage());
        let out = run(&mut s, &json!({"pattern": "divide"}));
        let o = &out.observation;
        assert!(o.contains(&format!("src/calc.py\n  {}|def divide(a, b):\n", anchor(3, "def divide(a, b):"))), "{o}");
        assert!(o.contains("tests/test_calc.py\n"));
        assert!(o.contains(".github/workflows/ci.yml\n"), "hidden config dirs are searched: {o}");
        assert!(!o.contains("node_modules") && !o.contains(".git/config") && !o.contains("data.bin"), "{o}");
        // Smart case: a lowercase pattern also finds `Divide_all`.
        assert!(o.contains("Divide_all"));
        assert!(o.ends_with("[7 matches in 3 files]"), "{o}");
    }

    #[test]
    fn smart_case_glob_path_and_context() {
        let mut s = fixture(Features::presage());
        let out = run(&mut s, &json!({"pattern": "Divide", "glob": "*.py"}));
        assert!(out.observation.ends_with("[1 match in 1 file]"), "{}", out.observation);

        let out = run(&mut s, &json!({"pattern": "return a / b", "path": "src", "context": 1}));
        let o = &out.observation;
        assert!(o.contains(&format!("  {}|def divide(a, b):\n> {}|    return a / b\n", anchor(3, "def divide(a, b):"), anchor(4, "    return a / b"))), "{o}");

        let none = run(&mut s, &json!({"pattern": "zzz_not_here", "glob": "*.py"}));
        assert_eq!(none.status, StepStatus::Ok);
        assert_eq!(none.observation, "[no matches for /zzz_not_here/ in . matching *.py]");
    }

    #[test]
    fn invalid_regex_falls_back_to_literal_and_glob_alone_lists_files() {
        let mut s = fixture(Features::presage());
        let out = run(&mut s, &json!({"pattern": "divide(4, 2"}));
        assert!(out.observation.contains("searched literally") && out.observation.contains("test_calc.py"), "{}", out.observation);

        let files = run(&mut s, &json!({"glob": "**/*.py"}));
        assert_eq!(files.observation, "src/calc.py\ntests/test_calc.py\n[2 files]");
        assert_eq!(run(&mut s, &json!({})).status, StepStatus::Error);
        assert_eq!(run(&mut s, &json!({"pattern": "x", "path": "missing"})).status, StepStatus::Error);
    }

    #[test]
    fn caps_output_and_says_how_much_is_missing() {
        let mut s = test_session("search-cap", Features::presage());
        for f in 0..5 {
            let body: String = (0..40).map(|i| format!("needle {i}\n")).collect();
            std::fs::write(s.ws.root.join(format!("f{f}.txt")), body).unwrap();
        }
        let out = run(&mut s, &json!({"pattern": "needle"}));
        assert!(out.observation.contains("… 20 more matches in this file"));
        assert!(out.observation.ends_with("[60 of 200 matches shown, in 5 files — narrow with path, glob or a tighter pattern]"), "{}", out.observation);
        assert_eq!(out.observation.matches("|needle").count(), 60);
    }
}
