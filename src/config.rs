use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

pub const CAT_COLORS: &[&str] = &[
    "silver", "cyan", "magenta", "yellow", "green", "blue", "red", "white", "orange", "pink",
];

#[derive(Serialize, Deserialize, Clone)]
pub struct CatConfig {
    pub name: String,
    pub color: String,
}

impl Default for CatConfig {
    fn default() -> Self {
        Self { name: "Silver".into(), color: "silver".into() }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ThemeConfig {
    pub accent: String,
    pub dim: String,
    /// "block" | "bar" | "underline" | "hollow"
    #[serde(default = "default_cursor_style")]
    pub cursor: String,
    #[serde(default = "default_true")]
    pub cursor_blink: bool,
}

fn default_cursor_style() -> String {
    "block".into()
}

fn default_true() -> bool {
    true
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            accent: "cyan".into(),
            dim: "darkgray".into(),
            cursor: default_cursor_style(),
            cursor_blink: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RecentProject {
    pub name: String,
    pub path: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub cat: CatConfig,
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default)]
    pub recents: Vec<RecentProject>,
    /// action name -> key combo, e.g. "popup_terminal" = "ctrl+t".
    /// Users can edit these in config.toml.
    #[serde(default)]
    pub keys: BTreeMap<String, String>,
    /// action name -> the word that triggers it in the popup terminal,
    /// e.g. "spawn" = "t". Also editable in config.toml.
    #[serde(default)]
    pub commands: BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cat: CatConfig::default(),
            theme: ThemeConfig::default(),
            recents: Vec::new(),
            keys: default_keys(),
            commands: default_commands(),
        }
    }
}

pub fn default_commands() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for (action, word) in [
        ("files", "ls"),
        ("open", "open"),
        ("cd", "cd"),
        ("save", "save"),
        ("help", "help"),
        ("home", "home"),
        ("quit", "exit"),
        ("clear", "clear"),
        ("cat", "cat"),
        ("theme", "theme"),
        ("cursor", "cursor"),
        ("spawn", "spawn"),
        ("run", "run"),
        ("keys", "keys"),
        ("check", "check"),
        ("debug", "debug"),
        ("break", "break"),
    ] {
        m.insert(action.to_string(), word.to_string());
    }
    m
}

pub fn default_keys() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("popup_terminal".into(), "ctrl+t".into());
    m.insert("save".into(), "ctrl+s".into());
    m.insert("files_panel".into(), "ctrl+b".into());
    m.insert("location".into(), "ctrl+l".into());
    m.insert("home".into(), "ctrl+h".into());
    m.insert("quit".into(), "ctrl+q".into());
    m.insert("switch_pane".into(), "ctrl+w".into());
    m.insert("next_tab".into(), "ctrl+n".into());
    m.insert("open_right".into(), "ctrl+o".into());
    m.insert("cycle_files".into(), "ctrl+tab".into());
    m.insert("close_pane".into(), "ctrl+x".into());
    m.insert("split_left".into(), "alt+left".into());
    m.insert("split_right".into(), "alt+right".into());
    m.insert("toggle_breakpoint".into(), "ctrl+p".into());
    m.insert("debug_run".into(), "ctrl+g".into());
    m.insert("complete".into(), "ctrl+space".into());
    m
}

impl Config {
    pub fn path() -> PathBuf {
        if let Some(dirs) = directories::ProjectDirs::from("dev", "silver", "silver-cli") {
            dirs.config_dir().join("config.toml")
        } else {
            PathBuf::from(".silver-cli.toml")
        }
    }

    pub fn load() -> Self {
        let mut cfg: Config = fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default();
        // Fill in any keybindings missing from the user's file so new
        // actions get defaults without wiping customisations.
        for (k, v) in default_keys() {
            cfg.keys.entry(k).or_insert(v);
        }
        for (k, v) in default_commands() {
            cfg.commands.entry(k).or_insert(v);
        }
        cfg
    }

