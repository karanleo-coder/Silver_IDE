use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Position as GridPos, Rect};
use ratatui::DefaultTerminal;

use crate::commands::{self, Action};
use crate::config::{expand_tilde, Config, CAT_COLORS};
use crate::editor::buffer::Buffer;
use crate::ui;

pub enum Screen {
    Home,
    Editor,
}

pub struct CustomizeState {
    pub name: String,
    pub color_idx: usize,
}

/// Folder browser that replaces the recents panel after `cd <path>`.
pub struct HomeBrowser {
    pub dir: PathBuf,
    pub selected: usize,
    pub entries: Vec<FileEntry>,
}

pub struct HomeState {
    pub selected: usize,
    pub focus_terminal: bool,
    pub term_input: String,
    pub term_output: Vec<String>,
    /// Lines scrolled up from the bottom of the output.
    pub term_scroll: usize,
    pub customize: Option<CustomizeState>,
    pub browser: Option<HomeBrowser>,
}

impl Default for HomeState {
    fn default() -> Self {
        Self {
            selected: 0,
            focus_terminal: false,
            term_input: String::new(),
            term_output: vec![
                "welcome to silver — `cd <path>` browses a folder,".into(),
                "`start` opens the editor there. or pick a recent".into(),
                "project with ↑/↓ and Enter.".into(),
            ],
            term_scroll: 0,
            customize: None,
            browser: None,
        }
    }
}

#[derive(Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub depth: usize,
    pub is_dir: bool,
}

pub struct FilesPanel {
    pub selected: usize,
    pub expanded: BTreeSet<PathBuf>,
    pub entries: Vec<FileEntry>,
}

pub struct LocationDropdown {
    pub dir: PathBuf,
    pub selected: usize,
    pub entries: Vec<FileEntry>,
}

/// The "open here" picker: reached by hovering the right edge, the
/// tab bar's `+`, or the open_right key. Pick a file with Enter and
/// it appears in the chosen pane.
pub struct PlacePicker {
    pub selected: usize,
    pub expanded: BTreeSet<PathBuf>,
    pub entries: Vec<FileEntry>,
    /// Pane the file will show in; None = decide on pick (asks
    /// which side to replace when both panes are busy).
    pub target: Option<usize>,
    /// A picked file waiting for that left/right answer.
    pub choosing: Option<PathBuf>,
    /// Highlighted side while choosing: 0 = left, 1 = right.
    pub side: usize,
}

pub struct Popup {
    pub input: String,
    pub lines: Vec<String>,
    /// Lines scrolled up from the bottom of the output.
    pub scroll: usize,
}

/// One editor split. Files open here as a stack of tabs shown in the
/// pane's tab bar; `tab` is the one currently displayed.
pub struct Pane {
    pub tabs: Vec<Buffer>,
    pub tab: usize,
}

impl Pane {
    pub fn new() -> Self {
        Self { tabs: Vec::new(), tab: 0 }
    }

    pub fn buf(&self) -> Option<&Buffer> {
        self.tabs.get(self.tab)
    }

    pub fn buf_mut(&mut self) -> Option<&mut Buffer> {
        self.tabs.get_mut(self.tab)
    }
}

/// Ctrl+Tab switcher: an OS-app-switcher style overlay listing every
/// open file. Confirming swaps the pick into the last active pane.
pub struct Switcher {
    /// All open files as (pane, tab), left pane first.
    pub items: Vec<(usize, usize)>,
    pub selected: usize,
}

pub struct EditorState {
    pub root: PathBuf,
    /// Editor splits, left to right. At most two.
    pub panes: Vec<Pane>,
    pub active: usize,
    pub popup: Option<Popup>,
    pub files: Option<FilesPanel>,
    pub location: Option<LocationDropdown>,
    pub place: Option<PlacePicker>,
    pub switcher: Option<Switcher>,
    pub pending_path: Option<PathBuf>,
    /// Counts spawned terminals so each gets its own number.
    pub term_seq: usize,
    /// Left pane's share of the width, in percent (20..=80).
    pub split: u16,
}

pub struct App {
    pub config: Config,
    pub screen: Screen,
    pub tick: u64,
    pub should_quit: bool,
    pub quit_armed: bool,
    pub home: HomeState,
    pub editor: Option<EditorState>,
    pub toast: Option<(String, Instant)>,
    /// True when running inside the native window instead of a terminal.
    pub gui_mode: bool,
    /// Text waiting to be pushed to the clipboard by the window backend.
    pub clipboard_request: Option<String>,
    /// Clickable regions recorded during the last draw, for mouse support
    /// in the app window. Rebuilt every frame, in draw order.
    pub mouse_targets: Vec<MouseTarget>,
    /// Tab currently dragged with the mouse, as (pane, tab). Window mode only.
    pub drag: Option<(usize, usize)>,
    /// True while the pane divider is being dragged to resize.
    pub resize_drag: bool,
    /// Full rect of each editor pane from the last draw, for drag drops.
    pub pane_areas: Vec<Rect>,
    /// Grid cell currently under the mouse, when the backend knows it.
    pub hover: Option<(u16, u16)>,
    /// Watches whether any audio is playing, for the header wave.
    media: crate::media::MediaWatch,
    /// The shortcut-keys panel: browse bindings, press enter, press
    /// the new combination. Open from home (`k`) or `keys` in the popup.
    pub keys_editor: Option<KeysEditor>,
}

pub struct KeysEditor {
    pub selected: usize,
    /// True while waiting for the new combination / new command word.
    pub editing: bool,
    /// 0 = shortcut keys, 1 = terminal command words.
    pub tab: usize,
    /// The word being typed on the commands tab.
    pub input: String,
}

/// What each popup-terminal command does, for the customize panel.
pub fn command_action_help(action: &str) -> &'static str {
    match action {
        "files" => "toggle the folder panel",
        "open" => "open a file beside this one",
        "cd" => "switch project folder",
        "save" => "save the active file",
        "help" => "list the commands",
        "home" => "back to the start screen",
        "quit" => "quit silver",
        "clear" => "clear the popup terminal",
        "cat" => "rename / recolor your cat",
        "theme" => "change the accent color",
        "cursor" => "cursor style & blink",
        "spawn" => "a real terminal tab, right here",
        "run" => "run the active file's program",
        "keys" => "open this customize panel",
        _ => "",
    }
}

/// What each bindable action does, for the shortcuts panel.
pub fn key_action_help(action: &str) -> &'static str {
    match action {
        "popup_terminal" => "open the command popup",
        "save" => "save the active file",
        "files_panel" => "toggle the files panel",
        "location" => "open the location dropdown",
        "home" => "back to the home screen",
        "quit" => "quit silver",
        "switch_pane" => "jump to the other pane",
        "next_tab" => "next tab in this pane",
        "close_pane" => "close the shown tab",
        "open_right" => "pick a file to open here",
        "cycle_files" => "cycle / swap open files",
        "split_left" => "shrink the left pane",
        "split_right" => "grow the left pane",
        _ => "",
    }
}

/// A pressed key -> config combo text ("ctrl+shift+p"), only if the
/// combination round-trips through the parser. Plain keys without
/// ctrl/alt are refused: they'd fire while typing text.
fn combo_string(k: &KeyEvent) -> Option<String> {
    let code = match k.code {
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Char(c) => c.to_lowercase().to_string(),
        KeyCode::Enter => "enter".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::Up => "up".into(),
        KeyCode::Down => "down".into(),
        KeyCode::Left => "left".into(),
        KeyCode::Right => "right".into(),
        _ => return None,
    };
    if !k.modifiers.contains(KeyModifiers::CONTROL) && !k.modifiers.contains(KeyModifiers::ALT) {
        return None;
    }
    let mut s = String::new();
    if k.modifiers.contains(KeyModifiers::CONTROL) {
        s.push_str("ctrl+");
    }
    if k.modifiers.contains(KeyModifiers::ALT) {
        s.push_str("alt+");
    }
    if k.modifiers.contains(KeyModifiers::SHIFT) && !matches!(k.code, KeyCode::Char(_)) {
        s.push_str("shift+");
    }
    s.push_str(&code);
    crate::config::parse_key(&s).map(|_| s)
}

/// A screen region the mouse can interact with.
/// `first` is the index of the top visible row in scrolled lists.
pub enum MouseTarget {
    /// Text area of an editor pane: click moves the text cursor.
    EditorPane { pane: usize, area: Rect },
    /// Home side terminal: click focuses it.
    HomeTerminal { area: Rect },
    /// Home recents / folder browser list: click selects a row.
    HomeList { area: Rect, first: usize },
    /// Editor files panel list: click selects a row.
    FilesPanel { area: Rect, first: usize },
    /// Location dropdown list: click selects a row.
    Location { area: Rect, first: usize },
    /// A file tab in a pane's tab bar: click shows that file.
    Tab { pane: usize, tab: usize, area: Rect },
    /// Hover zone on the right edge: press to pick a file to show there.
    SplitZone { area: Rect },
    /// A card in the Ctrl+Tab switcher: click swaps that file in.
    SwitchItem { idx: usize, area: Rect },
    /// "open here" picker list: click selects a row, again activates it.
    PlacePanel { area: Rect, first: usize },
    /// Side chooser in the picker: click places the file on that side.
    PlaceSide { side: usize, area: Rect },
    /// The ▶ run button in the header: runs the active file's program.
    RunButton { area: Rect },
    /// A row in the shortcut-keys panel: click selects, click again edits.
    KeysRow { idx: usize, area: Rect },
}

const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", ".venv", "__pycache__", "dist"];
const MAX_TREE_ENTRIES: usize = 2000;

fn list_dir(dir: &Path, depth: usize, expanded: &BTreeSet<PathBuf>, out: &mut Vec<FileEntry>) {
    if out.len() >= MAX_TREE_ENTRIES {
        return;
    }
    let mut items: Vec<(PathBuf, bool)> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| {
                    let p = e.path();
                    let is_dir = p.is_dir();
                    (p, is_dir)
                })
                .collect()
        })
        .unwrap_or_default();
    items.retain(|(p, _)| {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        !name.starts_with('.') && !SKIP_DIRS.contains(&name.as_str())
    });
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for (path, is_dir) in items {
        if out.len() >= MAX_TREE_ENTRIES {
            return;
        }
        out.push(FileEntry { path: path.clone(), depth, is_dir });
        if is_dir && expanded.contains(&path) {
            list_dir(&path, depth + 1, expanded, out);
        }
    }
}

/// One level of a directory, dirs first.
fn read_dir_flat(dir: &Path) -> Vec<FileEntry> {
    let mut entries = Vec::new();
    list_dir(dir, 0, &BTreeSet::new(), &mut entries);
    entries
}

