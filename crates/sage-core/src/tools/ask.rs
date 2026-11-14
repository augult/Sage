//! `ask`: a question for the user, when the request leaves open something that changes the work
//! and that the workspace cannot settle (which language a new project is in, which of two readings
//! is meant). It ends the run; the answer comes back as the user's next message, picked from the
//! choices or written in their own words.

use super::str_arg;
use crate::events::Choice;
use serde_json::Value;

/// More than this and it is not a choice any more.
const MAX_CHOICES: usize = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    pub question: String,
    pub choices: Vec<Choice>,
}

/// What a choice says, under any of the names models give it.
fn text_of(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Reads the arguments: `question`, and `options` as plain strings or as `{ label, hint }`.
pub fn parse(args: &Value) -> Result<Question, String> {
    let question = str_arg(args, "question").unwrap_or("").trim().to_string();
    if question.is_empty() {
        return Err("`ask` needs a `question`".into());
    }
    let listed = args
        .get("options")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut choices: Vec<Choice> = listed
        .iter()
        .filter_map(|option| {
            let choice = match option {
                Value::String(label) => Choice {
                    label: label.trim().to_string(),
                    hint: String::new(),
                },
                Value::Object(_) => Choice {
                    label: text_of(
                        option,
                        &["label", "value", "title", "name", "text", "option"],
                    ),
                    hint: text_of(
                        option,
                        &["hint", "description", "note", "why", "recommended"],
                    ),
                },
                _ => return None,
            };
            (!choice.label.is_empty()).then_some(choice)
        })
        .collect();
    // The same answer twice is offered once.
    let mut seen = std::collections::HashSet::new();
    choices.retain(|choice| seen.insert(choice.label.to_lowercase()));
    if choices.len() > MAX_CHOICES {
        return Err(format!(
            "`ask` takes at most {MAX_CHOICES} options; keep the ones that matter"
        ));
    }
    Ok(Question { question, choices })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn options_come_as_strings_or_as_labels_with_hints() {
        let q = parse(&json!({"question": " Which language? ", "options": [{"label": "Rust", "hint": "Recommended for performance"}, "Python", {"title": "Go", "description": "quick to build"}, "", 7, "python"]})).unwrap();
        assert_eq!(q.question, "Which language?");
        let read: Vec<(&str, &str)> = q
            .choices
            .iter()
            .map(|c| (c.label.as_str(), c.hint.as_str()))
            .collect();
        assert_eq!(
            read,
            [
                ("Rust", "Recommended for performance"),
                ("Python", ""),
                ("Go", "quick to build")
            ]
        );
        assert!(
            parse(&json!({"question": "Free text?"}))
                .unwrap()
                .choices
                .is_empty()
        );
        assert!(parse(&json!({"options": ["a"]})).is_err());
        assert!(
            parse(&json!({"question": "q", "options": ["1", "2", "3", "4", "5", "6", "7"]}))
                .is_err()
        );
    }
}
