//! Other agent harnesses as benchmark variants. `ext:<name>` runs `<dir>/<name>/run.sh` with the
//! task's workspace as its working directory; the model is reached through the metering proxy, so
//! every harness is counted and priced alike.

use super::proxy::{Metered, Proxy};
use sage_core::util::one_line;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

/// How long a harness gets to wind down once its turns are used up.
const GRACE: Duration = Duration::from_secs(20);
/// What an adapter inherits from the environment. Everything else, API keys above all, is withheld.
const INHERITED: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LC_ALL",
    "TMPDIR",
    "TZ",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "BENCH_EXT_HOME",
];
/// Exit code by which an adapter says that its harness ran into its own step limit.
const STEP_LIMIT: i32 = 124;
/// The harness never holds the real key: the proxy adds it.
const PLACEHOLDER_KEY: &str = "sk-bench-proxy";

/// The adapters shipped with the source, unless the working directory has its own.
pub fn default_dir() -> PathBuf {
    let local = PathBuf::from("scripts/external");
    if local.is_dir() {
        local
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/external")
    }
}

pub fn script(dir: &Path, name: &str) -> PathBuf {
    dir.join(name).join("run.sh")
}

pub struct Job<'a> {
    /// Names the run in the proxy and on disk.
    pub id: &'a str,
    pub script: &'a Path,
    pub workspace: &'a Path,
    /// Holds the run's output log and, while it runs, the harness's scratch directory.
    pub home: &'a Path,
    pub prompt: &'a str,
    pub model: &'a str,
    pub max_turns: u32,
    pub time_budget: u64,
    pub keep: bool,
}

/// The part of an external run that Sage's own runs have no counterpart for.
#[derive(Clone, Debug, Serialize)]
pub struct Detail {
    pub exit_code: Option<i32>,
    /// What the harness printed.
    pub log: String,
    pub usage: Metered,
}

pub struct Ran {
    /// done, budget or error, as for Sage's runs.
    pub outcome: &'static str,
    /// Why the run failed, when the harness or its adapter was the cause.
    pub failure: Option<String>,
    /// Set when the provider failed and the run has no result.
    pub error: Option<String>,
    pub elapsed_ms: u64,
    pub detail: Detail,
}

fn kill_group(pid: u32) {
    // SAFETY: a plain syscall; the process group is the child's own (`process_group(0)`).
    #[cfg(unix)]
    unsafe {
        libc::killpg(pid as libc::pid_t, libc::SIGKILL);
    }
    // Windows has no process groups of this kind: the process is ended with what it started.
    #[cfg(not(unix))]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output();
    }
}

