pub mod editor_view;
pub mod home;

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{App, Screen};

pub fn draw(f: &mut Frame, app: &mut App) {
    app.mouse_targets.clear();
    app.pane_areas.clear();
    match app.screen {
        Screen::Home => home::draw(f, app),
        Screen::Editor => editor_view::draw(f, app),
    }
    if app.keys_editor.is_some() {
        dim_background(f);
        draw_keys_editor(f, app);
    }
    draw_toast(f, app);
}

/// The customize panel: shortcut keys on one tab, terminal command
/// words on the other. Enter (or a second click) edits — keys are
/// pressed, command words are typed.
fn draw_keys_editor(f: &mut Frame, app: &mut App) {
    let accent = app.config.accent();
    let dim = app.config.dim();
    let Some(ke) = app.keys_editor.as_ref() else { return };
    let (selected, editing, tab, input) = (ke.selected, ke.editing, ke.tab, ke.input.clone());

    let entries: Vec<(String, String)> = if tab == 0 {
        app.config.keys.iter().map(|(a, v)| (a.clone(), v.clone())).collect()
    } else {
        app.config.commands.iter().map(|(a, v)| (a.clone(), v.clone())).collect()
    };
    let n = entries.len() as u16;
    let area = f.area();
    let w = 62.min(area.width.saturating_sub(2));
    let h = (n + 4).min(area.height.saturating_sub(2));
    let rect = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, rect);
    let hint = match (editing, tab) {
        (true, 0) => " press the new keys (with ctrl/alt) · esc: cancel ",
        (true, _) => " type the new word · enter: save · esc: cancel ",
        (false, _) => " tab: switch · enter: edit · d: default · esc: close ",
    };
    let tab_style = |active: bool| {
        if active {
            Style::new().fg(Color::Black).bg(accent).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(dim)
        }
    };
    let block = Block::bordered()
        .border_style(Style::new().fg(accent))
        .title(Line::from(vec![
            Span::styled(" ⌨ ", Style::new().fg(accent)),
            Span::styled(" keys ", tab_style(tab == 0)),
            Span::styled("│", Style::new().fg(dim)),
            Span::styled(" commands ", tab_style(tab == 1)),
        ]))
        .title_bottom(Span::styled(hint, Style::new().fg(dim)));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let mut rows: Vec<Rect> = Vec::new();
    let mut lines: Vec<Line> = Vec::new();
    // Keep the selected row on screen in short windows.
    let visible = inner.height as usize;
    let first = selected.saturating_sub(visible.saturating_sub(1));
    for (i, (action, value)) in entries.iter().enumerate().skip(first).take(visible) {
        let is_sel = i == selected;
        let name = format!(" {:<14}", action.replace('_', " "));
        let value_txt = if is_sel && editing && tab == 0 {
            // Blinks while waiting for the new combination.
            if app.tick % 8 < 4 { format!("{:<12}", "...") } else { format!("{:<12}", "press keys") }
        } else if is_sel && editing {
            format!("{:<12}", format!("{input}▌"))
        } else {
            format!("{:<12}", value)
        };
        let help = if tab == 0 {
            crate::app::key_action_help(action)
        } else {
            crate::app::command_action_help(action)
        };
        let (name_st, value_st, help_st) = if is_sel {
            let base = Style::new().fg(Color::Black).bg(accent);
            (base, base.add_modifier(Modifier::BOLD), base)
        } else {
            (
                Style::new().fg(Color::Rgb(210, 210, 210)),
                Style::new().fg(accent),
                Style::new().fg(dim),
            )
        };
        lines.push(Line::from(vec![
            Span::styled(name, name_st),
            Span::styled(value_txt, value_st),
            Span::styled(format!(" {help}"), help_st),
        ]));
        rows.push(Rect { x: inner.x, y: inner.y + (i - first) as u16, width: inner.width, height: 1 });
    }
    f.render_widget(Paragraph::new(lines), inner);
    for (offset, row) in rows.into_iter().enumerate() {
        app.mouse_targets.push(crate::app::MouseTarget::KeysRow { idx: first + offset, area: row });
    }
}

fn draw_toast(f: &mut Frame, app: &App) {
    let Some((msg, _)) = &app.toast else { return };
    let area = f.area();
    let w = (msg.chars().count() as u16 + 4).min(area.width.saturating_sub(2));
    if w == 0 || area.height < 3 {
        return;
    }
    let rect = Rect {
        x: area.width.saturating_sub(w + 1),
        y: area.height.saturating_sub(2),
        width: w,
        height: 1,
    };
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(Line::from(format!("  {msg}  ")))
            .style(Style::new().fg(Color::Black).bg(app.config.accent())),
        rect,
    );
}

/// Fade everything drawn so far towards the dark background — the
/// terminal's stand-in for a blur — so whatever draws next reads
/// clearly as the focused window.
pub fn dim_background(f: &mut Frame) {
    let area = f.area();
    let buf = f.buffer_mut();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.fg = fade(cell.fg);
                cell.bg = fade(cell.bg);
                cell.modifier.remove(ratatui::style::Modifier::BOLD);
            }
        }
    }
}

/// A color pushed most of the way to the background, with a hint of
/// blue so the faded layer looks frosted rather than broken.
fn fade(c: Color) -> Color {
    let (r, g, b) = match c {
        Color::Reset => return Color::Reset,
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Black => (12, 12, 15),
        Color::Red | Color::LightRed => (224, 92, 92),
        Color::Green | Color::LightGreen => (92, 200, 130),
        Color::Yellow | Color::LightYellow => (229, 192, 92),
        Color::Blue | Color::LightBlue => (96, 140, 247),
        Color::Magenta | Color::LightMagenta => (197, 115, 227),
        Color::Cyan | Color::LightCyan => (92, 214, 214),
        Color::Gray => (160, 160, 165),
        Color::DarkGray => (105, 105, 112),
        Color::White => (238, 238, 240),
        Color::Indexed(_) => (120, 120, 126),
    };
    Color::Rgb(r / 3, g / 3 + 4, b / 3 + 8)
}

/// A rectangle centered in `r`, sized by percentage.
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let w = r.width * percent_x / 100;
    let h = r.height * percent_y / 100;
    Rect {
        x: r.x + (r.width.saturating_sub(w)) / 2,
        y: r.y + (r.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

/// First visible index so `selected` stays in a window of height `h`.
pub fn scroll_offset(selected: usize, h: usize) -> usize {
    if h == 0 {
        return 0;
    }
    (selected + 1).saturating_sub(h)
}
