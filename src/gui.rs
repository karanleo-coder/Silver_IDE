//! Windowed app mode: the same silver UI, rendered on a monospace grid
//! inside a native window. Keeps the terminal feel, adds real scrolling
//! with the mouse wheel and a vector-drawn cat in the header.

use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Rect as ERect, Stroke, Vec2};
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Position;
use ratatui::style::{Color as RColor, Modifier};
use ratatui::Terminal;

use crate::app::{App, Screen};
use crate::cat;
use crate::ui;

const FONT_SIZE: f32 = 15.0;
const HEADER_H: f32 = 60.0;
const BG: Color32 = Color32::from_rgb(16, 16, 20);
const BAR_BG: Color32 = Color32::from_rgb(24, 24, 31);
const TICK: Duration = Duration::from_millis(120);

pub fn run() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("silver")
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([560.0, 380.0]),
        ..Default::default()
    };
    eframe::run_native("silver", options, Box::new(|_cc| Ok(Box::new(Gui::new()))))
}

struct Gui {
    app: App,
    term: Terminal<TestBackend>,
    last_tick: Instant,
    wheel_accum: f32,
    /// Window position of the last primary-button press, while held.
    drag_from: Option<Pos2>,
    /// For handing freed memory back to the OS at quiet moments.
    last_relief: Instant,
    was_focused: bool,
}

impl Gui {
    fn new() -> Self {
        let mut app = App::new();
        app.gui_mode = true;
        Self {
            app,
            term: Terminal::new(TestBackend::new(100, 40)).expect("test backend"),
            last_tick: Instant::now(),
            wheel_accum: 0.0,
            drag_from: None,
            last_relief: Instant::now(),
            was_focused: true,
        }
    }
}

#[cfg(target_os = "macos")]
extern "C" {
    /// macOS libc: return freed heap pages to the OS so the process
    /// footprint reflects what's actually in use.
    fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
}

fn release_free_memory() {
    #[cfg(target_os = "macos")]
    unsafe {
        malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
    }
}

/// ratatui color -> egui color, with a fallback for `Reset`.
fn to_c32(c: RColor, default: Color32) -> Color32 {
    match c {
        RColor::Reset => default,
        RColor::Black => Color32::from_rgb(12, 12, 15),
        RColor::Red | RColor::LightRed => Color32::from_rgb(224, 92, 92),
        RColor::Green | RColor::LightGreen => Color32::from_rgb(92, 200, 130),
        RColor::Yellow | RColor::LightYellow => Color32::from_rgb(229, 192, 92),
        RColor::Blue | RColor::LightBlue => Color32::from_rgb(96, 140, 247),
        RColor::Magenta | RColor::LightMagenta => Color32::from_rgb(197, 115, 227),
        RColor::Cyan | RColor::LightCyan => Color32::from_rgb(92, 214, 214),
        RColor::Gray => Color32::from_rgb(160, 160, 165),
        RColor::DarkGray => Color32::from_rgb(105, 105, 112),
        RColor::White => Color32::from_rgb(238, 238, 240),
        RColor::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        RColor::Indexed(i) => ansi256(i),
    }
}

/// The standard xterm 256-color palette, for programs running in the
/// terminal tab.
fn ansi256(i: u8) -> Color32 {
    const BASE: [(u8, u8, u8); 16] = [
        (12, 12, 15), (224, 92, 92), (92, 200, 130), (229, 192, 92),
        (96, 140, 247), (197, 115, 227), (92, 214, 214), (200, 200, 205),
        (105, 105, 112), (255, 120, 120), (120, 230, 160), (245, 220, 120),
        (130, 170, 255), (225, 145, 255), (120, 235, 235), (238, 238, 240),
    ];
    match i {
        0..=15 => {
            let (r, g, b) = BASE[i as usize];
            Color32::from_rgb(r, g, b)
        }
        16..=231 => {
            let n = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color32::from_rgb(level(n / 36), level((n / 6) % 6), level(n % 6))
        }
        _ => {
            let v = 8 + (i - 232) * 10;
            Color32::from_rgb(v, v, v)
        }
    }
}

/// One merged run of same-colored text; skipped when it's only spaces.
fn draw_text_run(painter: &egui::Painter, pos: Pos2, text: &str, font: &FontId, color: Color32) {
    if !text.trim().is_empty() {
        painter.text(pos, Align2::LEFT_TOP, text, font.clone(), color);
    }
}

