//! The examiners and the referees: second readers of the task.
//!
//! A model that has implemented a specification tests its own reading of it, so whatever it
//! misread, its checks misread too. An examiner is a separate request with a fresh context that
//! sees the task and the same files, never the work, and writes one check script from the text
//! alone. It is asked for when the run starts and answers beside it, so it costs a request and no
//! waiting. When the worker is about to end its run, the script is run against the work.
//!
//! A script written blind is wrong often enough to do harm: handed a wrong case, a worker breaks
//! work that was right. So a failing case holds nothing by itself. It goes to referees, who read
//! the text again, each alone and without the work, and only a case that every one of them finds
//! the text to side with reaches the worker. What the worker does with it is still its own
//! ruling, by the text.

use crate::provider::Message;
use crate::util::truncate_chars;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

/// What an examiner reads the text for. Two briefs cover more of it than one brief asked twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Angle {
    /// The text rule by rule: a case for everything it states.
    Rules,
    /// What such a pass leaves out: errors and how they are reported, limits, rules that meet.
    Edges,
}

impl Angle {
    /// As the script's file and the log name it.
    pub fn label(self) -> &'static str {
        match self {
            Angle::Rules => "rules",
            Angle::Edges => "edges",
        }
    }
}

const RULES_INTRO: &str = "You are the examiner of a coding task that another engineer is working on right now. You never see their work. From the task and the files shown below you write one check script that tells whether the finished work does everything the task, and every document the task names, requires.";

const RULES_BRIEF: &str = "- Go through the task and each document it names sentence by sentence. Every rule, every worked example and every stated error case becomes at least one case, and so does every interaction between rules that the text defines. Expected values come from the text alone. Name each case after the clause it checks (for example `s5_pieces_joined_with_one_space`).";

const EDGES_INTRO: &str = "You are the second examiner of a coding task that another engineer is working on right now. You never see their work. From the task and the files shown below you write one check script for what the task, and every document the task names, requires where implementations most often go wrong.";

const EDGES_BRIEF: &str = "- The first examiner goes through the text rule by rule, with a plain case for each. Yours are the cases such a pass leaves out. Every error the text states, in every place it can occur and not only the first, with all the text says about how it is reported: its kind, its message, the position or line it names. The limits of each rule: nothing, exactly one, the first and the last, a value on the boundary, the same thing twice. Two rules meeting in one input, wherever the text says which applies first or which wins. Expected values come from the text alone. Name each case after the clauses it checks (for example `s2_s6_discount_before_rounding`). If the text gives you nothing of this kind to check, reply NONE: do not fall back on plain cases.";

const FORMAT: &str = "\
- Write the script as you go through the text. Do not work every case out before you begin to write: a reply that is all thought and no script is lost.
- Where the text leaves a point open or you are not sure what it requires, write no case. A wrong case costs more than a missing one.
- Cover what the existing tests leave out; do not repeat them.
- Use only the interface that the task, the documents and the existing code fix: names, signatures, files, commands. Never guess one.
- The script is stored outside the project and run from the project's root directory, so it reaches the project's code relative to the working directory (in Python: `sys.path.insert(0, os.getcwd())`). It uses the project's language and nothing that is not already installed. It changes no file of the project; anything it needs to write goes into a temporary directory.
- Every case runs on its own: a case that raises or crashes counts as failed and the others still run. Load the project's code inside the cases, so that code that is missing or does not load fails them and does not stop the script.
- Print nothing for a case that passes. For each failed case print one block: first a line `FAIL <name>`, then on lines of their own the clause in a few words, the input, what the text requires and what came back. The last line printed is exactly `EXAM: <passed> passed, <failed> failed`. The exit status is 0 only if no case failed.
- Lines of the files shown may begin with a short anchor such as `41kq`; it is not part of the file.

If the task is a question, an investigation or a review, or if nothing about its result can be checked by a script, reply with the single word NONE.

Otherwise reply with exactly this and nothing else:
COMMAND: <the command that runs the script, with {file} where its path goes>
```<language>
<the script>
```";

static SYSTEMS: LazyLock<[String; 2]> = LazyLock::new(|| [(RULES_INTRO, RULES_BRIEF), (EDGES_INTRO, EDGES_BRIEF)].map(|(intro, brief)| format!("{intro}\n\nHow to write it:\n{brief}\n{FORMAT}")));

