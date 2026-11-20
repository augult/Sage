//! `sage` in a terminal: a conversation with `/` commands.
//!
//! A line editor reads a message or a command. A message goes to the current session, or opens
//! one, and its run is followed here until it ends: Ctrl-C stops it, an approval is one key, and
//! a question the model asks is answered with the next message (a number picks a choice).

use super::{Paint, Printer, k, parse_run, pct};
use crossterm::event::{self as term, Event as TermEvent, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::{execute, terminal};
use nu_ansi_term::{Color, Style};
use reedline::{
    ColumnarMenu, Completer, CompletionResult, DefaultHinter, EditCommand, EditMode, Emacs,
    FileBackedHistory, MenuBuilder, Prompt, PromptEditMode, PromptHistorySearch,
    PromptHistorySearchStatus, Reedline, ReedlineEvent, ReedlineMenu, ReedlineRawEvent, Signal,
    Span, StatusLine, Suggestion, ValidationResult, Validator, default_emacs_keybindings,
};
use sage_core::events::{Channel, Event, EventKind, FinishStatus, Stats};
use sage_core::harness::{Changes, Harness, SessionMeta, SessionStatus, StartRequest};
use sage_core::util::{fmt_duration, one_line};
use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Sage green, from the 256-colour palette.
const ACCENT: u8 = 108;
const MENU: &str = "complete";

struct Command {
    name: &'static str,
    args: &'static str,
    about: &'static str,
}

const COMMANDS: &[Command] = &[
    Command {
        name: "help",
        args: "",
        about: "commands and keys",
    },
    Command {
        name: "new",
        args: "",
        about: "start a new session",
    },
    Command {
        name: "clear",
        args: "",
        about: "clear the screen and start a new session",
    },
    Command {
        name: "sessions",
        args: "",
        about: "list recent sessions",
    },
    Command {
        name: "resume",
        args: "<id>",
        about: "continue a stored session",
    },
    Command {
        name: "rename",
        args: "<title>",
        about: "rename this session",
    },
    Command {
        name: "connect",
        args: "<provider|url>",
        about: "add an API key, or an endpoint of your own",
    },
    Command {
        name: "provider",
        args: "[id]",
        about: "show or switch the provider",
    },
    Command {
        name: "model",
        args: "[name]",
        about: "show or switch the model",
    },
    Command {
        name: "effort",
        args: "[auto|low|high|max]",
        about: "how hard the model thinks",
    },
    Command {
        name: "permissions",
        args: "[auto|ask]",
        about: "act freely, or ask before every change",
    },
    Command {
        name: "skills",
        args: "[name]",
        about: "list skills, or switch one on",
    },
    Command {
        name: "goal",
        args: "[condition|clear]",
        about: "keep working until a check agrees",
    },
    Command {
        name: "init",
        args: "",
        about: "write an AGENTS.md for this project",
    },
    Command {
        name: "cd",
        args: "<dir>",
        about: "folder for new sessions",
    },
    Command {
        name: "diff",
        args: "",
        about: "what changed in the folder (git)",
    },
    Command {
        name: "status",
        args: "",
        about: "provider, model, folder and session",
    },
    Command {
        name: "cost",
        args: "",
        about: "tokens, cost and time of this session",
    },
    Command {
        name: "verbose",
        args: "",
        about: "show reasoning and every observation",
    },
    Command {
        name: "exit",
        args: "",
        about: "quit (also Ctrl-D)",
    },
];

const INIT: &str = "Write an AGENTS.md at the root of this project for coding agents that will work on it: \
what the project is, how to build, test and lint it, and the conventions worth knowing. Keep it short \
and accurate: confirm each command in the code and configuration, and run the cheap ones. If an \
AGENTS.md is already there, improve it instead.";

/// What the prompt, the footer and the completions show. Rebuilt before each line is read.
#[derive(Default)]
struct View {
    cwd: PathBuf,
    /// The line as the completer last saw it: what Enter decides by while a menu is open.
    typed: String,
    footer: String,
    providers: Vec<(String, String)>,
    models: Vec<String>,
    skills: Vec<(String, String)>,
    sessions: Vec<(String, String)>,
}

type Shared = Arc<Mutex<View>>;

enum Flow {
    Go,
    Quit,
}

enum Answer {
    Yes,
    No,
    Stop,
}

/// `~` for the home directory, as people write paths.
fn tilde(path: &Path) -> String {
    let home = sage_core::util::home_dir();
    match path.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn width() -> usize {
    terminal::size().map_or(80, |(w, _)| w as usize).max(20)
}

/// `~/x`, relative or absolute: a path as typed, against `cwd`.
fn expand(cwd: &Path, typed: &str) -> PathBuf {
    if typed == "~" {
        sage_core::util::home_dir()
    } else if let Some(rest) = typed.strip_prefix("~/") {
        sage_core::util::home_dir().join(rest)
    } else {
        cwd.join(typed)
    }
}

/// What `@path` words in a message name that are files.
fn attachments(text: &str, cwd: &Path) -> Vec<String> {
    text.split_whitespace()
        .filter_map(|word| word.strip_prefix('@'))
        .map(|path| path.trim_end_matches([',', '.', ';', ':', ')', '?', '!']))
        .filter(|path| !path.is_empty())
        .map(|path| expand(cwd, path))
        .filter(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

/// A finished run's state, in a word or two.
fn state_word(m: &SessionMeta) -> &'static str {
    match (m.status, m.outcome, m.verified) {
        (SessionStatus::Running, _, _) => "running",
        (_, Some(FinishStatus::Done), Some(true)) => "done ✓",
        (_, Some(FinishStatus::Done), _) => "done",
        (_, Some(FinishStatus::Yielded), _) => "waiting",
        (_, Some(FinishStatus::Stopped), _) => "stopped",
        (_, Some(FinishStatus::Budget), _) => "out of budget",
        (_, Some(FinishStatus::Incomplete), _) => "incomplete",
        (_, Some(FinishStatus::Error), _) => "failed",
        _ => "–",
    }
}

fn suggestion(
    value: String,
    description: String,
    span: Span,
    append_whitespace: bool,
) -> Suggestion {
    Suggestion {
        value,
        description: (!description.is_empty()).then_some(description),
        span,
        append_whitespace,
        ..Default::default()
    }
}

/// Completions for `/commands`, their arguments and `@files`.
struct Complete {
    view: Shared,
}

impl Complete {
    fn paths(
        &self,
        typed: &str,
        start: usize,
        end: usize,
        prefix: &str,
        dirs_only: bool,
    ) -> Vec<Suggestion> {
        let cwd = self.view.lock().unwrap().cwd.clone();
        let (dir, stem) = match typed.rfind('/') {
            Some(at) => (&typed[..=at], &typed[at + 1..]),
            None => ("", typed),
        };
        let Ok(entries) = std::fs::read_dir(expand(&cwd, if dir.is_empty() { "." } else { dir }))
        else {
            return Vec::new();
        };
        let mut found: Vec<(bool, String)> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let is_dir = e.file_type().ok()?.is_dir();
                (name.starts_with(stem)
                    && (stem.starts_with('.') || !name.starts_with('.'))
                    && (is_dir || !dirs_only))
                    .then_some((is_dir, name))
            })
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        found
            .into_iter()
            .take(40)
            .map(|(is_dir, name)| {
                let slash = if is_dir { "/" } else { "" };
                suggestion(
                    format!("{prefix}{dir}{name}{slash}"),
                    String::new(),
                    Span::new(start, end),
                    !is_dir,
                )
            })
            .collect()
    }
}

impl Completer for Complete {
    fn complete(&mut self, line: &str, pos: usize) -> CompletionResult {
        self.view.lock().unwrap().typed = line.to_string();
        let before = &line[..pos.min(line.len())];
        let word_start = before.rfind(char::is_whitespace).map_or(0, |at| at + 1);
        let word = &before[word_start..];
        if let Some(typed) = word.strip_prefix('@') {
            return CompletionResult::fresh(self.paths(typed, word_start, pos, "@", false));
        }
        let Some(rest) = before.strip_prefix('/') else {
            return CompletionResult::fresh(Vec::<Suggestion>::new());
        };
        let Some(space) = rest.find(' ') else {
            let found: Vec<Suggestion> = COMMANDS
                .iter()
                .filter(|c| c.name.starts_with(rest))
                .map(|c| {
                    let shown = if c.args.is_empty() {
                        c.about.to_string()
                    } else {
                        format!("{}  {}", c.args, c.about)
                    };
                    suggestion(
                        format!("/{}", c.name),
                        shown,
                        Span::new(0, pos),
                        !c.args.is_empty(),
                    )
                })
                .collect();
            return CompletionResult::fresh(found);
        };
        let command = &rest[..space];
        let arg = rest[space + 1..].trim_start();
        let start = pos - arg.len();
        if command == "cd" {
            return CompletionResult::fresh(self.paths(arg, start, pos, "", true));
        }
        let view = self.view.lock().unwrap();
        let fixed = |values: &[(&str, &str)]| -> Vec<(String, String)> {
            values
                .iter()
                .map(|(v, d)| (v.to_string(), d.to_string()))
                .collect()
        };
        let candidates: Vec<(String, String)> = match command {
            "provider" | "connect" => view.providers.clone(),
            "model" => view
                .models
                .iter()
                .map(|m| (m.clone(), String::new()))
                .collect(),
            "skills" => view.skills.clone(),
            "resume" => view.sessions.clone(),
            "effort" => fixed(&[
                (
                    "auto",
                    "recommended: starts low, rises by itself when the task needs it",
                ),
                ("low", "fast and cheap"),
                ("high", "thinks more"),
                ("max", "thinks most"),
            ]),
            "mode" => fixed(&[
                ("sage", "predictions, scripts and watches (default)"),
                ("default", "the conventional loop of other harnesses"),
            ]),
            "permissions" => fixed(&[
                ("auto", "edit and run commands without asking"),
                ("ask", "ask before every edit and command"),
            ]),
            "goal" => fixed(&[("clear", "drop the goal")]),
            _ => Vec::new(),
        };
        let found: Vec<Suggestion> = candidates
            .into_iter()
            .filter(|(value, _)| value.starts_with(arg))
            .map(|(value, about)| suggestion(value, about, Span::new(start, pos), false))
            .collect();
        CompletionResult::fresh(found)
    }
}

struct Line;

impl Prompt for Line {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }
    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }
    fn render_prompt_indicator(&self, _mode: PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed("› ")
    }
    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Borrowed("  ")
    }
    fn render_prompt_history_search_indicator(&self, search: PromptHistorySearch) -> Cow<'_, str> {
        let failing = match search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "no match: ",
        };
        Cow::Owned(format!("({failing}search {}) ", search.term))
    }
    fn get_indicator_color(&self) -> Color {
        Color::Fixed(ACCENT)
    }
}