fn last_lines(path: &Path, n: usize) -> String {
    let text = std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Sorts out how a run ended: (outcome, failure, error).
fn classify(
    exit: Option<i32>,
    timed_out: bool,
    job: &Job<'_>,
    usage: &Metered,
    said: &str,
) -> (&'static str, Option<String>, Option<String>) {
    let hint = usage
        .last_error
        .clone()
        .unwrap_or_else(|| one_line(said.lines().last().unwrap_or("it printed nothing"), 200));
    let provider_down = usage.last_error.is_some()
        && (usage.last_status == 0 || usage.last_status == 429 || usage.last_status >= 500);
    if usage.refused > 0 {
        (
            "budget",
            Some(format!(
                "stopped at the limit of {} model requests",
                job.max_turns
            )),
            None,
        )
    } else if provider_down {
        ("error", None, usage.last_error.clone())
    } else if timed_out {
        (
            "budget",
            Some(format!("killed at the time budget of {}s", job.time_budget)),
            None,
        )
    } else if exit == Some(STEP_LIMIT) {
        (
            "budget",
            Some("stopped at its own step limit".to_string()),
            None,
        )
    } else if exit != Some(0) {
        let how = exit.map_or("killed by a signal".to_string(), |code| {
            format!("exit code {code}")
        });
        ("error", Some(format!("{how}: {hint}")), None)
    } else if usage.turns == 0 {
        (
            "error",
            Some(format!("made no model request: {hint}")),
            None,
        )
    } else {
        ("done", None, None)
    }
}

/// Runs one adapter to its end, to the time budget, or until its turns are used up.
pub async fn run(proxy: &Proxy, job: &Job<'_>) -> Ran {
    let started = Instant::now();
    let log = job.home.join("output.log");
    let scratch = job.home.join("scratch");
    let (base_url, meter) = proxy.open(job.id, job.max_turns);
    let spawned = (|| {
        let _ = std::fs::remove_dir_all(job.home);
        std::fs::create_dir_all(&scratch)?;
        let out = std::fs::File::create(&log)?;
        let mut cmd = tokio::process::Command::new(job.script);
        cmd.env_clear();
        for name in INHERITED {
            if let Some(value) = std::env::var_os(name) {
                cmd.env(name, value);
            }
        }
        cmd.current_dir(job.workspace)
            .env("BENCH_PROMPT", job.prompt)
            .env("BENCH_MODEL", job.model)
            .env("BENCH_BASE_URL", &base_url)
            .env("BENCH_API_KEY", PLACEHOLDER_KEY)
            .env("BENCH_MAX_TURNS", job.max_turns.to_string())
            .env("BENCH_TIME_BUDGET", job.time_budget.to_string())
            .env("BENCH_SCRATCH", &scratch)
            .stdin(Stdio::null())
            .stderr(out.try_clone()?)
            .stdout(out)
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);
        cmd.spawn()
    })();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => {
            let usage = proxy.close(job.id).await;
            let failure = Some(format!("could not start {}: {e}", job.script.display()));
            return Ran {
                outcome: "error",
                failure,
                error: None,
                elapsed_ms: 0,
                detail: Detail {
                    exit_code: None,
                    log: log.to_string_lossy().into_owned(),
                    usage,
                },
            };
        }
    };
    let pid = child.id();
    let budget = tokio::time::sleep(Duration::from_secs(job.time_budget));
    tokio::pin!(budget);
    let (status, timed_out) = tokio::select! {
        status = child.wait() => (status.ok(), false),
        _ = &mut budget => (None, true),
        _ = meter.capped.notified() => (tokio::time::timeout(GRACE, child.wait()).await.ok().and_then(Result::ok), false),
    };
    // Whatever the harness left running goes with it.
    if let Some(pid) = pid {
        kill_group(pid);
    }
    let status = match status {
        Some(status) => Some(status),
        None => child.wait().await.ok(),
    };
    let exit = status.and_then(|s| s.code());
    let usage = proxy.close(job.id).await;
    let said = last_lines(&log, 12);
    let (outcome, failure, error) = classify(exit, timed_out, job, &usage, &said);
    if !job.keep {
        let _ = std::fs::remove_dir_all(&scratch);
    }
    Ran {
        outcome,
        failure,
        error,
        elapsed_ms: started.elapsed().as_millis() as u64,
        detail: Detail {
            exit_code: exit,
            log: log.to_string_lossy().into_owned(),
            usage,
        },
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use sage_core::util::rand_hex;
    use std::os::unix::fs::PermissionsExt;

    fn job<'a>(script: &'a Path, root: &'a Path) -> Job<'a> {
        Job {
            id: "t",
            script,
            workspace: root,
            home: root,
            prompt: "p",
            model: "m",
            max_turns: 3,
            time_budget: 600,
            keep: false,
        }
    }

    #[test]
    fn a_run_is_sorted_by_how_it_ended() {
        let (script, root) = (PathBuf::from("run.sh"), PathBuf::from("."));
        let job = job(&script, &root);
        let ok = Metered {
            turns: 4,
            ..Metered::default()
        };
        assert_eq!(
            classify(Some(0), false, &job, &ok, "bye"),
            ("done", None, None)
        );
        assert_eq!(
            classify(Some(3), false, &job, &ok, "a\nTraceback: boom"),
            ("error", Some("exit code 3: Traceback: boom".into()), None)
        );
        assert_eq!(
            classify(None, false, &job, &ok, ""),
            (
                "error",
                Some("killed by a signal: it printed nothing".into()),
                None
            )
        );
        assert_eq!(classify(None, true, &job, &ok, "").0, "budget");
        assert_eq!(
            classify(Some(124), false, &job, &ok, ""),
            ("budget", Some("stopped at its own step limit".into()), None)
        );
        assert_eq!(
            classify(Some(0), false, &job, &Metered::default(), "no key").1,
            Some("made no model request: no key".into())
        );
        let capped = Metered {
            turns: 3,
            refused: 1,
            ..Metered::default()
        };
        assert_eq!(
            classify(Some(1), false, &job, &capped, ""),
            (
                "budget",
                Some("stopped at the limit of 3 model requests".into()),
                None
            )
        );

        // The provider failing is no result; the provider rejecting what the harness sent is the harness's failure.
        let down = Metered {
            failed: 2,
            last_error: Some("provider returned HTTP 503: overloaded".into()),
            last_status: 503,
            ..Metered::default()
        };
        assert_eq!(
            classify(Some(1), false, &job, &down, "x"),
            (
                "error",
                None,
                Some("provider returned HTTP 503: overloaded".into())
            )
        );
        let rejected = Metered {
            failed: 1,
            last_error: Some("provider returned HTTP 400: unknown parameter".into()),
            last_status: 400,
            ..Metered::default()
        };
        assert_eq!(
            classify(Some(1), false, &job, &rejected, "x"),
            (
                "error",
                Some("exit code 1: provider returned HTTP 400: unknown parameter".into()),
                None
            )
        );
    }

    fn adapter(root: &Path, body: &str) -> PathBuf {
        let path = root.join("run.sh");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[tokio::test]
    async fn an_adapter_gets_its_environment_and_is_killed_with_its_children_at_the_time_budget() {
        let root = std::env::temp_dir().join(format!("sage-ext-{}", rand_hex(4)));
        let (ws, home) = (root.join("ws"), root.join("home"));
        std::fs::create_dir_all(&ws).unwrap();
        assert!(
            std::env::var_os("CARGO_MANIFEST_DIR").is_some(),
            "cargo sets this for tests; the adapter must not see it"
        );
        let proxy = Proxy::start("http://127.0.0.1:1/v1", "real-key")
            .await
            .unwrap();

        let script = adapter(
            &root,
            r#"echo "$BENCH_PROMPT|$BENCH_MODEL|$BENCH_MAX_TURNS|$BENCH_TIME_BUDGET|$BENCH_API_KEY|${CARGO_MANIFEST_DIR:-withheld}" > seen.txt
echo "$BENCH_BASE_URL" > "$BENCH_SCRATCH/url"; ls -A "$BENCH_SCRATCH" > scratch.txt; echo "no key configured" >&2; exit 0"#,
        );
        let quick = Job {
            id: "quick",
            script: &script,
            workspace: &ws,
            home: &home,
            prompt: "fix `it`",
            model: "glm-5.3",
            max_turns: 7,
            time_budget: 30,
            keep: false,
        };
        let ran = run(&proxy, &quick).await;
        assert_eq!(
            (ran.outcome, ran.failure.as_deref(), ran.error),
            (
                "error",
                Some("made no model request: no key configured"),
                None
            )
        );
        assert_eq!(
            std::fs::read_to_string(ws.join("seen.txt")).unwrap().trim(),
            "fix `it`|glm-5.3|7|30|sk-bench-proxy|withheld"
        );
        assert_eq!(
            std::fs::read_to_string(ws.join("scratch.txt"))
                .unwrap()
                .trim(),
            "url",
            "the scratch directory starts empty"
        );
        assert!(!home.join("scratch").exists() && home.join("output.log").is_file());

        let script = adapter(&root, "sleep 300 &\necho $! > child.pid\nwait");
        let slow = Job {
            id: "slow",
            script: &script,
            time_budget: 1,
            ..quick
        };
        let ran = run(&proxy, &slow).await;
        assert_eq!(
            (ran.outcome, ran.failure.as_deref()),
            ("budget", Some("killed at the time budget of 1s"))
        );
        assert!(ran.elapsed_ms < 10_000);
        let pid = std::fs::read_to_string(ws.join("child.pid"))
            .unwrap()
            .trim()
            .to_string();
        tokio::time::sleep(Duration::from_millis(200)).await;
        let alive = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .is_ok_and(|stat| !stat.contains(") Z "));
        assert!(
            !alive || !Path::new("/proc").is_dir(),
            "the harness's child {pid} outlived it"
        );

        let missing = Job {
            id: "missing",
            script: Path::new("/nonexistent/run.sh"),
            ..quick
        };
        assert!(
            run(&proxy, &missing)
                .await
                .failure
                .unwrap()
                .starts_with("could not start /nonexistent/run.sh")
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
