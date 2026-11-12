//! Skills: named sets of instructions for one kind of work, switched on per session.
//!
//! A skill is a markdown file with a small front matter block (`name`, `title`, `description`) and
//! the instructions as its body. A project's skills (`<cwd>/.sage/skills`) take precedence over
//! the user's (`<data_dir>/skills`), which take precedence over the built-in ones.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// In the order they are listed.
const BUILTIN: &[(&str, &str)] = &[
    ("ui", include_str!("../skills/ui.md")),
    ("security", include_str!("../skills/security.md")),
    ("cleanup", include_str!("../skills/cleanup.md")),
    ("debug", include_str!("../skills/debug.md")),
    ("tests", include_str!("../skills/tests.md")),
    ("performance", include_str!("../skills/performance.md")),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillSource {
    Builtin,
    User,
    Project,
}

impl SkillSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::User => "user",
            Self::Project => "project",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SkillInfo {
    pub name: String,
    pub title: String,
    /// One line, for pickers.
    pub description: String,
    pub source: SkillSource,
    /// A user or project skill that stands in for a built-in one of the same name.
    pub overrides_builtin: bool,
    /// The instructions the model gets.
    pub body: String,
}

/// `[a-z0-9][a-z0-9-]{0,31}`
pub fn valid_name(name: &str) -> bool {
    let b = name.as_bytes();
    (1..=32).contains(&b.len()) && b[0] != b'-' && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

fn is_builtin(name: &str) -> bool {
    BUILTIN.iter().any(|(n, _)| *n == name)
}

/// Splits a skill file into its front matter (`key: value` lines between two `---` lines) and its
/// body. A file that does not open with a closed front matter block is all body.
fn front_matter(text: &str) -> (Vec<(String, String)>, &str) {
    let text = text.trim_start_matches('\u{feff}');
    let mut lines = text.split_inclusive('\n');
    if lines.next().map(str::trim) != Some("---") {
        return (Vec::new(), text);
    }
    let mut fields = Vec::new();
    let mut offset = text.find('\n').map_or(text.len(), |i| i + 1);
    for line in lines {
        offset += line.len();
        if line.trim() == "---" {
            return (fields, &text[offset..]);
        }
        if let Some((key, value)) = line.split_once(':') {
            let value = value.trim();
            let unquoted = ['"', '\''].iter().find_map(|q| value.strip_prefix(*q)?.strip_suffix(*q)).unwrap_or(value);
            fields.push((key.trim().to_ascii_lowercase(), unquoted.to_string()));
        }
    }
    (Vec::new(), text)
}

/// Reads a skill file. `stem` names the skill when the front matter does not. `None` when the
/// resulting name is not a valid one.
pub fn parse(stem: &str, text: &str, source: SkillSource) -> Option<SkillInfo> {
    let (fields, body) = front_matter(text);
    let field = |key: &str| fields.iter().find(|(k, v)| k == key && !v.is_empty()).map(|(_, v)| v.clone());
    let name = field("name").unwrap_or_else(|| stem.to_string());
    if !valid_name(&name) {
        return None;
    }
    Some(SkillInfo {
        title: field("title").unwrap_or_else(|| name.clone()),
        description: field("description").unwrap_or_default(),
        source,
        overrides_builtin: source != SkillSource::Builtin && is_builtin(&name),
        body: body.trim().to_string(),
        name,
    })
}

fn user_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("skills")
}

/// The skills in one directory with the file each came from, in file-name order. Where two files
/// claim the same name, the first one has it.
fn read_dir(dir: &Path, source: SkillSource) -> Vec<(SkillInfo, PathBuf)> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "md")).collect();
    paths.sort();
    let mut found: Vec<(SkillInfo, PathBuf)> = Vec::new();
    for path in paths {
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        if let Some(skill) = parse(stem, &text, source).filter(|s| found.iter().all(|(f, _)| f.name != s.name)) {
            found.push((skill, path));
        }
    }
    found
}

/// Every skill with the text of its file: the built-in ones in their own order, each replaced by a
/// user or project skill of the same name, then the others by name.
fn load(data_dir: &Path, cwd: Option<&Path>) -> Vec<(SkillInfo, String)> {
    let mut all: Vec<(SkillInfo, String)> = BUILTIN.iter().filter_map(|(name, text)| Some((parse(name, text, SkillSource::Builtin)?, text.to_string()))).collect();
    let mut extra: Vec<(SkillInfo, String)> = Vec::new();
    // `.presage` is the folder's former name; a skill in `.sage` replaces one of the same name there.
    let project: Vec<_> = cwd.map(|c| [".presage", ".sage"].into_iter().flat_map(|dir| read_dir(&c.join(dir).join("skills"), SkillSource::Project)).collect()).unwrap_or_default();
    for (skill, path) in read_dir(&user_dir(data_dir), SkillSource::User).into_iter().chain(project) {
        let entry = (skill, std::fs::read_to_string(&path).unwrap_or_default());
        match all.iter_mut().chain(extra.iter_mut()).find(|(s, _)| s.name == entry.0.name) {
            Some(slot) => *slot = entry,
            None => extra.push(entry),
        }
    }
    extra.sort_by(|a, b| a.0.name.cmp(&b.0.name));
    all.extend(extra);
    all
}

