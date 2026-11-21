//! The workspace as the harness sees it: path policy, what the model has already been shown, and a
//! cheap fingerprint that answers "did anything change since the last passing check?".

use crate::util::{fnv64, home_dir};
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct Workspace {
    /// Canonical root the session was started in. Writes are confined to it (and the temp dir).
    pub root: PathBuf,
    /// The root as the user reached it (through a symlink, say), when that is not its canonical
    /// form. Commands may print either.
    pub as_given: Option<PathBuf>,
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
        if let Ok(canon) = crate::util::canonical(&existing) {
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

/// The directory the user's shell believes it is in, which keeps the symlinks they came through.
fn logical_cwd() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let same = |p: &PathBuf| {
        p.is_absolute() && crate::util::canonical(p).ok() == crate::util::canonical(&cwd).ok()
    };
    Some(
        std::env::var_os("PWD")
            .map(PathBuf::from)
            .filter(same)
            .unwrap_or(cwd),
    )
}

/// `given` as an absolute path with its symlinks left in, if that spells `canonical` differently.
fn as_given(given: &Path, canonical: &Path) -> Option<PathBuf> {
    let spelled = if given.is_absolute() {
        normalize(given)
    } else {
        normalize(&logical_cwd()?.join(given))
    };
    (spelled != canonical && crate::util::canonical(&spelled).is_ok_and(|real| real == canonical))
        .then_some(spelled)
}

/// What can sit inside a file name: next to one of these, a path goes on.
fn in_name(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '+' | '~' | '@' | '%' | '#')
}

/// True where a path written in running text is over.
fn ends_path(next: Option<char>) -> bool {
    next.is_none_or(|c| {
        c.is_whitespace()
            || matches!(
                c,
                '"' | '\''
                    | '`'
                    | ')'
                    | ']'
                    | '}'
                    | '>'
                    | '<'
                    | ','
                    | ';'
                    | ':'
                    | '|'
                    | '&'
                    | '’'
                    | '”'
            )
    })
}

/// `text` without `dir` wherever a path starts with it: `dir/x` becomes `x`, `dir` on its own `.`
/// and `dir/` on its own `./`. `None` when nothing does. A path that only shares the prefix
/// (`dir-old/x`, `/mnt/dir/x`) is another place and is left as it is.
fn cut_dir(text: &str, dir: &str, windows_paths: bool) -> Option<String> {
    // Match either Windows separator, including mixed spellings from tools. Substitution is only
    // for matching: it preserves byte offsets, and the output still copies the original text.
    let search_text = if windows_paths {
        Cow::Owned(text.replace('\\', "/"))
    } else {
        Cow::Borrowed(text)
    };
    let search_dir = if windows_paths {
        Cow::Owned(dir.replace('\\', "/"))
    } else {
        Cow::Borrowed(dir)
    };
    let step = search_dir.chars().next()?.len_utf8();
    let mut out: Option<String> = None;
    let (mut copied, mut from) = (0, 0);
    while let Some(at) = search_text[from..]
        .find(search_dir.as_ref())
        .map(|i| from + i)
    {
        from = at + step;
        let rest = &text[at + dir.len()..];
        let below = rest.trim_start_matches(|c| c == '/' || (windows_paths && c == '\\'));
        let slashes = rest.len() - below.len();
        // Glued to what stands before it, the match is the tail of a longer path.
        let glued = text[..at]
            .chars()
            .next_back()
            .is_some_and(|c| in_name(c) || matches!(c, '/' | '\\' | '$' | '}' | ')'));
        let put = match (slashes > 0, ends_path(below.chars().next())) {
            _ if glued => continue,
            (true, false) => "",
            (true, true) => "./",
            (false, true) => ".",
            (false, false) => continue,
        };
        let short = out.get_or_insert_with(|| String::with_capacity(text.len()));
        short.push_str(&text[copied..at]);
        short.push_str(put);
        copied = at + dir.len() + slashes;
        from = copied;
    }
    out.map(|mut short| {
        short.push_str(&text[copied..]);
        short
    })
}

