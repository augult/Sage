//! A scripted walkthrough of the harness mechanics, playable without an API key.
//!
//! A tiny workspace with one bug, and a stand-in "model" that fixes it in three replies: reproduce
//! (the check it ran becomes a watch), get it wrong (the script halts and `finish` is skipped), get
//! it right (`finish` rides along with the edit and the watch vouches for it). Everything it shows is
//! the real harness; only the model is fake.

use crate::anchors::anchor;
use crate::provider::Message;
use crate::provider::mock::{MockProvider, MockTurn, TurnFn};
use serde_json::json;
use std::path::{Path, PathBuf};

/// Worded to name neither a file nor a test command. The walkthrough opens with the stand-in reading
/// and reproducing, so nothing may be attached or run for it before the first turn.
pub const TASK: &str =
    "The check fails: the report total is wrong. Fix the report script so that the check passes.";

pub fn create_workspace(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join("prices.csv"),
        "item,price,qty\nwidget,4,5\ngadget,10,3\ngizmo,2,5\n",
    )?;
    std::fs::write(
        dir.join("report.sh"),
        "#!/bin/sh\n# Prints the total value of the stock in prices.csv.\nawk -F, 'NR > 1 { total += $2 } END { print total }' prices.csv\n",
    )?;
    std::fs::write(
        dir.join("check.sh"),
        "#!/bin/sh\ngot=$(sh report.sh)\nif [ \"$got\" = \"60\" ]; then\n  echo \"PASS: total is 60\"\nelse\n  echo \"FAIL: expected 60, got $got\"\n  echo \"  computed at report.sh:3\"\n  exit 1\nfi\n",
    )?;
    std::fs::write(
        dir.join("README.md"),
        "# Stock report\n\n`sh report.sh` prints the total value of the stock (price × quantity for every item).\n`sh check.sh` verifies it.\n",
    )?;
    Ok(())
}

fn awk_line(root: &Path) -> (String, String) {
    let text = std::fs::read_to_string(root.join("report.sh")).unwrap_or_default();
    let line = text.lines().nth(2).unwrap_or("").to_string();
    (anchor(3, &line), line)
}

