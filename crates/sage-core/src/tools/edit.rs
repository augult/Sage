//! The `edit` tool: anchored or exact-text replacement, verified against the file as it is now.

use super::{StepOutput, StepStatus, bool_arg, str_arg};
use crate::anchors::{self, Past, Splice, parse_anchor, parse_range, push_anchored};
use crate::state::Session;
use crate::util::fnv64;
use serde_json::Value;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const SNIPPET_MAX: usize = 16;

pub(crate) struct FileText {
    pub lines: Vec<String>,
    crlf: bool,
    trailing_newline: bool,
    /// The file starts with a byte-order mark, which is kept apart from its first line.
    bom: bool,
}

pub(crate) fn split(content: &str) -> FileText {
    let (bom, content) = match content.strip_prefix('\u{feff}') {
        Some(rest) => (true, rest),
        None => (false, content),
    };
    // CRLF only where every line ends so. In a file of mixed endings a `\r` stays with its line,
    // so that an edit leaves the other lines as they are.
    let crlf = content.contains("\r\n")
        && content.matches("\r\n").count() == content.matches('\n').count();
    let mut lines: Vec<String> = if content.is_empty() {
        Vec::new()
    } else {
        content
            .split('\n')
            .map(|l| {
                if crlf {
                    l.strip_suffix('\r').unwrap_or(l)
                } else {
                    l
                }
                .to_string()
            })
            .collect()
    };
    if content.ends_with('\n') {
        lines.pop();
    }
    FileText {
        lines,
        crlf,
        trailing_newline: content.ends_with('\n') || content.is_empty(),
        bom,
    }
}

fn join(ft: &FileText, lines: &[String]) -> String {
    let eol = if ft.crlf { "\r\n" } else { "\n" };
    let mut s = if ft.bom {
        String::from('\u{feff}')
    } else {
        String::new()
    };
    s.push_str(&lines.join(eol));
    if ft.trailing_newline && !lines.is_empty() {
        s.push_str(eol);
    }
    s
}

fn text_lines(text: &str) -> Vec<String> {
    split(text).lines
}

/// The lines `radius` either side of `around`, with anchors, and which lines those are.
fn nearby(lines: &[String], around: usize, radius: usize) -> (String, (usize, usize)) {
    if lines.is_empty() {
        return ("(the file is empty)\n".into(), (1, 0));
    }
    let center = around.clamp(1, lines.len());
    let (from, to) = (
        center.saturating_sub(radius).max(1),
        (center + radius).min(lines.len()),
    );
    let mut out = String::new();
    for ln in from..=to {
        push_anchored(&mut out, "  ", ln, &lines[ln - 1], 240);
    }
    (out, (from, to))
}

/// Why an edit was not made, and the line windows the refusal prints: their anchors are fresh, so
/// they count as shown.
struct Refused {
    message: String,
    windows: Vec<(usize, usize)>,
}

impl From<String> for Refused {
    fn from(message: String) -> Self {
        Refused {
            message,
            windows: Vec::new(),
        }
    }
}

impl From<&str> for Refused {
    fn from(message: &str) -> Self {
        Refused {
            message: message.to_string(),
            windows: Vec::new(),
        }
    }
}

/// An anchor that fits nowhere, with the current lines around every place it might have meant.
fn unplaced(ft: &FileText, u: anchors::Unresolved) -> Refused {
    let mut text = Vec::new();
    let mut windows: Vec<(usize, usize)> = Vec::new();
    for line in u.near {
        let (shown, window) = nearby(&ft.lines, line, 4);
        if !windows.iter().any(|w| w.0 <= window.0 && w.1 >= window.1) {
            text.push(shown);
            windows.push(window);
        }
    }
    Refused {
        message: format!(
            "{}. Nothing was changed. Current lines there:\n{}",
            u.message,
            text.join("  …\n")
        ),
        windows,
    }
}

type Applied = Result<(Vec<String>, String), Refused>;

fn apply_range(ft: &FileText, spec: &str, new: &str, past: &Past) -> Applied {
    let Some((a, b)) = parse_range(spec) else {
        return Err(format!("could not read `range` {spec:?}; use anchors from read/search output, e.g. \"41kq-47px\"").into());
    };
    let refs: Vec<&str> = ft.lines.iter().map(String::as_str).collect();
    let (s, e) = anchors::resolve_range(&refs, a, b, past).map_err(|u| unplaced(ft, u))?;
    if e.line < s.line {
        return Err(format!(
            "the range ends (line {}) before it starts (line {})",
            e.line, s.line
        )
        .into());
    }
    let repl = text_lines(new);
    let mut out = ft.lines[..s.line - 1].to_vec();
    out.extend(repl.iter().cloned());
    out.extend_from_slice(&ft.lines[e.line..]);
    let moved = if s.moved || e.moved {
        format!(
            " (the anchored text had moved; found at lines {}-{})",
            s.line, e.line
        )
    } else {
        String::new()
    };
    let what = if repl.is_empty() {
        format!("lines {}-{} deleted{moved}", s.line, e.line)
    } else {
        format!(
            "lines {}-{} replaced ({} → {} lines){moved}",
            s.line,
            e.line,
            e.line - s.line + 1,
            repl.len()
        )
    };
    Ok((out, what))
}

