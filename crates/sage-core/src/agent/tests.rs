use super::*;
use crate::anchors::anchor;
use crate::events::MemorySink;
use crate::provider::mock::{MockProvider, MockTurn, TurnFn};
use crate::state::test_session;
use crate::tools::StepStatus;
use serde_json::json;

struct Rig {
    agent: Agent,
    sink: Arc<MemorySink>,
}

fn rig(name: &str, cfg: AgentConfig, turns: Vec<TurnFn>) -> Rig {
    // The survey: these workspaces are small, and the tests say what the first message holds.
    let mut cfg = cfg;
    cfg.features.survey = cfg.features.survey && name == "agent-survey";
    let sess = test_session(name, cfg.features);
    std::fs::write(sess.ws.root.join("n.txt"), "alpha\nbeta\ngamma\n").unwrap();
    let sink = Arc::new(MemorySink::default());
    let agent = Agent::new(
        Provider::Mock(MockProvider::new(turns)),
        sess,
        cfg,
        sink.clone(),
        Arc::new(Approvals::default()),
    );
    Rig { agent, sink }
}

fn fixed(t: MockTurn) -> TurnFn {
    Box::new(move |_| t.clone())
}

fn events(r: &Rig) -> Vec<EventKind> {
    r.sink.events.lock().unwrap().clone()
}

