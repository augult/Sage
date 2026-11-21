//! Expectations: the model predicts an outcome, the harness checks it mechanically.
//!
//! A confirmed prediction carries almost no information, so it is answered with a one-line receipt.
//! A violated one is the only thing worth the model's attention, so it halts the rest of the turn's
//! script and is reported in detail. See `script.rs` for how verdicts drive execution.

use regex::RegexBuilder;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ExitExpect {
    Code(i32),
    Nonzero,
    Any,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Expect {
    /// `None` means the default: exit code 0, or any exit code when only output text is predicted.
    pub exit: Option<ExitExpect>,
    pub has: Vec<String>,
    pub lacks: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Verdict {
    /// False when the model made no prediction and only the implicit "exit 0" applied.
    pub explicit: bool,
    pub held: bool,
    pub violations: Vec<String>,
}

fn parse_exit(v: &Value) -> Option<ExitExpect> {
    match v {
        Value::Number(n) => n.as_i64().map(|c| ExitExpect::Code(c as i32)),
        Value::Bool(true) => Some(ExitExpect::Code(0)),
        Value::Bool(false) => Some(ExitExpect::Nonzero),
        Value::String(s) => parse_exit_word(s),
        _ => None,
    }
}

fn parse_exit_word(s: &str) -> Option<ExitExpect> {
    let t = s.trim().to_ascii_lowercase();
    let t = t
        .strip_prefix("exit")
        .map(|r| r.trim_start_matches([' ', '=', ':', '_']))
        .unwrap_or(&t)
        .trim();
    if let Ok(c) = t.parse::<i32>() {
        return Some(ExitExpect::Code(c));
    }
    match t {
        "any" | "*" | "ignore" | "either" => return Some(ExitExpect::Any),
        "fail" | "fails" | "failure" | "error" | "errors" => return Some(ExitExpect::Nonzero),
        "pass" | "passes" | "ok" | "success" | "succeeds" | "zero" => {
            return Some(ExitExpect::Code(0));
        }
        _ => {}
    }
    // "non-zero", "not 0", "!= 0", "> 0", however it is spaced.
    let squashed: String = t
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_'))
        .collect();
    let negated = squashed.len() <= 12
        && ["nonzero", "not0", "notzero", "!=0", "!0", ">0", "≠0"]
            .iter()
            .any(|form| squashed.contains(form));
    negated.then_some(ExitExpect::Nonzero)
}

/// The exit a `expect_exit` value predicts. A number in it is the prediction ("code 2", "1 (usage
/// error)"). One that cannot be read at all predicts nothing that could be checked: holding it to
/// "exit 0" would call the model wrong for the harness's failure to read it.
fn stated_exit(v: &Value) -> Option<ExitExpect> {
    let number = |s: &str| {
        s.chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .ok()
            .map(ExitExpect::Code)
    };
    Some(
        parse_exit(v)
            .or_else(|| v.as_str().and_then(number))
            .unwrap_or(ExitExpect::Any),
    )
}

fn strings(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) if !s.is_empty() => vec![s.clone()],
        Value::Array(items) => items
            .iter()
            .filter_map(|i| i.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect(),
        Value::Number(n) => vec![n.to_string()],
        _ => vec![],
    }
}

/// GLM's native tool-call syntax sometimes leaks into a string where a nested object was meant:
/// `exit</arg_key><arg_value>nonzero`. Recover the key/value pairs.
pub fn unleak(s: &str) -> Option<Value> {
    const SEP: &str = "</arg_key><arg_value>";
    if !s.contains(SEP) {
        return None;
    }
    let mut map = serde_json::Map::new();
    for pair in s.split("</arg_value><arg_key>") {
        let (k, v) = pair.split_once(SEP)?;
        let k = k.trim().trim_start_matches("<arg_key>").trim();
        let v = v.trim().trim_end_matches("</arg_value>").trim();
        if k.is_empty() {
            return None;
        }
        map.insert(k.to_string(), Value::String(v.to_string()));
    }
    Some(Value::Object(map))
}

impl Expect {
    /// The prediction attached to a call: the flat `expect_exit` / `expect_has` / `expect_lacks`
    /// fields (what the schema advertises: flat strings survive every tool-call format), merged with a
    /// legacy nested `expect` if the model sent one.
    pub fn from_args(args: &Value) -> Option<Expect> {
        let mut e = args.get("expect").and_then(Expect::from_value);
        let present = |k: &str| {
            args.get(k)
                .filter(|v| !v.is_null() && v.as_str().is_none_or(|s| !s.trim().is_empty()))
        };
        if let Some(v) = present("expect_exit") {
            e.get_or_insert_with(Expect::default).exit = stated_exit(v);
        }
        if let Some(v) = present("expect_has") {
            e.get_or_insert_with(Expect::default).has.extend(strings(v));
        }
        if let Some(v) = present("expect_lacks") {
            e.get_or_insert_with(Expect::default)
                .lacks
                .extend(strings(v));
        }
        e
    }

    /// Lenient parse of whatever the model put in `expect`. Cheap models get this slightly wrong in
    /// predictable ways (a bare string, an alias for a field name), and rejecting a turn over that would
    /// cost far more than accepting it. Returns `None` when there is no usable prediction.
    pub fn from_value(v: &Value) -> Option<Expect> {
        match v {
            Value::Null => None,
            Value::Object(map) => {
                let mut e = Expect::default();
                for (k, val) in map {
                    match k.to_ascii_lowercase().as_str() {
                        "exit" | "exit_code" | "exitcode" | "code" | "status" | "returncode" => {
                            e.exit = stated_exit(val)
                        }
                        "has" | "contains" | "includes" | "output" | "stdout" | "match"
                        | "matches" => e.has.extend(strings(val)),
                        "lacks" | "not_contains" | "excludes" | "absent" | "not" | "without" => {
                            e.lacks.extend(strings(val))
                        }
                        _ => {}
                    }
                }
                Some(e)
            }
            // A bare word is either an exit prediction or free text we cannot check; free text still
            // counts as an explicit "I expect this to work".
            Value::String(s) if s.trim().is_empty() => None,
            Value::String(s) => match unleak(s) {
                Some(obj) => Expect::from_value(&obj),
                None => Some(Expect {
                    exit: parse_exit_word(s),
                    ..Default::default()
                }),
            },
            other => parse_exit(other).map(|x| Expect {
                exit: Some(x),
                ..Default::default()
            }),
        }
    }

    /// The exit code this prediction is about. One that names only output text leaves the exit code
    /// open: a model expecting a failure often says just what the failure will print, and holding it
    /// to "exit 0" would call a correct prediction a surprise. (Whether a run counts as a passing
    /// check is decided on its real exit code, in `script.rs`.)
    fn exit_expected(&self) -> ExitExpect {
        self.exit
            .unwrap_or(if self.has.is_empty() && self.lacks.is_empty() {
                ExitExpect::Code(0)
            } else {
                ExitExpect::Any
            })
    }

    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        match (self.exit, self.exit_expected()) {
            (None, ExitExpect::Any) => {}
            (_, ExitExpect::Code(c)) => parts.push(format!("exit {c}")),
            (_, ExitExpect::Nonzero) => parts.push("exit ≠ 0".to_string()),
            (_, ExitExpect::Any) => parts.push("any exit".to_string()),
        }
        parts.extend(self.has.iter().map(|h| format!("has {h:?}")));
        parts.extend(self.lacks.iter().map(|l| format!("lacks {l:?}")));
        parts.join(" · ")
    }
}