fn translate_key(key: egui::Key, m: egui::Modifiers) -> Option<KeyEvent> {
    use egui::Key as K;
    let mut mods = KeyModifiers::empty();
    let ctrl_like = m.ctrl || m.mac_cmd || m.command;
    if ctrl_like {
        mods |= KeyModifiers::CONTROL;
    }
    if m.alt {
        mods |= KeyModifiers::ALT;
    }
    if m.shift {
        mods |= KeyModifiers::SHIFT;
    }
    let code = match key {
        K::Enter => KeyCode::Enter,
        K::Escape => KeyCode::Esc,
        K::Backspace => KeyCode::Backspace,
        K::Delete => KeyCode::Delete,
        K::Tab => KeyCode::Tab,
        K::ArrowUp => KeyCode::Up,
        K::ArrowDown => KeyCode::Down,
        K::ArrowLeft => KeyCode::Left,
        K::ArrowRight => KeyCode::Right,
        K::Home => KeyCode::Home,
        K::End => KeyCode::End,
        K::PageUp => KeyCode::PageUp,
        K::PageDown => KeyCode::PageDown,
        K::Space if ctrl_like => KeyCode::Char(' '),
        other => {
            // Letters/digits only matter here for ctrl/cmd shortcuts;
            // plain typing arrives as Text events instead.
            if !ctrl_like {
                return None;
            }
            let name = other.name();
            let mut chars = name.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => KeyCode::Char(c.to_ascii_lowercase()),
                _ => return None,
            }
        }
    };
    Some(KeyEvent::new(code, mods))
}

impl eframe::App for Gui {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        // Animation clock, same pace as the terminal loop.
        while self.last_tick.elapsed() >= TICK {
            self.last_tick += TICK;
            self.app.tick_update();
        }