fn apply_after(ft: &FileText, spec: &str, new: &str, past: &Past) -> Applied {
    let repl = text_lines(new);
    if repl.is_empty() {
        return Err("`new` is empty, so there is nothing to insert".into());
    }
    let at = if matches!(spec.trim(), "0" | "top" | "start") {
        0
    } else {
        let Some(a) = parse_anchor(spec) else {
            return Err(format!("could not read `after` {spec:?}; use an anchor such as \"41kq\", or \"0\" for the top of the file").into());
        };
        let refs: Vec<&str> = ft.lines.iter().map(String::as_str).collect();
        anchors::resolve_range(&refs, a, a, past)
            .map_err(|u| unplaced(ft, u))?
            .0
            .line
    };
    let mut out = ft.lines[..at].to_vec();
    out.extend(repl.iter().cloned());
    out.extend_from_slice(&ft.lines[at..]);
    let n = repl.len();
    Ok((
        out,
        if at == 0 {
            format!("{n} lines inserted at the top")
        } else {
            format!("{n} lines inserted after line {at}")
        },
    ))
}

fn indent_of(s: &str) -> &str {
    &s[..s.len() - s.trim_start().len()]
}

/// Token overlap between two lines, for "did you mean this spot" suggestions.
fn similarity(a: &str, b: &str) -> f64 {
    let ta: Vec<&str> = a
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| !t.is_empty())
        .collect();
    let tb: Vec<&str> = b
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| !t.is_empty())
        .collect();
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    let shared = ta.iter().filter(|t| tb.contains(t)).count();
    shared as f64 / ta.len().max(tb.len()) as f64
}

fn nearby_numbered(lines: &[String], around: usize, radius: usize) -> (String, (usize, usize)) {
    if lines.is_empty() {
        return ("(the file is empty)\n".into(), (1, 0));
    }
    let center = around.clamp(1, lines.len());
    let (from, to) = (
        center.saturating_sub(radius).max(1),
        (center + radius).min(lines.len()),
    );
    let mut out = String::new();
    for ln in from..=to {
        out.push_str(&format!(
            "{ln:>6}\t{}\n",
            crate::util::truncate_chars(&lines[ln - 1], 240)
        ));
    }
    (out, (from, to))
}

fn apply_old(ft: &FileText, old: &str, new: &str, all: bool, anchors: bool) -> Applied {
    let old = old.replace("\r\n", "\n");
    let new = new.replace("\r\n", "\n");
    if old.is_empty() {
        return Err(
            "`old` is empty; give the exact text to replace, or use `range` / `after`".into(),
        );
    }
    let mut text = ft.lines.join("\n");
    if ft.trailing_newline && !ft.lines.is_empty() {
        text.push('\n');
    }
    let count = text.matches(&old).count();
    if count == 1 || (count > 1 && all) {
        let replaced = if all {
            text.replace(&old, &new)
        } else {
            text.replacen(&old, &new, 1)
        };
        let what = if count == 1 {
            "1 replacement".to_string()
        } else {
            format!("{count} replacements")
        };
        return Ok((text_lines(&replaced), what));
    }
    if count > 1 {
        let mut at: Vec<String> = Vec::new();
        let mut from = 0;
        while let Some(i) = text[from..].find(&old) {
            at.push((text[..from + i].matches('\n').count() + 1).to_string());
            from += i + old.len();
            if at.len() >= 6 {
                break;
            }
        }
        let how = if anchors {
            ", pass all:true, or use `range`"
        } else {
            " or pass all:true"
        };
        return Err(format!(
            "`old` matches {count} places (lines {}). Add surrounding text to make it unique{how}.",
            at.join(", ")
        )
        .into());
    }

    // No exact match. Models most often get whitespace wrong, so retry line-wise, ignoring first
    // trailing whitespace and then indentation, and apply only if exactly one place fits.
    let old_lines: Vec<&str> = old.trim_matches('\n').split('\n').collect();
    let window = old_lines.len();
    if window <= ft.lines.len() {
        for (relaxed, eq) in [
            (
                false,
                (|a: &str, b: &str| a.trim_end() == b.trim_end()) as fn(&str, &str) -> bool,
            ),
            (true, |a: &str, b: &str| a.trim() == b.trim()),
        ] {
            let starts: Vec<usize> = (0..=ft.lines.len() - window)
                .filter(|s| (0..window).all(|k| eq(&ft.lines[s + k], old_lines[k])))
                .collect();
            if starts.len() != 1 {
                continue;
            }
            let s = starts[0];
            let mut repl = text_lines(&new);
            if relaxed {
                let k = old_lines
                    .iter()
                    .position(|l| !l.trim().is_empty())
                    .unwrap_or(0);
                let (theirs, ours) = (
                    indent_of(old_lines[k]).to_string(),
                    indent_of(&ft.lines[s + k]).to_string(),
                );
                if theirs != ours {
                    for line in repl.iter_mut().filter(|l| !l.trim().is_empty()) {
                        *line = match line.strip_prefix(theirs.as_str()) {
                            Some(rest) => format!("{ours}{rest}"),
                            None => format!("{ours}{}", line.trim_start()),
                        };
                    }
                }
            }
            let mut out = ft.lines[..s].to_vec();
            out.extend(repl);
            out.extend_from_slice(&ft.lines[s + window..]);
            let how = if relaxed {
                "matched ignoring indentation; `new` was re-indented to fit"
            } else {
                "matched ignoring trailing whitespace"
            };
            return Ok((out, format!("1 replacement ({how})")));
        }
    }

    let probe = old_lines
        .iter()
        .find(|l| !l.trim().is_empty())
        .copied()
        .unwrap_or("");
    let best = ft
        .lines
        .iter()
        .enumerate()
        .map(|(i, l)| (similarity(probe, l), i))
        .filter(|(score, _)| *score >= 0.5)
        .max_by(|a, b| a.0.total_cmp(&b.0));
    Err(match best {
        Some((_, i)) if anchors => {
            let (shown, window) = nearby(&ft.lines, i + 1, 3);
            Refused { message: format!("`old` was not found (it must match the file exactly). The closest text is around line {}:\n{shown}Use `range` with these anchors, or copy the text exactly.", i + 1), windows: vec![window] }
        }
        Some((_, i)) => {
            let (shown, window) = nearby_numbered(&ft.lines, i + 1, 3);
            Refused { message: format!("`old` was not found (it must match the file exactly). The closest text is around line {}:\n{shown}Copy the current text exactly.", i + 1), windows: vec![window] }
        }
        None if anchors => "`old` was not found, and nothing similar either. Read the file and use `range` with its anchors.".into(),
        None => "`old` was not found, and nothing similar either. Read the file and copy the exact current text.".into(),
    })
}