/// The regex a needle written `/pattern/` (with optional `i`, `m`, `s` flags) stands for. A path
/// such as `/usr/lib/` or `/api/v1/` is not one: a slash inside a regex between slashes would be
/// escaped, so one that is not makes the needle plain text.
fn needle_regex(needle: &str) -> Option<regex::Regex> {
    if needle.len() <= 2 || !needle.starts_with('/') {
        return None;
    }
    let end = needle.rfind('/').filter(|e| *e > 0)?;
    let (pat, flags) = (&needle[1..end], &needle[end + 1..]);
    if !flags.chars().all(|c| matches!(c, 'i' | 'm' | 's')) {
        return None;
    }
    let mut escaped = false;
    for c in pat.chars() {
        match c {
            '/' if !escaped => return None,
            '\\' => escaped = !escaped,
            _ => escaped = false,
        }
    }
    RegexBuilder::new(pat)
        .case_insensitive(flags.contains('i'))
        .multi_line(true)
        .dot_matches_new_line(flags.contains('s'))
        .build()
        .ok()
}

/// `/pattern/` is a regex; anything else is a case-insensitive substring.
fn output_contains(output: &str, needle: &str) -> bool {
    match needle_regex(needle) {
        Some(re) => re.is_match(output),
        None => output.to_lowercase().contains(&needle.to_lowercase()),
    }
}

