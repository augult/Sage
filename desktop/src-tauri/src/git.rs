//! Committing and pushing a session's workspace: what the Push button runs. Plain `git`, as the
//! user's own shell would run it, and never interactive: a push that would ask for a password
//! fails with git's own message instead.

use serde::Serialize;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

/// The longest any one git command may take. Only a push over a slow link comes close.
const TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ChangedFile {
    pub path: String,
    /// `modified`, `added`, `deleted`, `renamed` or `untracked`.
    pub status: &'static str,
}

/// Where a workspace stands with git: what a push from it would do.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct GitState {
    /// The folder is inside a git work tree. Everything below is empty when it is not.
    pub repo: bool,
    /// The branch checked out; `None` while HEAD is detached.
    pub branch: Option<String>,
    /// The remote a push goes to: the branch's own, else `origin`, else the first there is.
    pub remote: Option<String>,
    /// Its URL, without any credentials written into it.
    pub url: Option<String>,
    /// `owner/name` when the remote is on github.com.
    pub github: Option<String>,
    /// The branch follows a branch on the remote.
    pub upstream: bool,
    /// Commits here that the remote does not have.
    pub ahead: u32,
    /// Uncommitted changes, untracked files included.
    pub files: Vec<ChangedFile>,
    /// The latest commits of what is checked out, newest first. The first `ahead` are unpushed.
    pub commits: Vec<Commit>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Commit {
    /// Short hash.
    pub hash: String,
    /// First line of the message.
    pub subject: String,
    /// How long ago, as git words it: "3 hours ago".
    pub when: String,
}

/// How many commits `state` lists.
const LISTED_COMMITS: &str = "12";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Pushed {
    /// Short hash of the commit made for the uncommitted changes, when there were any.
    pub commit: Option<String>,
    pub remote: String,
    pub branch: String,
    /// False when the remote already had everything.
    pub pushed: bool,
}

/// Runs `git` in `cwd`. Gives back what it printed, or what it said went wrong.
async fn git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(args).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    // Nobody is at a terminal to answer a prompt, and a session may be using the index right now.
    cmd.env("GIT_TERMINAL_PROMPT", "0").env("GIT_OPTIONAL_LOCKS", "0");
    // CREATE_NO_WINDOW: no console flashing up behind the app.
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let name = args.first().copied().unwrap_or_default();
    let output = match tokio::time::timeout(TIMEOUT, cmd.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => return Err(format!("cannot run git: {e}")),
        Err(_) => return Err(format!("git {name} did not finish in {}s", TIMEOUT.as_secs())),
    };
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let said = [&output.stderr, &output.stdout].into_iter().map(|bytes| String::from_utf8_lossy(bytes).trim().to_string()).find(|text| !text.is_empty());
    Err(said.unwrap_or_else(|| format!("git {name} failed ({})", output.status)))
}

/// A remote URL without the user name and password that may be written into it.
fn without_credentials(url: &str) -> String {
    for scheme in ["https://", "http://"] {
        if let Some(rest) = url.strip_prefix(scheme) {
            let authority = &rest[..rest.find('/').unwrap_or(rest.len())];
            if let Some(at) = authority.rfind('@') {
                return format!("{scheme}{}", &rest[at + 1..]);
            }
        }
    }
    url.to_string()
}

/// `owner/name` of a remote on github.com, in any of the forms git takes for one.
fn github_slug(url: &str) -> Option<String> {
    let url = without_credentials(url.trim());
    let lower = url.to_ascii_lowercase();
    let prefix = ["https://github.com/", "http://github.com/", "ssh://git@github.com/", "git://github.com/", "git@github.com:"].into_iter().find(|prefix| lower.starts_with(prefix))?;
    let path = url[prefix.len()..].trim_end_matches('/');
    let (owner, name) = path.strip_suffix(".git").unwrap_or(path).split_once('/')?;
    (!owner.is_empty() && !name.is_empty() && !name.contains('/')).then(|| format!("{owner}/{name}"))
}

/// Reads `git status --porcelain=v1 -z`: `XY path`, NUL-ended, a rename followed by its old name.
fn parse_status(out: &str) -> Vec<ChangedFile> {
    let mut files = Vec::new();
    let mut entries = out.split('\0');
    while let Some(entry) = entries.next() {
        let Some((code, path)) = entry.split_at_checked(3).filter(|(_, path)| !path.is_empty()) else { continue };
        let moved = code.contains(['R', 'C']);
        if moved {
            entries.next();
        }
        let status = if code.starts_with("??") {
            "untracked"
        } else if moved {
            "renamed"
        } else if code.contains('D') {
            "deleted"
        } else if code.starts_with('A') {
            "added"
        } else {
            "modified"
        };
        files.push(ChangedFile { path: path.to_string(), status });
    }
    files
}

