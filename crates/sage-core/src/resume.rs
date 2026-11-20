//! What a session keeps on disk so that it can be continued after the program has been restarted,
//! and the lock that keeps two processes from running the same session.
//!
//! Each checkpoint publishes `snapshot.json` atomically after appending and syncing its new
//! conversation records. The snapshot names a committed prefix of the journal and stores the
//! counters, policies, and standing checks. A rewrite of old messages starts a new journal.
//! Older checkpoints embed the conversation directly. The goal is in `meta.json`.

use crate::events::Stats;
use crate::provider::Message;
use crate::state::SessionState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::Path;

pub const SNAPSHOT: &str = "snapshot.json";
pub const LOCK: &str = "session.lock";

/// The version of the snapshot format this build writes and reads.
pub const VERSION: u32 = 2;

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
    #[serde(default)]
    pub max_requests: Option<u32>,
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

#[derive(Serialize, Deserialize)]
struct StoredSnapshot {
    #[serde(flatten)]
    state: Snapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    journal: Option<Journal>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Journal {
    generation: String,
    pub entries: usize,
    #[serde(skip)]
    pub revision: u64,
}

impl Journal {
    fn name(&self) -> String {
        format!("history-{}.jsonl", self.generation)
    }
}

pub(crate) fn write_checkpoint(
    path: &Path,
    state: Snapshot,
    previous: Option<Journal>,
    revision: u64,
    messages: Vec<Message>,
    stubs: Vec<Option<String>>,
) -> std::io::Result<Journal> {
    let new_generation = previous.is_none();
    let mut journal = previous.unwrap_or_else(|| Journal {
        generation: crate::util::rand_hex(12),
        entries: 0,
        revision,
    });
    if new_generation || !messages.is_empty() {
        let mut options = OpenOptions::new();
        options.append(true).create_new(new_generation);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = BufWriter::new(options.open(path.with_file_name(journal.name()))?);
        for entry in messages.iter().zip(&stubs) {
            serde_json::to_writer(&mut file, &entry)?;
            writeln!(file)?;
        }
        file.flush()?;
        file.get_ref().sync_all()?;
        if new_generation {
            sync_parent(path)?;
        }
        journal.entries += messages.len();
    }
    let mut stored = StoredSnapshot {
        state,
        journal: Some(journal),
    };
    write_atomic(path, &serde_json::to_vec(&stored)?)?;
    let journal = stored.journal.take().unwrap();
    if new_generation && let Some(dir) = path.parent() {
        sync_parent(path)?;
        let name = journal.name();
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            if entry.file_name().to_str().is_some_and(|file| {
                file.strip_prefix("history-")
                    .and_then(|s| s.strip_suffix(".jsonl"))
                    .is_some_and(|id| id.len() == 24 && id.bytes().all(|b| b.is_ascii_hexdigit()))
                    && file != name
            }) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    Ok(journal)
}

fn sync_parent(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Writes `bytes` to `path` so that a reader sees the old file or the new one, never half of one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension(format!("{}.tmp", crate::util::rand_hex(8)));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    let result = written.and_then(|()| std::fs::rename(&temporary, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// What the snapshot in `dir` is, or why there is none to be had.
pub fn read(dir: &Path) -> Result<Snapshot, String> {
    let path = dir.join(SNAPSHOT);
    let text = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "it has no snapshot: it was stored before sessions could be continued".to_string()
        } else {
            format!("its snapshot cannot be read: {e}")
        }
    })?;
    let StoredSnapshot {
        state: mut snapshot,
        journal,
    } = serde_json::from_str(&text).map_err(|e| format!("its snapshot cannot be read: {e}"))?;
    if snapshot.version > VERSION {
        return Err(format!(
            "its snapshot is version {}, from a newer Sage than this one ({VERSION})",
            snapshot.version
        ));
    }
    if let Some(journal) = journal {
        if journal.generation.len() != 24
            || !journal.generation.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err("its snapshot names an invalid history journal".into());
        }
        let file = File::open(dir.join(journal.name()))
            .map_err(|e| format!("its history cannot be read: {e}"))?;
        for line in BufReader::new(file).lines().take(journal.entries) {
            let line = line.map_err(|e| format!("its history cannot be read: {e}"))?;
            let (message, stub) = serde_json::from_str(&line)
                .map_err(|e| format!("its history cannot be read: {e}"))?;
            snapshot.messages.push(message);
            snapshot.stubs.push(stub);
        }
        if snapshot.messages.len() != journal.entries {
            return Err("its history ends before the saved checkpoint".into());
        }
    }
    Ok(snapshot)
}

/// The claim of one process on a session. Held for as long as it is kept, and let go of by the
/// operating system when the process ends however it ends, so a lock left behind by one that was
/// killed is never in the way.
#[derive(Debug)]
pub struct SessionLock {
    file: File,
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        // Let go of the lock itself rather than leave that to closing the file: the lock lasts
        // until the last copy of the file is closed, and a command being started in that moment
        // holds one (between fork and exec), so a session just closed could still read as open
        // in another process and be refused to whoever continues it.
        let _ = self.file.unlock();
    }
}