impl Workspace {
    pub fn new(root: &Path) -> std::io::Result<Self> {
        let given = root;
        let root = crate::util::canonical(given)?;
        if !root.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotADirectory,
                "workspace root is not a directory",
            ));
        }
        Ok(Self {
            cwd: root.clone(),
            as_given: as_given(given, &root),
            root,
            allow_outside_writes: false,
        })
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
        let abs = if expanded.is_absolute() {
            expanded
        } else {
            self.cwd.join(expanded)
        };
        normalize(&abs)
    }

    /// Shortest unambiguous way to name a path back to the model, using `/` separators on Windows.
    pub fn display(&self, abs: &Path) -> String {
        let path = match abs.strip_prefix(&self.cwd) {
            Ok(rel) if rel.as_os_str().is_empty() => return ".".to_string(),
            Ok(rel) => rel,
            Err(_) => abs,
        };
        let text = path.to_string_lossy();
        if cfg!(windows) {
            text.replace('\\', "/")
        } else {
            text.into_owned()
        }
    }

    /// Every absolute spelling of the directory the tools run in. None once a `cd` has left the
    /// workspace: paths out there are named in full.
    fn cwd_spellings(&self) -> Vec<String> {
        let Ok(below) = self.cwd.strip_prefix(&self.root) else {
            return Vec::new();
        };
        let at = |root: &PathBuf| {
            if below.as_os_str().is_empty() {
                root.clone()
            } else {
                root.join(below)
            }
        };
        [Some(&self.root), self.as_given.as_ref()]
            .into_iter()
            .flatten()
            .map(at)
            .filter_map(|dir| dir.to_str().map(String::from))
            .filter(|dir| dir.len() > 1)
            .collect()
    }

    /// `text` with absolute paths into the directory the tools run in made relative to it, which is
    /// how `display` names them and how the model is asked to give them: `<cwd>/src/a.py` becomes
    /// `src/a.py` and a bare `<cwd>` becomes `.`. That directory is the root until a `cd`; after one,
    /// workspace paths outside the new directory stay absolute, so that every path shown can be used
    /// as it stands.
    pub fn shorten<'a>(&self, text: &'a str) -> Cow<'a, str> {
        let mut short: Option<String> = None;
        for dir in self.cwd_spellings() {
            if let Some(cut) = cut_dir(short.as_deref().unwrap_or(text), &dir, cfg!(windows)) {
                short = Some(cut);
            }
        }
        short.map_or(Cow::Borrowed(text), Cow::Owned)
    }

    pub fn contains(&self, abs: &Path) -> bool {
        real_path(abs).starts_with(&self.root)
    }

    pub fn check_writable(&self, abs: &Path) -> Result<(), String> {
        if self.allow_outside_writes {
            return Ok(());
        }
        let real = real_path(abs);
        let tmp =
            crate::util::canonical(std::env::temp_dir()).unwrap_or_else(|_| std::env::temp_dir());
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
    pub fn observe(
        &mut self,
        path: &Path,
        hash: u64,
        from: usize,
        to: usize,
        step: u32,
    ) -> SeenCheck {
        let file = self.files.entry(path.to_path_buf()).or_insert(SeenFile {
            hash,
            windows: Vec::new(),
        });
        if file.hash != hash {
            *file = SeenFile {
                hash,
                windows: Vec::new(),
            };
        }
        if let Some(w) = file
            .windows
            .iter_mut()
            .find(|w| w.from <= from && w.to >= to)
        {
            if !w.stubbed {
                w.stubbed = true;
                return SeenCheck::AlreadyShown { step: w.step };
            }
            w.stubbed = false;
            w.step = step;
            return SeenCheck::Fresh;
        }
        file.windows.push(SeenWindow {
            from,
            to,
            step,
            stubbed: false,
        });
        SeenCheck::Fresh
    }

    /// Records a window without asking, for text the harness attaches on its own initiative.
    pub fn mark(&mut self, path: &Path, hash: u64, from: usize, to: usize, step: u32) {
        let file = self.files.entry(path.to_path_buf()).or_insert(SeenFile {
            hash,
            windows: Vec::new(),
        });
        if file.hash != hash {
            *file = SeenFile {
                hash,
                windows: Vec::new(),
            };
        }
        if !file.windows.iter().any(|w| w.from <= from && w.to >= to) {
            file.windows.push(SeenWindow {
                from,
                to,
                step,
                stubbed: false,
            });
        }
    }

    pub fn covered(&self, path: &Path, hash: u64, from: usize, to: usize) -> bool {
        self.files.get(path).is_some_and(|f| {
            f.hash == hash && f.windows.iter().any(|w| w.from <= from && w.to >= to)
        })
    }

    /// The line windows shown of a file while its content had this hash.
    pub fn windows(&self, path: &Path, hash: u64) -> Vec<(usize, usize)> {
        self.files
            .get(path)
            .filter(|f| f.hash == hash)
            .map(|f| f.windows.iter().map(|w| (w.from, w.to)).collect())
            .unwrap_or_default()
    }

    /// Forget everything: called when old observations are dropped from the context.
    pub fn clear(&mut self) {
        self.files.clear();
    }
}