async fn remote_for(cwd: &Path, branch: Option<&str>) -> Option<String> {
    if let Some(branch) = branch
        && let Ok(name) = git(cwd, &["config", "--get", &format!("branch.{branch}.remote")]).await
        && !name.trim().is_empty()
        && name.trim() != "."
    {
        return Some(name.trim().to_string());
    }
    let remotes = git(cwd, &["remote"]).await.ok()?;
    let names: Vec<&str> = remotes.lines().map(str::trim).filter(|name| !name.is_empty()).collect();
    names.iter().find(|name| **name == "origin").or(names.first()).map(|name| name.to_string())
}

pub async fn state(cwd: &Path) -> GitState {
    if git(cwd, &["rev-parse", "--is-inside-work-tree"]).await.is_ok_and(|out| out.trim() == "true") {
        let branch = git(cwd, &["symbolic-ref", "--quiet", "--short", "HEAD"]).await.ok().map(|name| name.trim().to_string()).filter(|name| !name.is_empty());
        let remote = remote_for(cwd, branch.as_deref()).await;
        let url = match &remote {
            Some(name) => git(cwd, &["remote", "get-url", name]).await.ok().map(|url| url.trim().to_string()),
            None => None,
        };
        let upstream = git(cwd, &["rev-parse", "--abbrev-ref", "@{upstream}"]).await.is_ok();
        // Without a branch to compare with, whatever no branch of the remote has is ahead.
        let unpushed = match (&remote, upstream) {
            (_, true) => git(cwd, &["rev-list", "--count", "@{upstream}..HEAD"]).await,
            (Some(name), false) => git(cwd, &["rev-list", "--count", "HEAD", "--not", &format!("--remotes={name}")]).await,
            (None, false) => Err(String::new()),
        };
        let files = git(cwd, &["status", "--porcelain=v1", "-z", "--untracked-files=all"]).await.map(|out| parse_status(&out)).unwrap_or_default();
        // Nothing yet in a repository without a commit.
        let log = git(cwd, &["log", "-n", LISTED_COMMITS, "--format=%h%x1f%s%x1f%cr"]).await.unwrap_or_default();
        let commits = log
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(3, '\u{1f}');
                Some(Commit { hash: parts.next()?.to_string(), subject: parts.next()?.to_string(), when: parts.next()?.to_string() })
            })
            .collect();
        GitState {
            repo: true,
            branch,
            github: url.as_deref().and_then(github_slug),
            url: url.as_deref().map(without_credentials),
            remote,
            upstream,
            ahead: unpushed.ok().and_then(|count| count.trim().parse().ok()).unwrap_or(0),
            files,
            commits,
        }
    } else {
        GitState::default()
    }
}

