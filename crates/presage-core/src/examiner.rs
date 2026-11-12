//! The examiner: a second reader of the task.
//!
//! A model that has implemented a specification tests its own reading of it, so whatever it
//! misread, its checks misread too. The examiner is a separate request with a fresh context that
//! sees the task and the same files, never the work, and writes one check script from the text
//! alone. It is asked for when the run starts and answers beside it, so it costs a request and no
//! waiting. When the worker is about to end its run, the script is run against the work; a failing case is
//! handed to the worker to settle by the text, as either a gap in the work or a mistake of the
//! examiner.

use crate::provider::Message;
use regex::Regex;
use std::sync::LazyLock;

pub const SYSTEM: &str = "\
You are the examiner of a coding task that another engineer is working on right now. You never see their work. From the task and the files shown below you write one check script that tells whether the finished work does everything the task, and every document the task names, requires.

How to write it:
- Go through the task and each document it names sentence by sentence. Every rule, every worked example and every stated error case becomes at least one case, and so does every interaction between rules that the text defines. Expected values come from the text alone. Name each case after the clause it checks (for example `s5_pieces_joined_with_one_space`).
- Where the text leaves a point open or you are not sure what it requires, write no case. A wrong case costs more than a missing one.
- Cover what the existing tests leave out; do not repeat them.
- Use only the interface that the task, the documents and the existing code fix: names, signatures, files, commands. Never guess one.
- The script is stored outside the project and run from the project's root directory, so it reaches the project's code relative to the working directory (in Python: `sys.path.insert(0, os.getcwd())`). It uses the project's language and nothing that is not already installed. It changes no file of the project; anything it needs to write goes into a temporary directory.
- Every case runs on its own: a case that raises or crashes counts as failed and the others still run. For each failed case print one block: its name, the clause in a few words, the input, what was expected and what came back. The last line printed is exactly `EXAM: <passed> passed, <failed> failed`. The exit status is 0 only if no case failed.
- Lines of the files shown may begin with a short anchor such as `41kq`; it is not part of the file.

If the task is a question, an investigation or a review, or if nothing about its result can be checked by a script, reply with the single word NONE.

Otherwise reply with exactly this and nothing else:
COMMAND: <the command that runs the script, with {file} where its path goes>
```<language>
<the script>
```";

const ASK: &str = "Write the check script for this task now.";

/// The examiner's request: its instructions, then what the worker was shown at the start.
pub fn brief(opening: &str) -> Vec<Message> {
    vec![Message::system(SYSTEM), Message::user(format!("{opening}\n\n{ASK}"))]
}

#[derive(Debug, PartialEq, Eq)]
pub struct Script {
    /// The command that runs it, with `{file}` for its path.
    pub command: String,
    /// File extension, from the language of the fenced block.
    pub ext: String,
    pub body: String,
}

/// Reads the examiner's reply. `None` when it holds no script, which is also what `NONE` is.
pub fn parse(reply: &str) -> Option<Script> {
    let command = reply.lines().find_map(|l| l.trim().strip_prefix("COMMAND:"))?.trim().trim_matches('`').trim().to_string();
    if !command.contains("{file}") {
        return None;
    }
    let after = &reply[reply.find("```")? + 3..];
    let (lang, rest) = after.split_once('\n')?;
    let body = rest[..rest.rfind("```")?].trim_end();
    let ext = match lang.trim().to_ascii_lowercase().as_str() {
        "python" | "python3" | "py" => "py",
        "javascript" | "js" | "node" => "js",
        "typescript" | "ts" => "ts",
        "bash" | "sh" | "shell" => "sh",
        "ruby" | "rb" => "rb",
        "go" => "go",
        "rust" | "rs" => "rs",
        _ => "txt",
    };
    (!body.trim().is_empty()).then(|| Script { command, ext: ext.to_string(), body: body.to_string() })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tally {
    pub passed: u32,
    pub failed: u32,
}

impl Tally {
    pub fn total(&self) -> u32 {
        self.passed + self.failed
    }

    /// More than three cases in five fail. Against work whose own checks pass, that says more
    /// about the script than about the work.
    pub fn mostly_failing(&self) -> bool {
        self.failed * 5 > self.total() * 3
    }
}

static TALLY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"EXAM:\s*(\d+)\s+passed,\s*(\d+)\s+failed").expect("valid regex"));

/// The script's own count, from the last `EXAM:` line of its output. A script that did not get
/// that far did not run to its end.
pub fn tally(output: &str) -> Option<Tally> {
    let found = TALLY.captures_iter(output).last()?;
    Some(Tally { passed: found[1].parse().ok()?, failed: found[2].parse().ok()? })
}

