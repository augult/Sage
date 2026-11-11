//! When output points at code (`File "x.py", line 41`, `src/a.rs:12:5`), the model's next move is
//! almost always "read that spot". Attaching the spot up front, with edit anchors, costs a few hundred
//! fresh input tokens and saves a whole turn of deliberation.

use crate::anchors::push_anchored;
use crate::util::fnv64;
use crate::workspace::{Seen, Workspace};
use regex::Regex;
use std::path::PathBuf;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loc {
    pub path: PathBuf,
    pub line: usize,
}

static PY_FRAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"File "([^"\n]+)", line (\d+)"#).unwrap());
static PATH_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:^|[\s('"\[<=])(?:file://)?((?:[A-Za-z]:)?[\w./\\@+~-]*[\w@+~-]\.[A-Za-z][A-Za-z0-9_]{0,7})(?::|\(|", line |, line )(\d{1,7})"#).unwrap()
});

const VENDORED: &[&str] = &[
    "/node_modules/", "/site-packages/", "/dist-packages/", "/.venv/", "/venv/", "/target/", "/.cargo/", "/lib/python",
    "/.rustup/", "/vendor/",
];

const MAX_SOURCE_BYTES: u64 = 1_500_000;

/// Workspace source locations mentioned in `text`, in order of first appearance.
pub fn find_locations(text: &str, ws: &Workspace) -> Vec<Loc> {
    let mut hits: Vec<(usize, Loc)> = Vec::new();
    let mut push = |pos: usize, raw: &str, line: &str| {
        let Ok(line) = line.parse::<usize>() else { return };
        if line == 0 {
            return;
        }
        let abs = ws.resolve(raw);
        let lossy = abs.to_string_lossy().replace('\\', "/");
        if VENDORED.iter().any(|v| lossy.contains(v)) || !ws.contains(&abs) {
            return;
        }
        match std::fs::metadata(&abs) {
            Ok(md) if md.is_file() && md.len() <= MAX_SOURCE_BYTES => {}
            _ => return,
        }
        let loc = Loc { path: abs, line };
        if !hits.iter().any(|(_, l)| *l == loc) {
            hits.push((pos, loc));
        }
    };
    for c in PY_FRAME.captures_iter(text) {
        push(c.get(0).unwrap().start(), &c[1], &c[2]);
    }
    for c in PATH_LINE.captures_iter(text) {
        push(c.get(1).unwrap().start(), &c[1], &c[2]);
    }
    hits.sort_by_key(|(pos, _)| *pos);
    hits.into_iter().map(|(_, l)| l).collect()
}

/// Picks the most telling locations: compilers put the root cause first, tracebacks put it last.
fn select(locs: Vec<Loc>, max: usize) -> Vec<Loc> {
    if locs.len() <= max {
        return locs;
    }
    let mut picked = vec![locs[0].clone()];
    for l in &locs[locs.len() - (max - 1)..] {
        if !picked.contains(l) {
            picked.push(l.clone());
        }
    }
    picked
}

/// Renders anchored windows around the locations found in `text`. Windows already in the model's
/// context (same file content, covered range) are skipped. Returns an empty string if nothing is new.
pub fn expand(text: &str, ws: &Workspace, seen: &mut Seen, step: u32, radius: usize, max_windows: usize, budget_chars: usize) -> String {
    let locs = select(find_locations(text, ws), max_windows);
    let mut out = String::new();
    // Merge nearby lines of one file into a single window.
    let mut windows: Vec<(PathBuf, usize, usize, Vec<usize>)> = Vec::new();
    for loc in locs {
        let (from, to) = (loc.line.saturating_sub(radius).max(1), loc.line + radius);
        match windows.iter_mut().find(|w| w.0 == loc.path && from <= w.2 + 1 && to + 1 >= w.1) {
            Some(w) => {
                w.1 = w.1.min(from);
                w.2 = w.2.max(to);
                w.3.push(loc.line);
            }
            None => windows.push((loc.path, from, to, vec![loc.line])),
        }
    }
    for (path, from, to, targets) in windows {
        let Ok(content) = std::fs::read_to_string(&path) else { continue };
        let lines: Vec<&str> = content.lines().collect();
        if from > lines.len() {
            continue;
        }
        let to = to.min(lines.len());
        let hash = fnv64(content.as_bytes());
        if seen.covered(&path, hash, from, to) {
            continue;
        }
        let mut block = format!("{}\n", ws.display(&path));
        for ln in from..=to {
            let prefix = if targets.contains(&ln) { "> " } else { "  " };
            push_anchored(&mut block, prefix, ln, lines[ln - 1], 240);
        }
        if out.len() + block.len() > budget_chars {
            break;
        }
        seen.mark(&path, hash, from, to, step);
        out.push_str(&block);
    }
    out
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn ws_with(files: &[(&str, &str)]) -> Workspace {
        let root = std::env::temp_dir().join(format!("presage-loc-{}", crate::util::rand_hex(4)));
        for (name, body) in files {
            let p = root.join(name);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, body).unwrap();
        }
        Workspace::new(&root).unwrap()
    }

    fn numbered(n: usize) -> String {
        (1..=n).map(|i| format!("line {i}\n")).collect()
    }

    #[test]
    fn finds_python_compiler_and_node_locations_inside_the_workspace_only() {
        let ws = ws_with(&[("src/calc.py", &numbered(60)), ("tests/test_calc.py", &numbered(30)), ("lib/a.rs", &numbered(40)), ("web/app.js", &numbered(20))]);
        let out = format!(
            "Traceback (most recent call last):\n  File \"{root}/tests/test_calc.py\", line 12, in test_div\n  File \"src/calc.py\", line 41, in divide\n  File \"/usr/lib/python3.12/unittest/case.py\", line 58, in run\nZeroDivisionError\nerror[E0308]: mismatched types\n  --> lib/a.rs:7:9\n    at run (web/app.js:3:15)\nversion 1.2:3 is not a path, nor is missing/file.py:9\n",
            root = ws.root.display()
        );
        let locs = find_locations(&out, &ws);
        let short: Vec<(String, usize)> = locs.iter().map(|l| (ws.display(&l.path), l.line)).collect();
        assert_eq!(
            short,
            vec![("tests/test_calc.py".to_string(), 12), ("src/calc.py".to_string(), 41), ("lib/a.rs".to_string(), 7), ("web/app.js".to_string(), 3)]
        );
    }

    #[test]
    fn locations_are_found_in_shortened_output_as_in_the_original() {
        let mut ws = ws_with(&[("src/calc.py", &numbered(60)), ("pkg/mod.py", &numbered(20)), ("top.py", &numbered(9))]);
        let root = ws.root.display().to_string();
        let out = format!("Traceback (most recent call last):\n  File \"{root}/src/calc.py\", line 41, in divide\n  File \"{root}/top.py\", line 3, in <module>\n{root}/pkg/mod.py:7: error\n");
        let short = ws.shorten(&out);
        assert_eq!(short, "Traceback (most recent call last):\n  File \"src/calc.py\", line 41, in divide\n  File \"top.py\", line 3, in <module>\npkg/mod.py:7: error\n");
        let found = find_locations(&out, &ws);
        assert_eq!(found.iter().map(|l| (ws.display(&l.path), l.line)).collect::<Vec<_>>(), vec![("src/calc.py".to_string(), 41), ("top.py".to_string(), 3), ("pkg/mod.py".to_string(), 7)]);
        assert_eq!(find_locations(&short, &ws), found);

        // After a `cd`, relative means relative to the new directory, for the output and the lookup alike.
        ws.cwd = ws.root.join("pkg");
        let short = ws.shorten(&out);
        assert_eq!(short, format!("Traceback (most recent call last):\n  File \"{root}/src/calc.py\", line 41, in divide\n  File \"{root}/top.py\", line 3, in <module>\nmod.py:7: error\n"));
        assert_eq!(find_locations(&short, &ws), found);
    }

    #[test]
    fn expand_marks_target_merges_neighbours_and_skips_what_was_shown() {
        let ws = ws_with(&[("src/calc.py", &numbered(60))]);
        let mut seen = Seen::default();
        let text = "src/calc.py:41: AssertionError\nsrc/calc.py:44: note";
        let block = expand(text, &ws, &mut seen, 9, 3, 3, 4000);
        assert!(block.starts_with("src/calc.py\n"), "{block}");
        assert_eq!(block.matches("> ").count(), 2, "{block}");
        assert_eq!(block.lines().count(), 1 + (47 - 38 + 1), "one merged window 38..=47:\n{block}");
        assert!(block.contains(&format!("> {}|line 41", crate::anchors::anchor(41, "line 41"))));

        // Same failure again: the window is already in context, so nothing is re-attached.
        assert_eq!(expand(text, &ws, &mut seen, 10, 3, 3, 4000), "");
    }

    #[test]
    fn select_keeps_first_and_last() {
        let locs: Vec<Loc> = (1..=6).map(|i| Loc { path: PathBuf::from("a"), line: i }).collect();
        let lines: Vec<usize> = select(locs, 3).into_iter().map(|l| l.line).collect();
        assert_eq!(lines, vec![1, 5, 6]);
    }
}
