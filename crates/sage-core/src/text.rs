//! Turning raw process output into something worth paying for.
//!
//! Every token of tool output is bought once at the cache-miss price and then re-read on every later
//! turn, so terminal noise (escape codes, progress redraws, package-manager chatter) is stripped before
//! the model sees it. Nothing is lost: the raw bytes stay on disk and are addressable as `out:N`.

/// Strips ANSI escapes and control characters, applies carriage-return/backspace overwrites the way a
/// terminal would, and trims trailing whitespace on every line.
pub fn clean(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    let mut out = String::with_capacity(s.len());
    let mut line_start = 0usize;
    let mut pending_cr = false;
    let mut chars = s.chars().peekable();

    fn trim_line_end(out: &mut String, line_start: usize) {
        while out.len() > line_start && matches!(out.as_bytes()[out.len() - 1], b' ' | b'\t') {
            out.pop();
        }
    }

    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    for n in chars.by_ref() {
                        if ('\x40'..='\x7e').contains(&n) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(n) = chars.next() {
                        if n == '\x07' {
                            break;
                        }
                        if n == '\x1b' {
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        }
                    }
                }
                Some('(' | ')') => {
                    chars.next();
                    chars.next();
                }
                Some(n) if ('\x40'..='\x5f').contains(&n) => {
                    chars.next();
                }
                _ => {}
            },
            '\r' => {
                if chars.peek() != Some(&'\n') {
                    pending_cr = true;
                }
            }
            '\n' => {
                pending_cr = false;
                trim_line_end(&mut out, line_start);
                out.push('\n');
                line_start = out.len();
            }
            '\x08' => {
                if out.len() > line_start {
                    out.pop();
                }
            }
            '\t' => {
                if pending_cr {
                    out.truncate(line_start);
                    pending_cr = false;
                }
                out.push('\t');
            }
            // The control characters of both ranges: a terminal may act on the upper ones as on an escape.
            c if (c as u32) < 0x20 || c == '\x7f' || ('\u{80}'..='\u{9f}').contains(&c) => {}
            c => {
                // A bare CR means "redraw this line": the text that follows replaces it.
                if pending_cr {
                    out.truncate(line_start);
                    pending_cr = false;
                }
                out.push(c);
            }
        }
    }
    trim_line_end(&mut out, line_start);
    out
}

const NOISE_VERBS: &[&str] = &[
    "downloading",
    "downloaded",
    "collecting",
    "compiling",
    "fetching",
    "fetched",
    "installing",
    "resolving",
    "building",
    "extracting",
    "unpacking",
    "preparing",
    "selecting",
    "setting up",
    "get:",
    "hit:",
    "receiving",
    "remote:",
    "progress",
    "requirement already satisfied",
    "using cached",
    "checking",
    "adding",
    "locking",
    "updating",
    "reading",
    "processing triggers",
    "added",
    "reused",
    "copying",
    "creating",
    "linking",
];

fn is_progress_like(line: &str) -> bool {
    let l = line.trim_start().to_ascii_lowercase();
    NOISE_VERBS.iter().any(|v| l.starts_with(v))
        || l.contains('%')
        || l.contains("it/s")
        || l.contains(" eta ")
        || l.contains("kb/s")
        || l.contains("mb/s")
        || l.contains("kib/s")
        || l.contains("mib/s")
}

/// The line with every digit run replaced by `#`, used to recognise "the same line with a counter".
fn digit_mask(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_digits = false;
    for c in line.chars() {
        if c.is_ascii_digit() || (in_digits && (c == '.' || c == ',')) {
            if !in_digits {
                out.push('#');
                in_digits = true;
            }
        } else {
            in_digits = false;
            out.push(c);
        }
    }
    out
}