/// A needle as a violation names it: a regex is said to be one, so that `/src/` failing on a line
/// that holds `src` but no `/src/` is not a mystery.
fn shown_needle(needle: &str) -> String {
    match needle_regex(needle) {
        Some(_) => format!("{needle:?} (read as a regex)"),
        None => format!("{needle:?}"),
    }
}

/// Checks a receipt without treating an excerpt as complete evidence.
pub fn check_capture(
    expect: Option<&Expect>,
    exit: Option<i32>,
    text: &str,
    complete: bool,
) -> Verdict {
    let Some(e) = expect.filter(|e| !complete && e.has_output_predicates()) else {
        return check(expect, exit, text);
    };
    // Only part of the output is at hand. The exit code is known, and text the part holds is
    // there; text it does not hold may be in what is missing, and so may text it must not hold.
    let mut verdict = check(
        Some(&Expect {
            exit: Some(e.exit_expected()),
            ..Expect::default()
        }),
        exit,
        text,
    );
    verdict.violations.extend(
        e.lacks
            .iter()
            .filter(|l| output_contains(text, l))
            .map(|l| {
                format!(
                    "expected output not to contain {}, but it does",
                    shown_needle(l)
                )
            }),
    );
    let unknown = e.lacks.iter().any(|l| !output_contains(text, l))
        || e.has.iter().any(|h| !output_contains(text, h));
    if unknown && verdict.violations.is_empty() {
        verdict
            .violations
            .push("output was incomplete, so its prediction could not be verified".into());
    }
    verdict.held = verdict.violations.is_empty();
    verdict
}

impl Expect {
    pub fn has_output_predicates(&self) -> bool {
        !self.has.is_empty() || !self.lacks.is_empty()
    }
}

