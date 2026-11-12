//! Files the task names, shown before the first turn.
//!
//! When a task says "fix `pager.py`", the model's first move is known before it has thought at all,
//! and it still costs a whole turn: a fresh round of thinking and one more pass over the fixed
//! prefix, spent on asking for a file. So the harness reads the files the task names and puts them in
//! the first message. It does so through the `read` tool itself: the lines carry the anchors `edit`
//! accepts and the file is on record as shown, exactly as if the model had asked for it.
//!
//! This is the harness guessing, not the model asking, so it is kept cheap and safe: a few small
//! files, from inside the workspace only, and none whose name says it holds credentials.

use crate::state::Session;
use crate::tools::{StepStatus, read};
use crate::util::fmt_bytes;
use crate::workspace::{Workspace, small_tree};
use serde_json::json;
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Files attached at most.
const MAX_FILES: usize = 4;
/// A longer file is left for the model to read in parts.
const MAX_FILE_LINES: usize = 250;
const MAX_FILE_BYTES: u64 = 12 * 1024;
/// All attached files together.
const MAX_TOTAL_LINES: usize = 400;
const MAX_TOTAL_BYTES: u64 = 24 * 1024;
/// With `survey`, the rest of a workspace of at most `SURVEY_TREE` files follows the named ones,
/// up to these limits for all attached files together.
const SURVEY_TREE: usize = 40;
const SURVEY_FILES: usize = 24;
const SURVEY_TOTAL_LINES: usize = 1000;
const SURVEY_TOTAL_BYTES: u64 = 40 * 1024;
/// Files too long to attach that are still mentioned, with their length.
const MAX_MENTIONED: usize = 4;
/// Named files looked at, however many a long task lists.
const MAX_NAMED: usize = 16;
/// Lines are not counted in a file larger than this.
const COUNT_UP_TO: u64 = 2 * 1024 * 1024;

/// What separates words of prose besides whitespace, none of it part of a path.
const SEPARATORS: [char; 10] = ['(', ')', '[', ']', '{', '}', '<', '>', ',', ';'];
const QUOTES: [char; 7] = ['"', '\'', '“', '”', '‘', '’', '*'];

/// A word without the quotes and the sentence punctuation around it, and without a possessive.
fn bare(word: &str) -> &str {
    let word = word.trim_start_matches(QUOTES).trim_end_matches(|c: char| QUOTES.contains(&c) || matches!(c, '.' | ':' | '!' | '?' | '…'));
    word.strip_suffix("'s").or_else(|| word.strip_suffix("’s")).unwrap_or(word)
}

/// The words of `task` that could be a file name, in order: `backticked` spans of one word and plain
/// words, without what surrounds them. A span of several words is a command or code, so nothing in
/// it counts.
fn words(task: &str) -> Vec<&str> {
    let parts: Vec<&str> = task.split('`').collect();
    let mut out = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        // Odd parts sit between two backticks, unless the last backtick was never closed.
        if i % 2 == 1 && i + 1 < parts.len() {
            let span = part.trim();
            if !span.contains(char::is_whitespace) {
                out.push(bare(span));
            }
        } else {
            out.extend(part.split(|c: char| c.is_whitespace() || SEPARATORS.contains(&c)).map(bare));
        }
    }
    out.retain(|word| !word.is_empty());
    out
}

/// `src/a.rs:12:5` names `src/a.rs`: a position after the name is not part of it.
fn without_position(word: &str) -> &str {
    let mut name = word;
    for _ in 0..2 {
        match name.rsplit_once(':') {
            Some((head, n)) if !head.is_empty() && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => name = head,
            _ => break,
        }
    }
    name
}

/// A file the task names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Named {
    /// The task's spelling of it, as the `read` tool resolves that.
    pub path: PathBuf,
    /// Where that leads once symlinks are followed: a regular file inside the workspace.
    pub real: PathBuf,
}

/// The file a word names, if it names one. Whatever leads out of the workspace (`..`, an absolute
/// path, a symlink) names nothing.
fn inside(ws: &Workspace, word: &str) -> Option<Named> {
    let path = ws.resolve(word);
    let real = crate::util::canonical(&path).ok()?;
    (real.starts_with(&ws.root) && real.is_file()).then_some(Named { path, real })
}

