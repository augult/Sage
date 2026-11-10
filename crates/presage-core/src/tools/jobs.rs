//! Process plumbing: spawning commands in their own process group, capturing output without unbounded
//! memory, and keeping long-running commands alive as jobs instead of killing them at a timeout.

use std::collections::VecDeque;
use std::io::Write;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

const HEAD_CAP: usize = 48 * 1024;
const TAIL_CAP: usize = 256 * 1024;

/// Captured output: the first and last parts stay in memory, everything goes to the spill file.
pub struct OutBuf {
    head: Vec<u8>,
    tail: VecDeque<u8>,
    dropped: bool,
    pub total: u64,
    file: Option<std::fs::File>,
}

impl OutBuf {
    pub fn new(spill: &Path) -> Self {
        Self { head: Vec::new(), tail: VecDeque::new(), dropped: false, total: 0, file: std::fs::File::create(spill).ok() }
    }

    pub fn push(&mut self, mut data: &[u8]) {
        if let Some(f) = &mut self.file {
            let _ = f.write_all(data);
        }
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
            let start = joined.iter().position(|c| *c == b'\n').map(|p| p + 1).unwrap_or(0);
            out.extend_from_slice(&joined[start..]);
        } else {
            out.extend_from_slice(a);
            out.extend_from_slice(b);
        }
        (out, self.dropped)
    }

    /// Bytes produced after `offset`, as far as they are still in memory.
    pub fn since(&self, offset: u64) -> Vec<u8> {
        let retained = (self.head.len() + self.tail.len()) as u64;
        let all: Vec<u8> = self.head.iter().chain(self.tail.iter()).copied().collect();
        let wanted = self.total.saturating_sub(offset).min(retained) as usize;
        if !self.dropped {
            return all[all.len() - wanted..].to_vec();
        }
        let tail_len = self.tail.len().min(wanted);
        all[all.len() - tail_len..].to_vec()
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
#[cfg(unix)]
const RESTORE: &str = r#"__ps="$PRESAGE_STATE"; unset PRESAGE_STATE
[ -f "$__ps.env" ] && . "$__ps.env" >/dev/null 2>&1
"#;
#[cfg(unix)]
const SAVE: &str = r#"trap '__rc=$?; pwd > "$__ps.cwd" 2>/dev/null; export -p 2>/dev/null | grep -vE "^declare -x (PWD|OLDPWD|SHLVL|_)=" > "$__ps.env" 2>/dev/null; exit $__rc' EXIT
"#;

#[cfg(unix)]
fn shell_command(cmd: &str, state: StateMode<'_>) -> Command {
    let mut c = Command::new("bash");
    let script = match state {
        StateMode::ReadWrite(p) => {
            c.env("PRESAGE_STATE", p);
            format!("{RESTORE}{SAVE}{cmd}")
        }
        StateMode::ReadOnly(p) => {
            c.env("PRESAGE_STATE", p);
            format!("{RESTORE}{cmd}")
        }
        StateMode::None => cmd.to_string(),
    };
    c.arg("-c").arg(script);
    c.process_group(0);
    c
}

#[cfg(not(unix))]
fn shell_command(cmd: &str, _state: StateMode<'_>) -> Command {
    let mut c = Command::new("cmd");
    c.arg("/D").arg("/S").arg("/C").arg(cmd);
    c
}

async fn pump<R: AsyncRead + Unpin>(mut reader: R, buf: Arc<Mutex<OutBuf>>) {
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.lock().unwrap().push(&chunk[..n]),
        }
    }
}

pub fn spawn(cmd: &str, cwd: &Path, envs: &[(String, String)], state: StateMode<'_>, spill: &Path) -> std::io::Result<Running> {
    let mut c = shell_command(cmd, state);
    c.current_dir(cwd)
        .envs(envs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false);
    let mut child = c.spawn()?;
    let pid = child.id();
    let buf = Arc::new(Mutex::new(OutBuf::new(spill)));
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        readers.push(tokio::spawn(pump(out, buf.clone())));
    }
    if let Some(err) = child.stderr.take() {
        readers.push(tokio::spawn(pump(err, buf.clone())));
    }
    let (tx, rx) = watch::channel(None);
    tokio::spawn(async move {
        let exit = match child.wait().await {
            Ok(status) => {
                #[cfg(unix)]
                let signal = std::os::unix::process::ExitStatusExt::signal(&status);
                #[cfg(not(unix))]
                let signal = None;
                Exit { code: status.code(), signal }
            }
            Err(_) => Exit { code: None, signal: None },
        };
        let _ = tx.send(Some(exit));
    });
    Ok(Running { pid, buf, exit: rx, readers, started: Instant::now() })
}

#[derive(Debug, PartialEq, Eq)]
pub enum Waited {
    Exited(Exit),
    TimedOut,
    Cancelled,
}