/// The shell command that runs a file's program, by extension.
/// Rust projects go through cargo when the root has a Cargo.toml.
fn run_command_for(path: &Path, root: &Path) -> Option<String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let p = path.display();
    let tmp_bin = std::env::temp_dir().join("silver_run");
    let bin = tmp_bin.display();
    match ext.as_str() {
        "rs" => Some(if root.join("Cargo.toml").exists() {
            "cargo run".into()
        } else {
            format!("rustc \"{p}\" -o \"{bin}\" && \"{bin}\"")
        }),
        "py" => Some(format!("python3 \"{p}\"")),
        "js" | "mjs" => Some(format!("node \"{p}\"")),
        "ts" => Some(format!("npx tsx \"{p}\"")),
        "sh" => Some(format!("sh \"{p}\"")),
        "go" => Some(format!("go run \"{p}\"")),
        "rb" => Some(format!("ruby \"{p}\"")),
        "lua" => Some(format!("lua \"{p}\"")),
        "php" => Some(format!("php \"{p}\"")),
        "c" => Some(format!("cc \"{p}\" -o \"{bin}\" && \"{bin}\"")),
        "cpp" | "cc" | "cxx" => Some(format!("c++ \"{p}\" -o \"{bin}\" && \"{bin}\"")),
        "java" => Some(format!("java \"{p}\"")),
        _ => None,
    }
}

/// Copy text to the system clipboard via the platform's clipboard command.
pub fn copy_to_clipboard(text: &str) -> bool {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    #[cfg(target_os = "macos")]
    let mut cmd = Command::new("pbcopy");
    #[cfg(target_os = "windows")]
    let mut cmd = Command::new("clip");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = {
        let mut c = Command::new("xclip");
        c.args(["-selection", "clipboard"]);
        c
    };

    let Ok(mut child) = cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
    else {
        return false;
    };
    let ok = child
        .stdin
        .take()
        .map(|mut sin| sin.write_all(text.as_bytes()).is_ok())
        .unwrap_or(false);
    child.wait().map(|s| s.success()).unwrap_or(false) && ok
}

impl App {
    pub fn new() -> Self {
        Self {
            config: Config::load(),
            screen: Screen::Home,
            tick: 0,
            should_quit: false,
            quit_armed: false,
            home: HomeState::default(),
            editor: None,
            toast: None,
            gui_mode: false,
            clipboard_request: None,
            mouse_targets: Vec::new(),
            drag: None,
            resize_drag: false,
            pane_areas: Vec::new(),
            hover: None,
            media: crate::media::MediaWatch::start(),
            keys_editor: None,
        }
    }

    /// True while some audio plays on the machine (checked every ~2s).
    pub fn media_playing(&self) -> bool {
        self.media.playing()
    }

    /// A mouse click at grid cell (x, y), hit-tested against the regions
    /// recorded during the last draw. Topmost (last drawn) wins.
    pub fn on_mouse_click(&mut self, x: u16, y: u16) {
        if self.home.customize.is_some() {
            return;
        }
        // The shortcuts panel: only its own rows react while open.
        if let Some(ke) = self.keys_editor.as_ref() {
            let was = ke.selected;
            let hit = self.mouse_targets.iter().find_map(|t| match t {
                MouseTarget::KeysRow { idx, area }
                    if x >= area.x
                        && x < area.x + area.width
                        && y >= area.y
                        && y < area.y + area.height =>
                {
                    Some(*idx)
                }
                _ => None,
            });
            if let Some(i) = hit {
                let seed = if ke.tab == 1 {
                    self.config.commands.values().nth(i).cloned().unwrap_or_default()
                } else {
                    String::new()
                };
                if let Some(ke) = self.keys_editor.as_mut() {
                    ke.editing = i == was; // second click starts the edit
                    ke.selected = i;
                    if ke.editing {
                        ke.input = seed;
                    }
                }
            }
            return;
        }
        if let Screen::Editor = self.screen {
            // The popup terminal grabs all input while open.
            if self.editor.as_ref().map(|e| e.popup.is_some()).unwrap_or(false) {
                return;
            }
        }
        // Actions that need `&mut self` run after the loop releases
        // its borrow of the target list.
        enum After {
            Zone,
            Side(usize),
            Activate,
            Switch(usize),
            Run,
        }
        let pos = GridPos { x, y };
        let mut after: Option<After> = None;
        let targets = std::mem::take(&mut self.mouse_targets);
        for t in targets.iter().rev() {
            match t {
                MouseTarget::Tab { pane, tab, area } if area.contains(pos) => {
                    if let Some(ed) = self.editor.as_mut() {
                        ed.active = *pane;
                        if let Some(p) = ed.panes.get_mut(*pane) {
                            if *tab < p.tabs.len() {
                                p.tab = *tab;
                            }
                        }
                    }
                    break;
                }
                MouseTarget::SwitchItem { idx, area } if area.contains(pos) => {
                    after = Some(After::Switch(*idx));
                    break;
                }
                MouseTarget::RunButton { area } if area.contains(pos) => {
                    after = Some(After::Run);
                    break;
                }
                MouseTarget::SplitZone { area } if area.contains(pos) => {
                    after = Some(After::Zone);
                    break;
                }
                MouseTarget::PlaceSide { side, area } if area.contains(pos) => {
                    after = Some(After::Side(*side));
                    break;
                }
                MouseTarget::PlacePanel { area, first } if area.contains(pos) => {
                    if let Some(pl) =
                        self.editor.as_mut().and_then(|e| e.place.as_mut())
                    {
                        let idx = first + (y - area.y) as usize;
                        if idx < pl.entries.len() {
                            if idx == pl.selected {
                                // Second press on a row activates it.
                                after = Some(After::Activate);
                            } else {
                                pl.selected = idx;
                            }
                        }
                    }
                    break;
                }
                MouseTarget::EditorPane { pane, area } if area.contains(pos) => {
                    if let Some(ed) = self.editor.as_mut() {
                        ed.active = *pane;
                        if let Some(buf) = ed.panes.get_mut(*pane).and_then(|p| p.buf_mut()) {
                            let row = (y - area.y) as usize;
                            let col = (x - area.x) as usize;
                            buf.cy = (buf.scroll + row).min(buf.lines.len().saturating_sub(1));
                            let line_len =
                                buf.lines.get(buf.cy).map(|l| l.chars().count()).unwrap_or(0);
                            buf.cx = (buf.hscroll + col).min(line_len);
                            buf.ensure_visible();
                        }
                    }
                    break;
                }
                MouseTarget::HomeTerminal { area } if area.contains(pos) => {
                    self.home.focus_terminal = true;
                    break;
                }
                MouseTarget::HomeList { area, first } if area.contains(pos) => {
                    self.home.focus_terminal = false;
                    let idx = first + (y - area.y) as usize;
                    if let Some(br) = self.home.browser.as_mut() {
                        if idx < br.entries.len() {
                            br.selected = idx;
                        }
                    } else if idx < self.config.recents.len() {
                        self.home.selected = idx;
                    }
                    break;
                }
                MouseTarget::FilesPanel { area, first } if area.contains(pos) => {
                    if let Some(ed) = self.editor.as_mut() {
                        if let Some(files) = ed.files.as_mut() {
                            let idx = first + (y - area.y) as usize;
                            if idx < files.entries.len() {
                                files.selected = idx;
                            }
                        }
                    }
                    break;
                }
                MouseTarget::Location { area, first } if area.contains(pos) => {
                    if let Some(ed) = self.editor.as_mut() {
                        if let Some(loc) = ed.location.as_mut() {
                            let idx = first + (y - area.y) as usize;
                            if idx < loc.entries.len() {
                                loc.selected = idx;
                            }
                        }
                    }
                    break;
                }
                _ => {}
            }
        }
        self.mouse_targets = targets;
        match after {
            Some(After::Zone) => self.open_place_picker(None),
            Some(After::Side(s)) => self.place_choose_side(s),
            Some(After::Activate) => self.place_activate(),
            Some(After::Switch(i)) => {
                if let Some(sw) = self.editor.as_mut().and_then(|e| e.switcher.as_mut()) {
                    if i < sw.items.len() {
                        sw.selected = i;
                    }
                }
                self.switcher_confirm();
            }
            Some(After::Run) => self.run_current_file(),
            None => {}
        }
    }

    /// Ctrl+Tab: open the switcher overlay on the next file, or step
    /// its highlight forward when it's already up.
    pub fn switch_next(&mut self) {
        let Some(ed) = self.editor.as_mut() else { return };
        if ed.popup.is_some() {
            return;
        }
        if let Some(sw) = ed.switcher.as_mut() {
            if !sw.items.is_empty() {
                sw.selected = (sw.selected + 1) % sw.items.len();
            }
            return;
        }
        let mut items: Vec<(usize, usize)> = Vec::new();
        for (pi, p) in ed.panes.iter().enumerate() {
            for ti in 0..p.tabs.len() {
                items.push((pi, ti));
            }
        }
        if items.len() < 2 {
            return;
        }
        let cur = ed
            .panes
            .get(ed.active)
            .map(|p| (ed.active, p.tab))
            .unwrap_or(items[0]);
        let idx = items.iter().position(|&x| x == cur).unwrap_or(0);
        let selected = (idx + 1) % items.len();
        ed.switcher = Some(Switcher { items, selected });
    }

    /// The switcher's pick: show it in the pane that was last active.
    /// A file shown on the other side trades places with the current
    /// one; a background file just comes over on top.
    fn switcher_confirm(&mut self) {
        let mut msg = None;
        if let Some(ed) = self.editor.as_mut() {
            if let Some(sw) = ed.switcher.take() {
                if let Some(&(pi, ti)) = sw.items.get(sw.selected) {
                    let a = ed.active;
                    if pi < ed.panes.len() && ti < ed.panes[pi].tabs.len() && a < ed.panes.len()
                    {
                        if pi == a {
                            ed.panes[a].tab = ti;
                        } else if ed.panes[pi].tab == ti {
                            // Both files are on screen: trade places, so
                            // the pick lands where you were working.
                            let shown = ed.panes[a].tab;
                            let picked = ed.panes[pi].tabs.remove(ti);
                            let current = ed.panes[a].tabs.remove(shown);
                            ed.panes[a].tabs.insert(shown, picked);
                            ed.panes[pi].tabs.insert(ti, current);
                        } else {
                            let picked = ed.panes[pi].tabs.remove(ti);
                            let p = &mut ed.panes[pi];
                            if p.tab >= p.tabs.len() {
                                p.tab = p.tabs.len().saturating_sub(1);
                            }
                            let d = &mut ed.panes[a];
                            d.tabs.push(picked);
                            d.tab = d.tabs.len() - 1;
                        }
                        let name = ed.panes[a].buf().map(|b| b.name()).unwrap_or_default();
                        let side = if a == 0 { "left" } else { "right" };
                        msg = Some(format!("showing {name} on the {side}"));
                    }
                }
            }
        }
        if let Some(m) = msg {
            self.toast(m);
        }
    }

    /// The tab under a grid cell, if any — used to start a mouse drag.
    pub fn tab_at(&self, x: u16, y: u16) -> Option<(usize, usize)> {
        let pos = GridPos { x, y };
        self.mouse_targets.iter().rev().find_map(|t| match t {
            MouseTarget::Tab { pane, tab, area } if area.contains(pos) => Some((*pane, *tab)),
            _ => None,
        })
    }