impl SessionLock {
    /// Takes the lock on the session in `dir`, or says it is already open, with the holder's pid
    /// when the operating system permits reading the locked file.
    pub fn acquire(dir: &Path) -> Result<Self, String> {
        let path = dir.join(LOCK);
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        // The lock of a holder that ended without letting go is freed when the last copy of its
        // file is closed, and a command being started at that moment (between fork and exec)
        // holds one for an instant: wait for it.
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
                Ok(Self { file })
            }
            Err(TryLockError::WouldBlock) => {
                let mut pid = String::new();
                // Windows locks also prevent reading the pid through another handle, so the
                // holder's identity is best-effort; the native lock alone decides ownership.
                let _ = File::open(&path).and_then(|mut f| f.read_to_string(&mut pid));
                let who = pid.trim().parse::<u32>().map_or_else(
                    |_| "another process".to_string(),
                    |pid| format!("another process (PID {pid})"),
                );
                Err(format!(
                    "it is open in {who}: only one process can run a session at a time"
                ))
            }
            Err(TryLockError::Error(error)) => {
                Err(format!("cannot lock {}: {error}", path.display()))
            }
        }
    }

    /// Whether another holder has the session in `dir` right now.
    ///
    /// A holder lets go when it is dropped. One that ended without that (a process killed) is let
    /// go of only when the last copy of its file is closed, and as in `acquire`, a command being
    /// started at that moment (between fork and exec) holds a copy for an instant: a lock still
    /// there a moment later is held.
    pub fn held(dir: &Path) -> bool {
        let Ok(file) = File::open(dir.join(LOCK)) else {
            return false;
        };
        for _ in 0..5 {
            match file.try_lock_shared() {
                Ok(()) => {
                    // Not left to the close either, for the same reason as `drop`.
                    let _ = file.unlock();
                    return false;
                }
                Err(TryLockError::WouldBlock) => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(TryLockError::Error(_)) => return false,
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sage-resume-{name}-{}", crate::util::rand_hex(4)));
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
        let pid = std::process::id().to_string();
        // The pid is readable under advisory locks, but Windows denies reads through another
        // handle while the native lock is held. Either way the second holder must be refused.
        let who = match std::fs::read_to_string(dir.join(LOCK)) {
            Ok(stored_pid) => {
                assert_eq!(stored_pid, pid);
                format!("another process (PID {pid})")
            }
            Err(_) => "another process".to_string(),
        };
        let refused = SessionLock::acquire(&dir).unwrap_err();
        assert_eq!(
            refused,
            format!("it is open in {who}: only one process can run a session at a time")
        );
        assert!(
            SessionLock::held(&dir),
            "a refused acquisition must not release the first holder"
        );
        drop(first);
        assert!(!SessionLock::held(&dir));
        assert_eq!(
            std::fs::read_to_string(dir.join(LOCK)).unwrap(),
            pid,
            "the pid remains as metadata after the lock is released"
        );
        let second =
            SessionLock::acquire(&dir).expect("the lock is available after its holder is dropped");
        assert!(SessionLock::held(&dir));
        drop(second);
        assert!(!SessionLock::held(&dir));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_dropped_lock_is_free_at_once_while_a_copy_of_its_file_is_still_open() {
        // A command being started holds a copy of every open file for an instant (between fork
        // and exec): the session is free when its holder lets go, not when that copy is closed.
        let dir = scratch("lock-copy");
        let lock = SessionLock::acquire(&dir).unwrap();
        let copy = lock.file.try_clone().unwrap();
        drop(lock);
        assert!(!SessionLock::held(&dir));
        let again = SessionLock::acquire(&dir).expect("the lock is free while the copy is open");
        drop(again);
        drop(copy);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_snapshot_is_written_whole_and_read_back_and_a_missing_or_newer_one_says_so() {
        let dir = scratch("snapshot");
        assert!(read(&dir).unwrap_err().contains("no snapshot"));
        write_atomic(&dir.join(SNAPSHOT), b"{ not json").unwrap();
        assert!(read(&dir).unwrap_err().contains("cannot be read"));
        assert!(
            !dir.join("snapshot.json.tmp").exists(),
            "the temporary file is renamed away"
        );
        let mut newer = serde_json::json!({"version": VERSION + 1});
        newer["x"] = 1.into();
        write_atomic(&dir.join(SNAPSHOT), newer.to_string().as_bytes()).unwrap();
        assert!(
            read(&dir).unwrap_err().contains("cannot be read"),
            "a newer format that this build cannot type is refused, not guessed at"
        );
    }
}