/// Checks an outcome against a prediction. `exit` is `None` while a command is still running.
pub fn check(expect: Option<&Expect>, exit: Option<i32>, output: &str) -> Verdict {
    let explicit = expect.is_some();
    let default = Expect::default();
    let e = expect.unwrap_or(&default);
    let mut violations = Vec::new();

    let got = match exit {
        Some(c) => format!("exit {c}"),
        None => "no exit (still running or killed)".to_string(),
    };
    match (e.exit_expected(), exit) {
        (ExitExpect::Any, _) => {}
        (ExitExpect::Code(want), Some(c)) if c == want => {}
        (ExitExpect::Nonzero, Some(c)) if c != 0 => {}
        (ExitExpect::Code(want), _) => violations.push(format!("expected exit {want}, got {got}")),
        (ExitExpect::Nonzero, _) => violations.push(format!("expected a non-zero exit, got {got}")),
    }
    for h in &e.has {
        if !output_contains(output, h) {
            violations.push(format!(
                "expected output to contain {}, it does not",
                shown_needle(h)
            ));
        }
    }
    for l in &e.lacks {
        if output_contains(output, l) {
            violations.push(format!(
                "expected output not to contain {}, but it does",
                shown_needle(l)
            ));
        }
    }
    Verdict {
        explicit,
        held: violations.is_empty(),
        violations,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_object_string_and_scalar_forms() {
        let e =
            Expect::from_value(&json!({"exit": 0, "has": "passed", "lacks": ["FAILED", "Error"]}))
                .unwrap();
        assert_eq!(e.exit, Some(ExitExpect::Code(0)));
        assert_eq!(e.has, vec!["passed"]);
        assert_eq!(e.lacks.len(), 2);

        assert_eq!(
            Expect::from_value(&json!({"exit": "nonzero"}))
                .unwrap()
                .exit,
            Some(ExitExpect::Nonzero)
        );
        assert_eq!(
            Expect::from_value(&json!({"exit_code": "2", "contains": "x"}))
                .unwrap()
                .exit,
            Some(ExitExpect::Code(2))
        );
        assert_eq!(
            Expect::from_value(&json!("exit 1")).unwrap().exit,
            Some(ExitExpect::Code(1))
        );
        assert_eq!(
            Expect::from_value(&json!("fails")).unwrap().exit,
            Some(ExitExpect::Nonzero)
        );
        assert_eq!(
            Expect::from_value(&json!("any")).unwrap().exit,
            Some(ExitExpect::Any)
        );
        assert_eq!(
            Expect::from_value(&json!(3)).unwrap().exit,
            Some(ExitExpect::Code(3))
        );
        assert!(Expect::from_value(&json!(null)).is_none());
        assert!(Expect::from_value(&json!("")).is_none());
        // Uncheckable free text still counts as a prediction of success.
        let free = Expect::from_value(&json!("all tests are green")).unwrap();
        assert_eq!(free, Expect::default());
    }

    #[test]
    fn flat_fields_and_leaked_glm_syntax_are_understood() {
        let e = Expect::from_args(
            &json!({"cmd": "x", "expect_exit": "nonzero", "expect_has": "FAILED"}),
        )
        .unwrap();
        assert_eq!(
            (e.exit, e.has.as_slice()),
            (Some(ExitExpect::Nonzero), &["FAILED".to_string()][..])
        );
        assert!(Expect::from_args(&json!({"cmd": "x"})).is_none());
        assert!(Expect::from_args(&json!({"cmd": "x", "expect_has": ""})).is_none());
        assert_eq!(
            Expect::from_args(&json!({"expect_lacks": "Error"}))
                .unwrap()
                .describe(),
            "lacks \"Error\""
        );

        // An exit prediction is read however it is worded; one that cannot be read is not held
        // against the model as "exit 0".
        let exit = |said: &str| {
            Expect::from_args(&json!({"expect_exit": said}))
                .unwrap()
                .exit
        };
        for said in ["not 0", "non zero", "Non-Zero", "!= 0", "exit != 0"] {
            assert_eq!(exit(said), Some(ExitExpect::Nonzero), "{said}");
        }
        assert_eq!(exit("code 2"), Some(ExitExpect::Code(2)));
        assert_eq!(exit("1 (usage error)"), Some(ExitExpect::Code(1)));
        assert_eq!(exit("whatever the linter says"), Some(ExitExpect::Any));
        assert!(
            check(
                Expect::from_args(&json!({"expect_exit": "not 0"})).as_ref(),
                Some(1),
                ""
            )
            .held
        );

        // Seen live from GLM-5.3: the nested object arrives as a string in its native call syntax.
        let e = Expect::from_args(&json!({"expect": "exit</arg_key><arg_value>nonzero"})).unwrap();
        assert_eq!(e.exit, Some(ExitExpect::Nonzero));
        let e = Expect::from_args(&json!({"expect": "has</arg_key><arg_value>OK</arg_value><arg_key>exit</arg_key><arg_value>0"})).unwrap();
        assert_eq!(
            (e.exit, e.has.as_slice()),
            (Some(ExitExpect::Code(0)), &["OK".to_string()][..])
        );
        assert!(unleak("plain text").is_none());
    }

    #[test]
    fn implicit_expectation_is_exit_zero() {
        let v = check(None, Some(0), "");
        assert!(v.held && !v.explicit);
        let v = check(None, Some(2), "");
        assert!(!v.held);
        assert_eq!(v.violations, vec!["expected exit 0, got exit 2"]);
    }

    #[test]
    fn a_prediction_about_the_output_alone_leaves_the_exit_code_open() {
        // Seen live: GLM-5.3, expecting a failing test run, said only that the output would lack "OK".
        let e = Expect::from_args(&json!({"expect_lacks": "OK"})).unwrap();
        assert!(check(Some(&e), Some(1), "FAILED (failures=3)").held);
        assert!(!check(Some(&e), Some(0), "OK").held);
        // A failed run can still never pass as a check: that is decided on its exit code, in `script.rs`.
        let e = Expect::from_value(&json!({"has": "passing"})).unwrap();
        assert!(check(Some(&e), Some(1), "5 passing, 1 failing").held);
        // Naming an exit code makes it part of the prediction again.
        let e = Expect::from_value(&json!({"exit": 0, "has": "passing"})).unwrap();
        assert!(!check(Some(&e), Some(1), "5 passing, 1 failing").held);
    }

    #[test]
    fn has_and_lacks_are_case_insensitive_substrings_or_regex() {
        let e = Expect::from_value(&json!({"has": "PASSED", "lacks": "/\\bfail(ed|ure)\\b/i"}))
            .unwrap();
        assert!(check(Some(&e), Some(0), "12 passed in 0.3s").held);
        let v = check(Some(&e), Some(0), "11 passed, 1 FAILED");
        assert_eq!(v.violations.len(), 1);
        assert!(v.violations[0].contains("not to contain"));
    }

    #[test]
    fn nonzero_any_and_running() {
        let fail = Expect {
            exit: Some(ExitExpect::Nonzero),
            ..Default::default()
        };
        assert!(check(Some(&fail), Some(1), "").held);
        assert!(!check(Some(&fail), Some(0), "").held);
        let any = Expect {
            exit: Some(ExitExpect::Any),
            ..Default::default()
        };
        assert!(check(Some(&any), None, "").held);
        assert!(
            !check(None, None, "").held,
            "a command that never exited did not meet 'exit 0'"
        );
    }

    #[test]
    fn describe_is_compact() {
        let e = Expect::from_value(&json!({"has": "ok"})).unwrap();
        assert_eq!(e.describe(), "has \"ok\"");
        let e = Expect::from_value(&json!({"exit": 0, "has": "ok"})).unwrap();
        assert_eq!(e.describe(), "exit 0 · has \"ok\"");
        assert_eq!(Expect::default().describe(), "exit 0");
    }

    #[test]
    fn part_of_the_output_proves_what_it_shows_and_nothing_it_does_not() {
        let e = |has: &[&str], lacks: &[&str]| Expect {
            exit: None,
            has: has.iter().map(|s| s.to_string()).collect(),
            lacks: lacks.iter().map(|s| s.to_string()).collect(),
        };
        let part = "server ready\n";
        // What the part holds is there, whatever is missing.
        assert!(check_capture(Some(&e(&["ready"], &[])), Some(0), part, false).held);
        // What it does not hold, and what must be absent, may be in the rest.
        for unknown in [e(&["done"], &[]), e(&["ready"], &["Traceback"])] {
            let v = check_capture(Some(&unknown), Some(0), part, false);
            assert!(!v.held);
            assert_eq!(
                v.violations,
                ["output was incomplete, so its prediction could not be verified"]
            );
        }
        // Text that must be absent and is in the part is a violation, as is a wrong exit code.
        let v = check_capture(Some(&e(&[], &["ready"])), Some(0), part, false);
        assert_eq!(
            v.violations,
            ["expected output not to contain \"ready\", but it does"]
        );
        let exit_zero = Expect {
            exit: Some(ExitExpect::Code(0)),
            ..e(&["ready"], &[])
        };
        let v = check_capture(Some(&exit_zero), Some(2), part, false);
        assert_eq!(v.violations, ["expected exit 0, got exit 2"]);
    }

    #[test]
    fn a_path_between_slashes_is_text_and_a_word_between_slashes_is_a_regex() {
        // A slash inside means a path: `/src/main/` is looked for as it is.
        assert!(!output_contains("build/src_gen/main.o", "/src/main/"));
        assert!(output_contains("at /app/src/main/x.rs", "/src/main/"));
        assert!(needle_regex("/usr/lib/").is_none() && needle_regex(r"/src\/main/").is_some());
        // One word between slashes is a regex, as the tool's schema says, and a violation says so.
        assert!(output_contains("3 passed", "/pass(ed)?/"));
        let v = check(
            Some(&Expect {
                exit: None,
                has: vec![],
                lacks: vec!["/src/".into()],
            }),
            Some(0),
            "build/src_gen/main.o",
        );
        assert_eq!(
            v.violations,
            ["expected output not to contain \"/src/\" (read as a regex), but it does"]
        );
    }
}
