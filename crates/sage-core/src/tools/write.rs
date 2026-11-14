//! The `write` tool: create a file, or deliberately replace one.

use super::edit::broken_by_change;
use super::{StepOutput, StepStatus, bool_arg, str_arg};
use crate::state::Session;
use crate::util::{fmt_bytes, fnv64};
use serde_json::Value;

pub async fn run(sess: &mut Session, args: &Value) -> StepOutput {
    let Some(path_arg) = str_arg(args, "path")
        .map(str::trim)
        .filter(|p| !p.is_empty())
    else {
        return StepOutput::error("`path` is required");
    };
    let Some(content) = str_arg(args, "content") else {
        return StepOutput::error("`content` is required: the full text of the file");
    };
    let abs = sess.ws.resolve(path_arg);
    let shown = sess.ws.display(&abs);
    if let Err(e) = sess.ws.check_writable(&abs) {
        return StepOutput {
            status: StepStatus::Denied,
            ..StepOutput::error(e)
        };
    }
    let existing = match std::fs::metadata(&abs) {
        Ok(md) if md.is_dir() => return StepOutput::error(format!("{shown} is a directory")),
        Ok(_) => std::fs::read(&abs).ok(),
        Err(_) => None,
    };
    if let Some(old) = &existing {
        if old.as_slice() == content.as_bytes() {
            return StepOutput::ok(format!(
                "[ok] {shown}: no change — the file already has this content"
            ));
        }
        // Overwriting by accident destroys work the model may never have read. A file it has been
        // shown whole, as it now stands, is replaced on purpose: refusing that costs a reply and
        // the whole text a second time.
        if !old.is_empty() && !bool_arg(args, "overwrite") && !shown_whole(sess, &abs, old) {
            let lines = old.iter().filter(|b| **b == b'\n').count();
            return StepOutput::error(format!(
                "{shown} already exists ({lines} lines). Use edit to change it, or pass overwrite:true to replace the whole file."
            ));
        }
    }
    if let Some(parent) = abs.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return StepOutput::error(format!("cannot create {}: {e}", sess.ws.display(parent)));
    }
    if let Err(e) = std::fs::write(&abs, content) {
        return StepOutput::error(format!("cannot write {shown}: {e}"));
    }
    let lines = content.lines().count();
    let verb = if existing.is_some() {
        "replaced"
    } else {
        "wrote"
    };
    let before = existing.as_deref().map(String::from_utf8_lossy);
    let broken = if sess.features.post_edit_check {
        broken_by_change(sess, &abs, before.as_deref(), content).await
    } else {
        None
    };
    let mut out = match broken {
        Some((line, msg)) => {
            let msg = sess.shown(msg);
            let at = line.map(|l| format!(" at line {l}")).unwrap_or_default();
            let head = format!("[SURPRISE] {verb} {shown}, but it does not parse{at}: {msg}");
            StepOutput {
                status: StepStatus::Surprise,
                violations: vec![format!("{shown} does not parse: {msg}")],
                stub: head.clone(),
                observation: head,
                ..Default::default()
            }
        }
        None => StepOutput::ok(format!(
            "[ok] {verb} {shown} ({lines} lines, {})",
            fmt_bytes(content.len() as u64)
        )),
    };
    out.changed_files = true;
    let old_lines: Option<Vec<String>> = before
        .as_deref()
        .map(|text| text.lines().map(String::from).collect());
    let new_lines: Vec<String> = content.lines().map(String::from).collect();
    out.change = Some(crate::diff::change(
        &shown,
        old_lines.as_deref(),
        &new_lines,
    ));
    out
}

