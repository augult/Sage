//! The `read` tool: files (with anchors), directories, symbol definitions, and spilled outputs.

use super::{StepOutput, str_arg};
use crate::anchors::push_anchored;
use crate::state::Session;
use crate::text::clean;
use crate::util::{fmt_bytes, fnv64};
use crate::workspace::SeenCheck;
use regex::Regex;
use serde_json::Value;
use std::path::Path;

const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_COLS: usize = 400;
const MAX_DIR_ENTRIES: usize = 200;
const MAX_SYMBOL_LINES: usize = 150;

/// Parses `120-260`, `120:260`, `120..260`, `-80` (last 80), `120` or `120+` (a window from 120).
fn parse_window(spec: &str, total: usize, window: usize) -> Result<(usize, usize), String> {
    let s = spec.trim();
    let num = |t: &str| t.trim().trim_start_matches(['L', 'l']).parse::<usize>().ok();
    let (from, to) = if let Some(n) = s.strip_prefix('-').and_then(num) {
        (total.saturating_sub(n.saturating_sub(1)).max(1), total)
    } else if let Some((a, b)) = s.split_once("..").or_else(|| s.split_once('-')).or_else(|| s.split_once(':')).or_else(|| s.split_once(',')) {
        match (num(a), num(b)) {
            (Some(a), Some(b)) => (a, b),
            _ => return Err(format!("could not read `lines` {spec:?}; use e.g. \"120-260\" or \"-80\"")),
        }
    } else if let Some(a) = num(s.trim_end_matches('+')) {
        (a, a + window - 1)
    } else {
        return Err(format!("could not read `lines` {spec:?}; use e.g. \"120-260\" or \"-80\""));
    };
    let from = from.max(1);
    if from > total {
        return Err(format!("line {from} is past the end (the file has {total} lines)"));
    }
    Ok((from, to.clamp(from, total)))
}

fn not_found(sess: &Session, abs: &Path, shown: &str) -> StepOutput {
    let mut msg = format!("no such file or directory: {shown}");
    let (Some(parent), Some(name)) = (abs.parent(), abs.file_name().and_then(|n| n.to_str())) else { return StepOutput::error(msg) };
    if !parent.is_dir() {
        let mut existing = parent;
        while !existing.is_dir() {
            match existing.parent() {
                Some(p) => existing = p,
                None => break,
            }
        }
        msg.push_str(&format!(" (the nearest existing directory is {})", sess.ws.display(existing)));
        return StepOutput::error(msg);
    }
    let lower = name.to_ascii_lowercase();
    let stem = lower.split('.').next().unwrap_or(&lower).to_string();
    let mut similar: Vec<String> = std::fs::read_dir(parent)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| {
            let l = n.to_ascii_lowercase();
            l == lower || (stem.len() >= 3 && (l.starts_with(&stem) || l.contains(&stem))) || lower.contains(l.split('.').next().unwrap_or(""))
        })
        .take(5)
        .collect();
    similar.sort();
    if !similar.is_empty() {
        msg.push_str(&format!(" — in that directory: {}", similar.join(", ")));
    }
    StepOutput::error(msg)
}

fn list_dir(sess: &Session, abs: &Path) -> StepOutput {
    let mut dirs: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    let mut total = 0usize;
    let entries = match std::fs::read_dir(abs) {
        Ok(e) => e,
        Err(e) => return StepOutput::error(format!("cannot list {}: {e}", sess.ws.display(abs))),
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else { continue };
        if name == ".git" {
            continue;
        }
        total += 1;
        match entry.metadata() {
            Ok(md) if md.is_dir() => {
                let count = std::fs::read_dir(entry.path()).map(|d| d.take(1000).count()).unwrap_or(0);
                dirs.push(format!("{name}/  ({count} items)"));
            }
            Ok(md) => files.push(format!("{name}  ({})", fmt_bytes(md.len()))),
            Err(_) => files.push(name),
        }
    }
    dirs.sort();
    files.sort();
    let all: Vec<String> = dirs.into_iter().chain(files).collect();
    let mut out = format!("{}/ · {total} entries\n", sess.ws.display(abs).trim_end_matches('/'));
    for line in all.iter().take(MAX_DIR_ENTRIES) {
        out.push_str("  ");
        out.push_str(line);
        out.push('\n');
    }
    if all.len() > MAX_DIR_ENTRIES {
        out.push_str(&format!("  … {} more (use search with a glob to list them)\n", all.len() - MAX_DIR_ENTRIES));
    }
    StepOutput::ok(out.trim_end())
}