/// A cheap parse of the edited file, so a broken edit is reported now rather than by a confusing
/// test failure a step later. Returns the error line (if known) and message.
pub(crate) async fn syntax_check(path: &Path, content: &str) -> Option<(Option<usize>, String)> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let run = |program: &'static str, args: Vec<String>| async move {
        let child = tokio::process::Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .ok()?;
        let out = tokio::time::timeout(Duration::from_secs(8), child.wait_with_output())
            .await
            .ok()?
            .ok()?;
        if out.status.success() {
            return None;
        }
        Some(format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ))
    };
    let p = path.to_string_lossy().into_owned();
    let line_in = |text: &str, re: &str| {
        regex::Regex::new(re)
            .ok()?
            .captures(text)?
            .get(1)?
            .as_str()
            .parse::<usize>()
            .ok()
    };
    match ext.as_str() {
        "json" => {
            // Configuration that is JSON with comments by convention.
            let name = path.file_name()?.to_str()?.to_ascii_lowercase();
            let with_comments = name.starts_with("tsconfig")
                || name.starts_with("jsconfig")
                || name == ".eslintrc.json"
                || name == "devcontainer.json"
                || path.components().any(|c| c.as_os_str() == ".vscode");
            if with_comments {
                return None;
            }
            serde_json::from_str::<Value>(content)
                .err()
                .map(|e| (Some(e.line()), e.to_string()))
        }
        "py" => {
            let script = "import ast,sys\ntry:\n    ast.parse(open(sys.argv[1],encoding='utf-8').read())\nexcept SyntaxError as e:\n    print(f'{e.lineno}:{e.msg}')\n    sys.exit(1)";
            let out = run("python3", vec!["-c".into(), script.into(), p]).await?;
            let first = out.lines().next()?;
            let (line, msg) = first.split_once(':')?;
            Some((
                line.trim().parse().ok(),
                format!("SyntaxError: {}", msg.trim()),
            ))
        }
        "js" | "mjs" | "cjs" => {
            let out = run("node", vec!["--check".into(), p]).await?;
            // JSX, or module syntax an older node will not take from a `.js` file: another dialect,
            // not a mistake.
            if out.contains("Unexpected token '<'")
                || out.contains("outside a module")
                || out.contains("Unexpected token 'export'")
            {
                return None;
            }
            let msg = out
                .lines()
                .find(|l| l.contains("Error"))
                .unwrap_or("syntax error")
                .trim()
                .to_string();
            Some((line_in(&out, r"(?m)^[^\n]*:(\d+)\s*$"), msg))
        }
        "sh" | "bash" => {
            let out = run("bash", vec!["-n".into(), p]).await?;
            let msg = out
                .lines()
                .next()
                .unwrap_or("syntax error")
                .trim()
                .to_string();
            Some((line_in(&out, r"line (\d+)"), msg))
        }
        _ => None,
    }
}

