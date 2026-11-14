//! The benchmark's tasks: small synthetic projects, embedded in the binary.
//!
//! Each has the workspace the agent starts in, held-out checks that are added only for
//! verification, and a reference solution. The solution is what lets `sage bench --check` prove
//! that a task fails as shipped and can be solved.

pub struct Task {
    pub id: &'static str,
    pub title: &'static str,
    /// What the agent is told.
    pub prompt: &'static str,
    /// Programs the task needs on PATH.
    pub needs: &'static [&'static str],
    /// The workspace as the agent finds it: (relative path, content).
    pub files: &'static [(&'static str, &'static str)],
    /// The check the task statement points at. The scripted stand-in runs it.
    pub check: &'static str,
    /// Workspace files the agent is told to leave alone; put back before verification.
    pub restore: &'static [&'static str],
    /// Held-out checks, written only for verification.
    pub hidden: &'static [(&'static str, &'static str)],
    /// Run in the workspace once the agent is done. Exit 0 means solved.
    pub verify: &'static str,
    /// The files of a correct solution.
    pub solution: &'static [(&'static str, &'static str)],
    /// The built-in skill this task exercises, or "" for none.
    pub skill: &'static str,
}

/// `(path, content)` pairs for files under `bench/<dir>/`, embedded at compile time.
macro_rules! embed {
    ($dir:literal: $($path:literal),+ $(,)?) => {
        &[$(($path, include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/bench/", $dir, "/", $path)))),+]
    };
}

pub mod hard;
pub mod skilled;

/// The task sets `--tasks` accepts by name.
pub const SETS: &[(&str, &[Task])] = &[
    ("easy", TASKS),
    ("hard", hard::TASKS),
    ("skill", skilled::TASKS),
];

pub fn all() -> impl Iterator<Item = &'static Task> {
    SETS.iter().flat_map(|(_, tasks)| tasks.iter())
}

pub fn find(id: &str) -> Option<&'static Task> {
    all().find(|t| t.id == id)
}

fn keep(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// The text files under `dir` as `(relative path, content)`, sorted. A directory that is not
/// there holds none.
fn files_under(dir: &std::path::Path) -> Result<&'static [(&'static str, &'static str)], String> {
    fn walk(
        root: &std::path::Path,
        dir: &std::path::Path,
        out: &mut Vec<(&'static str, &'static str)>,
    ) -> Result<(), String> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Ok(());
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out)?;
                continue;
            }
            let content = std::fs::read_to_string(&path)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let relative = path.strip_prefix(root).unwrap_or(&path);
            out.push((
                keep(relative.to_string_lossy().replace('\\', "/")),
                keep(content),
            ));
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(dir, dir, &mut files)?;
    files.sort();
    Ok(Box::leak(files.into_boxed_slice()))
}

/// Tasks kept outside the binary, for a slice of a public benchmark: one directory each, laid
/// out like the built-in ones (`workspace/`, `hidden/`, `solution/`), with a `task.json` that
/// gives `prompt` and `verify` and may give `title`, `needs`, `check`, `restore` and `skill`.
/// They live as long as the program does.
pub fn load_dir(dir: &std::path::Path) -> Result<Vec<&'static Task>, String> {
    let mut ids: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .filter(|entry| entry.path().join("task.json").is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    ids.sort();
    if ids.is_empty() {
        return Err(format!(
            "{} holds no tasks: each is a directory with a task.json",
            dir.display()
        ));
    }
    let mut tasks = Vec::new();
    for id in ids {
        let root = dir.join(&id);
        let about: serde_json::Value = std::fs::read_to_string(root.join("task.json"))
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()))
            .map_err(|e| format!("{id}/task.json: {e}"))?;
        let text = |key: &str| about[key].as_str().unwrap_or_default().to_string();
        let list = |key: &str| -> &'static [&'static str] {
            let items: Vec<&'static str> = about[key]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                .unwrap_or_default()
                .into_iter()
                .map(|s| keep(s.to_string()))
                .collect();
            Box::leak(items.into_boxed_slice())
        };
        if text("prompt").is_empty() || text("verify").is_empty() {
            return Err(format!("{id}/task.json: `prompt` and `verify` are needed"));
        }
        tasks.push(&*Box::leak(Box::new(Task {
            title: keep(text("title")),
            prompt: keep(text("prompt")),
            needs: list("needs"),
            files: files_under(&root.join("workspace"))?,
            check: keep(text("check")),
            restore: list("restore"),
            hidden: files_under(&root.join("hidden"))?,
            verify: keep(text("verify")),
            solution: files_under(&root.join("solution"))?,
            skill: keep(text("skill")),
            id: keep(id),
        })));
    }
    Ok(tasks)
}