/// An examiner's instructions.
pub fn system(angle: Angle) -> &'static str {
    match angle {
        Angle::Rules => &SYSTEMS[0],
        Angle::Edges => &SYSTEMS[1],
    }
}

const ASK: &str = "Write the check script for this task now.";

/// An examiner's request: its instructions, then what the worker was shown at the start.
pub fn brief(angle: Angle, opening: &str) -> Vec<Message> {
    vec![Message::system(system(angle)), Message::user(format!("{opening}\n\n{ASK}"))]
}

#[derive(Debug, PartialEq, Eq)]
pub struct Script {
    /// The command that runs it, with `{file}` for its path.
    pub command: String,
    /// File extension, from the language of the fenced block.
    pub ext: String,
    pub body: String,
}

/// Reads an examiner's reply. `None` when it holds no script, which is also what `NONE` is.
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

/// A failed case as its script reported it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Case {
    pub name: String,
    /// Its block of the output, from the `FAIL` line on, cut to what a reply can take in.
    pub text: String,
}

static FAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*FAIL(?:ED)?[:\s]\s*(\w\S*)").expect("valid regex"));

const CASE_LINES: usize = 14;
const CASE_CHARS: usize = 1_200;

/// The failed cases of a run: the blocks of its output that open with `FAIL <name>`. Empty where
/// the script did not report them that way, so that they cannot be told apart.
pub fn failed_cases(output: &str) -> Vec<Case> {
    let mut blocks: Vec<(String, Vec<&str>)> = Vec::new();
    let mut open = false;
    for line in output.lines() {
        if TALLY.is_match(line) {
            open = false;
        } else if let Some(found) = FAIL.captures(line) {
            let name = found[1].trim_end_matches([':', ',', '`', '"', '\'', ')']).to_string();
            // The same name twice is two cases to rule on.
            let repeats = blocks.iter().filter(|(known, _)| *known == name || known.strip_prefix(name.as_str()).is_some_and(|rest| rest.starts_with('#'))).count();
            blocks.push((if repeats == 0 { name } else { format!("{name}#{}", repeats + 1) }, vec![line]));
            open = true;
        } else if open && let Some((_, lines)) = blocks.last_mut() {
            lines.push(line);
        }
    }
    blocks
        .into_iter()
        .map(|(name, mut lines)| {
            while lines.last().is_some_and(|l| l.trim().is_empty()) {
                lines.pop();
            }
            let left_out = lines.len().saturating_sub(CASE_LINES);
            let mut text = lines.iter().take(CASE_LINES).map(|l| l.trim_end()).collect::<Vec<_>>().join("\n");
            if left_out > 0 {
                text.push_str(&format!("\n[… {left_out} more lines …]"));
            }
            Case { name, text: truncate_chars(&text, CASE_CHARS) }
        })
        .collect()
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

const SAME_FORMAT: &str = "Reply in the same format as before, with the whole script.";

/// What an examiner is told when its script did not get as far as its count.
pub fn repair_broken(output: &str) -> String {
    let shown = if output.trim().is_empty() { "(it printed nothing, or ran for too long)".to_string() } else { excerpt(output) };
    format!(
        "Your script was run against the finished work and did not run to its end: it printed no `EXAM:` line. Its output:\n{shown}\n\nWhere the fault is in the script, correct it. Where the output shows that the work lacks something the task fixes by name (a file, a function, a command), keep the cases and let each of them fail on its own instead of stopping the script. {SAME_FORMAT}"
    )
}

/// What an examiner is told when most of its cases fail against work whose own checks pass.
pub fn repair_doubted(tally: Tally, output: &str) -> String {
    format!(
        "Your script was run against the finished work, which passes the project's own checks: {} of your {} cases fail. When most cases fail, the usual cause is the script: how it reaches or calls the code, or how it compares results. Its output:\n{}\n\nCheck the script against the task and the files you were shown, and correct it where it is at fault. Do not change what a case expects in order to make it pass: expected values come from the text alone, and what the work returned is no evidence of what is right. {SAME_FORMAT}",
        tally.failed,
        tally.total(),
        excerpt(output)
    )
}

/// What an examiner is told when its failed cases cannot be told apart.
pub fn repair_unlabelled(output: &str) -> String {
    format!(
        "Your script reports failed cases, but their blocks do not each start with a line `FAIL <name>`, so they cannot be told apart. Its output:\n{}\n\nChange the script in that alone. {SAME_FORMAT}",
        excerpt(output)
    )
}

/// What a referee finds about a failing case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ruling {
    /// The text settles the point and sides with the case: the work has a gap.
    Gap,
    /// The case is at fault.
    Case,
    /// The text does not settle the point.
    Open,
}

