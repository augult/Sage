//! Line anchors: `<line><two letters>`, e.g. `41kq`.
//!
//! The letters are a hash of the line's content. An edit that quotes an anchor is therefore checked
//! against the file as it is now, not as the model remembers it: a stale or miscounted line number is
//! caught, or put right where the harness knows how the lines have moved, instead of corrupting the
//! file. The model never has to retype the text it wants to replace, which matters when output
//! tokens are the expensive ones.

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


/// One change the tools made to a file: `removed` lines from line `at` on gave way to `inserted`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Splice {
    pub at: usize,
    pub removed: usize,
    pub inserted: usize,
}

impl Splice {
    /// The change that turns `before` into `after`: what lies between their common head and tail.
    pub fn between(before: &[String], after: &[String]) -> Splice {
        let head = before.iter().zip(after).take_while(|(a, b)| a == b).count();
        let room = before.len().min(after.len()) - head;
        let tail = before.iter().rev().zip(after.iter().rev()).take(room).take_while(|(a, b)| a == b).count();
        Splice { at: head + 1, removed: before.len() - head - tail, inserted: after.len() - head - tail }
    }
}

/// Where a line is once `splices` have been applied, or `None` if one of them replaced it.
fn carry(mut line: usize, splices: &[Splice]) -> Option<usize> {
    for s in splices {
        if line >= s.at + s.removed {
            line = line + s.inserted - s.removed;
        } else if line >= s.at {
            return None;
        }
    }
    Some(line)
}

/// What is known of a file's past when an anchor into it is resolved.
///
/// An anchor is a line number from some earlier state of the file. Which state is not said, but it
/// can only be one the model has seen: never one made by an edit earlier in the reply that carries
/// the anchor, because that reply was written before any of its edits ran.
pub struct Past<'a> {
    /// The tools' own changes, oldest first, since anything else last changed the file.
    pub splices: &'a [Splice],
    /// How many of them were made before the reply now running.
    pub seen: usize,
    /// Whether the model has been shown this line of the file as it was after `seen` splices.
    pub shown: &'a dyn Fn(usize) -> bool,
}

impl Past<'_> {
    /// Nothing known: the file is taken as the model last saw it.
    pub const NONE: Past<'static> = Past { splices: &[], seen: 0, shown: &|_| true };
}

/// The tools' own changes to one file, for as long as nothing else has changed it.
#[derive(Debug, Default)]
pub struct History {
    pub splices: Vec<Splice>,
    /// Hash of the content the newest splice left behind. Any other content means the file was
    /// changed from outside, and the splices no longer lead to it.
    pub hash: u64,
    /// The reply the newest splice was made in.
    pub reply: u32,
    /// How many splices there were before that reply, and the line windows the model had been
    /// shown of the file as it was then.
    pub seen: usize,
    pub visible: Vec<(usize, usize)>,
}

impl History {
    /// More than a session ever makes to one file; older splices are forgotten.
    const MAX_SPLICES: usize = 200;

    /// Records a splice made in `reply` that left content with `hash` behind. `visible` is what
    /// the model had been shown of the file before it.
    pub fn record(&mut self, splice: Splice, hash: u64, reply: u32, visible: Vec<(usize, usize)>) {
        if self.reply != reply || self.splices.is_empty() {
            self.seen = self.splices.len();
            self.visible = visible;
            self.reply = reply;
        }
        self.splices.push(splice);
        self.hash = hash;
        if self.splices.len() > Self::MAX_SPLICES {
            self.splices.remove(0);
            self.seen = self.seen.saturating_sub(1);
        }
    }

    /// The past as it bears on an anchor sent in `reply`. `live` says whether the model has been
    /// shown a line of the file as it is now.
    pub fn past<'a>(&'a self, reply: u32, live: &'a dyn Fn(usize) -> bool, frozen: &'a dyn Fn(usize) -> bool) -> Past<'a> {
        // Splices of the reply now running are ones the model has not seen the outcome of.
        if self.reply == reply { Past { splices: &self.splices, seen: self.seen, shown: frozen } } else { Past { splices: &self.splices, seen: self.splices.len(), shown: live } }
    }

    pub fn was_visible(&self, line: usize) -> bool {
        self.visible.iter().any(|(from, to)| (*from..=*to).contains(&line))
    }
}