/// The output of a failing run, cut to what a reply can take in: its start, where the failed cases
/// are, and its last lines, where the count is.
pub fn excerpt(output: &str) -> String {
    const HEAD: usize = 70;
    const TAIL: usize = 6;
    const CHARS: usize = 7_000;
    let lines: Vec<&str> = output.lines().collect();
    let mut text = if lines.len() > HEAD + TAIL {
        format!("{}\n[… {} lines left out …]\n{}", lines[..HEAD].join("\n"), lines.len() - HEAD - TAIL, lines[lines.len() - TAIL..].join("\n"))
    } else {
        lines.join("\n")
    };
    if text.len() > CHARS {
        let mut cut = CHARS;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text = format!("{}\n[… cut …]\n{}", &text[..cut], lines.last().copied().unwrap_or_default());
    }
    text
}

const RULE: &str = "A failing case is either a gap in your work or a mistake of the examiner. Settle each one by the text it cites, not by your code: where the text sides with the case, fix the work; where the text contradicts the case, leave the work as it is. Then call `finish` again, and name in its summary each case you leave failing with the sentence that shows it wrong.";

/// What the worker is told when `finish` is held, the `nth` time (from 1).
pub fn hold(nth: u8, tally: Tally, path: &str, cmd: &str, output: &str) -> String {
    let head = if nth == 1 {
        format!(
            "[examiner] The work is not accepted yet. A second engineer, who has not seen it, wrote checks from the task and its documents alone: {} pass, {} fail. The script is {path} (run it with `{cmd}`; do not edit it).",
            tally.passed, tally.failed
        )
    } else {
        format!("[examiner] Still not accepted: {} of the examiner's cases fail, and not as before (`{cmd}`).", tally.failed)
    };
    format!("{head}\n{}\n{RULE}", excerpt(output))
}

pub const ROUTER_SYSTEM: &str = "You route coding tasks to playbooks. You are given a list of playbooks and a task. Name the playbooks whose subject is the main work of the task. Most tasks need none, and a loose association is no reason to pick one. Answer with the names separated by commas, or with NONE. Nothing else.";

/// The router's request for `task`, with the playbooks as `(name, description)`.
pub fn route_brief(task: &str, playbooks: &[(&str, &str)]) -> Vec<Message> {
    let list: Vec<String> = playbooks.iter().map(|(name, about)| format!("- {name}: {about}")).collect();
    vec![Message::system(ROUTER_SYSTEM), Message::user(format!("Playbooks:\n{}\n\nTask:\n{task}", list.join("\n")))]
}

/// The names the router's answer picks out of `known`, at most two, in the order of `known`.
pub fn routed<'a>(answer: &str, known: &[&'a str]) -> Vec<&'a str> {
    let words: Vec<String> = answer.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')).map(|w| w.to_ascii_lowercase()).collect();
    known.iter().copied().filter(|name| words.iter().any(|w| w == &name.to_ascii_lowercase())).take(2).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_is_a_command_and_a_script_or_nothing() {
        let reply = "COMMAND: python3 {file}\n```python\nimport os\nprint('EXAM: 1 passed, 0 failed')\n```\n";
        let script = parse(reply).unwrap();
        assert_eq!((script.command.as_str(), script.ext.as_str()), ("python3 {file}", "py"));
        assert!(script.body.starts_with("import os") && script.body.ends_with("failed')"));
        assert_eq!(parse("NONE"), None);
        assert_eq!(parse("COMMAND: python3 exam.py\n```python\nx\n```"), None, "a command that does not say where the file goes");
        assert_eq!(parse("COMMAND: node {file}\n```js\n\n```"), None);
    }

    #[test]
    fn the_count_is_the_last_one_the_script_printed() {
        assert_eq!(tally("case a failed\nEXAM: 3 passed, 1 failed\n"), Some(Tally { passed: 3, failed: 1 }));
        assert_eq!(tally("EXAM: 0 passed, 9 failed\nEXAM: 30 passed, 2 failed"), Some(Tally { passed: 30, failed: 2 }));
        assert_eq!(tally("Traceback (most recent call last):\nImportError"), None);
        assert!(Tally { passed: 3, failed: 7 }.mostly_failing() && !Tally { passed: 4, failed: 6 }.mostly_failing());
    }

    #[test]
    fn a_long_output_keeps_its_start_and_its_count() {
        let long: String = (0..400).map(|i| format!("line {i}\n")).collect::<String>() + "EXAM: 1 passed, 399 failed";
        let cut = excerpt(&long);
        assert!(cut.starts_with("line 0\n") && cut.ends_with("EXAM: 1 passed, 399 failed") && cut.contains("lines left out"));
        assert!(cut.len() < 7_200);
    }

    #[test]
    fn the_router_picks_known_names_only() {
        assert_eq!(routed("ui", &["ui", "debug"]), ["ui"]);
        assert_eq!(routed("NONE", &["ui", "debug"]), Vec::<&str>::new());
        assert_eq!(routed("Debug, build-ui, tests.", &["ui", "debug", "tests", "security"]), ["debug", "tests"]);
    }
}
