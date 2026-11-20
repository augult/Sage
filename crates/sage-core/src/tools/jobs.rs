//! Process plumbing: spawning commands in their own process group, capturing output without unbounded
//! memory, and keeping long-running commands alive as jobs instead of killing them at a timeout.

use std::collections::VecDeque;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, BufWriter};
use tokio::process::Command;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

const HEAD_CAP: usize = 48 * 1024;
const TAIL_CAP: usize = 256 * 1024;
/// How much of one command's output the spill file keeps. A command that never stops printing (a
/// loop, a server left chattering in the background) would otherwise fill the disk.
const SPILL_CAP: u64 = 256 * 1024 * 1024;

/// Captured output: the first and last parts stay in memory, everything goes to the spill file.
#[derive(Default)]
pub struct OutBuf {
    head: Vec<u8>,
    tail: VecDeque<u8>,
    dropped: bool,
    pub total: u64,
}

impl OutBuf {
    pub fn push(&mut self, mut data: &[u8]) {
        self.total += data.len() as u64;
        if self.head.len() < HEAD_CAP {
            let n = (HEAD_CAP - self.head.len()).min(data.len());
            self.head.extend_from_slice(&data[..n]);
            data = &data[n..];
        }
        if !data.is_empty() {
            self.tail.extend(data);
            let excess = self.tail.len().saturating_sub(TAIL_CAP);
            if excess > 0 {
                self.tail.drain(..excess);
                self.dropped = true;
            }
        }
    }

    /// Retained bytes and whether a middle section exists only in the spill file.
    pub fn snapshot(&self) -> (Vec<u8>, bool) {
        let mut out = self.head.clone();
        let (a, b) = self.tail.as_slices();
        if self.dropped {
            // Resume at a line boundary so the cut does not produce a half line.
            out.extend_from_slice(b"\n");
            let joined: Vec<u8> = a.iter().chain(b.iter()).copied().collect();
            let start = joined
                .iter()
                .position(|c| *c == b'\n')
                .map(|p| p + 1)
                .unwrap_or(0);
            out.extend_from_slice(&joined[start..]);
        } else {
            out.extend_from_slice(a);
            out.extend_from_slice(b);
        }
        (out, self.dropped)
    }