fn read_spill(sess: &Session, handle: &str, lines_arg: Option<&str>) -> StepOutput {
    let Some(path) = sess.spill_path(handle) else { return StepOutput::error(format!("{handle:?} is not an output handle (they look like out:3)")) };
    let Ok(md) = std::fs::metadata(&path) else { return StepOutput::error(format!("no saved output {handle}")) };
    if md.len() > 4 * MAX_FILE_BYTES {
        return StepOutput::error(format!("{handle} is {}; filter it with shell tools instead: {}", fmt_bytes(md.len()), path.display()));
    }
    let text = clean(&std::fs::read(&path).unwrap_or_default());
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return StepOutput::ok(format!("{handle} · empty"));
    }
    let (from, to) = match lines_arg {
        Some(spec) => match parse_window(spec, lines.len(), sess.limits.read_lines) {
            Ok(w) => w,
            Err(e) => return StepOutput::error(e),
        },
        None => (1, lines.len().min(sess.limits.read_lines)),
    };
    let mut out = format!("{handle} · lines {from}-{to} of {}\n", lines.len());
    let mut chars = 0;
    let mut last = from;
    for ln in from..=to {
        let line = crate::util::truncate_chars(lines[ln - 1], MAX_COLS);
        chars += line.len() + 1;
        if chars > sess.limits.read_chars && ln > from {
            break;
        }
        out.push_str(&format!("{ln}|{line}\n"));
        last = ln;
    }
    if last < lines.len() {
        out.push_str(&format!("… {} more lines · next: lines \"{}-{}\"\n", lines.len() - last, last + 1, (last + sess.limits.read_lines).min(lines.len())));
    }
    StepOutput { stub: format!("[{handle} lines {from}-{last} were shown here]"), ..StepOutput::ok(out.trim_end()) }
}

/// Finds definitions of `name` and the extent of each (by braces, else by indentation).
fn find_symbol(lines: &[&str], name: &str) -> Vec<(usize, usize)> {
    let leaf = name.rsplit(['.', ':', '#']).next().unwrap_or(name);
    let n = regex::escape(leaf);
    let patterns = [
        format!(
            r"^\s*(?:(?:export|default|pub(?:\([^)]*\))?|public|private|protected|internal|static|async|abstract|final|unsafe|extern|inline|virtual|override|declare)\s+)*(?:def|class|function\*?|fn|func|struct|enum|trait|interface|type|module|namespace|const|let|var|val|object|macro_rules!|sub|proc|impl(?:<[^>]*>)?)\s+(?:\([^)]*\)\s*)?{n}\b"
        ),
        format!(r"^\s*(?:[\w:<>\[\],*&]+\s+)+\*?{n}\s*\([^;]*$"),
        format!(r"^\s*(?:async\s+|static\s+|get\s+|set\s+)*{n}\s*(?:=\s*(?:async\s*)?(?:function|\(|[\w$]+\s*=>)|\([^)]*\)\s*(?::\s*[^{{]+)?\{{\s*$|:\s*(?:async\s*)?(?:function|\())"),
    ];
    let res: Vec<Regex> = patterns.iter().filter_map(|p| Regex::new(p).ok()).collect();
    let control = Regex::new(r"^\s*(?:if|for|while|switch|return|else|catch|await|yield|throw|new)\b").unwrap();
    let mut found: Vec<(usize, usize)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if control.is_match(line) || !res.iter().any(|r| r.is_match(line)) {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let opens_brace = lines[i..lines.len().min(i + 3)].iter().any(|l| l.contains('{'));
        let mut end = i;
        if opens_brace {
            let mut depth = 0i32;
            let mut seen_open = false;
            'outer: for (j, l) in lines.iter().enumerate().skip(i).take(MAX_SYMBOL_LINES) {
                for c in l.chars() {
                    match c {
                        '{' => {
                            depth += 1;
                            seen_open = true;
                        }
                        '}' => depth -= 1,
                        _ => {}
                    }
                }
                end = j;
                if seen_open && depth <= 0 {
                    break 'outer;
                }
            }
        } else {
            for (j, l) in lines.iter().enumerate().skip(i + 1).take(MAX_SYMBOL_LINES) {
                if !l.trim().is_empty() && l.len() - l.trim_start().len() <= indent {
                    break;
                }
                end = j;
            }
            while end > i && lines[end].trim().is_empty() {
                end -= 1;
            }
        }
        if !found.iter().any(|(s, e)| i + 1 >= *s && i + 1 <= *e) {
            found.push((i + 1, end + 1));
        }
        if found.len() >= 3 {
            break;
        }
    }
    found
}