/// Under the line being typed: a rule, and where the next message goes.
struct Footer {
    view: Shared,
    color: bool,
}

impl StatusLine for Footer {
    fn status(&mut self, _line: &str, _pos: usize, columns: u16, _ansi: bool) -> String {
        let columns = (columns as usize).max(20);
        let rule = "─".repeat(columns - 1);
        let info = one_line(&self.view.lock().unwrap().footer, columns.saturating_sub(4));
        if self.color {
            format!("\x1b[2m{rule}\n  {info}\x1b[0m")
        } else {
            format!("{rule}\n  {info}")
        }
    }
}

/// What Enter does, by the line the completer last saw. On a command it runs the entry the menu
/// has picked (`/sta` runs `/status`), or the line as typed where the argument is still empty
/// (`/connect ` lists providers rather than picking the first). On `@file` it only takes the
/// file, and the message goes on.
fn enter_for(line: &str) -> ReedlineEvent {
    let last_word = line.rsplit(char::is_whitespace).next().unwrap_or("");
    if !line.starts_with('/') || last_word.starts_with('@') {
        return ReedlineEvent::Enter;
    }
    match line.split_once(' ') {
        Some((_, arg)) if arg.trim().is_empty() => {
            ReedlineEvent::Multiple(vec![ReedlineEvent::Esc, ReedlineEvent::Submit])
        }
        _ => ReedlineEvent::Multiple(vec![ReedlineEvent::Enter, ReedlineEvent::Submit]),
    }
}

/// Emacs keys, with Enter as `enter_for` says.
struct Keys {
    inner: Emacs,
    view: Shared,
}

impl EditMode for Keys {
    fn parse_event(&mut self, event: ReedlineRawEvent) -> ReedlineEvent {
        match self.inner.parse_event(event) {
            ReedlineEvent::Enter => enter_for(&self.view.lock().unwrap().typed),
            other => other,
        }
    }

    fn edit_mode(&self) -> PromptEditMode {
        self.inner.edit_mode()
    }
}

/// A line that ends in `\` goes on to the next.
struct Continues;

impl Validator for Continues {
    fn validate(&self, line: &str) -> ValidationResult {
        if line.ends_with('\\') {
            ValidationResult::Incomplete
        } else {
            ValidationResult::Complete
        }
    }
}

fn editor(view: &Shared, color: bool, history: PathBuf) -> Reedline {
    let mut keys = default_emacs_keybindings();
    keys.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu(MENU.into()),
            ReedlineEvent::MenuNext,
        ]),
    );
    keys.add_binding(
        KeyModifiers::SHIFT,
        KeyCode::BackTab,
        ReedlineEvent::MenuPrevious,
    );
    // `/` and `@` open the menu as they are typed. It stays empty, and closed, where neither
    // starts a command or a file name.
    for c in ['/', '@'] {
        for modifiers in [KeyModifiers::NONE, KeyModifiers::SHIFT] {
            keys.add_binding(
                modifiers,
                KeyCode::Char(c),
                ReedlineEvent::Multiple(vec![
                    ReedlineEvent::Edit(vec![EditCommand::InsertChar(c)]),
                    ReedlineEvent::Menu(MENU.into()),
                ]),
            );
        }
    }
    for modifiers in [KeyModifiers::ALT, KeyModifiers::SHIFT] {
        keys.add_binding(
            modifiers,
            KeyCode::Enter,
            ReedlineEvent::Edit(vec![EditCommand::InsertNewline]),
        );
    }
    let menu = ColumnarMenu::default()
        .with_name(MENU)
        .with_marker("› ")
        .with_text_style(Style::new())
        .with_selected_text_style(Style::new().fg(Color::Fixed(16)).on(Color::Fixed(ACCENT)))
        .with_description_text_style(Style::new().dimmed())
        .with_match_text_style(Style::new().bold())
        .with_selected_match_text_style(
            Style::new()
                .bold()
                .fg(Color::Fixed(16))
                .on(Color::Fixed(ACCENT)),
        );
    if let Some(dir) = history.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // What was typed can be as private as the keys beside it: readable by its owner alone.
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&history);
        let _ = std::fs::set_permissions(&history, std::fs::Permissions::from_mode(0o600));
    }
    let history: Box<FileBackedHistory> = match FileBackedHistory::with_file(1000, history) {
        Ok(file) => Box::new(file),
        Err(_) => Box::new(FileBackedHistory::new(1000).expect("an in-memory history")),
    };
    Reedline::create()
        .with_edit_mode(Box::new(Keys {
            inner: Emacs::new(keys),
            view: view.clone(),
        }))
        .with_completer(Box::new(Complete { view: view.clone() }))
        .with_menu(ReedlineMenu::EngineCompleter(Box::new(menu)))
        .with_history(history)
        .with_history_exclusion_prefix(Some(" ".into()))
        .with_hinter(Box::new(
            DefaultHinter::default().with_style(Style::new().dimmed()),
        ))
        .with_validator(Box::new(Continues))
        .with_status_line(Box::new(Footer {
            view: view.clone(),
            color,
        }))
        .with_ansi_colors(color)
        .use_bracketed_paste(true)
}

