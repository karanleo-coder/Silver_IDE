use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::cat;
use crate::config::{parse_color, CAT_COLORS};
use crate::ui::{centered_rect, scroll_offset};

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let accent = app.config.accent();
    let dim = app.config.dim();
    let cat_color = app.config.cat_color();

    let [header, body, footer] = Layout::vertical([
        Constraint::Length(6),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(area);

    // ---- header: the cat ----
    let art = cat::big_cat(app.tick, app.media_playing());
    let mut lines: Vec<Line> = vec![Line::from("")];
    for row in art {
        lines.push(Line::from(Span::styled(
            row,
            Style::new().fg(cat_color).add_modifier(Modifier::BOLD),
        )));
    }
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {}  ", app.config.cat.name),
            Style::new().fg(cat_color).add_modifier(Modifier::BOLD),
        ),
        Span::styled("· silver cli ", Style::new().fg(dim)),
        Span::styled("· press c to customize", Style::new().fg(accent)),
    ]));
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), header);

    // ---- body: recents | side terminal ----
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
            .areas(body);

    if app.home.browser.is_some() {
        draw_browser(f, app, left);
    } else {
        draw_recents(f, app, left);
    }
    app.mouse_targets.push(crate::app::MouseTarget::HomeTerminal { area: right });
    draw_side_terminal(f, app, right);

    // ---- footer ----
    let footer_text = if app.home.browser.is_some() {
        Line::from(vec![
            Span::styled(" tab ", Style::new().fg(Color::Black).bg(accent)),
            Span::styled(" switch focus  ", Style::new().fg(dim)),
            Span::styled(" enter ", Style::new().fg(Color::Black).bg(accent)),
            Span::styled(" copy path  ", Style::new().fg(dim)),
            Span::styled(" s ", Style::new().fg(Color::Black).bg(accent)),
            Span::styled(" start here  ", Style::new().fg(dim)),
            Span::styled(" bksp ", Style::new().fg(Color::Black).bg(accent)),
            Span::styled(" up a folder  ", Style::new().fg(dim)),
            Span::styled(" esc ", Style::new().fg(Color::Black).bg(accent)),
            Span::styled(" recents ", Style::new().fg(dim)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" tab ", Style::new().fg(Color::Black).bg(accent)),
            Span::styled(" switch focus  ", Style::new().fg(dim)),
            Span::styled(" ↑/↓ + enter ", Style::new().fg(Color::Black).bg(accent)),
            Span::styled(" open recent  ", Style::new().fg(dim)),
            Span::styled(" c ", Style::new().fg(Color::Black).bg(accent)),
            Span::styled(" customize cat  ", Style::new().fg(dim)),
            Span::styled(" k ", Style::new().fg(Color::Black).bg(accent)),
            Span::styled(" shortcut keys  ", Style::new().fg(dim)),
            Span::styled(" ctrl+q ", Style::new().fg(Color::Black).bg(accent)),
            Span::styled(" quit ", Style::new().fg(dim)),
        ])
    };
    f.render_widget(Paragraph::new(footer_text), footer);

    if app.home.customize.is_some() {
        crate::ui::dim_background(f);
        draw_customize(f, app);
    }
}

fn draw_recents(f: &mut Frame, app: &mut App, area: Rect) {
    let accent = app.config.accent();
    let dim = app.config.dim();
    let focused = !app.home.focus_terminal && app.home.customize.is_none();

    let border_style = if focused { Style::new().fg(accent) } else { Style::new().fg(dim) };
    let block = Block::bordered()
        .border_style(border_style)
        .title(Span::styled(" recent projects ", Style::new().fg(accent).add_modifier(Modifier::BOLD)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.config.recents.is_empty() {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled("  no projects yet", Style::new().fg(dim))),
                Line::from(""),
                Line::from(Span::styled(
                    "  press tab, then:  cd ~/path/to/your/project",
                    Style::new().fg(Color::Rgb(220, 220, 220)),
                )),
            ]),
            inner,
        );
        return;
    }

    let h = inner.height as usize;
    let start = scroll_offset(app.home.selected, h);
    let mut lines: Vec<Line> = Vec::new();
    for (i, r) in app.config.recents.iter().enumerate().skip(start).take(h) {
        let selected = i == app.home.selected;
        let marker = if selected { "▸ " } else { "  " };
        let name_style = if selected {
            Style::new().fg(Color::Black).bg(accent).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(Color::Rgb(220, 220, 220))
        };
        lines.push(Line::from(vec![
            Span::styled(marker, Style::new().fg(accent)),
            Span::styled(format!(" {} ", r.name), name_style),
            Span::styled(format!("  {}", r.path), Style::new().fg(dim)),
        ]));
    }
    f.render_widget(Paragraph::new(lines), inner);
    app.mouse_targets.push(crate::app::MouseTarget::HomeList { area: inner, first: start });
}

