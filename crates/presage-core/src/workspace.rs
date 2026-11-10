//! The workspace as the harness sees it: path policy, what the model has already been shown, and a
//! cheap fingerprint that answers "did anything change since the last passing check?".

use crate::util::{fnv64, home_dir};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct Workspace {
    /// Canonical root the session was started in. Writes are confined to it (and the temp dir).
    pub root: PathBuf,
    /// Current directory; follows `cd` across shell calls.
    pub cwd: PathBuf,
    pub allow_outside_writes: bool,
}

/// Lexical normalisation: resolves `.` and `..` without touching the filesystem.
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Canonical form of the deepest existing ancestor, with the not-yet-existing remainder re-attached.
/// This is what makes the write policy hold through symlinks.
fn real_path(p: &Path) -> PathBuf {
    let mut existing = p.to_path_buf();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(canon) = std::fs::canonicalize(&existing) {
            let mut out = canon;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match existing.file_name() {
            Some(name) => {
                rest.push(name.to_os_string());
                existing.pop();
            }
            None => return p.to_path_buf(),
        }
    }
}

impl Workspace {
    pub fn new(root: &Path) -> std::io::Result<Self> {
        let root = std::fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(std::io::Error::new(std::io::ErrorKind::NotADirectory, "workspace root is not a directory"));
        }
        Ok(Self { cwd: root.clone(), root, allow_outside_writes: false })
    }

    /// Absolute, normalised path for something the model named (relative paths are relative to cwd).
    pub fn resolve(&self, p: &str) -> PathBuf {
        let p = p.trim();
        let expanded = if p == "~" {
            home_dir()
        } else if let Some(rest) = p.strip_prefix("~/") {
            home_dir().join(rest)
        } else {
            PathBuf::from(p)
        };
        let abs = if expanded.is_absolute() { expanded } else { self.cwd.join(expanded) };
        normalize(&abs)
    }

    /// Shortest unambiguous way to name a path back to the model.
    pub fn display(&self, abs: &Path) -> String {
        match abs.strip_prefix(&self.cwd) {
            Ok(rel) if rel.as_os_str().is_empty() => ".".to_string(),
            Ok(rel) => rel.to_string_lossy().into_owned(),
            Err(_) => abs.to_string_lossy().into_owned(),
        }
    }

    pub fn contains(&self, abs: &Path) -> bool {
        real_path(abs).starts_with(&self.root)
    }

    pub fn check_writable(&self, abs: &Path) -> Result<(), String> {
        if self.allow_outside_writes {
            return Ok(());
        }
        let real = real_path(abs);
        let tmp = std::fs::canonicalize(std::env::temp_dir()).unwrap_or_else(|_| std::env::temp_dir());
        if real.starts_with(&self.root) || real.starts_with(&tmp) {
            Ok(())
        } else {
            Err(format!(
                "refusing to write outside the workspace: {} is not under {}",
                real.display(),
                self.root.display()
            ))
        }
    }
}

/// Tracks which file windows are already in the model's context, so an unchanged window is not paid
/// for twice. The first repeat gets a stub; if the model asks again it evidently needs the text.
#[derive(Debug, Default)]
pub struct Seen {
    files: HashMap<PathBuf, SeenFile>,
}

#[derive(Debug)]
struct SeenFile {
    hash: u64,
    windows: Vec<SeenWindow>,
}

#[derive(Debug)]
struct SeenWindow {
    from: usize,
    to: usize,
    step: u32,
    stubbed: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SeenCheck {
    Fresh,
    AlreadyShown { step: u32 },
}

impl Seen {
    pub fn observe(&mut self, path: &Path, hash: u64, from: usize, to: usize, step: u32) -> SeenCheck {
        let file = self.files.entry(path.to_path_buf()).or_insert(SeenFile { hash, windows: Vec::new() });
        if file.hash != hash {
            *file = SeenFile { hash, windows: Vec::new() };
        }
        if let Some(w) = file.windows.iter_mut().find(|w| w.from <= from && w.to >= to) {
            if !w.stubbed {
                w.stubbed = true;
                return SeenCheck::AlreadyShown { step: w.step };
            }
            w.stubbed = false;
            w.step = step;
            return SeenCheck::Fresh;
        }
        file.windows.push(SeenWindow { from, to, step, stubbed: false });
        SeenCheck::Fresh
    }

    /// Records a window without asking, for text the harness attaches on its own initiative.
    pub fn mark(&mut self, path: &Path, hash: u64, from: usize, to: usize, step: u32) {
        let file = self.files.entry(path.to_path_buf()).or_insert(SeenFile { hash, windows: Vec::new() });
        if file.hash != hash {
            *file = SeenFile { hash, windows: Vec::new() };
        }
        if !file.windows.iter().any(|w| w.from <= from && w.to >= to) {
            file.windows.push(SeenWindow { from, to, step, stubbed: false });
        }
    }

    pub fn covered(&self, path: &Path, hash: u64, from: usize, to: usize) -> bool {
        self.files.get(path).is_some_and(|f| f.hash == hash && f.windows.iter().any(|w| w.from <= from && w.to >= to))
    }