/// The files `task` names, each once, in the order it names them. Words are taken relative to the
/// directory the session starts in, which is the workspace root.
pub fn named_files(task: &str, ws: &Workspace) -> Vec<Named> {
    let mut found: Vec<Named> = Vec::new();
    let mut tried = HashSet::new();
    for word in words(task) {
        if !tried.insert(word) {
            continue;
        }
        let named = inside(ws, word).or_else(|| Some(without_position(word)).filter(|name| *name != word).and_then(|name| inside(ws, name)));
        if let Some(named) = named.filter(|n| !found.iter().any(|f| f.real == n.real)) {
            found.push(named);
            if found.len() == MAX_NAMED {
                break;
            }
        }
    }
    found
}

/// Names under which credentials are usually kept. Such a file is never sent unasked; the model can
/// still read it. A short list of the usual suspects, not a guarantee.
fn may_hold_secrets(below_root: &Path) -> bool {
    below_root.components().any(|part| {
        let name = part.as_os_str().to_string_lossy().to_ascii_lowercase();
        name.starts_with(".env")
            || name.contains("secret")
            || name.contains("credential")
            || [".pem", ".key", ".p12", ".pfx"].iter().any(|end| name.ends_with(end))
            || matches!(name.as_str(), ".netrc" | ".npmrc" | ".pypirc" | "id_rsa" | "id_dsa" | "id_ecdsa" | "id_ed25519")
    })
}

/// How much text a file holds.
pub(crate) struct Size {
    pub(crate) bytes: u64,
    /// Not counted in a very large file.
    pub(crate) lines: Option<usize>,
}

impl Size {
    fn describe(&self) -> String {
        match self.lines {
            Some(lines) => format!("{lines} lines, {}", fmt_bytes(self.bytes)),
            None => fmt_bytes(self.bytes),
        }
    }
}

/// The size of a text file. `None` for one that is binary, not UTF-8 or unreadable: there is nothing
/// to show of it and nothing `edit` could do with it.
pub(crate) fn measure(path: &Path) -> Option<Size> {
    let bytes = std::fs::metadata(path).ok()?.len();
    let mut head = Vec::new();
    std::fs::File::open(path).ok()?.take(COUNT_UP_TO).read_to_end(&mut head).ok()?;
    if head.iter().take(8192).any(|b| *b == 0) {
        return None;
    }
    if head.len() as u64 != bytes {
        return Some(Size { bytes, lines: None });
    }
    Some(Size { bytes, lines: Some(std::str::from_utf8(&head).ok()?.lines().count()) })
}

/// What the model is told below the files: that they need no `read` of their own. Worded as a
/// permission, because a task often names files only to say that they must stay as they are.
fn note(files: usize, surveyed: bool) -> &'static str {
    if surveyed {
        "This is `read` output for the files of this workspace that fit here, those the task names first. They are already read and can be edited directly: do not read them again."
    } else if files == 1 {
        "This is `read` output for the file the task names. It is already read and can be edited directly: do not read it first."
    } else {
        "This is `read` output for the files the task names. They are already read and can be edited directly: do not read them first."
    }
}

/// What `attach` adds to the first message.
pub struct Attached {
    /// The `<files>` block, with the line that says what it is.
    pub text: String,
    /// What was attached and what was not, in one line, for the event log.
    pub summary: String,
}