pub const REFEREE_SYSTEM: &str = "\
You are a referee in a coding task. An engineer has finished the task. An examiner, who never saw the work, wrote check cases from the task's text alone, and some of them fail against the work. Either the work has a gap there or the case is wrong. You decide which, and the text is the only authority: the task and the documents shown below. You do not see the work. What it returned is no evidence of what is right, and neither is what the case expected.

For each failed case:
1. Read in the script what exactly the case does, and find every sentence of the task and its documents that bears on its input.
2. Work out from those sentences alone what the result has to be.
3. Only then compare it with what the case expected and with what came back.

Rule one of:
- GAP: the text settles the point, and what it requires is what the case expected. Quote the sentence that settles it.
- CASE: the text requires something other than the case expected; or the case reaches the code in a way the task and the files do not fix (a wrong name, signature, path or command); or the case is faulty in itself.
- OPEN: the text does not settle the point, or you cannot tell. When in doubt, this.

A wrong GAP makes the engineer break work that was right, which costs more than a gap nobody notices. Rule GAP only where the quoted sentence alone would convince a reader who has not seen the case.

Reply with one line for each failed case and nothing else:
<name of the case>: GAP|CASE|OPEN: <the sentence quoted, or the reason in a few words>";

/// A referee's request: its instructions, what the worker was shown at the start, the examiner's
/// script and the cases to rule on.
pub fn referee_brief(opening: &str, script: &str, cases: &[&Case]) -> Vec<Message> {
    let blocks: Vec<&str> = cases.iter().map(|c| c.text.as_str()).collect();
    let user = format!("{opening}\n\n<script>\n{}\n</script>\n\n<failed_cases>\n{}\n</failed_cases>\n\nRule on each failed case now.", script.trim_end(), blocks.join("\n\n"));
    vec![Message::system(REFEREE_SYSTEM), Message::user(user)]
}

/// Bullets, numbering, emphasis and quotes: what a model puts in front of a name.
fn bare(line: &str) -> &str {
    line.trim_start_matches(|c: char| c.is_whitespace() || "*_#>`\"'-[(\u{2022}".contains(c))
}

const MAX_GROUND_CHARS: usize = 400;

/// What a referee's reply rules on each of `names`, with the sentence it quotes or its reason. A
/// case the reply has no readable line for is not in the result.
pub fn rulings(reply: &str, names: &[String]) -> HashMap<String, (Ruling, String)> {
    let named = |line: &str| -> Option<(String, usize)> {
        names.iter().filter(|n| line.starts_with(n.as_str()) && !line[n.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_')).max_by_key(|n| n.len()).map(|n| (n.clone(), n.len()))
    };
    let mut found = HashMap::new();
    for line in reply.lines() {
        let line = bare(line);
        // A numbered list: but only where the number is not the start of a name.
        let unnumbered = bare(line.trim_start_matches(|c: char| c.is_ascii_digit()).trim_start_matches(['.', ')']));
        let Some((name, line)) = named(line).map(|(n, len)| (n, &line[len..])).or_else(|| named(unnumbered).map(|(n, len)| (n, &unnumbered[len..]))) else { continue };
        let rest = line.trim_start_matches(|c: char| c.is_whitespace() || "`*_\"':=>-\u{2013}\u{2014}".contains(c));
        // ASCII case only, so that byte positions in `upper` are those of `rest`.
        let upper = rest.to_ascii_uppercase();
        let Some((ruling, len)) = [("GAP", Ruling::Gap), ("CASE", Ruling::Case), ("OPEN", Ruling::Open)]
            .into_iter()
            .find_map(|(word, ruling)| (upper.starts_with(word) && !upper[word.len()..].starts_with(|c: char| c.is_ascii_alphanumeric())).then_some((ruling, word.len())))
        else {
            continue;
        };
        let ground = rest[len..].trim_matches(|c: char| c.is_whitespace() || "`*_:-\u{2013}\u{2014}".contains(c));
        found.entry(name).or_insert((ruling, truncate_chars(ground, MAX_GROUND_CHARS)));
    }
    found
}

/// A failing case that holds a `finish`.
pub struct Held<'a> {
    pub case: &'a Case,
    /// What each referee quoted for it; empty where none was asked.
    pub grounds: &'a [String],
    /// It passed the last time the script ran: what the worker changed since broke it.
    pub broken: bool,
}

/// One examiner's script, and what of its run the worker is shown.
pub struct Section<'a> {
    pub path: &'a str,
    pub cmd: &'a str,
    pub tally: Tally,
    pub held: Vec<Held<'a>>,
    /// The output as it is, where its failed cases could not be told apart.
    pub raw: Option<&'a str>,
    /// Failing cases the referees did not confirm, by name.
    pub set_aside: Vec<&'a str>,
}

