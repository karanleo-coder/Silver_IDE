use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::term::TermSession;

/// A single open tab: a file, or a terminal session when `term` is
/// set. Plain `Vec<String>` line storage: simple, predictable, and
/// cheap for the file sizes a human edits by hand.
pub struct Buffer {
    pub path: PathBuf,
    pub lines: Vec<String>,
    /// Cursor position in characters (not bytes).
    pub cx: usize,
    pub cy: usize,
    pub scroll: usize,
    pub hscroll: usize,
    pub dirty: bool,
    /// Last known viewport size, updated during draw so key handling
    /// can scroll correctly.
    pub view_w: usize,
    pub view_h: usize,
    /// Set when this tab is a terminal instead of a file.
    pub term: Option<TermSession>,
}

fn byte_idx(line: &str, cx: usize) -> usize {
    line.char_indices().nth(cx).map(|(i, _)| i).unwrap_or(line.len())
}

impl Buffer {
    pub fn open(path: &Path) -> io::Result<Self> {
        let content = fs::read_to_string(path).unwrap_or_default();
        let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        Ok(Self {
            path: path.to_path_buf(),
            lines,
            cx: 0,
            cy: 0,
            scroll: 0,
            hscroll: 0,
            dirty: false,
            view_w: 80,
            view_h: 24,
            term: None,
        })
    }

    /// A terminal tab: behaves like a file in panes, the rail, and
    /// the switcher, but runs commands instead of holding text.
    pub fn terminal(id: usize, cwd: PathBuf) -> Self {
        Self {
            path: PathBuf::from(format!("silver-term://{id}")),
            lines: vec![String::new()],
            cx: 0,
            cy: 0,
            scroll: 0,
            hscroll: 0,
            dirty: false,
            view_w: 80,
            view_h: 24,
            term: Some(TermSession::new(id, cwd)),
        }
    }

    pub fn save(&mut self) -> io::Result<()> {
        if self.term.is_some() {
            return Ok(()); // a terminal has nothing to save
        }
        let mut out = self.lines.join("\n");
        out.push('\n');
        fs::write(&self.path, out)?;
        self.dirty = false;
        Ok(())
    }

    pub fn name(&self) -> String {
        if let Some(t) = &self.term {
            return format!("⌁ term {}", t.id);
        }
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "untitled".into())
    }

    pub fn ext(&self) -> String {
        self.path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    }

    fn line_len(&self, y: usize) -> usize {
        self.lines.get(y).map(|l| l.chars().count()).unwrap_or(0)
    }

    fn clamp_x(&mut self) {
        let len = self.line_len(self.cy);
        if self.cx > len {
            self.cx = len;
        }
    }

    pub fn insert_char(&mut self, c: char) {
        let idx = byte_idx(&self.lines[self.cy], self.cx);
        self.lines[self.cy].insert(idx, c);
        self.cx += 1;
        self.dirty = true;
    }

    pub fn insert_tab(&mut self) {
        for _ in 0..4 {
            self.insert_char(' ');
        }
    }

    pub fn newline(&mut self) {
        let idx = byte_idx(&self.lines[self.cy], self.cx);
        let rest = self.lines[self.cy].split_off(idx);
        // Keep the previous line's leading indentation.
        let indent: String = self.lines[self.cy]
            .chars()
            .take_while(|c| *c == ' ')
            .collect();
        self.cy += 1;
        self.cx = indent.chars().count();
        self.lines.insert(self.cy, format!("{indent}{rest}"));
        self.dirty = true;
    }

    pub fn backspace(&mut self) {
        if self.cx > 0 {
            let idx = byte_idx(&self.lines[self.cy], self.cx - 1);
            self.lines[self.cy].remove(idx);
            self.cx -= 1;
            self.dirty = true;
        } else if self.cy > 0 {
            let line = self.lines.remove(self.cy);
            self.cy -= 1;
            self.cx = self.line_len(self.cy);
            self.lines[self.cy].push_str(&line);
            self.dirty = true;
        }
    }

    pub fn delete(&mut self) {
        let len = self.line_len(self.cy);
        if self.cx < len {
            let idx = byte_idx(&self.lines[self.cy], self.cx);
            self.lines[self.cy].remove(idx);
            self.dirty = true;
        } else if self.cy + 1 < self.lines.len() {
            let line = self.lines.remove(self.cy + 1);
            self.lines[self.cy].push_str(&line);
            self.dirty = true;
        }
    }

    pub fn move_left(&mut self) {
        if self.cx > 0 {
            self.cx -= 1;
        } else if self.cy > 0 {
            self.cy -= 1;
            self.cx = self.line_len(self.cy);
        }
    }

    pub fn move_right(&mut self) {
        if self.cx < self.line_len(self.cy) {
            self.cx += 1;
        } else if self.cy + 1 < self.lines.len() {
            self.cy += 1;
            self.cx = 0;
        }
    }

    pub fn move_up(&mut self) {
        if self.cy > 0 {
            self.cy -= 1;
            self.clamp_x();
        }
    }

    pub fn move_down(&mut self) {
        if self.cy + 1 < self.lines.len() {
            self.cy += 1;
            self.clamp_x();
        }
    }

    pub fn move_home(&mut self) {
        self.cx = 0;
    }

    pub fn move_end(&mut self) {
        self.cx = self.line_len(self.cy);
    }

    pub fn page_up(&mut self) {
        self.cy = self.cy.saturating_sub(self.view_h.max(1));
        self.clamp_x();
    }

    pub fn page_down(&mut self) {
        self.cy = (self.cy + self.view_h.max(1)).min(self.lines.len() - 1);
        self.clamp_x();
    }

    pub fn ensure_visible(&mut self) {
        let h = self.view_h.max(1);
        let w = self.view_w.max(1);
        if self.cy < self.scroll {
            self.scroll = self.cy;
        }
        if self.cy >= self.scroll + h {
            self.scroll = self.cy + 1 - h;
        }
        if self.cx < self.hscroll {
            self.hscroll = self.cx;
        }
        if self.cx >= self.hscroll + w {
            self.hscroll = self.cx + 1 - w;
        }
    }
}