/// Reads the files the task names into a block for the first message. `None` when the task names
/// none that can be shown, or the mechanism is off.
pub fn attach(sess: &mut Session, task: &str) -> Option<Attached> {
    if !sess.features.attach {
        return None;
    }
    let survey = sess.features.survey;
    let (mut entries, mut attached, mut left_out): (Vec<String>, Vec<String>, Vec<String>) = Default::default();
    let (max_files, mut lines_left, mut bytes_left) = if survey { (SURVEY_FILES, SURVEY_TOTAL_LINES, SURVEY_TOTAL_BYTES) } else { (MAX_FILES, MAX_TOTAL_LINES, MAX_TOTAL_BYTES) };
    let named = named_files(task, &sess.ws);
    let named_count = named.len();
    // The rest of a small workspace comes after the files the task names, for as long as it fits:
    // the model would otherwise spend its first replies asking for these one by one.
    let rest: Vec<Named> = match survey {
        true => small_tree(&sess.ws.root, SURVEY_TREE).unwrap_or_default().into_iter().map(|rel| sess.ws.root.join(rel)).map(|p| Named { path: p.clone(), real: p }).collect(),
        false => Vec::new(),
    };
    let mut taken: HashSet<PathBuf> = HashSet::new();
    let mut surveyed = false;
    for (i, named) in named.into_iter().chain(rest).enumerate() {
        let asked_for = i < named_count;
        if attached.len() == max_files {
            break;
        }
        // Project instructions are in the system prompt already.
        let instructions = !asked_for && named.real.file_name().is_some_and(|n| n == "AGENTS.md" || n == "CLAUDE.md");
        if instructions || !taken.insert(named.real.clone()) || named.real.strip_prefix(&sess.ws.root).is_ok_and(may_hold_secrets) {
            continue;
        }
        let (Some(size), Some(path)) = (measure(&named.real), named.path.to_str()) else { continue };
        let shown = sess.ws.display(&named.path);
        let fits = size.lines.filter(|lines| *lines <= MAX_FILE_LINES.min(lines_left) && size.bytes <= MAX_FILE_BYTES.min(bytes_left));
        let Some(lines) = fits else {
            // One line with its length, so that the model knows to read the part it needs.
            if asked_for && left_out.len() < MAX_MENTIONED {
                entries.push(format!("{shown} · {} · not attached: read the part you need", size.describe()));
                left_out.push(format!("{shown} ({})", size.describe()));
            }
            continue;
        };
        // The `read` tool itself: its anchors are the ones `edit` checks, and it puts the file on
        // record as shown, so that asking for it again is answered as a repeat.
        let out = read::run(sess, &json!({"path": path}));
        // Of the files nobody named, only real content is worth the room.
        if out.status != StepStatus::Ok || (!asked_for && (lines == 0 || out.observation.starts_with('['))) {
            continue;
        }
        lines_left -= lines;
        bytes_left -= size.bytes;
        surveyed |= !asked_for;
        entries.push(out.observation);
        attached.push(format!("{shown} ({lines} lines)"));
    }
    if entries.is_empty() {
        return None;
    }
    let mut text = format!("<files>\n{}\n</files>", entries.join("\n\n"));
    let mut summary = Vec::new();
    if !attached.is_empty() {
        text.push('\n');
        text.push_str(note(attached.len(), surveyed));
        summary.push(format!("attached {}", attached.join(", ")));
    }
    if !left_out.is_empty() {
        summary.push(format!("too long to attach: {}", left_out.join(", ")));
    }
    Some(Attached { text, summary: summary.join("; ") })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchors::anchor;
    use crate::state::{Features, test_session};
    use crate::tools::edit;

    fn put(sess: &Session, name: &str, body: &str) {
        let path = sess.ws.root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn names(task: &str, sess: &Session) -> Vec<String> {
        named_files(task, &sess.ws).iter().map(|n| sess.ws.display(&n.path)).collect()
    }

    fn numbered(n: usize) -> String {
        (1..=n).map(|i| format!("line {i}\n")).collect()
    }

    #[test]
    fn a_task_names_files_in_backticks_or_plainly_and_a_command_is_not_a_file() {
        let s = test_session("attach-names", Features::ecosage().without("survey").unwrap());
        for name in ["pager.py", "src/util.py", "notes.txt", "Makefile", "test_pager.py"] {
            put(&s, name, "x\n");
        }
        let task = "`python3 -m unittest test_pager.py` fails. Fix `pager.py` (see src/util.py, \"notes.txt\".) and the Makefile; `missing.py` and `to_page(n)` are not there. Then read pager.py: again.";
        assert_eq!(names(task, &s), ["pager.py", "src/util.py", "notes.txt", "Makefile"]);
        // A position after the name, a leading `./`, a possessive and a backtick that is never closed.
        assert_eq!(names("It breaks at src/util.py:12:5, or is it ./pager.py?", &s), ["src/util.py", "pager.py"]);
        assert_eq!(names("an unbalanced ` tick before notes.txt's last line", &s), ["notes.txt"]);
        // A directory is not a file, and neither are the words of a command.
        assert_eq!(names("Nothing here names a file, not even src or `sh pager.py --all`.", &s), Vec::<String>::new());
    }

    #[cfg(unix)]
    #[test]
    fn nothing_outside_the_workspace_is_named() {
        let s = test_session("attach-escape", Features::ecosage().without("survey").unwrap());
        put(&s, "inside.txt", "in\n");
        put(&s, "sub/deep.txt", "deep\n");
        let outside = s.ws.root.parent().unwrap().join("outside.txt");
        std::fs::write(&outside, "out\n").unwrap();
        std::os::unix::fs::symlink(&outside, s.ws.root.join("escape.txt")).unwrap();
        std::os::unix::fs::symlink("..", s.ws.root.join("up")).unwrap();
        std::os::unix::fs::symlink("inside.txt", s.ws.root.join("alias.txt")).unwrap();

        // `..`, an absolute path, the home directory, a symlinked file and a symlinked directory.
        let task = format!("Look at ../outside.txt, sub/../../outside.txt, {}, /etc/passwd, ~/.profile, `escape.txt` and up/outside.txt.", outside.display());
        assert_eq!(names(&task, &s), Vec::<String>::new());
        // Inside it any spelling will do, and two spellings of one file are one file.
        let task = format!("{}/inside.txt, sub/../inside.txt and alias.txt, then sub/deep.txt", s.ws.root.display());
        assert_eq!(names(&task, &s), ["inside.txt", "sub/deep.txt"]);
    }

    #[tokio::test]
    async fn an_attached_file_is_read_output_and_counts_as_read() {
        let mut s = test_session("attach-read", Features::ecosage().without("survey").unwrap());
        put(&s, "a.txt", "alpha\nbeta\n");
        put(&s, "b.txt", "gamma\n");
        let got = attach(&mut s, "Swap the lines of `a.txt`; b.txt stays.").expect("the task names two files");
        let (a1, a2, b1) = (anchor(1, "alpha"), anchor(2, "beta"), anchor(1, "gamma"));
        assert_eq!(got.text, format!("<files>\na.txt · 2 lines\n{a1}|alpha\n{a2}|beta\n\nb.txt · 1 lines\n{b1}|gamma\n</files>\n{}", note(2, false)));
        assert_eq!(got.summary, "attached a.txt (2 lines), b.txt (1 lines)");

        // The model asking for it is answered as a repeat, and the anchors are good for an edit.
        let again = read::run(&mut s, &json!({"path": "a.txt"}));
        assert!(again.observation.starts_with("[unchanged] a.txt lines 1-2 are already in your context (shown in the first message)"), "{}", again.observation);
        let edited = edit::run(&mut s, &json!({"path": "a.txt", "range": format!("{a1}-{a2}"), "new": "beta\nalpha"})).await;
        assert_eq!(edited.status, StepStatus::Ok, "{}", edited.observation);
        assert_eq!(std::fs::read_to_string(s.ws.root.join("a.txt")).unwrap(), "beta\nalpha\n");

        // With nothing named, or switched off, there is nothing to add.
        assert!(attach(&mut s, "Tidy up.").is_none());
        s.features = Features::ecosage().without("survey").unwrap().without("attach").unwrap();
        assert!(attach(&mut s, "Swap the lines of `a.txt`.").is_none());
    }

    #[test]
    fn only_small_text_files_are_attached_and_a_long_one_is_mentioned_with_its_length() {
        let mut s = test_session("attach-size", Features::ecosage().without("survey").unwrap());
        put(&s, "long.txt", &numbered(MAX_FILE_LINES + 1));
        put(&s, "wide.txt", &format!("{}\n", "x".repeat(MAX_FILE_BYTES as usize)));
        put(&s, "empty.txt", "");
        put(&s, "ok.txt", &numbered(MAX_FILE_LINES));
        std::fs::write(s.ws.root.join("blob.bin"), [0u8, 159, 146, 150]).unwrap();
        std::fs::write(s.ws.root.join("latin1.txt"), [b'c', b'a', b'f', 0xe9, b'\n']).unwrap();
        let long = fmt_bytes(numbered(MAX_FILE_LINES + 1).len() as u64);

        let got = attach(&mut s, "long.txt, wide.txt, blob.bin, latin1.txt, empty.txt and ok.txt").unwrap();
        let head = format!("<files>\nlong.txt · 251 lines, {long} · not attached: read the part you need\n\nwide.txt · 1 lines, 12.0 KB · not attached: read the part you need\n\n[empty] empty.txt exists and is empty\n\nok.txt · 250 lines\n{}|line 1\n", anchor(1, "line 1"));
        assert!(got.text.starts_with(&head), "{}", got.text);
        assert!(got.text.ends_with(&format!("{}|line 250\n</files>\n{}", anchor(250, "line 250"), note(2, false))), "{}", got.text);
        assert_eq!(got.summary, format!("attached empty.txt (0 lines), ok.txt (250 lines); too long to attach: long.txt (251 lines, {long}), wide.txt (1 lines, 12.0 KB)"));

        // With nothing short enough there is nothing that is already read, and nothing says so.
        let got = attach(&mut s, "Summarise long.txt.").unwrap();
        assert_eq!(got.text, format!("<files>\nlong.txt · 251 lines, {long} · not attached: read the part you need\n</files>"));
    }

    #[test]
    fn the_total_is_bounded_in_lines_and_in_files_and_credentials_stay_out() {
        let mut s = test_session("attach-total", Features::ecosage().without("survey").unwrap());
        for name in ["a.txt", "b.txt", "c.txt"] {
            put(&s, name, &numbered(180));
        }
        put(&s, "d.txt", &numbered(40));
        // The third would take the total past the limit; a later one that still fits is attached.
        let got = attach(&mut s, "a.txt b.txt c.txt d.txt").unwrap();
        let size = fmt_bytes(numbered(180).len() as u64);
        assert_eq!(got.summary, format!("attached a.txt (180 lines), b.txt (180 lines), d.txt (40 lines); too long to attach: c.txt (180 lines, {size})"));
        assert_eq!(got.text.lines().filter(|l| l.contains("|line ")).count(), 400);

        let mut s = test_session("attach-count", Features::ecosage().without("survey").unwrap());
        for i in 1..=6 {
            put(&s, &format!("f{i}.txt"), "x\n");
        }
        put(&s, ".env", "KEY=hunter2\n");
        put(&s, "deploy/secrets.yaml", "token: hunter2\n");
        let got = attach(&mut s, "Do not touch .env or deploy/secrets.yaml; change f1.txt f2.txt f3.txt f4.txt f5.txt f6.txt").unwrap();
        assert_eq!(got.summary, "attached f1.txt (1 lines), f2.txt (1 lines), f3.txt (1 lines), f4.txt (1 lines)");
        assert!(!got.text.contains("hunter2") && !got.text.contains(".env") && !got.text.contains("f5.txt"), "{}", got.text);
    }

    #[tokio::test]
    async fn the_rest_of_a_small_workspace_follows_the_named_files_and_a_large_one_is_left_alone() {
        let mut s = test_session("attach-survey", Features::ecosage());
        for (name, body) in [("pkg/b.py", "b = 2\n"), ("pkg/a.py", "a = 1\n"), ("main.py", "import pkg\n"), ("AGENTS.md", "Run make test.\n"), ("empty.txt", ""), ("secrets.txt", "hunter2\n"), ("big.csv", numbered(300).as_str())] {
            put(&s, name, body);
        }
        let got = attach(&mut s, "Fix pkg/b.py.").unwrap();
        assert_eq!(got.summary, "attached pkg/b.py (1 lines), main.py (1 lines), pkg/a.py (1 lines)");
        assert!(got.text.ends_with("those the task names first. They are already read and can be edited directly: do not read them again."), "{}", got.text);
        assert!(!got.text.contains("hunter2") && !got.text.contains("make test") && !got.text.contains("big.csv"), "{}", got.text);
        // Named or not, the file is on record as read.
        let line = "a = 1";
        assert_eq!(edit::run(&mut s, &json!({"path": "pkg/a.py", "range": anchor(1, line), "new": "a = 3"})).await.status, StepStatus::Ok);

        let mut s = test_session("attach-survey-large", Features::ecosage());
        for i in 0..=SURVEY_TREE {
            put(&s, &format!("src/f{i}.py"), "x = 1\n");
        }
        assert_eq!(attach(&mut s, "Fix src/f1.py.").unwrap().summary, "attached src/f1.py (1 lines)");
    }
}