/// One key: y, n, or Ctrl-C to stop the run. Enter is no.
fn answer_key() -> Answer {
    let _ = std::io::stderr().flush();
    if terminal::enable_raw_mode().is_err() {
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        return if line.trim().eq_ignore_ascii_case("y") {
            Answer::Yes
        } else {
            Answer::No
        };
    }
    let answer = loop {
        let Ok(TermEvent::Key(key)) = term::read() else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                break Answer::Stop;
            }
            KeyCode::Char('y' | 'Y') => break Answer::Yes,
            KeyCode::Char('n' | 'N') | KeyCode::Enter | KeyCode::Esc => break Answer::No,
            _ => {}
        }
    };
    let _ = terminal::disable_raw_mode();
    answer
}

/// A secret typed or pasted without showing it. `None` when cancelled.
fn read_secret() -> Option<String> {
    let mut err = std::io::stderr();
    let _ = err.flush();
    if !std::io::stdin().is_terminal() || terminal::enable_raw_mode().is_err() {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).ok()?;
        return Some(line.trim().to_string());
    }
    let _ = execute!(err, crossterm::event::EnableBracketedPaste);
    let mut secret = String::new();
    let done = loop {
        match term::read() {
            Ok(TermEvent::Paste(text)) => {
                let text = text.trim();
                secret.push_str(text);
                let _ = write!(err, "{}", "•".repeat(text.chars().count()));
            }
            Ok(TermEvent::Key(key)) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Enter => break true,
                KeyCode::Esc => break false,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break false,
                KeyCode::Backspace => {
                    if secret.pop().is_some() {
                        let _ = write!(err, "\x08 \x08");
                    }
                }
                KeyCode::Char(c) => {
                    secret.push(c);
                    let _ = write!(err, "•");
                }
                _ => {}
            },
            Ok(_) => {}
            Err(_) => break false,
        }
        let _ = err.flush();
    };
    let _ = execute!(err, crossterm::event::DisableBracketedPaste);
    let _ = terminal::disable_raw_mode();
    eprintln!();
    done.then(|| secret.trim().to_string())
}

/// The line under a run in progress: what it is doing and for how long.
struct Spinner {
    on: bool,
    shown: bool,
    frame: usize,
    since: Instant,
    doing: String,
}

impl Spinner {
    const FRAMES: [&'static str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

    fn draw(&mut self) {
        if !self.on {
            return;
        }
        let frame = Self::FRAMES[self.frame % Self::FRAMES.len()];
        self.frame += 1;
        let secs = self.since.elapsed().as_secs();
        let text = one_line(
            &format!("{} · {secs}s · ctrl-c stops", self.doing),
            width().saturating_sub(6),
        );
        eprint!("\r\x1b[2K  \x1b[38;5;{ACCENT}m{frame}\x1b[0m \x1b[2m{text}\x1b[0m");
        let _ = std::io::stderr().flush();
        self.shown = true;
    }

    fn clear(&mut self) {
        if self.shown {
            eprint!("\r\x1b[2K");
            let _ = std::io::stderr().flush();
            self.shown = false;
        }
    }
}

struct Chat {
    h: Harness,
    paint: Paint,
    color: bool,
    view: Shared,
    cwd: PathBuf,
    /// The session messages go to; none until the first message opens one.
    session: Option<String>,
    /// Parsed launch options, also used for new sessions opened later in this chat.
    start: StartRequest,
    /// Effective permission for the selected session.
    /// `auto` or `ask`.
    autonomy: String,
    /// What changes about the session with the next message to it.
    pending: Changes,
    verbose: bool,
    /// The choices of the question the last run ended on, for an answer by number.
    choices: Vec<String>,
    /// Ctrl-C pressed at an empty prompt: a second one quits.
    quitting: bool,
}

/// An explicit request to ask carries across session switches. Otherwise keep the saved mode.
fn resume_permission<'a>(requested: Option<&str>, events: &'a [Event]) -> &'a str {
    if requested == Some("ask") {
        return "ask";
    }
    events
        .iter()
        .rev()
        .find_map(|event| match &event.kind {
            EventKind::SessionStart { autonomy, .. }
            | EventKind::Settings {
                autonomy: Some(autonomy),
                ..
            } => Some(autonomy.as_str()),
            _ => None,
        })
        .filter(|mode| matches!(*mode, "auto" | "ask"))
        .unwrap_or("ask")
}

impl Chat {
    fn note(&self, text: &str) {
        eprintln!("  {}", self.paint.dim(text));
    }

    fn ok(&self, text: &str) {
        eprintln!(
            "  {} {text}",
            self.paint.wrap(&format!("38;5;{ACCENT}"), "✓")
        );
    }

    fn fail(&self, text: &str) {
        eprintln!("  {} {text}", self.paint.red("✗"));
    }

    fn accent(&self, text: &str) -> String {
        self.paint.wrap(&format!("38;5;{ACCENT}"), text)
    }

    fn meta(&self) -> Option<SessionMeta> {
        self.session.as_deref().and_then(|id| self.h.meta(id).ok())
    }

    /// The provider and model the next message goes to, as far as is known before it is sent.
    fn target(&self) -> (Option<String>, Option<String>) {
        if let Some(meta) = self.meta() {
            let provider = self.pending.provider.clone().unwrap_or(meta.provider);
            let model = self
                .pending
                .model
                .clone()
                .or_else(|| self.pending.provider.is_none().then_some(meta.model));
            return (Some(provider), model);
        }
        let state = self.h.state();
        let provider = self.start.provider.clone().or(state.provider.clone());
        let model = self.start.model.clone().or_else(|| {
            if self.start.provider.is_none() || self.start.provider == state.provider {
                state.model.clone()
            } else {
                state
                    .providers
                    .iter()
                    .find(|p| Some(&p.id) == provider.as_ref())
                    .and_then(|p| p.models.first().cloned())
            }
        });
        (provider, model)
    }

    fn effort_label(&self) -> String {
        let effort = self
            .pending
            .effort
            .clone()
            .or_else(|| self.meta().map(|m| m.effort))
            .or(self.start.effort.clone())
            .unwrap_or_else(|| self.h.state().effort);
        if effort.starts_with("ratchet") {
            "auto".into()
        } else {
            effort
        }
    }