    pub fn save(&self) {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(s) = toml::to_string_pretty(self) {
            let _ = fs::write(path, s);
        }
    }

    pub fn add_recent(&mut self, path: &std::path::Path) {
        let p = path.to_string_lossy().to_string();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| p.clone());
        self.recents.retain(|r| r.path != p);
        self.recents.insert(0, RecentProject { name, path: p });
        self.recents.truncate(12);
        self.save();
    }

    pub fn binding(&self, action: &str) -> Option<(KeyModifiers, KeyCode)> {
        self.keys.get(action).and_then(|s| parse_key(s))
    }

    pub fn key_is(&self, action: &str, ev: &KeyEvent) -> bool {
        match self.binding(action) {
            Some((mods, code)) => {
                if ev.code != code {
                    return false;
                }
                // Ignore SHIFT for plain-char bindings so 'c' still matches
                // if the terminal reports a shift state.
                let ev_mods = ev.modifiers & !(KeyModifiers::SHIFT);
                let want = mods & !(KeyModifiers::SHIFT);
                ev_mods == want
            }
            None => false,
        }
    }

    pub fn accent(&self) -> Color {
        parse_color(&self.theme.accent)
    }

    pub fn dim(&self) -> Color {
        parse_color(&self.theme.dim)
    }

    pub fn cat_color(&self) -> Color {
        parse_color(&self.cat.color)
    }
}

pub fn parse_key(s: &str) -> Option<(KeyModifiers, KeyCode)> {
    let mut mods = KeyModifiers::empty();
    let mut code = None;
    for part in s.split('+') {
        let p = part.trim().to_lowercase();
        match p.as_str() {
            "ctrl" | "control" => mods |= KeyModifiers::CONTROL,
            "alt" | "option" => mods |= KeyModifiers::ALT,
            "shift" => mods |= KeyModifiers::SHIFT,
            "esc" | "escape" => code = Some(KeyCode::Esc),
            "enter" | "return" => code = Some(KeyCode::Enter),
            "tab" => code = Some(KeyCode::Tab),
            "space" => code = Some(KeyCode::Char(' ')),
            "up" => code = Some(KeyCode::Up),
            "down" => code = Some(KeyCode::Down),
            "left" => code = Some(KeyCode::Left),
            "right" => code = Some(KeyCode::Right),
            other => {
                let mut chars = other.chars();
                if let (Some(c), None) = (chars.next(), chars.next()) {
                    code = Some(KeyCode::Char(c));
                }
            }
        }
    }
    code.map(|c| (mods, c))
}

pub fn parse_color(s: &str) -> Color {
    let s = s.trim().to_lowercase();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&hex[0..2], 16),
                u8::from_str_radix(&hex[2..4], 16),
                u8::from_str_radix(&hex[4..6], 16),
            ) {
                return Color::Rgb(r, g, b);
            }
        }
    }
    match s.as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" | "purple" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "white" => Color::White,
        "silver" => Color::Rgb(192, 192, 192),
        "orange" => Color::Rgb(255, 165, 0),
        "pink" => Color::Rgb(255, 121, 198),
        _ => Color::Rgb(192, 192, 192),
    }
}

/// On Windows, `canonicalize` returns `\\?\C:\...` verbatim paths.
/// cmd.exe refuses those as a working directory (the terminal would
/// silently fail to open) and they look wrong in titles, so strip the
/// prefix back off. Everywhere else the path passes through untouched.
pub fn clean_path(p: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(rest.to_string());
        }
    }
    p
}

pub fn expand_tilde(input: &str) -> PathBuf {
    if let Some(rest) = input.strip_prefix('~') {
        if let Some(base) = directories::BaseDirs::new() {
            let rest = rest.trim_start_matches(['/', '\\']);
            return base.home_dir().join(rest);
        }
    }
    PathBuf::from(input)
}