const RULE: &str = "A failing case is either a gap in your work or a mistake of the examiner. Settle each one by the text it cites, not by your code: where the text sides with the case, fix the work; where the text contradicts the case, leave the work as it is. Whatever passes now has to pass afterwards. Then call `finish` again, and name in its summary each case you leave failing with the sentence that shows it wrong.";

const RULE_REFEREED: &str = "Each of these is taken to be a gap in your work. Read the quoted sentence and the text around it again, and fix the work where the text requires what the case expects. A case can still be wrong: where the text contradicts it, leave the work as it is, and change nothing only to make a case pass. Whatever passes now has to pass afterwards. Then call `finish` again, and name in its summary each case you leave failing with the sentence that shows it wrong.";

/// The cases of a hold are shown up to about this many characters; the script has the rest.
const HOLD_CHARS: usize = 9_000;

/// What the worker is told when `finish` is held, the `nth` time (from 1). `examiners` is how many
/// were asked; `refereed` says that the cases shown are the ones the referees confirmed.
pub fn hold(nth: u8, examiners: usize, refereed: bool, sections: &[Section]) -> String {
    let held: usize = sections.iter().map(|s| if s.raw.is_some() { s.tally.failed as usize } else { s.held.len() }).sum();
    let (passed, failed) = sections.iter().fold((0, 0), |(p, f), s| (p + s.tally.passed, f + s.tally.failed));
    let authors = if examiners == 1 { "A second engineer, who has not seen it," } else { "Two more engineers, who have not seen it," };
    let mut out = match (nth, refereed) {
        (1, true) => format!(
            "[examiner] The work is not accepted yet. {authors} wrote checks from the task and its documents alone. Of the cases that fail, {held} {} confirmed by two more readers of the text, each on their own: under each case is what they quote.",
            if held == 1 { "was" } else { "were" }
        ),
        (1, false) => format!("[examiner] The work is not accepted yet. {authors} wrote checks from the task and its documents alone: {passed} pass, {failed} fail."),
        _ => "[examiner] Still not accepted. Among the cases below is at least one that you have not been shown failing, or not failing the way it does now.".to_string(),
    };
    let mut room = HOLD_CHARS;
    let mut left_out = 0usize;
    for section in sections.iter().filter(|s| !s.held.is_empty() || s.raw.is_some()) {
        out.push_str(&format!("\n\nScript {} (run it with `{}`; do not edit it):", section.path, section.cmd));
        if let Some(raw) = section.raw {
            out.push('\n');
            out.push_str(&excerpt(raw));
        }
        for held in &section.held {
            let mut block = held.case.text.clone();
            if held.broken {
                block.push_str("\n  This case passed before your last changes.");
            }
            for ground in held.grounds.iter().filter(|g| !g.trim().is_empty()) {
                block.push_str(&format!("\n  referee: {ground}"));
            }
            if block.len() > room {
                left_out += 1;
                continue;
            }
            room -= block.len();
            out.push('\n');
            out.push_str(&block);
        }
        if !section.set_aside.is_empty() {
            out.push_str(&format!("\nFailing too, but not confirmed against the text. Leave these alone; the script goes on reporting them: {}.", section.set_aside.join(", ")));
        }
    }
    if left_out > 0 {
        out.push_str(&format!("\n[… {left_out} more failing cases of this kind: run the scripts to see them …]"));
    }
    format!("{out}\n\n{}", if refereed { RULE_REFEREED } else { RULE })
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
    fn the_two_examiners_are_briefed_differently_and_report_alike() {
        let (rules, edges) = (system(Angle::Rules), system(Angle::Edges));
        assert!(rules.starts_with("You are the examiner") && rules.contains("sentence by sentence"));
        assert!(edges.starts_with("You are the second examiner") && edges.contains("The limits of each rule") && !edges.contains("sentence by sentence"));
        for system in [rules, edges] {
            assert!(system.contains("first a line `FAIL <name>`") && system.contains("EXAM: <passed> passed, <failed> failed") && system.ends_with("<the script>\n```"), "{system}");
        }
        let asked = brief(Angle::Edges, "<task>\nFix it.\n</task>");
        assert_eq!((asked[0].content.as_str(), asked[1].content.as_str()), (edges, "<task>\nFix it.\n</task>\n\nWrite the check script for this task now."));
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
    fn failed_cases_are_the_blocks_that_open_with_fail() {
        let output = "loading\nFAIL s2_empty_name\n  clause: a name is not empty\n  input: []\n  expected: error\n  got: {}\n\nFAIL: s5_last_line,\n  got: ok\nEXAM: 7 passed, 2 failed\ntrailing noise";
        let cases = failed_cases(output);
        assert_eq!(cases.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["s2_empty_name", "s5_last_line"]);
        assert_eq!(cases[0].text, "FAIL s2_empty_name\n  clause: a name is not empty\n  input: []\n  expected: error\n  got: {}");
        assert_eq!(cases[1].text, "FAIL: s5_last_line,\n  got: ok");

        // What a test runner prints of its own is read the same way; its summary is no case.
        let unittest = "FAIL: test_dollars (tests.Refs)\nAssertionError: '5$' != '5'\nFAILED (failures=1)\nEXAM: 3 passed, 1 failed";
        assert_eq!(failed_cases(unittest).iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["test_dollars"]);
        // A name used twice is two cases; a long block is cut; output without the marker has none.
        let twice = failed_cases("FAIL a\nFAIL a\nFAIL a\nEXAM: 0 passed, 3 failed");
        assert_eq!(twice.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["a", "a#2", "a#3"]);
        let long = format!("FAIL big\n{}EXAM: 0 passed, 1 failed", "x\n".repeat(40));
        assert!(failed_cases(&long)[0].text.ends_with("[… 27 more lines …]"), "{}", failed_cases(&long)[0].text);
        assert!(failed_cases("case a failed\nEXAM: 1 passed, 1 failed").is_empty());
    }

    #[test]
    fn a_referee_rules_case_by_case() {
        let names: Vec<String> = ["s5_last", "s5_last_line", "1_endings"].iter().map(|n| n.to_string()).collect();
        let reply = "Here are my rulings.\n- `s5_last_line`: **GAP** — \"A backslash at the end of the last line of the file is an error.\"\n2. s5_last: open: the text does not say\n1_endings: CASE: the case passes a path where the task fixes a string\nunknown_case: GAP: x\ns5_last_line: CASE: a second line for the same case is not read";
        let ruled = rulings(reply, &names);
        assert_eq!(ruled["s5_last_line"], (Ruling::Gap, "\"A backslash at the end of the last line of the file is an error.\"".to_string()));
        assert_eq!(ruled["s5_last"], (Ruling::Open, "the text does not say".to_string()));
        assert_eq!(ruled["1_endings"].0, Ruling::Case);
        assert_eq!(ruled.len(), 3);
        // A line that rules nothing rules nothing: a word that only begins like a ruling, a name alone.
        assert!(rulings("s5_last: GAPS in the text\ns5_last_line", &names).is_empty());

        let cases = [Case { name: "a".into(), text: "FAIL a\n  got: 1".into() }, Case { name: "b".into(), text: "FAIL b".into() }];
        let asked = referee_brief("<task>\nT\n</task>", "print(1)\n", &cases.iter().collect::<Vec<_>>());
        assert_eq!(asked[0].content, REFEREE_SYSTEM);
        assert_eq!(asked[1].content, "<task>\nT\n</task>\n\n<script>\nprint(1)\n</script>\n\n<failed_cases>\nFAIL a\n  got: 1\n\nFAIL b\n</failed_cases>\n\nRule on each failed case now.");
    }

    #[test]
    fn a_hold_shows_what_holds_it() {
        let case = |name: &str| Case { name: name.into(), text: format!("FAIL {name}\n  got: nothing") };
        let (a, b) = (case("a"), case("b"));
        let grounds = ["\"It is an error.\"".to_string(), String::new()];

        // Unrefereed: every failing case, and the worker is to settle each by the text.
        let section = Section { path: "/s/exam_rules.py", cmd: "python3 '/s/exam_rules.py'", tally: Tally { passed: 5, failed: 2 }, held: vec![Held { case: &a, grounds: &[], broken: false }, Held { case: &b, grounds: &[], broken: true }], raw: None, set_aside: vec![] };
        let text = hold(1, 1, false, &[section]);
        assert!(text.starts_with("[examiner] The work is not accepted yet. A second engineer, who has not seen it, wrote checks from the task and its documents alone: 5 pass, 2 fail.\n\nScript /s/exam_rules.py (run it with `python3 '/s/exam_rules.py'`; do not edit it):\nFAIL a\n  got: nothing\nFAIL b\n  got: nothing\n  This case passed before your last changes.\n\nA failing case is either a gap"), "{text}");

        // Refereed: the confirmed ones with what was quoted, the others named as not to be acted on.
        let section = Section { path: "/s/exam_edges.py", cmd: "python3 '/s/exam_edges.py'", tally: Tally { passed: 5, failed: 2 }, held: vec![Held { case: &a, grounds: &grounds, broken: false }], raw: None, set_aside: vec!["b"] };
        let passing = Section { path: "/s/exam_rules.py", cmd: "x", tally: Tally { passed: 9, failed: 0 }, held: vec![], raw: None, set_aside: vec![] };
        let text = hold(1, 2, true, &[passing, section]);
        assert!(text.starts_with("[examiner] The work is not accepted yet. Two more engineers, who have not seen it, wrote checks from the task and its documents alone. Of the cases that fail, 1 was confirmed by two more readers of the text, each on their own: under each case is what they quote.\n\nScript /s/exam_edges.py"), "{text}");
        assert!(text.contains("FAIL a\n  got: nothing\n  referee: \"It is an error.\"\nFailing too, but not confirmed against the text. Leave these alone; the script goes on reporting them: b.\n\nEach of these is taken to be a gap") && !text.contains("exam_rules"), "{text}");

        // Later holds say so; output whose cases cannot be told apart is shown as it is; too many cases are counted.
        let raw = Section { path: "/s/e.sh", cmd: "sh '/s/e.sh'", tally: Tally { passed: 1, failed: 1 }, held: vec![], raw: Some("case x failed\nEXAM: 1 passed, 1 failed"), set_aside: vec![] };
        let text = hold(2, 1, false, &[raw]);
        assert!(text.starts_with("[examiner] Still not accepted. Among the cases below") && text.contains("do not edit it):\ncase x failed\nEXAM: 1 passed, 1 failed\n\nA failing case"), "{text}");
        let big = Case { name: "big".into(), text: "x".repeat(5_000) };
        let many = Section { path: "p", cmd: "c", tally: Tally { passed: 0, failed: 3 }, held: (0..3).map(|_| Held { case: &big, grounds: &[], broken: false }).collect(), raw: None, set_aside: vec![] };
        assert!(hold(1, 1, false, &[many]).contains("[… 2 more failing cases of this kind"));
    }

    #[test]
    fn an_examiner_is_told_what_came_of_its_script() {
        assert!(repair_broken("Traceback\nImportError").contains("printed no `EXAM:` line. Its output:\nTraceback\nImportError\n\n") && repair_broken("").contains("(it printed nothing, or ran for too long)"));
        let doubted = repair_doubted(Tally { passed: 1, failed: 4 }, "FAIL a\nEXAM: 1 passed, 4 failed");
        assert!(doubted.contains("4 of your 5 cases fail") && doubted.contains("Do not change what a case expects in order to make it pass") && doubted.ends_with("with the whole script."));
        assert!(repair_unlabelled("x").contains("`FAIL <name>`"));
    }

    #[test]
    fn the_router_picks_known_names_only() {
        assert_eq!(routed("ui", &["ui", "debug"]), ["ui"]);
        assert_eq!(routed("NONE", &["ui", "debug"]), Vec::<&str>::new());
        assert_eq!(routed("Debug, build-ui, tests.", &["ui", "debug", "tests", "security"]), ["debug", "tests"]);
    }
}