    fn refresh(&self) {
        let state = self.h.state();
        let (provider, model) = self.target();
        let mut parts = vec![tilde(&self.cwd)];
        match (&provider, &model) {
            (Some(p), Some(m)) => parts.push(format!("{m} via {p}")),
            (Some(p), None) => parts.push(format!("via {p}")),
            _ => parts.push("no key yet: /connect".into()),
        }
        parts.push(format!("effort {}", self.effort_label()));
        if self.autonomy == "ask" {
            parts.push("asks first".into());
        }
        if let Some(meta) = self.meta() {
            parts.push(format!("session {}", short(&meta.id)));
        } else {
            parts.push("new session".into());
        }
        let models = state
            .providers
            .iter()
            .find(|p| Some(&p.id) == provider.as_ref())
            .map(|p| p.models.clone())
            .unwrap_or_default();
        let providers = state
            .providers
            .iter()
            .map(|p| {
                let key = if p.has_key { "key set" } else { "no key" };
                (p.id.clone(), format!("{} · {key}", p.name))
            })
            .collect();
        let skills = self
            .h
            .skills(Some(&self.cwd.to_string_lossy()))
            .into_iter()
            .map(|s| (s.name, s.description))
            .collect();
        let sessions = self
            .h
            .list()
            .into_iter()
            .filter(|m| m.why_closed.is_none())
            .take(20)
            .map(|m| (m.id.clone(), one_line(&m.title, 50)))
            .collect();
        *self.view.lock().unwrap() = View {
            cwd: self.cwd.clone(),
            typed: String::new(),
            footer: parts.join(" · "),
            providers,
            models,
            skills,
            sessions,
        };
    }

    fn banner(&self) {
        let state = self.h.state();
        let (provider, model) = self.target();
        let inner = width().clamp(44, 78) - 4;
        let rows: Vec<(String, String)> = vec![
            (
                format!("◆ Sage {}", state.version),
                self.paint
                    .bold(&self.accent(&format!("◆ Sage {}", state.version))),
            ),
            (
                "  an agent harness for cheap reasoning models".into(),
                self.paint
                    .dim("  an agent harness for cheap reasoning models"),
            ),
            (String::new(), String::new()),
            {
                let text = match (&provider, &model) {
                    (Some(p), Some(m)) => format!("{m} via {p}"),
                    (Some(p), None) => format!("via {p}"),
                    _ => "no provider has a key yet".into(),
                };
                (
                    format!("  model   {text}"),
                    format!("  {}   {text}", self.paint.dim("model")),
                )
            },
            {
                // The mode is a developer option: shown only when one was set that is not the default.
                let mode = match self.start.mode.as_deref() {
                    Some(mode) if mode != "sage" => format!(" · mode {mode}"),
                    _ => String::new(),
                };
                let text = format!(
                    "{}{mode} · permissions {}",
                    self.effort_label(),
                    self.autonomy
                );
                (
                    format!("  effort  {text}"),
                    format!("  {}  {text}", self.paint.dim("effort")),
                )
            },
            {
                let text = tilde(&self.cwd);
                (
                    format!("  folder  {text}"),
                    format!("  {}  {text}", self.paint.dim("folder")),
                )
            },
        ];
        let edge = |s: &str| self.paint.dim(s);
        eprintln!("{}", edge(&format!("╭{}╮", "─".repeat(inner + 2))));
        for (plain, painted) in &rows {
            // Too long for the box (a deep folder): cut the plain text, as cutting the painted
            // one could split a colour code.
            let (painted, plain) = if plain.chars().count() > inner {
                let cut = one_line(plain, inner - 1);
                (cut.clone(), cut.chars().count())
            } else {
                (painted.clone(), plain.chars().count())
            };
            eprintln!(
                "{} {painted}{} {}",
                edge("│"),
                " ".repeat(inner.saturating_sub(plain)),
                edge("│")
            );
        }
        eprintln!("{}", edge(&format!("╰{}╯", "─".repeat(inner + 2))));
        if provider.is_none() {
            eprintln!(
                "  {} {} adds a key, for example {}",
                self.paint.yellow("!"),
                self.accent("/connect <provider>"),
                self.accent("/connect openrouter"),
            );
        }
        eprintln!(
            "  {}",
            self.paint
                .dim("Type a task, or / for commands · @ attaches a file · ! runs a shell command")
        );
        eprintln!(
            "  {}",
            self.paint
                .dim("Ctrl-C stops a run · Ctrl-D quits · a line ending in \\ continues")
        );
    }

    fn help(&self) {
        eprintln!();
        for c in COMMANDS {
            let name = format!("/{} {}", c.name, c.args);
            eprintln!(
                "  {:<30} {}",
                self.accent(&format!("{name:<30}")),
                self.paint.dim(c.about)
            );
        }
        eprintln!();
        for (key, about) in [
            ("@path", "attach a file to the message (Tab completes)"),
            (
                "!command",
                "run a shell command yourself; the model does not see it",
            ),
            ("Tab", "complete a command, an argument or a path"),
            ("Ctrl-R", "search earlier messages"),
            ("Alt-Enter, \\", "new line"),
            ("Ctrl-C", "stop a run; twice at an empty prompt quits"),
            ("Ctrl-D", "quit"),
        ] {
            eprintln!(
                "  {}  {}",
                self.accent(&format!("{key:<29}")),
                self.paint.dim(about)
            );
        }
        eprintln!();
    }

    /// Runs one line: a command, a shell command, or a message.
    async fn line(&mut self, text: &str) -> Flow {
        if let Some(command) = text.strip_prefix('!') {
            self.shell(command.trim());
            return Flow::Go;
        }
        if text.starts_with('/') && !text.starts_with("//") {
            return self.command(text).await;
        }
        self.message(text).await;
        Flow::Go
    }

    async fn command(&mut self, text: &str) -> Flow {
        let body = text.trim_start_matches('/');
        let (name, arg) = body.split_once(char::is_whitespace).unwrap_or((body, ""));
        let arg = arg.trim();
        match name {
            "help" | "?" | "commands" => self.help(),
            "exit" | "quit" | "q" => return Flow::Quit,
            "new" | "reset" => {
                self.session = None;
                self.pending = Changes::default();
                self.choices.clear();
                self.ok("new session: your next message starts it");
            }
            "clear" => {
                eprint!("\x1b[2J\x1b[3J\x1b[H");
                self.session = None;
                self.pending = Changes::default();
                self.choices.clear();
                self.banner();
            }
            "sessions" | "history" => self.sessions(),
            "resume" | "continue" => self.resume(arg),
            "rename" => match (&self.session, arg) {
                (None, _) => self.fail("no session yet: the first message starts one"),
                (Some(_), "") => self.fail("usage: /rename <title>"),
                (Some(id), title) => match self.h.rename(id, title) {
                    Ok(meta) => self.ok(&format!("renamed: {}", meta.title)),
                    Err(e) => self.fail(&e),
                },
            },
            "connect" | "login" | "key" => self.connect(arg),
            "provider" | "providers" => self.provider(arg),
            "model" | "models" => self.model(arg),
            "effort" => self.effort(arg),
            // Not in the menu or /help: a developer option (docs/DESIGN.md, "Developer commands").
            "mode" => self.mode(arg),
            "permissions" | "autonomy" => self.permissions(arg),
            "skills" | "skill" => self.skills(arg),
            "goal" => self.message(text).await,
            "init" => self.message(INIT).await,
            "cd" => self.cd(arg),
            "diff" => self.diff(),
            "status" => self.status(),
            "cost" | "stats" | "usage" => self.cost(),
            "verbose" => {
                self.verbose = !self.verbose;
                self.ok(if self.verbose {
                    "verbose: reasoning and every observation are shown"
                } else {
                    "verbose off"
                });
            }
            other => {
                let close: Vec<String> = COMMANDS
                    .iter()
                    .filter(|c| c.name.starts_with(&other[..other.len().min(2)]))
                    .map(|c| format!("/{}", c.name))
                    .collect();
                if close.is_empty() {
                    self.fail(&format!("no command /{other}: /help lists them"));
                } else {
                    self.fail(&format!(
                        "no command /{other}; did you mean {}?",
                        close.join(", ")
                    ));
                }
            }
        }
        Flow::Go
    }