/// Directories that are large, derived, and never the deliverable.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".venv",
    "venv",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    ".tox",
    ".next",
    ".cache",
    ".gradle",
    ".idea",
    ".ruff_cache",
    ".hypothesis",
    ".nox",
    ".build",
    "_build",
    ".dart_tool",
    ".stack-work",
    "dist-newstyle",
    ".zig-cache",
    "zig-cache",
    ".parcel-cache",
    ".turbo",
    ".svelte-kit",
    ".nuxt",
];

/// True for a path inside one of the directories that hold derived files (caches, dependencies).
pub(crate) fn derived(path: &str) -> bool {
    path.split('/').any(|part| SKIP_DIRS.contains(&part))
}

/// Files a fingerprint walks at most. Past that, what git says has changed stands in for the rest.
const FINGERPRINT_MAX_FILES: u32 = 100_000;
/// A walk that takes longer than this stops, and is covered by git as past the cap.
const FINGERPRINT_BUDGET: Duration = Duration::from_secs(2);
const GIT_TIMEOUT: Duration = Duration::from_secs(5);
/// A file whose time of change is this close to a look at the workspace may be changed again
/// without its times changing: file systems keep time in ticks of a few milliseconds, and some in
/// whole seconds. Such a file is told apart by its content, as git does with its racy entries.
const RACY_WINDOW: Duration = Duration::from_secs(2);
/// The largest such file whose content is kept, and the most content kept in one look.
const RACY_MAX_FILE: u64 = 1024 * 1024;
const RACY_MAX_TOTAL: u64 = 16 * 1024 * 1024;

/// Every file of a small workspace, relative to `root` and sorted. `None` when there are more than
/// `max_files`: a larger tree is for the model to explore.
pub fn small_tree(root: &Path, max_files: usize) -> Option<Vec<PathBuf>> {
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .follow_links(false)
        .filter_entry(|e| {
            !e.file_name()
                .to_str()
                .is_some_and(|n| SKIP_DIRS.contains(&n))
        })
        .build();
    let mut files = Vec::new();
    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(root) else {
            continue;
        };
        files.push(rel.to_path_buf());
        if files.len() > max_files {
            return None;
        }
    }
    files.sort();
    Some(files)
}