fn draw_browser(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(br) = &app.home.browser else { return };
    let accent = app.config.accent();
    let dim = app.config.dim();
    let focused = !app.home.focus_terminal && app.home.customize.is_none();

    // Show the tail of the path if it is too wide for the title.
    let path = br.dir.display().to_string();
    let max = area.width.saturating_sub(6) as usize;
    let title = if path.chars().count() > max {
        let tail: String = path.chars().rev().take(max.saturating_sub(1)).collect::<Vec<_>>()
            .into_iter().rev().collect();
        format!(" …{tail} ")
    } else {
        format!(" {path} ")
    };

    let border_style = if focused { Style::new().fg(accent) } else { Style::new().fg(dim) };
    let block = Block::bordered()
        .border_style(border_style)
        .title(Span::styled(title, Style::new().fg(accent).add_modifier(Modifier::BOLD)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if br.entries.is_empty() {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled("  empty folder", Style::new().fg(dim))),
                Line::from(""),
                Line::from(Span::styled(
                    "  `start` opens the editor here",
                    Style::new().fg(Color::Rgb(220, 220, 220)),
                )),
            ]),
            inner,
        );
        return;
    }

    let h = inner.height as usize;
    let start = scroll_offset(br.selected, h);
    let mut lines: Vec<Line> = Vec::new();
    for (i, e) in br.entries.iter().enumerate().skip(start).take(h) {
        let selected = i == br.selected && focused;
        let marker = if selected { "▸ " } else { "  " };
        let mut name = e
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if e.is_dir {
            name.push('/');
        }
        let name_style = if selected {
            Style::new().fg(Color::Black).bg(accent).add_modifier(Modifier::BOLD)
        } else if e.is_dir {
            Style::new().fg(accent)
        } else {
            Style::new().fg(Color::Rgb(220, 220, 220))
        };
        lines.push(Line::from(vec![
            Span::styled(marker, Style::new().fg(accent)),
            Span::styled(format!(" {name} "), name_style),
        ]));
    }
    f.render_widget(Paragraph::new(lines), inner);
    app.mouse_targets.push(crate::app::MouseTarget::HomeList { area: inner, first: start });
}

fn draw_side_terminal(f: &mut Frame, app: &App, area: Rect) {
    let accent = app.config.accent();
    let dim = app.config.dim();
    let focused = app.home.focus_terminal && app.home.customize.is_none();

    let border_style = if focused { Style::new().fg(accent) } else { Style::new().fg(dim) };
    let block = Block::bordered()
        .border_style(border_style)
        .title(Span::styled(" terminal ", Style::new().fg(accent).add_modifier(Modifier::BOLD)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height < 2 {
        return;
    }

    let [out_area, in_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);

    let mut h = out_area.height as usize;
    let total = app.home.term_output.len();
    let scroll = app.home.term_scroll.min(total.saturating_sub(h));
    if scroll > 0 {
        h = h.saturating_sub(1); // reserve a row for the indicator
    }
    let end = total - scroll;
    let start = end.saturating_sub(h);
    let mut lines: Vec<Line> = app.home.term_output[start..end]
        .iter()
        .map(|l| Line::from(Span::styled(l.clone(), Style::new().fg(Color::Rgb(190, 190, 190)))))
        .collect();
    if scroll > 0 {
        lines.push(Line::from(Span::styled(
            format!("· · · {scroll} lines below (pgdn / wheel) · · ·"),
            Style::new().fg(dim),
        )));
    }
    f.render_widget(Paragraph::new(lines), out_area);

    let prompt = "~ $ ";
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(prompt, Style::new().fg(accent).add_modifier(Modifier::BOLD)),
            Span::styled(app.home.term_input.clone(), Style::new().fg(Color::White)),
        ])),
        in_area,
    );
    if focused {
        let x = in_area.x + prompt.len() as u16 + app.home.term_input.chars().count() as u16;
        f.set_cursor_position((x.min(in_area.right().saturating_sub(1)), in_area.y));
    }
}

fn draw_customize(f: &mut Frame, app: &App) {
    let Some(cust) = &app.home.customize else { return };
    let accent = app.config.accent();
    let dim = app.config.dim();
    let color_name = CAT_COLORS[cust.color_idx];
    let color = parse_color(color_name);

    let rect = centered_rect(50, 55, f.area());
    f.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_style(Style::new().fg(accent))
        .title(Span::styled(" customize your cat ", Style::new().fg(accent).add_modifier(Modifier::BOLD)));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let mut lines: Vec<Line> = vec![Line::from("")];
    for row in crate::cat::big_cat(app.tick, app.media_playing()) {
        lines.push(Line::from(Span::styled(row, Style::new().fg(color).add_modifier(Modifier::BOLD))));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  name:  ", Style::new().fg(dim)),
        Span::styled(cust.name.clone(), Style::new().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::styled("▏", Style::new().fg(accent)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  color: ", Style::new().fg(dim)),
        Span::styled("◂ ", Style::new().fg(accent)),
        Span::styled(color_name, Style::new().fg(color).add_modifier(Modifier::BOLD)),
        Span::styled(" ▸", Style::new().fg(accent)),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  type to rename · ←/→ color · enter save · esc cancel",
        Style::new().fg(dim),
    )));
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
}