    /// Finish a tab drag at grid cell (x, y). Dropping on the other pane
    /// moves the tab there; dropping on the right half of a lone pane
    /// splits the view into two.
    pub fn drop_tab(&mut self, x: u16, y: u16) {
        let Some((src_pane, src_tab)) = self.drag.take() else { return };
        if self.editor.as_ref().map(|e| e.popup.is_some()).unwrap_or(true) {
            return;
        }
        let pos = GridPos { x, y };
        let dest = self.pane_areas.iter().position(|a| a.contains(pos));
        match dest {
            Some(p) if p != src_pane => self.move_tab(src_pane, src_tab, p),
            Some(p) if self.pane_areas.len() == 1 => {
                let area = self.pane_areas[p];
                let can_split = self
                    .editor
                    .as_ref()
                    .and_then(|e| e.panes.get(p))
                    .map(|pane| pane.tabs.len() > 1)
                    .unwrap_or(false);
                if can_split && x >= area.x + area.width / 2 {
                    self.move_tab(src_pane, src_tab, 1);
                }
            }
            _ => {}
        }
    }

    /// Move one tab to another pane (`dst == panes.len()` creates the
    /// second split). An emptied pane collapses away.
    fn move_tab(&mut self, src: usize, tab: usize, dst: usize) {
        let mut moved = None;
        if let Some(ed) = self.editor.as_mut() {
            let ok = src != dst
                && src < ed.panes.len()
                && tab < ed.panes[src].tabs.len()
                && dst <= ed.panes.len()
                && dst < 2;
            if ok {
                let buf = ed.panes[src].tabs.remove(tab);
                moved = Some(buf.name());
                let p = &mut ed.panes[src];
                if p.tab >= p.tabs.len() {
                    p.tab = p.tabs.len().saturating_sub(1);
                }
                if dst == ed.panes.len() {
                    ed.panes.push(Pane::new());
                }
                let d = &mut ed.panes[dst];
                d.tabs.push(buf);
                d.tab = d.tabs.len() - 1;
                ed.active = dst;
                if ed.panes[src].tabs.is_empty() && ed.panes.len() > 1 {
                    ed.panes.remove(src);
                    if src < ed.active {
                        ed.active -= 1;
                    }
                }
            }
        }
        if let Some(name) = moved {
            self.toast(format!("{name} moved"));
        }
    }

    /// Open the "put a file here" picker. `target` fixes the pane it
    /// fills; None decides on pick (and asks when both panes are busy).
    pub fn open_place_picker(&mut self, target: Option<usize>) {
        let Some(ed) = self.editor.as_mut() else { return };
        let expanded = BTreeSet::new();
        let mut entries = Vec::new();
        list_dir(&ed.root, 0, &expanded, &mut entries);
        ed.place = Some(PlacePicker {
            selected: 0,
            expanded,
            entries,
            target,
            choosing: None,
            side: 1,
        });
        ed.files = None;
        ed.location = None;
    }

    /// Activate the picker's selected row: folders expand, files get
    /// placed (or queue the left/right question when both panes are busy).
    fn place_activate(&mut self) {
        let mut pick: Option<PathBuf> = None;
        {
            let Some(ed) = self.editor.as_mut() else { return };
            let Some(pl) = ed.place.as_mut() else { return };
            if let Some(entry) = pl.entries.get(pl.selected).cloned() {
                if entry.is_dir {
                    if !pl.expanded.remove(&entry.path) {
                        pl.expanded.insert(entry.path.clone());
                    }
                    let mut entries = Vec::new();
                    list_dir(&ed.root, 0, &pl.expanded, &mut entries);
                    pl.entries = entries;
                    pl.selected = pl
                        .entries
                        .iter()
                        .position(|e| e.path == entry.path)
                        .unwrap_or(0);
                } else {
                    pick = Some(entry.path);
                }
            }
        }
        if let Some(path) = pick {
            self.pick_place_file(path);
        }
    }

    /// A file was picked: place it right away when the target is known
    /// or a side is free, otherwise ask which side to replace.
    fn pick_place_file(&mut self, path: PathBuf) {
        let (target, busy) = {
            let Some(ed) = self.editor.as_ref() else { return };
            (ed.place.as_ref().and_then(|p| p.target), ed.panes.len() >= 2)
        };
        match target {
            Some(p) => self.place_file(&path, p),
            None if !busy => self.place_file(&path, 1),
            None => {
                if let Some(pl) = self.editor.as_mut().and_then(|e| e.place.as_mut()) {
                    pl.choosing = Some(path);
                    pl.side = 1;
                }
            }
        }
    }

    /// The left/right question was answered (0 = left, 1 = right).
    fn place_choose_side(&mut self, side: usize) {
        let path = self
            .editor
            .as_mut()
            .and_then(|e| e.place.as_mut())
            .and_then(|p| p.choosing.take());
        if let Some(path) = path {
            self.place_file(&path, side);
        }
    }