fn mix(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// Size and time of change, mixed.
fn stat_hash(md: &std::fs::Metadata) -> u64 {
    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    #[cfg(unix)]
    let changed = {
        use std::os::unix::fs::MetadataExt;
        mix(md.ctime() as u64).rotate_left(7) ^ mix(md.ctime_nsec() as u64)
    };
    #[cfg(not(unix))]
    let changed = 0;
    mix(md.len()) ^ mix(mtime).rotate_left(21) ^ changed
}

/// What a look at the workspace found.
#[derive(Clone, Debug, Default)]
pub struct Fingerprint {
    pub digest: u64,
    /// Every file that counts was seen. False only for a tree too large to walk that is not a git
    /// repository: there a change a command makes may go unnoticed.
    pub complete: bool,
    /// Files changed so shortly before the look that a rewrite of the same size could keep their
    /// times (`RACY_WINDOW`), with a hash of what they held.
    racy: Vec<(PathBuf, u64)>,
}

impl Fingerprint {
    /// Whether nothing has changed between `before` and this look: the same files, sizes and times,
    /// and in the files that were racy at `before`, the same content.
    pub fn unchanged_since(&self, before: &Fingerprint) -> bool {
        self.digest == before.digest
            && self.complete == before.complete
            && before
                .racy
                .iter()
                .all(|(path, hash)| content_hash(path) == Some(*hash))
    }
}

/// Collects the racy files of one look, within the limits on how much is read.
#[derive(Default)]
struct Racy {
    now: Option<std::time::SystemTime>,
    read: u64,
    files: Vec<(PathBuf, u64)>,
}

impl Racy {
    fn see(&mut self, path: &Path, md: &std::fs::Metadata) {
        let now = *self.now.get_or_insert_with(std::time::SystemTime::now);
        let modified = md.modified().ok();
        // The time of the last change to the file's status as well: a rewrite whose times were
        // put back still moves it.
        #[cfg(unix)]
        let stamp = {
            use std::os::unix::fs::MetadataExt;
            let ctime = UNIX_EPOCH
                + Duration::new(
                    md.ctime().max(0) as u64,
                    md.ctime_nsec().clamp(0, 999_999_999) as u32,
                );
            modified.max(Some(ctime))
        };
        #[cfg(not(unix))]
        let stamp = modified;
        let Some(stamp) = stamp else { return };
        // A time in the future is as racy as a recent one.
        let recent = now
            .duration_since(stamp)
            .map_or(true, |age| age < RACY_WINDOW);
        if !recent || md.len() > RACY_MAX_FILE || self.read + md.len() > RACY_MAX_TOTAL {
            return;
        }
        if let Some(hash) = content_hash(path) {
            self.read += md.len();
            self.files.push((path.to_path_buf(), hash));
        }
    }
}

fn content_hash(path: &Path) -> Option<u64> {
    std::fs::read(path).ok().map(|bytes| fnv64(&bytes))
}

/// Order-independent digest of (path, size, mtime) for every file under `root` that is not ignored
/// (`.gitignore`, `.ignore`, the derived directories): the build output and caches a test run
/// writes are nobody's change.
///
/// It is metadata-only, but for files changed in the last moments before it (`RACY_WINDOW`), whose
/// content is kept too: the question is "was anything touched", and it is asked around every
/// command. The walk goes in path order, so a tree too large for it is always cut at the same
/// files; in a git repository, what git says has changed then stands in for the whole tree.
pub fn fingerprint(root: &Path) -> Fingerprint {
    fingerprint_within(root, FINGERPRINT_MAX_FILES, FINGERPRINT_BUDGET)
}

/// A new look at the workspace, and whether anything changed since `before`, on a blocking thread:
/// the comparison reads the files that were racy then. A look that could not be taken counts as
/// unknown, and so as a change.
pub async fn look_again_off_thread(
    root: &Path,
    before: Option<Fingerprint>,
) -> (Fingerprint, bool) {
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let now = fingerprint(&root);
        let changed = before.is_some_and(|before| !now.unchanged_since(&before));
        (now, changed)
    })
    .await
    .unwrap_or((Fingerprint::default(), true))
}

fn fingerprint_within(root: &Path, max_files: u32, budget: Duration) -> Fingerprint {
    // The digest does not depend on the order files come in, so the walk takes them as the disk
    // gives them, which is fastest. Only one that is cut short needs them in path order.
    let mut racy = Racy::default();
    if let Some(digest) = walk_digest(root, max_files, budget, &mut racy) {
        return Fingerprint {
            digest,
            complete: true,
            racy: racy.files,
        };
    }
    let mut racy = Racy::default();
    if let Some(digest) = git_changes(root, &mut racy) {
        return Fingerprint {
            digest,
            complete: true,
            racy: racy.files,
        };
    }
    let mut racy = Racy::default();
    // No git to speak for the rest: the first files in path order, the same ones every time unless
    // the clock cuts this walk too.
    let start = Instant::now();
    let (mut acc, mut count) = (0u64, 0u32);
    for (path, md) in walk(root, true) {
        if count >= max_files || (count.is_multiple_of(256) && start.elapsed() > budget) {
            break;
        }
        acc = acc.wrapping_add(file_digest(&path, &md));
        racy.see(&path, &md);
        count += 1;
    }
    Fingerprint {
        digest: acc ^ u64::from(count),
        complete: false,
        racy: racy.files,
    }
}