/// Whether every line of the file, as it is on disk now, is on record as shown to the model.
fn shown_whole(sess: &Session, path: &std::path::Path, content: &[u8]) -> bool {
    let total = String::from_utf8_lossy(content).lines().count();
    let mut windows = sess.seen.windows(path, fnv64(content));
    windows.sort_unstable();
    // The windows of several reads together, as long as they leave no gap.
    let mut reached = 0;
    for (from, to) in windows {
        if from > reached + 1 {
            break;
        }
        reached = reached.max(to);
    }
    total > 0 && reached >= total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Features, test_session};
    use serde_json::json;

    #[tokio::test]
    async fn creates_files_and_parents_but_will_not_clobber_silently() {
        let mut s = test_session("write", Features::ecosage());
        let out = run(
            &mut s,
            &json!({"path": "pkg/sub/notes.md", "content": "# Notes\n\nhello\n"}),
        )
        .await;
        assert_eq!(
            out.observation,
            "[ok] wrote pkg/sub/notes.md (3 lines, 15 B)"
        );
        assert!(out.changed_files);
        assert_eq!(
            out.change
                .as_ref()
                .map(|c| (c.path.as_str(), c.created, c.added, c.removed)),
            Some(("pkg/sub/notes.md", true, 3, 0))
        );

        let clobber = run(
            &mut s,
            &json!({"path": "pkg/sub/notes.md", "content": "gone"}),
        )
        .await;
        assert_eq!(clobber.status, StepStatus::Error);
        assert!(clobber.observation.contains("already exists (3 lines)") && !clobber.changed_files);
        assert_eq!(
            std::fs::read_to_string(s.ws.root.join("pkg/sub/notes.md")).unwrap(),
            "# Notes\n\nhello\n"
        );

        let forced = run(
            &mut s,
            &json!({"path": "pkg/sub/notes.md", "content": "new\n", "overwrite": true}),
        )
        .await;
        assert!(
            forced
                .observation
                .starts_with("[ok] replaced pkg/sub/notes.md")
        );
        let same = run(
            &mut s,
            &json!({"path": "pkg/sub/notes.md", "content": "new\n"}),
        )
        .await;
        assert!(same.observation.contains("no change") && !same.changed_files);
    }

    #[tokio::test]
    async fn a_file_that_was_shown_whole_is_replaced_without_being_asked_twice() {
        let mut s = test_session("write-seen", Features::ecosage());
        let text: String = (1..=40).map(|n| format!("line {n}\n")).collect();
        std::fs::write(s.ws.root.join("long.txt"), &text).unwrap();

        // Half of it is not all of it.
        super::super::read::run(&mut s, &json!({"path": "long.txt", "lines": "1-20"}));
        assert!(
            run(&mut s, &json!({"path": "long.txt", "content": "x\n"}))
                .await
                .observation
                .contains("already exists (40 lines)")
        );
        // Two reads that meet cover it.
        super::super::read::run(&mut s, &json!({"path": "long.txt", "lines": "21-40"}));
        let replaced = run(&mut s, &json!({"path": "long.txt", "content": "x\n"})).await;
        assert!(
            replaced.observation.starts_with("[ok] replaced long.txt") && replaced.changed_files,
            "{}",
            replaced.observation
        );

        // What is there now was never shown: it changed since.
        std::fs::write(s.ws.root.join("long.txt"), "someone else's work\n").unwrap();
        assert_eq!(
            run(&mut s, &json!({"path": "long.txt", "content": "y\n"}))
                .await
                .status,
            StepStatus::Error
        );
    }

    #[tokio::test]
    async fn flags_unparseable_content_and_respects_the_write_policy() {
        let mut s = test_session("write-check", Features::ecosage());
        let bad = run(&mut s, &json!({"path": "cfg.json", "content": "{\"a\": }"})).await;
        assert_eq!(bad.status, StepStatus::Surprise);
        assert!(
            bad.observation.contains("does not parse at line 1"),
            "{}",
            bad.observation
        );

        let denied = run(&mut s, &json!({"path": "/etc/sage-test", "content": "x"})).await;
        assert_eq!(denied.status, StepStatus::Denied);
        assert_eq!(
            run(&mut s, &json!({"path": "x.txt"})).await.status,
            StepStatus::Error
        );
    }
}
