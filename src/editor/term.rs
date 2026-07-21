//! A real terminal that lives in an editor pane like a file tab.
//! Backed by a PTY running the user's login shell, so interactive
//! programs (claude, python, git prompts, ...) work like they do in a
//! normal terminal, and the shell's own prompt shows the location.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

// Same scrollback depth VS Code gives its terminal, at a fraction of
// the overhead; rows only materialise once output actually scrolls.
const SCROLLBACK: usize = 1000;

pub struct TermSession {
    pub id: usize,
    /// Folder the shell starts in; the prompt tracks it from there.
    pub cwd: PathBuf,
    /// Rows scrolled back into history; 0 = the live view.
    pub scroll: usize,
    /// The emulated screen, fed by the PTY reader thread.
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Option<Box<dyn Write + Send>>,
    master: Option<Box<dyn MasterPty + Send>>,
    child: Option<Box<dyn Child + Send + Sync>>,
    /// Set when the PTY could not start; drawn instead of a screen.
    pub error: Option<String>,
    size: (u16, u16),
}

impl TermSession {
    pub fn new(id: usize, cwd: PathBuf) -> Self {
        let mut s = Self {
            id,
            cwd,
            scroll: 0,
            parser: Arc::new(Mutex::new(vt100::Parser::new(24, 80, SCROLLBACK))),
            writer: None,
            master: None,
            child: None,
            error: None,
            size: (24, 80),
        };
        if let Err(e) = s.start() {
            s.error = Some(e.to_string());
        }
        s
    }

    fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let pty = native_pty_system();
        let pair = pty.openpty(PtySize {
            rows: self.size.0,
            cols: self.size.1,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        #[cfg(target_os = "windows")]
        let mut cmd = {
            // ConPTY wants a full path more often than not; COMSPEC
            // always has one (C:\Windows\system32\cmd.exe).
            let shell = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into());
            CommandBuilder::new(shell)
        };
        #[cfg(not(target_os = "windows"))]
        let mut cmd = {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
            let mut c = CommandBuilder::new(shell);
            // Login shell: PATH and tools (claude, nvm, cargo, ...)
            // match the user's normal terminal.
            c.arg("-l");
            c
        };
        cmd.cwd(&self.cwd);
        #[cfg(not(target_os = "windows"))]
        {
            cmd.env("TERM", "xterm-256color");
            cmd.env("COLORTERM", "truecolor");
        }

        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        let parser = Arc::clone(&self.parser);
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if let Ok(mut p) = parser.lock() {
                            p.process(&buf[..n]);
                        }
                    }
                }
            }
        });

        self.master = Some(pair.master);
        self.writer = Some(writer);
        self.child = Some(child);
        Ok(())
    }

    /// Whether the shell (or whatever replaced it) is still alive.
    pub fn is_running(&mut self) -> bool {
        match self.child.as_mut() {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// Send raw bytes (keystrokes) to the shell; typing snaps the
    /// view back to the live screen.
    pub fn send(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.scroll = 0;
        if let Some(w) = self.writer.as_mut() {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }

    /// Type a whole command line and press enter (the ▶ run button).
    pub fn exec(&mut self, cmd: &str) {
        self.send(format!("{cmd}\r").as_bytes());
    }

    /// Ctrl+C for whatever is running.
    pub fn interrupt(&mut self) {
        self.send(b"\x03");
    }

    /// Match the PTY and emulated screen to the pane size.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        if rows == 0 || cols == 0 || self.size == (rows, cols) {
            return;
        }
        self.size = (rows, cols);
        if let Ok(mut p) = self.parser.lock() {
            p.set_size(rows, cols);
        }
        if let Some(m) = &self.master {
            let _ = m.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        }
    }

    /// The live screen as plain text (ignores the user's scrollback
    /// position and puts it back), for spotting a finished run.
    pub fn live_text(&mut self) -> String {
        let Ok(mut p) = self.parser.lock() else { return String::new() };
        let keep = p.screen().scrollback();
        p.set_scrollback(0);
        let text = p.screen().contents();
        p.set_scrollback(keep);
        text
    }

    /// Look at the screen, scrolled back by `self.scroll` rows.
    pub fn with_screen<R>(&mut self, f: impl FnOnce(&vt100::Screen) -> R) -> Option<R> {
        let mut p = self.parser.lock().ok()?;
        p.set_scrollback(self.scroll);
        self.scroll = p.screen().scrollback();
        Some(f(p.screen()))
    }
}

impl Drop for TermSession {
    fn drop(&mut self) {
        if let Some(c) = self.child.as_mut() {
            let _ = c.kill();
        }
    }
}