pub fn list(data_dir: &Path, cwd: Option<&Path>) -> Vec<SkillInfo> {
    load(data_dir, cwd).into_iter().map(|(skill, _)| skill).collect()
}

/// The skills with these names, in the order asked for and each once. An unknown name is an error.
pub fn resolve(data_dir: &Path, cwd: Option<&Path>, names: &[String]) -> Result<Vec<SkillInfo>, String> {
    let all = list(data_dir, cwd);
    let mut picked: Vec<SkillInfo> = Vec::new();
    for name in names.iter().map(|n| n.trim()).filter(|n| !n.is_empty()) {
        let skill = all.iter().find(|s| s.name == name).ok_or_else(|| format!("unknown skill {name:?}; available: {}", all.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")))?;
        if picked.iter().all(|p| p.name != skill.name) {
            picked.push(skill.clone());
        }
    }
    Ok(picked)
}

/// The whole file of a skill, front matter included.
pub fn file_text(data_dir: &Path, cwd: Option<&Path>, name: &str) -> Result<String, String> {
    load(data_dir, cwd).into_iter().find(|(s, _)| s.name == name).map(|(_, text)| text).ok_or_else(|| format!("unknown skill {name:?}"))
}

/// Writes a user skill to `<data_dir>/skills/<name>.md`. `content` is the whole file.
pub fn save_user(data_dir: &Path, name: &str, content: &str) -> Result<SkillInfo, String> {
    if !valid_name(name) {
        return Err(format!("{name:?} is not a skill name: use lower-case letters, digits and dashes, 32 characters at most"));
    }
    let skill = parse(name, content, SkillSource::User).filter(|s| s.name == name).ok_or_else(|| format!("the front matter gives another name than {name:?}"))?;
    if skill.body.is_empty() {
        return Err("the skill has no instructions".into());
    }
    let dir = user_dir(data_dir);
    let path = dir.join(format!("{name}.md"));
    std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, content)).map_err(|e| format!("could not save the skill: {e}"))?;
    // Another file claiming the name would otherwise shadow this one or outlive its deletion.
    for (other, file) in read_dir(&dir, SkillSource::User) {
        if other.name == name && file != path {
            let _ = std::fs::remove_file(file);
        }
    }
    Ok(skill)
}

/// Deletes a user skill. Where it stood in for a built-in one, that one is back.
pub fn delete_user(data_dir: &Path, name: &str) -> Result<(), String> {
    let dir = user_dir(data_dir);
    let mut removed = false;
    // A second file with the same name is hidden by the first, so look again after each removal.
    while let Some((_, path)) = read_dir(&dir, SkillSource::User).into_iter().find(|(s, _)| s.name == name) {
        std::fs::remove_file(&path).map_err(|e| format!("could not delete the skill: {e}"))?;
        removed = true;
    }
    if removed { Ok(()) } else { Err(format!("there is no user skill named {name:?}")) }
}