/// The files under `root` that count, with their metadata: what is ignored and the derived
/// directories are left out. `sorted` puts them in path order.
fn walk(root: &Path, sorted: bool) -> impl Iterator<Item = (PathBuf, std::fs::Metadata)> {
    let mut builder = ignore::WalkBuilder::new(root);
    builder.hidden(false).follow_links(false).filter_entry(|e| {
        !e.file_name()
            .to_str()
            .is_some_and(|n| SKIP_DIRS.contains(&n))
    });
    if sorted {
        builder.sort_by_file_name(|a, b| a.cmp(b));
    }
    builder.build().flatten().filter_map(|entry| {
        let md = entry.metadata().ok()?;
        (!md.is_dir()).then(|| (entry.into_path(), md))
    })
}

fn file_digest(path: &Path, md: &std::fs::Metadata) -> u64 {
    mix(fnv64(path.as_os_str().as_encoded_bytes()) ^ stat_hash(md))
}

/// The digest of every file that counts, or `None` when there are more than `max_files` or the walk
/// takes longer than `budget`.
fn walk_digest(root: &Path, max_files: u32, budget: Duration, racy: &mut Racy) -> Option<u64> {
    let start = Instant::now();
    let (mut acc, mut count) = (0u64, 0u32);
    for (path, md) in walk(root, false) {
        if count >= max_files || (count.is_multiple_of(256) && start.elapsed() > budget) {
            return None;
        }
        acc = acc.wrapping_add(file_digest(&path, &md));
        racy.see(&path, &md);
        count += 1;
    }
    Some(acc ^ u64::from(count))
}

/// `git -C root <args>`, looking only, given up on after `GIT_TIMEOUT`. Its output if it succeeded.
fn git_output(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(root)
        .args(["-c", "core.quotepath=off"])
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flashing up behind the desktop app.
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });
    let deadline = Instant::now() + GIT_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let out = reader.join().ok()?;
    status.success().then_some(out)
}

