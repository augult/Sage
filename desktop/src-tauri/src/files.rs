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
        let _ = std::fs::remove_dir_all(&base);
    }
}
