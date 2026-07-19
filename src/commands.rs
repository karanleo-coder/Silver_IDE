use std::collections::BTreeMap;

/// Commands typed into the popup terminal (Ctrl+T).
/// Kept deliberately simple, cheat-code style. The word that triggers
/// each action comes from the user's config, so `spawn` can become
/// `t`, `open` can become `e`, and so on.
pub enum Action {
    ToggleFiles,
    /// `open` with no arg opens the last copied path; `open <path>` opens that.
    Open(Option<String>),
    Cd(String),
    Save,
    Help,
    Home,
    QuitApp,
    CatName(String),
    CatColor(String),
    Accent(String),
    /// `cursor <style>` / `cursor blink on|off`; validated in the handler.
    Cursor(String),
    Clear,
    /// Spawn a real terminal tab where you are; the file moves aside.
    Spawn,
    /// Run the active file's program in a terminal tab.
    Run,
    /// Open the customize panel to view and rebind keys & commands.
    Keys,
    Unknown(String),
}

/// Extra spellings that always work — unless the user has claimed the
/// word for a different command.
fn builtin_alias(word: &str) -> Option<&'static str> {
    Some(match word {
        "files" | "dir" | "tree" | "list" => "files",
        "o" => "open",
        "w" => "save",
        "?" => "help",
        "quit" | "q" => "quit",
        "cls" => "clear",
        "accent" => "theme",
        "term" | "terminal" => "spawn",
        "r" => "run",
        "shortcuts" | "binds" | "commands" | "cmds" => "keys",
        _ => return None,
    })
}

pub fn parse(input: &str, cmds: &BTreeMap<String, String>) -> Action {
    let input = input.trim();
    let mut parts = input.splitn(2, char::is_whitespace);
    let word = parts.next().unwrap_or("").to_lowercase();
    let rest = parts.next().unwrap_or("").trim().to_string();

    // The configured word for an action wins; built-in aliases fill in
    // only when the word isn't claimed by anything.
    let action = cmds
        .iter()
        .find(|(_, w)| w.as_str() == word)
        .map(|(a, _)| a.clone())
        .or_else(|| builtin_alias(&word).map(str::to_string));

    match action.as_deref() {
        Some("files") => Action::ToggleFiles,
        Some("open") => {
            if rest.is_empty() {
                Action::Open(None)
            } else {
                Action::Open(Some(rest))
            }
        }
        Some("cd") => {
            if rest.is_empty() {
                Action::Unknown("usage: cd <path>".into())
            } else {
                Action::Cd(rest)
            }
        }
        Some("save") => Action::Save,
        Some("help") => Action::Help,
        Some("home") => Action::Home,
        Some("quit") => Action::QuitApp,
        Some("clear") => Action::Clear,
        Some("cat") => {
            let mut sub = rest.splitn(2, char::is_whitespace);
            let field = sub.next().unwrap_or("").to_lowercase();
            let value = sub.next().unwrap_or("").trim().to_string();
            match (field.as_str(), value.is_empty()) {
                ("name", false) => Action::CatName(value),
                ("color", false) => Action::CatColor(value),
                _ => Action::Unknown("usage: cat name <name> | cat color <color>".into()),
            }
        }
        Some("theme") => {
            if rest.is_empty() {
                Action::Unknown("usage: theme <color>".into())
            } else {
                Action::Accent(rest)
            }
        }
        Some("cursor") => Action::Cursor(rest),
        Some("spawn") => Action::Spawn,
        Some("run") => Action::Run,
        Some("keys") => Action::Keys,
        _ => Action::Unknown(format!("unknown command: {input}  (try `help`)")),
    }
}

/// The help text, spelled with the user's own command words.
pub fn help_lines(cmds: &BTreeMap<String, String>) -> Vec<String> {
    let w = |a: &str| cmds.get(a).cloned().unwrap_or_else(|| a.to_string());
    let dots = |s: &str| format!("{s} {}", ".".repeat(20usize.saturating_sub(s.chars().count())));
    vec![
        format!("{} toggle the folder panel", dots(&w("files"))),
        format!("{} open a file beside the current one", dots(&format!("{} <path>", w("open")))),
        format!("{} open the last copied path", dots(&w("open"))),
        format!("{} switch project folder", dots(&format!("{} <path>", w("cd")))),
        format!("{} save the active file", dots(&w("save"))),
        format!("{} a real terminal tab, here; the file moves right", dots(&w("spawn"))),
        format!("{} run the active file's program in a terminal", dots(&w("run"))),
        format!("{} view & edit shortcut keys and commands", dots(&w("keys"))),
        format!("{} rename your cat", dots(&format!("{} name <name>", w("cat")))),
        format!("{} recolor your cat (names or #hex)", dots(&format!("{} color <color>", w("cat")))),
        format!("{} change the accent color", dots(&format!("{} <color>", w("theme")))),
        format!("{} block | bar | underline | hollow", dots(&format!("{} <style>", w("cursor")))),
        format!("{} steady or blinking cursor", dots(&format!("{} blink on|off", w("cursor")))),
        format!("{} back to the start screen", dots(&w("home"))),
        format!("{} clear this terminal", dots(&w("clear"))),
        format!("{} quit silver", dots(&w("quit"))),
    ]
}
