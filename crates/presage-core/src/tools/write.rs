//! The `write` tool: create a file, or deliberately replace one.

use super::edit::syntax_check;
use super::{StepOutput, StepStatus, bool_arg, str_arg};
use crate::state::Session;
use crate::util::fmt_bytes;
use serde_json::Value;

pub async fn run(sess: &mut Session, args: &Value) -> StepOutput {
    let Some(path_arg) = str_arg(args, "path").map(str::trim).filter(|p| !p.is_empty()) else {
        return StepOutput::error("`path` is required");
    };
    let Some(content) = str_arg(args, "content") else {
        return StepOutput::error("`content` is required: the full text of the file");
    };
    let abs = sess.ws.resolve(path_arg);
    let shown = sess.ws.display(&abs);
    if let Err(e) = sess.ws.check_writable(&abs) {
        return StepOutput { status: StepStatus::Denied, ..StepOutput::error(e) };
    }
    let existing = match std::fs::metadata(&abs) {
        Ok(md) if md.is_dir() => return StepOutput::error(format!("{shown} is a directory")),
        Ok(_) => std::fs::read(&abs).ok(),
        Err(_) => None,
    };
    if let Some(old) = &existing {
        if old.as_slice() == content.as_bytes() {
            return StepOutput::ok(format!("[ok] {shown}: no change — the file already has this content"));
        }
        // Overwriting by accident destroys work the model may never have read.
        if !old.is_empty() && !bool_arg(args, "overwrite") {
            let lines = old.iter().filter(|b| **b == b'\n').count();
            return StepOutput::error(format!("{shown} already exists ({lines} lines). Use edit to change it, or pass overwrite:true to replace the whole file."));
        }
    }
    if let Some(parent) = abs.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return StepOutput::error(format!("cannot create {}: {e}", sess.ws.display(parent)));
        }
    }
    if let Err(e) = std::fs::write(&abs, content) {
        return StepOutput::error(format!("cannot write {shown}: {e}"));
    }
    let lines = content.lines().count();
    let verb = if existing.is_some() { "replaced" } else { "wrote" };
    let broken = if sess.features.post_edit_check { syntax_check(&abs, content).await } else { None };
    let mut out = match broken {
        Some((line, msg)) => {
            let at = line.map(|l| format!(" at line {l}")).unwrap_or_default();
            let head = format!("[SURPRISE] {verb} {shown}, but it does not parse{at}: {msg}");
            StepOutput { status: StepStatus::Surprise, violations: vec![format!("{shown} does not parse: {msg}")], stub: head.clone(), observation: head, ..Default::default() }
        }
        None => StepOutput::ok(format!("[ok] {verb} {shown} ({lines} lines, {})", fmt_bytes(content.len() as u64))),
    };
    out.changed_files = true;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Features, test_session};
    use serde_json::json;

    #[tokio::test]
    async fn creates_files_and_parents_but_will_not_clobber_silently() {
        let mut s = test_session("write", Features::presage());
        let out = run(&mut s, &json!({"path": "pkg/sub/notes.md", "content": "# Notes\n\nhello\n"})).await;
        assert_eq!(out.observation, "[ok] wrote pkg/sub/notes.md (3 lines, 15 B)");
        assert!(out.changed_files);

        let clobber = run(&mut s, &json!({"path": "pkg/sub/notes.md", "content": "gone"})).await;
        assert_eq!(clobber.status, StepStatus::Error);
        assert!(clobber.observation.contains("already exists (3 lines)") && !clobber.changed_files);
        assert_eq!(std::fs::read_to_string(s.ws.root.join("pkg/sub/notes.md")).unwrap(), "# Notes\n\nhello\n");

        let forced = run(&mut s, &json!({"path": "pkg/sub/notes.md", "content": "new\n", "overwrite": true})).await;
        assert!(forced.observation.starts_with("[ok] replaced pkg/sub/notes.md"));
        let same = run(&mut s, &json!({"path": "pkg/sub/notes.md", "content": "new\n"})).await;
        assert!(same.observation.contains("no change") && !same.changed_files);
    }

    #[tokio::test]
    async fn flags_unparseable_content_and_respects_the_write_policy() {
        let mut s = test_session("write-check", Features::presage());
        let bad = run(&mut s, &json!({"path": "cfg.json", "content": "{\"a\": }"})).await;
        assert_eq!(bad.status, StepStatus::Surprise);
        assert!(bad.observation.contains("does not parse at line 1"), "{}", bad.observation);

        let denied = run(&mut s, &json!({"path": "/etc/presage-test", "content": "x"})).await;
        assert_eq!(denied.status, StepStatus::Denied);
        assert_eq!(run(&mut s, &json!({"path": "x.txt"})).await.status, StepStatus::Error);
    }
}