/// Digest of what git says differs from the last commit below `root`, untracked files included,
/// with the size and time of change of each such file, so that one changed again is told apart.
/// The status letters are left out: staging a file changes nothing in the workspace. `None` outside
/// a repository, or when git does not answer in time.
fn git_changes(root: &Path, racy: &mut Racy) -> Option<u64> {
    let top = git_output(root, &["rev-parse", "--show-toplevel"])?;
    let top = PathBuf::from(String::from_utf8_lossy(&top).trim());
    let status = git_output(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            ".",
        ],
    )?;
    let mut acc = fnv64(b"git");
    if let Some(head) = git_output(root, &["rev-parse", "--verify", "HEAD"]) {
        acc ^= mix(fnv64(&head));
    }
    let mut entries = status.split(|b| *b == 0).filter(|e| !e.is_empty());
    while let Some(entry) = entries.next() {
        let (Some(code), Some(path)) = (entry.get(..2), entry.get(3..)) else {
            continue;
        };
        // A rename or a copy is followed by the path it came from, as an entry of its own.
        if code.contains(&b'R') || code.contains(&b'C') {
            entries.next();
        }
        let file = top.join(String::from_utf8_lossy(path).as_ref());
        let stat = match std::fs::symlink_metadata(&file) {
            Ok(md) => {
                if md.is_file() {
                    racy.see(&file, &md);
                }
                stat_hash(&md)
            }
            Err(_) => 0,
        };
        acc = acc.wrapping_add(mix(fnv64(path) ^ stat));
    }
    Some(acc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Whether the later of two looks at the workspace finds nothing changed since the earlier.
    fn same(before: &Fingerprint, after: &Fingerprint) -> bool {
        after.unchanged_since(before)
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sage-ws-{name}-{}", crate::util::rand_hex(4)));
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
        let outside = ws.display(&ws.root.join("other.rs"));
        assert!(Path::new(&outside).is_absolute());
        assert_eq!(ws.resolve(&outside), ws.root.join("other.rs"));
    }

    #[cfg(unix)]
    #[test]
    fn display_preserves_literal_backslashes_in_unix_file_names() {
        let ws = Workspace::new(&tmp("display-backslash")).unwrap();
        let name = r"dir/file\name.rs";
        assert_eq!(ws.display(&ws.root.join(name)), name);
        assert_eq!(
            ws.resolve(&ws.display(&ws.root.join(name))),
            ws.root.join(name)
        );
    }

    #[test]
    fn write_policy_confines_to_root_and_tmp_even_through_symlinks() {
        let root = tmp("policy");
        let outside = tmp("policy-outside");
        let ws = Workspace::new(&root).unwrap();
        assert!(ws.check_writable(&ws.resolve("new/dir/file.txt")).is_ok());
        assert!(
            ws.check_writable(&ws.resolve("../../etc/passwd")).is_err()
                || ws
                    .resolve("../../etc/passwd")
                    .starts_with(std::env::temp_dir())
        );
        assert!(
            ws.check_writable(Path::new("/etc/sage-should-not-write"))
                .is_err()
        );
        #[cfg(unix)]
        {
            // The policy must hold for a symlink that points out of the workspace into a non-temp place.
            std::os::unix::fs::symlink("/etc", root.join("escape")).unwrap();
            assert!(ws.check_writable(&ws.resolve("escape/hosts")).is_err());
        }
        let _ = outside;
    }

    #[test]
    fn shortening_cuts_the_tools_directory_from_paths_and_from_nothing_else() {
        let ws = Workspace::new(&tmp("short")).unwrap();
        let r = ws.root.display().to_string();
        let short = |text: String| ws.shorten(&text).into_owned();
        assert_eq!(short(format!("{r}/x/y.py:12: error")), "x/y.py:12: error");
        assert_eq!(
            short(format!(
                "  File \"{r}/pager.py\", line 3, in page\n  File \"{r}/test_pager.py\", line 9"
            )),
            "  File \"pager.py\", line 3, in page\n  File \"test_pager.py\", line 9"
        );
        // The directory on its own is `.`, whatever closes it.
        assert_eq!(short(format!("rootdir: {r}")), "rootdir: .");
        assert_eq!(
            short(format!("cd {r} && ls '{r}' {r}/")),
            "cd . && ls '.' ./"
        );
        assert_eq!(short(format!("PYTHONPATH={r}:{r}/src")), "PYTHONPATH=.:src");
        assert_eq!(short(format!("{r}//x.py")), "x.py");
        // Another place that only shares the prefix, or has it in the middle, is left as it is.
        for other in [
            format!("{r}-other/x.py"),
            format!("{r}.bak"),
            format!("{r}2/x.py"),
            format!("/mnt{r}/x.py"),
            format!("file://{r}/x.html"),
            format!("see {r}."),
        ] {
            assert!(matches!(ws.shorten(&other), Cow::Borrowed(_)), "{other}");
        }
        assert!(
            matches!(ws.shorten("3 passed in 0.12s"), Cow::Borrowed(_)),
            "nothing to cut, nothing copied"
        );
    }

    #[test]
    fn shortening_matches_windows_separators_without_rewriting_output() {
        let dir = r"C:\work\prøject\pkg";
        for prefix in [
            r"C:\work\prøject\pkg",
            "C:/work/prøject/pkg",
            r"C:\work/prøject\pkg",
        ] {
            assert_eq!(
                cut_dir(&format!("{prefix}/a.py:3"), dir, true),
                Some("a.py:3".to_string())
            );
            assert_eq!(
                cut_dir(&format!(r"{prefix}\deep\a.py:3"), dir, true),
                Some(r"deep\a.py:3".to_string())
            );
            assert_eq!(
                cut_dir(&format!(r"cd {prefix}\ && echo '\n'"), dir, true),
                Some(r"cd ./ && echo '\n'".to_string())
            );
            assert_eq!(cut_dir(prefix, dir, true), Some(".".to_string()));
            for other in [
                format!("{prefix}-old/a.py"),
                format!("{prefix}.bak"),
                format!(r"D:\parent\{prefix}\a.py"),
                format!("file://{prefix}/a.py"),
            ] {
                assert_eq!(cut_dir(&other, dir, true), None, "{other}");
            }
        }
        assert_eq!(
            cut_dir(
                r"\\host/share\project/pkg\a.py",
                r"\\host\share\project\pkg",
                true
            ),
            Some("a.py".to_string())
        );
        // On Unix a backslash can be a literal part of a file name, not a separator.
        assert_eq!(cut_dir(r"/work/pkg\a.py", "/work/pkg", false), None);
        assert_eq!(cut_dir(r"/work/pkg/a.py", r"/work\pkg", false), None);
    }

    #[test]
    fn shortening_follows_cd_and_knows_the_root_by_the_name_it_was_given() {
        let root = tmp("short-cd");
        fs::create_dir_all(root.join("pkg")).unwrap();
        let mut ws = Workspace::new(&root).unwrap();
        let r = ws.root.display().to_string();
        // After `cd pkg` the tools run there: what lies below is relative to it, the rest stays whole.
        ws.cwd = ws.root.join("pkg");
        assert_eq!(
            ws.shorten(&format!("{r}/pkg/a.py {r}/pkg {r}/b.py {r}")),
            format!("a.py . {r}/b.py {r}")
        );
        // Outside the workspace nothing is relative to anything.
        ws.cwd = ws.root.parent().unwrap().to_path_buf();
        assert!(matches!(ws.shorten(&format!("{r}/b.py")), Cow::Borrowed(_)));

        #[cfg(unix)]
        {
            // Reached through a symlink, the root has two spellings and commands may print either.
            let link = tmp("short-link").join("project");
            std::os::unix::fs::symlink(&ws.root, &link).unwrap();
            let linked = Workspace::new(&link).unwrap();
            let l = link.display().to_string();
            assert_eq!(
                (
                    linked.root.display().to_string(),
                    linked.as_given.as_deref()
                ),
                (r.clone(), Some(link.as_path()))
            );
            assert_eq!(
                linked.shorten(&format!("{l}/a.py:3 and {r}/b.py:4 in {l}")),
                "a.py:3 and b.py:4 in ."
            );
            assert!(matches!(
                linked.shorten(&format!("{l}-old/a.py")),
                Cow::Borrowed(_)
            ));
            // Given by its canonical name, it has only that one.
            assert_eq!(Workspace::new(&linked.root).unwrap().as_given, None);
        }
    }

    #[test]
    fn seen_stubs_once_then_reprints() {
        let mut seen = Seen::default();
        let p = Path::new("/w/a.rs");
        assert_eq!(seen.observe(p, 1, 1, 100, 3), SeenCheck::Fresh);
        assert_eq!(
            seen.observe(p, 1, 10, 50, 5),
            SeenCheck::AlreadyShown { step: 3 }
        );
        assert_eq!(
            seen.observe(p, 1, 10, 50, 6),
            SeenCheck::Fresh,
            "asking twice means it is needed"
        );
        assert_eq!(
            seen.observe(p, 1, 90, 140, 7),
            SeenCheck::Fresh,
            "not fully covered"
        );
        assert_eq!(
            seen.observe(p, 2, 1, 100, 8),
            SeenCheck::Fresh,
            "content changed"
        );
        assert!(seen.covered(p, 2, 5, 20));
        seen.clear();
        assert!(!seen.covered(p, 2, 5, 20));
    }

    #[test]
    fn fingerprint_changes_on_edit_create_delete_but_not_on_ignored_dirs() {
        let root = tmp("fp");
        fs::write(root.join("a.txt"), "one").unwrap();
        fs::create_dir_all(root.join("node_modules/x")).unwrap();
        let f0 = fingerprint(&root);
        assert!(f0.complete);
        assert!(
            same(&f0, &fingerprint(&root)),
            "stable when nothing changed"
        );

        fs::write(root.join("node_modules/x/big.js"), "ignored").unwrap();
        assert!(same(&f0, &fingerprint(&root)));

        fs::write(root.join("a.txt"), "three").unwrap();
        let f1 = fingerprint(&root);
        assert!(!same(&f0, &f1));

        fs::write(root.join("b.txt"), "").unwrap();
        let f2 = fingerprint(&root);
        assert!(!same(&f1, &f2));

        fs::remove_file(root.join("b.txt")).unwrap();
        assert!(same(&f1, &fingerprint(&root)));
    }

    fn git(root: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    #[cfg(unix)]
    #[test]
    fn fingerprint_detects_a_rewrite_with_same_size_and_restored_mtime() {
        let root = tmp("fingerprint-ctime");
        let path = root.join("data");
        fs::write(&path, "old").unwrap();
        let original = fingerprint(&root);
        let mtime = path.metadata().unwrap().modified().unwrap();
        std::thread::sleep(Duration::from_millis(10));
        fs::write(&path, "new").unwrap();
        let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_modified(mtime).unwrap();
        assert!(!same(&original, &fingerprint(&root)));
    }

    #[test]
    fn a_rewrite_that_keeps_the_size_and_times_of_a_racy_file_is_still_a_change() {
        let root = tmp("fp-racy");
        let path = root.join("a.txt");
        fs::write(&path, "one").unwrap();
        let before = fingerprint(&root);
        // A file system that keeps time in coarse ticks gives a rewrite in the same tick the same
        // times: the metadata cannot tell, so the digest is made to agree here as it would there.
        fs::write(&path, "two").unwrap();
        let mut after = fingerprint(&root);
        after.digest = before.digest;
        assert!(!after.unchanged_since(&before));
        // Nothing rewritten: the same content is no change, even after the file stops being racy.
        fs::write(&path, "one").unwrap();
        let before = fingerprint(&root);
        let mut later = fingerprint(&root);
        later.racy.clear();
        assert!(later.unchanged_since(&before));
        // A file changed long before the look is not read; one changed just now is.
        let md = path.metadata().unwrap();
        let mut racy = Racy::default();
        racy.see(&path, &md);
        assert_eq!(racy.files.len(), 1);
        let mut racy = Racy {
            now: Some(std::time::SystemTime::now() + Duration::from_secs(60)),
            ..Racy::default()
        };
        racy.see(&path, &md);
        assert!(racy.files.is_empty());
    }

    #[test]
    fn what_a_repository_ignores_is_no_change() {
        let root = tmp("fp-ignored");
        git(&root, &["init", "-q"]);
        fs::write(root.join(".gitignore"), "build/\n*.log\n").unwrap();
        fs::write(root.join("a.txt"), "one").unwrap();
        let f0 = fingerprint(&root);
        // What a test run leaves behind: build output and logs the project ignores.
        fs::create_dir_all(root.join("build/classes")).unwrap();
        fs::write(root.join("build/classes/A.class"), "x").unwrap();
        fs::write(root.join("test.log"), "ran").unwrap();
        assert!(same(&f0, &fingerprint(&root)));
        fs::write(root.join("a.txt"), "two").unwrap();
        assert!(!same(&f0, &fingerprint(&root)));
    }

    #[test]
    fn a_tree_past_the_cap_is_covered_by_git_and_said_to_be_partial_without_it() {
        let root = tmp("fp-cap");
        for name in ["a", "b", "c", "d", "e"] {
            fs::write(root.join(format!("{name}.txt")), name).unwrap();
        }
        // No repository: the same files are seen each time, a change past them is not, and it says so.
        let budget = Duration::from_secs(60);
        let f0 = fingerprint_within(&root, 2, budget);
        assert!(!f0.complete);
        assert!(
            same(&f0, &fingerprint_within(&root, 2, budget)),
            "a cut in path order is stable"
        );

        git(&root, &["init", "-q"]);
        git(&root, &["add", "."]);
        git(&root, &["commit", "-q", "-m", "start"]);
        let g0 = fingerprint_within(&root, 2, budget);
        assert!(g0.complete);
        assert!(same(&g0, &fingerprint_within(&root, 2, budget)));
        // The last file in path order, far past the cap: git sees it.
        fs::write(root.join("e.txt"), "changed").unwrap();
        let g1 = fingerprint_within(&root, 2, budget);
        assert!(!same(&g0, &g1));
        // Staging it changes nothing in the workspace; writing it again does.
        git(&root, &["add", "e.txt"]);
        assert!(same(&g1, &fingerprint_within(&root, 2, budget)));
        std::thread::sleep(Duration::from_millis(20));
        fs::write(root.join("e.txt"), "changed again!").unwrap();
        assert!(!same(&g1, &fingerprint_within(&root, 2, budget)));
        // A new file anywhere, too.
        fs::write(root.join("f.txt"), "new").unwrap();
        assert!(!same(&g1, &fingerprint_within(&root, 2, budget)));
    }
}