pub fn provider(root: PathBuf, chunk_delay_ms: u64) -> MockProvider {
    let (r2, r3) = (root.clone(), root.clone());
    let turns: Vec<TurnFn> = vec![
        Box::new(|_: &[Message]| {
            MockTurn::calls(
                "Three small files. I'll read the script and the check together, and reproduce the failure in the same reply. I expect the check to fail, so I predict that: if I'm right I only need a receipt.",
                vec![
                    ("read", json!({"path": "report.sh"})),
                    ("read", json!({"path": "check.sh"})),
                    (
                        "shell",
                        json!({"cmd": "sh check.sh", "expect": {"exit": "nonzero", "has": "FAIL"}}),
                    ),
                ],
            )
        }),
        Box::new(move |_: &[Message]| {
            let (at, line) = awk_line(&r2);
            MockTurn::calls(
                "The total ignores the quantity column. The harness is watching the check for me now, so I add the quantity, predict that it passes and put `finish` in the same reply.",
                vec![
                    (
                        "edit",
                        json!({"path": "report.sh", "range": at, "new": line.replace("total += $2", "total += $2 + $3")}),
                    ),
                    (
                        "shell",
                        json!({"cmd": "sh check.sh", "expect": {"exit": "0", "has": "PASS"}}),
                    ),
                    (
                        "finish",
                        json!({"summary": "report.sh now takes the quantity into account."}),
                    ),
                ],
            )
        }),
        Box::new(move |_: &[Message]| {
            let (at, line) = awk_line(&r3);
            MockTurn::calls(
                "Surprise: 29, not 60. I added the quantity where I should have multiplied by it. The harness stopped before `finish`, so nothing wrong was declared done. Fixing the operator; the watch re-runs the check, so `finish` can ride along.",
                vec![
                    (
                        "edit",
                        json!({"path": "report.sh", "range": at, "new": line.replace("$2 + $3", "$2 * $3")}),
                    ),
                    (
                        "finish",
                        json!({"summary": "report.sh summed prices only. It now sums price × quantity; check.sh passes (total 60)."}),
                    ),
                ],
            )
        }),
    ];
    let mut p = MockProvider::new(turns);
    p.chunk_delay_ms = chunk_delay_ms;
    p
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::agent::{Agent, AgentConfig};
    use crate::events::{EventKind, FinishStatus, MemorySink};
    use crate::provider::Provider;
    use crate::script::Approvals;
    use crate::state::{Features, Session};
    use crate::tools::StepStatus;
    use crate::watches::{WatchChange, WatchState};
    use crate::workspace::Workspace;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn the_walkthrough_plays_out_as_described() {
        let base = std::env::temp_dir().join(format!("sage-demo-{}", crate::util::rand_hex(4)));
        let root = base.join("ws");
        create_workspace(&root).unwrap();
        let sess = Session::new(
            Workspace::new(&root).unwrap(),
            base.join("session"),
            Features::ecosage().without("survey").unwrap(),
            None,
        )
        .unwrap();
        let sink = Arc::new(MemorySink::default());
        let real_root = sess.ws.root.clone();
        let mut agent = Agent::new(
            Provider::Mock(provider(real_root, 0)),
            sess,
            AgentConfig::new("demo"),
            sink.clone(),
            Arc::new(Approvals::default()),
        );

        let out = agent.run(TASK.into(), &CancellationToken::new()).await;
        assert_eq!(out.status, FinishStatus::Done);
        assert!(out.verified);
        assert_eq!(out.evidence, "verified by watch tests");
        assert_eq!(
            (
                agent.stats.turns,
                agent.stats.surprises,
                agent.stats.skipped
            ),
            (3, 1, 1)
        );

        let events = sink.events.lock().unwrap();
        let steps: Vec<(String, StepStatus)> = events
            .iter()
            .filter_map(|e| {
                if let EventKind::StepEnd { tool, status, .. } = e {
                    Some((tool.clone(), *status))
                } else {
                    None
                }
            })
            .collect();
        let expect = |t: &str, s| (t.to_string(), s);
        assert_eq!(
            steps,
            vec![
                expect("read", StepStatus::Ok),
                expect("read", StepStatus::Ok),
                expect("shell", StepStatus::Ok),
                expect("edit", StepStatus::Ok),
                expect("shell", StepStatus::Surprise),
                expect("finish", StepStatus::Skipped),
                expect("edit", StepStatus::Ok),
                expect("finish", StepStatus::Ok),
            ]
        );
        // The task is the whole first message but for the environment: the stand-in's first reply
        // reads the script and gets it, not a note that it was shown already.
        assert!(
            crate::attach::named_files(TASK, &agent.sess.ws).is_empty()
                && crate::watches::named_in(TASK).is_none()
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, EventKind::Note { kind, .. } if kind == "attach"))
        );
        let read = events
            .iter()
            .find_map(|e| {
                if let EventKind::StepEnd { observation, .. } = e {
                    Some(observation.clone())
                } else {
                    None
                }
            })
            .unwrap();
        assert!(read.starts_with("report.sh · 3 lines\n"), "{read}");
        // The surprise carries the failing output and the line it points at, ready to edit.
        let surprise = events
            .iter()
            .find_map(|e| {
                if let EventKind::StepEnd {
                    status: StepStatus::Surprise,
                    observation,
                    ..
                } = e
                {
                    Some(observation.clone())
                } else {
                    None
                }
            })
            .unwrap();
        assert!(surprise.contains("FAIL: expected 60, got 29"), "{surprise}");
        // report.sh was fully shown by the edit just before, so its lines are not attached again.
        assert!(!surprise.contains("--- where"), "{surprise}");
        // The check the stand-in ran in its first reply became a watch; its own second run updated
        // that watch, and the third reply needed no run of its own.
        let flips: Vec<(WatchState, WatchChange)> = events
            .iter()
            .filter_map(|e| {
                if let EventKind::Watch { state, change, .. } = e {
                    Some((*state, *change))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            flips,
            vec![
                (WatchState::Fail, WatchChange::New),
                (WatchState::Fail, WatchChange::Changed),
                (WatchState::Pass, WatchChange::Flip)
            ]
        );
    }
}