    /// Show `path` in pane `dst` (0 = left, 1 = right). A file that's
    /// already open moves over instead of reopening; anything else
    /// opens as a new tab on top of that pane's stack.
    pub fn place_file(&mut self, path: &Path, dst: usize) {
        let canon = match path.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                self.toast(format!("cannot open: {e}"));
                return;
            }
        };
        if canon.is_dir() {
            self.toast("that's a folder — pick a file");
            return;
        }
        let mut msg = None;
        let mut retrieve = None;
        if let Some(ed) = self.editor.as_mut() {
            ed.place = None;
            ed.files = None;
            ed.location = None;
            if ed.panes.is_empty() {
                ed.panes.push(Pane::new());
            }
            let dst = dst.min(ed.panes.len()).min(1);
            let found = ed.panes.iter().enumerate().find_map(|(pi, p)| {
                p.tabs.iter().position(|b| b.path == canon).map(|ti| (pi, ti))
            });
            match found {
                Some((pi, ti)) if pi == dst => {
                    ed.panes[pi].tab = ti;
                    ed.active = pi;
                }
                Some((pi, ti)) => retrieve = Some((pi, ti, dst)),
                None => match Buffer::open(&canon) {
                    Ok(buf) => {
                        let name = buf.name();
                        if dst == ed.panes.len() {
                            ed.panes.push(Pane::new());
                        }
                        let p = &mut ed.panes[dst];
                        p.tabs.push(buf);
                        p.tab = p.tabs.len() - 1;
                        ed.active = dst;
                        msg = Some(format!(
                            "showing {name} on the {}",
                            if dst == 0 { "left" } else { "right" }
                        ));
                    }
                    Err(e) => msg = Some(format!("open failed: {e}")),
                },
            }
        }
        if let Some((pi, ti, d)) = retrieve {
            self.move_tab(pi, ti, d);
        }
        if let Some(m) = msg {
            self.toast(m);
        }
    }

    /// `spawn`: a real terminal tab right where you're working; the
    /// file that was there is sent to the other side, still on screen.
    pub fn spawn_terminal(&mut self) {
        let mut displaced: Option<(usize, usize, usize)> = None;
        {
            let Some(ed) = self.editor.as_mut() else { return };
            if ed.panes.is_empty() {
                ed.panes.push(Pane::new());
            }
            let a = ed.active.min(ed.panes.len() - 1);
            ed.term_seq += 1;
            let term = Buffer::terminal(ed.term_seq, ed.root.clone());
            let shown = if ed.panes[a].tabs.is_empty() { None } else { Some(ed.panes[a].tab) };
            let p = &mut ed.panes[a];
            p.tabs.push(term);
            p.tab = p.tabs.len() - 1;
            ed.active = a;
            ed.popup = None;
            if let Some(ti) = shown {
                displaced = Some((a, ti, 1 - a.min(1)));
            }
        }
        if let Some((pane, tab, other)) = displaced {
            self.move_tab(pane, tab, other);
            // Stay on the new terminal, not the moved file.
            if let Some(ed) = self.editor.as_mut() {
                ed.active = pane.min(ed.panes.len() - 1);
            }
        }
        self.toast("terminal spawned — a real shell, right where you were");
    }

    /// ▶ run: the active file's program, in a terminal on the other
    /// side (reusing one that's already open).
    pub fn run_current_file(&mut self) {
        let file = {
            let Some(ed) = self.editor.as_ref() else { return };
            ed.panes
                .get(ed.active)
                .and_then(|p| p.buf())
                .filter(|b| b.term.is_none())
                .map(|b| b.path.clone())
        };
        let Some(path) = file else {
            self.toast("nothing to run here — open a file first");
            return;
        };
        let root = self.editor.as_ref().map(|e| e.root.clone()).unwrap_or_default();
        let Some(cmd) = run_command_for(&path, &root) else {
            let ext = path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
            self.toast(format!("don't know how to run .{ext} — use a terminal (`spawn`)"));
            return;
        };
        let (pi, ti) = self.ensure_terminal();
        if let Some(ed) = self.editor.as_mut() {
            if let Some(pane) = ed.panes.get_mut(pi) {
                pane.tab = ti;
                if let Some(t) = pane.tabs.get_mut(ti).and_then(|b| b.term.as_mut()) {
                    if t.is_running() {
                        t.interrupt();
                    }
                    t.exec(&cmd);
                }
            }
        }
        self.toast(format!("running: {cmd}"));
    }

    /// A terminal to run things in: the first one already open, or a
    /// fresh one on the side opposite the active file.
    fn ensure_terminal(&mut self) -> (usize, usize) {
        let Some(ed) = self.editor.as_mut() else { return (0, 0) };
        if ed.panes.is_empty() {
            ed.panes.push(Pane::new());
        }
        let existing = ed.panes.iter().enumerate().find_map(|(pi, p)| {
            p.tabs.iter().position(|b| b.term.is_some()).map(|ti| (pi, ti))
        });
        if let Some(found) = existing {
            return found;
        }
        let a = ed.active.min(ed.panes.len() - 1);
        let dst = (1 - a.min(1)).min(ed.panes.len());
        if dst == ed.panes.len() {
            ed.panes.push(Pane::new());
        }
        ed.term_seq += 1;
        let term = Buffer::terminal(ed.term_seq, ed.root.clone());
        let p = &mut ed.panes[dst];
        p.tabs.push(term);
        p.tab = p.tabs.len() - 1;
        (dst, p.tabs.len() - 1)
    }

    /// Copy text to the clipboard in whatever mode we're running in:
    /// the terminal spawns the OS clipboard command, the window backend
    /// picks up `clipboard_request` and uses the native clipboard.
    /// Returns false only when no route worked.
    fn request_copy(&mut self, text: String) -> bool {
        let os_ok = copy_to_clipboard(&text);
        if self.gui_mode {
            self.clipboard_request = Some(text);
            return true;
        }
        os_ok
    }

    /// Push the configured cursor shape to the real terminal (TUI only).
    pub fn apply_cursor_style(&self) {
        if self.gui_mode {
            return;
        }
        use ratatui::crossterm::cursor::SetCursorStyle;
        use ratatui::crossterm::execute;
        let style = match (self.config.theme.cursor.as_str(), self.config.theme.cursor_blink) {
            ("bar", true) => SetCursorStyle::BlinkingBar,
            ("bar", false) => SetCursorStyle::SteadyBar,
            ("underline", true) => SetCursorStyle::BlinkingUnderScore,
            ("underline", false) => SetCursorStyle::SteadyUnderScore,
            ("block", true) => SetCursorStyle::BlinkingBlock,
            ("block", false) => SetCursorStyle::SteadyBlock,
            // The terminal has no hollow cursor; fall back to the user's default.
            _ => SetCursorStyle::DefaultUserShape,
        };
        let _ = execute!(io::stdout(), style);
    }

    /// Handle `cursor ...` typed in either terminal; returns the reply line.
    pub fn cursor_command(&mut self, rest: &str) -> String {
        let v = rest.trim().to_lowercase();
        match v.as_str() {
            "block" | "bar" | "underline" | "hollow" => {
                self.config.theme.cursor = v.clone();
                self.config.save();
                self.apply_cursor_style();
                format!("cursor style set to {v}")
            }
            "blink on" | "blink off" => {
                let on = v.ends_with("on");
                self.config.theme.cursor_blink = on;
                self.config.save();
                self.apply_cursor_style();
                format!("cursor blink {}", if on { "on" } else { "off" })
            }
            _ => "usage: cursor block|bar|underline|hollow · cursor blink on|off".into(),
        }
    }

    pub fn run(mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        use ratatui::crossterm::event::{
            DisableMouseCapture, EnableMouseCapture, MouseButton, MouseEventKind,
        };
        use ratatui::crossterm::execute;
        // Mouse support in the terminal: clicks, hover, and the wheel.
        let _ = execute!(io::stdout(), EnableMouseCapture);
        self.apply_cursor_style();
        while !self.should_quit {
            terminal.draw(|f| ui::draw(f, &mut self))?;
            if event::poll(Duration::from_millis(120))? {
                match event::read()? {
                    Event::Key(k) if k.kind == KeyEventKind::Press => self.on_key(k),
                    Event::Mouse(m) => match m.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            if self.divider_hit(m.column, m.row) {
                                self.resize_drag = true;
                            } else {
                                self.on_mouse_click(m.column, m.row)
                            }
                        }
                        MouseEventKind::Drag(MouseButton::Left) if self.resize_drag => {
                            self.set_split_to(m.column)
                        }
                        MouseEventKind::Moved | MouseEventKind::Drag(_) => {
                            self.hover = Some((m.column, m.row))
                        }
                        MouseEventKind::Up(_) => self.resize_drag = false,
                        MouseEventKind::ScrollUp => self.on_scroll(1),
                        MouseEventKind::ScrollDown => self.on_scroll(-1),
                        _ => {}
                    },
                    _ => {}
                }
            }
            self.tick_update();
        }
        let _ = execute!(io::stdout(), DisableMouseCapture);
        Ok(())
    }

    /// True when (x, y) sits on the border between two panes, where a
    /// drag resizes the split.
    pub fn divider_hit(&self, x: u16, y: u16) -> bool {
        if self.pane_areas.len() < 2 {
            return false;
        }
        let r = self.pane_areas[1];
        y >= r.y && y < r.y + r.height && (x == r.x || x + 1 == r.x)
    }

    /// Drag the divider to column `x`: the split follows the mouse.
    pub fn set_split_to(&mut self, x: u16) {
        if self.pane_areas.len() < 2 {
            return;
        }
        let l = self.pane_areas[0];
        let total = (l.width + self.pane_areas[1].width) as i32;
        if total <= 0 {
            return;
        }
        let pct = ((x.saturating_sub(l.x)) as i32 * 100 / total).clamp(20, 80);
        if let Some(ed) = self.editor.as_mut() {
            ed.split = pct as u16;
        }
    }

    /// Nudge the split by keyboard (alt+left / alt+right).
    pub fn adjust_split(&mut self, delta: i16) {
        let Some(ed) = self.editor.as_mut() else { return };
        if ed.panes.len() < 2 {
            return;
        }
        ed.split = (ed.split as i16 + delta).clamp(20, 80) as u16;
    }

    /// Route mouse-wheel lines to whatever is on screen.
    /// Positive `lines` scrolls up (towards older content).
    pub fn on_scroll(&mut self, lines: i32) {
        if lines == 0 {
            return;
        }
        let arrows = |app: &mut App, lines: i32| {
            let code = if lines > 0 { KeyCode::Up } else { KeyCode::Down };
            for _ in 0..lines.unsigned_abs().min(20) {
                app.on_key(KeyEvent::new(code, KeyModifiers::empty()));
            }
        };
        match self.screen {
            Screen::Home => {
                if self.home.focus_terminal {
                    self.scroll_home_terminal(lines);
                } else {
                    arrows(self, lines);
                }
            }
            Screen::Editor => {
                let popup_open =
                    self.editor.as_ref().map(|e| e.popup.is_some()).unwrap_or(false);
                if popup_open {
                    self.scroll_popup_terminal(lines);
                    return;
                }
                // The wheel over a terminal tab scrolls its history.
                let term = self.editor.as_mut().and_then(|e| {
                    let a = e.active;
                    e.panes.get_mut(a).and_then(|p| p.buf_mut()).and_then(|b| b.term.as_mut())
                });
                if let Some(term) = term {
                    let cur = term.scroll as i32;
                    term.scroll = (cur + lines).max(0) as usize;
                } else {
                    arrows(self, lines);
                }
            }
        }
    }

    /// Advance animations and expire toasts; called ~8 times a second
    /// by both the terminal loop and the windowed app.
    pub fn tick_update(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        if let Some((_, at)) = &self.toast {
            if at.elapsed() > Duration::from_secs(3) {
                self.toast = None;
            }
        }
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    pub fn on_key(&mut self, k: KeyEvent) {
        // The shortcuts panel takes every key while open, so the new
        // combination can be captured without side effects.
        if self.keys_editor.is_some() {
            self.on_key_keys(k);
            return;
        }
        if self.config.key_is("quit", &k) {
            let dirty = self
                .editor
                .as_ref()
                .map(|e| e.panes.iter().flat_map(|p| p.tabs.iter()).any(|b| b.dirty))
                .unwrap_or(false);
            if dirty && !self.quit_armed {
                self.quit_armed = true;
                self.toast("unsaved changes — Ctrl+S to save, or press quit again to discard");
            } else {
                self.should_quit = true;
            }
            return;
        }
        self.quit_armed = false;

        match self.screen {
            Screen::Home => self.on_key_home(k),
            Screen::Editor => self.on_key_editor(k),
        }
    }

    // ---------- shortcut keys panel ----------

    pub fn open_keys_editor(&mut self) {
        self.keys_editor =
            Some(KeysEditor { selected: 0, editing: false, tab: 0, input: String::new() });
    }

    fn on_key_keys(&mut self, k: KeyEvent) {
        let (selected, editing, tab) = match &self.keys_editor {
            Some(ke) => (ke.selected, ke.editing, ke.tab),
            None => return,
        };
        let n = if tab == 0 { self.config.keys.len() } else { self.config.commands.len() }.max(1);

        // Commands tab: the new word is typed, not pressed.
        if editing && tab == 1 {
            match k.code {
                KeyCode::Esc => {
                    if let Some(ke) = self.keys_editor.as_mut() {
                        ke.editing = false;
                        ke.input.clear();
                    }
                }
                KeyCode::Backspace => {
                    if let Some(ke) = self.keys_editor.as_mut() {
                        ke.input.pop();
                    }
                }
                KeyCode::Char(c) if !c.is_whitespace() && !k.modifiers.contains(KeyModifiers::CONTROL) => {
                    if let Some(ke) = self.keys_editor.as_mut() {
                        if ke.input.chars().count() < 16 {
                            ke.input.push(c.to_ascii_lowercase());
                        }
                    }
                }
                KeyCode::Enter => {
                    let word = self
                        .keys_editor
                        .as_ref()
                        .map(|ke| ke.input.trim().to_string())
                        .unwrap_or_default();
                    if word.is_empty() {
                        self.toast("type the new command word first");
                        return;
                    }
                    let Some(action) = self.config.commands.keys().nth(selected).cloned() else {
                        return;
                    };
                    // One word, one meaning.
                    let taken = self
                        .config
                        .commands
                        .iter()
                        .find(|(a, w)| **a != action && **w == word)
                        .map(|(a, _)| a.clone());
                    if let Some(other) = taken {
                        self.toast(format!("`{word}` already means \"{other}\""));
                        return;
                    }
                    self.config.commands.insert(action.clone(), word.clone());
                    self.config.save();
                    if let Some(ke) = self.keys_editor.as_mut() {
                        ke.editing = false;
                        ke.input.clear();
                    }
                    self.toast(format!("{action} → `{word}`"));
                }
                _ => {}
            }
            return;
        }

        if editing {
            if k.code == KeyCode::Esc {
                if let Some(ke) = self.keys_editor.as_mut() {
                    ke.editing = false;
                }
                return;
            }
            let Some(combo) = combo_string(&k) else {
                self.toast("hold ctrl or alt with a key — plain keys would fire while typing");
                return;
            };
            let Some(action) = self.config.keys.keys().nth(selected).cloned() else {
                return;
            };
            // A combo can only mean one thing.
            let taken = self
                .config
                .keys
                .iter()
                .find(|(a, v)| **a != action && **v == combo)
                .map(|(a, _)| a.clone());
            if let Some(other) = taken {
                self.toast(format!("{combo} already does \"{}\"", other.replace('_', " ")));
                return;
            }
            self.config.keys.insert(action.clone(), combo.clone());
            self.config.save();
            if let Some(ke) = self.keys_editor.as_mut() {
                ke.editing = false;
            }
            self.toast(format!("{} → {combo}", action.replace('_', " ")));
            return;
        }

        match k.code {
            KeyCode::Esc | KeyCode::Char('q') => self.keys_editor = None,
            KeyCode::Tab => {
                if let Some(ke) = self.keys_editor.as_mut() {
                    ke.tab = 1 - ke.tab;
                    ke.selected = 0;
                }
            }
            KeyCode::Up => {
                if let Some(ke) = self.keys_editor.as_mut() {
                    ke.selected = ke.selected.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if let Some(ke) = self.keys_editor.as_mut() {
                    ke.selected = (ke.selected + 1).min(n - 1);
                }
            }
            KeyCode::Enter | KeyCode::Char('e') => {
                // Commands start from the current word, ready to tweak.
                let seed = if tab == 1 {
                    self.config.commands.values().nth(selected).cloned().unwrap_or_default()
                } else {
                    String::new()
                };
                if let Some(ke) = self.keys_editor.as_mut() {
                    ke.editing = true;
                    ke.input = seed;
                }
            }
            // Back to the factory default for the selected action.
            KeyCode::Char('d') => {
                let (map, defaults) = if tab == 0 {
                    (&self.config.keys, crate::config::default_keys())
                } else {
                    (&self.config.commands, crate::config::default_commands())
                };
                let Some(action) = map.keys().nth(selected).cloned() else {
                    return;
                };
                let Some(def) = defaults.get(&action).cloned() else {
                    return;
                };
                let clash = map.iter().any(|(a, v)| *a != action && *v == def);
                if clash {
                    self.toast(format!("{def} is taken by another action now"));
                    return;
                }
                if tab == 0 {
                    self.config.keys.insert(action.clone(), def.clone());
                } else {
                    self.config.commands.insert(action.clone(), def.clone());
                }
                self.config.save();
                self.toast(format!("{} → {def} (default)", action.replace('_', " ")));
            }
            _ => {}
        }
    }

    // ---------- home screen ----------

    fn on_key_home(&mut self, k: KeyEvent) {
        if self.home.customize.is_some() {
            self.on_key_customize(k);
            return;
        }

        match k.code {
            KeyCode::Tab => {
                self.home.focus_terminal = !self.home.focus_terminal;
                return;
            }
            _ => {}
        }

        if self.home.focus_terminal {
            match k.code {
                KeyCode::Enter => {
                    self.home.term_scroll = 0;
                    let cmd = std::mem::take(&mut self.home.term_input);
                    self.run_home_command(&cmd);
                }
                KeyCode::Backspace => {
                    self.home.term_input.pop();
                }
                KeyCode::Esc => self.home.focus_terminal = false,
                KeyCode::PageUp => self.scroll_home_terminal(5),
                KeyCode::PageDown => self.scroll_home_terminal(-5),
                KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.home.term_input.push(c);
                }
                _ => {}
            }
            return;
        }

        if self.home.browser.is_some() {
            self.on_key_browser(k);
            return;
        }

        match k.code {
            KeyCode::Up => self.home.selected = self.home.selected.saturating_sub(1),
            KeyCode::Down => {
                if !self.config.recents.is_empty() {
                    self.home.selected =
                        (self.home.selected + 1).min(self.config.recents.len() - 1);
                }
            }
            KeyCode::Enter => {
                if let Some(r) = self.config.recents.get(self.home.selected) {
                    let path = PathBuf::from(&r.path);
                    self.open_project(&path);
                }
            }
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('k') => self.open_keys_editor(),
            _ => {
                if self.config.key_is("customize", &k) || k.code == KeyCode::Char('c') {
                    let idx = CAT_COLORS
                        .iter()
                        .position(|c| *c == self.config.cat.color)
                        .unwrap_or(0);
                    self.home.customize = Some(CustomizeState {
                        name: self.config.cat.name.clone(),
                        color_idx: idx,
                    });
                }
            }
        }
    }

    fn on_key_browser(&mut self, k: KeyEvent) {
        let mut copied: Option<PathBuf> = None;
        let mut start_dir: Option<PathBuf> = None;
        {
            let Some(br) = self.home.browser.as_mut() else { return };
            match k.code {
                KeyCode::Esc => {
                    self.home.browser = None;
                    return;
                }
                KeyCode::Up => br.selected = br.selected.saturating_sub(1),
                KeyCode::Down => {
                    if !br.entries.is_empty() {
                        br.selected = (br.selected + 1).min(br.entries.len() - 1);
                    }
                }
                KeyCode::Backspace | KeyCode::Left => {
                    if let Some(parent) = br.dir.parent().map(|p| p.to_path_buf()) {
                        br.dir = parent;
                        br.entries = read_dir_flat(&br.dir);
                        br.selected = 0;
                    }
                }
                KeyCode::Enter => {
                    if let Some(entry) = br.entries.get(br.selected).cloned() {
                        copied = Some(entry.path.clone());
                        if entry.is_dir {
                            br.dir = entry.path;
                            br.entries = read_dir_flat(&br.dir);
                            br.selected = 0;
                        }
                    }
                }
                KeyCode::Char('s') => {
                    // Start the session from the highlighted folder,
                    // or from the current folder if a file is highlighted.
                    start_dir = match br.entries.get(br.selected) {
                        Some(e) if e.is_dir => Some(e.path.clone()),
                        _ => Some(br.dir.clone()),
                    };
                }
                KeyCode::Char('q') => {
                    self.should_quit = true;
                    return;
                }
                _ => {}
            }
        }
        if let Some(path) = copied {
            let shown = path.display().to_string();
            if self.request_copy(shown.clone()) {
                self.toast(format!("copied to clipboard: {shown}"));
            } else {
                self.toast("could not reach the system clipboard");
            }
        }
        if let Some(dir) = start_dir {
            self.open_project(&dir);
        }
    }

    fn on_key_customize(&mut self, k: KeyEvent) {
        let Some(cust) = self.home.customize.as_mut() else { return };
        match k.code {
            KeyCode::Esc => self.home.customize = None,
            KeyCode::Enter => {
                let cust = self.home.customize.take().unwrap();
                if !cust.name.trim().is_empty() {
                    self.config.cat.name = cust.name.trim().to_string();
                }
                self.config.cat.color = CAT_COLORS[cust.color_idx].to_string();
                self.config.save();
                self.toast(format!("saved — say hi to {}!", self.config.cat.name));
            }
            KeyCode::Left => {
                cust.color_idx = (cust.color_idx + CAT_COLORS.len() - 1) % CAT_COLORS.len();
            }
            KeyCode::Right => cust.color_idx = (cust.color_idx + 1) % CAT_COLORS.len(),
            KeyCode::Backspace => {
                cust.name.pop();
            }
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                if cust.name.chars().count() < 16 {
                    cust.name.push(c);
                }
            }
            _ => {}
        }
    }

    fn home_println(&mut self, s: impl Into<String>) {
        self.home.term_output.push(s.into());
        let len = self.home.term_output.len();
        if len > 500 {
            self.home.term_output.drain(0..len - 500);
        }
    }

    /// Scroll the home terminal output; positive = older lines.
    pub fn scroll_home_terminal(&mut self, delta: i32) {
        let max = self.home.term_output.len();
        let cur = self.home.term_scroll as i32;
        self.home.term_scroll = (cur + delta).clamp(0, max as i32) as usize;
    }

    /// Scroll the popup terminal output; positive = older lines.
    pub fn scroll_popup_terminal(&mut self, delta: i32) {
        if let Some(ed) = self.editor.as_mut() {
            if let Some(popup) = ed.popup.as_mut() {
                let max = popup.lines.len() as i32;
                popup.scroll = (popup.scroll as i32 + delta).clamp(0, max) as usize;
            }
        }
    }

    fn run_home_command(&mut self, cmd: &str) {
        let cmd = cmd.trim().to_string();
        if cmd.is_empty() {
            return;
        }
        self.home_println(format!("~ $ {cmd}"));
        let mut parts = cmd.splitn(2, char::is_whitespace);
        let head = parts.next().unwrap_or("").to_lowercase();
        let rest = parts.next().unwrap_or("").trim().to_string();
        match head.as_str() {
            "cd" => {
                if rest.is_empty() {
                    self.home.browser = None;
                    self.home_println("  back to recent projects");
                } else {
                    self.home_cd(&rest);
                }
            }
            "start" | "s" => {
                let target = if rest.is_empty() {
                    self.home.browser.as_ref().map(|br| br.dir.clone())
                } else {
                    Some(self.resolve_home_path(&rest))
                };
                match target {
                    Some(dir) => self.open_project(&dir),
                    None => {
                        self.home_println("  start <path>, or `cd <path>` first and then `start`")
                    }
                }
            }
            "ls" => {
                let dir = if rest.is_empty() {
                    self.home
                        .browser
                        .as_ref()
                        .map(|br| br.dir.clone())
                        .unwrap_or_else(|| expand_tilde("~"))
                } else {
                    self.resolve_home_path(&rest)
                };
                match std::fs::read_dir(&dir) {
                    Ok(rd) => {
                        let mut names: Vec<String> = rd
                            .filter_map(|e| e.ok())
                            .map(|e| {
                                let mut n = e.file_name().to_string_lossy().to_string();
                                if e.path().is_dir() {
                                    n.push('/');
                                }
                                n
                            })
                            .filter(|n| !n.starts_with('.'))
                            .collect();
                        names.sort();
                        for chunk in names.chunks(4) {
                            self.home_println(format!("  {}", chunk.join("   ")));
                        }
                    }
                    Err(e) => self.home_println(format!("  ls: {e}")),
                }
            }
            "cursor" => {
                let msg = self.cursor_command(&rest);
                self.home_println(format!("  {msg}"));
            }
            "help" | "?" => {
                self.home_println("  cd <path> ..... browse a folder in the left panel");
                self.home_println("  cd ............ back to recent projects");
                self.home_println("  start [path] .. open the editor session there");
                self.home_println("  ls [path] ..... list a folder");
                self.home_println("  cursor <s> .... block | bar | underline | hollow");
                self.home_println("  exit .......... quit silver");
            }
            "exit" | "quit" | "q" => self.should_quit = true,
            "clear" | "cls" => self.home.term_output.clear(),
            _ => self.home_println(format!("  unknown: {head}  (try `help`)")),
        }
    }

    /// Resolve a typed path: `~` expands, relative paths follow the browser dir.
    fn resolve_home_path(&self, raw: &str) -> PathBuf {
        let path = expand_tilde(raw);
        if path.is_absolute() {
            return path;
        }
        match &self.home.browser {
            Some(br) => br.dir.join(path),
            None => expand_tilde("~").join(path),
        }
    }

    fn home_cd(&mut self, raw: &str) {
        let path = self.resolve_home_path(raw);
        match path.canonicalize() {
            Ok(p) if p.is_dir() => {
                self.home_println(format!("  now in {}", p.display()));
                self.home_println("  `start` opens the editor here · enter copies a path");
                let entries = read_dir_flat(&p);
                self.home.browser = Some(HomeBrowser { dir: p, selected: 0, entries });
            }
            Ok(_) => {
                self.home_println(format!("  not a folder: {}", path.display()));
            }
            Err(e) => self.home_println(format!("  cd: {e}")),
        }
    }

    pub fn open_project(&mut self, path: &Path) {
        let canon = match path.canonicalize() {
            Ok(p) if p.is_dir() => p,
            Ok(_) => {
                self.home_println(format!("  not a folder: {}", path.display()));
                self.toast("that path is a file — give me a folder");
                return;
            }
            Err(e) => {
                self.home_println(format!("  cd: {e}"));
                self.toast(format!("cannot open: {}", path.display()));
                return;
            }
        };
        self.config.add_recent(&canon);
        self.editor = Some(EditorState {
            root: canon,
            panes: vec![Pane::new()],
            active: 0,
            popup: None,
            files: None,
            location: None,
            place: None,
            switcher: None,
            pending_path: None,
            term_seq: 0,
            split: 50,
        });
        self.screen = Screen::Editor;
        self.toast("project opened — Ctrl+T then `ls` to browse files");
    }

    // ---------- editor screen ----------

    fn on_key_editor(&mut self, k: KeyEvent) {
        // Global editor shortcuts first.
        if self.config.key_is("popup_terminal", &k) {
            if let Some(ed) = self.editor.as_mut() {
                if ed.popup.is_some() {
                    ed.popup = None;
                } else {
                    ed.popup = Some(Popup { input: String::new(), lines: Vec::new(), scroll: 0 });
                }
            }
            return;
        }
        if self.config.key_is("save", &k) {
            self.save_active();
            return;
        }
        if self.config.key_is("files_panel", &k) {
            self.toggle_files_panel();
            return;
        }
        if self.config.key_is("open_right", &k) {
            let open = self.editor.as_ref().map(|e| e.place.is_some()).unwrap_or(false);
            if open {
                if let Some(ed) = self.editor.as_mut() {
                    ed.place = None;
                }
            } else {
                self.open_place_picker(None);
            }
            return;
        }
        if self.config.key_is("location", &k) {
            self.toggle_location();
            return;
        }
        if self.config.key_is("home", &k) {
            self.screen = Screen::Home;
            return;
        }
        if self.config.key_is("switch_pane", &k) {
            if let Some(ed) = self.editor.as_mut() {
                if ed.panes.len() > 1 {
                    ed.active = (ed.active + 1) % ed.panes.len();
                }
            }
            return;
        }
        if self.config.key_is("cycle_files", &k) {
            self.switch_next();
            return;
        }
        if self.config.key_is("split_left", &k) {
            self.adjust_split(-5);
            return;
        }
        if self.config.key_is("split_right", &k) {
            self.adjust_split(5);
            return;
        }
        if self.config.key_is("next_tab", &k) {
            if let Some(ed) = self.editor.as_mut() {
                if let Some(p) = ed.panes.get_mut(ed.active) {
                    if p.tabs.len() > 1 {
                        p.tab = (p.tab + 1) % p.tabs.len();
                    }
                }
            }
            return;
        }
        if self.config.key_is("close_pane", &k) {
            if let Some(ed) = self.editor.as_mut() {
                if let Some(p) = ed.panes.get_mut(ed.active) {
                    if !p.tabs.is_empty() {
                        p.tabs.remove(p.tab);
                        if p.tab >= p.tabs.len() && p.tab > 0 {
                            p.tab -= 1;
                        }
                    }
                    // An emptied second pane collapses away.
                    if p.tabs.is_empty() && ed.panes.len() > 1 {
                        ed.panes.remove(ed.active);
                        if ed.active >= ed.panes.len() {
                            ed.active = ed.panes.len() - 1;
                        }
                    }
                }
            }
            return;
        }

        let Some(ed) = self.editor.as_mut() else { return };

        // The switcher grabs navigation while open.
        if ed.switcher.is_some() {
            self.on_key_switcher(k);
            return;
        }
        // Popup terminal grabs input while open.
        if ed.popup.is_some() {
            self.on_key_popup(k);
            return;
        }
        // Then the "open here" picker.
        if ed.place.is_some() {
            self.on_key_place(k);
            return;
        }
        // Then the files panel.
        if ed.files.is_some() {
            self.on_key_files(k);
            return;
        }
        // Then the location dropdown.
        if ed.location.is_some() {
            self.on_key_location(k);
            return;
        }

        // Plain editing.
        let Some(buf) = ed.panes.get_mut(ed.active).and_then(|p| p.buf_mut()) else { return };
        // A terminal tab: keys go straight to the shell, like a real
        // terminal. PageUp/PageDown scroll back through history.
        if let Some(term) = buf.term.as_mut() {
            match k.code {
                KeyCode::PageUp => term.scroll += 5,
                KeyCode::PageDown => term.scroll = term.scroll.saturating_sub(5),
                KeyCode::Char(c) if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    // ctrl+a..z become control bytes (ctrl+c = 0x03).
                    let lc = c.to_ascii_lowercase();
                    if lc.is_ascii_lowercase() {
                        term.send(&[lc as u8 - b'a' + 1]);
                    }
                }
                KeyCode::Char(c) => {
                    let mut b = [0u8; 4];
                    term.send(c.encode_utf8(&mut b).as_bytes());
                }
                KeyCode::Enter => term.send(b"\r"),
                KeyCode::Backspace => term.send(&[0x7f]),
                KeyCode::Tab => term.send(b"\t"),
                KeyCode::Esc => term.send(b"\x1b"),
                KeyCode::Up => term.send(b"\x1b[A"),
                KeyCode::Down => term.send(b"\x1b[B"),
                KeyCode::Right => term.send(b"\x1b[C"),
                KeyCode::Left => term.send(b"\x1b[D"),
                KeyCode::Home => term.send(b"\x1b[H"),
                KeyCode::End => term.send(b"\x1b[F"),
                KeyCode::Delete => term.send(b"\x1b[3~"),
                _ => {}
            }
            return;
        }
        match k.code {
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                buf.insert_char(c)
            }
            KeyCode::Enter => buf.newline(),
            KeyCode::Backspace => buf.backspace(),
            KeyCode::Delete => buf.delete(),
            KeyCode::Tab => buf.insert_tab(),
            KeyCode::Left => buf.move_left(),
            KeyCode::Right => buf.move_right(),
            KeyCode::Up => buf.move_up(),
            KeyCode::Down => buf.move_down(),
            KeyCode::Home => buf.move_home(),
            KeyCode::End => buf.move_end(),
            KeyCode::PageUp => buf.page_up(),
            KeyCode::PageDown => buf.page_down(),
            _ => {}
        }
        buf.ensure_visible();
    }

    fn save_active(&mut self) {
        let mut msg = None;
        if let Some(ed) = self.editor.as_mut() {
            if let Some(buf) = ed.panes.get_mut(ed.active).and_then(|p| p.buf_mut()) {
                msg = Some(if buf.term.is_some() {
                    "a terminal has nothing to save".into()
                } else {
                    match buf.save() {
                        Ok(()) => format!("saved {}", buf.name()),
                        Err(e) => format!("save failed: {e}"),
                    }
                });
            }
        }
        if let Some(m) = msg {
            self.toast(m);
        }
    }

    fn toggle_files_panel(&mut self) {
        let Some(ed) = self.editor.as_mut() else { return };
        if ed.files.is_some() {
            ed.files = None;
        } else {
            let expanded = BTreeSet::new();
            let mut entries = Vec::new();
            list_dir(&ed.root, 0, &expanded, &mut entries);
            ed.files = Some(FilesPanel { selected: 0, expanded, entries });
            ed.location = None;
        }
    }

    fn toggle_location(&mut self) {
        let Some(ed) = self.editor.as_mut() else { return };
        if ed.location.is_some() {
            ed.location = None;
        } else {
            let dir = ed.root.clone();
            let mut entries = Vec::new();
            list_dir(&dir, 0, &BTreeSet::new(), &mut entries);
            entries.retain(|e| e.depth == 0);
            ed.location = Some(LocationDropdown { dir, selected: 0, entries });
            ed.files = None;
        }
    }

    fn on_key_popup(&mut self, k: KeyEvent) {
        let Some(ed) = self.editor.as_mut() else { return };
        let Some(popup) = ed.popup.as_mut() else { return };
        match k.code {
            KeyCode::Esc => {
                ed.popup = None;
            }
            KeyCode::Backspace => {
                popup.input.pop();
            }
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                popup.input.push(c);
            }
            KeyCode::PageUp => self.scroll_popup_terminal(5),
            KeyCode::PageDown => self.scroll_popup_terminal(-5),
            KeyCode::Enter => {
                popup.scroll = 0;
                let input = std::mem::take(&mut popup.input);
                if input.trim().is_empty() {
                    return;
                }
                popup.lines.push(format!("» {input}"));
                let len = popup.lines.len();
                if len > 500 {
                    popup.lines.drain(0..len - 500);
                }
                self.run_popup_command(&input);
            }
            _ => {}
        }
    }

    fn popup_println(&mut self, s: impl Into<String>) {
        if let Some(ed) = self.editor.as_mut() {
            if let Some(popup) = ed.popup.as_mut() {
                popup.lines.push(s.into());
            }
        }
    }

    fn run_popup_command(&mut self, input: &str) {
        match commands::parse(input, &self.config.commands) {
            Action::ToggleFiles => {
                if let Some(ed) = self.editor.as_mut() {
                    ed.popup = None;
                }
                self.toggle_files_panel();
            }
            Action::Open(arg) => {
                let target = match arg {
                    Some(p) => {
                        let path = expand_tilde(&p);
                        if path.is_absolute() {
                            Some(path)
                        } else {
                            self.editor.as_ref().map(|ed| ed.root.join(path))
                        }
                    }
                    None => self.editor.as_ref().and_then(|ed| ed.pending_path.clone()),
                };
                match target {
                    Some(path) => self.open_file_beside(&path),
                    None => self.popup_println("  nothing to open — `open <path>`, or copy a path from the files panel first"),
                }
            }
            Action::Cd(p) => {
                let path = expand_tilde(&p);
                self.open_project(&path);
            }
            Action::Save => {
                self.save_active();
                if let Some(ed) = self.editor.as_mut() {
                    ed.popup = None;
                }
            }
            Action::Help => {
                for line in commands::help_lines(&self.config.commands) {
                    self.popup_println(format!("  {line}"));
                }
            }
            Action::Home => {
                if let Some(ed) = self.editor.as_mut() {
                    ed.popup = None;
                }
                self.screen = Screen::Home;
            }
            Action::QuitApp => self.should_quit = true,
            Action::Clear => {
                if let Some(ed) = self.editor.as_mut() {
                    if let Some(popup) = ed.popup.as_mut() {
                        popup.lines.clear();
                    }
                }
            }
            Action::CatName(name) => {
                self.config.cat.name = name.clone();
                self.config.save();
                self.popup_println(format!("  your cat is now called {name}"));
            }
            Action::CatColor(color) => {
                self.config.cat.color = color.clone();
                self.config.save();
                self.popup_println(format!("  {} got a new {color} coat", self.config.cat.name));
            }
            Action::Accent(color) => {
                self.config.theme.accent = color.clone();
                self.config.save();
                self.popup_println(format!("  accent color set to {color}"));
            }
            Action::Cursor(rest) => {
                let msg = self.cursor_command(&rest);
                self.popup_println(format!("  {msg}"));
            }
            Action::Spawn => self.spawn_terminal(),
            Action::Run => {
                if let Some(ed) = self.editor.as_mut() {
                    ed.popup = None;
                }
                self.run_current_file();
            }
            Action::Keys => {
                if let Some(ed) = self.editor.as_mut() {
                    ed.popup = None;
                }
                self.open_keys_editor();
            }
            Action::Unknown(msg) => self.popup_println(format!("  {msg}")),
        }
    }

    fn on_key_switcher(&mut self, k: KeyEvent) {
        let mut confirm = false;
        {
            let Some(ed) = self.editor.as_mut() else { return };
            let Some(sw) = ed.switcher.as_mut() else { return };
            let n = sw.items.len().max(1);
            match k.code {
                KeyCode::Esc => {
                    ed.switcher = None;
                    return;
                }
                KeyCode::Tab | KeyCode::Right | KeyCode::Down => {
                    sw.selected = (sw.selected + 1) % n;
                }
                KeyCode::BackTab | KeyCode::Left | KeyCode::Up => {
                    sw.selected = (sw.selected + n - 1) % n;
                }
                KeyCode::Enter | KeyCode::Char(' ') => confirm = true,
                _ => {}
            }
        }
        if confirm {
            self.switcher_confirm();
        }
    }

    fn on_key_place(&mut self, k: KeyEvent) {
        let mut side_pick: Option<usize> = None;
        let mut activate = false;
        {
            let Some(ed) = self.editor.as_mut() else { return };
            let Some(pl) = ed.place.as_mut() else { return };
            if pl.choosing.is_some() {
                match k.code {
                    KeyCode::Esc => pl.choosing = None,
                    KeyCode::Left | KeyCode::Up => pl.side = 0,
                    KeyCode::Right | KeyCode::Down => pl.side = 1,
                    KeyCode::Tab => pl.side = 1 - pl.side,
                    KeyCode::Enter => side_pick = Some(pl.side),
                    _ => {}
                }
            } else {
                match k.code {
                    KeyCode::Esc => ed.place = None,
                    KeyCode::Up => pl.selected = pl.selected.saturating_sub(1),
                    KeyCode::Down => {
                        if !pl.entries.is_empty() {
                            pl.selected = (pl.selected + 1).min(pl.entries.len() - 1);
                        }
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => activate = true,
                    _ => {}
                }
            }
        }
        if let Some(side) = side_pick {
            self.place_choose_side(side);
        } else if activate {
            self.place_activate();
        }
    }

    fn on_key_files(&mut self, k: KeyEvent) {
        let mut copied: Option<PathBuf> = None;
        {
            let Some(ed) = self.editor.as_mut() else { return };
            let Some(files) = ed.files.as_mut() else { return };
            match k.code {
                KeyCode::Esc => {
                    ed.files = None;
                    return;
                }
                KeyCode::Up => files.selected = files.selected.saturating_sub(1),
                KeyCode::Down => {
                    if !files.entries.is_empty() {
                        files.selected = (files.selected + 1).min(files.entries.len() - 1);
                    }
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    if let Some(entry) = files.entries.get(files.selected).cloned() {
                        if entry.is_dir {
                            // Dirs are "buttons": minimized by default, expand on press.
                            if !files.expanded.remove(&entry.path) {
                                files.expanded.insert(entry.path.clone());
                            }
                            let mut entries = Vec::new();
                            list_dir(&ed.root, 0, &files.expanded, &mut entries);
                            files.entries = entries;
                            files.selected = files
                                .entries
                                .iter()
                                .position(|e| e.path == entry.path)
                                .unwrap_or(0);
                        } else {
                            ed.pending_path = Some(entry.path.clone());
                            copied = Some(entry.path);
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(path) = copied {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            self.request_copy(path.display().to_string());
            self.toast(format!(
                "path copied: {name} — Ctrl+T then `open` to open it beside the current file"
            ));
        }
    }

    fn on_key_location(&mut self, k: KeyEvent) {
        let mut copied: Option<PathBuf> = None;
        {
            let Some(ed) = self.editor.as_mut() else { return };
            let Some(loc) = ed.location.as_mut() else { return };
            match k.code {
                KeyCode::Esc => {
                    ed.location = None;
                    return;
                }
                KeyCode::Up => loc.selected = loc.selected.saturating_sub(1),
                KeyCode::Down => {
                    if !loc.entries.is_empty() {
                        loc.selected = (loc.selected + 1).min(loc.entries.len() - 1);
                    }
                }
                KeyCode::Backspace => {
                    if let Some(parent) = loc.dir.parent().map(|p| p.to_path_buf()) {
                        loc.dir = parent;
                        let mut entries = Vec::new();
                        list_dir(&loc.dir, 0, &BTreeSet::new(), &mut entries);
                        entries.retain(|e| e.depth == 0);
                        loc.entries = entries;
                        loc.selected = 0;
                    }
                }
                KeyCode::Enter => {
                    if let Some(entry) = loc.entries.get(loc.selected).cloned() {
                        if entry.is_dir {
                            loc.dir = entry.path.clone();
                            let mut entries = Vec::new();
                            list_dir(&loc.dir, 0, &BTreeSet::new(), &mut entries);
                            entries.retain(|e| e.depth == 0);
                            loc.entries = entries;
                            loc.selected = 0;
                        } else {
                            ed.pending_path = Some(entry.path.clone());
                            copied = Some(entry.path);
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(path) = copied {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            self.request_copy(path.display().to_string());
            self.toast(format!(
                "path copied: {name} — Ctrl+T then `open` to open it beside the current file"
            ));
        }
    }

    #[cfg(test)]
    pub fn test_editor(root: PathBuf, tabs: Vec<Buffer>) -> Self {
        let mut app = Self::new();
        app.gui_mode = true;
        app.screen = Screen::Editor;
        app.editor = Some(EditorState {
            root,
            panes: vec![Pane { tabs, tab: 0 }],
            active: 0,
            popup: None,
            files: None,
            location: None,
            place: None,
            switcher: None,
            pending_path: None,
            term_seq: 0,
            split: 50,
        });
        app
    }

    pub fn open_file_beside(&mut self, path: &Path) {
        let canon = match path.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                self.popup_println(format!("  open: {e}"));
                self.toast(format!("cannot open: {}", path.display()));
                return;
            }
        };
        if canon.is_dir() {
            self.toast("that's a folder — use `cd` to switch projects");
            return;
        }
        let mut opened = None;
        if let Some(ed) = self.editor.as_mut() {
            if ed.panes.is_empty() {
                ed.panes.push(Pane::new());
                ed.active = 0;
            }
            let already = ed.panes.iter().enumerate().find_map(|(pi, p)| {
                p.tabs.iter().position(|b| b.path == canon).map(|ti| (pi, ti))
            });
            if let Some((pi, ti)) = already {
                ed.active = pi;
                ed.panes[pi].tab = ti;
                opened = Some(format!("already open: {}", canon.display()));
            } else {
                match Buffer::open(&canon) {
                    Ok(buf) => {
                        let name = buf.name();
                        // New files stack as tabs in the active pane.
                        let idx = ed.active.min(ed.panes.len() - 1);
                        let pane = &mut ed.panes[idx];
                        pane.tabs.push(buf);
                        pane.tab = pane.tabs.len() - 1;
                        ed.active = idx;
                        ed.pending_path = None;
                        ed.popup = None;
                        ed.files = None;
                        ed.location = None;
                        opened = Some(format!("opened {name}"));
                    }
                    Err(e) => opened = Some(format!("open failed: {e}")),
                }
            }
        }
        if let Some(msg) = opened {
            self.toast(msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn draw_once(app: &mut App) {
        let mut term = Terminal::new(TestBackend::new(100, 40)).unwrap();
        term.draw(|f| crate::ui::draw(f, app)).unwrap();
    }

    #[test]
    fn click_moves_editor_cursor() {
        let dir = std::env::temp_dir().join("silver_click_test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("t.txt");
        std::fs::write(&file, "hello world\nsecond line here\nthird\nfourth line\n").unwrap();

        let buf = Buffer::open(&file).unwrap();
        let mut app = App::test_editor(dir, vec![buf]);
        draw_once(&mut app);

        let area = app
            .mouse_targets
            .iter()
            .find_map(|t| match t {
                MouseTarget::EditorPane { area, .. } => Some(*area),
                _ => None,
            })
            .expect("pane target recorded during draw");

        // Click column 4 of the second visible row -> line 1, col 4.
        app.on_mouse_click(area.x + 4, area.y + 1);
        let ed = app.editor.as_ref().unwrap();
        let b = ed.panes[ed.active].buf().unwrap();
        assert_eq!(b.cy, 1);
        assert_eq!(b.cx, 4);

        // Click far beyond the end of a short line -> clamps to line end.
        app.on_mouse_click(area.x + 60, area.y + 2);
        let ed = app.editor.as_ref().unwrap();
        let b = ed.panes[ed.active].buf().unwrap();
        assert_eq!(b.cy, 2);
        assert_eq!(b.cx, "third".len());

        // Click below the last line -> clamps to the last line.
        app.on_mouse_click(area.x, area.y + area.height - 1);
        let ed = app.editor.as_ref().unwrap();
        let b = ed.panes[ed.active].buf().unwrap();
        assert_eq!(b.cy, b.lines.len() - 1);
    }

    #[test]
    fn tab_click_drag_split_and_place_back() {
        let dir = std::env::temp_dir().join("silver_tab_test");
        std::fs::create_dir_all(&dir).unwrap();
        // Canonical paths so the picker's entries match the open buffers.
        let dir = dir.canonicalize().unwrap();
        let f1 = dir.join("a.txt");
        let f2 = dir.join("b.txt");
        std::fs::write(&f1, "aaa\n").unwrap();
        std::fs::write(&f2, "bbb\n").unwrap();
        let b1 = Buffer::open(&f1).unwrap();
        let b2 = Buffer::open(&f2).unwrap();
        let mut app = App::test_editor(dir, vec![b1, b2]);
        draw_once(&mut app);

        // Click the second tab -> it becomes the shown file.
        let tab_area = app
            .mouse_targets
            .iter()
            .find_map(|t| match t {
                MouseTarget::Tab { pane: 0, tab: 1, area } => Some(*area),
                _ => None,
            })
            .expect("second tab recorded during draw");
        app.on_mouse_click(tab_area.x, tab_area.y);
        assert_eq!(app.editor.as_ref().unwrap().panes[0].tab, 1);

        // Drag it to the right half -> the view splits into two panes.
        app.drag = Some((0, 1));
        draw_once(&mut app);
        let a = app.pane_areas[0];
        app.drop_tab(a.x + a.width - 2, a.y + a.height / 2);
        let ed = app.editor.as_ref().unwrap();
        assert_eq!(ed.panes.len(), 2);
        assert_eq!(ed.panes[0].tabs.len(), 1);
        assert_eq!(ed.panes[1].tabs.len(), 1);
        assert_eq!(ed.active, 1);

        // Press the rail's `＋` -> the "open here" picker opens.
        draw_once(&mut app);
        let plus = app
            .mouse_targets
            .iter()
            .find_map(|t| match t {
                MouseTarget::SplitZone { area } => Some(*area),
                _ => None,
            })
            .expect("rail `+` recorded during draw");
        app.on_mouse_click(plus.x + 1, plus.y);
        assert!(app.editor.as_ref().unwrap().place.is_some());

        // Pick b.txt (open on the right) while both panes are busy:
        // the side prompt appears; choose left -> it moves to the left
        // pane and the emptied right pane collapses.
        {
            let pl = app.editor.as_mut().unwrap().place.as_mut().unwrap();
            pl.selected = pl
                .entries
                .iter()
                .position(|e| e.path.ends_with("b.txt"))
                .expect("b.txt listed");
        }
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(app.editor.as_ref().unwrap().place.as_ref().unwrap().choosing.is_some());
        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::empty()));
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        let ed = app.editor.as_ref().unwrap();
        assert!(ed.place.is_none());
        assert_eq!(ed.panes.len(), 1);
        assert_eq!(ed.panes[0].tabs.len(), 2);
        assert_eq!(ed.panes[0].buf().unwrap().name(), "b.txt");
    }

    #[test]
    fn rail_plus_picks_file_and_asks_side_when_busy() {
        let dir = std::env::temp_dir().join("silver_place_test");
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        for (n, c) in [("a.txt", "aaa\n"), ("b.txt", "bbb\n"), ("c.txt", "ccc\n")] {
            std::fs::write(dir.join(n), c).unwrap();
        }
        let b1 = Buffer::open(&dir.join("a.txt")).unwrap();
        let mut app = App::test_editor(dir, vec![b1]);
        draw_once(&mut app);

        // The rail's `＋` sits on the right edge without any hover.
        let zone = app
            .mouse_targets
            .iter()
            .find_map(|t| match t {
                MouseTarget::SplitZone { area } => Some(*area),
                _ => None,
            })
            .expect("rail `+` recorded during draw");

        // Press it and pick b.txt -> it opens in a new right pane.
        app.on_mouse_click(zone.x + 1, zone.y);
        {
            let pl = app.editor.as_mut().unwrap().place.as_mut().expect("picker opened");
            pl.selected = pl.entries.iter().position(|e| e.path.ends_with("b.txt")).unwrap();
        }
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        {
            let ed = app.editor.as_ref().unwrap();
            assert_eq!(ed.panes.len(), 2);
            assert_eq!(ed.panes[1].buf().unwrap().name(), "b.txt");
        }

        // Both sides busy: picking c.txt asks which side; choose left.
        app.open_place_picker(None);
        {
            let pl = app.editor.as_mut().unwrap().place.as_mut().unwrap();
            pl.selected = pl.entries.iter().position(|e| e.path.ends_with("c.txt")).unwrap();
        }
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(app
            .editor
            .as_ref()
            .unwrap()
            .place
            .as_ref()
            .unwrap()
            .choosing
            .is_some());
        app.on_key(KeyEvent::new(KeyCode::Left, KeyModifiers::empty()));
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        let ed = app.editor.as_ref().unwrap();
        assert!(ed.place.is_none());
        assert_eq!(ed.panes.len(), 2);
        assert_eq!(ed.panes[0].buf().unwrap().name(), "c.txt");
        assert_eq!(ed.panes[0].tabs.len(), 2);
    }

    #[test]
    fn ctrl_tab_switcher_swaps_into_last_active_pane() {
        let dir = std::env::temp_dir().join("silver_cycle_test");
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let f1 = dir.join("a.txt");
        let f2 = dir.join("b.txt");
        std::fs::write(&f1, "aaa\n").unwrap();
        std::fs::write(&f2, "bbb\n").unwrap();
        let b1 = Buffer::open(&f1).unwrap();
        let b2 = Buffer::open(&f2).unwrap();
        let mut app = App::test_editor(dir, vec![b1, b2]);
        // Put b.txt on the right; the right pane is now the active one.
        app.place_file(&f2, 1);
        assert_eq!(app.editor.as_ref().unwrap().panes.len(), 2);
        assert_eq!(app.editor.as_ref().unwrap().active, 1);

        // Ctrl+Tab opens the switcher with the next file highlighted.
        let ctrl_tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::CONTROL);
        app.on_key(ctrl_tab);
        {
            let ed = app.editor.as_ref().unwrap();
            let sw = ed.switcher.as_ref().expect("switcher opened");
            assert_eq!(sw.items, vec![(0, 0), (1, 0)]);
            assert_eq!(sw.selected, 0); // a.txt, the other file
        }
        // The switcher draws as a centered overlay with clickable cards.
        draw_once(&mut app);
        assert!(app
            .mouse_targets
            .iter()
            .any(|t| matches!(t, MouseTarget::SwitchItem { idx: 0, .. })));

        // Enter: a.txt (shown on the left) trades places with b.txt,
        // landing in the last active pane -- the right one.
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        {
            let ed = app.editor.as_ref().unwrap();
            assert!(ed.switcher.is_none());
            assert_eq!(ed.active, 1);
            assert_eq!(ed.panes[1].buf().unwrap().name(), "a.txt");
            assert_eq!(ed.panes[0].buf().unwrap().name(), "b.txt");
        }

        // Now work on the left, switch again: the pick lands left.
        app.editor.as_mut().unwrap().active = 0;
        app.on_key(ctrl_tab);
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        let ed = app.editor.as_ref().unwrap();
        assert_eq!(ed.active, 0);
        assert_eq!(ed.panes[0].buf().unwrap().name(), "a.txt");
        assert_eq!(ed.panes[1].buf().unwrap().name(), "b.txt");
    }

    #[test]
    fn spawn_terminal_displaces_file_and_runs_commands() {
        let dir = std::env::temp_dir().join("silver_term_test");
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let f1 = dir.join("a.txt");
        std::fs::write(&f1, "aaa\n").unwrap();
        let b1 = Buffer::open(&f1).unwrap();
        let mut app = App::test_editor(dir, vec![b1]);

        // The header shows a ▶ run button for the open file.
        draw_once(&mut app);
        assert!(app
            .mouse_targets
            .iter()
            .any(|t| matches!(t, MouseTarget::RunButton { .. })));

        // `spawn`: terminal takes this spot, the file is sent right.
        app.spawn_terminal();
        {
            let ed = app.editor.as_ref().unwrap();
            assert_eq!(ed.panes.len(), 2);
            assert!(ed.panes[0].buf().unwrap().term.is_some());
            assert_eq!(ed.panes[1].buf().unwrap().name(), "a.txt");
            assert_eq!(ed.active, 0); // focus stays on the terminal
        }

        // Typing + enter runs a real command in the PTY shell; its
        // output appears on the emulated screen (on its own line, so
        // the echoed `echo ...` doesn't count).
        for c in "echo silver_ok".chars() {
            app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()));
        }
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        let mut seen = false;
        for _ in 0..200 {
            let ed = app.editor.as_mut().unwrap();
            let term = ed.panes[0].buf_mut().unwrap().term.as_mut().unwrap();
            let text = term.with_screen(|s| s.contents()).unwrap_or_default();
            if text.lines().any(|l| l.trim() == "silver_ok") {
                seen = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(seen, "command output arrived in the terminal");

        // ▶ run on the file reuses that terminal (no second one).
        if let Some(ed) = app.editor.as_mut() {
            ed.active = 1;
        }
        let (pi, ti) = app.ensure_terminal();
        assert_eq!((pi, ti), (0, 0));
    }

    #[test]
    fn command_words_are_configurable() {
        let mut cmds = crate::config::default_commands();
        // Defaults and built-in aliases work out of the box.
        assert!(matches!(commands::parse("spawn", &cmds), commands::Action::Spawn));
        assert!(matches!(commands::parse("r", &cmds), commands::Action::Run));
        // Rename spawn -> t: the new word works, the old one is gone.
        cmds.insert("spawn".into(), "t".into());
        assert!(matches!(commands::parse("t", &cmds), commands::Action::Spawn));
        assert!(matches!(commands::parse("spawn", &cmds), commands::Action::Unknown(_)));
        // A configured word beats a built-in alias.
        cmds.insert("run".into(), "o".into());
        assert!(matches!(commands::parse("o", &cmds), commands::Action::Run));
        // Help speaks the user's words.
        let help = commands::help_lines(&cmds).join("\n");
        assert!(help.contains("t "), "help mentions the new spawn word");
    }

    #[test]
    fn keys_panel_opens_navigates_and_captures_combos() {
        // Combo capture: ctrl/alt required, round-trips through the parser.
        assert_eq!(
            combo_string(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)).as_deref(),
            Some("ctrl+s")
        );
        assert_eq!(
            combo_string(&KeyEvent::new(KeyCode::Left, KeyModifiers::ALT)).as_deref(),
            Some("alt+left")
        );
        assert_eq!(combo_string(&KeyEvent::new(KeyCode::Char('s'), KeyModifiers::empty())), None);

        let dir = std::env::temp_dir().join("silver_keys_test");
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let f1 = dir.join("a.txt");
        std::fs::write(&f1, "aaa\n").unwrap();
        let b1 = Buffer::open(&f1).unwrap();
        let mut app = App::test_editor(dir, vec![b1]);

        app.open_keys_editor();
        draw_once(&mut app);
        assert!(app
            .mouse_targets
            .iter()
            .any(|t| matches!(t, MouseTarget::KeysRow { .. })));

        // Navigate, start an edit, cancel it, close the panel.
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::empty()));
        assert_eq!(app.keys_editor.as_ref().unwrap().selected, 1);
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert!(app.keys_editor.as_ref().unwrap().editing);
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        assert!(!app.keys_editor.as_ref().unwrap().editing);
        app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()));
        assert!(app.keys_editor.is_none());
    }

    #[test]
    fn split_adjusts_and_clamps() {
        let dir = std::env::temp_dir().join("silver_split_test");
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let f1 = dir.join("a.txt");
        let f2 = dir.join("b.txt");
        std::fs::write(&f1, "aaa\n").unwrap();
        std::fs::write(&f2, "bbb\n").unwrap();
        let b1 = Buffer::open(&f1).unwrap();
        let b2 = Buffer::open(&f2).unwrap();
        let mut app = App::test_editor(dir, vec![b1, b2]);

        // One pane: nothing to resize.
        app.adjust_split(-5);
        assert_eq!(app.editor.as_ref().unwrap().split, 50);

        // Split, then nudge left; the divider clamps at 20/80.
        app.move_tab(0, 1, 1);
        app.adjust_split(-5);
        assert_eq!(app.editor.as_ref().unwrap().split, 45);
        for _ in 0..20 {
            app.adjust_split(-5);
        }
        assert_eq!(app.editor.as_ref().unwrap().split, 20);

        // The drawn panes follow the split.
        draw_once(&mut app);
        assert!(app.pane_areas[0].width < app.pane_areas[1].width);

        // Dragging the divider to a column moves the split there.
        let mid = app.pane_areas[0].x + (app.pane_areas[0].width + app.pane_areas[1].width) / 2;
        assert!(app.divider_hit(app.pane_areas[1].x, app.pane_areas[1].y + 1));
        app.set_split_to(mid);
        let split = app.editor.as_ref().unwrap().split;
        assert!((45..=55).contains(&split), "split near the middle, got {split}");
    }
}