    /// Bytes produced after `offset`, as far as they are still in memory.
    pub fn since(&self, offset: u64) -> Vec<u8> {
        let retained = self.head.len() + self.tail.len();
        let wanted = self.total.saturating_sub(offset).min(retained as u64) as usize;
        if self.dropped {
            return self
                .tail
                .iter()
                .skip(self.tail.len() - wanted.min(self.tail.len()))
                .copied()
                .collect();
        }
        self.head
            .iter()
            .chain(self.tail.iter())
            .skip(retained - wanted)
            .copied()
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exit {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

pub struct Running {
    pub pid: Option<u32>,
    pub buf: Arc<Mutex<OutBuf>>,
    pub exit: watch::Receiver<Option<Exit>>,
    pub output: watch::Receiver<u64>,
    pub readers: Vec<JoinHandle<()>>,
    pub started: Instant,
}

/// How a command relates to the session's persistent shell state (cwd and exported variables).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateMode<'a> {
    /// Restore state before the command and save it after: ordinary `shell` calls.
    ReadWrite(&'a Path),
    /// Restore only: watches and checks must not move the session's cwd.
    ReadOnly(&'a Path),
    None,
}

// The state file is sourced, then rewritten on exit. PWD-like variables are excluded so a restored
// environment can never disagree with the directory the command actually runs in.
//
// Both variants also record PIPESTATUS of the last pipeline next to the spill file, so a failure
// hidden behind `| tail` can be told apart from a success (see `masked_failure`).
//
// All of it comes before the first line of the command, on the same line where bash counts the
// lines of a `-c` string from 1 and on a line of its own where it counts them from 0 (bash 3.2, the
// one macOS ships), so that the line numbers bash reports for an error are those of the command
// as the model wrote it. A command that has been handed over as a background job (see `detach`)
// no longer saves: when it ends, later commands have moved on.
#[cfg(unix)]
const RESTORE: &str = r#"__ps="$SAGE_STATE"; __pp="$SAGE_PIPE"; unset SAGE_STATE SAGE_PIPE; [ -f "$__ps.env" ] && . "$__ps.env" >/dev/null 2>&1; "#;
#[cfg(unix)]
const SAVE: &str = r#"trap '__rc=$? __pipe="${PIPESTATUS[*]}"; echo "$__pipe" > "$__pp" 2>/dev/null; [ -e "$__pp.detached" ] || { pwd > "$__ps.cwd" 2>/dev/null; export -p 2>/dev/null | grep -vE "^declare -x (PWD|OLDPWD|SHLVL|_)=" > "$__ps.env" 2>/dev/null; }; exit $__rc' EXIT; "#;
#[cfg(unix)]
const NOTE_PIPE: &str = r#"trap '__rc=$? __pipe="${PIPESTATUS[*]}"; echo "$__pipe" > "$__pp" 2>/dev/null; exit $__rc' EXIT; "#;
// Bash 3.2 does not take the OLDPWD that `spawn` gives it from the environment: `cd -` would fail.
#[cfg(unix)]
const HOME_OLDPWD: &str = "OLDPWD=$PWD; ";

/// Whether the `bash` that runs commands numbers the lines of a `-c` string from 0, as bash 3.2
/// does, rather than from 1. Asked once.
#[cfg(unix)]
fn counts_lines_from_zero() -> bool {
    static FROM_ZERO: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FROM_ZERO.get_or_init(|| {
        std::process::Command::new("bash")
            .args(["-c", "echo $LINENO"])
            .env_remove("BASH_ENV")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .is_ok_and(|out| out.stdout.trim_ascii() == b"0")
    })
}

/// Marks a still-running command as no longer the session's foreground: its end must not write
/// its directory and variables over those of the commands that have run since.
pub fn detach(spill: &Path) {
    let mut marker = pipe_file(spill).into_os_string();
    marker.push(".detached");
    let _ = std::fs::write(marker, "");
}

/// Programs whose exit 1 is an answer ("no match", "they differ") rather than a failure.
const ANSWERS_WITH_ONE: &[&str] = &["grep", "egrep", "fgrep", "rg", "ag", "diff", "cmp"];

/// The stages of the last pipeline on a command line, cut at `|` outside quotes.
fn last_pipeline(cmd: &str) -> Vec<String> {
    let mut pipelines: Vec<Vec<String>> = vec![vec![String::new()]];
    let mut quote: Option<char> = None;
    let mut chars = cmd.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '\'' | '"' => quote = Some(c),
                ';' | '\n' => {
                    pipelines.push(vec![String::new()]);
                    continue;
                }
                '&' | '|' if chars.peek() == Some(&c) => {
                    chars.next();
                    pipelines.push(vec![String::new()]);
                    continue;
                }
                '|' => {
                    pipelines.last_mut().unwrap().push(String::new());
                    continue;
                }
                _ => {}
            },
        }
        pipelines.last_mut().unwrap().last_mut().unwrap().push(c);
    }
    pipelines
        .into_iter()
        .rev()
        .find(|p| p.iter().any(|stage| !stage.trim().is_empty()))
        .unwrap_or_default()
}

/// If the command's last pipeline exited 0 only because its final stage did (`pytest | tail`),
/// returns the exit code of the stage that failed. SIGPIPE (141) is not a failure: `cat big | head`.
/// Nor is the 1 of a stage that searches or compares: `grep -c x log | cat`, `diff a b | head`.
pub fn masked_failure(spill: &Path, cmd: &str) -> Option<i32> {
    let text = std::fs::read_to_string(pipe_file(spill)).ok()?;
    let codes: Vec<i32> = text
        .split_whitespace()
        .filter_map(|c| c.parse().ok())
        .collect();
    let (last, earlier) = codes.split_last()?;
    if *last != 0 {
        return None;
    }
    let stages = last_pipeline(cmd);
    let answers = |i: usize| {
        let program = stages
            .get(i)
            .and_then(|stage| stage.split_whitespace().find(|word| !word.contains('=')))
            .map(|word| word.rsplit('/').next().unwrap_or(word));
        stages.len() == codes.len() && program.is_some_and(|p| ANSWERS_WITH_ONE.contains(&p))
    };
    earlier
        .iter()
        .enumerate()
        .find(|(i, c)| **c != 0 && **c != 141 && !(**c == 1 && answers(*i)))
        .map(|(_, c)| *c)
}

fn pipe_file(spill: &Path) -> std::path::PathBuf {
    spill.with_extension("pipe")
}

#[cfg(unix)]
fn shell_command(cmd: &str, state: StateMode<'_>) -> Command {
    let mut c = Command::new("bash");
    let prelude = match state {
        StateMode::ReadWrite(p) => {
            c.env("SAGE_STATE", p);
            format!("{RESTORE}{HOME_OLDPWD}{SAVE}")
        }
        StateMode::ReadOnly(p) => {
            c.env("SAGE_STATE", p);
            format!("{RESTORE}{HOME_OLDPWD}{NOTE_PIPE}")
        }
        StateMode::None => HOME_OLDPWD.to_string(),
    };
    let own_line = if counts_lines_from_zero() { "\n" } else { "" };
    c.arg("-c").arg(format!("{prelude}{own_line}{cmd}"));
    c.process_group(0);
    c
}

#[cfg(not(unix))]
fn shell_command(cmd: &str, _state: StateMode<'_>) -> Command {
    let mut c = Command::new("cmd");
    c.arg("/D").arg("/S").arg("/C").arg(cmd);
    c
}

async fn pump<R: AsyncRead + Unpin>(mut reader: R, chunks: mpsc::Sender<Vec<u8>>) {
    let mut chunk = [0u8; 8192];
    while let Ok(n) = reader.read(&mut chunk).await {
        if n == 0 || chunks.send(chunk[..n].to_vec()).await.is_err() {
            break;
        }
    }
}

async fn collect_output(
    mut chunks: mpsc::Receiver<Vec<u8>>,
    buf: Arc<Mutex<OutBuf>>,
    output: watch::Sender<u64>,
    path: std::path::PathBuf,
    limit: u64,
) {
    let mut file = tokio::fs::File::create(path)
        .await
        .ok()
        .map(|file| BufWriter::with_capacity(64 * 1024, file));
    while let Some(chunk) = chunks.recv().await {
        let (from, total) = {
            let mut buf = buf.lock().unwrap();
            let from = buf.total;
            buf.push(&chunk);
            (from, buf.total)
        };
        if let Some(writer) = &mut file {
            let keep = limit.saturating_sub(from).min(chunk.len() as u64) as usize;
            let mut result = writer.write_all(&chunk[..keep]).await;
            if result.is_ok() && keep < chunk.len() {
                let marker = format!(
                    "\n[the output went on past {}: the rest was not kept]\n",
                    crate::util::fmt_bytes(limit)
                );
                result = writer.write_all(marker.as_bytes()).await;
            }
            if result.is_ok() && (chunks.is_empty() || keep < chunk.len()) {
                result = writer.flush().await;
            }
            if result.is_err() || keep < chunk.len() {
                file = None;
            }
        }
        if chunks.is_empty() {
            output.send_replace(total);
        }
    }
    if let Some(writer) = &mut file {
        let _ = writer.flush().await;
    }
    output.send_replace(buf.lock().unwrap().total);
}

pub fn spawn(
    cmd: &str,
    cwd: &Path,
    envs: &[(String, String)],
    state: StateMode<'_>,
    spill: &Path,
) -> std::io::Result<Running> {
    let mut c = shell_command(cmd, state);
    c.env("SAGE_PIPE", pipe_file(spill));
    // `cd -` goes to $OLDPWD, and an inherited one is wherever Sage was launched from: a
    // directory that has nothing to do with the session. Each command starts with it at home.
    // Bash 4 and later take it from here; bash 3.2 sets its own (`HOME_OLDPWD`).
    c.env("OLDPWD", cwd);
    c.current_dir(cwd)
        .envs(envs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false);
    let mut child = c.spawn()?;
    let pid = child.id();
    let buf = Arc::new(Mutex::new(OutBuf::default()));
    let (chunks, buffered) = mpsc::channel(16);
    let (output, output_rx) = watch::channel(0);
    let mut readers = vec![tokio::spawn(collect_output(
        buffered,
        buf.clone(),
        output,
        spill.to_owned(),
        SPILL_CAP,
    ))];
    if let Some(out) = child.stdout.take() {
        readers.push(tokio::spawn(pump(out, chunks.clone())));
    }
    if let Some(err) = child.stderr.take() {
        readers.push(tokio::spawn(pump(err, chunks)));
    }
    let (tx, rx) = watch::channel(None);
    tokio::spawn(async move {
        let exit = match child.wait().await {
            Ok(status) => {
                #[cfg(unix)]
                let signal = std::os::unix::process::ExitStatusExt::signal(&status);
                #[cfg(not(unix))]
                let signal = None;
                Exit {
                    code: status.code(),
                    signal,
                }
            }
            Err(_) => Exit {
                code: None,
                signal: None,
            },
        };
        let _ = tx.send(Some(exit));
    });
    Ok(Running {
        pid,
        buf,
        exit: rx,
        output: output_rx,
        readers,
        started: Instant::now(),
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum Waited {
    Exited(Exit),
    TimedOut,
    Cancelled,
}

pub async fn wait_exit(
    rx: &mut watch::Receiver<Option<Exit>>,
    timeout: Duration,
    cancel: &CancellationToken,
) -> Waited {
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    loop {
        if let Some(exit) = *rx.borrow() {
            return Waited::Exited(exit);
        }
        tokio::select! {
            changed = rx.changed() => {
                if changed.is_err() {
                    return Waited::Exited(Exit { code: None, signal: None });
                }
            }
            _ = &mut deadline => return Waited::TimedOut,
            _ = cancel.cancelled() => return Waited::Cancelled,
        }
    }
}

/// After the shell itself exits, give its output pipes a moment to drain. A process it left in the
/// background may hold them open indefinitely, so this never waits long.
pub async fn drain(readers: &mut Vec<JoinHandle<()>>) -> bool {
    let pending: Vec<JoinHandle<()>> = std::mem::take(readers);
    tokio::time::timeout(
        Duration::from_millis(250),
        futures_util::future::join_all(pending),
    )
    .await
    .is_ok_and(|results| results.iter().all(Result::is_ok))
}

/// Terminates the whole process group: politely first, then for certain.
pub fn kill_group(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    #[cfg(unix)]
    {
        // SAFETY: plain syscalls on a pid we spawned as its own process-group leader.
        unsafe {
            libc::killpg(pid as i32, libc::SIGTERM);
        }
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            unsafe {
                libc::killpg(pid as i32, libc::SIGKILL);
            }
        });
    }
    #[cfg(not(unix))]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output();
    }
}

pub struct Job {
    pub id: String,
    pub spill: String,
    pub run: Running,
    /// How much of the output the model has already been shown.
    pub read_offset: u64,
}

#[derive(Default)]
pub struct Jobs {
    pub list: Vec<Job>,
    next: u32,
}

impl Jobs {
    pub fn add(&mut self, spill: &str, run: Running, read_offset: u64) -> String {
        self.next += 1;
        let id = format!("bg:{}", self.next);
        self.list.push(Job {
            id: id.clone(),
            spill: spill.to_string(),
            run,
            read_offset,
        });
        id
    }