/// Collapses what a reader would skip: blank-line runs, exact repeats, and runs of progress chatter.
/// Data-like lines are never collapsed unless they are byte-identical.
pub fn squeeze(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];

        if line.is_empty() {
            let mut j = i;
            while j < lines.len() && lines[j].is_empty() {
                j += 1;
            }
            out.push(String::new());
            i = j;
            continue;
        }

        let mut j = i + 1;
        while j < lines.len() && lines[j] == line {
            j += 1;
        }
        if j - i >= 4 {
            out.push(line.to_string());
            out.push(format!(
                "  … (previous line repeated {} more times)",
                j - i - 1
            ));
            i = j;
            continue;
        }

        if is_progress_like(line) {
            let mask = digit_mask(line);
            let mut j = i + 1;
            while j < lines.len() && is_progress_like(lines[j]) && digit_mask(lines[j]) == mask {
                j += 1;
            }
            if j - i >= 6 {
                out.push(line.to_string());
                out.push(format!("  … ({} similar progress lines)", j - i - 2));
                out.push(lines[j - 1].to_string());
                i = j;
                continue;
            }
        }

        out.push(line.to_string());
        i += 1;
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out
}

#[derive(Debug, Clone)]
pub struct Bounded {
    pub text: String,
    pub hidden_lines: usize,
}

const MAX_COLS: usize = 400;

fn cut_cols(line: &str) -> std::borrow::Cow<'_, str> {
    match line.char_indices().nth(MAX_COLS) {
        None => std::borrow::Cow::Borrowed(line),
        Some((idx, _)) => std::borrow::Cow::Owned(format!(
            "{}…[+{} chars]",
            &line[..idx],
            line[idx..].chars().count()
        )),
    }
}

/// Keeps the first `head` and last `tail` lines within `max_chars`, replacing the middle with a marker
/// that says how much is hidden and where the full text can be read.
pub fn bound(
    lines: &[String],
    head: usize,
    tail: usize,
    max_chars: usize,
    spill: Option<&str>,
) -> Bounded {
    let total = lines.len();
    let (mut head, mut tail) = (head.min(total), tail.min(total));
    // Everything fits by line count: still two ends, so that a cut for size comes out of the middle
    // and not off the end, where the verdict is. Also where the head alone would take every line.
    if head + tail >= total {
        head = head.min(total / 2);
        tail = total - head;
    }
    loop {
        let shown: usize = lines[..head]
            .iter()
            .chain(lines[total - tail..].iter())
            .map(|l| l.len().min(MAX_COLS * 4) + 1)
            .sum();
        if shown <= max_chars || head + tail <= 2 {
            break;
        }
        // Shrink the larger side first; the tail usually carries the verdict, so it wins ties.
        if head > tail {
            head -= (head / 4).max(1);
        } else {
            tail -= (tail / 4).max(1);
        }
    }
    let hidden = total - head - tail;
    let mut text = String::new();
    for l in &lines[..head] {
        text.push_str(&cut_cols(l));
        text.push('\n');
    }
    if hidden > 0 {
        match spill {
            Some(name) => text.push_str(&format!(
                "… [{hidden} lines hidden · read path \"{name}\" for all of it] …\n"
            )),
            None => text.push_str(&format!("… [{hidden} lines hidden] …\n")),
        }
    }
    for l in &lines[total - tail..] {
        text.push_str(&cut_cols(l));
        text.push('\n');
    }
    Bounded {
        text,
        hidden_lines: hidden,
    }
}

/// The last `n` non-empty lines, for receipts.
pub fn tail(lines: &[String], n: usize) -> Vec<&str> {
    let mut picked: Vec<&str> = lines
        .iter()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .take(n)
        .map(String::as_str)
        .collect();
    picked.reverse();
    picked
}

