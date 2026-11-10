//! Line anchors: `<line><two letters>`, e.g. `41kq`.
//!
//! The letters are a hash of the line's content. An edit that quotes an anchor is therefore checked
//! against the file as it is now, not as the model remembers it: a stale or miscounted line number is
//! caught (or silently relocated when the line merely moved) instead of corrupting the file. The model
//! never has to retype the text it wants to replace, which matters when output tokens are the
//! expensive ones.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    /// 1-based line number. 0 is only meaningful for "insert at top of file".
    pub line: usize,
    pub hash: Option<[u8; 2]>,
}

pub fn line_hash(line: &str) -> [u8; 2] {
    let mut h: u32 = 0x811c_9dc5;
    for b in line.trim_end().bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    let n = h % 676;
    [b'a' + (n / 26) as u8, b'a' + (n % 26) as u8]
}

pub fn anchor(line_no: usize, text: &str) -> String {
    let h = line_hash(text);
    format!("{line_no}{}{}", h[0] as char, h[1] as char)
}

/// Appends `<anchor>|<text>\n`, cutting very long lines (the hash still covers the whole line).
pub fn push_anchored(out: &mut String, prefix: &str, line_no: usize, text: &str, max_cols: usize) {
    out.push_str(prefix);
    out.push_str(&anchor(line_no, text));
    out.push('|');
    let text = text.trim_end_matches(['\r', '\n']);
    match text.char_indices().nth(max_cols) {
        None => out.push_str(text),
        Some((idx, _)) => {
            out.push_str(&text[..idx]);
            out.push_str(&format!("…[+{} chars]", text[idx..].chars().count()));
        }
    }
    out.push('\n');
}

/// Parses `41kq`, `41`, `41KQ|`, `L41`. Returns `None` for anything else.
pub fn parse_anchor(s: &str) -> Option<Anchor> {
    let s = s.trim().trim_end_matches(['|', ':']).trim_start_matches(['L', 'l', '#']);
    let digits = s.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 9 {
        return None;
    }
    let line: usize = s[..digits].parse().ok()?;
    let rest = &s[digits..];
    let hash = match rest.len() {
        0 => None,
        2 if rest.bytes().all(|b| b.is_ascii_alphabetic()) => {
            let b = rest.as_bytes();
            Some([b[0].to_ascii_lowercase(), b[1].to_ascii_lowercase()])
        }
        _ => return None,
    };
    Some(Anchor { line, hash })
}

/// Parses `41kq-47px`, `41kq..47px`, `41kq:47px`, `41kq to 47px`, or a single anchor (start == end).
pub fn parse_range(s: &str) -> Option<(Anchor, Anchor)> {
    let s = s.trim();
    for sep in ["..", " to ", "-", ":", ","] {
        if let Some((a, b)) = s.split_once(sep) {
            if let (Some(a), Some(b)) = (parse_anchor(a), parse_anchor(b)) {
                return Some((a, b));
            }
        }
    }
    parse_anchor(s).map(|a| (a, a))
}

/// Where an anchor points in the current file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolved {
    /// 1-based line number in the current content.
    pub line: usize,
    /// True when the hash was found on a different line than the one quoted.
    pub moved: bool,
}

/// How far from the quoted line number a moved line is searched for.
const RELOCATE_WINDOW: usize = 60;