    fn sessions(&self) {
        let all = self.h.list();
        if all.is_empty() {
            self.note("no sessions yet");
            return;
        }
        for m in all.iter().take(15) {
            let here = if Some(&m.id) == self.session.as_ref() {
                self.accent("●")
            } else {
                " ".into()
            };
            let closed = m
                .why_closed
                .as_deref()
                .map(|why| format!(" [read-only: {why}]"))
                .unwrap_or_default();
            eprintln!(
                "  {here} {}  {:<10} {:>3} turns  {}  {}{}",
                self.accent(&m.id),
                state_word(m),
                m.stats.turns,
                self.paint.dim(&one_line(&m.model, 24)),
                one_line(&m.title, 48),
                self.paint.dim(&closed),
            );
        }
        if all.len() > 15 {
            self.note(&format!("… and {} older", all.len() - 15));
        }
        self.note("/resume <id> continues one (the start of the id is enough)");
    }

    fn resume(&mut self, arg: &str) {
        let all = self.h.list();
        let found: Vec<&SessionMeta> = if arg.is_empty() {
            // The most recent one in this folder.
            all.iter()
                .filter(|m| m.why_closed.is_none() && Path::new(&m.cwd) == self.cwd)
                .take(1)
                .collect()
        } else {
            all.iter().filter(|m| m.id.starts_with(arg)).collect()
        };
        let meta = match found.as_slice() {
            [one] => (*one).clone(),
            [] if arg.is_empty() => {
                return self
                    .fail("no session to continue in this folder: /sessions lists them all");
            }
            [] => return self.fail(&format!("no session {arg}: /sessions lists them")),
            _ => {
                return self.fail(&format!(
                    "{arg} could be {} sessions: type more of the id",
                    found.len()
                ));
            }
        };
        if let Some(why) = &meta.why_closed {
            return self.fail(&format!(
                "{} can be read, not continued ({why}): sage show {}",
                meta.id, meta.id
            ));
        }
        let events = match self.h.events(&meta.id) {
            Ok(events) => events,
            Err(e) => return self.fail(&e),
        };
        self.session = Some(meta.id.clone());
        self.autonomy = resume_permission(self.start.autonomy.as_deref(), &events).into();
        self.pending = Changes {
            autonomy: (self.autonomy == "ask").then(|| self.autonomy.clone()),
            interactive: Some(true),
            max_turns: self.start.max_turns,
            time_budget_secs: self.start.time_budget_secs,
            ..Changes::default()
        };
        self.cwd = PathBuf::from(&meta.cwd);
        self.ok(&format!(
            "continuing “{}” · {} turns · {}",
            meta.title,
            meta.stats.turns,
            tilde(&self.cwd)
        ));
        let last = events.into_iter().rev().find_map(|e| match e.kind {
            EventKind::Finish { summary, .. } if !summary.trim().is_empty() => Some(summary),
            _ => None,
        });
        if let Some(summary) = last {
            eprintln!(
                "{}",
                self.paint.dim(&super::indent(summary.trim(), "    ", 8))
            );
        }
    }