pub fn run(sess: &mut Session, args: &Value) -> StepOutput {
    let Some(path_arg) = str_arg(args, "path").map(str::trim).filter(|p| !p.is_empty()) else {
        return StepOutput::error("`path` is required");
    };
    let lines_arg = str_arg(args, "lines").map(str::trim).filter(|l| !l.is_empty()).map(String::from).or_else(|| args.get("lines").and_then(Value::as_u64).map(|n| n.to_string()));
    if path_arg.starts_with("out:") {
        return read_spill(sess, path_arg, lines_arg.as_deref());
    }
    let abs = sess.ws.resolve(path_arg);
    let shown = sess.ws.display(&abs);
    let md = match std::fs::metadata(&abs) {
        Ok(md) => md,
        Err(_) => return not_found(sess, &abs, &shown),
    };
    if md.is_dir() {
        return list_dir(sess, &abs);
    }
    if md.len() > MAX_FILE_BYTES {
        return StepOutput::error(format!("{shown} is {}; look at parts of it with search, or shell (head, tail, sed -n)", fmt_bytes(md.len())));
    }
    let bytes = match std::fs::read(&abs) {
        Ok(b) => b,
        Err(e) => return StepOutput::error(format!("cannot read {shown}: {e}")),
    };
    if bytes.iter().take(8192).any(|b| *b == 0) {
        return StepOutput::ok(format!("[binary] {shown} is a binary file ({}); inspect it with shell tools (file, xxd, strings)", fmt_bytes(md.len())));
    }
    let content = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    if total == 0 {
        return StepOutput::ok(format!("[empty] {shown} exists and is empty"));
    }
    let hash = fnv64(&bytes);
    let anchors = sess.features.anchors;
    let push = |out: &mut String, ln: usize, text: &str| {
        if anchors {
            push_anchored(out, "", ln, text, MAX_COLS);
        } else {
            out.push_str(&format!("{ln:>6}\t{}\n", crate::util::truncate_chars(text, MAX_COLS)));
        }
    };

    if let Some(symbol) = str_arg(args, "symbol").map(str::trim).filter(|s| !s.is_empty()) {
        let blocks = find_symbol(&lines, symbol);
        if blocks.is_empty() {
            return StepOutput::error(format!("no definition of `{symbol}` found in {shown}; try search for it"));
        }
        let mut out = String::new();
        for (from, to) in &blocks {
            out.push_str(&format!("{shown} · `{symbol}` · lines {from}-{to} of {total}\n"));
            for ln in *from..=*to {
                push(&mut out, ln, lines[ln - 1]);
            }
            sess.seen.mark(&abs, hash, *from, *to, sess.step);
        }
        return StepOutput { stub: format!("[read {shown} `{symbol}` — dropped from context, read again if needed]"), ..StepOutput::ok(out.trim_end()) };
    }

    let (from, mut to) = match &lines_arg {
        Some(spec) => match parse_window(spec, total, sess.limits.read_lines) {
            Ok(w) => w,
            Err(e) => return StepOutput::error(format!("{shown}: {e}")),
        },
        None if total <= sess.limits.read_whole_up_to => (1, total),
        None => (1, sess.limits.read_lines),
    };
    let mut chars = 0usize;
    for ln in from..=to {
        chars += lines[ln - 1].len().min(MAX_COLS) + 8;
        if chars > sess.limits.read_chars && ln > from {
            to = ln - 1;
            break;
        }
    }

    if sess.features.dedup {
        if let SeenCheck::AlreadyShown { step } = sess.seen.observe(&abs, hash, from, to, sess.step) {
            let msg = format!("[unchanged] {shown} lines {from}-{to} are already in your context (shown at step {step}) and the file has not changed since. Ask again if you need them re-printed.");
            return StepOutput { stub: msg.clone(), observation: msg, ..Default::default() };
        }
    }

    let mut out = if from == 1 && to == total { format!("{shown} · {total} lines\n") } else { format!("{shown} · lines {from}-{to} of {total}\n") };
    for ln in from..=to {
        push(&mut out, ln, lines[ln - 1]);
    }
    if to < total {
        out.push_str(&format!("… {} more lines · next: lines \"{}-{}\"\n", total - to, to + 1, (to + sess.limits.read_lines).min(total)));
    }
    StepOutput { stub: format!("[read {shown} lines {from}-{to} — dropped from context, read again if needed]"), ..StepOutput::ok(out.trim_end()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchors::anchor;
    use crate::state::{Features, test_session};
    use crate::tools::StepStatus;
    use serde_json::json;

    fn numbered(n: usize) -> String {
        (1..=n).map(|i| format!("line {i}\n")).collect()
    }

    #[test]
    fn small_files_come_whole_with_anchors_and_large_ones_as_a_window() {
        let mut s = test_session("read", Features::presage());
        std::fs::write(s.ws.root.join("a.txt"), "alpha\nbeta\n").unwrap();
        std::fs::write(s.ws.root.join("big.txt"), numbered(1000)).unwrap();

        let out = run(&mut s, &json!({"path": "a.txt"}));
        assert_eq!(out.observation, format!("a.txt · 2 lines\n{}|alpha\n{}|beta", anchor(1, "alpha"), anchor(2, "beta")));

        let out = run(&mut s, &json!({"path": "big.txt"}));
        assert!(out.observation.starts_with("big.txt · lines 1-250 of 1000\n"));
        assert!(out.observation.ends_with("… 750 more lines · next: lines \"251-500\""));

        let out = run(&mut s, &json!({"path": "big.txt", "lines": "-3"}));
        assert!(out.observation.starts_with("big.txt · lines 998-1000 of 1000\n"));
        let out = run(&mut s, &json!({"path": "big.txt", "lines": "400-402"}));
        assert_eq!(out.observation.lines().count(), 5, "{}", out.observation);
        assert_eq!(run(&mut s, &json!({"path": "big.txt", "lines": "5000-6000"})).status, StepStatus::Error);
    }

    #[test]
    fn rereading_an_unchanged_window_costs_one_line_until_the_file_changes() {
        let mut s = test_session("read-dedup", Features::presage());
        let p = s.ws.root.join("a.txt");
        std::fs::write(&p, numbered(20)).unwrap();
        s.step = 3;
        assert!(run(&mut s, &json!({"path": "a.txt"})).observation.contains("line 20"));
        s.step = 9;
        let again = run(&mut s, &json!({"path": "a.txt", "lines": "5-10"}));
        assert!(again.observation.starts_with("[unchanged] a.txt lines 5-10 are already in your context (shown at step 3)"));
        assert!(run(&mut s, &json!({"path": "a.txt", "lines": "5-10"})).observation.contains("line 7"), "asking twice prints it");

        std::fs::write(&p, numbered(21)).unwrap();
        assert!(run(&mut s, &json!({"path": "a.txt"})).observation.contains("line 21"));
    }

    #[test]
    fn directories_missing_paths_binaries_and_empties_are_all_legible() {
        let mut s = test_session("read-misc", Features::presage());
        std::fs::create_dir_all(s.ws.root.join("src/util")).unwrap();
        std::fs::write(s.ws.root.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(s.ws.root.join("src/blob.bin"), [0u8, 1, 2, 3]).unwrap();
        std::fs::write(s.ws.root.join("src/empty.txt"), "").unwrap();

        let dir = run(&mut s, &json!({"path": "src"}));
        assert!(dir.observation.starts_with("src/ · 4 entries\n  util/  (0 items)\n"), "{}", dir.observation);
        assert!(dir.observation.contains("main.rs  (13 B)"));

        let missing = run(&mut s, &json!({"path": "src/main.py"}));
        assert_eq!(missing.status, StepStatus::Error);
        assert!(missing.observation.contains("in that directory: main.rs"), "{}", missing.observation);
        let deep = run(&mut s, &json!({"path": "nope/deeper/x.rs"}));
        assert!(deep.observation.contains("nearest existing directory is ."), "{}", deep.observation);

        assert!(run(&mut s, &json!({"path": "src/blob.bin"})).observation.starts_with("[binary]"));
        assert!(run(&mut s, &json!({"path": "src/empty.txt"})).observation.starts_with("[empty]"));
    }

    #[test]
    fn symbol_lookup_returns_just_the_definition() {
        let mut s = test_session("read-symbol", Features::presage());
        let py = "import os\n\ndef helper(x):\n    return x\n\nclass Calc:\n    def divide(self, a, b):\n        if b == 0:\n            raise ValueError\n        return a / b\n\n    def add(self, a, b):\n        return a + b\n";
        std::fs::write(s.ws.root.join("calc.py"), py).unwrap();
        let out = run(&mut s, &json!({"path": "calc.py", "symbol": "Calc.divide"}));
        assert!(out.observation.starts_with("calc.py · `Calc.divide` · lines 7-10 of 13\n"), "{}", out.observation);
        assert!(!out.observation.contains("def add"));

        let rs = "use std::fmt;\n\npub fn parse(input: &str) -> Result<u32, String> {\n    let n = input.trim();\n    n.parse().map_err(|e| format!(\"{e}\"))\n}\n\nfn other() {}\n";
        std::fs::write(s.ws.root.join("lib.rs"), rs).unwrap();
        let out = run(&mut s, &json!({"path": "lib.rs", "symbol": "parse"}));
        assert!(out.observation.contains("lines 3-6 of 8"), "{}", out.observation);
        assert_eq!(run(&mut s, &json!({"path": "lib.rs", "symbol": "missing"})).status, StepStatus::Error);
    }

    #[test]
    fn spilled_output_can_be_reopened_by_handle() {
        let mut s = test_session("read-spill", Features::presage());
        let (handle, path) = s.next_spill();
        std::fs::write(&path, numbered(600)).unwrap();
        let out = run(&mut s, &json!({"path": handle}));
        assert!(out.observation.starts_with("out:1 · lines 1-250 of 600\n1|line 1\n"));
        let out = run(&mut s, &json!({"path": "out:1", "lines": "598-600"}));
        assert!(out.observation.ends_with("600|line 600"));
        assert_eq!(run(&mut s, &json!({"path": "out:77"})).status, StepStatus::Error);
    }

    #[test]
    fn classic_mode_prints_plain_line_numbers() {
        let mut s = test_session("read-classic", Features::classic());
        std::fs::write(s.ws.root.join("a.txt"), "alpha\n").unwrap();
        assert_eq!(run(&mut s, &json!({"path": "a.txt"})).observation, "a.txt · 1 lines\n     1\talpha");
        assert!(run(&mut s, &json!({"path": "a.txt"})).observation.contains("alpha"), "no dedup in classic mode");
    }
}