    /// Forget everything: called when old observations are dropped from the context.
    pub fn clear(&mut self) {
        self.files.clear();
    }
}

/// Directories that are large, derived, and never the deliverable.
const SKIP_DIRS: &[&str] = &[
    ".git", "node_modules", "target", ".venv", "venv", "__pycache__", ".mypy_cache", ".pytest_cache", ".tox",
    ".next", ".cache", ".gradle", ".idea", ".ruff_cache",
];

const FINGERPRINT_MAX_FILES: u32 = 20_000;

fn mix(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// Order-independent digest of (path, size, mtime) for every file under `root`.
///
/// It is deliberately metadata-only: the question is "was anything touched", and it has to be cheap
/// enough to ask after every shell command. Very large trees are sampled up to a cap and a time budget.
pub fn fingerprint(root: &Path, budget: Duration) -> u64 {
    let start = Instant::now();
    let mut acc: u64 = 0;
    let mut count: u32 = 0;
    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .follow_links(false)
        .filter_entry(|e| !e.file_name().to_str().is_some_and(|n| SKIP_DIRS.contains(&n)))
        .build();
    for entry in walker.flatten() {
        let Ok(md) = entry.metadata() else { continue };
        if md.is_dir() {
            continue;
        }
        let mtime = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        let h = fnv64(entry.path().as_os_str().as_encoded_bytes()) ^ mix(md.len()) ^ mix(mtime).rotate_left(21);
        acc = acc.wrapping_add(mix(h));
        count += 1;
        if count >= FINGERPRINT_MAX_FILES || (count % 256 == 0 && start.elapsed() > budget) {
            break;
        }
    }
    acc ^ (count as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("presage-ws-{name}-{}", crate::util::rand_hex(4)));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn resolves_relative_to_cwd_and_normalises() {
        let root = tmp("resolve");
        fs::create_dir_all(root.join("src/deep")).unwrap();
        let mut ws = Workspace::new(&root).unwrap();
        assert_eq!(ws.resolve("src/../src/./a.rs"), ws.root.join("src/a.rs"));
        ws.cwd = ws.root.join("src");
        assert_eq!(ws.resolve("deep/x.rs"), ws.root.join("src/deep/x.rs"));
        assert_eq!(ws.display(&ws.root.join("src/deep/x.rs")), "deep/x.rs");
        assert_eq!(ws.display(&ws.root.join("src")), ".");
        assert!(ws.display(&ws.root.join("other.rs")).starts_with('/'));
    }

    #[test]
    fn write_policy_confines_to_root_and_tmp_even_through_symlinks() {
        let root = tmp("policy");
        let outside = tmp("policy-outside");
        let ws = Workspace::new(&root).unwrap();
        assert!(ws.check_writable(&ws.resolve("new/dir/file.txt")).is_ok());
        assert!(ws.check_writable(&ws.resolve("../../etc/passwd")).is_err() || ws.resolve("../../etc/passwd").starts_with(std::env::temp_dir()));
        assert!(ws.check_writable(Path::new("/etc/presage-should-not-write")).is_err());
        #[cfg(unix)]
        {
            // The policy must hold for a symlink that points out of the workspace into a non-temp place.
            std::os::unix::fs::symlink("/etc", root.join("escape")).unwrap();
            assert!(ws.check_writable(&ws.resolve("escape/hosts")).is_err());
        }
        let _ = outside;
    }

    #[test]
    fn seen_stubs_once_then_reprints() {
        let mut seen = Seen::default();
        let p = Path::new("/w/a.rs");
        assert_eq!(seen.observe(p, 1, 1, 100, 3), SeenCheck::Fresh);
        assert_eq!(seen.observe(p, 1, 10, 50, 5), SeenCheck::AlreadyShown { step: 3 });
        assert_eq!(seen.observe(p, 1, 10, 50, 6), SeenCheck::Fresh, "asking twice means it is needed");
        assert_eq!(seen.observe(p, 1, 90, 140, 7), SeenCheck::Fresh, "not fully covered");
        assert_eq!(seen.observe(p, 2, 1, 100, 8), SeenCheck::Fresh, "content changed");
        assert!(seen.covered(p, 2, 5, 20));
        seen.clear();
        assert!(!seen.covered(p, 2, 5, 20));
    }

    #[test]
    fn fingerprint_changes_on_edit_create_delete_but_not_on_ignored_dirs() {
        let root = tmp("fp");
        fs::write(root.join("a.txt"), "one").unwrap();
        fs::create_dir_all(root.join("node_modules/x")).unwrap();
        let budget = Duration::from_secs(2);
        let f0 = fingerprint(&root, budget);
        assert_eq!(f0, fingerprint(&root, budget), "stable when nothing changed");

        fs::write(root.join("node_modules/x/big.js"), "ignored").unwrap();
        assert_eq!(f0, fingerprint(&root, budget));

        fs::write(root.join("a.txt"), "three").unwrap();
        let f1 = fingerprint(&root, budget);
        assert_ne!(f0, f1);

        fs::write(root.join("b.txt"), "").unwrap();
        let f2 = fingerprint(&root, budget);
        assert_ne!(f1, f2);

        fs::remove_file(root.join("b.txt")).unwrap();
        assert_eq!(f1, fingerprint(&root, budget));
    }
}
