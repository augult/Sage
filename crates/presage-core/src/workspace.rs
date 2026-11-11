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

/// The directory the user's shell believes it is in, which keeps the symlinks they came through.
fn logical_cwd() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let same = |p: &PathBuf| p.is_absolute() && std::fs::canonicalize(p).ok() == std::fs::canonicalize(&cwd).ok();
    Some(std::env::var_os("PWD").map(PathBuf::from).filter(same).unwrap_or(cwd))
}

/// `given` as an absolute path with its symlinks left in, if that spells `canonical` differently.
fn as_given(given: &Path, canonical: &Path) -> Option<PathBuf> {
    let spelled = if given.is_absolute() { normalize(given) } else { normalize(&logical_cwd()?.join(given)) };
    (spelled != canonical && std::fs::canonicalize(&spelled).is_ok_and(|real| real == canonical)).then_some(spelled)
}

/// What can sit inside a file name: next to one of these, a path goes on.
fn in_name(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '+' | '~' | '@' | '%' | '#')
}

/// True where a path written in running text is over.
fn ends_path(next: Option<char>) -> bool {
    next.is_none_or(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ')' | ']' | '}' | '>' | '<' | ',' | ';' | ':' | '|' | '&' | '’' | '”'))
}

/// `text` without `dir` wherever a path starts with it: `dir/x` becomes `x`, `dir` on its own `.`
/// and `dir/` on its own `./`. `None` when nothing does. A path that only shares the prefix
/// (`dir-old/x`, `/mnt/dir/x`) is another place and is left as it is.
fn cut_dir(text: &str, dir: &str) -> Option<String> {
    let step = dir.chars().next()?.len_utf8();
    let mut out: Option<String> = None;
    let (mut copied, mut from) = (0, 0);
    while let Some(at) = text[from..].find(dir).map(|i| from + i) {
        from = at + step;
        let rest = &text[at + dir.len()..];
        let below = rest.trim_start_matches('/');
        let slashes = rest.len() - below.len();
        // Glued to what stands before it, the match is the tail of a longer path.
        let glued = text[..at].chars().next_back().is_some_and(|c| in_name(c) || matches!(c, '/' | '\\' | '$' | '}' | ')'));
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
        let root = std::fs::canonicalize(given)?;
        if !root.is_dir() {
            return Err(std::io::Error::new(std::io::ErrorKind::NotADirectory, "workspace root is not a directory"));
        }
        Ok(Self { cwd: root.clone(), as_given: as_given(given, &root), root, allow_outside_writes: false })
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

    /// Every absolute spelling of the directory the tools run in. None once a `cd` has left the
    /// workspace: paths out there are named in full.
    fn cwd_spellings(&self) -> Vec<String> {
        let Ok(below) = self.cwd.strip_prefix(&self.root) else { return Vec::new() };
        let at = |root: &PathBuf| if below.as_os_str().is_empty() { root.clone() } else { root.join(below) };
        [Some(&self.root), self.as_given.as_ref()].into_iter().flatten().map(at).filter_map(|dir| dir.to_str().map(String::from)).filter(|dir| dir.len() > 1).collect()
    }

    /// `text` with absolute paths into the directory the tools run in made relative to it, which is
    /// how `display` names them and how the model is asked to give them: `<cwd>/src/a.py` becomes
    /// `src/a.py` and a bare `<cwd>` becomes `.`. That directory is the root until a `cd`; after one,
    /// workspace paths outside the new directory stay absolute, so that every path shown can be used
    /// as it stands.
    pub fn shorten<'a>(&self, text: &'a str) -> Cow<'a, str> {
        let mut short: Option<String> = None;
        for dir in self.cwd_spellings() {
            if let Some(cut) = cut_dir(short.as_deref().unwrap_or(text), &dir) {
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

    /// The line windows shown of a file while its content had this hash.
    pub fn windows(&self, path: &Path, hash: u64) -> Vec<(usize, usize)> {
        self.files.get(path).filter(|f| f.hash == hash).map(|f| f.windows.iter().map(|w| (w.from, w.to)).collect()).unwrap_or_default()
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

/// True for a path inside one of the directories that hold derived files (caches, dependencies).
pub(crate) fn derived(path: &str) -> bool {
    path.split('/').any(|part| SKIP_DIRS.contains(&part))
}

const FINGERPRINT_MAX_FILES: u32 = 20_000;

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
        .filter_entry(|e| !e.file_name().to_str().is_some_and(|n| SKIP_DIRS.contains(&n)))
        .build();
    let mut files = Vec::new();
    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(root) else { continue };
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
        if count >= FINGERPRINT_MAX_FILES || (count.is_multiple_of(256) && start.elapsed() > budget) {
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
    fn shortening_cuts_the_tools_directory_from_paths_and_from_nothing_else() {
        let ws = Workspace::new(&tmp("short")).unwrap();
        let r = ws.root.display().to_string();
        let short = |text: String| ws.shorten(&text).into_owned();
        assert_eq!(short(format!("{r}/x/y.py:12: error")), "x/y.py:12: error");
        assert_eq!(short(format!("  File \"{r}/pager.py\", line 3, in page\n  File \"{r}/test_pager.py\", line 9")), "  File \"pager.py\", line 3, in page\n  File \"test_pager.py\", line 9");
        // The directory on its own is `.`, whatever closes it.
        assert_eq!(short(format!("rootdir: {r}")), "rootdir: .");
        assert_eq!(short(format!("cd {r} && ls '{r}' {r}/")), "cd . && ls '.' ./");
        assert_eq!(short(format!("PYTHONPATH={r}:{r}/src")), "PYTHONPATH=.:src");
        assert_eq!(short(format!("{r}//x.py")), "x.py");
        // Another place that only shares the prefix, or has it in the middle, is left as it is.
        for other in [format!("{r}-other/x.py"), format!("{r}.bak"), format!("{r}2/x.py"), format!("/mnt{r}/x.py"), format!("file://{r}/x.html"), format!("see {r}.")] {
            assert!(matches!(ws.shorten(&other), Cow::Borrowed(_)), "{other}");
        }
        assert!(matches!(ws.shorten("3 passed in 0.12s"), Cow::Borrowed(_)), "nothing to cut, nothing copied");
    }

    #[test]
    fn shortening_follows_cd_and_knows_the_root_by_the_name_it_was_given() {
        let root = tmp("short-cd");
        fs::create_dir_all(root.join("pkg")).unwrap();
        let mut ws = Workspace::new(&root).unwrap();
        let r = ws.root.display().to_string();
        // After `cd pkg` the tools run there: what lies below is relative to it, the rest stays whole.
        ws.cwd = ws.root.join("pkg");
        assert_eq!(ws.shorten(&format!("{r}/pkg/a.py {r}/pkg {r}/b.py {r}")), format!("a.py . {r}/b.py {r}"));
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
            assert_eq!((linked.root.display().to_string(), linked.as_given.as_deref()), (r.clone(), Some(link.as_path())));
            assert_eq!(linked.shorten(&format!("{l}/a.py:3 and {r}/b.py:4 in {l}")), "a.py:3 and b.py:4 in .");
            assert!(matches!(linked.shorten(&format!("{l}-old/a.py")), Cow::Borrowed(_)));
            // Given by its canonical name, it has only that one.
            assert_eq!(Workspace::new(&linked.root).unwrap().as_given, None);
        }
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