        // ---- input ----
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            match ev {
                egui::Event::Text(s) => {
                    for c in s.chars() {
                        self.app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()));
                    }
                }
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    if let Some(k) = translate_key(key, modifiers) {
                        self.app.on_key(k);
                    }
                }
                egui::Event::Paste(s) => {
                    // Cmd+V: feed the pasted text in as typed characters.
                    for c in s.chars().filter(|c| !c.is_control()) {
                        self.app.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::empty()));
                    }
                }
                _ => {}
            }
        }

        // Copies requested by the app (file lists, folder browser) go
        // through the window's native clipboard.
        if let Some(text) = self.app.clipboard_request.take() {
            ctx.copy_text(text);
        }

        if self.app.should_quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // ---- header bar with the cat ----
        egui::Panel::top("cat_bar")
            .exact_size(HEADER_H)
            .frame(egui::Frame::NONE.fill(BAR_BG))
            .show(root, |ui| {
                draw_header_cat(ui, &self.app);
            });

        // ---- terminal grid ----
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(BG))
            .show(root, |ui| {
                let font = FontId::monospace(FONT_SIZE);
                let (cw, rh) = ctx.fonts_mut(|f| {
                    (f.glyph_width(&font, 'M'), f.row_height(&font))
                });
                let avail = ui.available_rect_before_wrap();
                let pad = 6.0;
                let grid = avail.shrink(pad);
                let cols = ((grid.width() / cw) as u16).clamp(40, 400);
                let rows = ((grid.height() / rh) as u16).clamp(12, 200);

                // Mouse wheel -> lines.
                let dy = ctx.input(|i| i.smooth_scroll_delta.y);
                self.wheel_accum += dy;
                let lines = (self.wheel_accum / rh) as i32;
                if lines != 0 {
                    self.wheel_accum -= lines as f32 * rh;
                    self.app.on_scroll(lines);
                }

                // Grid cell under a window position, if inside the grid.
                let cell = |p: Pos2| -> Option<(u16, u16)> {
                    if !grid.contains(p) {
                        return None;
                    }
                    let col = ((p.x - grid.min.x) / cw) as i64;
                    let row = ((p.y - grid.min.y) / rh) as i64;
                    if (0..cols as i64).contains(&col) && (0..rows as i64).contains(&row) {
                        Some((col as u16, row as u16))
                    } else {
                        None
                    }
                };

                // Track hover so the UI can show the "open here" zone.
                self.app.hover =
                    ctx.input(|i| i.pointer.hover_pos()).and_then(|p| cell(p));

                // Mouse press -> grid click; holding and moving drags a tab.
                let (pressed, down_pos, released) = ctx.input(|i| {
                    (
                        i.pointer.primary_pressed(),
                        if i.pointer.primary_down() { i.pointer.interact_pos() } else { None },
                        i.pointer.primary_released(),
                    )
                });
                if pressed {
                    if let Some(p) = down_pos {
                        self.drag_from = Some(p);
                        if let Some((x, y)) = cell(p) {
                            // On the pane divider a press starts a resize
                            // instead of a click.
                            if self.app.divider_hit(x, y) {
                                self.app.resize_drag = true;
                            } else {
                                self.app.on_mouse_click(x, y);
                            }
                        }
                    }
                }
                if self.app.resize_drag {
                    // The split follows the held pointer.
                    if let Some(p) = down_pos {
                        if let Some((x, _)) = cell(p) {
                            self.app.set_split_to(x);
                        }
                    }
                } else if let (Some(from), Some(cur)) = (self.drag_from, down_pos) {
                    // A real drag starts once the pointer leaves the pressed cell.
                    if self.app.drag.is_none() && (cur - from).length() > cw.max(rh) {
                        if let Some((x, y)) = cell(from) {
                            self.app.drag = self.app.tab_at(x, y);
                        }
                    }
                }
                if released {
                    self.app.resize_drag = false;
                    if self.app.drag.is_some() {
                        let end = ctx.input(|i| i.pointer.interact_pos());
                        match end.and_then(|p| cell(p)) {
                            Some((x, y)) => self.app.drop_tab(x, y),
                            None => self.app.drag = None,
                        }
                    }
                    self.drag_from = None;
                }

                self.term.backend_mut().resize(cols, rows);
                // Sentinel: if the frame sets a cursor, this gets overwritten.
                let _ = self
                    .term
                    .set_cursor_position(Position { x: u16::MAX, y: u16::MAX });
                let app = &mut self.app;
                let completed = self.term.draw(|f| ui::draw(f, app));

                let painter = ui.painter();
                if let Ok(frame) = completed {
                    let buf = frame.buffer;
                    let area = frame.area;
                    let origin = grid.min;
                    // Runs of equal-colored cells become one rect / one
                    // text shape instead of one per character: far fewer
                    // allocations and draw calls every frame.
                    let mut run = String::new();
                    for y in 0..area.height {
                        let py = origin.y + y as f32 * rh;

                        // Backgrounds, merged.
                        let mut sx = 0u16;
                        let mut cur = Color32::TRANSPARENT;
                        for x in 0..=area.width {
                            let bg = if x < area.width {
                                buf.cell((x, y))
                                    .map(|c| to_c32(c.bg, Color32::TRANSPARENT))
                                    .unwrap_or(Color32::TRANSPARENT)
                            } else {
                                Color32::TRANSPARENT
                            };
                            if bg != cur {
                                if cur != Color32::TRANSPARENT {
                                    painter.rect_filled(
                                        ERect::from_min_size(
                                            Pos2::new(origin.x + sx as f32 * cw, py),
                                            Vec2::new((x - sx) as f32 * cw + 0.5, rh + 0.5),
                                        ),
                                        0.0,
                                        cur,
                                    );
                                }
                                sx = x;
                                cur = bg;
                            }
                        }

                        // Text: ASCII runs share a shape; wider glyphs
                        // (box art, emoji) keep exact per-cell placement.
                        let mut run_x = 0u16;
                        let mut run_fg = Color32::WHITE;
                        for x in 0..area.width {
                            let Some(cell) = buf.cell((x, y)) else { continue };
                            let sym = cell.symbol();
                            if sym == " " {
                                if !run.is_empty() {
                                    run.push(' ');
                                }
                                continue;
                            }
                            let mut fg = to_c32(cell.fg, Color32::from_rgb(220, 220, 220));
                            if cell.modifier.contains(Modifier::BOLD) {
                                fg = fg.gamma_multiply(1.25);
                            }
                            if cell.modifier.contains(Modifier::DIM) {
                                fg = fg.gamma_multiply(0.6);
                            }
                            let ascii = sym.len() == 1 && sym.is_ascii();
                            if ascii && !run.is_empty() && fg == run_fg {
                                run.push(sym.chars().next().unwrap());
                                continue;
                            }
                            if !run.is_empty() {
                                draw_text_run(painter, Pos2::new(origin.x + run_x as f32 * cw, py), &run, &font, run_fg);
                                run.clear();
                            }
                            if ascii {
                                run_x = x;
                                run_fg = fg;
                                run.push(sym.chars().next().unwrap());
                            } else if !sym.trim().is_empty() {
                                painter.text(
                                    Pos2::new(origin.x + x as f32 * cw, py),
                                    Align2::LEFT_TOP,
                                    sym,
                                    font.clone(),
                                    fg,
                                );
                            }
                        }
                        if !run.is_empty() {
                            draw_text_run(painter, Pos2::new(origin.x + run_x as f32 * cw, py), &run, &font, run_fg);
                            run.clear();
                        }
                    }
                }

                // Text cursor, in the user's configured style.
                if let Ok(pos) = self.term.get_cursor_position() {
                    if pos.x != u16::MAX && pos.x < cols && pos.y < rows {
                        let px = grid.min.x + pos.x as f32 * cw;
                        let py = grid.min.y + pos.y as f32 * rh;
                        let accent = to_c32(self.app.config.accent(), Color32::LIGHT_BLUE);
                        let blink_on =
                            !self.app.config.theme.cursor_blink || self.app.tick % 8 < 5;
                        if blink_on {
                            let cell = ERect::from_min_size(
                                Pos2::new(px, py),
                                Vec2::new(cw, rh),
                            );
                            match self.app.config.theme.cursor.as_str() {
                                "bar" => painter.rect_filled(
                                    ERect::from_min_size(cell.min, Vec2::new(2.0, rh)),
                                    0.0,
                                    accent,
                                ),
                                "underline" => painter.rect_filled(
                                    ERect::from_min_size(
                                        Pos2::new(px, py + rh - 2.0),
                                        Vec2::new(cw, 2.0),
                                    ),
                                    0.0,
                                    accent,
                                ),
                                "hollow" => painter.rect_stroke(
                                    cell.shrink(0.75),
                                    1.0,
                                    Stroke::new(1.5, accent),
                                    egui::StrokeKind::Inside,
                                ),
                                _ => painter.rect_filled(
                                    cell,
                                    1.0,
                                    accent.gamma_multiply(0.45),
                                ),
                            };
                        }
                    }
                }
            });

        // Nearly sleep while the window is in the background: the
        // user's programs get the CPU, not the IDE.
        let focused = ctx.input(|i| i.raw.focused);
        ctx.request_repaint_after(if focused { TICK } else { Duration::from_millis(600) });

        // Give freed memory back to the OS when attention moves away,
        // and on a slow heartbeat, so the footprint stays honest.
        if (self.was_focused && !focused) || self.last_relief.elapsed() > Duration::from_secs(30)
        {
            release_free_memory();
            self.last_relief = Instant::now();
        }
        self.was_focused = focused;
    }
}