    fn connect(&mut self, arg: &str) {
        let state = self.h.state();
        let mut words = arg.split_whitespace();
        let Some(first) = words.next() else {
            for p in &state.providers {
                let key = if p.has_key {
                    self.accent("key set")
                } else {
                    self.paint.dim("no key ")
                };
                eprintln!(
                    "  {:<12} {key}  {}",
                    p.id,
                    self.paint.dim(&format!("{} · {}", p.name, p.key_env))
                );
            }
            self.note("/connect <provider> stores a key · /connect <base-url> <model> adds an OpenAI-compatible endpoint (Ollama, vLLM, LM Studio)");
            return;
        };
        if first.starts_with("http://") || first.starts_with("https://") {
            let models: Vec<String> = words.map(String::from).collect();
            if models.is_empty() {
                return self
                    .fail("name the model too: /connect http://localhost:11434/v1 qwen3-coder");
            }
            let name = first
                .split("://")
                .nth(1)
                .and_then(|rest| rest.split(['/', ':']).next())
                .unwrap_or("custom")
                .to_string();
            eprint!("  API key for {name} (hidden; Enter for none): ");
            let Some(key) = tokio::task::block_in_place(read_secret) else {
                return self.note("cancelled");
            };
            match self.h.add_provider(&name, first, &models, &key) {
                Ok(id) => {
                    self.use_provider(&id, Some(models[0].clone()));
                    self.ok(&format!("added {id}: {} via {first}", models[0]));
                }
                Err(e) => self.fail(&e),
            }
            return;
        }
        if words.next().is_some() {
            return self.fail(&format!(
                "type /connect {first} alone: the key is then asked for, hidden (this line was not saved)"
            ));
        }
        let Some(info) = state.providers.iter().find(|p| p.id == first) else {
            return self.fail(&format!(
                "no provider {first}; one of: {}",
                state
                    .providers
                    .iter()
                    .map(|p| p.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        };
        eprint!(
            "  API key for {} (hidden; Enter on nothing cancels): ",
            info.name
        );
        let key = match tokio::task::block_in_place(read_secret) {
            Some(key) if !key.is_empty() => key,
            _ => return self.note("nothing changed"),
        };
        match self.h.set_key(&info.id, &key) {
            Ok(()) => {
                self.ok(&format!(
                    "key for {} stored in {} (only you can read it)",
                    info.name,
                    tilde(Path::new(&state.data_dir).join("config.json").as_path())
                ));
                let keyed = |id: &Option<String>| {
                    id.as_ref()
                        .and_then(|id| state.providers.iter().find(|p| &p.id == id))
                        .is_some_and(|p| p.has_key)
                };
                if !keyed(&self.start.provider) && !keyed(&state.provider) {
                    self.use_provider(&info.id, None);
                    let (_, model) = self.target();
                    self.ok(&format!(
                        "using {} via {}",
                        model.unwrap_or_default(),
                        info.id
                    ));
                } else if self.target().0.as_deref() != Some(&info.id) {
                    self.note(&format!("/provider {} switches to it", info.id));
                }
            }
            Err(e) => self.fail(&e),
        }
    }

    /// Switches to `provider` (and `model`) here and as the default from now on.
    fn use_provider(&mut self, provider: &str, model: Option<String>) {
        self.start.provider = Some(provider.to_string());
        self.start.model = model.clone();
        if self.session.is_some() {
            self.pending.provider = Some(provider.to_string());
            self.pending.model = model.clone();
        }
        if let Err(e) = self.h.set_defaults(Some(provider.to_string()), model, None) {
            self.fail(&format!("could not save it as the default: {e}"));
        }
    }

    fn provider(&mut self, arg: &str) {
        let state = self.h.state();
        let (current, _) = self.target();
        if arg.is_empty() {
            for p in &state.providers {
                let mark = if Some(&p.id) == current.as_ref() {
                    self.accent("●")
                } else {
                    " ".into()
                };
                let key = if p.has_key {
                    self.accent("key set")
                } else {
                    self.paint.dim("no key ")
                };
                eprintln!(
                    "  {mark} {:<12} {key}  {}",
                    p.id,
                    self.paint.dim(&one_line(
                        &format!("{} · {}", p.name, p.models.join(", ")),
                        70
                    ))
                );
            }
            self.note("/provider <id> switches · /connect <id> adds a key");
            return;
        }
        let Some(info) = state.providers.iter().find(|p| p.id == arg) else {
            return self.fail(&format!("no provider {arg}: /provider lists them"));
        };
        if !info.has_key {
            return self.fail(&format!(
                "{} has no key yet: /connect {}",
                info.name, info.id
            ));
        }
        self.use_provider(&info.id, None);
        let (_, model) = self.target();
        self.ok(&format!(
            "provider {}{}",
            info.id,
            model.map(|m| format!(" · model {m}")).unwrap_or_default()
        ));
        if self.session.is_some() {
            self.note("this session goes on there with your next message");
        }
    }

    fn model(&mut self, arg: &str) {
        let (provider, current) = self.target();
        let Some(provider) = provider else {
            return self.fail("no provider has a key yet: /connect <provider>");
        };
        if arg.is_empty() {
            let models = self.view.lock().unwrap().models.clone();
            for m in &models {
                let mark = if Some(m) == current.as_ref() {
                    self.accent("●")
                } else {
                    " ".into()
                };
                eprintln!("  {mark} {m}");
            }
            if current.as_ref().is_some_and(|c| !models.contains(c)) {
                eprintln!("  {} {}", self.accent("●"), current.unwrap_or_default());
            }
            self.note(&format!(
                "/model <name> switches; any model {provider} serves will do"
            ));
            return;
        }
        self.use_provider(&provider, Some(arg.to_string()));
        self.ok(&format!("model {arg} via {provider}"));
    }

    fn effort(&mut self, arg: &str) {
        if arg.is_empty() {
            return self.note(&format!(
                "effort {} · /effort auto (recommended: it rises by itself when the task needs it), low, high or max",
                self.effort_label()
            ));
        }
        let value = match arg {
            "auto" | "ratchet" => "ratchet",
            "low" | "high" | "max" => arg,
            _ => return self.fail("effort is auto, low, high or max"),
        };
        self.start.effort = Some(value.into());
        if self.session.is_some() {
            self.pending.effort = Some(value.into());
        }
        let _ = self.h.set_defaults(None, None, Some(value.into()));
        self.ok(&format!("effort {arg}"));
    }

    fn mode(&mut self, arg: &str) {
        match arg {
            "" => self.note(&format!(
                "mode {} · /mode sage or /mode default (for new sessions)",
                self.start.mode.as_deref().unwrap_or("sage")
            )),
            "sage" | "default" => {
                self.start.mode = Some(arg.into());
                self.ok(&format!("mode {arg} for new sessions"));
                if self.session.is_some() {
                    self.note("this session keeps its mode: /new starts one in the new mode");
                }
            }
            _ => self.fail("mode is sage or default"),
        }
    }

    fn permissions(&mut self, arg: &str) {
        match arg {
            "" => self.note(&format!(
                "permissions {} · /permissions ask asks before every edit and command, auto does not",
                self.autonomy
            )),
            "auto" | "ask" => {
                self.start.autonomy = Some(arg.into());
                self.autonomy = arg.into();
                if self.session.is_some() {
                    self.pending.autonomy = Some(arg.into());
                }
                self.ok(if arg == "ask" {
                    "Sage asks before every edit and command"
                } else {
                    "Sage edits and runs commands without asking (it still asks before destructive ones)"
                });
            }
            _ => self.fail("permissions are auto or ask"),
        }
    }

    fn skills(&mut self, arg: &str) {
        let all = self.h.skills(Some(&self.cwd.to_string_lossy()));
        let on: Vec<String> = self.meta().map(|m| m.skills).unwrap_or_default();
        if arg.is_empty() {
            for s in &all {
                let mark = if on.contains(&s.name) || self.start.skills.contains(&s.name) {
                    self.accent("●")
                } else {
                    " ".into()
                };
                eprintln!("  {mark} {:<12} {}", s.name, self.paint.dim(&s.description));
            }
            self.note("/skills <name> switches one on with your next message; a project's .sage/skills adds its own");
            return;
        }
        for name in arg.split([',', ' ']).filter(|n| !n.is_empty()) {
            if !all.iter().any(|s| s.name == name) {
                self.fail(&format!("no skill {name}: /skills lists them"));
            } else if on.iter().chain(&self.start.skills).any(|s| s == name) {
                self.note(&format!("{name} is on already"));
            } else {
                self.start.skills.push(name.to_string());
                self.ok(&format!("{name} switches on with your next message"));
            }
        }
    }

    fn cd(&mut self, arg: &str) {
        if arg.is_empty() {
            return self.note(&format!("folder {}", tilde(&self.cwd)));
        }
        let path = expand(&self.cwd, arg);
        match sage_core::util::canonical(&path) {
            Ok(dir) if dir.is_dir() => {
                self.cwd = dir;
                self.ok(&format!("folder {}", tilde(&self.cwd)));
                if self.session.take().is_some() {
                    self.pending = Changes::default();
                    self.note("your next message starts a new session there");
                }
            }
            _ => self.fail(&format!("no folder {}", path.display())),
        }
    }

    fn diff(&self) {
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&self.cwd)
                .args(args)
                .output()
                .ok()
                .filter(|out| out.status.success())
                .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        };
        let Some(short) = git(&["status", "--short"]) else {
            return self.fail("not a git repository (or git is not installed)");
        };
        if short.trim().is_empty() {
            return self.note("nothing changed since the last commit");
        }
        let stat = git(&["diff", "--stat", "HEAD"]).unwrap_or_default();
        for line in stat.lines() {
            let painted = line
                .replace('+', &self.paint.green("+"))
                .replace('-', &self.paint.red("-"));
            eprintln!("  {painted}");
        }
        let untracked: Vec<&str> = short
            .lines()
            .filter_map(|l| l.strip_prefix("?? "))
            .collect();
        if !untracked.is_empty() {
            eprintln!(
                "  {} {}",
                self.paint.dim("new, untracked:"),
                untracked.join(", ")
            );
        }
    }

    fn status(&self) {
        let state = self.h.state();
        let (provider, model) = self.target();
        let row = |name: &str, value: String| {
            eprintln!("  {}  {value}", self.paint.dim(&format!("{name:<11}")))
        };
        row("version", state.version.clone());
        row("folder", tilde(&self.cwd));
        row(
            "model",
            match (&provider, &model) {
                (Some(p), Some(m)) => format!("{m} via {p}"),
                (Some(p), None) => format!("via {p}"),
                _ => "none: no provider has a key (/connect)".into(),
            },
        );
        row("effort", self.effort_label());
        if let Some(mode) = self.start.mode.as_deref().filter(|mode| *mode != "sage") {
            row("mode", mode.to_string());
        }
        row("permissions", self.autonomy.clone());
        match self.meta() {
            Some(meta) => {
                row(
                    "session",
                    format!("{} · {} · {} turns", meta.id, meta.title, meta.stats.turns),
                );
                if !meta.skills.is_empty() {
                    row("skills", meta.skills.join(", "));
                }
                if let Some(goal) = &meta.goal {
                    row(
                        "goal",
                        format!("{}{}", goal.text, if goal.met { " (met)" } else { "" }),
                    );
                }
            }
            None => row("session", "none yet: your next message starts one".into()),
        }
        if !self.start.skills.is_empty() {
            row("next skills", self.start.skills.join(", "));
        }
        row("data", tilde(Path::new(&state.data_dir)));
    }

    fn cost(&self) {
        match self.meta() {
            None => self.note("no session yet"),
            Some(meta) => {
                let printer = Printer {
                    paint: Paint { on: self.color },
                    verbose: false,
                    pending: None,
                    width: 0,
                };
                eprintln!(
                    "{}",
                    super::indent(&printer.stats_block(&meta.stats, meta.simulated), "  ", 8)
                );
            }
        }
    }

    fn shell(&self, command: &str) {
        if command.is_empty() {
            return self.fail("usage: !<command>, run in the session's folder");
        }
        #[cfg(windows)]
        let mut cmd = sage_core::util::windows_shell_command(command, &[]);
        #[cfg(not(windows))]
        let mut cmd = {
            let mut c = std::process::Command::new("sh");
            c.arg("-c").arg(command);
            c
        };
        match tokio::task::block_in_place(|| cmd.current_dir(&self.cwd).status()) {
            Ok(status) if status.success() => {}
            Ok(status) => self.note(&format!(
                "exit {}",
                status.code().map_or("by signal".into(), |c| c.to_string())
            )),
            Err(e) => self.fail(&e.to_string()),
        }
    }

    /// Sends a message to the session, or opens one with it.
    async fn message(&mut self, typed: &str) {
        let mut text = typed.to_string();
        if let Ok(n) = text.trim().parse::<usize>()
            && (1..=self.choices.len()).contains(&n)
        {
            text = self.choices[n - 1].clone();
            eprintln!("  {}", self.paint.dim(&format!("→ {text}")));
        }
        self.choices.clear();
        let files = attachments(&text, &self.cwd);
        let Some(id) = self.session.clone() else {
            let req = StartRequest {
                cwd: self.cwd.to_string_lossy().into_owned(),
                task: text,
                autonomy: Some(self.autonomy.clone()),
                interactive: true,
                files,
                ..self.start.clone()
            };
            match self.h.start(req) {
                Ok(meta) => {
                    self.start.goal = None;
                    self.start.skills.clear();
                    self.session = Some(meta.id.clone());
                    self.pending = Changes::default();
                    self.follow(&meta.id, 0).await;
                }
                Err(e) if e.contains("no provider has an API key") => {
                    self.fail("no provider has a key yet");
                    self.note("/connect <provider> adds one, for example /connect openrouter");
                }
                Err(e) => self.fail(&e),
            }
            return;
        };
        let before = self
            .h
            .events(&id)
            .ok()
            .and_then(|e| e.last().map(|e| e.seq))
            .unwrap_or(0);
        match self
            .h
            .send_with(&id, &text, &self.start.skills, &files, self.pending.clone())
        {
            Ok(()) => {
                self.pending = Changes::default();
                self.start.skills.clear();
                self.follow(&id, before).await;
            }
            Err(e) => self.fail(&e),
        }
    }

    /// Shows the session's run until it ends. Ctrl-C stops it; approvals are asked for here.
    async fn follow(&mut self, id: &str, after: u64) {
        let (backlog, mut rx) = match self.h.subscribe(id) {
            Ok(feed) => feed,
            Err(e) => return self.fail(&e),
        };
        let mut printer = Printer {
            paint: Paint { on: self.color },
            verbose: self.verbose,
            pending: None,
            width: if self.verbose {
                0
            } else {
                width().saturating_sub(1)
            },
        };
        let started = Instant::now();
        let mut spinner = Spinner {
            on: self.color,
            shown: false,
            frame: 0,
            since: started,
            doing: "starting".into(),
        };
        let mut queue: VecDeque<Event> = backlog.into_iter().filter(|e| e.seq > after).collect();
        let mut stats = Stats::default();
        let mut ended: Option<FinishStatus> = None;
        let mut stopping = false;
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        let mut interrupt = Box::pin(tokio::signal::ctrl_c());
        eprintln!();
        'run: loop {
            while let Some(ev) = queue.pop_front() {
                if ev.seq != 0 && ev.seq <= after {
                    continue;
                }
                spinner.doing = match &ev.kind {
                    EventKind::TurnStart { .. } => "thinking".into(),
                    EventKind::Delta {
                        channel: Channel::Reasoning,
                        ..
                    } => "thinking".into(),
                    EventKind::Delta {
                        channel: Channel::Content,
                        ..
                    } => "writing".into(),
                    EventKind::StepStart { tool, summary, .. } => {
                        one_line(&format!("{tool} {summary}"), 60)
                    }
                    EventKind::StepEnd { .. } | EventKind::Watch { .. } => "working".into(),
                    _ => std::mem::take(&mut spinner.doing),
                };
                if ev.kind.transient() {
                    continue;
                }
                spinner.clear();
                match &ev.kind {
                    EventKind::SessionStart { .. } | EventKind::User { .. } => {}
                    EventKind::Approval {
                        id: asked,
                        tool,
                        summary,
                        reason,
                        ..
                    } => {
                        eprintln!(
                            "    {} {} {}  {}",
                            self.paint.yellow("?"),
                            self.paint.bold(tool),
                            summary,
                            self.paint.dim(&format!("({reason})"))
                        );
                        eprint!(
                            "      allow? {} {}  ",
                            self.accent("[y]es"),
                            self.paint.dim("[n]o · ctrl-c stops the run")
                        );
                        let answer = tokio::task::block_in_place(answer_key);
                        let allow = matches!(answer, Answer::Yes);
                        eprintln!(
                            "{}",
                            if allow {
                                self.paint.green("yes")
                            } else {
                                self.paint.red("no")
                            }
                        );
                        if let Err(e) = self.h.approve(id, *asked, allow) {
                            self.fail(&e);
                        }
                        if matches!(answer, Answer::Stop) && !stopping {
                            stopping = true;
                            let _ = self.h.stop(id);
                            eprintln!("  {}", self.paint.yellow("stopping…"));
                        }
                    }
                    EventKind::Question { choices, .. } => {
                        printer.print(&ev);
                        self.choices = choices.iter().map(|c| c.label.clone()).collect();
                    }
                    // The question is shown on its own, with its choices: not as a step too.
                    EventKind::StepEnd { .. }
                        if printer
                            .pending
                            .as_ref()
                            .is_some_and(|(tool, ..)| tool == "ask") =>
                    {
                        printer.pending = None;
                    }
                    // What a watch tells the model runs long; a person needs its first words.
                    EventKind::Watch { detail, name, .. } if !self.verbose => {
                        let line = match detail.lines().next() {
                            Some(first) if !first.is_empty() => first.to_string(),
                            _ => format!("watch {name}: still passing"),
                        };
                        eprintln!(
                            "    {} {}",
                            self.paint.cyan("◦"),
                            one_line(&line, width().saturating_sub(7))
                        );
                    }
                    EventKind::Stats { stats: now } => stats = now.clone(),
                    EventKind::Finish { status, .. } => {
                        // A run that ends on a question has said everything in asking it.
                        if !(*status == FinishStatus::Yielded && !self.choices.is_empty()) {
                            printer.print(&ev);
                        }
                        ended = Some(*status);
                    }
                    _ => printer.print(&ev),
                }
            }
            if ended.is_some() {
                break;
            }
            tokio::select! {
                ev = rx.recv() => match ev {
                    Ok(ev) => queue.push_back(ev),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break 'run,
                },
                _ = &mut interrupt => {
                    interrupt = Box::pin(tokio::signal::ctrl_c());
                    spinner.clear();
                    if stopping {
                        eprintln!("  {}", self.paint.yellow("still stopping; back to the prompt (the run ends on its own)"));
                        break 'run;
                    }
                    stopping = true;
                    eprintln!("  {}", self.paint.yellow("stopping… (Ctrl-C again to return to the prompt)"));
                    let _ = self.h.stop(id);
                },
                _ = tick.tick() => spinner.draw(),
            }
        }
        spinner.clear();
        let cost = if stats.cost <= 0.0 && !stats.priced {
            String::new()
        } else {
            format!(" · ${:.4} so far", stats.cost)
        };
        eprintln!(
            "  {}",
            self.paint.dim(&format!(
                "└ {} · {} turns · {} tokens in ({} cached), {} out{cost}",
                fmt_duration(started.elapsed().as_millis() as u64),
                stats.turns,
                k(stats.prompt_tokens),
                pct(stats.cached_tokens, stats.prompt_tokens),
                k(stats.output_tokens),
            ))
        );
        if !self.choices.is_empty() {
            self.note("answer with a number, or in your own words");
        }
    }
}