/// Where an anchor points in the current file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolved {
    /// 1-based line number in the current content.
    pub line: usize,
    /// True when that is not the line number quoted.
    pub moved: bool,
}

/// An anchor that could not be placed, and the current lines worth showing with the refusal.
#[derive(Debug, PartialEq, Eq)]
pub struct Unresolved {
    pub message: String,
    pub near: Vec<usize>,
}

/// How far a range is looked for when its anchors fit no known state of the file. Both ends must
/// be found the same distance away, which two unrelated lines do by chance once in some 450,000.
const RANGE_WINDOW: isize = 60;
/// A single anchor has one two-letter hash to go by, which one line in 676 shares: it is looked
/// for only as far as a miscounted line number goes.
const SINGLE_WINDOW: isize = 3;

fn label(a: Anchor) -> String {
    match a.hash {
        Some(h) => format!("{}{}{}", a.line, h[0] as char, h[1] as char),
        None => a.line.to_string(),
    }
}

/// Resolves the two ends of a range (the same anchor twice for a single line) against the current
/// content.
///
/// Each state of the file the model can have seen is tried: the anchors are carried through the
/// splices made since, and fit where both land on lines with their hashes. That is exact where the
/// old way of looking for the hash nearby was a guess: after an insertion further up, the old line
/// number often holds another `}` or `return None`, and a two-letter hash is found on some unrelated
/// line of a 120-line window one time in six.
pub fn resolve_range(lines: &[&str], a: Anchor, b: Anchor, past: &Past) -> Result<(Resolved, Resolved), Unresolved> {
    let n = lines.len();
    let fits = |anchor: Anchor, ln: isize| ln >= 1 && ln as usize <= n && anchor.hash.is_none_or(|h| line_hash(lines[ln as usize - 1]) == h);
    let found = |s: usize, e: usize| Ok((Resolved { line: s, moved: s != a.line }, Resolved { line: e, moved: e != b.line }));
    let hashed = a.hash.is_some() || b.hash.is_some();

    // Newest state first. Bare line numbers say nothing about which state they are from, so they
    // are read as the newest.
    let mut readings: Vec<(usize, usize, usize)> = Vec::new();
    for state in (0..=past.seen.min(past.splices.len())).rev() {
        let since = &past.splices[state..];
        if let (Some(s), Some(e)) = (carry(a.line, since), carry(b.line, since)) {
            if fits(a, s as isize) && fits(b, e as isize) && !readings.iter().any(|r| (r.1, r.2) == (s, e)) {
                readings.push((state, s, e));
            }
        }
        if !hashed {
            break;
        }
    }
    // The file as the model last saw it is what a fresh anchor is about, so that reading wins where
    // the model has been shown the line. Where it has not, the number can only come from an older
    // state, and saying which is not for the harness to guess.
    if let Some((_, s, e)) = readings.first().copied().filter(|r| r.0 == past.seen && (readings.len() == 1 || (past.shown)(a.line))) {
        return found(s, e);
    }
    match readings.as_slice() {
        [(_, s, e)] => return found(*s, *e),
        [] => {}
        several => {
            let places = several.iter().map(|(_, s, _)| format!("line {s}")).collect::<Vec<_>>().join(" and ");
            return Err(Unresolved { message: format!("anchor {} is ambiguous: after the edits made since it was shown it fits {places}", label(a)), near: several.iter().map(|r| r.1).collect() });
        }
    }

    // No state fits. An earlier edit of this very reply may have replaced the line.
    let pending = &past.splices[past.seen.min(past.splices.len())..];
    let (Some(s), Some(e)) = (carry(a.line, pending), carry(b.line, pending)) else {
        let gone = if carry(a.line, pending).is_none() { a } else { b };
        let at = pending.iter().find(|p| gone.line >= p.at && gone.line < p.at + p.removed).map_or(gone.line, |p| p.at);
        return Err(Unresolved { message: format!("anchor {} points into lines an earlier edit of this reply replaced", label(gone)), near: vec![at] });
    };
    if s > n || e > n || s == 0 {
        return Err(Unresolved { message: format!("line {} is out of range (file has {n} lines)", if s > n || s == 0 { a.line } else { b.line }), near: vec![n] });
    }
    // Otherwise a miscounted number, or a change the tools did not make: look nearby.
    let window = if a != b && a.hash.is_some() && b.hash.is_some() { RANGE_WINDOW } else { SINGLE_WINDOW };
    let offsets: Vec<isize> = if hashed { (-window..=window).filter(|d| fits(a, s as isize + d) && fits(b, e as isize + d)).collect() } else { Vec::new() };
    let stale = if fits(a, s as isize) { b } else { a };
    match offsets.as_slice() {
        [d] => found((s as isize + d) as usize, (e as isize + d) as usize),
        [] => Err(Unresolved { message: format!("anchor {} no longer matches: line {} has different content now", label(stale), stale.line), near: vec![if stale == a { s } else { e }] }),
        _ => Err(Unresolved { message: format!("anchor {} is ambiguous: line {} changed and {} nearby lines share that content hash", label(stale), stale.line, offsets.len()), near: vec![s] }),
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
        assert_eq!((a.line, a.hash), (41, Some(*b"kq")));
        assert_eq!(parse_anchor("41").unwrap().hash, None);
        assert_eq!(parse_anchor(" 41KQ| ").unwrap().hash, Some(*b"kq"));
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

    fn one(lines: &[&str], a: Anchor, past: &Past) -> Result<Resolved, Unresolved> {
        resolve_range(lines, a, a, past).map(|(s, _)| s)
    }

    #[test]
    fn resolves_exact_miscounted_stale_and_ambiguous() {
        let file = ["fn a() {}", "", "fn b() {}", "let x = 1;", "fn c() {}"];
        let at = |ln: usize| Anchor { line: ln, hash: Some(line_hash(file[ln - 1])) };
        assert_eq!(one(&file, at(3), &Past::NONE).unwrap(), Resolved { line: 3, moved: false });

        // A number that is a line or two off is put right; a range is found further away, because
        // both of its ends have to be there.
        let shifted = ["// new", "// new", "fn a() {}", "", "fn b() {}", "let x = 1;", "fn c() {}"];
        assert_eq!(one(&shifted, at(3), &Past::NONE).unwrap(), Resolved { line: 5, moved: true });
        let far: Vec<&str> = std::iter::repeat_n("// new", 30).chain(file).collect();
        assert!(one(&far, at(3), &Past::NONE).unwrap_err().message.contains("no longer matches"), "one hash is too little to go looking with");
        let (s, e) = resolve_range(&far, at(3), at(5), &Past::NONE).unwrap();
        assert_eq!((s, e), (Resolved { line: 33, moved: true }, Resolved { line: 35, moved: true }));

        // Content changed: refuse rather than guess.
        let changed = ["fn a() {}", "", "fn b2() {}", "let x = 1;", "fn c() {}"];
        let refused = one(&changed, at(3), &Past::NONE).unwrap_err();
        assert!(refused.message.contains("no longer matches") && refused.near == [3], "{refused:?}");

        // Hash present on several nearby lines after the quoted one changed: refuse.
        let dup = ["x", "y", "CHANGED", "y", "y"];
        let stale = Anchor { line: 3, hash: Some(line_hash("y")) };
        assert!(one(&dup, stale, &Past::NONE).unwrap_err().message.contains("ambiguous"));

        // Bare line numbers are trusted but range-checked.
        assert_eq!(one(&file, Anchor { line: 4, hash: None }, &Past::NONE).unwrap().line, 4);
        assert!(one(&file, Anchor { line: 9, hash: None }, &Past::NONE).unwrap_err().message.contains("out of range"));
    }

    #[test]
    fn an_anchor_is_carried_through_the_edits_made_since_it_was_shown() {
        // As read: three methods that end alike. Then a method was put in after line 1.
        let before = ["class A:", "    def a(self):", "        return None", "", "    def b(self):", "        return None"];
        let now = ["class A:", "    def z(self):", "        return 0", "", "    def a(self):", "        return None", "", "    def b(self):", "        return None"];
        let splices = [Splice { at: 2, removed: 0, inserted: 3 }];
        let at = |ln: usize| Anchor { line: ln, hash: Some(line_hash(before[ln - 1])) };

        // In the reply that made the insertion, every anchor is from before it: line 6 is now line
        // 9, although line 6 holds the same text again.
        let same_reply = Past { splices: &splices, seen: 0, shown: &|_| true };
        assert_eq!(one(&now, at(6), &same_reply).unwrap(), Resolved { line: 9, moved: true });
        assert_eq!(one(&now, at(1), &same_reply).unwrap(), Resolved { line: 1, moved: false });
        let (s, e) = resolve_range(&now, at(5), at(6), &same_reply).unwrap();
        assert_eq!((s.line, e.line), (8, 9));
        // A bare number is carried too.
        assert_eq!(one(&now, Anchor { line: 5, hash: None }, &same_reply).unwrap().line, 8);

        // A reply later, a line the model has been shown since is the line it means.
        let shown = Past { splices: &splices, seen: 1, shown: &|_| true };
        assert_eq!(one(&now, at(6), &shown).unwrap(), Resolved { line: 6, moved: false });
        // One it has not been shown can only be an old number. Where both readings fit, it is asked.
        let unshown = Past { splices: &splices, seen: 1, shown: &|_| false };
        let asked = one(&now, at(6), &unshown).unwrap_err();
        assert!(asked.message.contains("is ambiguous") && asked.near == [6, 9], "{asked:?}");
        // Where only the old reading fits, that is the one.
        assert_eq!(one(&now, at(5), &unshown).unwrap(), Resolved { line: 8, moved: true });
    }

    #[test]
    fn an_anchor_into_replaced_lines_is_refused_not_found_elsewhere() {
        let before: Vec<String> = (1..=200).map(|i| format!("    let value_{i} = compute({i});")).collect();
        let mut now = before.clone();
        now[99] = "    return new_value".to_string();
        let refs: Vec<&str> = now.iter().map(String::as_str).collect();
        let stale = Anchor { line: 100, hash: Some(line_hash(&before[99])) };
        let replaced = [Splice { at: 100, removed: 1, inserted: 1 }];

        let same_reply = one(&refs, stale, &Past { splices: &replaced, seen: 0, shown: &|_| true }).unwrap_err();
        assert!(same_reply.message.contains("an earlier edit of this reply replaced") && same_reply.near == [100], "{same_reply:?}");
        let later = one(&refs, stale, &Past { splices: &replaced, seen: 1, shown: &|_| true }).unwrap_err();
        assert!(later.message.contains("no longer matches"), "{later:?}");

        // Whatever the old text was, its hash is not looked for across the file.
        let accepted = (0..2000).filter(|k| one(&refs, Anchor { line: 100, hash: Some(line_hash(&format!("    return old_{k}"))) }, &Past::NONE).is_ok()).count();
        assert!(accepted < 40, "{accepted} of 2000 stale anchors were placed on some other line");
    }

    #[test]
    fn a_splice_is_what_lies_between_the_common_head_and_tail() {
        let v = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
        assert_eq!(Splice::between(&v("a b c d"), &v("a X Y c d")), Splice { at: 2, removed: 1, inserted: 2 });
        assert_eq!(Splice::between(&v("a b"), &v("n a b")), Splice { at: 1, removed: 0, inserted: 1 });
        assert_eq!(Splice::between(&v("a b c"), &v("a c")), Splice { at: 2, removed: 1, inserted: 0 });
        assert_eq!(carry(3, &[Splice { at: 2, removed: 1, inserted: 2 }]), Some(4));
        assert_eq!(carry(2, &[Splice { at: 2, removed: 1, inserted: 2 }]), None);
        assert_eq!(carry(1, &[Splice { at: 2, removed: 1, inserted: 2 }]), Some(1));
    }

    #[test]
    fn anchored_lines_are_cut_but_keep_full_hash() {
        let long = "x".repeat(50);
        let mut out = String::new();
        push_anchored(&mut out, "  ", 7, &long, 10);
        assert!(out.starts_with(&format!("  {}|xxxxxxxxxx…[+40 chars]", anchor(7, &long))));
    }
}