/// A syntax error that a change brought in. A file that did not parse before the tools first touched
/// it is written in something the checker does not read (a template, a dialect), and every edit to
/// it would be called a surprise: nothing is reported for such a file.
pub(crate) async fn broken_by_change(
    sess: &mut Session,
    path: &Path,
    before: Option<&str>,
    after: &str,
) -> Option<(Option<usize>, String)> {
    let Some(found) = syntax_check(path, after).await else {
        sess.dialects.insert(path.to_path_buf(), false);
        return None;
    };
    let dialect = match sess.dialects.get(path) {
        Some(known) => *known,
        None => {
            let mut already = false;
            if let (Some(before), Some(ext)) = (before, path.extension().and_then(|e| e.to_str())) {
                let scratch = sess.dir.join(format!("before-edit.{ext}"));
                if std::fs::write(&scratch, before).is_ok() {
                    already = syntax_check(&scratch, before).await.is_some();
                    let _ = std::fs::remove_file(&scratch);
                }
            }
            sess.dialects.insert(path.to_path_buf(), already);
            already
        }
    };
    (!dialect).then_some(found)
}

/// The lines that differ after an edit, with a little context and fresh anchors, and the line
/// windows actually printed (a long change is shown by its two ends).
fn snippet(before: &[String], after: &[String]) -> (String, Vec<(usize, usize)>) {
    let prefix = before
        .iter()
        .zip(after.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let max_suffix = before.len().min(after.len()) - prefix;
    let suffix = before
        .iter()
        .rev()
        .zip(after.iter().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    if after.is_empty() {
        return ("  (the file is now empty)\n".into(), Vec::new());
    }
    let from = prefix.saturating_sub(1).max(1).min(after.len());
    let to = (after.len() - suffix + 2).min(after.len()).max(from);
    let mut out = String::new();
    if to - from + 1 > SNIPPET_MAX {
        let (head_end, tail_start) = (from + SNIPPET_MAX / 2 - 1, to + 1 - SNIPPET_MAX / 2);
        for ln in from..=head_end {
            push_anchored(&mut out, "  ", ln, &after[ln - 1], 240);
        }
        out.push_str(&format!("  … {} lines …\n", to - from + 1 - SNIPPET_MAX));
        for ln in tail_start..=to {
            push_anchored(&mut out, "  ", ln, &after[ln - 1], 240);
        }
        return (out, vec![(from, head_end), (tail_start, to)]);
    }
    for ln in from..=to {
        push_anchored(&mut out, "  ", ln, &after[ln - 1], 240);
    }
    (out, vec![(from, to)])
}

pub async fn run(sess: &mut Session, args: &Value) -> StepOutput {
    let Some(path_arg) = str_arg(args, "path")
        .map(str::trim)
        .filter(|p| !p.is_empty())
    else {
        return StepOutput::error("`path` is required");
    };
    let abs = sess.ws.resolve(path_arg);
    let shown = sess.ws.display(&abs);
    if let Err(e) = sess.ws.check_writable(&abs) {
        return StepOutput {
            status: StepStatus::Denied,
            ..StepOutput::error(e)
        };
    }
    let md = match std::fs::metadata(&abs) {
        Ok(md) if md.is_file() => md,
        Ok(_) => return StepOutput::error(format!("{shown} is a directory")),
        Err(_) => {
            return StepOutput::error(format!(
                "{shown} does not exist; use write to create a new file"
            ));
        }
    };
    if md.len() > MAX_FILE_BYTES {
        return StepOutput::error(format!(
            "{shown} is too large to edit this way ({}); use a shell command",
            crate::util::fmt_bytes(md.len())
        ));
    }
    let Ok(content) = std::fs::read_to_string(&abs) else {
        return StepOutput::error(format!(
            "{shown} is not a UTF-8 text file; use a shell command to change it"
        ));
    };
    let Some(new) = str_arg(args, "new") else {
        return StepOutput::error("`new` is required: the replacement text (use \"\" to delete)");
    };
    let (range, old, after) = (
        str_arg(args, "range"),
        str_arg(args, "old"),
        str_arg(args, "after"),
    );
    let after_num = args
        .get("after")
        .and_then(Value::as_u64)
        .map(|n| n.to_string());
    let after = after.or(after_num.as_deref());
    let ft = split(&content);
    // A file changed by anything but these tools has a past the splices do not describe.
    let hash = fnv64(content.as_bytes());
    if sess.histories.get(&abs).is_some_and(|h| h.hash != hash) {
        sess.histories.remove(&abs);
    }
    let applied = {
        let history = sess.histories.get(&abs);
        let live = |line: usize| sess.seen.covered(&abs, hash, line, line);
        let frozen = |line: usize| history.is_some_and(|h| h.was_visible(line));
        let past = history.map_or(
            Past {
                splices: &[],
                seen: 0,
                shown: &live,
            },
            |h| h.past(sess.reply, &live, &frozen),
        );
        let anchors_on = sess.features.anchors;
        match (range.filter(|r| !r.trim().is_empty()), old.filter(|o| !o.is_empty()), after.filter(|a| !a.trim().is_empty())) {
            (Some(r), None, None) => apply_range(&ft, r, new, &past),
            (None, Some(o), None) => apply_old(&ft, o, new, bool_arg(args, "all"), anchors_on),
            (None, None, Some(a)) => apply_after(&ft, a, new, &past),
            (None, None, None) if anchors_on => Err("say where: give exactly one of `range` (anchors, e.g. \"41kq-47px\"), `old` (exact current text) or `after` (anchor to insert after)".into()),
            (None, None, None) => Err("say where: give `old` (exact current text to replace)".into()),
            _ => Err("give exactly one of `range`, `old`, `after` — not several".into()),
        }
    };
    let (lines, what) = match applied {
        Ok(v) => v,
        Err(refused) => {
            for (from, to) in refused.windows {
                sess.seen.mark(&abs, hash, from, to, sess.step);
            }
            return StepOutput::error(format!("{shown}: {}", refused.message));
        }
    };
    if lines == ft.lines {
        return StepOutput::ok(format!(
            "[ok] {shown}: no change — the file already has this content"
        ));
    }
    let updated = join(&ft, &lines);
    if let Err(e) = std::fs::write(&abs, &updated) {
        return StepOutput::error(format!("cannot write {shown}: {e}"));
    }
    let new_hash = fnv64(updated.as_bytes());
    let visible = sess.seen.windows(&abs, hash);
    let was_whole = !ft.lines.is_empty() && sess.seen.covered(&abs, hash, 1, ft.lines.len());
    sess.histories.entry(abs.clone()).or_default().record(
        Splice::between(&ft.lines, &lines),
        new_hash,
        sess.reply,
        visible,
    );
    if was_whole && !lines.is_empty() {
        sess.seen.mark(&abs, new_hash, 1, lines.len(), sess.step);
    }

    let broken = if sess.features.post_edit_check {
        broken_by_change(sess, &abs, Some(&content), &updated).await
    } else {
        None
    };
    let mut out = match broken {
        Some((line, msg)) => {
            // The checker was given the absolute path and may quote it back.
            let msg = sess.shown(msg);
            let around = line.map(|l| nearby(&lines, l, 3));
            if let Some((_, (from, to))) = &around {
                sess.seen.mark(&abs, new_hash, *from, *to, sess.step);
            }
            let around = around
                .map(|(text, _)| format!("\n{}", text.trim_end()))
                .unwrap_or_default();
            let at = line.map(|l| format!(" at line {l}")).unwrap_or_default();
            let head =
                format!("[SURPRISE] {shown}: {what}, but the file no longer parses{at}: {msg}");
            StepOutput {
                status: StepStatus::Surprise,
                violations: vec![format!("{shown} no longer parses: {msg}")],
                stub: head.clone(),
                observation: format!("{head}{around}"),
                ..Default::default()
            }
        }
        None => {
            let head = format!("[ok] {shown}: {what}");
            let body = if sess.features.anchors {
                // Only what is printed counts as shown: the middle of a long change is not.
                let (snip, windows) = snippet(&ft.lines, &lines);
                for (from, to) in windows {
                    sess.seen.mark(&abs, new_hash, from, to, sess.step);
                }
                format!("\n{}", snip.trim_end())
            } else {
                String::new()
            };
            StepOutput {
                stub: head.clone(),
                observation: format!("{head}{body}"),
                ..Default::default()
            }
        }
    };
    out.changed_files = true;
    out.change = Some(crate::diff::change(&shown, Some(&ft.lines), &lines));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchors::anchor;
    use crate::state::{Features, test_session};
    use serde_json::json;

    const SRC: &str = "def divide(a, b):\n    return a / b\n\ndef add(a, b):\n    return a + b\n";

    fn fixture(name: &str, body: &str) -> (Session, std::path::PathBuf) {
        let s = test_session(name, Features::sage());
        let p = s.ws.root.join("calc.txt");
        std::fs::write(&p, body).unwrap();
        (s, p)
    }

    fn at(body: &str, line: usize) -> String {
        anchor(line, body.lines().nth(line - 1).unwrap())
    }

    #[tokio::test]
    async fn replaces_a_range_by_anchor_and_returns_fresh_anchors() {
        let (mut s, p) = fixture("edit-range", SRC);
        let out = run(&mut s, &json!({"path": "calc.txt", "range": at(SRC, 2), "new": "    if b == 0:\n        raise ZeroDivisionError('b')\n    return a / b"})).await;
        assert_eq!(out.status, StepStatus::Ok, "{}", out.observation);
        assert!(out.changed_files);
        let now = std::fs::read_to_string(&p).unwrap();
        assert_eq!(
            now,
            "def divide(a, b):\n    if b == 0:\n        raise ZeroDivisionError('b')\n    return a / b\n\ndef add(a, b):\n    return a + b\n"
        );
        assert!(
            out.observation
                .starts_with("[ok] calc.txt: lines 2-2 replaced (1 → 3 lines)\n"),
            "{}",
            out.observation
        );
        // The UI's diff keeps the line that is still there and adds the two before it.
        let change = out.change.as_ref().expect("an edit carries its diff");
        assert_eq!(
            (
                change.path.as_str(),
                change.created,
                change.added,
                change.removed
            ),
            ("calc.txt", false, 2, 0)
        );
        assert_eq!(
            change.hunks[0].lines,
            [
                " def divide(a, b):",
                "+    if b == 0:",
                "+        raise ZeroDivisionError('b')",
                "     return a / b",
                " ",
                " def add(a, b):"
            ]
        );
        assert!(
            out.observation
                .contains(&format!("  {}|    if b == 0:", at(&now, 2))),
            "{}",
            out.observation
        );
    }

    #[tokio::test]
    async fn chained_edits_survive_line_shifts_because_anchors_carry_content() {
        let (mut s, p) = fixture("edit-chain", SRC);
        // Both anchors come from the same read; the first edit shifts everything below it by two lines.
        let (first, second) = (at(SRC, 1), at(SRC, 5));
        run(
            &mut s,
            &json!({"path": "calc.txt", "after": first, "new": "    # guard\n    assert b"}),
        )
        .await;
        let out = run(
            &mut s,
            &json!({"path": "calc.txt", "range": second, "new": "    return b + a"}),
        )
        .await;
        assert_eq!(out.status, StepStatus::Ok, "{}", out.observation);
        assert!(
            out.observation
                .contains("the anchored text had moved; found at lines 7-7"),
            "{}",
            out.observation
        );
        assert!(
            std::fs::read_to_string(&p)
                .unwrap()
                .ends_with("def add(a, b):\n    return b + a\n")
        );
    }

    #[tokio::test]
    async fn anchors_from_before_an_edit_of_the_same_reply_find_their_lines() {
        // A range that ends on a brace, after two lines went in above it: the end is not the first
        // brace that comes along.
        let src = "a\nb\nfn x() {\n    foo();\n}\n    bar();\n}\ntail\n";
        let (mut s, p) = fixture("edit-carry-range", src);
        let range = format!("{}-{}", at(src, 3), at(src, 7));
        run(
            &mut s,
            &json!({"path": "calc.txt", "after": "0", "new": "// one\n// two"}),
        )
        .await;
        let out = run(
            &mut s,
            &json!({"path": "calc.txt", "range": range, "new": "fn x() {}"}),
        )
        .await;
        assert_eq!(out.status, StepStatus::Ok, "{}", out.observation);
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "// one\n// two\na\nb\nfn x() {}\ntail\n"
        );

        // The old line number now holds a twin of the anchored line: it is still the other one.
        let src = "class A:\n    def a(self):\n        return None\n\n    def b(self):\n        return None\n";
        let (mut s, p) = fixture("edit-carry-twin", src);
        run(&mut s, &json!({"path": "calc.txt", "after": at(src, 1), "new": "    def z(self):\n        return 0\n\n"})).await;
        let out = run(
            &mut s,
            &json!({"path": "calc.txt", "range": at(src, 6), "new": "        return 'b'"}),
        )
        .await;
        assert!(
            out.observation.contains("found at lines 9-9"),
            "{}",
            out.observation
        );
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "class A:\n    def z(self):\n        return 0\n\n    def a(self):\n        return None\n\n    def b(self):\n        return 'b'\n"
        );

        // An anchor into lines the reply has already replaced is refused, with what is there now.
        let (mut s, p) = fixture("edit-carry-gone", SRC);
        run(
            &mut s,
            &json!({"path": "calc.txt", "range": at(SRC, 2), "new": "    return a // b"}),
        )
        .await;
        let out = run(
            &mut s,
            &json!({"path": "calc.txt", "range": at(SRC, 2), "new": "    return 0"}),
        )
        .await;
        assert_eq!(out.status, StepStatus::Error);
        assert!(
            out.observation
                .contains("an earlier edit of this reply replaced")
                && out.observation.contains("|    return a // b"),
            "{}",
            out.observation
        );
        // The anchors the refusal printed are good in the next reply.
        s.reply += 1;
        let now = std::fs::read_to_string(&p).unwrap();
        let out = run(
            &mut s,
            &json!({"path": "calc.txt", "range": at(&now, 2), "new": "    return 0"}),
        )
        .await;
        assert_eq!(out.status, StepStatus::Ok, "{}", out.observation);

        // A file changed by something else has no known past: its anchors are taken as they stand.
        std::fs::write(&p, SRC).unwrap();
        let out = run(
            &mut s,
            &json!({"path": "calc.txt", "range": at(SRC, 5), "new": "    return b + a"}),
        )
        .await;
        assert_eq!(out.status, StepStatus::Ok, "{}", out.observation);
        assert!(
            std::fs::read_to_string(&p)
                .unwrap()
                .ends_with("def add(a, b):\n    return b + a\n")
        );
    }

    #[tokio::test]
    async fn only_the_lines_a_snippet_prints_count_as_shown() {
        let body: String = (1..=60).map(|i| format!("line {i}\n")).collect();
        let (mut s, _) = fixture("edit-shown", &body);
        let new: String = (1..=40).map(|i| format!("new {i}\n")).collect();
        let out = run(&mut s, &json!({"path": "calc.txt", "range": format!("{}-{}", at(&body, 5), at(&body, 6)), "new": new})).await;
        assert!(
            out.observation.contains("\n  … "),
            "the middle is left out: {}",
            out.observation
        );
        // So it is printed when asked for, while the head of the change is known already.
        let middle =
            crate::tools::read::run(&mut s, &json!({"path": "calc.txt", "lines": "18-24"}));
        assert!(
            middle
                .observation
                .starts_with("calc.txt · lines 18-24 of 98"),
            "{}",
            middle.observation
        );
        let head = crate::tools::read::run(&mut s, &json!({"path": "calc.txt", "lines": "5-8"}));
        assert!(
            head.observation.starts_with("[unchanged]"),
            "{}",
            head.observation
        );
    }

    #[tokio::test]
    async fn stale_anchor_is_refused_with_the_current_lines() {
        let (mut s, p) = fixture("edit-stale", SRC);
        let stale = at(SRC, 2);
        std::fs::write(&p, SRC.replace("return a / b", "return a // b")).unwrap();
        let out = run(
            &mut s,
            &json!({"path": "calc.txt", "range": stale, "new": "x"}),
        )
        .await;
        assert_eq!(out.status, StepStatus::Error);
        assert!(
            out.observation.contains("no longer matches")
                && out.observation.contains("Nothing was changed"),
            "{}",
            out.observation
        );
        assert!(
            out.observation.contains("|    return a // b"),
            "shows what is there now: {}",
            out.observation
        );
        assert!(!out.changed_files);
    }

    #[tokio::test]
    async fn exact_text_mode_requires_uniqueness_and_tolerates_whitespace_slips() {
        let (mut s, p) = fixture("edit-old", SRC);
        let dup = run(
            &mut s,
            &json!({"path": "calc.txt", "old": "(a, b)", "new": "(x, y)"}),
        )
        .await;
        assert!(
            dup.observation.contains("matches 2 places (lines 1, 4)"),
            "{}",
            dup.observation
        );
        let all = run(
            &mut s,
            &json!({"path": "calc.txt", "old": "(a, b)", "new": "(x, y)", "all": true}),
        )
        .await;
        assert!(all.observation.starts_with("[ok] calc.txt: 2 replacements"));

        // `old` quoted with the wrong indentation: still exactly one place it can mean, so it is applied
        // and `new` is shifted by the same amount.
        std::fs::write(&p, SRC).unwrap();
        let out = run(&mut s, &json!({"path": "calc.txt", "old": "    def add(a, b):\n        return a + b", "new": "    def add(a, b):\n        total = a + b\n        return total"})).await;
        assert!(
            out.observation.contains("matched ignoring indentation"),
            "{}",
            out.observation
        );
        assert!(
            std::fs::read_to_string(&p)
                .unwrap()
                .ends_with("def add(a, b):\n    total = a + b\n    return total\n")
        );

        let miss = run(
            &mut s,
            &json!({"path": "calc.txt", "old": "return a * b_typo", "new": "x"}),
        )
        .await;
        assert_eq!(miss.status, StepStatus::Error);
        assert!(
            miss.observation.contains("The closest text is around line"),
            "{}",
            miss.observation
        );
    }

    #[tokio::test]
    async fn preserves_crlf_and_missing_final_newline() {
        let (mut s, p) = fixture("edit-eol", "one\r\ntwo\r\nthree");
        run(
            &mut s,
            &json!({"path": "calc.txt", "old": "two", "new": "2"}),
        )
        .await;
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "one\r\n2\r\nthree");

        // Mixed endings stay as they are line by line, and a byte-order mark stays in front.
        let (mut s, p) = fixture("edit-mixed", "a\r\nb\nc\n");
        run(&mut s, &json!({"path": "calc.txt", "old": "b", "new": "B"})).await;
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "a\r\nB\nc\n");
        let body = "\u{feff}first\nsecond\n";
        let (mut s, p) = fixture("edit-bom", body);
        let shown = crate::tools::read::run(&mut s, &json!({"path": "calc.txt"})).observation;
        assert!(
            shown.contains(&format!("{}|first\n", anchor(1, "first"))),
            "{shown}"
        );
        run(
            &mut s,
            &json!({"path": "calc.txt", "range": anchor(1, "first"), "new": "FIRST"}),
        )
        .await;
        run(
            &mut s,
            &json!({"path": "calc.txt", "after": "0", "new": "zero"}),
        )
        .await;
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "\u{feff}zero\nFIRST\nsecond\n"
        );
    }

    #[tokio::test]
    async fn deletes_inserts_at_top_and_rejects_ambiguous_requests() {
        let (mut s, p) = fixture("edit-misc", SRC);
        let out = run(&mut s, &json!({"path": "calc.txt", "range": format!("{}-{}", at(SRC, 3), at(SRC, 5)), "new": ""})).await;
        assert!(
            out.observation
                .starts_with("[ok] calc.txt: lines 3-5 deleted"),
            "{}",
            out.observation
        );
        run(
            &mut s,
            &json!({"path": "calc.txt", "after": "0", "new": "# header"}),
        )
        .await;
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "# header\ndef divide(a, b):\n    return a / b\n"
        );

        for bad in [
            json!({"path": "calc.txt", "new": "x"}),
            json!({"path": "calc.txt", "new": "x", "old": "a", "range": "1"}),
            json!({"path": "calc.txt", "range": "1"}),
            json!({"path": "nope.txt", "range": "1", "new": "x"}),
        ] {
            assert_eq!(run(&mut s, &bad).await.status, StepStatus::Error, "{bad}");
        }
        let same = run(
            &mut s,
            &json!({"path": "calc.txt", "old": "# header", "new": "# header"}),
        )
        .await;
        assert!(same.observation.contains("no change") && !same.changed_files);
    }

    #[tokio::test]
    async fn a_syntax_breaking_edit_is_reported_immediately() {
        let s0 = test_session("edit-syntax", Features::sage());
        let mut s = s0;
        let p = s.ws.root.join("conf.json");
        std::fs::write(&p, "{\n  \"a\": 1,\n  \"b\": 2\n}\n").unwrap();
        let out = run(
            &mut s,
            &json!({"path": "conf.json", "old": "\"b\": 2", "new": "\"b\": 2,"}),
        )
        .await;
        assert_eq!(out.status, StepStatus::Surprise);
        assert!(
            out.changed_files,
            "the edit is kept so the model can patch it"
        );
        assert!(
            out.observation.starts_with(
                "[SURPRISE] conf.json: 1 replacement, but the file no longer parses at line 4"
            ),
            "{}",
            out.observation
        );
        assert!(
            out.observation.contains("|}"),
            "shows the lines around the error"
        );

        // A checker that quotes the path it was given names the file the way the model does.
        #[cfg(unix)]
        {
            std::fs::write(s.ws.root.join("run.sh"), "if true; then\n  echo ok\nfi\n").unwrap();
            let out = run(
                &mut s,
                &json!({"path": "run.sh", "old": "fi", "new": "done"}),
            )
            .await;
            assert!(out.observation.starts_with("[SURPRISE] run.sh: 1 replacement, but the file no longer parses at line 3: run.sh: line 3: "), "{}", out.observation);
        }

        // A file the checker could not read before the edit either is in some dialect of its own
        // (comments in JSON here): an edit to it is not a surprise.
        std::fs::write(
            s.ws.root.join("settings.json"),
            "{\n  // local\n  \"a\": 1\n}\n",
        )
        .unwrap();
        let out = run(
            &mut s,
            &json!({"path": "settings.json", "old": "\"a\": 1", "new": "\"a\": 2"}),
        )
        .await;
        assert_eq!(out.status, StepStatus::Ok, "{}", out.observation);
        std::fs::write(s.ws.root.join("tsconfig.json"), "{}\n").unwrap();
        let out = run(&mut s, &json!({"path": "tsconfig.json", "old": "{}", "new": "{\n  // strict\n  \"strict\": true\n}"})).await;
        assert_eq!(out.status, StepStatus::Ok, "{}", out.observation);
        // What an edit breaks in a file that did parse is reported for as long as it stays broken.
        let out = run(
            &mut s,
            &json!({"path": "conf.json", "old": "\"a\": 1,", "new": "\"a\": 1,,"}),
        )
        .await;
        assert_eq!(
            out.status,
            StepStatus::Surprise,
            "still broken, and still said: {}",
            out.observation
        );
    }

    #[tokio::test]
    async fn refuses_to_write_outside_the_workspace() {
        let mut s = test_session("edit-outside", Features::sage());
        let out = run(
            &mut s,
            &json!({"path": "/etc/hostname", "old": "x", "new": "y"}),
        )
        .await;
        assert_eq!(out.status, StepStatus::Denied);
    }

    #[tokio::test]
    async fn classic_mode_edits_give_plain_line_numbers_and_keep_whole_files_writable() {
        let mut s = test_session("edit-classic", Features::classic());
        std::fs::write(s.ws.root.join("calc.txt"), SRC).unwrap();
        super::super::read::run(&mut s, &json!({"path": "calc.txt"}));

        // A missed `old` in classic mode prints plain numbered lines and never mentions anchors.
        let miss = run(
            &mut s,
            &json!({"path": "calc.txt", "old": "def divide(a, c):", "new": "def divide(a, b):"}),
        )
        .await;
        assert_eq!(miss.status, StepStatus::Error);
        assert!(
            miss.observation.contains("Copy the current text exactly.")
                && !miss.observation.contains("anchors"),
            "{}",
            miss.observation
        );

        // Editing a file that was read whole keeps it on record as whole, so a later `write` needs no `overwrite: true`.
        let ok = run(
            &mut s,
            &json!({"path": "calc.txt", "old": "return a / b", "new": "return a // b"}),
        )
        .await;
        assert_eq!(ok.status, StepStatus::Ok);
        let replaced =
            super::super::write::run(&mut s, &json!({"path": "calc.txt", "content": "x = 1\n"}))
                .await;
        assert_eq!(replaced.status, StepStatus::Ok, "{}", replaced.observation);
    }
}