/// The header cat: the ASCII cat from cat.txt, drawn in monospace.
/// It blinks, snacks, and sings — jiggling its head to the music.
fn draw_header_cat(ui: &mut egui::Ui, app: &App) {
    let rect = ui.max_rect();
    let painter = ui.painter();
    let fur = to_c32(app.config.cat_color(), Color32::from_rgb(192, 192, 192));
    let accent = to_c32(app.config.accent(), Color32::LIGHT_BLUE);
    let dim = Color32::from_rgb(130, 130, 138);

    let font = FontId::monospace(11.0);
    let rows = cat::big_cat(app.tick, app.media_playing());
    let row_h = 12.5;
    // A little sideways head-bob while a song really plays.
    let playing = app.media_playing();
    let jiggle = if cat::is_singing(app.tick, playing) && app.tick % 4 < 2 { 2.0 } else { 0.0 };
    let x = rect.min.x + 14.0 + jiggle;
    let top = rect.center().y - row_h * rows.len() as f32 / 2.0;
    let mut art_w = 0.0f32;
    for (i, row) in rows.iter().enumerate() {
        let r = painter.text(
            Pos2::new(x, top + i as f32 * row_h),
            Align2::LEFT_TOP,
            row.trim_end(),
            font.clone(),
            fur,
        );
        art_w = art_w.max(r.width());
    }

    // Name + context text.
    let name_x = rect.min.x + 14.0 + art_w.max(76.0) + 16.0;
    painter.text(
        Pos2::new(name_x, rect.center().y),
        Align2::LEFT_CENTER,
        &app.config.cat.name,
        FontId::proportional(17.0),
        Color32::from_rgb(235, 235, 238),
    );
    let name_w = app.config.cat.name.chars().count() as f32 * 9.5;
    painter.text(
        Pos2::new(name_x + 8.0 + name_w, rect.center().y),
        Align2::LEFT_CENTER,
        "· silver",
        FontId::proportional(14.0),
        dim,
    );
    let hint = match app.screen {
        Screen::Home => "home · scroll with the wheel · ctrl+q quit",
        Screen::Editor => "editor · ctrl+t terminal · ctrl+h home",
    };
    painter.text(
        Pos2::new(rect.max.x - 14.0, rect.center().y),
        Align2::RIGHT_CENTER,
        hint,
        FontId::proportional(13.0),
        dim,
    );
    // Accent separator under the bar.
    painter.line_segment(
        [
            Pos2::new(rect.min.x, rect.max.y - 1.0),
            Pos2::new(rect.max.x, rect.max.y - 1.0),
        ],
        Stroke::new(1.0, accent.gamma_multiply(0.5)),
    );
}