#[tokio::test]
async fn the_model_the_effort_and_the_autonomy_can_change_between_two_messages() {
    let mut cfg = AgentConfig::new("first");
    cfg.interactive = true;
    let mut r = rig(
        "agent-retune",
        cfg,
        vec![fixed(MockTurn::say("from the first"))],
    );
    let cancel = CancellationToken::new();
    assert_eq!(
        r.agent.run("One.".into(), &cancel).await.summary,
        "from the first"
    );

    // What is already so is not a change.
    r.agent.retune(Retune::default());
    r.agent.retune(Retune {
        effort: Some(EffortPolicy::Ratchet(Effort::Low)),
        autonomy: Some(Autonomy::Auto),
        ..Default::default()
    });
    let second = Provider::Mock(MockProvider::new(vec![fixed(MockTurn::say(
        "from the second",
    ))]));
    r.agent.retune(Retune {
        provider: Some((second, "other".into(), "second".into())),
        effort: Some(EffortPolicy::Fixed(Effort::Max)),
        autonomy: Some(Autonomy::Ask),
        ..Default::default()
    });
    assert_eq!(
        r.agent.run("Two.".into(), &cancel).await.summary,
        "from the second"
    );

    // The second model is sent the whole conversation, and none of it is expected to be cached there.
    let sent = requests(&r);
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0][1].content.contains("One.")
            && sent[0][2].content == "from the first"
            && sent[0].last().unwrap().content == "Two.",
        "{:?}",
        sent[0]
    );
    let ev = events(&r);
    let verdicts: Vec<CacheVerdict> = ev
        .iter()
        .filter_map(|e| {
            if let EventKind::Cache { verdict, .. } = e {
                Some(*verdict)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(verdicts, vec![CacheVerdict::First, CacheVerdict::First]);
    assert_eq!(r.agent.stats.cache_misses + r.agent.stats.cache_busts, 0);

    // What changed is said once, after the message it came with, and holds from its first turn.
    let at = ev
        .iter()
        .position(|e| matches!(e, EventKind::User { text, .. } if text == "Two."))
        .unwrap();
    assert!(
        matches!(&ev[at + 1], EventKind::Settings { provider: Some(p), model: Some(m), effort: Some(e), autonomy: Some(a) } if p == "other" && m == "second" && e == "max" && a == "ask"),
        "{:?}",
        ev[at + 1]
    );
    assert_eq!(
        ev.iter()
            .filter(|e| matches!(e, EventKind::Settings { .. }))
            .count(),
        1
    );
    assert!(
        matches!(ev.iter().rfind(|e| matches!(e, EventKind::TurnStart { .. })), Some(EventKind::TurnStart { effort, .. }) if effort == "max")
    );

    // Files attached to a message go with that message alone, by path and in front of it.
    r.agent.retune(Retune {
        provider: Some((
            Provider::Mock(MockProvider::new(vec![
                fixed(MockTurn::say("read")),
                fixed(MockTurn::say("again")),
            ])),
            "other".into(),
            "third".into(),
        )),
        ..Default::default()
    });
    r.agent.attach(vec!["/tmp/notes.txt".into()]);
    r.agent.run("Three.".into(), &cancel).await;
    r.agent.run("Four.".into(), &cancel).await;
    let sent = requests(&r);
    assert!(
        sent[0]
            .last()
            .unwrap()
            .content
            .starts_with("<attached_files>\n/tmp/notes.txt\n</attached_files>")
            && sent[0].last().unwrap().content.ends_with("Three."),
        "{:?}",
        sent[0].last()
    );
    assert_eq!(sent[1].last().unwrap().content, "Four.");
    let attached: Vec<Vec<String>> = events(&r)
        .iter()
        .filter_map(|e| {
            if let EventKind::User { files, .. } = e {
                Some(files.clone())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        attached,
        vec![vec![], vec![], vec!["/tmp/notes.txt".to_string()], vec![]]
    );
}

fn requests(r: &Rig) -> Vec<Vec<Message>> {
    match &*r.agent.provider {
        Provider::Mock(m) => m.requests.lock().unwrap().clone(),
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn a_fix_takes_two_replies_and_the_prefix_stays_cached() {
    let cfg = AgentConfig::new("mock");
    let mut r = rig("agent-happy", cfg, vec![]);
    let root = r.agent.sess.ws.root.clone();
    let turns: Vec<TurnFn> = vec![
        fixed(MockTurn::calls(
            "look first",
            vec![
                ("read", json!({"path": "n.txt"})),
                (
                    "shell",
                    json!({"cmd": "grep -qx BETA n.txt", "expect": {"exit": "nonzero"}}),
                ),
            ],
        )),
        Box::new(move |_| {
            let text = std::fs::read_to_string(root.join("n.txt")).unwrap();
            let line = text.lines().nth(1).unwrap();
            MockTurn::calls(
                "fix, check, finish",
                vec![
                    (
                        "edit",
                        json!({"path": "n.txt", "range": anchor(2, line), "new": "BETA"}),
                    ),
                    (
                        "shell",
                        json!({"cmd": "grep -qx BETA n.txt && seq 1 40", "expect": {"exit": "0", "has": "40"}}),
                    ),
                    ("finish", json!({"summary": "upper-cased beta"})),
                ],
            )
        }),
    ];
    r.agent.provider = Arc::new(Provider::Mock(MockProvider::new(turns)));

    let out = r
        .agent
        .run(
            "Upper-case the second line of n.txt.".into(),
            &CancellationToken::new(),
        )
        .await;
    assert_eq!(out.status, FinishStatus::Done);
    assert!(out.verified);
    assert_eq!(
        out.evidence,
        "verified by `grep -qx BETA n.txt && seq 1 40`"
    );
    assert_eq!(
        std::fs::read_to_string(r.agent.sess.ws.root.join("n.txt")).unwrap(),
        "alpha\nBETA\ngamma\n"
    );
    assert_eq!(
        (
            r.agent.stats.turns,
            r.agent.stats.steps,
            r.agent.stats.surprises,
            r.agent.stats.predictions_held
        ),
        (2, 5, 0, 2)
    );

    let reqs = requests(&r);
    let first_user = &reqs[0][1].content;
    assert!(
        first_user.starts_with("<env>\ncwd: ")
            && first_user.ends_with("<task>\nUpper-case the second line of n.txt.\n</task>"),
        "{first_user}"
    );
    // The second request is the first plus appended messages, nothing rewritten.
    assert_eq!(&reqs[1][..reqs[0].len()], &reqs[0][..]);
    // Reasoning is replayed verbatim; the confirmed check came back as a receipt, not 40 lines.
    assert_eq!(reqs[1][2].reasoning.as_deref(), Some("look first"));
    let last_tools: Vec<&Message> = r
        .agent
        .ctx
        .messages
        .iter()
        .filter(|m| m.role == Role::Tool)
        .collect();
    assert!(
        last_tools[3]
            .content
            .contains("40 lines not shown, your prediction held"),
        "{}",
        last_tools[3].content
    );

    let ev = events(&r);
    let verdicts: Vec<(bool, CacheVerdict)> = ev
        .iter()
        .filter_map(|e| {
            if let EventKind::Cache {
                prefix_stable,
                verdict,
                ..
            } = e
            {
                Some((*prefix_stable, *verdict))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        verdicts,
        vec![(true, CacheVerdict::First), (true, CacheVerdict::Hit)]
    );
    assert!(matches!(
        ev.last(),
        Some(EventKind::Finish {
            status: FinishStatus::Done,
            verified: true,
            ..
        })
    ));
    assert!(ev.iter().any(|e| matches!(e, EventKind::Env { .. })));
    assert!(ev.iter().any(|e| matches!(
        e,
        EventKind::Delta {
            channel: Channel::Reasoning,
            ..
        }
    )));
}

#[tokio::test]
async fn a_wrong_fix_stops_the_script_and_the_next_reply_recovers() {
    let turns = vec![
        fixed(MockTurn::calls(
            "",
            vec![
                (
                    "edit",
                    json!({"path": "n.txt", "old": "beta", "new": "Beta"}),
                ),
                (
                    "shell",
                    json!({"cmd": "grep -qx BETA n.txt", "expect": {"exit": "0"}}),
                ),
                ("finish", json!({"summary": "done"})),
            ],
        )),
        fixed(MockTurn::calls(
            "",
            vec![
                (
                    "edit",
                    json!({"path": "n.txt", "old": "Beta", "new": "BETA"}),
                ),
                (
                    "shell",
                    json!({"cmd": "grep -qx BETA n.txt", "expect": {"exit": "0"}}),
                ),
                ("finish", json!({"summary": "done properly"})),
            ],
        )),
    ];
    let mut r = rig("agent-recover", AgentConfig::new("mock"), turns);
    let out = r.agent.run("fix".into(), &CancellationToken::new()).await;
    assert_eq!(
        (out.status, out.summary.as_str()),
        (FinishStatus::Done, "done properly")
    );
    assert_eq!(
        (
            r.agent.stats.turns,
            r.agent.stats.surprises,
            r.agent.stats.skipped
        ),
        (2, 1, 1)
    );
    let statuses: Vec<StepStatus> = events(&r)
        .iter()
        .filter_map(|e| {
            if let EventKind::StepEnd { status, .. } = e {
                Some(*status)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        statuses,
        vec![
            StepStatus::Ok,
            StepStatus::Surprise,
            StepStatus::Skipped,
            StepStatus::Ok,
            StepStatus::Ok,
            StepStatus::Ok
        ]
    );
}

#[tokio::test]
async fn a_question_ends_the_run_and_the_answer_is_the_next_message() {
    let ask = json!({"question": "Which language?", "options": [{"label": "Rust", "hint": "Recommended for performance"}, {"label": "Python", "hint": "Recommended to get it done quickly"}]});
    let turns = vec![
        // What comes after the question in the same reply is not run.
        fixed(MockTurn::calls(
            "",
            vec![
                ("ask", ask.clone()),
                (
                    "write",
                    json!({"path": "main.rs", "content": "fn main() {}\n"}),
                ),
            ],
        )),
        fixed(MockTurn::calls(
            "",
            vec![
                ("write", json!({"path": "main.py", "content": "print(1)\n"})),
                ("finish", json!({"summary": "done", "unverified": "a stub"})),
            ],
        )),
    ];
    let mut cfg = AgentConfig::new("mock");
    cfg.interactive = true;
    let mut r = rig("agent-ask", cfg, turns);
    let out = r
        .agent
        .run("make me a tool".into(), &CancellationToken::new())
        .await;
    assert_eq!(
        (out.status, out.summary.as_str(), r.agent.stats.turns),
        (FinishStatus::Yielded, "Which language?", 1)
    );
    let asked = events(&r).into_iter().find_map(|e| match e {
        EventKind::Question {
            question, choices, ..
        } => Some((question, choices)),
        _ => None,
    });
    let (question, choices) = asked.expect("the question is put to the user");
    assert_eq!(
        (question.as_str(), choices.len(), choices[0].hint.as_str()),
        ("Which language?", 2, "Recommended for performance")
    );
    assert!(!r.agent.sess.ws.root.join("main.rs").exists());
    let out = r
        .agent
        .run("Python".into(), &CancellationToken::new())
        .await;
    assert_eq!(out.status, FinishStatus::Done);

    // With nobody there, the model is told to decide, and the run goes on.
    let turns = vec![fixed(MockTurn::calls("", vec![("ask", ask)])), finish()];
    let mut r = rig("agent-ask-alone", AgentConfig::new("mock"), turns);
    let out = r
        .agent
        .run("make me a tool".into(), &CancellationToken::new())
        .await;
    assert_eq!(out.status, FinishStatus::Done);
    assert!(
        !events(&r)
            .iter()
            .any(|e| matches!(e, EventKind::Question { .. }))
    );
}

#[tokio::test]
async fn prose_with_unchecked_changes_gets_one_reminder_then_is_accepted_as_unverified() {
    let turns = vec![
        fixed(MockTurn::calls(
            "",
            vec![(
                "edit",
                json!({"path": "n.txt", "old": "beta", "new": "BETA"}),
            )],
        )),
        fixed(MockTurn::say("All done.")),
        fixed(MockTurn::say("Really done.")),
    ];
    let mut r = rig("agent-nudge", AgentConfig::new("mock"), turns);
    let out = r.agent.run("fix".into(), &CancellationToken::new()).await;
    assert_eq!((out.status, out.verified), (FinishStatus::Yielded, false));
    assert_eq!(out.evidence, "not checked since the edit of n.txt");
    let nudges: Vec<&Message> = r
        .agent
        .ctx
        .messages
        .iter()
        .filter(|m| {
            m.role == Role::User
                && m.content
                    .starts_with("[harness] You replied without calling finish")
        })
        .collect();
    assert_eq!(nudges.len(), 1);
    // The lone edit also drew the reminder that the check and finish could have ridden along.
    let tool = r
        .agent
        .ctx
        .messages
        .iter()
        .find(|m| m.role == Role::Tool)
        .unwrap();
    assert!(
        tool.content
            .contains("--- harness ---\nTip: send the check"),
        "{}",
        tool.content
    );

    // With a person watching, prose simply hands the turn back.
    let mut cfg = AgentConfig::new("mock");
    cfg.interactive = true;
    let mut r = rig(
        "agent-interactive",
        cfg,
        vec![
            fixed(MockTurn::calls(
                "",
                vec![("edit", json!({"path": "n.txt", "old": "beta", "new": "B"}))],
            )),
            fixed(MockTurn::say("Changed it. Want tests too?")),
        ],
    );
    let out = r.agent.run("fix".into(), &CancellationToken::new()).await;
    assert_eq!(
        (out.status, r.agent.stats.turns),
        (FinishStatus::Yielded, 2)
    );
}

#[tokio::test]
async fn unattended_a_plan_in_prose_or_a_reply_cut_off_is_not_taken_for_the_answer() {
    let plan = MockTurn::say("I'll start by reading the file.");
    let cut = MockTurn {
        finish: Some("length".into()),
        ..MockTurn::say("First I will")
    };
    let finish = MockTurn::calls("", vec![("finish", json!({"summary": "three lines"}))]);
    let mut r = rig(
        "agent-plan",
        AgentConfig::new("mock"),
        vec![fixed(plan), fixed(cut), fixed(finish)],
    );
    let out = r
        .agent
        .run(
            "How many lines has n.txt?".into(),
            &CancellationToken::new(),
        )
        .await;
    assert_eq!(
        (out.status, out.summary.as_str(), r.agent.stats.turns),
        (FinishStatus::Done, "three lines", 3)
    );
    let said: Vec<&str> = r
        .agent
        .ctx
        .messages
        .iter()
        .filter(|m| m.role == Role::User)
        .map(|m| m.content.as_str())
        .collect();
    assert!(
        said[1].starts_with("[harness] You replied without a tool call, and nobody is here")
            && said[2].starts_with("[harness] Your reply hit the output limit"),
        "{said:?}"
    );
}

#[tokio::test]
async fn a_call_cut_off_by_the_output_limit_is_never_executed() {
    let mut cut = MockTurn::calls(
        "",
        vec![
            ("shell", json!({"cmd": "echo first"})),
            ("write", json!({"path": "big.txt", "content": "half a fi"})),
        ],
    );
    cut.finish = Some("length".into());
    let mut r = rig(
        "agent-length",
        AgentConfig::new("mock"),
        vec![fixed(cut), finish()],
    );
    r.agent.run("write".into(), &CancellationToken::new()).await;
    assert!(!r.agent.sess.ws.root.join("big.txt").exists());
    let tools: Vec<&Message> = r
        .agent
        .ctx
        .messages
        .iter()
        .filter(|m| m.role == Role::Tool)
        .collect();
    assert!(tools[0].content.contains("first"));
    assert!(
        tools[1]
            .content
            .contains("hit the output limit in the middle of this call")
    );
}

#[tokio::test]
async fn stops_on_cancel_and_on_the_turn_limit() {
    let mut r = rig("agent-stop", AgentConfig::new("mock"), vec![]);
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        r.agent.run("x".into(), &cancel).await.status,
        FinishStatus::Stopped
    );

    let mut cfg = AgentConfig::new("mock");
    cfg.max_turns = 4;
    let spin = || fixed(MockTurn::calls("", vec![("shell", json!({"cmd": "true"}))]));
    let mut r = rig(
        "agent-budget",
        cfg,
        vec![spin(), spin(), spin(), spin(), spin()],
    );
    let out = r.agent.run("x".into(), &CancellationToken::new()).await;
    assert_eq!((out.status, r.agent.stats.turns), (FinishStatus::Budget, 4));
    let notes: Vec<String> = events(&r)
        .iter()
        .filter_map(|e| {
            if let EventKind::Note { kind, text, .. } = e {
                (kind == "harness").then(|| text.clone())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        notes,
        ["Half of the budget for this task is used.", LAST_REPLY_NOTE],
        "the last reply is announced once, before it"
    );
    assert!(
        requests(&r)[3]
            .last()
            .unwrap()
            .content
            .ends_with(&format!("\n--- harness ---\n{LAST_REPLY_NOTE}"))
    );
}

#[tokio::test]
async fn the_last_reply_of_the_budget_is_the_report_whatever_it_holds() {
    let edit = || {
        fixed(MockTurn::calls(
            "",
            vec![(
                "edit",
                json!({"path": "n.txt", "old": "beta", "new": "BETA"}),
            )],
        ))
    };
    let two_replies = || {
        let mut cfg = AgentConfig::new("mock");
        cfg.max_turns = 2;
        cfg
    };

    // A check that fails in front of it: `finish` is heard all the same.
    let last = fixed(MockTurn::calls(
        "",
        vec![
            (
                "shell",
                json!({"cmd": "grep -qx GAMMA n.txt", "expect_exit": "0"}),
            ),
            ("finish", json!({"summary": "beta is done, gamma is not"})),
        ],
    ));
    let mut r = rig("agent-landing-failed", two_replies(), vec![edit(), last]);
    let out = r
        .agent
        .run(
            "Upper-case lines two and three of n.txt.".into(),
            &CancellationToken::new(),
        )
        .await;
    assert_eq!(
        (out.status, out.verified, out.summary.as_str()),
        (FinishStatus::Done, false, "beta is done, gamma is not")
    );
    assert_eq!(
        out.evidence,
        "unverified: nothing was checked after the edit of n.txt before the budget ran out"
    );

    // Checked work is accepted as verified.
    let last = fixed(MockTurn::calls(
        "",
        vec![
            (
                "shell",
                json!({"cmd": "grep -qx BETA n.txt", "expect_exit": "0"}),
            ),
            ("finish", json!({"summary": "done"})),
        ],
    ));
    let mut r = rig("agent-landed", two_replies(), vec![edit(), last]);
    let out = r
        .agent
        .run(
            "Upper-case the second line of n.txt.".into(),
            &CancellationToken::new(),
        )
        .await;
    assert_eq!(
        (out.status, out.verified, out.evidence.as_str()),
        (
            FinishStatus::Done,
            true,
            "verified by `grep -qx BETA n.txt`"
        )
    );

    // Prose is not answered with a reminder that there is no reply left to act on.
    let mut r = rig(
        "agent-landing-prose",
        two_replies(),
        vec![
            edit(),
            fixed(MockTurn::say("Line two is done; line three is not.")),
        ],
    );
    let out = r
        .agent
        .run(
            "Upper-case lines two and three of n.txt.".into(),
            &CancellationToken::new(),
        )
        .await;
    assert_eq!(
        (out.status, out.verified, out.summary.as_str()),
        (
            FinishStatus::Yielded,
            false,
            "Line two is done; line three is not."
        )
    );
}

#[tokio::test]
async fn repeated_failures_draw_a_note_and_three_surprises_raise_the_effort() {
    let fail = || {
        fixed(MockTurn::calls(
            "",
            vec![(
                "shell",
                json!({"cmd": "grep -q nope n.txt", "expect": {"exit": "0"}}),
            )],
        ))
    };
    let mut r = rig(
        "agent-stall",
        AgentConfig::new("mock"),
        vec![fail(), fail(), fail(), fixed(MockTurn::say("giving up"))],
    );
    assert_eq!(r.agent.effort(), Effort::Low);
    r.agent.run("x".into(), &CancellationToken::new()).await;
    assert_eq!(r.agent.effort(), Effort::High);
    let ev = events(&r);
    assert!(
        ev.iter().any(
            |e| matches!(e, EventKind::Effort { from, to, .. } if from == "low" && to == "high")
        )
    );
    let notes: Vec<&str> = ev
        .iter()
        .filter_map(|e| {
            if let EventKind::Note { text, .. } = e {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();
    assert!(
        notes
            .iter()
            .any(|n| n.starts_with("The same step has now failed twice")),
        "{notes:?}"
    );
    assert!(
        notes
            .iter()
            .any(|n| n.starts_with("Three replies in a row"))
    );
    // The note reaches the model as a footer on the observation, not as a rewritten message.
    let tools: Vec<&Message> = r
        .agent
        .ctx
        .messages
        .iter()
        .filter(|m| m.role == Role::Tool)
        .collect();
    assert!(
        tools[1]
            .content
            .contains("\n--- harness ---\nThe same step has now failed twice"),
        "{}",
        tools[1].content
    );
}

#[tokio::test]
async fn a_background_job_costs_no_cache_restart_because_the_job_tool_is_always_offered() {
    let turns = vec![
        fixed(MockTurn::calls(
            "",
            vec![("shell", json!({"cmd": "sleep 30", "background": true}))],
        )),
        fixed(MockTurn::calls(
            "",
            vec![("job", json!({"id": "bg:1", "kill": true}))],
        )),
        fixed(MockTurn::calls(
            "",
            vec![(
                "finish",
                json!({"summary": "Started it and stopped it again."}),
            )],
        )),
    ];
    let mut r = rig("agent-jobs", AgentConfig::new("mock"), turns);
    r.agent
        .run(
            "start the server, then stop it".into(),
            &CancellationToken::new(),
        )
        .await;
    let cache: Vec<(bool, CacheVerdict)> = events(&r)
        .iter()
        .filter_map(|e| {
            if let EventKind::Cache {
                prefix_stable,
                verdict,
                ..
            } = e
            {
                Some((*prefix_stable, *verdict))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        cache,
        vec![
            (true, CacheVerdict::First),
            (true, CacheVerdict::Hit),
            (true, CacheVerdict::Hit)
        ]
    );
    assert_eq!(r.agent.stats.cache_busts, 0);
}

#[tokio::test]
async fn the_test_command_the_task_names_is_checked_before_the_first_turn() {
    let turns = || {
        vec![
            fixed(MockTurn::calls(
                "",
                vec![(
                    "edit",
                    json!({"path": "n.txt", "old": "beta", "new": "BETA"}),
                )],
            )),
            fixed(MockTurn::calls(
                "",
                vec![("finish", json!({"summary": "upper-cased beta"}))],
            )),
        ]
    };
    let check = "grep -qx BETA n.txt && echo PASS || { echo 'FAIL: no BETA line'; exit 1; }\n";
    let task = "`sh check.sh` fails. Upper-case the second line of n.txt.";

    let mut r = rig("agent-preflight", AgentConfig::new("mock"), turns());
    std::fs::write(r.agent.sess.ws.root.join("check.sh"), check).unwrap();
    let out = r.agent.run(task.into(), &CancellationToken::new()).await;
    assert_eq!(
        (out.status, out.verified, out.evidence.as_str()),
        (FinishStatus::Done, true, "verified by watch tests")
    );
    assert_eq!(
        r.agent.stats.turns, 2,
        "no turn was spent on running the check"
    );
    // The model starts from the failing output, and its edit alone comes back with the verdict.
    let first = &requests(&r)[0][1].content;
    assert!(first.contains("\n\n<tests>\nwatch tests: FAIL (expected exit 0, got exit 1)\nFAIL: no BETA line\n</tests>\nThe harness ran the test command"), "{first}");
    assert!(first.starts_with("<env>\ncwd: ") && first.ends_with("</task>"));
    let tool = r
        .agent
        .ctx
        .messages
        .iter()
        .find(|m| m.role == Role::Tool)
        .unwrap();
    assert!(
        tool.content
            .contains("--- harness ---\nwatch tests: FAIL → PASS · PASS\nTip: every check passes"),
        "{}",
        tool.content
    );

    // Where every command waits for approval, none is run unasked.
    let mut cfg = AgentConfig::new("mock");
    cfg.autonomy = Autonomy::Ask;
    let mut r = rig("agent-preflight-ask", cfg, turns());
    std::fs::write(r.agent.sess.ws.root.join("check.sh"), check).unwrap();
    r.agent.run(task.into(), &CancellationToken::new()).await;
    assert!(!requests(&r)[0][1].content.contains("<tests>"));
}

#[tokio::test]
async fn the_files_the_task_names_are_in_the_first_message_and_can_be_edited_at_once() {
    let (a1, a2, a3) = (anchor(1, "alpha"), anchor(2, "beta"), anchor(3, "gamma"));
    let task = "Upper-case the second line of `n.txt`.";
    let fix = || {
        fixed(MockTurn::calls(
            "",
            vec![
                (
                    "edit",
                    json!({"path": "n.txt", "range": anchor(2, "beta"), "new": "BETA"}),
                ),
                (
                    "shell",
                    json!({"cmd": "grep -qx BETA n.txt", "expect_exit": "0"}),
                ),
                ("finish", json!({"summary": "upper-cased beta"})),
            ],
        ))
    };
    let read = || {
        fixed(MockTurn::calls(
            "",
            vec![("read", json!({"path": "n.txt"}))],
        ))
    };
    let first_result = |r: &Rig| {
        r.agent
            .ctx
            .messages
            .iter()
            .find(|m| m.role == Role::Tool)
            .unwrap()
            .content
            .clone()
    };

    let mut r = rig("agent-attach", AgentConfig::new("mock"), vec![fix()]);
    let out = r.agent.run(task.into(), &CancellationToken::new()).await;
    assert_eq!(
        (out.status, out.verified, r.agent.stats.turns),
        (FinishStatus::Done, true, 1),
        "nothing had to be read first"
    );
    assert_eq!(
        std::fs::read_to_string(r.agent.sess.ws.root.join("n.txt")).unwrap(),
        "alpha\nBETA\ngamma\n"
    );
    // The file is there as `read` prints it, between the environment and the task.
    let first = &requests(&r)[0][1].content;
    let files = format!(
        "\n</env>\n\nBudget: 80 replies. It counts replies, not tool calls: read everything you need in one reply, and send the edits, their check and `finish` together.\n\n<files>\nn.txt · 3 lines\n{a1}|alpha\n{a2}|beta\n{a3}|gamma\n</files>\nThis is `read` output for the file the task names. It is already read and can be edited directly: do not read it first.\n\n<task>\n{task}\n</task>"
    );
    assert!(
        first.starts_with("<env>\ncwd: ") && first.ends_with(&files),
        "{first}"
    );
    let ev = events(&r);
    assert!(ev.iter().any(|e| matches!(e, EventKind::Note { turn: 0, kind, text } if kind == "attach" && text == "attached n.txt (3 lines)")));
    let tools: Vec<String> = ev
        .iter()
        .filter_map(|e| {
            if let EventKind::StepEnd { tool, .. } = e {
                Some(tool.clone())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(tools, ["edit", "shell", "finish"]);

    // A model that asks for the file all the same is told that it has it.
    let mut r = rig(
        "agent-attach-reread",
        AgentConfig::new("mock"),
        vec![read(), fixed(MockTurn::say("ok"))],
    );
    r.agent.run(task.into(), &CancellationToken::new()).await;
    assert!(
        first_result(&r).starts_with(
            "[unchanged] n.txt lines 1-3 are already in your context (shown in the first message)"
        ),
        "{}",
        first_result(&r)
    );

    // Switched off, the first message is what it used to be and the file has to be asked for.
    let mut cfg = AgentConfig::new("mock");
    cfg.features = Features::sage().without("attach").unwrap();
    let mut r = rig("agent-no-attach", cfg, vec![read(), fix()]);
    let out = r.agent.run(task.into(), &CancellationToken::new()).await;
    assert_eq!((out.status, r.agent.stats.turns), (FinishStatus::Done, 2));
    let env = events(&r)
        .iter()
        .find_map(|e| {
            if let EventKind::Env { text } = e {
                Some(text.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        requests(&r)[0][1].content,
        format!(
            "{env}\n\nBudget: 80 replies. It counts replies, not tool calls: read everything you need in one reply, and send the edits, their check and `finish` together.\n\n<task>\n{task}\n</task>"
        )
    );
    assert!(
        first_result(&r).starts_with("n.txt · 3 lines\n"),
        "{}",
        first_result(&r)
    );
    assert!(
        !events(&r)
            .iter()
            .any(|e| matches!(e, EventKind::Note { kind, .. } if kind == "attach"))
    );
}

#[tokio::test]
async fn an_attached_file_and_the_failing_output_do_not_repeat_each_other() {
    let turns = || {
        vec![fixed(MockTurn::calls(
            "",
            vec![
                (
                    "edit",
                    json!({"path": "n.txt", "old": "beta", "new": "BETA"}),
                ),
                ("finish", json!({"summary": "upper-cased beta"})),
            ],
        ))]
    };
    // The check points at the line that is wrong, and at one in a file the task does not name.
    let check = "grep -qx BETA n.txt && echo PASS || { echo 'n.txt:2: expected BETA'; echo 'see rules.txt:1'; exit 1; }\n";
    let task = "`sh check.sh` fails. Upper-case the second line of `n.txt`.";
    let beta = format!("{}|beta", anchor(2, "beta"));
    let prepare = |r: &Rig| {
        std::fs::write(r.agent.sess.ws.root.join("check.sh"), check).unwrap();
        std::fs::write(
            r.agent.sess.ws.root.join("rules.txt"),
            "second line in capitals\n",
        )
        .unwrap();
    };

    let mut r = rig("agent-attach-preflight", AgentConfig::new("mock"), turns());
    prepare(&r);
    let out = r.agent.run(task.into(), &CancellationToken::new()).await;
    assert_eq!(
        (out.status, out.evidence.as_str(), r.agent.stats.turns),
        (FinishStatus::Done, "verified by watch tests", 1),
        "the fix and `finish` in the first reply"
    );
    // The named file comes whole; of what the output points at, only the other file is added.
    let first = &requests(&r)[0][1].content;
    let (files, tests) = (first.find("\n\n<files>\nn.txt · 3 lines\n").expect("attached"), first.find("\n\n<tests>\nwatch tests: FAIL (expected exit 0, got exit 1)\nn.txt:2: expected BETA\n").expect("checked"));
    assert!(files < tests && first.ends_with("</task>"), "{first}");
    assert_eq!(first.matches(&beta).count(), 1, "{first}");
    assert!(first.contains(&format!("\n--- where (current file content; anchors work in edit) ---\nrules.txt\n> {}|second line in capitals\n</tests>\n", anchor(1, "second line in capitals"))), "{first}");

    // Without the attachment the lines come once as well, with the failing output.
    let mut cfg = AgentConfig::new("mock");
    cfg.features = Features::sage().without("attach").unwrap();
    let mut r = rig("agent-preflight-alone", cfg, turns());
    prepare(&r);
    r.agent.run(task.into(), &CancellationToken::new()).await;
    let first = &requests(&r)[0][1].content;
    assert!(
        !first.contains("<files>")
            && first.contains(&format!(
                "\nn.txt\n  {}|alpha\n> {beta}\n",
                anchor(1, "alpha")
            )),
        "{first}"
    );
    assert_eq!(first.matches(&beta).count(), 1, "{first}");
}

#[tokio::test]
async fn a_traceback_reaches_the_model_without_the_workspace_path_and_with_its_source_lines() {
    let trace = "echo 'Traceback (most recent call last):'; echo \"  File \\\"$PWD/calc.py\\\", line 12, in f\"; echo ZeroDivisionError; exit 1";
    let turns = || {
        vec![
            fixed(MockTurn::calls(
                "",
                vec![("shell", json!({"cmd": trace, "expect_exit": "0"}))],
            )),
            fixed(MockTurn::say("I see.")),
        ]
    };
    let body: String = (1..=30).map(|i| format!("line_{i} = {i}\n")).collect();
    let target = format!("\n> {}|line_12 = 12\n", anchor(12, "line_12 = 12"));
    // What the model was sent for the step, in the request that followed it.
    let observed = |r: &Rig| requests(r)[1].last().unwrap().content.clone();

    let mut r = rig("agent-short-paths", AgentConfig::new("mock"), turns());
    let root = r.agent.sess.ws.root.display().to_string();
    std::fs::write(r.agent.sess.ws.root.join("calc.py"), &body).unwrap();
    r.agent
        .run("Run it.".into(), &CancellationToken::new())
        .await;
    let seen = observed(&r);
    assert!(
        seen.starts_with("[SURPRISE] expected exit 0, got exit 1 · "),
        "{seen}"
    );
    assert!(seen.contains("\nTraceback (most recent call last):\n  File \"calc.py\", line 12, in f\nZeroDivisionError\n--- where (current file content; anchors work in edit) ---\ncalc.py\n") && seen.contains(&target), "{seen}");
    assert!(!seen.contains(&root), "{seen}");
    // What is kept on disk is what the command printed.
    let kept = std::fs::read_to_string(r.agent.sess.spill_path("out:1").unwrap()).unwrap();
    assert!(
        kept.contains(&format!("  File \"{root}/calc.py\", line 12, in f")),
        "{kept}"
    );

    // Switched off, the path is there in full, and the lines are found all the same.
    let mut cfg = AgentConfig::new("mock");
    cfg.features = Features::sage().without("short-paths").unwrap();
    let mut r = rig("agent-long-paths", cfg, turns());
    let root = r.agent.sess.ws.root.display().to_string();
    std::fs::write(r.agent.sess.ws.root.join("calc.py"), &body).unwrap();
    r.agent
        .run("Run it.".into(), &CancellationToken::new())
        .await;
    let seen = observed(&r);
    assert!(
        seen.contains(&format!("\n  File \"{root}/calc.py\", line 12, in f\n"))
            && seen.contains(&target),
        "{seen}"
    );
}

#[tokio::test]
async fn classic_mode_is_a_plain_loop_and_handles_prose_and_edit_errors_cleanly() {
    let mut cfg = AgentConfig::new("mock");
    cfg.features = Features::classic();
    let turns = vec![
        fixed(MockTurn::say("Let me read and update n.txt first.")),
        fixed(MockTurn::calls(
            "",
            vec![
                ("read", json!({"path": "n.txt"})),
                (
                    "edit",
                    json!({"path": "n.txt", "old": "beta extra", "new": "BETA"}),
                ),
            ],
        )),
        fixed(MockTurn::calls(
            "",
            vec![
                (
                    "edit",
                    json!({"path": "n.txt", "old": "beta", "new": "BETA"}),
                ),
                (
                    "write",
                    json!({"path": "n.txt", "content": "alpha\nBETA\nGAMMA\n"}),
                ),
                ("finish", json!({"summary": "done"})),
            ],
        )),
    ];
    let mut r = rig("agent-classic", cfg, turns);
    let out = r.agent.run("fix".into(), &CancellationToken::new()).await;
    assert_eq!(
        (out.status, out.verified, r.agent.stats.turns),
        (FinishStatus::Done, false, 3)
    );
    assert_eq!(
        std::fs::read_to_string(r.agent.sess.ws.root.join("n.txt")).unwrap(),
        "alpha\nBETA\nGAMMA\n"
    );
    let reqs = requests(&r);
    assert_eq!(reqs[0][0].content, prompt::SYSTEM_CLASSIC);
    let root = r.agent.sess.ws.root.display().to_string();
    assert_eq!(
        reqs[0][1].content,
        format!("cwd: {root}\n\n<task>\nfix\n</task>"),
        "no environment snapshot in the baseline, only where it works"
    );
    // Turn 1 prose plan was nudged back to work instead of ending the unattended run early.
    assert!(
        reqs[1]
            .last()
            .unwrap()
            .content
            .starts_with("[harness] You replied without a tool call, and nobody is here")
    );
    // Turn 2 failed `old` match gave plain line numbers rather than non-existent anchor advice.
    let edit_err = reqs[2]
        .iter()
        .rev()
        .find(|m| m.role == Role::Tool)
        .unwrap()
        .content
        .clone();
    assert!(
        edit_err.contains("Copy the current text exactly.") && !edit_err.contains("anchors"),
        "{edit_err}"
    );
}

#[test]
fn effort_policy_parses() {
    assert_eq!(
        EffortPolicy::parse("ratchet"),
        Some(EffortPolicy::Ratchet(Effort::Low))
    );
    assert_eq!(
        EffortPolicy::parse("ratchet-high"),
        Some(EffortPolicy::Ratchet(Effort::High))
    );
    assert_eq!(
        EffortPolicy::parse("MAX"),
        Some(EffortPolicy::Fixed(Effort::Max))
    );
    assert_eq!(EffortPolicy::parse("fast"), None);
    assert_eq!(EffortPolicy::Ratchet(Effort::Low).label(), "ratchet-low");
}

#[test]
fn a_reply_that_only_reasoned_costs_a_level_but_thinking_is_not_switched_off_at_the_first() {
    assert_eq!(
        (
            less_thinking(Effort::Max, 1),
            less_thinking(Effort::High, 1),
            less_thinking(Effort::High, 2)
        ),
        (Some(Effort::High), Some(Effort::Low), Some(Effort::Low))
    );
    // Where the ratchet starts, the first one changes nothing; the second switches thinking off.
    assert_eq!(
        (
            less_thinking(Effort::Low, 1),
            less_thinking(Effort::Low, 2),
            less_thinking(Effort::Off, 3)
        ),
        (None, Some(Effort::Off), None)
    );
}

fn shapes(r: &Rig) -> Vec<crate::provider::mock::Shape> {
    match &*r.agent.provider {
        Provider::Mock(m) => m.shapes.lock().unwrap().clone(),
        _ => unreachable!(),
    }
}

/// A reply that edits a line of n.txt, checks it and finishes.
fn upper(old: &str, new: &str) -> TurnFn {
    fixed(MockTurn::calls(
        "",
        vec![
            ("edit", json!({"path": "n.txt", "old": old, "new": new})),
            (
                "shell",
                json!({"cmd": format!("grep -qx {new} n.txt"), "expect_exit": "0"}),
            ),
            ("finish", json!({"summary": format!("{new} done")})),
        ],
    ))
}

fn finish() -> TurnFn {
    fixed(MockTurn::calls(
        "",
        vec![("finish", json!({"summary": "done"}))],
    ))
}

fn with_goal(r: &mut Rig, text: &str) {
    r.agent.retune(Retune {
        goal: Some(text.into()),
        ..Default::default()
    });
}

fn goal_events(r: &Rig) -> Vec<(GoalState, String, String)> {
    events(r)
        .into_iter()
        .filter_map(|e| {
            if let EventKind::Goal {
                state,
                text,
                detail,
            } = e
            {
                Some((state, text, detail))
            } else {
                None
            }
        })
        .collect()
}

const TASK: &str = "Upper-case lines two and three of n.txt.";

#[tokio::test]
async fn a_goal_that_is_not_met_sends_the_model_back_until_a_check_says_it_is() {
    let turns = vec![
        upper("beta", "BETA"),
        fixed(MockTurn::say("NOT MET\nGamma is still lower case.")),
        upper("gamma", "GAMMA"),
        fixed(MockTurn::say(
            "**MET**\nBoth lines are upper case and checked.",
        )),
    ];
    let mut r = rig("agent-goal", AgentConfig::new("mock"), turns);
    with_goal(&mut r, "lines two and three of n.txt are upper case");
    let out = r.agent.run(TASK.into(), &CancellationToken::new()).await;
    assert_eq!(
        (
            out.status,
            out.verified,
            r.agent.stats.turns,
            r.agent.stats.goal_checks
        ),
        (FinishStatus::Done, true, 2, 2)
    );
    assert_eq!(out.summary, "GAMMA done");
    assert!(out.evidence.ends_with("; goal met"), "{}", out.evidence);
    assert_eq!(
        r.agent.goal(),
        Some(&Goal {
            text: "lines two and three of n.txt are upper case".into(),
            met: true
        })
    );

    let sent = requests(&r);
    assert_eq!(sent.len(), 4, "two replies of the model and two checks");
    // The check is shown the goal, what the model said and what the harness knows; not the conversation.
    assert_eq!(sent[1].len(), 2);
    let shown = &sent[1][1].content;
    assert!(
        shown.contains("<goal>\nlines two and three of n.txt are upper case\n</goal>")
            && shown.contains("<worker_summary>\nBETA done\n</worker_summary>"),
        "{shown}"
    );
    assert!(
        shown.contains("verified by `grep -qx BETA n.txt`")
            && shown.contains("Files you changed with edit or write: n.txt"),
        "{shown}"
    );
    assert!(!shown.contains("<env>") && !shown.contains(TASK), "{shown}");
    // The model is sent back with the reasons, as the harness speaking.
    assert_eq!(
        sent[2].last().unwrap().content,
        "[harness] The session goal is not met yet: Gamma is still lower case.\nCarry on until it holds, then call `finish` again."
    );
    // No tools, the least thinking and a small cap for the check; the work has its tools and its effort.
    let shapes = shapes(&r);
    assert_eq!(
        (shapes[1].effort, shapes[1].tools, shapes[1].max_tokens),
        (Effort::Off, 0, goal::MAX_TOKENS)
    );
    assert!(shapes[0].tools > 0 && shapes[0].effort == Effort::Low && shapes[2].tools > 0);

    let goals = goal_events(&r);
    assert_eq!(
        goals.iter().map(|(state, ..)| *state).collect::<Vec<_>>(),
        [GoalState::Set, GoalState::NotMet, GoalState::Met]
    );
    assert_eq!(goals[1].2, "Gamma is still lower case.");
    // What the checks cost is in the session's figures.
    let replied: u64 = events(&r)
        .iter()
        .filter_map(|e| {
            if let EventKind::Model { usage, .. } = e {
                Some(usage.output)
            } else {
                None
            }
        })
        .sum();
    assert!(
        r.agent.stats.output_tokens > replied,
        "{} against {replied}",
        r.agent.stats.output_tokens
    );

    // A goal that is met no longer gates what comes after it.
    r.agent.provider = Arc::new(Provider::Mock(MockProvider::new(vec![finish()])));
    let out = r
        .agent
        .run("Is it done?".into(), &CancellationToken::new())
        .await;
    assert_eq!(
        (out.status, r.agent.stats.goal_checks, requests(&r).len()),
        (FinishStatus::Done, 2, 1)
    );
}

#[tokio::test]
async fn without_a_goal_nothing_is_asked_at_the_end() {
    let mut r = rig(
        "agent-no-goal",
        AgentConfig::new("mock"),
        vec![upper("beta", "BETA"), fixed(MockTurn::say("NOT MET"))],
    );
    let out = r.agent.run(TASK.into(), &CancellationToken::new()).await;
    assert_eq!(
        (out.status, requests(&r).len(), r.agent.stats.goal_checks),
        (FinishStatus::Done, 1, 0)
    );
    assert!(goal_events(&r).is_empty() && !out.evidence.contains("goal"));
}

#[tokio::test]
async fn the_goal_is_checked_three_times_at_most_per_message_and_then_stays_unmet() {
    let no = || fixed(MockTurn::say("NOT MET\nStill nothing about gamma."));
    let turns = vec![
        upper("beta", "BETA"),
        no(),
        finish(),
        no(),
        finish(),
        no(),
        finish(),
        fixed(MockTurn::say("MET\nNow it is.")),
    ];
    let mut r = rig("agent-goal-cap", AgentConfig::new("mock"), turns);
    with_goal(&mut r, "gamma is upper case too");
    let out = r.agent.run(TASK.into(), &CancellationToken::new()).await;
    // The run ends as any other would, and says that the goal did not hold.
    assert_eq!(
        (
            out.status,
            r.agent.stats.turns,
            r.agent.stats.goal_checks,
            requests(&r).len()
        ),
        (FinishStatus::Incomplete, 3, 3, 6)
    );
    assert!(
        out.evidence
            .ends_with("; goal not met after 3 checks: Still nothing about gamma."),
        "{}",
        out.evidence
    );
    assert_eq!(r.agent.goal().map(|g| g.met), Some(false));
    assert_eq!(
        goal_events(&r)
            .iter()
            .filter(|(state, ..)| *state == GoalState::NotMet)
            .count(),
        3
    );
    // The goal stays: the next message is checked, with three checks of its own.
    let out = r
        .agent
        .run("Go on.".into(), &CancellationToken::new())
        .await;
    assert!(
        out.evidence.ends_with("; goal met") && r.agent.goal().is_some_and(|g| g.met),
        "{}",
        out.evidence
    );
    assert_eq!(r.agent.stats.goal_checks, 4);
}

#[tokio::test]
async fn a_check_that_fails_or_cannot_be_read_leaves_the_run_incomplete() {
    for (name, answer) in [
        (
            "agent-goal-fails",
            MockTurn::fail(ProviderError::Network("connection reset".into())),
        ),
        ("agent-goal-unreadable", MockTurn::say("Looks fine to me.")),
    ] {
        let mut r = rig(
            name,
            AgentConfig::new("mock"),
            vec![upper("beta", "BETA"), fixed(answer)],
        );
        with_goal(&mut r, "beta is upper case");
        let out = r.agent.run(TASK.into(), &CancellationToken::new()).await;
        assert_eq!(
            (
                out.status,
                out.verified,
                r.agent.stats.turns,
                requests(&r).len()
            ),
            (FinishStatus::Incomplete, false, 1, 2),
            "{name}"
        );
        assert!(
            out.evidence.contains("; goal could not be checked: "),
            "{name}: {}",
            out.evidence
        );
        assert_eq!(
            r.agent.goal().map(|g| g.met),
            Some(false),
            "{name}: the goal stays"
        );
        assert!(
            matches!(goal_events(&r).last(), Some((GoalState::Unchecked, text, why)) if text == "beta is upper case" && !why.is_empty()),
            "{name}"
        );
        assert!(matches!(
            events(&r).last(),
            Some(EventKind::Finish {
                status: FinishStatus::Incomplete,
                ..
            })
        ));
    }
}

#[tokio::test]
async fn stopping_during_the_check_stops_the_run() {
    let cancel = CancellationToken::new();
    let stop = {
        let cancel = cancel.clone();
        Box::new(move |_: &[Message]| {
            cancel.cancel();
            MockTurn::say("MET")
        }) as TurnFn
    };
    let mut r = rig(
        "agent-goal-stop",
        AgentConfig::new("mock"),
        vec![upper("beta", "BETA"), stop],
    );
    with_goal(&mut r, "beta is upper case");
    let out = r.agent.run(TASK.into(), &cancel).await;
    assert_eq!(
        (out.status, out.summary.as_str()),
        (FinishStatus::Stopped, "Stopped by the user.")
    );
    assert_eq!(r.agent.goal().map(|g| g.met), Some(false));
    assert!(
        !goal_events(&r)
            .iter()
            .any(|(state, ..)| *state == GoalState::Met)
    );
}

#[tokio::test]
async fn on_the_last_reply_the_check_is_made_and_recorded_but_nothing_can_follow_it() {
    let edit = || {
        fixed(MockTurn::calls(
            "",
            vec![(
                "edit",
                json!({"path": "n.txt", "old": "beta", "new": "BETA"}),
            )],
        ))
    };
    let last = || {
        fixed(MockTurn::calls(
            "",
            vec![
                (
                    "shell",
                    json!({"cmd": "grep -qx BETA n.txt", "expect_exit": "0"}),
                ),
                ("finish", json!({"summary": "beta is done, gamma is not"})),
            ],
        ))
    };
    let two_replies = || AgentConfig {
        max_turns: 2,
        ..AgentConfig::new("mock")
    };

    let mut r = rig(
        "agent-goal-landing",
        two_replies(),
        vec![
            edit(),
            last(),
            fixed(MockTurn::say("NOT MET\nGamma was not done.")),
        ],
    );
    with_goal(&mut r, "lines two and three are upper case");
    let out = r.agent.run(TASK.into(), &CancellationToken::new()).await;
    assert_eq!(
        (out.status, r.agent.stats.turns, requests(&r).len()),
        (FinishStatus::Incomplete, 2, 3)
    );
    assert!(
        out.evidence
            .ends_with("; goal not met: Gamma was not done."),
        "{}",
        out.evidence
    );
    assert_eq!(r.agent.goal().map(|g| g.met), Some(false));
    assert!(matches!(
        goal_events(&r).last(),
        Some((GoalState::NotMet, ..))
    ));

    let mut r = rig(
        "agent-goal-landed",
        two_replies(),
        vec![
            edit(),
            last(),
            fixed(MockTurn::say("MET\nThe goal as stated holds.")),
        ],
    );
    with_goal(&mut r, "line two is upper case");
    r.agent.run(TASK.into(), &CancellationToken::new()).await;
    assert_eq!(r.agent.goal().map(|g| g.met), Some(true));
}

#[tokio::test]
async fn an_answer_in_prose_is_checked_against_the_goal_too() {
    let mut cfg = AgentConfig::new("mock");
    cfg.interactive = true;
    let turns = vec![
        fixed(MockTurn::say("I think that is it.")),
        fixed(MockTurn::say("NOT MET\nNothing was run.")),
        fixed(MockTurn::say("Ran it: all lines are right.")),
        fixed(MockTurn::say("MET")),
    ];
    let mut r = rig("agent-goal-prose", cfg, turns);
    with_goal(&mut r, "n.txt has three lines");
    let out = r
        .agent
        .run(
            "Count the lines of n.txt.".into(),
            &CancellationToken::new(),
        )
        .await;
    assert_eq!(
        (out.status, out.summary.as_str(), r.agent.stats.turns),
        (FinishStatus::Yielded, "Ran it: all lines are right.", 2)
    );
    assert!(r.agent.goal().is_some_and(|g| g.met));
    assert!(
        requests(&r)[2]
            .last()
            .unwrap()
            .content
            .starts_with("[harness] The session goal is not met yet: Nothing was run.")
    );
    assert!(
        requests(&r)[1][1]
            .content
            .contains("<worker_summary>\nI think that is it.\n</worker_summary>")
    );
}

#[tokio::test]
async fn goal_commands_are_answered_by_the_session_and_cost_no_request() {
    let mut r = rig("agent-goal-command", AgentConfig::new("mock"), vec![]);
    r.agent.goal_command("/goal", goal::Command::Show).await;
    r.agent
        .goal_command("/goal tests pass", goal::Command::Set("tests pass".into()))
        .await;
    r.agent.goal_command("/goal", goal::Command::Show).await;
    r.agent
        .goal_command("/goal clear", goal::Command::Clear)
        .await;
    r.agent
        .goal_command("/goal clear", goal::Command::Clear)
        .await;
    assert!(requests(&r).is_empty() && r.agent.goal().is_none());
    let log: Vec<String> = events(&r)
        .iter()
        .map(|e| match e {
            EventKind::User { text, .. } => format!("user {text}"),
            EventKind::Goal {
                state,
                text,
                detail,
            } => format!("{state:?} {text:?} {detail:?}"),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        log,
        [
            "user /goal",
            "Shown \"\" \"\"",
            "user /goal tests pass",
            "Set \"tests pass\" \"\"",
            "user /goal",
            "Shown \"tests pass\" \"not met yet\"",
            "user /goal clear",
            "Cleared \"tests pass\" \"\"",
            "user /goal clear",
            "Cleared \"\" \"\"",
        ]
    );
}

fn git(r: &Rig, script: &str) {
    assert!(
        std::process::Command::new("bash")
            .arg("-c")
            .arg(script)
            .current_dir(&r.agent.sess.ws.root)
            .output()
            .unwrap()
            .status
            .success(),
        "{script}"
    );
}

const COMMIT: &str =
    "git -c user.name=t -c user.email=t@localhost -c commit.gpgsign=false commit -q -m base";

#[tokio::test]
async fn in_a_repository_the_check_is_shown_the_diff_of_the_session_and_not_the_work_that_was_there_before()
 {
    let edit = fixed(MockTurn::calls(
        "",
        vec![
            (
                "edit",
                json!({"path": "n.txt", "old": "beta", "new": "BETA"}),
            ),
            ("write", json!({"path": "scratch.txt", "content": "tmp\n"})),
            (
                "shell",
                json!({"cmd": "grep -qx BETA n.txt", "expect_exit": "0"}),
            ),
            ("finish", json!({"summary": "done"})),
        ],
    ));
    let mut r = rig(
        "agent-goal-git",
        AgentConfig::new("mock"),
        vec![edit, fixed(MockTurn::say("MET"))],
    );
    git(
        &r,
        &format!("printf 'mine\\n' > wip.txt && git init -q && git add n.txt && {COMMIT}"),
    );
    with_goal(&mut r, "n.txt says BETA");
    r.agent.run(TASK.into(), &CancellationToken::new()).await;
    let shown = requests(&r)[1][1].content.clone();
    // Its own two files and the diff of the tracked one; not the file that lay there before.
    assert!(shown.contains("<changes>\nPaths (`git status --short`):\n M n.txt\n?? scratch.txt\nDiff of the tracked files (`git diff HEAD`):\n"), "{shown}");
    assert!(
        shown.contains("\n-beta\n+BETA\n") && !shown.contains("wip.txt"),
        "{shown}"
    );
}

/// Whether the session had a baseline of the uncommitted work once it had run.
async fn baseline_taken(name: &str, features: Features, goal: Option<&str>) -> bool {
    let mut r = rig(
        name,
        AgentConfig {
            features,
            interactive: true,
            ..AgentConfig::new("mock")
        },
        vec![fixed(MockTurn::say("fine")), fixed(MockTurn::say("MET"))],
    );
    git(&r, &format!("git init -q && git add n.txt && {COMMIT}"));
    if let Some(goal) = goal {
        with_goal(&mut r, goal);
    }
    r.agent
        .run("Look at n.txt.".into(), &CancellationToken::new())
        .await;
    r.agent.sess.baseline.is_some()
}

#[tokio::test]
async fn the_baseline_of_the_uncommitted_work_is_taken_where_the_mode_tracks_changes_or_there_is_a_goal()
 {
    assert!(
        baseline_taken("agent-baseline", Features::sage(), None).await,
        "taken whenever the mode tracks changes"
    );
    assert!(
        !baseline_taken("agent-baseline-classic", Features::classic(), None).await,
        "the plain loop tracks nothing"
    );
    assert!(
        baseline_taken(
            "agent-baseline-goal",
            Features::classic(),
            Some("n.txt is read")
        )
        .await,
        "unless there is a goal to check"
    );
}

#[tokio::test]
async fn a_goal_is_checked_in_the_classic_loop_too() {
    let cfg = AgentConfig {
        features: Features::classic(),
        ..AgentConfig::new("mock")
    };
    let edit = fixed(MockTurn::calls(
        "",
        vec![
            (
                "edit",
                json!({"path": "n.txt", "old": "beta", "new": "BETA"}),
            ),
            ("finish", json!({"summary": "done"})),
        ],
    ));
    let mut r = rig(
        "agent-goal-classic",
        cfg,
        vec![
            edit,
            fixed(MockTurn::say("NOT MET\nNothing was run.")),
            finish(),
            fixed(MockTurn::say("MET")),
        ],
    );
    with_goal(&mut r, "n.txt says BETA");
    let out = r.agent.run("fix".into(), &CancellationToken::new()).await;
    assert_eq!(
        (out.status, r.agent.stats.goal_checks, requests(&r).len()),
        (FinishStatus::Done, 2, 4)
    );
    assert!(
        requests(&r)[1][1]
            .content
            .contains("not tracked in classic mode")
            && out.evidence.ends_with("; goal met"),
        "{}",
        out.evidence
    );
}

#[tokio::test]
async fn an_effort_the_user_fixed_is_kept_in_every_mode() {
    let read = || {
        fixed(MockTurn::calls(
            "",
            vec![("read", json!({"path": "n.txt"}))],
        ))
    };
    for (name, features) in [("sage", Features::sage()), ("default", Features::classic())] {
        let mut cfg = AgentConfig::new("mock");
        cfg.features = features;
        cfg.effort = EffortPolicy::Fixed(Effort::Max);
        let turns = vec![
            fixed(MockTurn::fail(ProviderError::Overthinking(100_000))),
            read(),
            finish(),
        ];
        let mut r = rig(&format!("agent-max-{name}"), cfg, turns);
        let out = r
            .agent
            .run("How long is n.txt?".into(), &CancellationToken::new())
            .await;
        assert_eq!(out.status, FinishStatus::Done, "{name}");
        // At `max` a reply may think as long as a reply is ever allowed to, whatever the mode. One
        // that did nothing else is asked for again a level lower, and then the session is back.
        let asked: Vec<(Effort, usize)> = shapes(&r)
            .iter()
            .map(|s| (s.effort, s.reasoning_cap))
            .collect();
        assert_eq!(
            asked,
            [
                (Effort::Max, LONG_THOUGHT_CHARS),
                (Effort::High, 0),
                (Effort::Max, LONG_THOUGHT_CHARS)
            ],
            "{name}"
        );
        let moves: Vec<(String, String)> = events(&r)
            .into_iter()
            .filter_map(|e| {
                if let EventKind::Effort { from, to, .. } = e {
                    Some((from, to))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            moves,
            [
                ("max".to_string(), "high".to_string()),
                ("high".to_string(), "max".to_string())
            ],
            "{name}"
        );
        assert_eq!(r.agent.effort(), Effort::Max, "{name}");
    }

    // An effort left to the harness stays where such a reply took it, and thinks under the usual cap.
    let mut cfg = AgentConfig::new("mock");
    cfg.effort = EffortPolicy::Ratchet(Effort::High);
    let mut r = rig(
        "agent-ratchet-cut",
        cfg,
        vec![
            fixed(MockTurn::fail(ProviderError::Overthinking(100_000))),
            read(),
            finish(),
        ],
    );
    r.agent
        .run("How long is n.txt?".into(), &CancellationToken::new())
        .await;
    assert_eq!(
        shapes(&r)
            .iter()
            .map(|s| (s.effort, s.reasoning_cap))
            .collect::<Vec<_>>(),
        [(Effort::High, 0), (Effort::Low, 0), (Effort::Low, 0)]
    );
}
#[tokio::test]
async fn guard_files_holds_finish_until_a_protected_file_is_restored_and_is_off_by_default() {
    let task = "Audit `n.txt` and report. Do not edit `n.txt`.";
    let touch_forbidden = || upper("beta", "BETA");
    let restore = fixed(MockTurn::calls(
        "",
        vec![
            (
                "edit",
                json!({"path": "n.txt", "old": "BETA", "new": "beta"}),
            ),
            (
                "shell",
                json!({"cmd": "grep -qx beta n.txt", "expect_exit": "0"}),
            ),
            ("finish", json!({"summary": "restored"})),
        ],
    ));
    // With `protect`, touching a protected file holds `finish` until the file is restored.
    let mut cfg = AgentConfig::new("mock");
    cfg.features.protect = true;
    let mut r = rig(
        "agent-contract-protected",
        cfg,
        vec![touch_forbidden(), restore],
    );
    let out = r.agent.run(task.into(), &CancellationToken::new()).await;
    assert_eq!(
        (out.status, out.verified, r.agent.stats.turns),
        (FinishStatus::Done, true, 2)
    );
    assert!(
        requests(&r)[1]
            .last()
            .unwrap()
            .content
            .contains("`n.txt` was modified, though the task says not to change it")
    );
    assert_eq!(
        std::fs::read_to_string(r.agent.sess.ws.root.join("n.txt")).unwrap(),
        "alpha\nbeta\ngamma\n"
    );

    // As Sage comes, the model is trusted with the task's words.
    let mut r = rig(
        "agent-contract-off",
        AgentConfig::new("mock"),
        vec![touch_forbidden()],
    );
    let out = r.agent.run(task.into(), &CancellationToken::new()).await;
    assert_eq!((out.status, r.agent.stats.turns), (FinishStatus::Done, 1));
}

#[tokio::test]
async fn a_reply_still_coming_when_the_time_budget_runs_out_is_cut_and_booked() {
    let slow = MockTurn::calls(
        &"thinking it over ".repeat(400),
        vec![("read", json!({"path": "n.txt"}))],
    );
    let mut cfg = AgentConfig::new("mock");
    cfg.features = Features::sage().without("bootstrap").unwrap();
    cfg.time_budget = Some(Duration::from_secs(1));
    let mut r = rig("agent-deadline", cfg, vec![]);
    let mut provider = MockProvider::new(vec![fixed(slow)]);
    // About ten seconds of streaming.
    provider.chunk_delay_ms = 20;
    r.agent.provider = Arc::new(Provider::Mock(provider));
    let started = Instant::now();
    let out = r
        .agent
        .run("Read n.txt.".into(), &CancellationToken::new())
        .await;
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(out.status, FinishStatus::Budget);
    assert!(out.summary.contains("time budget"), "{}", out.summary);
    // What the cut reply streamed, and its prompt, are in the figures.
    assert!(r.agent.stats.output_tokens > 0 && r.agent.stats.prompt_tokens > 0);
    assert_eq!(
        r.agent.sess.deadline, None,
        "the next message starts afresh"
    );
}

#[tokio::test]
async fn after_the_last_cut_for_reasoning_without_acting_the_next_reply_is_the_last() {
    let cut = || fixed(MockTurn::fail(ProviderError::Overthinking(70_000)));
    let read = || {
        fixed(MockTurn::calls(
            "",
            vec![("read", json!({"path": "n.txt"}))],
        ))
    };
    // It acts but does not finish: the run ends there, as out of its budget.
    let mut r = rig(
        "agent-wrap-up",
        AgentConfig::new("mock"),
        vec![cut(), cut(), cut(), read(), read()],
    );
    let out = r
        .agent
        .run("How long is n.txt?".into(), &CancellationToken::new())
        .await;
    assert_eq!(out.status, FinishStatus::Budget);
    assert!(
        out.summary.contains("cut off three times"),
        "{}",
        out.summary
    );
    let sent = requests(&r);
    assert_eq!(sent.len(), 4);
    assert!(
        sent[3].last().unwrap().content.contains(WRAP_UP_NOTE),
        "{:?}",
        sent[3].last()
    );
    // It finishes: taken as it stands, as on the last reply of any budget.
    let mut r = rig(
        "agent-wrap-up-finish",
        AgentConfig::new("mock"),
        vec![cut(), cut(), cut(), finish()],
    );
    let out = r
        .agent
        .run("How long is n.txt?".into(), &CancellationToken::new())
        .await;
    assert_eq!(out.status, FinishStatus::Done);
    // Cut a fourth time, it fails as before.
    let mut r = rig(
        "agent-wrap-up-fails",
        AgentConfig::new("mock"),
        vec![cut(), cut(), cut(), cut()],
    );
    let out = r
        .agent
        .run("How long is n.txt?".into(), &CancellationToken::new())
        .await;
    assert_eq!(out.status, FinishStatus::Error);
}

#[tokio::test]
async fn what_a_cut_or_failed_reply_streamed_is_booked() {
    let looped = MockTurn {
        reasoning: "a".repeat(4000),
        ..MockTurn::fail(ProviderError::Degenerate)
    };
    let mut r = rig(
        "agent-booked",
        AgentConfig::new("mock"),
        vec![fixed(looped), finish()],
    );
    r.agent
        .run("How long is n.txt?".into(), &CancellationToken::new())
        .await;
    let (mut prompt, mut output) = (0, 0);
    for e in events(&r) {
        if let EventKind::Model { usage, .. } = e {
            prompt += usage.prompt;
            output += usage.output;
        }
    }
    // Beside the reply that finished: the one that looped, its 1,000 tokens and its prompt.
    assert_eq!(r.agent.stats.output_tokens, output + 1000);
    assert_eq!(r.agent.stats.reasoning_tokens, 1000);
    assert!(r.agent.stats.prompt_tokens > prompt);

    // An attempt that streamed and then failed is booked; one that failed before a word is not.
    let mut r = rig("agent-booked-retries", AgentConfig::new("mock"), vec![]);
    let streamed = Streamed::default();
    streamed.seen(true, 400);
    streamed.retried();
    streamed.retried();
    streamed.seen(false, 80);
    r.agent.book_unreported(&streamed, false, (1000, 900));
    let s = &r.agent.stats;
    assert_eq!(
        (
            s.prompt_tokens,
            s.cached_tokens,
            s.output_tokens,
            s.reasoning_tokens
        ),
        (1000, 900, 100, 100)
    );
}

#[tokio::test]
async fn old_observations_are_trimmed_only_where_enough_replies_are_left_to_pay_for_it() {
    let glm = |r: &mut Rig| {
        r.agent.sess.prices = prices_for("glm-5.3");
        // Old, bulky results, then a dozen recent messages that are kept as they are.
        for i in 0..4 {
            r.agent
                .ctx
                .push_tool(&format!("old{i}"), "x".repeat(40_000), "[old]".into());
        }
        for i in 0..KEEP_RECENT_MESSAGES {
            r.agent
                .ctx
                .push_tool(&format!("new{i}"), "y".repeat(5_000), "[new]".into());
        }
    };
    let decays = |r: &Rig| {
        events(r)
            .iter()
            .filter(|e| matches!(e, EventKind::Decay { .. }))
            .count()
    };
    let started = Instant::now();
    // Many replies left: it pays.
    let mut r = rig("agent-decay-pays", AgentConfig::new("mock"), vec![]);
    glm(&mut r);
    r.agent.maybe_decay(10, false, started);
    assert_eq!(decays(&r), 1);
    // The last reply of the budget, or one more after this: it would not.
    for (name, landing, max_turns) in [("last", true, 80), ("one-left", false, 11)] {
        let mut cfg = AgentConfig::new("mock");
        cfg.max_turns = max_turns;
        let mut r = rig(&format!("agent-decay-{name}"), cfg, vec![]);
        glm(&mut r);
        r.agent.maybe_decay(10, landing, started);
        assert_eq!(decays(&r), 0, "{name}");
    }
}

#[tokio::test]
async fn a_file_the_task_asks_for_is_left_to_the_model() {
    let mut r = rig(
        "agent-missing-final-deliverable",
        AgentConfig::new("mock"),
        vec![finish()],
    );
    let out = r
        .agent
        .run(
            "Create `findings.json` with your findings.".into(),
            &CancellationToken::new(),
        )
        .await;
    assert_eq!((out.status, r.agent.stats.turns), (FinishStatus::Done, 1));
}

#[tokio::test]
async fn oversized_history_drops_sealed_exchanges_and_retains_the_task() {
    let think = "reasoning ".repeat(2_000);
    let mut turns: Vec<TurnFn> = (0..4)
        .map(|_| {
            fixed(MockTurn::calls(
                &think,
                vec![("read", json!({"path":"n.txt"}))],
            ))
        })
        .collect();
    turns.push(finish());
    let mut r = rig(
        "agent-bounded-history",
        AgentConfig {
            context_limit: 12_000,
            ..AgentConfig::new("mock")
        },
        turns,
    );
    let outcome = r
        .agent
        .run("Read n.txt.".into(), &CancellationToken::new())
        .await;
    assert_eq!(outcome.status, FinishStatus::Done);
    let sent = requests(&r);
    assert_eq!(sent.len(), 5);
    let last = sent.last().unwrap();
    assert!(last[1].content.contains("Read n.txt."));
    assert!(last.iter().filter(|m| m.reasoning.is_some()).count() < 4);
    for (i, message) in last
        .iter()
        .enumerate()
        .filter(|(_, m)| !m.tool_calls.is_empty())
    {
        assert_eq!(
            last[i + 1].tool_call_id.as_deref(),
            Some(message.tool_calls[0].id.as_str())
        );
    }
}

#[tokio::test]
async fn a_finish_accepted_in_the_reply_that_used_up_the_time_is_the_outcome() {
    // The last reply the budget allows: a command that runs into the deadline, then the report.
    let reply = MockTurn::calls(
        "",
        vec![
            ("shell", json!({"cmd": "sleep 5", "expect_exit": "0"})),
            ("finish", json!({"summary": "half of it is done"})),
        ],
    );
    let mut cfg = AgentConfig::new("mock");
    cfg.features = Features::sage().without("bootstrap").unwrap();
    cfg.time_budget = Some(Duration::from_secs(1));
    cfg.max_turns = 1;
    let mut r = rig("agent-deadline-finish", cfg, vec![fixed(reply)]);
    let out = r
        .agent
        .run("Do it.".into(), &CancellationToken::new())
        .await;
    assert_eq!(out.status, FinishStatus::Done, "{}", out.summary);
    assert_eq!(out.summary, "half of it is done");
    assert!(!out.verified);
}

/// A local OpenAI-style endpoint that streams `bodies` in turn, one per request, and counts them.
async fn streaming_endpoint(bodies: Vec<String>) -> (String, Arc<std::sync::atomic::AtomicU32>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let served = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let count = served.clone();
    tokio::spawn(async move {
        let mut bodies = bodies.into_iter();
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut request = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                let text = String::from_utf8_lossy(&request).to_ascii_lowercase();
                if let Some(head) = text.find("\r\n\r\n") {
                    let length = text[..head]
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if request.len() >= head + 4 + length {
                        break;
                    }
                }
                match socket.read(&mut chunk).await {
                    Ok(n) if n > 0 => request.extend_from_slice(&chunk[..n]),
                    _ => break,
                }
            }
            count.fetch_add(1, Ordering::SeqCst);
            let body = bodies.next().unwrap_or_default();
            let answer = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(answer.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });
    (format!("http://{address}/v1"), served)
}

#[tokio::test]
async fn an_empty_reply_is_booked_once_and_answered_with_a_note_not_retried() {
    use crate::provider::openai::{Dialect, OpenAiProvider};
    let empty = [
        r#"{"choices":[{"index":0,"delta":{"role":"assistant","content":""},"finish_reason":null}]}"#,
        r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
        r#"{"choices":[],"usage":{"prompt_tokens":20000,"completion_tokens":1}}"#,
    ];
    let finish = [
        r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"f1","type":"function","function":{"name":"finish","arguments":"{\"summary\":\"hi\"}"}}]},"finish_reason":"tool_calls"}]}"#,
        r#"{"choices":[],"usage":{"prompt_tokens":20100,"completion_tokens":9}}"#,
    ];
    let sse = |chunks: &[&str]| {
        let mut body: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
        body.push_str("data: [DONE]\n\n");
        body
    };
    let (url, served) = streaming_endpoint(vec![sse(&empty), sse(&finish)]).await;
    let mut provider = OpenAiProvider::new(&url, "k", "m", Dialect::DeepSeek).unwrap();
    provider.max_retries = 2;
    let sess = test_session("agent-empty-reply", Features::sage());
    let sink = Arc::new(MemorySink::default());
    let mut agent = Agent::new(
        Provider::OpenAi(provider),
        sess,
        AgentConfig::new("m"),
        sink.clone(),
        Arc::new(Approvals::default()),
    );
    let out = agent.run("Say hi.".into(), &CancellationToken::new()).await;
    assert_eq!(
        (out.status, out.summary.as_str()),
        (FinishStatus::Done, "hi")
    );
    assert_eq!(
        served.load(Ordering::SeqCst),
        2,
        "no retry of the empty reply"
    );
    assert_eq!(
        (
            agent.stats.requests,
            agent.stats.retries,
            agent.stats.prompt_tokens
        ),
        (2, 0, 40_100),
        "both replies are booked"
    );
    assert!(sink.events.lock().unwrap().iter().any(|e| matches!(
        e,
        EventKind::Note { kind, .. } if kind == "empty"
    )));
}

fn note_texts(r: &Rig) -> Vec<String> {
    events(r)
        .into_iter()
        .filter_map(|e| match e {
            EventKind::Note { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn edits_sent_together_count_as_one_reply_of_work() {
    let mut r = rig("agent-edits-together", AgentConfig::new("mock"), vec![]);
    std::fs::write(r.agent.sess.ws.root.join("m.txt"), "a\nb\nc\nd\ne\nf\n").unwrap();
    let edits: Vec<(&str, serde_json::Value)> = ["a", "b", "c", "d", "e", "f"]
        .iter()
        .map(|l| {
            (
                "edit",
                json!({"path": "m.txt", "old": format!("{l}\n"), "new": format!("{l}{l}\n")}),
            )
        })
        .collect();
    r.agent.provider = Arc::new(Provider::Mock(MockProvider::new(vec![
        fixed(MockTurn::calls("all six", edits)),
        fixed(MockTurn::calls(
            "done",
            vec![("finish", json!({"summary": "ok", "unverified": "none"}))],
        )),
    ])));
    r.agent
        .run(
            "Double every letter in m.txt.".into(),
            &CancellationToken::new(),
        )
        .await;
    let notes = note_texts(&r);
    assert!(!notes.iter().any(|n| n.contains("Step back")), "{notes:?}");
    assert_eq!(r.agent.edits.get("m.txt"), Some(&1));
}

#[tokio::test]
async fn the_same_check_after_a_change_is_not_a_repeat() {
    let mut r = rig("agent-check-again", AgentConfig::new("mock"), vec![]);
    let check = (
        "shell",
        json!({"cmd": "grep -qx 3 n.txt", "expect_exit": "0"}),
    );
    r.agent.provider = Arc::new(Provider::Mock(MockProvider::new(vec![
        fixed(MockTurn::calls(
            "try 1",
            vec![
                ("edit", json!({"path": "n.txt", "old": "alpha", "new": "1"})),
                check.clone(),
            ],
        )),
        fixed(MockTurn::calls(
            "try 2",
            vec![
                ("edit", json!({"path": "n.txt", "old": "beta", "new": "2"})),
                check.clone(),
            ],
        )),
        // Sent again with nothing changed: that is a repeat.
        fixed(MockTurn::calls("try 3", vec![check.clone()])),
        fixed(MockTurn::calls(
            "done",
            vec![("finish", json!({"summary": "ok", "unverified": "none"}))],
        )),
    ])));
    r.agent
        .run("Make n.txt hold 3.".into(), &CancellationToken::new())
        .await;
    let repeats: Vec<usize> = requests(&r)
        .iter()
        .enumerate()
        .filter(|(_, sent)| {
            sent.iter()
                .any(|m| m.content.contains("The same step has now failed twice"))
        })
        .map(|(at, _)| at)
        .collect();
    // Said once, after the third try, and so in the fourth request and not before.
    assert_eq!(repeats.first(), Some(&3), "{:?}", note_texts(&r));
}

/// A local endpoint whose first answer streams `first` and breaks off (no finish reason, no
/// `[DONE]`); every later one is a `finish` call that reports its usage.
async fn breaks_off_once(first: &'static str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut served = 0;
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut request = Vec::new();
            let mut chunk = [0u8; 65536];
            loop {
                let text = String::from_utf8_lossy(&request).to_ascii_lowercase();
                if let Some(head) = text.find("\r\n\r\n") {
                    let length = text[..head]
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if request.len() >= head + 4 + length {
                        break;
                    }
                }
                match socket.read(&mut chunk).await {
                    Ok(n) if n > 0 => request.extend_from_slice(&chunk[..n]),
                    _ => break,
                }
            }
            let body = if served == 0 {
                format!("data: {first}\n\n")
            } else {
                "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"f\",\"function\":{\"name\":\"finish\",\"arguments\":\"{\\\"summary\\\":\\\"ok\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":10}}\n\ndata: [DONE]\n\n".to_string()
            };
            served += 1;
            let answer = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n{body}"
            );
            let _ = socket.write_all(answer.as_bytes()).await;
            let _ = socket.shutdown().await;
        }
    });
    format!("http://{address}/v1")
}

#[tokio::test]
async fn an_attempt_that_streamed_only_a_tool_call_and_broke_off_is_paid_for() {
    let mut booked = Vec::new();
    for first in [
        r#"{"choices":[{"delta":{"reasoning_content":"We need to look"},"finish_reason":null}]}"#,
        r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"a","function":{"name":"shell","arguments":"{\"cmd\":\"ls\"}"}}]},"finish_reason":null}]}"#,
    ] {
        let url = breaks_off_once(first).await;
        let provider = crate::provider::openai::OpenAiProvider::new(
            &url,
            "k",
            "deepseek-flash",
            crate::provider::openai::Dialect::OpenAi,
        )
        .unwrap();
        let mut sess = test_session("agent-unreported", Features::sage());
        sess.prices = prices_for("deepseek-flash");
        let mut agent = Agent::new(
            Provider::OpenAi(provider),
            sess,
            AgentConfig::new("deepseek-flash"),
            Arc::new(MemorySink::default()),
            Arc::new(Approvals::default()),
        );
        agent.run("Say ok.".into(), &CancellationToken::new()).await;
        assert_eq!((agent.stats.requests, agent.stats.retries), (2, 1));
        booked.push((agent.stats.prompt_tokens, agent.stats.output_tokens > 10));
    }
    // Both lost attempts are booked by estimate on top of the reported 1000 tokens.
    assert!(
        booked
            .iter()
            .all(|(prompt, output)| *prompt > 1000 && *output),
        "{booked:?}"
    );
}

/// A local endpoint that cuts the first `cuts` answers off after the same time, as a gateway
/// with a length limit on a stream does; later ones are a `finish` call.
async fn cuts_long_answers(cuts: usize) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut served = 0;
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut request = Vec::new();
            let mut chunk = [0u8; 65536];
            loop {
                let text = String::from_utf8_lossy(&request).to_ascii_lowercase();
                if let Some(head) = text.find("\r\n\r\n") {
                    let length = text[..head]
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if request.len() >= head + 4 + length {
                        break;
                    }
                }
                match socket.read(&mut chunk).await {
                    Ok(n) if n > 0 => request.extend_from_slice(&chunk[..n]),
                    _ => break,
                }
            }
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"reasoning_content\":\"Writing it all\"},\"finish_reason\":null}]}\n\n")
                .await;
            if served < cuts {
                tokio::time::sleep(Duration::from_millis(1200)).await;
            } else {
                let _ = socket.write_all(b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"f\",\"function\":{\"name\":\"finish\",\"arguments\":\"{\\\"summary\\\":\\\"ok\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":10}}\n\ndata: [DONE]\n\n").await;
            }
            served += 1;
            let _ = socket.shutdown().await;
        }
    });
    format!("http://{address}/v1")
}

#[tokio::test]
async fn an_answer_cut_twice_at_the_same_length_is_not_asked_for_a_third_time() {
    let url = cuts_long_answers(2).await;
    let mut provider = crate::provider::openai::OpenAiProvider::new(
        &url,
        "k",
        "m",
        crate::provider::openai::Dialect::Logfare,
    )
    .unwrap();
    provider.min_stream_limit = Duration::from_secs(1);
    let sink = Arc::new(MemorySink::default());
    let mut agent = Agent::new(
        Provider::OpenAi(provider),
        test_session("agent-stream-cut", Features::sage()),
        AgentConfig::new("m"),
        sink.clone(),
        Arc::new(Approvals::default()),
    );
    let out = agent.run("Say ok.".into(), &CancellationToken::new()).await;
    assert_eq!(out.status, FinishStatus::Done);
    // Two cut attempts, then the reply asked for again with the model told why.
    assert_eq!(agent.stats.requests, 3);
    let told = agent.ctx.messages.iter().any(|m| {
        m.content
            .contains("ends any reply that streams longer than about 1 seconds")
    });
    assert!(told);
}

/// Serves `bodies` in turn, closing each after it: the first after a pause, without ending it.
async fn serves_then_cuts(bodies: Vec<&'static str>) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut served = 0;
        while let Ok((mut socket, _)) = listener.accept().await {
            let mut request = Vec::new();
            let mut chunk = [0u8; 65536];
            loop {
                let text = String::from_utf8_lossy(&request).to_ascii_lowercase();
                if let Some(head) = text.find("\r\n\r\n") {
                    let length = text[..head]
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if request.len() >= head + 4 + length {
                        break;
                    }
                }
                match socket.read(&mut chunk).await {
                    Ok(n) if n > 0 => request.extend_from_slice(&chunk[..n]),
                    _ => break,
                }
            }
            let body = bodies[served.min(bodies.len() - 1)];
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n")
                .await;
            let _ = socket.write_all(body.as_bytes()).await;
            if served == 0 {
                tokio::time::sleep(Duration::from_millis(1200)).await;
            }
            served += 1;
            let _ = socket.shutdown().await;
        }
    });
    format!("http://{address}/v1")
}

#[tokio::test]
async fn a_cut_answer_keeps_the_calls_that_arrived_whole() {
    let url = serves_then_cuts(vec![
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"w\",\"function\":{\"name\":\"write\",\"arguments\":\"{\\\"path\\\":\\\"a.txt\\\",\\\"content\\\":\\\"one\\\\n\\\"}\"}}]},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"v\",\"function\":{\"name\":\"write\",\"arguments\":\"{\\\"path\\\":\\\"b.txt\\\",\\\"content\\\":\\\"tw\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"f\",\"function\":{\"name\":\"finish\",\"arguments\":\"{\\\"summary\\\":\\\"ok\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":10}}\n\ndata: [DONE]\n\n",
    ])
    .await;
    let mut provider = crate::provider::openai::OpenAiProvider::new(
        &url,
        "k",
        "m",
        crate::provider::openai::Dialect::Logfare,
    )
    .unwrap();
    provider.min_stream_limit = Duration::from_secs(1);
    let sink = Arc::new(MemorySink::default());
    let mut agent = Agent::new(
        Provider::OpenAi(provider),
        test_session("agent-stream-salvage", Features::sage()),
        AgentConfig::new("m"),
        sink.clone(),
        Arc::new(Approvals::default()),
    );
    agent
        .run("Write the files.".into(), &CancellationToken::new())
        .await;
    // The cut reply is not asked for again: its whole call ran, the half one did not. (The other
    // requests are the finish gate asking for a check.)
    assert!(
        sink.events
            .lock()
            .unwrap()
            .iter()
            .all(|e| !matches!(e, EventKind::Error { .. }))
    );
    let first_reply = agent
        .ctx
        .messages
        .iter()
        .find(|m| m.role == Role::Assistant)
        .unwrap();
    assert_eq!(first_reply.tool_calls.len(), 2);
    let root = agent.sess.ws.root.clone();
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\n"
    );
    assert!(!root.join("b.txt").exists());
    let told = agent
        .ctx
        .messages
        .iter()
        .any(|m| m.content.contains("was cut off by the provider"));
    assert!(told);
}