/// Resolves an anchor against current content. `hint` is the offset by which a sibling anchor was
/// found to have moved, tried first so both ends of a range relocate together.
pub fn resolve(lines: &[&str], a: Anchor, hint: isize) -> Result<Resolved, String> {
    let n = lines.len();
    let Some(hash) = a.hash else {
        return if a.line >= 1 && a.line <= n {
            Ok(Resolved { line: a.line, moved: false })
        } else {
            Err(format!("line {} is out of range (file has {n} lines)", a.line))
        };
    };
    let matches = |ln: usize| ln >= 1 && ln <= n && line_hash(lines[ln - 1]) == hash;
    if matches(a.line) {
        return Ok(Resolved { line: a.line, moved: false });
    }
    if hint != 0 {
        let hinted = a.line as isize + hint;
        if hinted >= 1 && matches(hinted as usize) {
            return Ok(Resolved { line: hinted as usize, moved: true });
        }
    }
    let lo = a.line.saturating_sub(RELOCATE_WINDOW).max(1);
    let hi = (a.line + RELOCATE_WINDOW).min(n);
    let candidates: Vec<usize> = (lo..=hi).filter(|ln| matches(*ln)).collect();
    match candidates.as_slice() {
        [one] => Ok(Resolved { line: *one, moved: true }),
        [] => Err(format!(
            "anchor {}{}{} no longer matches: line {} has different content now",
            a.line, hash[0] as char, hash[1] as char, a.line
        )),
        _ => Err(format!(
            "anchor {}{}{} is ambiguous: line {} changed and {} nearby lines share that content hash",
            a.line,
            hash[0] as char,
            hash[1] as char,
            a.line,
            candidates.len()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_two_lowercase_letters_and_ignores_trailing_space() {
        let h = line_hash("    return a / b");
        assert!(h.iter().all(|b| b.is_ascii_lowercase()));
        assert_eq!(h, line_hash("    return a / b   \r"));
        assert_ne!(h, line_hash("return a / b"), "leading indentation is content");
    }

    #[test]
    fn parses_anchor_forms() {
        let a = parse_anchor("41kq").unwrap();
        assert_eq!((a.line, a.hash), (41, Some([b'k', b'q'])));
        assert_eq!(parse_anchor("41").unwrap().hash, None);
        assert_eq!(parse_anchor(" 41KQ| ").unwrap().hash, Some([b'k', b'q']));
        assert_eq!(parse_anchor("L7").unwrap().line, 7);
        assert!(parse_anchor("kq").is_none());
        assert!(parse_anchor("41kqz").is_none());
        assert!(parse_anchor("41k").is_none());
    }

    #[test]
    fn parses_ranges() {
        let (a, b) = parse_range("41kq-47px").unwrap();
        assert_eq!((a.line, b.line), (41, 47));
        assert_eq!(parse_range("3..9").unwrap().1.line, 9);
        assert_eq!(parse_range("5ab to 6cd").unwrap().0.line, 5);
        let (a, b) = parse_range("12zz").unwrap();
        assert_eq!(a, b);
        assert!(parse_range("nope").is_none());
    }

    #[test]
    fn resolves_exact_moved_stale_and_ambiguous() {
        let file = ["fn a() {}", "", "fn b() {}", "let x = 1;", "fn c() {}"];
        let at = |ln: usize| Anchor { line: ln, hash: Some(line_hash(file[ln - 1])) };

        assert_eq!(resolve(&file, at(3), 0).unwrap(), Resolved { line: 3, moved: false });

        // Two lines inserted above: the quoted number is stale but the content is still unique.
        let shifted = ["// new", "// new", "fn a() {}", "", "fn b() {}", "let x = 1;", "fn c() {}"];
        assert_eq!(resolve(&shifted, at(3), 0).unwrap(), Resolved { line: 5, moved: true });
        assert_eq!(resolve(&shifted, at(4), 2).unwrap(), Resolved { line: 6, moved: true });

        // Content changed: refuse rather than guess.
        let changed = ["fn a() {}", "", "fn b2() {}", "let x = 1;", "fn c() {}"];
        assert!(resolve(&changed, at(3), 0).unwrap_err().contains("no longer matches"));

        // Hash present on several nearby lines after the quoted one changed: refuse.
        let dup = ["x", "y", "CHANGED", "y", "y"];
        let stale = Anchor { line: 3, hash: Some(line_hash("y")) };
        assert!(resolve(&dup, stale, 0).unwrap_err().contains("ambiguous"));

        // Bare line numbers are trusted but range-checked.
        assert!(resolve(&file, Anchor { line: 9, hash: None }, 0).is_err());
    }

    #[test]
    fn anchored_lines_are_cut_but_keep_full_hash() {
        let long = "x".repeat(50);
        let mut out = String::new();
        push_anchored(&mut out, "  ", 7, &long, 10);
        assert!(out.starts_with(&format!("  {}|xxxxxxxxxx…[+40 chars]", anchor(7, &long))));
    }
}
