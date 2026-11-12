//! What a session keeps on disk so that it can be continued after the program has been restarted,
//! and the lock that keeps two processes from running the same session.
//!
//! `snapshot.json` sits beside `meta.json` and `events.jsonl`. It is written whole (a temporary
//! file, then a rename) after every model reply and when a run ends, so that a crash or a kill
//! loses at most the turn in flight. It holds what continuing needs and nothing the workspace or
//! the log can say: the conversation as sent, and the counters, policies and standing checks the
//! agent would otherwise start empty with. The goal is in `meta.json`.

use crate::events::Stats;
use crate::provider::Message;
use crate::state::SessionState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Write};
use std::path::Path;

pub const SNAPSHOT: &str = "snapshot.json";
pub const LOCK: &str = "session.lock";

/// The version of the snapshot format this build writes and reads.
pub const VERSION: u32 = 1;

/// How a session was set up, as far as `Features` cannot say: the mechanism lists it started with.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Setup {
    /// The mode, by its name (`Mode::as_str`).
    pub mode: String,
    #[serde(default)]
    pub with: Vec<String>,
    #[serde(default)]
    pub without: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    /// Unix milliseconds.
    pub saved: u64,
    /// A run was in progress when this was written: if it is the last one, that run was cut off.
    pub in_run: bool,
    pub setup: Setup,
    pub model: String,
    /// `auto` or `ask`.
    pub autonomy: String,
    pub interactive: bool,
    pub max_turns: u32,
    pub time_budget_secs: Option<u64>,
    /// The effort policy as `EffortPolicy::parse` reads it, and the level it has reached.
    pub effort_policy: String,
    pub effort: String,
    /// The conversation exactly as sent, the system prompt first, and for each message the stub a
    /// tool result shrinks to if it is decayed.
    pub messages: Vec<Message>,
    pub stubs: Vec<Option<String>>,
    pub turn: u32,
    pub stats: Stats,
    /// How often each file has been edited: the note about the sixth edit counts the session's.
    #[serde(default)]
    pub edits: BTreeMap<String, u32>,
    pub chain_hints: u8,
    pub solo_hints: u8,
    pub session: SessionState,
}

/// Writes `bytes` to `path` so that a reader sees the old file or the new one, never half of one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension("json.tmp");
    let mut file = File::create(&temporary)?;
    file.write_all(bytes)?;
    // The rename must not outlive the data it points at if the machine goes down.
    file.sync_data()?;
    drop(file);
    std::fs::rename(&temporary, path)
}

/// What the snapshot in `dir` is, or why there is none to be had.
pub fn read(dir: &Path) -> Result<Snapshot, String> {
    let path = dir.join(SNAPSHOT);
    let text = std::fs::read_to_string(&path).map_err(|e| if e.kind() == std::io::ErrorKind::NotFound { "it has no snapshot: it was stored before sessions could be continued".to_string() } else { format!("its snapshot cannot be read: {e}") })?;
    let snapshot: Snapshot = serde_json::from_str(&text).map_err(|e| format!("its snapshot cannot be read: {e}"))?;
    if snapshot.version > VERSION {
        return Err(format!("its snapshot is version {}, from a newer Sage than this one ({VERSION})", snapshot.version));
    }
    Ok(snapshot)
}

/// The claim of one process on a session. Held for as long as it is kept, and let go of by the
/// operating system when the process ends however it ends, so a lock left behind by one that was
/// killed is never in the way.
#[derive(Debug)]
pub struct SessionLock {
    _file: File,
}

impl SessionLock {
    /// Takes the lock on the session in `dir`, or says which process has it.
    pub fn acquire(dir: &Path) -> Result<Self, String> {
        let path = dir.join(LOCK);
        let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        // A lock is let go of when the last copy of its file is closed, and a command being started
        // at that moment (between fork and exec) holds one for an instant: wait for it.
        let mut waited = 0;
        let locked = loop {
            match file.try_lock() {
                Err(TryLockError::WouldBlock) if waited < 20 => {
                    waited += 1;
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                other => break other,
            }
        };
        match locked {
            Ok(()) => {
                // The pid is only for the message of whoever is turned away.
                let _ = file.set_len(0);
                let _ = write!(file, "{}", std::process::id());
                Ok(Self { _file: file })
            }
            Err(TryLockError::WouldBlock) => {
                let mut pid = String::new();
                let _ = File::open(&path).and_then(|mut f| f.read_to_string(&mut pid));
                let who = pid.trim().parse::<u32>().map_or_else(|_| "another process".to_string(), |pid| format!("process {pid}"));
                Err(format!("it is open in {who}: only one process can run a session at a time"))
            }
            // A file system that cannot lock: the session is used without the protection.
            Err(TryLockError::Error(_)) => Ok(Self { _file: file }),
        }
    }

    /// Whether another holder has the session in `dir` right now.
    pub fn held(dir: &Path) -> bool {
        let Ok(file) = File::open(dir.join(LOCK)) else { return false };
        matches!(file.try_lock_shared(), Err(TryLockError::WouldBlock))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sage-resume-{name}-{}", crate::util::rand_hex(4)));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_lock_keeps_a_second_holder_out_until_it_is_dropped_and_a_stale_file_does_not() {
        let dir = scratch("lock");
        // A file left behind by a process that is gone locks nothing.
        std::fs::write(dir.join(LOCK), "999999").unwrap();
        assert!(!SessionLock::held(&dir));
        let first = SessionLock::acquire(&dir).expect("a stale lock file is ignored");
        assert!(SessionLock::held(&dir));
        let refused = SessionLock::acquire(&dir).unwrap_err();
        assert!(refused.contains(&format!("process {}", std::process::id())), "{refused}");
        drop(first);
        assert!(!SessionLock::held(&dir));
        assert!(SessionLock::acquire(&dir).is_ok());
    }

    #[test]
    fn a_snapshot_is_written_whole_and_read_back_and_a_missing_or_newer_one_says_so() {
        let dir = scratch("snapshot");
        assert!(read(&dir).unwrap_err().contains("no snapshot"));
        write_atomic(&dir.join(SNAPSHOT), b"{ not json").unwrap();
        assert!(read(&dir).unwrap_err().contains("cannot be read"));
        assert!(!dir.join("snapshot.json.tmp").exists(), "the temporary file is renamed away");
        let mut newer = serde_json::json!({"version": VERSION + 1});
        newer["x"] = 1.into();
        write_atomic(&dir.join(SNAPSHOT), newer.to_string().as_bytes()).unwrap();
        assert!(read(&dir).unwrap_err().contains("cannot be read"), "a newer format that this build cannot type is refused, not guessed at");
    }
}