/// `/connect <provider> <key>`: a key typed on the line, which the history must not keep.
fn names_a_key(line: &str) -> bool {
    let mut words = line.split_whitespace();
    matches!(words.next(), Some("/connect" | "/login" | "/key"))
        && words
            .next()
            .is_some_and(|w| !w.starts_with("http://") && !w.starts_with("https://"))
        && words.next().is_some()
}

/// Takes `line` out of the history file. The file keeps what is on it when the history is next
/// written: that writes only the lines typed since.
fn forget(history: &Path, line: &str) {
    let Ok(text) = std::fs::read_to_string(history) else {
        return;
    };
    let kept: Vec<&str> = text.lines().filter(|l| l.trim() != line.trim()).collect();
    let mut body = kept.join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    let _ = std::fs::write(history, body);
}

/// The end of a session id, which is what tells sessions of the same day apart.
fn short(id: &str) -> &str {
    id.rsplit('-').next().unwrap_or(id)
}

/// `sage` or `sage chat [options] [message…]`.
pub async fn run(h: &Harness, args: &[String]) -> ExitCode {
    if !std::io::stdin().is_terminal() {
        eprintln!("error: an interactive session needs a terminal; `sage run` works without one");
        return ExitCode::from(2);
    }
    let mut rest = Vec::new();
    let mut resume: Option<String> = None;
    let mut ask = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-c" | "--continue" => resume = Some(String::new()),
            "-r" | "--resume" => match it.next() {
                Some(id) => resume = Some(id.clone()),
                None => {
                    eprintln!("error: --resume needs a session id");
                    return ExitCode::from(2);
                }
            },
            "--ask" => ask = true,
            _ => rest.push(a.clone()),
        }
    }
    let mut opts = match parse_run(&rest, false) {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let cwd = match sage_core::util::canonical(&opts.req.cwd) {
        Ok(dir) if dir.is_dir() => dir,
        _ => {
            eprintln!("error: no folder {}", opts.req.cwd);
            return ExitCode::from(2);
        }
    };
    let color = std::io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let view: Shared = Arc::new(Mutex::new(View::default()));
    opts.req.autonomy = ask.then(|| "ask".into());
    let first = std::mem::take(&mut opts.req.task);
    let mut chat = Chat {
        h: h.clone(),
        paint: Paint { on: color },
        color,
        view: view.clone(),
        cwd,
        session: None,
        start: opts.req,
        autonomy: if ask { "ask".into() } else { "auto".into() },
        pending: Changes::default(),
        verbose: opts.verbose,
        choices: Vec::new(),
        quitting: false,
    };
    let history = PathBuf::from(h.state().data_dir).join("history.txt");
    let mut line_editor = editor(&view, color, history.clone());
    if let Some(id) = &resume {
        chat.resume(id);
        if chat.session.is_none() {
            return ExitCode::from(2);
        }
        chat.pending.provider.clone_from(&chat.start.provider);
        chat.pending.model.clone_from(&chat.start.model);
        chat.pending.effort.clone_from(&chat.start.effort);
    }
    chat.banner();
    if !first.trim().is_empty() || chat.start.goal.is_some() {
        let first = if first.is_empty() {
            "Work towards the goal.".to_string()
        } else {
            first
        };
        eprintln!("{} {first}", chat.accent("›"));
        chat.message(&first).await;
    }
    loop {
        chat.refresh();
        eprintln!();
        let read = tokio::task::block_in_place(|| line_editor.read_line(&Line));
        let text = match read {
            Ok(Signal::Success(text)) => {
                if names_a_key(&text) {
                    let _ = line_editor.sync_history();
                    forget(&history, &text);
                }
                text.replace("\\\n", "\n")
            }
            Ok(Signal::CtrlC(typed)) => {
                if typed.trim().is_empty() {
                    if chat.quitting {
                        break;
                    }
                    chat.quitting = true;
                    chat.note("Ctrl-C again (or Ctrl-D) quits");
                }
                continue;
            }
            Ok(Signal::CtrlD) => break,
            Ok(_) => continue,
            Err(e) => {
                eprintln!("error: {e}");
                break;
            }
        };
        chat.quitting = false;
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        if let Flow::Quit = chat.line(text).await {
            break;
        }
    }
    if let Some(meta) = chat.meta() {
        chat.note(&format!(
            "session {} is kept: sage --resume {} continues it",
            meta.id, meta.id
        ));
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete(line: &str) -> Vec<String> {
        let view: Shared = Arc::new(Mutex::new(View {
            cwd: std::env::temp_dir(),
            providers: vec![
                ("deepseek".into(), "DeepSeek".into()),
                ("openrouter".into(), "OpenRouter".into()),
            ],
            models: vec!["deepseek-flash".into(), "deepseek-v4-pro".into()],
            ..Default::default()
        }));
        Complete { view }
            .complete(line, line.len())
            .suggestions()
            .iter()
            .map(|s| s.value.clone())
            .collect()
    }

    #[test]
    fn a_slash_lists_the_commands_and_an_argument_lists_its_values() {
        assert_eq!(complete("/").len(), COMMANDS.len());
        assert_eq!(complete("/pro"), ["/provider"]);
        assert_eq!(
            complete("/mode"),
            ["/model"],
            "/mode is for developers, not offered"
        );
        assert_eq!(complete("/provider o"), ["openrouter"]);
        assert_eq!(complete("/model deepseek-v"), ["deepseek-v4-pro"]);
        assert_eq!(complete("/effort m"), ["max"]);
        assert!(
            complete("fix src/main.rs").is_empty(),
            "a slash inside a message is not a command"
        );
    }

    #[test]
    fn enter_runs_a_picked_command_and_only_takes_a_picked_file() {
        let runs_pick = |line: &str| matches!(enter_for(line), ReedlineEvent::Multiple(events) if matches!(events[0], ReedlineEvent::Enter));
        let runs_typed = |line: &str| matches!(enter_for(line), ReedlineEvent::Multiple(events) if matches!(events[0], ReedlineEvent::Esc));
        assert!(runs_pick("/sta"));
        assert!(runs_pick("/provider open"));
        assert!(runs_typed("/connect "));
        assert!(matches!(enter_for("look at @src/ma"), ReedlineEvent::Enter));
        assert!(matches!(
            enter_for("/goal read @src/ma"),
            ReedlineEvent::Enter
        ));
        assert!(matches!(enter_for("fix the tests"), ReedlineEvent::Enter));
    }

    #[test]
    fn at_words_name_files_to_attach() {
        let dir = std::env::temp_dir().join(format!("sage-chat-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
        let files = attachments("look at @src/main.rs, and @nope.rs", &dir);
        assert_eq!(
            files,
            [dir.join("src/main.rs").to_string_lossy().into_owned()]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_key_typed_on_the_line_is_taken_out_of_the_history() {
        assert!(names_a_key("/connect openrouter sk-or-123"));
        assert!(!names_a_key("/connect openrouter"));
        assert!(!names_a_key("/connect http://localhost:11434/v1 qwen3"));
        assert!(!names_a_key("connect openrouter sk"));
        let file = std::env::temp_dir().join(format!("sage-history-{}", std::process::id()));
        std::fs::write(
            &file,
            "fix the tests\n/connect openrouter sk-or-123\n/status\n",
        )
        .unwrap();
        forget(&file, "/connect openrouter sk-or-123");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "fix the tests\n/status\n"
        );
        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn ids_and_paths_read_short() {
        assert_eq!(short("20261120-134903-2ebc"), "2ebc");
        assert_eq!(tilde(&sage_core::util::home_dir()), "~");
    }
}
