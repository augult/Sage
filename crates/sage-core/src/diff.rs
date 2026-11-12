//! Line diffs of what `edit` and `write` did to a file, for a before/after view in the UI.
//! The model never reads these: its observation stays the changed lines with fresh anchors.

use serde::{Deserialize, Serialize};

/// Unchanged lines kept on each side of a change.
const CONTEXT: usize = 3;
/// The most lines a change carries. What is cut still counts in `added` and `removed`.
const MAX_LINES: usize = 400;
const MAX_COLS: usize = 400;
/// Above this many line comparisons the changed region is taken as one replacement.
const MAX_CELLS: usize = 400_000;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    /// Where the hunk starts in the file before and after, 1-based.
    pub old_start: u32,
    pub new_start: u32,
    /// Each line led by ' ' (unchanged), '-' (removed) or '+' (added).
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    /// The file did not exist before.
    pub created: bool,
    pub added: u32,
    pub removed: u32,
    pub hunks: Vec<Hunk>,
    /// `hunks` stops short of the whole change.
    pub truncated: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Keep,
    Remove,
    Add,
}

/// The way from `a` to `b` as kept, removed and added lines, along a longest common subsequence.
fn script(a: &[String], b: &[String]) -> Vec<Op> {
    let (m, n) = (a.len(), b.len());
    if m == 0 || n == 0 || m.saturating_mul(n) > MAX_CELLS {
        return std::iter::repeat_n(Op::Remove, m).chain(std::iter::repeat_n(Op::Add, n)).collect();
    }
    let width = n + 1;
    let mut common = vec![0u32; (m + 1) * width];
    for i in (0..m).rev() {
        for j in (0..n).rev() {
            common[i * width + j] = if a[i] == b[j] { common[(i + 1) * width + j + 1] + 1 } else { common[(i + 1) * width + j].max(common[i * width + j + 1]) };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut ops = Vec::with_capacity(m + n);
    while i < m && j < n {
        if a[i] == b[j] {
            ops.push(Op::Keep);
            i += 1;
            j += 1;
        } else if common[(i + 1) * width + j] >= common[i * width + j + 1] {
            ops.push(Op::Remove);
            i += 1;
        } else {
            ops.push(Op::Add);
            j += 1;
        }
    }
    ops.extend(std::iter::repeat_n(Op::Remove, m - i));
    ops.extend(std::iter::repeat_n(Op::Add, n - j));
    ops
}

fn clip(text: &str) -> &str {
    text.char_indices().nth(MAX_COLS).map_or(text, |(end, _)| &text[..end])
}

/// What turned `before` into `after`. `before` is `None` for a file that did not exist.
pub fn change(path: &str, before: Option<&[String]>, after: &[String]) -> FileChange {
    let created = before.is_none();
    let before = before.unwrap_or(&[]);
    let prefix = before.iter().zip(after).take_while(|(a, b)| a == b).count();
    let suffix = before[prefix..].iter().rev().zip(after[prefix..].iter().rev()).take_while(|(a, b)| a == b).count();
    let ops = script(&before[prefix..before.len() - suffix], &after[prefix..after.len() - suffix]);
    let count = |wanted: Op| ops.iter().filter(|op| **op == wanted).count() as u32;
    let mut out = FileChange { path: path.to_string(), created, added: count(Op::Add), removed: count(Op::Remove), ..Default::default() };

    // Every line in reach of the change: what happened to it and where it sits before and after.
    let (lead, trail) = (prefix.min(CONTEXT), suffix.min(CONTEXT));
    let mut rows: Vec<(Op, usize, usize)> = Vec::with_capacity(lead + ops.len() + trail);
    let (mut i, mut j) = (prefix - lead, prefix - lead);
    for op in std::iter::repeat_n(Op::Keep, lead).chain(ops).chain(std::iter::repeat_n(Op::Keep, trail)) {
        rows.push((op, i, j));
        i += usize::from(op != Op::Add);
        j += usize::from(op != Op::Remove);
    }

    // A hunk is a run of changes with its context; runs within two contexts of each other share one.
    let changed: Vec<usize> = rows.iter().enumerate().filter(|(_, row)| row.0 != Op::Keep).map(|(at, _)| at).collect();
    let mut budget = MAX_LINES;
    let mut next = 0;
    while next < changed.len() && !out.truncated {
        let first = changed[next];
        let mut last = first;
        while next + 1 < changed.len() && changed[next + 1] - last <= 2 * CONTEXT + 1 {
            next += 1;
            last = changed[next];
        }
        next += 1;
        let from = first.saturating_sub(CONTEXT);
        let to = (last + CONTEXT).min(rows.len() - 1);
        let mut hunk = Hunk { old_start: rows[from].1 as u32 + 1, new_start: rows[from].2 as u32 + 1, lines: Vec::new() };
        for &(op, i, j) in &rows[from..=to] {
            if budget == 0 {
                out.truncated = true;
                break;
            }
            budget -= 1;
            hunk.lines.push(match op {
                Op::Keep => format!(" {}", clip(&after[j])),
                Op::Remove => format!("-{}", clip(&before[i])),
                Op::Add => format!("+{}", clip(&after[j])),
            });
        }
        if !hunk.lines.is_empty() {
            out.hunks.push(hunk);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(String::from).collect()
    }

    #[test]
    fn a_replaced_line_comes_with_its_context() {
        let before = lines("a\nb\nc\nd\ne\nf\ng\nh\ni\nj");
        let after = lines("a\nb\nc\nd\nE\nf\ng\nh\ni\nj");
        let c = change("x.txt", Some(&before), &after);
        assert_eq!((c.added, c.removed, c.created, c.truncated), (1, 1, false, false));
        assert_eq!(c.hunks, vec![Hunk { old_start: 2, new_start: 2, lines: lines(" b\n c\n d\n-e\n+E\n f\n g\n h") }]);
    }

    #[test]
    fn insertions_and_deletions_keep_both_line_counts() {
        let before = lines("a\nb\nc");
        let c = change("x.txt", Some(&before), &lines("a\nb\nb2\nb3\nc"));
        assert_eq!((c.added, c.removed), (2, 0));
        assert_eq!(c.hunks[0].lines, lines(" a\n b\n+b2\n+b3\n c"));
        let c = change("x.txt", Some(&before), &lines("a\nc"));
        assert_eq!((c.added, c.removed), (0, 1));
        assert_eq!(c.hunks[0].lines, lines(" a\n-b\n c"));
    }

    #[test]
    fn distant_changes_are_separate_hunks_and_near_ones_share_one() {
        let before: Vec<String> = (1..=30).map(|n| n.to_string()).collect();
        let mut after = before.clone();
        after[2] = "three".into();
        after[20] = "twenty-one".into();
        let c = change("n.txt", Some(&before), &after);
        assert_eq!((c.added, c.removed, c.hunks.len()), (2, 2, 2));
        assert_eq!((c.hunks[0].old_start, c.hunks[1].old_start, c.hunks[1].new_start), (1, 18, 18));
        assert_eq!(c.hunks[1].lines, lines(" 18\n 19\n 20\n-21\n+twenty-one\n 22\n 23\n 24"));
        // Five unchanged lines between two changes are less than two contexts: one hunk.
        after[8] = "nine".into();
        assert_eq!(change("n.txt", Some(&before), &after).hunks.len(), 2);
    }

    #[test]
    fn a_new_file_is_all_additions() {
        let c = change("new.txt", None, &lines("a\nb"));
        assert!(c.created);
        assert_eq!((c.added, c.removed), (2, 0));
        assert_eq!(c.hunks, vec![Hunk { old_start: 1, new_start: 1, lines: lines("+a\n+b") }]);
        // An existing file that was empty is not a new one.
        assert!(!change("new.txt", Some(&[]), &lines("a")).created);
    }

    #[test]
    fn a_large_change_is_counted_whole_and_carried_in_part() {
        let after: Vec<String> = (1..=1000).map(|n| format!("line {n}")).collect();
        let c = change("big.txt", None, &after);
        assert_eq!((c.added, c.truncated, c.hunks[0].lines.len()), (1000, true, MAX_LINES));
        // Too many lines to compare one by one: taken as one replacement.
        let before: Vec<String> = (1..=700).map(|n| format!("old {n}")).collect();
        let c = change("big.txt", Some(&before), &after[..700]);
        assert_eq!((c.added, c.removed, c.truncated), (700, 700, true));
        assert!(c.hunks[0].lines.iter().all(|line| line.starts_with('-')));
        // A very long line is cut, on a character boundary.
        let long = vec!["é".repeat(MAX_COLS + 50)];
        assert_eq!(change("wide.txt", None, &long).hunks[0].lines[0].chars().count(), MAX_COLS + 1);
    }
}