/// Skills in the form they take in front of a user message, for a session whose system prompt was
/// sent without them.
pub fn blocks(skills: &[SkillInfo]) -> String {
    skills.iter().map(|s| format!("<skill name=\"{}\">\n{}\n</skill>", s.name, s.body)).collect::<Vec<_>>().join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::rand_hex;

    fn dirs(name: &str) -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("sage-skills-{name}-{}", rand_hex(4)));
        let (data, cwd) = (base.join("data"), base.join("ws"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(cwd.join(".sage/skills")).unwrap();
        (data, cwd)
    }

    #[test]
    fn front_matter_is_parsed_with_fallbacks() {
        let full = parse("file", "---\nname: sec-2\ntitle: \"Security audit\"\ndescription: One line: for pickers.\nlicense: MIT\n---\n\nLook for injection.\nReport it.\n", SkillSource::User).unwrap();
        assert_eq!((full.name.as_str(), full.title.as_str(), full.description.as_str()), ("sec-2", "Security audit", "One line: for pickers."));
        assert_eq!((full.body.as_str(), full.source, full.overrides_builtin), ("Look for injection.\nReport it.", SkillSource::User, false));

        // No name: the file stem. No title: the name.
        let bare = parse("deploy", "---\ndescription: Shipping.\n---\nShip it.", SkillSource::Project).unwrap();
        assert_eq!((bare.name.as_str(), bare.title.as_str(), bare.description.as_str(), bare.body.as_str()), ("deploy", "deploy", "Shipping.", "Ship it."));

        // No front matter, or one that is never closed: all of it is the body.
        let plain = parse("notes", "Just do it.\n---\nname: other\n", SkillSource::User).unwrap();
        assert_eq!((plain.name.as_str(), plain.body.as_str()), ("notes", "Just do it.\n---\nname: other"));
        assert_eq!(parse("open", "---\nname: other\nbody", SkillSource::User).unwrap().name, "open");
        assert_eq!(parse("crlf", "---\r\ntitle: T\r\n---\r\nBody\r\n", SkillSource::User).unwrap().title, "T");

        assert!(parse("My Skill", "body", SkillSource::User).is_none());
        assert!(parse("ok", "---\nname: Not Valid\n---\nbody", SkillSource::User).is_none());
        assert!(valid_name("a") && valid_name("9-lives") && valid_name(&"a".repeat(32)));
        assert!(!valid_name("") && !valid_name("-a") && !valid_name("A") && !valid_name("a_b") && !valid_name(&"a".repeat(33)));
    }

    #[test]
    fn the_built_in_skills_are_well_formed_and_listed_in_order() {
        let (data, _) = dirs("builtin");
        let all = list(&data, None);
        assert_eq!(all.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["ui", "security", "cleanup", "debug", "tests", "performance"]);
        for s in &all {
            assert!(s.source == SkillSource::Builtin && !s.overrides_builtin);
            assert!(!s.title.is_empty() && !s.description.is_empty() && !s.body.is_empty(), "{s:?}");
        }
        assert_eq!(serde_json::to_value(&all[0]).unwrap()["source"], "builtin");
    }

    #[test]
    fn project_skills_win_over_user_skills_which_win_over_built_in_ones() {
        let (data, cwd) = dirs("precedence");
        save_user(&data, "security", "---\ntitle: Mine\n---\nuser security").unwrap();
        save_user(&data, "zeta", "user zeta").unwrap();
        std::fs::write(cwd.join(".sage/skills/security.md"), "---\ntitle: Ours\n---\nproject security").unwrap();
        std::fs::write(cwd.join(".sage/skills/alpha.md"), "project alpha").unwrap();
        std::fs::write(cwd.join(".sage/skills/README.txt"), "not a skill").unwrap();

        let get = |cwd: Option<&Path>, name: &str| list(&data, cwd).into_iter().find(|s| s.name == name).unwrap();
        assert_eq!((get(Some(&cwd), "security").source, get(Some(&cwd), "security").body.as_str()), (SkillSource::Project, "project security"));
        assert_eq!((get(None, "security").source, get(None, "security").title.as_str()), (SkillSource::User, "Mine"));
        assert!(get(None, "security").overrides_builtin && get(Some(&cwd), "security").overrides_builtin && !get(None, "zeta").overrides_builtin);
        // An override keeps the built-in's place; the rest follow by name.
        assert_eq!(list(&data, Some(&cwd)).iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["ui", "security", "cleanup", "debug", "tests", "performance", "alpha", "zeta"]);
        assert_eq!(file_text(&data, Some(&cwd), "security").unwrap(), "---\ntitle: Ours\n---\nproject security");
        assert!(file_text(&data, None, "ui").unwrap().starts_with("---\nname: ui\n"));

        // Deleting the user's copy brings the built-in one back.
        delete_user(&data, "security").unwrap();
        assert_eq!((get(None, "security").source, get(None, "security").overrides_builtin), (SkillSource::Builtin, false));
        assert!(delete_user(&data, "security").is_err() && delete_user(&data, "ui").is_err());
    }

    #[test]
    fn saving_validates_and_resolving_rejects_unknown_names() {
        let (data, cwd) = dirs("save");
        assert!(save_user(&data, "Bad Name", "body").unwrap_err().contains("not a skill name"));
        assert!(save_user(&data, "../x", "body").is_err());
        assert!(save_user(&data, "one", "---\nname: two\n---\nbody").unwrap_err().contains("another name"));
        assert!(save_user(&data, "one", "---\nname: one\n---\n  \n").unwrap_err().contains("no instructions"));
        let saved = save_user(&data, "one", "---\nname: one\ndescription: First.\n---\nbody").unwrap();
        assert_eq!((saved.source, saved.description.as_str(), saved.body.as_str()), (SkillSource::User, "First.", "body"));

        // A hand-made file that claims the same name does not survive the next save or a delete.
        std::fs::write(data.join("skills/a.md"), "---\nname: one\n---\nold").unwrap();
        save_user(&data, "one", "new").unwrap();
        assert!(!data.join("skills/a.md").exists() && list(&data, None).iter().any(|s| s.name == "one" && s.body == "new"));

        let picked = resolve(&data, Some(&cwd), &["one".into(), " ui ".into(), "one".into(), String::new()]).unwrap();
        assert_eq!(picked.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["one", "ui"]);
        let err = resolve(&data, Some(&cwd), &["ui".into(), "telepathy".into()]).unwrap_err();
        assert!(err.contains("unknown skill \"telepathy\"") && err.contains("security"), "{err}");

        assert_eq!(blocks(&picked[..1]), "<skill name=\"one\">\nnew\n</skill>");
    }
}
