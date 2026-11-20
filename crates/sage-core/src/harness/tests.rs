use super::*;

fn harness(name: &str) -> Harness {
    Harness::at(std::env::temp_dir().join(format!("sage-harness-{name}-{}", rand_hex(4))))
}

async fn until_idle(h: &Harness, id: &str) -> SessionMeta {
    for _ in 0..400 {
        let m = h.meta(id).unwrap();
        if m.status == SessionStatus::Idle {
            return m;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("session did not become idle");
}

#[tokio::test]
async fn demo_session_streams_persists_and_can_be_listed_and_replayed() {
    let h = harness("demo");
    let meta = h.demo().unwrap();
    assert!(meta.simulated);
    let (backlog, mut rx) = h.subscribe(&meta.id).unwrap();
    assert!(matches!(
        backlog.first().map(|e| &e.kind),
        Some(EventKind::SessionStart {
            simulated: true,
            ..
        })
    ));

    let mut live_seqs = Vec::new();
    let mut saw_delta = false;
    loop {
        let ev = tokio::time::timeout(Duration::from_secs(30), rx.recv())
            .await
            .expect("events keep coming")
            .unwrap();
        saw_delta |= ev.kind.transient();
        live_seqs.push(ev.seq);
        if matches!(ev.kind, EventKind::Finish { .. }) {
            break;
        }
    }
    assert!(saw_delta, "reasoning is streamed");
    assert!(
        live_seqs.windows(2).all(|w| w[1] == w[0] + 1),
        "no gaps in the live feed"
    );

    let done = until_idle(&h, &meta.id).await;
    assert_eq!(
        (done.outcome, done.verified),
        (Some(FinishStatus::Done), Some(true))
    );
    assert_eq!((done.stats.turns, done.stats.surprises), (3, 1));

    // The log on disk holds every stored event, in order, and no streaming deltas.
    let stored = h.events(&meta.id).unwrap();
    assert!(stored.iter().all(|e| !e.kind.transient()));
    assert!(stored.windows(2).all(|w| w[1].seq > w[0].seq));
    let on_disk = std::fs::read_to_string(
        h.inner
            .data_dir
            .join("sessions")
            .join(&meta.id)
            .join("events.jsonl"),
    )
    .unwrap();
    assert_eq!(on_disk.lines().count(), stored.len());

    // A fresh process sees it as a closed session and can replay it.
    let later = Harness::at(h.inner.data_dir.clone());
    let listed = later.list();
    assert_eq!(
        (listed.len(), listed[0].status, listed[0].verified),
        (1, SessionStatus::Closed, Some(true))
    );
    assert_eq!(later.events(&meta.id).unwrap().len(), stored.len());
    assert!(
        later.send_with_skills(&meta.id, "more", &[]).is_err(),
        "closed sessions cannot be continued"
    );
    assert!(
        later
            .read_output(&meta.id, "out:1")
            .unwrap()
            .contains("FAIL: expected 60, got 16")
    );
    assert!(later.events("../../etc").is_err());
}

#[tokio::test]
async fn keys_defaults_and_start_validation() {
    let h = harness("keys");
    assert!(h.set_key("nope", "k").is_err());
    h.set_key("openrouter", "sk-test").unwrap();
    let state = h.state();
    assert!(
        state
            .providers
            .iter()
            .find(|p| p.id == "openrouter")
            .unwrap()
            .has_key
    );
    assert!(
        !serde_json::to_string(&state).unwrap().contains("sk-test"),
        "keys never leave through the state"
    );
    assert!(h.set_defaults(None, None, Some("turbo".into())).is_err());
    h.set_defaults(
        Some("openrouter".into()),
        Some("z-ai/glm-5.3".into()),
        Some("max".into()),
    )
    .unwrap();
    let state = Harness::at(h.inner.data_dir.clone()).state();
    assert_eq!(
        (
            state.provider.as_deref(),
            state.model.as_deref(),
            state.effort.as_str()
        ),
        (Some("openrouter"), Some("z-ai/glm-5.3"), "max")
    );
    // Another provider with no model named: the first provider's model is not carried over to it.
    h.set_defaults(Some("deepseek".into()), None, None).unwrap();
    assert_eq!(h.config().model, None);
    h.set_defaults(Some("openrouter".into()), Some("z-ai/glm-5.3".into()), None)
        .unwrap();

    let bad_dir = h.start(StartRequest {
        cwd: "/definitely/not/here".into(),
        task: "x".into(),
        provider: Some("openrouter".into()),
        ..Default::default()
    });
    assert!(bad_dir.unwrap_err().contains("cannot use"));
    assert!(
        h.start(StartRequest {
            cwd: "/tmp".into(),
            task: "  ".into(),
            ..Default::default()
        })
        .unwrap_err()
        .contains("empty")
    );
    h.set_key("openrouter", "").unwrap();
    assert!(
        !h.state()
            .providers
            .iter()
            .find(|p| p.id == "openrouter")
            .unwrap()
            .has_key
            || std::env::var("OPENROUTER_API_KEY").is_ok()
    );

    // An endpoint of the user's own: listed after the built-in ones, kept, and taken away with its key.
    assert!(
        h.add_provider("Ollama", "ftp://localhost", &["qwen3".into()], "")
            .is_err()
    );
    let id = h
        .add_provider(
            "Ollama",
            "http://localhost:11434/v1",
            &["qwen3".into()],
            " sk-own ",
        )
        .unwrap();
    h.set_key(&id, "sk-other").unwrap();
    h.set_defaults(Some(id.clone()), Some("qwen3".into()), None)
        .unwrap();
    let state = Harness::at(h.inner.data_dir.clone()).state();
    let own = state.providers.last().unwrap();
    assert_eq!(
        (
            own.id.as_str(),
            own.name.as_str(),
            own.custom,
            own.has_key,
            own.models.as_slice()
        ),
        (
            "custom-ollama",
            "Ollama",
            true,
            true,
            &["qwen3".to_string()][..]
        )
    );
    assert_eq!(
        (state.provider.as_deref(), state.model.as_deref()),
        (Some("custom-ollama"), Some("qwen3"))
    );
    assert!(!serde_json::to_string(&state).unwrap().contains("sk-other"));
    assert!(h.remove_provider("openrouter").is_err());
    h.remove_provider(&id).unwrap();
    let state = Harness::at(h.inner.data_dir.clone()).state();
    assert!(
        state.providers.iter().all(|p| !p.custom)
            && state.provider.as_deref() != Some("custom-ollama")
    );
    assert!(h.set_key(&id, "k").is_err());
}

#[tokio::test]
async fn scripted_sessions_can_switch_mechanisms_off_and_be_closed() {
    use crate::provider::mock::MockTurn;
    use serde_json::json;
    let h = harness("scripted");
    let ws = h.inner.data_dir.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let cwd = ws.to_string_lossy().into_owned();
    let turns = vec![MockTurn::calls(
        "",
        vec![
            ("write", json!({"path": "a.txt", "content": "x"})),
            ("shell", json!({"cmd": "test -f a.txt", "expect_exit": "0"})),
            ("finish", json!({"summary": "wrote a.txt"})),
        ],
    )];
    let req = StartRequest {
        cwd: cwd.clone(),
        task: "write a.txt".into(),
        without: vec!["auto-watch".into(), "survey".into()],
        ..Default::default()
    };
    let meta = h
        .start_scripted(req, MockProvider::scripted(turns))
        .unwrap();
    assert_eq!(
        (meta.mode.as_str(), meta.simulated),
        ("sage without auto-watch, survey", true)
    );
    let done = until_idle(&h, &meta.id).await;
    assert_eq!(
        (done.outcome, done.verified),
        (Some(FinishStatus::Done), Some(true))
    );

    // Closing frees the session; its log can still be replayed, and it is listed as stored: a
    // stand-in's cannot be continued.
    h.close(&meta.id).await.unwrap();
    assert!(h.close(&meta.id).await.is_err() && !h.is_live(&meta.id));
    let stored = h.meta(&meta.id).unwrap();
    assert_eq!(
        (stored.status, stored.why_closed.as_deref()),
        (SessionStatus::Closed, Some("simulated"))
    );
    assert!(
        h.events(&meta.id)
            .unwrap()
            .iter()
            .any(|e| matches!(e.kind, EventKind::Finish { .. }))
    );

    let bad = StartRequest {
        cwd,
        task: "x".into(),
        without: vec!["telepathy".into()],
        ..Default::default()
    };
    assert!(
        h.start_scripted(bad, MockProvider::scripted(vec![]))
            .unwrap_err()
            .contains("unknown mechanism")
    );

    // A harness that shares the keys but keeps its sessions elsewhere.
    h.set_key("openrouter", "sk-test").unwrap();
    let other = h.with_data_dir(h.inner.data_dir.join("elsewhere"));
    assert!(
        other
            .state()
            .providers
            .iter()
            .any(|p| p.id == "openrouter" && p.has_key)
    );
    assert!(other.list().is_empty());

    // Auto-retry is kept with the defaults, and is not handed on to a benchmark's harness.
    assert!(!h.state().auto_retry);
    h.set_auto_retry(true).unwrap();
    assert!(h.state().auto_retry && Harness::at(h.inner.data_dir.clone()).state().auto_retry);
    assert!(
        !h.with_data_dir(h.inner.data_dir.join("bench"))
            .state()
            .auto_retry
    );
    h.set_auto_retry(false).unwrap();
    assert!(!h.state().auto_retry && !Harness::at(h.inner.data_dir.clone()).state().auto_retry);
}

#[tokio::test]
async fn the_system_template_and_the_users_instructions_can_be_edited_and_reset() {
    let h = harness("instructions");
    let fresh = h.instructions();
    assert!(
        !fresh.system_custom && fresh.system == fresh.system_default && fresh.global.is_empty()
    );
    assert!(
        fresh.system_default.starts_with("You are Sage") && !fresh.system_default.contains('{')
    );

    h.set_system_prompt(Some("You are terse. {watch}")).unwrap();
    h.set_global_instructions("Use tabs.\n").unwrap();
    let edited = Harness::at(h.inner.data_dir.clone()).instructions();
    assert_eq!(
        (
            edited.system.as_str(),
            edited.system_custom,
            edited.global.as_str()
        ),
        ("You are terse. {watch}", true, "Use tabs.\n")
    );
    assert_eq!(edited.system_default, fresh.system_default);

    // The built-in text, nothing and `None` all go back to the built-in template.
    for reset in [Some(fresh.system_default.as_str()), Some(" \n"), None] {
        h.set_system_prompt(Some("Mine.")).unwrap();
        assert!(h.instructions().system_custom);
        h.set_system_prompt(reset).unwrap();
        assert!(!h.instructions().system_custom && !h.inner.data_dir.join("system.md").exists());
    }
    h.set_global_instructions("").unwrap();
    assert!(h.instructions().global.is_empty() && !h.inner.data_dir.join("AGENTS.md").exists());
}

#[tokio::test]
async fn a_session_gets_its_system_prompt_once_and_later_skills_with_the_message() {
    use crate::provider::mock::{MockTurn, TurnFn};
    use crate::provider::{Message, Role};
    let h = harness("skills");
    let ws = h.inner.data_dir.join("ws");
    std::fs::create_dir_all(ws.join(".sage/skills")).unwrap();
    std::fs::write(ws.join("AGENTS.md"), "Run make test.\n").unwrap();
    std::fs::write(
        ws.join(".sage/skills/house.md"),
        "---\ntitle: House rules\n---\nNo globals.\n",
    )
    .unwrap();
    h.set_global_instructions("Use tabs.").unwrap();
    let cwd = ws.to_string_lossy().into_owned();
    let req = || StartRequest {
        cwd: cwd.clone(),
        task: "hello".into(),
        interactive: true,
        skills: vec!["security".into(), "house".into()],
        ..Default::default()
    };
    let skill = |name: &str| {
        h.skills(Some(&cwd))
            .into_iter()
            .find(|s| s.name == name)
            .unwrap()
    };

    // Template, the user's instructions, the project's, then the skills in the order asked for.
    let built_in = prompt::system_prompt(&Features::sage());
    let expected = h.system_prompt(&req()).unwrap();
    let (user, project) = (prompt::USER_NOTE, prompt::project_note("AGENTS.md"));
    assert_eq!(
        expected,
        format!(
            "{built_in}\n\n# User instructions\n\n{user}\n\nUse tabs.\n\n# Project instructions (AGENTS.md)\n\n{project}\n\nRun make test.\n\n# Skill: {}\n\n{}\n\n# Skill: House rules\n\nNo globals.",
            skill("security").title,
            skill("security").body
        )
    );

    // A template for one run wins over the user's, which wins over the built-in one.
    let bare = StartRequest {
        skills: vec![],
        project_instructions: Some(false),
        ..req()
    };
    h.set_system_prompt(Some("Mine. {finish}")).unwrap();
    assert_eq!(
        h.system_prompt(&bare).unwrap(),
        format!(
            "Mine. Watched tests are re-run first and it is refused if they fail.\n\n# User instructions\n\n{user}\n\nUse tabs."
        )
    );
    assert_eq!(
        h.system_prompt(&StartRequest {
            system_prompt: Some("Be terse.\n".into()),
            ..bare.clone()
        })
        .unwrap(),
        format!("Be terse.\n\n# User instructions\n\n{user}\n\nUse tabs.")
    );
    h.set_system_prompt(None).unwrap();
    assert_eq!(
        h.system_prompt(&bare).unwrap(),
        format!("{built_in}\n\n# User instructions\n\n{user}\n\nUse tabs.")
    );

    // The baseline is the classic prompt and nothing else.
    assert_eq!(
        h.system_prompt(&StartRequest {
            mode: Some("classic".into()),
            system_prompt: Some("Be terse.".into()),
            ..req()
        })
        .unwrap(),
        prompt::SYSTEM_CLASSIC
    );
    let no_scripts = h
        .system_prompt(&StartRequest {
            without: vec!["scripts".into()],
            ..bare.clone()
        })
        .unwrap();
    assert_eq!(
        no_scripts,
        format!(
            "{}\n\n# User instructions\n\n{user}\n\nUse tabs.",
            prompt::SYSTEM_CLASSIC
        )
    );
    assert!(
        h.system_prompt(&StartRequest {
            with: vec!["telepathy".into()],
            ..bare.clone()
        })
        .unwrap_err()
        .contains("unknown mechanism")
    );

    let unknown = StartRequest {
        skills: vec!["ui".into(), "telepathy".into()],
        ..req()
    };
    assert!(
        h.system_prompt(&unknown)
            .unwrap_err()
            .contains("unknown skill \"telepathy\"")
    );
    assert!(
        h.start_scripted(unknown, MockProvider::scripted(vec![]))
            .unwrap_err()
            .contains("unknown skill \"telepathy\"")
    );
    assert!(h.list().is_empty(), "nothing was started");

    let seen: Arc<Mutex<Vec<Vec<Message>>>> = Arc::default();
    let turn = || {
        let seen = seen.clone();
        Box::new(move |messages: &[Message]| {
            seen.lock().unwrap().push(messages.to_vec());
            MockTurn::say("ok")
        }) as TurnFn
    };
    let meta = h
        .start_scripted(req(), MockProvider::new(vec![turn(), turn(), turn()]))
        .unwrap();
    assert_eq!(meta.skills, ["security", "house"]);
    until_idle(&h, &meta.id).await;

    // What changes on disk after the start does not reach a session that is under way.
    std::fs::write(ws.join("AGENTS.md"), "Changed.").unwrap();
    h.set_global_instructions("Changed.").unwrap();
    h.send_with_skills(&meta.id, "now the UI", &["house".into(), "ui".into()])
        .unwrap();
    until_idle(&h, &meta.id).await;
    h.send_with_skills(&meta.id, "again", &["ui".into()])
        .unwrap();
    let done = until_idle(&h, &meta.id).await;
    assert_eq!(done.skills, ["security", "house", "ui"]);
    assert!(
        h.send_with_skills(&meta.id, "more", &["telepathy".into()])
            .unwrap_err()
            .contains("unknown skill")
    );
    assert_eq!(h.meta(&meta.id).unwrap().status, SessionStatus::Idle);

    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert!(
        seen.iter().all(|messages| messages[0].content == expected),
        "the system prompt is the same bytes in every request"
    );
    let said: Vec<&str> = seen[2]
        .iter()
        .filter(|m| m.role == Role::User)
        .map(|m| m.content.as_str())
        .collect();
    assert!(
        said[0].ends_with("<task>\nhello\n</task>")
            && !said[0].contains("<skill")
            && !said[0].contains("Run make test."),
        "{}",
        said[0]
    );
    // Only the skill the session did not have yet, and only once.
    assert_eq!(
        said[1],
        format!(
            "<skill name=\"ui\">\n{}\n</skill>\n\nnow the UI",
            skill("ui").body
        )
    );
    assert_eq!(said[2], "again");

    let events = h.events(&meta.id).unwrap();
    assert!(
        matches!(&events[0].kind, EventKind::SessionStart { skills, .. } if *skills == ["security", "house"])
    );
    let with_message: Vec<(String, Vec<String>)> = events
        .iter()
        .filter_map(|e| {
            if let EventKind::User { text, skills, .. } = &e.kind {
                Some((text.clone(), skills.clone()))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        with_message,
        [
            ("hello".to_string(), vec![]),
            ("now the UI".to_string(), vec!["ui".to_string()]),
            ("again".to_string(), vec![])
        ]
    );
    assert_eq!(
        Harness::at(h.inner.data_dir.clone()).list()[0].skills,
        ["security", "house", "ui"]
    );
}

#[tokio::test]
async fn a_session_runs_in_one_of_three_modes() {
    use crate::provider::mock::MockTurn;
    let h = harness("modes");
    let ws = h.inner.data_dir.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let req = |mode: Option<&str>| StartRequest {
        cwd: ws.to_string_lossy().into_owned(),
        task: "hello".into(),
        mode: mode.map(str::to_string),
        interactive: true,
        ..Default::default()
    };
    // What each is called in the session's record. No mode chooses the effort for the user.
    for (asked, mode, effort) in [
        (None, "sage", "ratchet-low"),
        (Some("sage"), "sage", "ratchet-low"),
        (Some("default"), "default", "ratchet-low"),
        (Some("classic"), "default", "ratchet-low"),
        (Some("Sage"), "sage", "ratchet-low"),
    ] {
        let meta = h
            .start_scripted(
                req(asked),
                MockProvider::scripted(vec![MockTurn::say("ok")]),
            )
            .unwrap();
        assert_eq!(
            (meta.mode.as_str(), meta.effort.as_str()),
            (mode, effort),
            "{asked:?}"
        );
        until_idle(&h, &meta.id).await;
    }
    assert_eq!(
        h.start_scripted(req(Some("turbo")), MockProvider::scripted(vec![]))
            .unwrap_err(),
        "unknown mode \"turbo\"; use sage, default"
    );
    // Prosage is gone: asking for it is an error like any unknown mode.
    assert_eq!(
        h.start_scripted(req(Some("prosage")), MockProvider::scripted(vec![]))
            .unwrap_err(),
        "unknown mode \"prosage\"; use sage, default"
    );
    // An effort that was chosen stands, and a mode can be run without a part of itself.
    let meta = h
        .start_scripted(
            StartRequest {
                effort: Some("low".into()),
                without: vec!["watches".into()],
                ..req(Some("sage"))
            },
            MockProvider::scripted(vec![MockTurn::say("ok")]),
        )
        .unwrap();
    assert_eq!(
        (meta.mode.as_str(), meta.effort.as_str()),
        ("sage without watches", "low")
    );
    // What is stored for a restart names the mode, so the session comes back as what it was.
    until_idle(&h, &meta.id).await;
    let setup = resume::read(&session_dir(&h, &meta.id)).unwrap().setup;
    assert_eq!(
        (setup.mode.as_str(), setup.without.as_slice()),
        ("sage", &["watches".to_string()][..])
    );
    let back = features_of(&StartRequest {
        mode: Some(setup.mode),
        without: setup.without,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        back,
        (Features::sage().without("watches").unwrap(), Mode::Sage)
    );

    // Sage has every mechanism; the conventional loop has none.
    assert_eq!(Mode::Sage.features(), Features::sage());
    assert_eq!(Mode::Plain.features(), Features::classic());
    assert!(
        Features::NAMES
            .iter()
            .all(|name| Features::classic().with(name).unwrap() != Features::classic())
    );
}

#[tokio::test]
async fn the_classic_baseline_takes_no_skills() {
    use crate::provider::mock::MockTurn;
    let h = harness("classic-skills");
    let ws = h.inner.data_dir.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let req = StartRequest {
        cwd: ws.to_string_lossy().into_owned(),
        task: "hello".into(),
        mode: Some("classic".into()),
        interactive: true,
        skills: vec!["ui".into()],
        ..Default::default()
    };
    let meta = h
        .start_scripted(
            req,
            MockProvider::scripted(vec![MockTurn::say("ok"), MockTurn::say("ok")]),
        )
        .unwrap();
    assert!(meta.skills.is_empty());
    until_idle(&h, &meta.id).await;
    h.send_with_skills(&meta.id, "more", &["security".into()])
        .unwrap();
    assert!(until_idle(&h, &meta.id).await.skills.is_empty());
    assert!(
        h.events(&meta.id)
            .unwrap()
            .iter()
            .all(|e| !matches!(&e.kind, EventKind::User { skills, .. } if !skills.is_empty()))
    );
}

#[tokio::test]
async fn a_message_can_change_the_effort_and_the_autonomy_of_its_session() {
    use crate::provider::mock::MockTurn;
    let h = harness("changes");
    let ws = h.inner.data_dir.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let req = StartRequest {
        cwd: ws.to_string_lossy().into_owned(),
        task: "hello".into(),
        interactive: true,
        ..Default::default()
    };
    let meta = h
        .start_scripted(
            req,
            MockProvider::scripted(vec![
                MockTurn::say("ok"),
                MockTurn::say("ok"),
                MockTurn::say("ok"),
                MockTurn::say("ok"),
            ]),
        )
        .unwrap();
    assert_eq!(until_idle(&h, &meta.id).await.effort, "ratchet-low");

    // What cannot be done is refused before anything is sent.
    let change = |changes: Changes| h.send_with(&meta.id, "more", &[], &[], changes);
    assert!(
        change(Changes {
            effort: Some("turbo".into()),
            ..Default::default()
        })
        .unwrap_err()
        .contains("unknown effort")
    );
    assert!(
        change(Changes {
            autonomy: Some("maybe".into()),
            ..Default::default()
        })
        .unwrap_err()
        .contains("unknown autonomy")
    );
    assert!(
        change(Changes {
            model: Some("another".into()),
            ..Default::default()
        })
        .unwrap_err()
        .contains("stand-in")
    );
    assert_eq!(h.meta(&meta.id).unwrap().status, SessionStatus::Idle);

    change(Changes {
        effort: Some("max".into()),
        autonomy: Some("ask".into()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(until_idle(&h, &meta.id).await.effort, "max");
    // Naming what the session has already is no change.
    change(Changes {
        provider: Some("scripted".into()),
        model: Some("scripted stand-in".into()),
        effort: Some("max".into()),
        autonomy: Some("ask".into()),
        ..Changes::default()
    })
    .unwrap();
    until_idle(&h, &meta.id).await;
    let events = h.events(&meta.id).unwrap();
    let settings: Vec<&EventKind> = events
        .iter()
        .map(|e| &e.kind)
        .filter(|k| matches!(k, EventKind::Settings { .. }))
        .collect();
    assert!(
        matches!(settings.as_slice(), [EventKind::Settings { provider: None, model: None, effort: Some(effort), autonomy: Some(autonomy) }] if effort == "max" && autonomy == "ask"),
        "{settings:?}"
    );
    // It is kept with the session, for the listing and for a later launch.
    assert_eq!(
        Harness::at(h.inner.data_dir.clone()).list()[0].effort,
        "max"
    );

    // A file goes with a message when it is there to be read, and is named in the log.
    let note = ws.join("note.txt");
    std::fs::write(&note, "remember").unwrap();
    let note = note.to_string_lossy().into_owned();
    assert!(
        h.send_with(
            &meta.id,
            "more",
            &[],
            &["/no/such/file".into()],
            Changes::default()
        )
        .unwrap_err()
        .contains("cannot attach")
    );
    h.send_with(
        &meta.id,
        "more",
        &[],
        &[note.clone(), note.clone()],
        Changes::default(),
    )
    .unwrap();
    until_idle(&h, &meta.id).await;
    assert!(
        matches!(&h.events(&meta.id).unwrap().iter().rfind(|e| matches!(e.kind, EventKind::User { .. })).unwrap().kind, EventKind::User { files, .. } if *files == [note])
    );
}

#[test]
fn what_was_stored_before_there_were_skills_still_loads() {
    let meta = SessionMeta {
        id: "s".into(),
        title: "t".into(),
        cwd: "/".into(),
        provider: "p".into(),
        model: "m".into(),
        mode: "sage".into(),
        effort: "high".into(),
        simulated: false,
        created: 1,
        updated: 2,
        status: SessionStatus::Idle,
        outcome: None,
        verified: None,
        stats: Stats::default(),
        skills: vec!["ui".into()],
        goal: None,
        why_closed: None,
    };
    let mut old = serde_json::to_value(&meta).unwrap();
    assert_eq!(
        old.as_object_mut().unwrap().remove("skills"),
        Some(serde_json::json!(["ui"]))
    );
    assert!(
        serde_json::from_value::<SessionMeta>(old)
            .unwrap()
            .skills
            .is_empty()
    );

    let start = r#"{"seq":1,"ts":1,"type":"session_start","id":"s","title":"t","cwd":"/","provider":"p","model":"m","effort":"high","mode":"sage","autonomy":"auto","simulated":false}"#;
    assert!(
        matches!(serde_json::from_str::<Event>(start).unwrap().kind, EventKind::SessionStart { skills, .. } if skills.is_empty())
    );
    let user = r#"{"seq":2,"ts":1,"type":"user","text":"hi"}"#;
    let event = serde_json::from_str::<Event>(user).unwrap();
    assert!(matches!(&event.kind, EventKind::User { skills, .. } if skills.is_empty()));
    assert_eq!(serde_json::to_string(&event).unwrap(), user);

    let req: StartRequest = serde_json::from_str(r#"{"cwd":"/tmp","task":"x"}"#).unwrap();
    assert!(
        req.skills.is_empty() && req.system_prompt.is_none() && req.project_instructions.is_none()
    );
}

type Seen = Arc<Mutex<Vec<Vec<crate::provider::Message>>>>;

/// Scripted replies that keep what each request carried, so that a test can count them.
fn recording(seen: &Seen, turns: Vec<crate::provider::mock::MockTurn>) -> MockProvider {
    use crate::provider::mock::TurnFn;
    MockProvider::new(
        turns
            .into_iter()
            .map(|turn| {
                let seen = seen.clone();
                Box::new(move |messages: &[crate::provider::Message]| {
                    seen.lock().unwrap().push(messages.to_vec());
                    turn.clone()
                }) as TurnFn
            })
            .collect(),
    )
}

fn goals(h: &Harness, id: &str) -> Vec<(GoalState, String, String)> {
    h.events(id)
        .unwrap()
        .into_iter()
        .filter_map(|e| {
            if let EventKind::Goal {
                state,
                text,
                detail,
            } = e.kind
            {
                Some((state, text, detail))
            } else {
                None
            }
        })
        .collect()
}

fn user_messages(h: &Harness, id: &str) -> Vec<String> {
    h.events(id)
        .unwrap()
        .into_iter()
        .filter_map(|e| {
            if let EventKind::User { text, .. } = e.kind {
                Some(text)
            } else {
                None
            }
        })
        .collect()
}

#[tokio::test]
async fn a_goal_is_set_by_the_task_or_a_message_and_stays_until_it_is_met_or_cleared() {
    use crate::provider::mock::MockTurn;
    use serde_json::json;
    let h = harness("goal");
    let ws = h.inner.data_dir.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("n.txt"), "alpha\nbeta\n").unwrap();
    let upper = |old: &str, new: &str| {
        MockTurn::calls(
            "",
            vec![
                ("edit", json!({"path": "n.txt", "old": old, "new": new})),
                (
                    "shell",
                    json!({"cmd": format!("grep -qx {new} n.txt"), "expect_exit": "0"}),
                ),
                ("finish", json!({"summary": format!("{new} done")})),
            ],
        )
    };
    let finish = || MockTurn::calls("", vec![("finish", json!({"summary": "done"}))]);
    let seen = Seen::default();
    let turns = vec![
        // The task is a goal: the model works, and is sent back once.
        upper("alpha", "ALPHA"),
        MockTurn::say("NOT MET\nbeta is still lower case"),
        upper("beta", "BETA"),
        MockTurn::say("MET\nboth lines are upper case"),
        // A follow-up sets another goal; its check cannot be read, so it stays.
        finish(),
        MockTurn::say("Hard to say."),
        // The next message is checked against it, and this time it holds.
        finish(),
        MockTurn::say("MET\nit holds"),
        // Met: nothing is asked after this one.
        finish(),
    ];
    let req = StartRequest {
        cwd: ws.to_string_lossy().into_owned(),
        task: "/goal both lines of n.txt are upper case".into(),
        ..Default::default()
    };
    let meta = h.start_scripted(req, recording(&seen, turns)).unwrap();
    assert_eq!(
        meta.goal,
        Some(Goal {
            text: "both lines of n.txt are upper case".into(),
            met: false
        })
    );
    assert_eq!(meta.title, "both lines of n.txt are upper case");
    let done = until_idle(&h, &meta.id).await;
    assert_eq!(
        (done.outcome, done.goal.clone()),
        (
            Some(FinishStatus::Done),
            Some(Goal {
                text: "both lines of n.txt are upper case".into(),
                met: true
            })
        )
    );
    assert_eq!(
        seen.lock().unwrap().len(),
        4,
        "two replies of the model and two checks"
    );
    // The model is sent the goal as a task; the log shows what the user typed.
    assert!(
        seen.lock().unwrap()[0].last().unwrap().content.ends_with(
            "<task>\nWork until this holds: both lines of n.txt are upper case\n</task>"
        )
    );
    assert_eq!(
        user_messages(&h, &meta.id),
        ["/goal both lines of n.txt are upper case"]
    );
    let states: Vec<GoalState> = goals(&h, &meta.id).iter().map(|g| g.0).collect();
    assert_eq!(states, [GoalState::Set, GoalState::NotMet, GoalState::Met]);
    // It was met in the stored meta as well, not only in memory.
    assert_eq!(
        Harness::at(h.inner.data_dir.clone()).list()[0].goal,
        done.goal
    );

    // `/goal` says what it is and `/goal clear` drops it: neither asks the model anything.
    h.send_with(&meta.id, "/goal", &[], &[], Changes::default())
        .unwrap();
    until_idle(&h, &meta.id).await;
    h.send_with(&meta.id, " /goal clear ", &[], &[], Changes::default())
        .unwrap();
    let cleared = until_idle(&h, &meta.id).await;
    h.send_with(&meta.id, "/goal", &[], &[], Changes::default())
        .unwrap();
    until_idle(&h, &meta.id).await;
    assert_eq!(seen.lock().unwrap().len(), 4);
    assert_eq!(
        (cleared.goal, cleared.outcome),
        (None, Some(FinishStatus::Done))
    );
    let said: Vec<(GoalState, String, String)> = goals(&h, &meta.id).split_off(3);
    assert_eq!(
        said,
        [
            (
                GoalState::Shown,
                "both lines of n.txt are upper case".into(),
                "met".into()
            ),
            (
                GoalState::Cleared,
                "both lines of n.txt are upper case".into(),
                String::new()
            ),
            (GoalState::Shown, String::new(), String::new())
        ]
    );
    assert_eq!(
        user_messages(&h, &meta.id)[1..],
        ["/goal", "/goal clear", "/goal"]
    );
    assert_eq!(Harness::at(h.inner.data_dir.clone()).list()[0].goal, None);

    // A message sets a goal: the model is sent it as a task, and the goal stays while it is unmet.
    h.send_with(
        &meta.id,
        "/goal  n.txt still starts with ALPHA",
        &[],
        &[],
        Changes::default(),
    )
    .unwrap();
    let unchecked = until_idle(&h, &meta.id).await;
    assert_eq!(
        unchecked.goal,
        Some(Goal {
            text: "n.txt still starts with ALPHA".into(),
            met: false
        })
    );
    assert_eq!(seen.lock().unwrap().len(), 6);
    assert_eq!(
        seen.lock().unwrap()[4].last().unwrap().content,
        "Work until this holds: n.txt still starts with ALPHA"
    );
    assert_eq!(
        user_messages(&h, &meta.id).last().unwrap(),
        "/goal  n.txt still starts with ALPHA"
    );
    let last = goals(&h, &meta.id).pop().unwrap();
    assert_eq!(last.0, GoalState::Unchecked);
    // An ordinary message in between is still held to it, and ends it once the check agrees.
    assert!(
        matches!(h.events(&meta.id).unwrap().last().map(|e| &e.kind), Some(EventKind::Finish { evidence, .. }) if evidence.contains("goal could not be checked"))
    );
    h.send_with(&meta.id, "Anything to add?", &[], &[], Changes::default())
        .unwrap();
    let met = until_idle(&h, &meta.id).await;
    assert_eq!(
        met.goal,
        Some(Goal {
            text: "n.txt still starts with ALPHA".into(),
            met: true
        })
    );
    assert_eq!(seen.lock().unwrap().len(), 8);
    // A goal that is met no longer gates the messages after it.
    h.send_with(&meta.id, "And now?", &[], &[], Changes::default())
        .unwrap();
    until_idle(&h, &meta.id).await;
    assert_eq!(seen.lock().unwrap().len(), 9);
    assert_eq!(
        Harness::at(h.inner.data_dir.clone()).list()[0].goal,
        met.goal
    );
}

#[tokio::test]
async fn a_session_starts_with_a_goal_of_its_own_or_not_at_all_with_a_goal_command_that_has_none() {
    use crate::provider::mock::MockTurn;
    let h = harness("goal-start");
    let ws = h.inner.data_dir.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let cwd = ws.to_string_lossy().into_owned();
    let req = |task: &str, goal: Option<&str>| StartRequest {
        cwd: cwd.clone(),
        task: task.into(),
        goal: goal.map(String::from),
        interactive: true,
        ..Default::default()
    };
    let start = |req: StartRequest, seen: &Seen| {
        h.start_scripted(
            req,
            recording(seen, vec![MockTurn::say("ok"), MockTurn::say("MET")]),
        )
    };

    // Nothing to report or clear yet: the first message has to say what the goal is.
    for task in ["/goal", "/goal clear", "  /goal  "] {
        let err = start(req(task, None), &Seen::default()).unwrap_err();
        assert!(
            err.contains("no goal yet") && err.contains("/goal <condition>"),
            "{err}"
        );
    }
    assert!(
        start(req("fix it", Some("  ")), &Seen::default())
            .unwrap_err()
            .contains("goal is empty")
    );
    assert!(
        h.start(StartRequest {
            cwd: cwd.clone(),
            task: " ".into(),
            ..Default::default()
        })
        .unwrap_err()
        .contains("empty")
    );
    assert!(
        h.list().iter().all(|m| m.goal.is_none()),
        "nothing was started with a goal"
    );

    // A goal and no task: the goal is the task.
    let seen = Seen::default();
    let meta = start(req("", Some(" the docs name every flag ")), &seen).unwrap();
    assert_eq!(
        meta.goal,
        Some(Goal {
            text: "the docs name every flag".into(),
            met: false
        })
    );
    let done = until_idle(&h, &meta.id).await;
    assert!(
        seen.lock().unwrap()[0]
            .last()
            .unwrap()
            .content
            .ends_with("<task>\nWork until this holds: the docs name every flag\n</task>")
    );
    assert_eq!(
        user_messages(&h, &meta.id),
        ["Work until this holds: the docs name every flag"]
    );
    assert!(
        done.goal.is_some_and(|g| g.met),
        "a prose answer is checked too"
    );

    // A goal beside a task: the task stays the task, and the model is told of the goal after it.
    let seen = Seen::default();
    let meta = start(
        req("Document the flags.", Some("the docs name every flag")),
        &seen,
    )
    .unwrap();
    until_idle(&h, &meta.id).await;
    let first = seen.lock().unwrap()[0].last().unwrap().content.clone();
    assert!(first.ends_with("<task>\nDocument the flags.\n\nThe session goal, which an independent check judges when you finish: the docs name every flag\n</task>"), "{first}");
    assert_eq!(
        (user_messages(&h, &meta.id), meta.title.as_str()),
        (
            vec!["Document the flags.".to_string()],
            "Document the flags."
        )
    );
}

#[test]
fn logs_and_metas_that_hold_what_was_retired_still_open() {
    let h = harness("retired");
    let id = "20260101-000000-ab";
    let dir = h.inner.data_dir.join("sessions").join(id);
    std::fs::create_dir_all(&dir).unwrap();
    // A stats record as the engine wrote it when it still counted lessons.
    let mut stats = serde_json::to_value(Stats {
        turns: 7,
        ..Stats::default()
    })
    .unwrap();
    stats.as_object_mut().unwrap().remove("goal_checks");
    stats
        .as_object_mut()
        .unwrap()
        .insert("lessons_learned".into(), 2.into());
    let line = |seq: u64, body: serde_json::Value| {
        let mut event = serde_json::json!({"seq": seq, "ts": 1});
        event
            .as_object_mut()
            .unwrap()
            .extend(body.as_object().unwrap().clone());
        event.to_string()
    };
    let log = [
        line(
            1,
            serde_json::json!({"type": "session_start", "id": id, "title": "t", "cwd": "/", "provider": "p", "model": "m", "effort": "high", "mode": "sage", "autonomy": "auto", "simulated": false}),
        ),
        line(2, serde_json::json!({"type": "user", "text": "hi"})),
        line(
            3,
            serde_json::json!({"type": "lesson", "learned": true, "text": "`pytest -q` fails here → `uv run pytest -q` works"}),
        ),
        line(
            4,
            serde_json::json!({"type": "note", "turn": 1, "kind": "review", "text": "one review pass"}),
        ),
        line(5, serde_json::json!({"type": "stats", "stats": stats})),
        line(
            6,
            serde_json::json!({"type": "finish", "status": "done", "summary": "ok", "verified": true, "evidence": "e"}),
        ),
    ];
    std::fs::write(dir.join("events.jsonl"), log.join("\n") + "\n").unwrap();
    let meta = SessionMeta {
        id: id.into(),
        title: "t".into(),
        cwd: "/".into(),
        provider: "p".into(),
        model: "m".into(),
        mode: "sage".into(),
        effort: "high".into(),
        simulated: false,
        created: 1,
        updated: 2,
        status: SessionStatus::Idle,
        outcome: Some(FinishStatus::Done),
        verified: Some(true),
        stats: Stats {
            turns: 7,
            ..Stats::default()
        },
        skills: vec![],
        goal: None,
        why_closed: None,
    };
    let mut old = serde_json::to_value(&meta).unwrap();
    old.as_object_mut().unwrap().remove("goal");
    old["stats"] = stats.clone();
    std::fs::write(dir.join("meta.json"), old.to_string()).unwrap();

    // The retired event is left out; everything round it is there, the old stats included.
    let events = h.events(id).unwrap();
    let kinds: Vec<u64> = events.iter().map(|e| e.seq).collect();
    assert_eq!(kinds, [1, 2, 4, 5, 6]);
    assert!(events.iter().any(|e| matches!(&e.kind, EventKind::Stats { stats } if stats.turns == 7 && stats.goal_checks == 0)));
    let listed = h.list();
    assert_eq!(
        (
            listed.len(),
            listed[0].status,
            listed[0].goal.clone(),
            listed[0].stats.turns
        ),
        (1, SessionStatus::Closed, None, 7)
    );
    assert_eq!(
        listed[0].why_closed.as_deref(),
        Some("older"),
        "stored before snapshots: read-only"
    );
}

#[tokio::test]
async fn a_session_is_renamed_and_deleted_unless_it_runs_or_another_process_has_it() {
    use crate::provider::mock::MockTurn;
    use serde_json::json;
    let h = harness("rename-delete");
    let cwd = resume_workspace(&h);
    let start = |h: &Harness| {
        let turns = vec![MockTurn::calls(
            "",
            vec![
                ("shell", json!({"cmd": "test -f n.txt", "expect_exit": "0"})),
                ("finish", json!({"summary": "it is there"})),
            ],
        )];
        h.start_scripted(
            StartRequest {
                cwd: cwd.clone(),
                task: "is n.txt there".into(),
                without: vec!["survey".into()],
                ..Default::default()
            },
            MockProvider::scripted(turns),
        )
        .unwrap()
        .id
    };
    let stored_title = |h: &Harness, id: &str| {
        serde_json::from_str::<serde_json::Value>(
            &std::fs::read_to_string(session_dir(h, id).join("meta.json")).unwrap(),
        )
        .unwrap()["title"]
            .clone()
    };

    // Live: the title is the first line of what was asked for, and is on disk at once.
    let id = start(&h);
    until_idle(&h, &id).await;
    assert_eq!(
        h.rename(&id, "  Is it there?\nand more ").unwrap().title,
        "Is it there?"
    );
    assert_eq!(
        (h.meta(&id).unwrap().title.as_str(), stored_title(&h, &id)),
        ("Is it there?", json!("Is it there?"))
    );
    assert!(h.rename(&id, " \n ").unwrap_err().contains("needs a title"));
    assert_eq!(
        h.rename(&id, &"x".repeat(100))
            .unwrap()
            .title
            .chars()
            .count(),
        73,
        "as long as a title gets, and the mark that it was cut"
    );

    // Another harness on the same data, as another process would be: the session is not its to touch.
    let other = Harness::at(h.inner.data_dir.clone());
    other.resume_stand_ins(true);
    assert!(
        other
            .rename(&id, "mine")
            .unwrap_err()
            .contains("another process")
    );
    assert!(
        other
            .delete(&id)
            .await
            .unwrap_err()
            .contains("another process")
    );

    // Not while it runs; once it does not, it goes with everything it stored, and the folder stays.
    h.live(&id).unwrap().set_status(SessionStatus::Running);
    assert!(h.delete(&id).await.unwrap_err().contains("stop it first"));
    assert!(h.is_live(&id) && session_dir(&h, &id).is_dir());
    h.live(&id).unwrap().set_status(SessionStatus::Idle);
    h.delete(&id).await.unwrap();
    assert!(
        !h.is_live(&id)
            && !session_dir(&h, &id).exists()
            && h.list().is_empty()
            && h.meta(&id).is_err()
    );
    assert!(Path::new(&cwd).join("n.txt").is_file());
    assert!(h.delete(&id).await.unwrap_err().contains("no session"));

    // Stored: renamed in its file, where what this build does not know of is kept, and deleted.
    let id = start(&h);
    until_idle(&h, &id).await;
    h.close(&id).await.unwrap();
    listed_free(&other).await;
    let path = session_dir(&h, &id).join("meta.json");
    let mut meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    meta["from_a_newer_build"] = json!(7);
    std::fs::write(&path, meta.to_string()).unwrap();
    let renamed = other.rename(&id, "Stored and renamed").unwrap();
    assert_eq!(
        (renamed.title.as_str(), renamed.status),
        ("Stored and renamed", SessionStatus::Resumable)
    );
    let kept: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        (&kept["title"], &kept["from_a_newer_build"], &kept["id"]),
        (&json!("Stored and renamed"), &json!(7), &json!(id))
    );
    other.delete(&id).await.unwrap();
    assert!(!session_dir(&h, &id).exists() && other.list().is_empty());

    // An id is a name in the sessions folder, never a path out of it.
    for bad in ["", "..", "../ws", "a/b", "a\\b"] {
        assert!(
            h.delete(bad).await.is_err() && h.rename(bad, "x").is_err(),
            "{bad:?}"
        );
    }
    assert!(Path::new(&cwd).join("n.txt").is_file());
}

// --- continuing a session after the program that ran it has ended ---

/// The newest session as `list` gives it, once nobody has it. A session's lock is let go of when
/// the last copy of the file it was taken on is closed, and a command that is being started at
/// that moment holds a copy for an instant.
async fn listed_free(h: &Harness) -> SessionMeta {
    for _ in 0..100 {
        let meta = h.list().remove(0);
        if meta.why_closed.as_deref() != Some("busy") {
            return meta;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the session stayed busy");
}

fn resume_workspace(h: &Harness) -> String {
    let ws = h.inner.data_dir.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("n.txt"), "alpha\nbeta\n").unwrap();
    ws.to_string_lossy().into_owned()
}

fn session_dir(h: &Harness, id: &str) -> PathBuf {
    h.inner.data_dir.join("sessions").join(id)
}

/// A first run that edits `n.txt`, is held to a goal that its checks keep finding unmet, and ends
/// with the goal still active. Returns the harness, the session and what its requests carried.
async fn run_with_a_goal_left_open(name: &str) -> (Harness, String, String, Seen) {
    use crate::provider::mock::MockTurn;
    use serde_json::json;
    let h = harness(name);
    let cwd = resume_workspace(&h);
    let upper = MockTurn::calls(
        "",
        vec![
            (
                "edit",
                json!({"path": "n.txt", "old": "alpha", "new": "ALPHA"}),
            ),
            (
                "shell",
                json!({"cmd": "grep -qx ALPHA n.txt", "expect_exit": "0"}),
            ),
            ("finish", json!({"summary": "ALPHA done"})),
        ],
    );
    let finish = || MockTurn::calls("", vec![("finish", json!({"summary": "done"}))]);
    let seen = Seen::default();
    let turns = vec![
        upper,
        MockTurn::say("NOT MET\nbeta is lower case"),
        finish(),
        MockTurn::say("NOT MET\nstill"),
        finish(),
        MockTurn::say("NOT MET\nstill"),
    ];
    let req = StartRequest {
        cwd: cwd.clone(),
        task: "/goal n.txt is all upper case".into(),
        without: vec!["survey".into()],
        ..Default::default()
    };
    let meta = h.start_scripted(req, recording(&seen, turns)).unwrap();
    until_idle(&h, &meta.id).await;
    (h, meta.id, cwd, seen)
}

#[tokio::test]
async fn a_session_stored_as_prosage_comes_back_as_sage_without_the_retired_mechanisms() {
    use crate::provider::mock::MockTurn;
    let h = harness("resume-prosage");
    let cwd = resume_workspace(&h);
    let req = StartRequest {
        cwd,
        task: "hello".into(),
        interactive: true,
        ..Default::default()
    };
    let meta = h
        .start_scripted(req, MockProvider::scripted(vec![MockTurn::say("ok")]))
        .unwrap();
    until_idle(&h, &meta.id).await;
    let dir = session_dir(&h, &meta.id);
    let data = h.inner.data_dir.clone();
    drop(h);

    // What a session of the Prosage mode stored before the mode was taken out.
    let mut snapshot = resume::read(&dir).unwrap();
    snapshot.setup.mode = "prosage".into();
    snapshot.setup.without = vec!["examiner".into(), "watches".into()];
    snapshot.setup.with = vec!["long-thought".into()];
    resume::write_atomic(
        &dir.join(resume::SNAPSHOT),
        &serde_json::to_vec(&snapshot).unwrap(),
    )
    .unwrap();

    let back = Harness::at(data);
    back.resume_stand_ins(true);
    listed_free(&back).await;
    back.resume_scripted(&meta.id, MockProvider::scripted(vec![MockTurn::say("ok")]))
        .unwrap();
    back.send_with_skills(&meta.id, "again", &[]).unwrap();
    until_idle(&back, &meta.id).await;
    let setup = resume::read(&dir).unwrap().setup;
    assert_eq!(
        (
            setup.mode.as_str(),
            setup.with.len(),
            setup.without.as_slice()
        ),
        ("sage", 0, &["watches".to_string()][..])
    );
}

#[tokio::test]
async fn a_session_is_continued_by_another_harness_with_its_conversation_numbering_and_goal() {
    use crate::provider::mock::MockTurn;
    use serde_json::json;
    let (first, id, _, first_seen) = run_with_a_goal_left_open("resume-a").await;
    let data = first.inner.data_dir.clone();
    let ended = first.meta(&id).unwrap();
    assert_eq!(
        (ended.outcome, ended.goal.clone()),
        (
            Some(FinishStatus::Incomplete),
            Some(Goal::new("n.txt is all upper case"))
        )
    );
    let log_before = first.events(&id).unwrap();
    let last_seq = log_before.last().unwrap().seq;
    let snapshot = resume::read(&session_dir(&first, &id)).unwrap();
    assert!(
        !snapshot.in_run
            && snapshot.setup.without == ["survey"]
            && snapshot.turn == ended.stats.turns
            && snapshot.stats == ended.stats
    );

    // The session is still run here: another process, or harness, must keep out.
    let second = Harness::at(data.clone());
    second.resume_stand_ins(true);
    assert_eq!(second.list()[0].why_closed.as_deref(), Some("busy"));
    let busy = second
        .resume_scripted(&id, MockProvider::scripted(vec![]))
        .unwrap_err();
    assert!(busy.contains("only one process"), "{busy}");
    assert!(second.stop(&id).unwrap_err().contains("another process"));
    drop(first);

    // Now it is stored: listed as one that can be continued, with what it had.
    let listed = [listed_free(&second).await];
    assert_eq!(
        (
            listed[0].status,
            listed[0].why_closed.clone(),
            listed[0].stats.turns,
            listed[0].goal.clone()
        ),
        (
            SessionStatus::Resumable,
            None,
            ended.stats.turns,
            ended.goal.clone()
        )
    );
    assert_eq!(second.meta(&id).unwrap().status, SessionStatus::Resumable);
    // Nothing is running here to stop or to approve, and no live feed to give; the log can be read.
    assert!(second.stop(&id).is_ok());
    assert!(
        second
            .approve(&id, 1, true)
            .unwrap_err()
            .contains("not running")
    );
    assert!(
        second
            .subscribe(&id)
            .unwrap_err()
            .contains("stored, not running")
    );
    assert_eq!(second.events(&id).unwrap().len(), log_before.len());
    assert!(
        second
            .subscribe("nothing-like-it")
            .unwrap_err()
            .contains("not running in this process")
    );

    let seen = Seen::default();
    second
        .resume_scripted(
            &id,
            recording(
                &seen,
                vec![
                    MockTurn::calls("", vec![("finish", json!({"summary": "still upper case"}))]),
                    MockTurn::say("MET\nboth lines are upper case"),
                    MockTurn::say("noted"),
                ],
            ),
        )
        .unwrap();
    second
        .send_with(&id, "and keep it that way", &[], &[], Changes::default())
        .unwrap();
    let after = until_idle(&second, &id).await;

    // The model is sent what it had, then the new message in front of which it is told of the restart.
    let requests = seen.lock().unwrap().clone();
    let sent = &requests[0];
    assert_eq!(sent.len(), snapshot.messages.len() + 1);
    assert_eq!(sent[..snapshot.messages.len()], snapshot.messages[..]);
    let last_request_of_the_first_run = first_seen
        .lock()
        .unwrap()
        .iter()
        .rfind(|m| m.iter().any(|m| m.content.contains("<task>")))
        .unwrap()
        .clone();
    assert_eq!(
        sent[..last_request_of_the_first_run.len()],
        last_request_of_the_first_run[..],
        "everything the model saw last time is a prefix of what it sees now"
    );
    let message = sent.last().unwrap();
    assert!(
        message.role == crate::provider::Role::User
            && message
                .content
                .starts_with("[harness] This session was restored after a restart"),
        "{}",
        message.content
    );
    assert!(
        message.content.ends_with("and keep it that way") && !message.content.contains("cut off")
    );
    assert_eq!(sent[0].role, crate::provider::Role::System);
    // The note is said once.
    second
        .send_with(&id, "thanks", &[], &[], Changes::default())
        .unwrap();
    until_idle(&second, &id).await;
    assert_eq!(
        seen.lock().unwrap().last().unwrap().last().unwrap().content,
        "thanks"
    );

    // The log goes on from where it stopped, in order, with the resumption marked.
    let text = std::fs::read_to_string(session_dir(&second, &id).join("events.jsonl")).unwrap();
    let log: Vec<Event> = text
        .lines()
        .map(|l| serde_json::from_str(l).expect("every line is an event"))
        .collect();
    assert_eq!(
        log[..log_before.len()]
            .iter()
            .map(|e| e.seq)
            .collect::<Vec<_>>(),
        log_before.iter().map(|e| e.seq).collect::<Vec<_>>()
    );
    assert!(log.windows(2).all(|w| w[0].seq < w[1].seq));
    assert_eq!(log[log_before.len()].seq, last_seq + 1);
    assert!(matches!(
        log[log_before.len()].kind,
        EventKind::Resumed {
            cut_off: false,
            answered: 0
        }
    ));
    assert!(
        matches!(&log[log_before.len() + 1].kind, EventKind::User { text, .. } if text == "and keep it that way")
    );
    let turns: Vec<u32> = log
        .iter()
        .filter_map(|e| {
            if let EventKind::TurnStart { turn, .. } = e.kind {
                Some(turn)
            } else {
                None
            }
        })
        .collect();
    assert!(
        turns.windows(2).all(|w| w[0] < w[1]) && *turns.last().unwrap() > ended.stats.turns,
        "{turns:?}"
    );

    // Meta and stats carry on; the goal, still active when the first run ended, was met by the second.
    assert!(
        after.stats.turns > ended.stats.turns
            && after.stats.goal_checks > ended.stats.goal_checks
            && after.stats.cost >= ended.stats.cost
    );
    assert_eq!(
        after.goal.as_ref().map(|g| (g.text.as_str(), g.met)),
        Some(("n.txt is all upper case", true))
    );
    assert!(
        goals(&second, &id)
            .iter()
            .any(|(state, ..)| *state == GoalState::Met)
    );
    assert_eq!(second.list()[0].title, ended.title);
    let stored: SessionMeta = serde_json::from_str(
        &std::fs::read_to_string(session_dir(&second, &id).join("meta.json")).unwrap(),
    )
    .unwrap();
    assert!(stored.why_closed.is_none() && stored.goal.unwrap().met);
    let later = resume::read(&session_dir(&second, &id)).unwrap();
    assert!(later.turn > snapshot.turn && later.messages.len() > snapshot.messages.len());
}

#[tokio::test]
async fn a_snapshot_cut_off_in_the_middle_of_a_run_is_made_valid_and_the_model_is_told() {
    use crate::provider::Role;
    use crate::provider::mock::MockTurn;
    use serde_json::json;
    let h = harness("resume-cut");
    let cwd = resume_workspace(&h);
    let turns = vec![
        MockTurn::calls(
            "",
            vec![
                ("shell", json!({"cmd": "echo one"})),
                ("shell", json!({"cmd": "echo two"})),
            ],
        ),
        MockTurn::say("both said hello"),
    ];
    let id = h
        .start_scripted(
            StartRequest {
                cwd,
                task: "say hello twice".into(),
                without: vec!["survey".into()],
                ..Default::default()
            },
            MockProvider::scripted(turns),
        )
        .unwrap()
        .id;
    until_idle(&h, &id).await;
    let dir = session_dir(&h, &id);
    let mut snapshot = resume::read(&dir).unwrap();
    // As if the program was killed while the two calls ran: the reply is stored, their results are not.
    let at = snapshot
        .messages
        .iter()
        .rposition(|m| m.role == Role::Assistant && !m.tool_calls.is_empty())
        .unwrap();
    snapshot.messages.truncate(at + 1);
    snapshot.stubs.truncate(at + 1);
    snapshot.in_run = true;
    resume::write_atomic(
        &dir.join(resume::SNAPSHOT),
        &serde_json::to_vec(&snapshot).unwrap(),
    )
    .unwrap();
    let calls: Vec<String> = snapshot.messages[at]
        .tool_calls
        .iter()
        .map(|c| c.id.clone())
        .collect();
    assert_eq!(calls.len(), 2);
    h.close(&id).await.unwrap();
    drop(h);

    let second = Harness::at(dir.parent().unwrap().parent().unwrap().to_path_buf());
    let seen = Seen::default();
    second
        .resume_scripted(&id, recording(&seen, vec![MockTurn::say("understood")]))
        .unwrap();
    second
        .send_with(&id, "go on", &[], &[], Changes::default())
        .unwrap();
    until_idle(&second, &id).await;

    let sent = seen.lock().unwrap()[0].clone();
    assert_eq!(sent.len(), at + 1 + 2 + 1);
    for (n, call) in calls.iter().enumerate() {
        let result = &sent[at + 1 + n];
        assert!(
            result.role == Role::Tool
                && result.tool_call_id.as_deref() == Some(call.as_str())
                && result.content.contains("interrupted by a restart"),
            "{result:?}"
        );
    }
    let note = &sent.last().unwrap().content;
    assert!(
        note.contains("cut off by the restart: the tool calls it ended on were not finished")
            && note.ends_with("go on"),
        "{note}"
    );
    assert!(second.events(&id).unwrap().iter().any(|e| matches!(
        e.kind,
        EventKind::Resumed {
            cut_off: true,
            answered: 2
        }
    )));
}

#[test]
fn dangling_calls_are_answered_where_they_stand() {
    use crate::provider::{Message, Role, ToolCall};
    let call = |id: &str| ToolCall {
        id: id.into(),
        name: "shell".into(),
        arguments: "{}".into(),
    };
    let mut context = crate::context::Context::new("sys");
    context.push(Message::user("task"));
    context.push(Message {
        role: Role::Assistant,
        content: String::new(),
        reasoning: None,
        tool_calls: vec![call("a"), call("b")],
        tool_call_id: None,
    });
    context.push_tool("a", "one".into(), "[a]".into());
    context.push(Message::user("[harness] carry on"));
    context.push(Message {
        role: Role::Assistant,
        content: String::new(),
        reasoning: None,
        tool_calls: vec![call("c")],
        tool_call_id: None,
    });
    assert_eq!(context.answer_dangling("cut off"), 2);
    let shape: Vec<(Role, Option<&str>)> = context
        .messages
        .iter()
        .map(|m| (m.role, m.tool_call_id.as_deref()))
        .collect();
    assert_eq!(
        shape,
        [
            (Role::System, None),
            (Role::User, None),
            (Role::Assistant, None),
            (Role::Tool, Some("a")),
            (Role::Tool, Some("b")),
            (Role::User, None),
            (Role::Assistant, None),
            (Role::Tool, Some("c"))
        ]
    );
    assert_eq!(context.stubs().len(), context.messages.len());
    assert_eq!(
        context.answer_dangling("cut off"),
        0,
        "a conversation that is whole is left as it is"
    );
}

#[tokio::test]
async fn a_session_without_a_snapshot_is_read_only_and_says_why() {
    use crate::provider::mock::MockTurn;
    let h = harness("resume-older");
    let cwd = resume_workspace(&h);
    let id = h
        .start_scripted(
            StartRequest {
                cwd,
                task: "say hi".into(),
                ..Default::default()
            },
            MockProvider::scripted(vec![MockTurn::say("hi")]),
        )
        .unwrap()
        .id;
    until_idle(&h, &id).await;
    let data = h.inner.data_dir.clone();
    drop(h);
    let second = Harness::at(data);
    // A stand-in's session is not continued by the app unless it is asked to.
    assert_eq!(
        listed_free(&second).await.why_closed.as_deref(),
        Some("simulated")
    );
    second.resume_stand_ins(true);
    assert_eq!(listed_free(&second).await.status, SessionStatus::Resumable);
    // Stored before sessions kept snapshots: the log has no system prompt and not what the model
    // was shown beyond the user's words, so a conversation rebuilt from it would not be the same one.
    std::fs::remove_file(session_dir(&second, &id).join(resume::SNAPSHOT)).unwrap();
    let listed = [listed_free(&second).await];
    assert_eq!(
        (listed[0].status, listed[0].why_closed.as_deref()),
        (SessionStatus::Closed, Some("older"))
    );
    let refused = second
        .resume_scripted(&id, MockProvider::scripted(vec![]))
        .unwrap_err();
    assert!(
        refused.contains("cannot be continued") && refused.contains("no snapshot"),
        "{refused}"
    );
    assert!(
        second
            .send_with(&id, "more", &[], &[], Changes::default())
            .is_err()
    );
    assert!(
        second
            .send_with("no-such-session", "more", &[], &[], Changes::default())
            .unwrap_err()
            .contains("no session")
    );
    assert!(
        second
            .send_with("no-such-session", " ", &[], &[], Changes::default())
            .unwrap_err()
            .contains("empty")
    );
    assert!(!second.events(&id).unwrap().is_empty());
}

#[tokio::test]
async fn a_real_session_comes_back_through_the_current_configuration() {
    use crate::provider::mock::MockTurn;
    let (first, id, _, _) = run_with_a_goal_left_open("resume-real").await;
    let data = first.inner.data_dir.clone();
    let dir = session_dir(&first, &id);
    drop(first);
    // As if it had been started on a real provider.
    let edit = |provider: &str| {
        let mut meta: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("meta.json")).unwrap()).unwrap();
        meta["simulated"] = false.into();
        meta["provider"] = provider.into();
        meta["model"] = "qwen3".into();
        std::fs::write(dir.join("meta.json"), meta.to_string()).unwrap();
    };
    let h = Harness::at(data);
    let own = h
        .add_provider("Local", "http://127.0.0.1:9/v1", &["qwen3".into()], "")
        .unwrap();

    // Its provider is gone: a plain message says so; one that names another model does not need it.
    edit("a-provider-that-was-removed");
    assert_eq!(listed_free(&h).await.status, SessionStatus::Resumable);
    let refused = h.resume(&id).unwrap_err();
    assert!(
        refused.contains("cannot be continued")
            && refused.contains("a-provider-that-was-removed")
            && refused.contains("cannot be reached"),
        "{refused}"
    );
    assert!(
        h.resume_scripted(&id, MockProvider::scripted(vec![MockTurn::say("x")]))
            .unwrap_err()
            .contains("not played by a stand-in")
    );
    let changes = Changes {
        provider: Some(own.clone()),
        model: Some("qwen3".into()),
        ..Default::default()
    };
    h.revive(&id, None, &changes).unwrap();
    assert_eq!(
        (h.meta(&id).unwrap().status, h.is_live(&id)),
        (SessionStatus::Idle, true)
    );
    h.close(&id).await.unwrap();

    // Its provider is there: the session comes back idle, with nothing sent anywhere.
    edit(&own);
    let meta = h.resume(&id).unwrap();
    assert_eq!(
        (meta.status, meta.provider.as_str(), meta.model.as_str()),
        (SessionStatus::Idle, own.as_str(), "qwen3")
    );
    assert_eq!(
        h.resume(&id).unwrap().id,
        id,
        "resuming a session that is live is no error"
    );
    assert!(
        h.events(&id)
            .unwrap()
            .iter()
            .any(|e| matches!(e.kind, EventKind::Resumed { .. }))
    );
    assert!(h.subscribe(&id).is_ok());
}

#[tokio::test]
async fn a_log_that_was_cut_short_does_not_swallow_the_next_event() {
    use crate::provider::mock::MockTurn;
    let h = harness("resume-torn");
    let cwd = resume_workspace(&h);
    let id = h
        .start_scripted(
            StartRequest {
                cwd,
                task: "say hi".into(),
                ..Default::default()
            },
            MockProvider::scripted(vec![MockTurn::say("hi"), MockTurn::say("again")]),
        )
        .unwrap()
        .id;
    until_idle(&h, &id).await;
    let data = h.inner.data_dir.clone();
    let log = session_dir(&h, &id).join("events.jsonl");
    h.close(&id).await.unwrap();
    drop(h);
    let mut text = std::fs::read_to_string(&log).unwrap();
    text.push_str("{\"seq\": 99, \"ts\": 1, \"type\": \"fin");
    std::fs::write(&log, &text).unwrap();
    let second = Harness::at(data);
    second
        .resume_scripted(&id, MockProvider::scripted(vec![MockTurn::say("again")]))
        .unwrap();
    second
        .send_with(&id, "once more", &[], &[], Changes::default())
        .unwrap();
    until_idle(&second, &id).await;
    let lines = std::fs::read_to_string(&log).unwrap();
    let unreadable = lines
        .lines()
        .filter(|l| serde_json::from_str::<Event>(l).is_err())
        .count();
    assert_eq!(unreadable, 1, "only the torn line is lost");
    assert!(
        second
            .events(&id)
            .unwrap()
            .iter()
            .any(|e| matches!(&e.kind, EventKind::User { text, .. } if text == "once more"))
    );
}

#[tokio::test]
async fn closing_every_session_leaves_each_one_continuable_and_frees_it() {
    use crate::provider::mock::MockTurn;
    let h = harness("resume-quit");
    h.resume_stand_ins(true);
    let cwd = resume_workspace(&h);
    let id = h
        .start_scripted(
            StartRequest {
                cwd,
                task: "say hi".into(),
                ..Default::default()
            },
            MockProvider::scripted(vec![MockTurn::say("hi")]),
        )
        .unwrap()
        .id;
    until_idle(&h, &id).await;
    assert_eq!(h.close_all().await, 1);
    assert_eq!((h.close_all().await, h.is_live(&id)), (0, false));
    assert_eq!(
        listed_free(&h).await.status,
        SessionStatus::Resumable,
        "the app that quits can be opened again and the session sent a message"
    );
    h.resume_scripted(&id, MockProvider::scripted(vec![]))
        .unwrap();
    assert!(h.is_live(&id));
}
