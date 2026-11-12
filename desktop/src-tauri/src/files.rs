//! The files of a session's folder, for the editor beside the conversation: what a directory
//! holds, the text of a file, and that text written back. Nothing outside the folder is reached,
//! by a path that climbs out of it or through a link that leads out of it.

use serde::Serialize;
use std::path::{Path, PathBuf};

/// A longer file is not one to edit in a text box.
const MAX_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub name: String,
    /// From the folder's root, with `/` between its parts on every system.
    pub path: String,
    pub dir: bool,
}

/// Where `path` (from the root) is on disk. It has to be there, and under the root.
fn within(root: &Path, path: &str) -> Result<(PathBuf, PathBuf), String> {
    let root = root.canonicalize().map_err(|e| format!("cannot open {}: {e}", root.display()))?;
    let full = root.join(path).canonicalize().map_err(|e| format!("cannot open {path}: {e}"))?;
    if full.starts_with(&root) { Ok((root, full)) } else { Err(format!("{path} is outside the session's folder")) }
}

/// What the directory at `path` holds: folders first, then files, each by name. Git's own
/// directory is left out.
pub fn list(root: &Path, path: &str) -> Result<Vec<Entry>, String> {
    let (root, dir) = within(root, path)?;
    let mut entries: Vec<Entry> = std::fs::read_dir(&dir)
        .map_err(|e| format!("cannot list {path}: {e}"))?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let full = entry.path();
            let below = full.strip_prefix(&root).ok()?.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
            (name != ".git").then(|| Entry { name, path: below, dir: full.is_dir() })
        })
        .collect();
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(entries)
}

/// The text of the file at `path`.
pub fn read(root: &Path, path: &str) -> Result<String, String> {
    let (_, file) = within(root, path)?;
    let size = std::fs::metadata(&file).map_err(|e| format!("cannot read {path}: {e}"))?.len();
    if size > MAX_BYTES {
        return Err(format!("{path} is too large to edit here ({})", presage_core::util::fmt_bytes(size)));
    }
    String::from_utf8(std::fs::read(&file).map_err(|e| format!("cannot read {path}: {e}"))?).map_err(|_| format!("{path} is not text"))
}

/// Writes `text` to the file at `path`, which is there already.
pub fn write(root: &Path, path: &str, text: &str) -> Result<(), String> {
    let (_, file) = within(root, path)?;
    if !file.is_file() {
        return Err(format!("{path} is not a file"));
    }
    std::fs::write(&file, text).map_err(|e| format!("cannot write {path}: {e}"))
}

/// Where a new entry named by `path` would go: its folder has to be there and under the root, the
/// name has to be a plain one, and nothing may be there yet.
fn fresh(root: &Path, path: &str) -> Result<PathBuf, String> {
    let path = path.trim_matches('/');
    let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
    if name.is_empty() || name == "." || name == ".." || name.contains(['\\', ':']) || name == ".git" {
        return Err(format!("{name:?} is not a name a file can have"));
    }
    let (_, folder) = within(root, parent)?;
    if !folder.is_dir() {
        return Err(format!("{parent} is not a folder"));
    }
    let target = folder.join(name);
    if target.symlink_metadata().is_ok() {
        return Err(format!("{path} is there already"));
    }
    Ok(target)
}

/// Makes an empty file, or a folder when `dir`, at `path`.
pub fn create(root: &Path, path: &str, dir: bool) -> Result<(), String> {
    let target = fresh(root, path)?;
    if dir { std::fs::create_dir(&target) } else { std::fs::OpenOptions::new().write(true).create_new(true).open(&target).map(|_| ()) }.map_err(|e| format!("cannot make {path}: {e}"))
}

/// Moves the file or folder at `from` to `to`, both from the root.
pub fn rename(root: &Path, from: &str, to: &str) -> Result<(), String> {
    let (root_dir, source) = within(root, from)?;
    if source == root_dir {
        return Err("the session's folder itself cannot be renamed here".into());
    }
    let target = fresh(root, to)?;
    std::fs::rename(&source, &target).map_err(|e| format!("cannot rename {from}: {e}"))
}

/// Deletes the file or folder, with all it holds, at `path`.
pub fn remove(root: &Path, path: &str) -> Result<(), String> {
    let (root_dir, target) = within(root, path)?;
    if target == root_dir {
        return Err("the session's folder itself cannot be deleted here".into());
    }
    if target.is_dir() { std::fs::remove_dir_all(&target) } else { std::fs::remove_file(&target) }.map_err(|e| format!("cannot delete {path}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_is_listed_read_and_written_and_nothing_outside_it_is() {
        let base = std::env::temp_dir().join(format!("presage-files-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("work");
        std::fs::create_dir_all(root.join("src/deep")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("b.txt"), "one\n").unwrap();
        std::fs::write(root.join("A.md"), "# a\n").unwrap();
        std::fs::write(root.join("src/deep/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join("blob.bin"), [0xff, 0xfe, 0x00]).unwrap();
        std::fs::write(base.join("secret.txt"), "no").unwrap();

        let names = |path: &str| list(&root, path).unwrap().into_iter().map(|entry| (entry.path, entry.dir)).collect::<Vec<_>>();
        assert_eq!(names(""), [("src".to_string(), true), ("A.md".to_string(), false), ("b.txt".to_string(), false), ("blob.bin".to_string(), false)]);
        assert_eq!(names("src/deep"), [("src/deep/main.rs".to_string(), false)]);

        assert_eq!(read(&root, "src/deep/main.rs").unwrap(), "fn main() {}\n");
        write(&root, "b.txt", "two\n").unwrap();
        assert_eq!(read(&root, "b.txt").unwrap(), "two\n");
        assert!(read(&root, "blob.bin").unwrap_err().contains("not text"));
        assert!(write(&root, "new.txt", "x").is_err() && write(&root, "src", "x").unwrap_err().contains("not a file"));

        for outside in ["../secret.txt", base.join("secret.txt").to_str().unwrap()] {
            assert!(read(&root, outside).unwrap_err().contains("outside"), "{outside}");
            assert!(write(&root, outside, "x").unwrap_err().contains("outside"), "{outside}");
        }
        assert!(list(&root, "..").unwrap_err().contains("outside"));

        create(&root, "src/new.txt", false).unwrap();
        create(&root, "src/made", true).unwrap();
        assert!(create(&root, "src/new.txt", false).unwrap_err().contains("there already"));
        assert!(create(&root, "../escape.txt", false).is_err() && create(&root, "x/..", false).is_err());
        rename(&root, "src/new.txt", "src/made/moved.txt").unwrap();
        assert_eq!(names("src/made"), [("src/made/moved.txt".to_string(), false)]);
        assert!(rename(&root, "b.txt", "A.md").unwrap_err().contains("there already"));
        assert!(rename(&root, "b.txt", "../out.txt").is_err() && rename(&root, "", "x").is_err());
        remove(&root, "src/made").unwrap();
        remove(&root, "b.txt").unwrap();
        assert_eq!(names("src"), [("src/deep".to_string(), true)]);
        assert!(remove(&root, "").is_err() && remove(&root, "../secret.txt").is_err());
        let _ = std::fs::remove_dir_all(&base);
    }
}