    /// Whether this session has ever had a background job.
    pub fn any_started(&self) -> bool {
        self.next > 0
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Job> {
        let wanted = id.trim().trim_start_matches("bg:");
        self.list
            .iter_mut()
            .find(|j| j.id.trim_start_matches("bg:") == wanted)
    }

    #[cfg(test)]
    pub fn running(&self) -> Vec<&Job> {
        self.list
            .iter()
            .filter(|j| j.run.exit.borrow().is_none())
            .collect()
    }

    pub fn kill_all(&mut self) {
        for j in &self.list {
            if j.run.exit.borrow().is_none() {
                kill_group(j.run.pid);
            }
        }
    }
}

impl Drop for Jobs {
    fn drop(&mut self) {
        for j in &self.list {
            if j.run.exit.borrow().is_none() {
                #[cfg(unix)]
                if let Some(pid) = j.run.pid {
                    // SAFETY: as in `kill_group`; no runtime is assumed to exist during drop.
                    unsafe {
                        libc::killpg(pid as i32, libc::SIGKILL);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spill(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("sage-jobs-{}", crate::util::rand_hex(4)));
        std::fs::create_dir_all(&d).unwrap();
        crate::util::canonical(&d).unwrap_or(d).join(name)
    }

    async fn capture(path: &Path, limit: u64, line: &str, count: usize) -> Arc<Mutex<OutBuf>> {
        let buf = Arc::new(Mutex::new(OutBuf::default()));
        let (tx, rx) = mpsc::channel(16);
        let writer = tokio::spawn(collect_output(
            rx,
            buf.clone(),
            watch::channel(0).0,
            path.to_owned(),
            limit,
        ));
        for _ in 0..count {
            tx.send(line.as_bytes().to_vec()).await.unwrap();
        }
        drop(tx);
        writer.await.unwrap();
        buf
    }

    #[tokio::test]
    async fn outbuf_keeps_head_and_tail_and_spills_everything() {
        let path = spill("big.log");
        let line = format!("{}\n", "x".repeat(99));
        let buf = capture(&path, SPILL_CAP, &line, 5_000).await;
        let b = buf.lock().unwrap();
        let (snap, dropped) = b.snapshot();
        assert!(dropped);
        assert!(snap.len() <= HEAD_CAP + TAIL_CAP + 1);
        assert_eq!(b.total, 500_000);
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            500_000,
            "nothing is lost on disk"
        );
        assert!(snap.ends_with(line.as_bytes()));
        assert_eq!(
            b.since(b.total - 200),
            [line.as_bytes(), line.as_bytes()].concat()
        );
    }

    #[tokio::test]
    async fn the_spill_file_stops_growing_at_its_limit() {
        let path = spill("endless.log");
        let line = format!("{}\n", "y".repeat(99));
        let buf = capture(&path, 250, &line, 50).await;
        let b = buf.lock().unwrap();
        let kept = std::fs::read_to_string(&path).unwrap();
        assert!(
            kept.starts_with(&line.repeat(2)) && kept.ends_with("the rest was not kept]\n"),
            "{kept}"
        );
        assert!(kept.len() < 400, "{}", kept.len());
        // What the model is shown still follows the command to its end.
        assert_eq!(b.total, 5_000);
        assert!(b.snapshot().0.ends_with(line.as_bytes()));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn captures_both_streams_and_exit_code() {
        let mut run = spawn(
            "echo out; echo err >&2; exit 3",
            &std::env::temp_dir(),
            &[],
            StateMode::None,
            &spill("a.log"),
        )
        .unwrap();
        let waited = wait_exit(
            &mut run.exit,
            Duration::from_secs(10),
            &CancellationToken::new(),
        )
        .await;
        drain(&mut run.readers).await;
        assert_eq!(
            waited,
            Waited::Exited(Exit {
                code: Some(3),
                signal: None
            })
        );
        let text = String::from_utf8(run.buf.lock().unwrap().snapshot().0).unwrap();
        assert!(text.contains("out") && text.contains("err"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cd_minus_stays_in_the_session_directory() {
        // Without this an agent that runs `cd -` or `cd "$OLDPWD"` lands wherever Sage was started.
        let log = spill("d.log");
        let dir = log.parent().unwrap().to_path_buf();
        let mut run = spawn(
            "cd - >/dev/null && pwd && echo \"$OLDPWD\"",
            &dir,
            &[],
            StateMode::None,
            &log,
        )
        .unwrap();
        let waited = wait_exit(
            &mut run.exit,
            Duration::from_secs(10),
            &CancellationToken::new(),
        )
        .await;
        drain(&mut run.readers).await;
        let text = String::from_utf8(run.buf.lock().unwrap().snapshot().0).unwrap();
        assert_eq!(
            waited,
            Waited::Exited(Exit {
                code: Some(0),
                signal: None
            }),
            "{text}"
        );
        let here = dir.to_str().unwrap();
        assert_eq!(text.lines().collect::<Vec<_>>(), [here, here], "{text}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_leaves_the_process_running_and_kill_reaps_the_group() {
        let mut run = spawn(
            "sleep 30 & sleep 30",
            &std::env::temp_dir(),
            &[],
            StateMode::None,
            &spill("b.log"),
        )
        .unwrap();
        let cancel = CancellationToken::new();
        assert_eq!(
            wait_exit(&mut run.exit, Duration::from_millis(150), &cancel).await,
            Waited::TimedOut
        );
        assert!(run.exit.borrow().is_none());
        kill_group(run.pid);
        match wait_exit(&mut run.exit, Duration::from_secs(5), &cancel).await {
            Waited::Exited(e) => assert!(e.signal.is_some() || e.code.is_some()),
            other => panic!("expected exit after kill, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn returns_when_the_shell_exits_even_if_a_child_keeps_the_pipe_open() {
        let started = Instant::now();
        let mut run = spawn(
            "sleep 20 & echo started",
            &std::env::temp_dir(),
            &[],
            StateMode::None,
            &spill("c.log"),
        )
        .unwrap();
        let waited = wait_exit(
            &mut run.exit,
            Duration::from_secs(10),
            &CancellationToken::new(),
        )
        .await;
        drain(&mut run.readers).await;
        assert!(matches!(waited, Waited::Exited(Exit { code: Some(0), .. })));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "must not wait for the backgrounded sleep"
        );
        kill_group(run.pid);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_failure_hidden_by_a_pipe_is_recovered_but_sigpipe_is_not_a_failure() {
        let dir = spill("pipes").parent().unwrap().to_path_buf();
        let state = dir.join("shell");
        let cancel = CancellationToken::new();
        let cases = [
            ("sh -c 'echo boom; exit 3' | tail -5", Some(0), Some(3)),
            ("echo fine | tail -5", Some(0), None),
            ("yes | head -1", Some(0), None),
            ("true | sh -c 'exit 4'", Some(4), None),
            ("echo a && sh -c 'exit 2' | cat", Some(0), Some(2)),
            ("exit 5", Some(5), None),
            // No match and "they differ" are answers, not failures; a failed test run next to them still is.
            ("grep -c zzz /dev/null | cat", Some(0), None),
            (
                "echo a > x.txt; echo b | diff x.txt - | head -5",
                Some(0),
                None,
            ),
            (
                "sh -c 'exit 1' | grep -v PASSED | tail -1",
                Some(0),
                Some(1),
            ),
            ("echo 'a|b' | grep -c zzz | cat", Some(0), None),
        ];
        for (i, (cmd, code, masked)) in cases.iter().enumerate() {
            let log = dir.join(format!("p{i}.log"));
            for mode in [StateMode::ReadWrite(&state), StateMode::ReadOnly(&state)] {
                let mut run = spawn(cmd, &dir, &[], mode, &log).unwrap();
                let waited = wait_exit(&mut run.exit, Duration::from_secs(10), &cancel).await;
                assert_eq!(
                    waited,
                    Waited::Exited(Exit {
                        code: *code,
                        signal: None
                    }),
                    "{cmd}"
                );
                assert_eq!(masked_failure(&log, cmd), *masked, "{cmd}");
            }
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn state_persists_cwd_and_exports_across_commands() {
        let dir = spill("state").parent().unwrap().to_path_buf();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let state = dir.join("shell");
        let cancel = CancellationToken::new();

        let mut a = spawn(
            "cd sub && export GREETING='hello world'",
            &dir,
            &[],
            StateMode::ReadWrite(&state),
            &dir.join("1.log"),
        )
        .unwrap();
        wait_exit(&mut a.exit, Duration::from_secs(10), &cancel).await;
        let cwd = std::fs::read_to_string(dir.join("shell.cwd")).unwrap();
        assert!(cwd.trim().ends_with("/sub"));

        // A read-only command sees the exported variable but cannot move the session.
        let mut b = spawn(
            "echo \"$GREETING\"; cd /; export OTHER=1",
            &dir.join("sub"),
            &[],
            StateMode::ReadOnly(&state),
            &dir.join("2.log"),
        )
        .unwrap();
        wait_exit(&mut b.exit, Duration::from_secs(10), &cancel).await;
        drain(&mut b.readers).await;
        assert_eq!(
            String::from_utf8(b.buf.lock().unwrap().snapshot().0)
                .unwrap()
                .trim(),
            "hello world"
        );
        assert!(
            std::fs::read_to_string(dir.join("shell.cwd"))
                .unwrap()
                .trim()
                .ends_with("/sub")
        );
        assert!(
            !std::fs::read_to_string(dir.join("shell.env"))
                .unwrap()
                .contains("OTHER")
        );

        // The exit code of the user's command survives the state-saving trap.
        let mut c = spawn(
            "exit 7",
            &dir,
            &[],
            StateMode::ReadWrite(&state),
            &dir.join("3.log"),
        )
        .unwrap();
        assert_eq!(
            wait_exit(&mut c.exit, Duration::from_secs(10), &cancel).await,
            Waited::Exited(Exit {
                code: Some(7),
                signal: None
            })
        );
    }
}