/// Rough token count for telemetry when the provider has not reported real usage yet.
pub fn est_tokens(s: &str) -> u64 {
    (s.len() as u64).div_ceil(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_ansi_and_applies_carriage_returns() {
        let raw = b"\x1b[32mok\x1b[0m  \nprogress 10%\rprogress 100%\ndone\x1b]0;title\x07\r\n";
        assert_eq!(clean(raw), "ok\nprogress 100%\ndone\n");
    }

    #[test]
    fn keeps_final_redrawn_line_and_handles_backspace() {
        assert_eq!(clean(b"loading\rdone\r"), "done");
        assert_eq!(clean(b"abc\x08\x08de"), "ade");
        assert_eq!(clean(b"a\x00b\x07c"), "abc");
        assert_eq!(clean("a\u{9b}31mb\u{9d}c".as_bytes()), "a31mbc");
    }

    #[test]
    fn squeezes_repeats_and_progress_but_not_data() {
        let text = ["warn: x"; 6].join("\n");
        let out = squeeze(&text);
        assert_eq!(out.len(), 2);
        assert!(out[1].contains("repeated 5 more times"));

        let progress: Vec<String> = (1..=10)
            .map(|i| format!("Downloading pkg-{i}.whl ({i}0%)"))
            .collect();
        let out = squeeze(&progress.join("\n"));
        assert_eq!(out.len(), 3, "{out:?}");
        assert!(out[1].contains("8 similar progress lines"));

        // Numeric data must survive untouched: the model may have asked for exactly these values.
        let data: Vec<String> = (1..=12).map(|i| format!("{i} {}", i * i)).collect();
        assert_eq!(squeeze(&data.join("\n")).len(), 12);
    }

    #[test]
    fn collapses_blank_runs_and_trailing_blanks() {
        assert_eq!(squeeze("a\n\n\n\nb\n\n\n"), vec!["a", "", "b"]);
    }

    #[test]
    fn bound_keeps_head_and_tail_with_marker() {
        let lines: Vec<String> = (1..=100).map(|i| format!("line {i}")).collect();
        let b = bound(&lines, 3, 2, 10_000, Some("out:4"));
        assert_eq!(b.hidden_lines, 95);
        assert!(b.text.starts_with("line 1\nline 2\nline 3\n… [95 lines hidden · read path \"out:4\" for all of it] …\nline 99\nline 100\n"));

        let all = bound(&lines[..4], 3, 2, 10_000, None);
        assert_eq!(all.hidden_lines, 0);
        assert_eq!(all.text.lines().count(), 4);
    }

    #[test]
    fn bound_respects_char_budget() {
        let lines: Vec<String> = (1..=200)
            .map(|i| format!("{i:03} {}", "x".repeat(96)))
            .collect();
        let b = bound(&lines, 60, 60, 2_000, None);
        assert!(b.text.len() < 2_400, "{}", b.text.len());
        assert!(b.hidden_lines > 150);
        assert!(b.text.contains("200 "), "tail must survive");
    }

    #[test]
    fn a_cut_for_size_never_takes_the_last_lines() {
        // Few enough lines to show them all, but too long: what goes is the middle, not the verdict.
        let mut lines: Vec<String> = (1..=60)
            .map(|i| format!("step {i:02} {}", "x".repeat(200)))
            .collect();
        lines.push("AssertionError: expected 3 got 4".into());
        let b = bound(&lines, 20, 80, 10_000, Some("out:1"));
        assert!(
            b.text.ends_with("AssertionError: expected 3 got 4\n")
                && b.text.starts_with("step 01 "),
            "{}",
            b.text
        );
        assert!(b.hidden_lines > 0 && b.text.contains("lines hidden · read path \"out:1\""));
        // Fewer lines than the head alone may take, and still too long: the same holds.
        let mut wide: Vec<String> = (1..=14)
            .map(|i| format!("row {i:02} {}", "x".repeat(1000)))
            .collect();
        wide.push("AssertionError: expected 3 got 4".into());
        for (head, tail, chars) in [(20, 80, 10_000), (60, 60, 8_000)] {
            let b = bound(&wide, head, tail, chars, Some("out:2"));
            assert!(
                b.text.ends_with("AssertionError: expected 3 got 4\n")
                    && b.text.starts_with("row 01 ")
                    && b.hidden_lines > 0,
                "{}",
                b.text
            );
        }
        // Output that fits is shown once, whole.
        let short: Vec<String> = (1..=5).map(|i| format!("l{i}")).collect();
        assert_eq!(
            bound(&short, 20, 80, 10_000, None).text,
            "l1\nl2\nl3\nl4\nl5\n"
        );
        assert_eq!(
            bound(&short, 2, 80, 10_000, None).text,
            "l1\nl2\nl3\nl4\nl5\n"
        );
    }

    #[test]
    fn tail_skips_blank_lines() {
        let lines = vec![
            "a".to_string(),
            "".to_string(),
            "b".to_string(),
            "  ".to_string(),
        ];
        assert_eq!(tail(&lines, 2), vec!["a", "b"]);
    }
}