/// The original set: small single-file tasks that every variant solves.
pub const TASKS: &[Task] = &[
    Task {
        id: "pager",
        title: "two off-by-one errors in a pagination helper",
        prompt: "`python3 -m unittest` fails in this project. Find and fix the bugs in `pager.py`. Do not edit the tests.",
        needs: &["python3"],
        files: embed!("pager/workspace": "pager.py", "test_pager.py"),
        check: "python3 -m unittest",
        restore: &["test_pager.py"],
        hidden: embed!("pager/hidden": "test_pager_more.py"),
        verify: "python3 -m unittest test_pager test_pager_more",
        solution: embed!("pager/solution": "pager.py"),
        skill: "",
    },
    Task {
        id: "inventory",
        title: "a crash that hides two wrong results behind it",
        prompt: "The stock report is broken: `python3 -m unittest` fails. Fix `inventory.py` so that the tests pass. Do not edit the tests or `stock.csv`.",
        needs: &["python3"],
        files: embed!("inventory/workspace": "inventory.py", "stock.csv", "test_inventory.py"),
        check: "python3 -m unittest",
        restore: &["test_inventory.py", "stock.csv"],
        hidden: embed!("inventory/hidden": "test_inventory_more.py"),
        verify: "python3 -m unittest test_inventory test_inventory_more",
        solution: embed!("inventory/solution": "inventory.py"),
        skill: "",
    },
    Task {
        id: "slug",
        title: "a fix whose obvious version breaks a test that passes today",
        prompt: "`python3 -m unittest` fails. Fix `slugify` in `slug.py` so that it does what its docstring says and all tests pass. Do not edit the tests.",
        needs: &["python3"],
        files: embed!("slug/workspace": "slug.py", "test_slug.py"),
        check: "python3 -m unittest",
        restore: &["test_slug.py"],
        hidden: embed!("slug/hidden": "test_slug_more.py"),
        verify: "python3 -m unittest test_slug test_slug_more",
        solution: embed!("slug/solution": "slug.py"),
        skill: "",
    },
    Task {
        id: "tariff",
        title: "two boundary bugs under a check that prints 140 lines",
        prompt: "`sh run_checks.sh` reports failing cases. Fix `tariff.py` so that every case passes. Do not edit `checks.py` or `run_checks.sh`.",
        needs: &["python3", "sh"],
        files: embed!("tariff/workspace": "tariff.py", "checks.py", "run_checks.sh"),
        check: "sh run_checks.sh",
        restore: &["checks.py", "run_checks.sh"],
        hidden: embed!("tariff/hidden": "checks_more.py"),
        verify: "sh run_checks.sh > /dev/null && python3 checks_more.py",
        solution: embed!("tariff/solution": "tariff.py"),
        skill: "",
    },
    Task {
        id: "textstats",
        title: "a function to write from a README specification",
        prompt: "Implement `top_words` in `textstats.py` as specified in `README.md`, so that `python3 -m unittest` passes. Do not edit the tests.",
        needs: &["python3"],
        files: embed!("textstats/workspace": "textstats.py", "README.md", "test_textstats.py"),
        check: "python3 -m unittest",
        restore: &["test_textstats.py"],
        hidden: embed!("textstats/hidden": "test_textstats_more.py"),
        verify: "python3 -m unittest test_textstats test_textstats_more",
        solution: embed!("textstats/solution": "textstats.py"),
        skill: "",
    },
    Task {
        id: "roman",
        title: "a module to write with no tests given",
        prompt: "Create `roman.py` with two functions. `to_roman(n)` turns an integer from 1 to 3999 into a Roman numeral in standard subtractive notation (1994 -> \"MCMXCIV\"). `from_roman(s)` turns such a numeral back into the integer. Both raise `ValueError` for anything else: numbers outside 1..3999, and strings that are not correctly formed numerals (\"IIII\", \"VX\", \"IC\", \"\"). The project has no tests; check your work before you finish.",
        needs: &["python3"],
        files: embed!("roman/workspace": "README.md"),
        check: "python3 -c \"import roman; assert roman.to_roman(1994) == 'MCMXCIV' and roman.from_roman('XIV') == 14\"",
        restore: &[],
        hidden: embed!("roman/hidden": "test_roman.py"),
        verify: "python3 -m unittest test_roman",
        solution: embed!("roman/solution": "roman.py"),
        skill: "",
    },
    Task {
        id: "report",
        title: "a shell and awk report with three faults",
        prompt: "`sh check.sh` fails. Fix `summary.sh` so that it passes, for any orders file in the same format. Do not edit `check.sh`, `orders.csv` or `expected.txt`.",
        needs: &["sh", "awk", "diff"],
        files: embed!("report/workspace": "summary.sh", "check.sh", "orders.csv", "expected.txt"),
        check: "sh check.sh",
        restore: &["check.sh", "orders.csv", "expected.txt"],
        hidden: embed!("report/hidden": "orders2.csv", "expected2.txt"),
        verify: "sh check.sh && sh summary.sh orders2.csv | diff - expected2.txt",
        solution: embed!("report/solution": "summary.sh"),
        skill: "",
    },
];