/// Commits whatever is uncommitted under `message`, then pushes the branch to its remote.
pub async fn push(cwd: &Path, message: &str) -> Result<Pushed, String> {
    let before = state(cwd).await;
    if !before.repo {
        return Err("Not a git repository.".into());
    }
    let Some(branch) = before.branch else { return Err("No branch is checked out.".into()) };
    let Some(remote) = before.remote else { return Err("No remote to push to.".into()) };
    let mut commit = None;
    if !before.files.is_empty() {
        let message = message.trim();
        if message.is_empty() {
            return Err("No commit message.".into());
        }
        git(cwd, &["add", "--all"]).await?;
        git(cwd, &["commit", "--quiet", "--message", message]).await?;
        commit = git(cwd, &["rev-parse", "--short", "HEAD"]).await.ok().map(|hash| hash.trim().to_string());
    }
    if before.upstream && before.ahead == 0 && commit.is_none() {
        return Ok(Pushed { commit, remote, branch, pushed: false });
    }
    if before.upstream {
        git(cwd, &["push", "--quiet"]).await?;
    } else {
        git(cwd, &["push", "--quiet", "--set-upstream", &remote, &branch]).await?;
    }
    Ok(Pushed { commit, remote, branch, pushed: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remotes_are_shown_without_credentials_and_github_ones_by_owner_and_name() {
        assert_eq!(without_credentials("https://user:token@github.com/acme/logkit.git"), "https://github.com/acme/logkit.git");
        assert_eq!(without_credentials("git@github.com:acme/logkit.git"), "git@github.com:acme/logkit.git");
        for url in ["https://github.com/acme/logkit.git", "https://x-access-token:abc@github.com/acme/logkit", "git@github.com:acme/logkit.git", "ssh://git@github.com/acme/logkit.git", "https://GitHub.com/acme/logkit/"] {
            assert_eq!(github_slug(url).as_deref(), Some("acme/logkit"), "{url}");
        }
        for url in ["https://gitlab.com/acme/logkit.git", "/srv/git/logkit.git", "https://github.com/acme", "https://github.com/acme/logkit/tree/main"] {
            assert_eq!(github_slug(url), None, "{url}");
        }
    }

    #[test]
    fn status_entries_are_read_with_their_kind() {
        let files = parse_status(" M src/lib.rs\0?? notes/new file.md\0D  old.txt\0R  now.rs\0then.rs\0A  added.rs\0MM both.rs\0");
        let read: Vec<(&str, &str)> = files.iter().map(|file| (file.path.as_str(), file.status)).collect();
        assert_eq!(read, [("src/lib.rs", "modified"), ("notes/new file.md", "untracked"), ("old.txt", "deleted"), ("now.rs", "renamed"), ("added.rs", "added"), ("both.rs", "modified")]);
        assert!(parse_status("").is_empty());
    }

    #[tokio::test]
    async fn commits_what_changed_and_pushes_it_to_the_remote() {
        let base = std::env::temp_dir().join(format!("presage-git-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (origin, work) = (base.join("origin.git"), base.join("work"));
        std::fs::create_dir_all(&work).unwrap();
        git(&base, &["init", "--quiet", "--bare", "origin.git"]).await.unwrap();
        git(&work, &["init", "--quiet"]).await.unwrap();
        for args in [["symbolic-ref", "HEAD", "refs/heads/main"], ["config", "user.name", "Test"], ["config", "user.email", "test@example.com"], ["config", "commit.gpgsign", "false"]] {
            git(&work, &args).await.unwrap();
        }
        git(&work, &["remote", "add", "origin", origin.to_str().unwrap()]).await.unwrap();
        std::fs::write(work.join("a.txt"), "one\n").unwrap();

        let found = state(&work).await;
        assert_eq!((found.repo, found.branch.as_deref(), found.remote.as_deref(), found.upstream, found.github.as_deref()), (true, Some("main"), Some("origin"), false, None));
        assert_eq!(found.files, [ChangedFile { path: "a.txt".into(), status: "untracked" }]);

        // Uncommitted changes need a message; with one they are committed and pushed.
        assert_eq!(push(&work, "  ").await, Err("No commit message.".to_string()));
        let done = push(&work, "Add a").await.unwrap();
        assert!(done.pushed && done.commit.is_some());
        assert_eq!((done.remote.as_str(), done.branch.as_str()), ("origin", "main"));
        assert_eq!(git(&origin, &["log", "--format=%s", "main"]).await.unwrap().trim(), "Add a");

        // The branch now follows the remote's, and with nothing new nothing is pushed.
        let found = state(&work).await;
        assert!(found.upstream && found.ahead == 0 && found.files.is_empty());
        assert_eq!(push(&work, "").await.unwrap(), Pushed { commit: None, remote: "origin".into(), branch: "main".into(), pushed: false });

        // A commit made some other way is pushed as it is, with no message asked for.
        std::fs::write(work.join("a.txt"), "two\n").unwrap();
        assert_eq!(state(&work).await.files, [ChangedFile { path: "a.txt".into(), status: "modified" }]);
        git(&work, &["commit", "--quiet", "--all", "--message", "Change a"]).await.unwrap();
        let found = state(&work).await;
        assert_eq!(found.ahead, 1);
        assert_eq!(found.commits.iter().map(|commit| commit.subject.as_str()).collect::<Vec<_>>(), ["Change a", "Add a"]);
        assert!(found.commits.iter().all(|commit| !commit.hash.is_empty() && commit.when.ends_with("ago")));
        let done = push(&work, "").await.unwrap();
        assert!(done.pushed && done.commit.is_none());
        assert_eq!(git(&origin, &["log", "--format=%s", "main"]).await.unwrap().trim(), "Change a\nAdd a");

        std::fs::remove_dir_all(&base).unwrap();
    }
}