pub async fn wait_exit(rx: &mut watch::Receiver<Option<Exit>>, timeout: Duration, cancel: &CancellationToken) -> Waited {
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
pub async fn drain(readers: &mut Vec<JoinHandle<()>>) {
    let pending: Vec<JoinHandle<()>> = std::mem::take(readers);
    let _ = tokio::time::timeout(Duration::from_millis(250), futures_util::future::join_all(pending)).await;
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
        let _ = std::process::Command::new("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"]).output();
    }
}

pub struct Job {
    pub id: String,
    pub cmd: String,
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
    pub fn add(&mut self, cmd: &str, spill: &str, run: Running, read_offset: u64) -> String {
        self.next += 1;
        let id = format!("bg:{}", self.next);
        self.list.push(Job { id: id.clone(), cmd: cmd.to_string(), spill: spill.to_string(), run, read_offset });
        id
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Job> {
        let wanted = id.trim().trim_start_matches("bg:");
        self.list.iter_mut().find(|j| j.id.trim_start_matches("bg:") == wanted)
    }

    pub fn running(&self) -> Vec<&Job> {
        self.list.iter().filter(|j| j.run.exit.borrow().is_none()).collect()
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
        let d = std::env::temp_dir().join(format!("presage-jobs-{}", crate::util::rand_hex(4)));
        std::fs::create_dir_all(&d).unwrap();
        d.join(name)
    }

    #[test]
    fn outbuf_keeps_head_and_tail_and_spills_everything() {
        let path = spill("big.log");
        let mut b = OutBuf::new(&path);
        let line = format!("{}\n", "x".repeat(99));
        for _ in 0..5_000 {
            b.push(line.as_bytes());
        }
        let (snap, dropped) = b.snapshot();
        assert!(dropped);
        assert!(snap.len() <= HEAD_CAP + TAIL_CAP + 1);
        assert_eq!(b.total, 500_000);
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 500_000, "nothing is lost on disk");
        assert!(snap.ends_with(line.as_bytes()));
        assert_eq!(b.since(b.total - 200), [line.as_bytes(), line.as_bytes()].concat());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn captures_both_streams_and_exit_code() {
        let mut run = spawn("echo out; echo err >&2; exit 3", &std::env::temp_dir(), &[], StateMode::None, &spill("a.log")).unwrap();
        let waited = wait_exit(&mut run.exit, Duration::from_secs(10), &CancellationToken::new()).await;
        drain(&mut run.readers).await;
        assert_eq!(waited, Waited::Exited(Exit { code: Some(3), signal: None }));
        let text = String::from_utf8(run.buf.lock().unwrap().snapshot().0).unwrap();
        assert!(text.contains("out") && text.contains("err"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_leaves_the_process_running_and_kill_reaps_the_group() {
        let mut run = spawn("sleep 30 & sleep 30", &std::env::temp_dir(), &[], StateMode::None, &spill("b.log")).unwrap();
        let cancel = CancellationToken::new();
        assert_eq!(wait_exit(&mut run.exit, Duration::from_millis(150), &cancel).await, Waited::TimedOut);
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
        let mut run = spawn("sleep 20 & echo started", &std::env::temp_dir(), &[], StateMode::None, &spill("c.log")).unwrap();
        let waited = wait_exit(&mut run.exit, Duration::from_secs(10), &CancellationToken::new()).await;
        drain(&mut run.readers).await;
        assert!(matches!(waited, Waited::Exited(Exit { code: Some(0), .. })));
        assert!(started.elapsed() < Duration::from_secs(5), "must not wait for the backgrounded sleep");
        kill_group(run.pid);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn state_persists_cwd_and_exports_across_commands() {
        let dir = spill("state").parent().unwrap().to_path_buf();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let state = dir.join("shell");
        let cancel = CancellationToken::new();

        let mut a = spawn("cd sub && export GREETING='hello world'", &dir, &[], StateMode::ReadWrite(&state), &dir.join("1.log")).unwrap();
        wait_exit(&mut a.exit, Duration::from_secs(10), &cancel).await;
        let cwd = std::fs::read_to_string(dir.join("shell.cwd")).unwrap();
        assert!(cwd.trim().ends_with("/sub"));

        // A read-only command sees the exported variable but cannot move the session.
        let mut b = spawn("echo \"$GREETING\"; cd /; export OTHER=1", &dir.join("sub"), &[], StateMode::ReadOnly(&state), &dir.join("2.log")).unwrap();
        wait_exit(&mut b.exit, Duration::from_secs(10), &cancel).await;
        drain(&mut b.readers).await;
        assert_eq!(String::from_utf8(b.buf.lock().unwrap().snapshot().0).unwrap().trim(), "hello world");
        assert!(std::fs::read_to_string(dir.join("shell.cwd")).unwrap().trim().ends_with("/sub"));
        assert!(!std::fs::read_to_string(dir.join("shell.env")).unwrap().contains("OTHER"));

        // The exit code of the user's command survives the state-saving trap.
        let mut c = spawn("exit 7", &dir, &[], StateMode::ReadWrite(&state), &dir.join("3.log")).unwrap();
        assert_eq!(wait_exit(&mut c.exit, Duration::from_secs(10), &cancel).await, Waited::Exited(Exit { code: Some(7), signal: None }));
    }
}
