use chrono::{Local, Timelike};
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{App, EditorState};
use crate::cat;
use crate::editor::highlight::highlight_line;
use crate::ui::{centered_rect, scroll_offset};


pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let accent = app.config.accent();
    let dim = app.config.dim();

    let [header, rule, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(area);

    draw_header(f, app, header);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(rule.width as usize),
            Style::new().fg(dim),
        ))),
        rule,
    );

    // ---- body: optional files panel + editor panes + the file rail ----
    let files_open = app.editor.as_ref().map(|e| e.files.is_some()).unwrap_or(false);
    let any_tabs = app
        .editor
        .as_ref()
        .map(|e| e.panes.iter().any(|p| !p.tabs.is_empty()))
        .unwrap_or(false);
    let (panel_area, mut panes_area) = if files_open {
        let [p, e] =
            Layout::horizontal([Constraint::Length(34), Constraint::Min(10)]).areas(body);
        (Some(p), e)
    } else {
        (None, body)
    };
    // The open-files rail keeps its column on the right edge.
    let rail_area = if any_tabs {
        let [c, r] =
            Layout::horizontal([Constraint::Min(10), Constraint::Length(5)]).areas(panes_area);
        panes_area = c;
        Some(r)
    } else {
        None
    };

    draw_panes(f, app, panes_area);
    if let Some(rail) = rail_area {
        draw_rail(f, app, rail);
    }
    // Any floating window on top: fade what's behind it first, so the
    // window and the background are easy to tell apart.
    let window_open = app
        .editor
        .as_ref()
        .map(|e| {
            e.files.is_some()
                || e.location.is_some()
                || e.place.is_some()
                || e.popup.is_some()
                || e.switcher.is_some()
                || e.run_report.is_some()
        })
        .unwrap_or(false);
    if window_open {
        crate::ui::dim_background(f);
    }
    if let Some(panel) = panel_area {
        draw_files_panel(f, app, panel);
    }
    if app.editor.as_ref().map(|e| e.location.is_some()).unwrap_or(false) {
        draw_location(f, app, body);
    }
    if app.editor.as_ref().map(|e| e.place.is_some()).unwrap_or(false) {
        draw_place(f, app, body);
    }
    if app.editor.as_ref().map(|e| e.popup.is_some()).unwrap_or(false) {
        draw_popup_terminal(f, app);
    }
    if app.editor.as_ref().map(|e| e.switcher.is_some()).unwrap_or(false) {
        draw_switcher(f, app);
    }
    if app.editor.as_ref().map(|e| e.run_report.is_some()).unwrap_or(false) {
        draw_run_report(f, app);
    }

    // ---- footer ----
    let keys = &app.config.keys;
    let hint = |action: &str| keys.get(action).cloned().unwrap_or_default();
    let footer_text = Line::from(vec![
        Span::styled(format!(" {} ", hint("popup_terminal")), Style::new().fg(Color::Black).bg(accent)),
        Span::styled(" terminal  ", Style::new().fg(dim)),
        Span::styled(format!(" {} ", hint("save")), Style::new().fg(Color::Black).bg(accent)),
        Span::styled(" save  ", Style::new().fg(dim)),
        Span::styled(format!(" {} ", hint("files_panel")), Style::new().fg(Color::Black).bg(accent)),
        Span::styled(" files  ", Style::new().fg(dim)),
        Span::styled(format!(" {} ", hint("open_right")), Style::new().fg(Color::Black).bg(accent)),
        Span::styled(" open right  ", Style::new().fg(dim)),
        Span::styled(format!(" {} ", hint("location")), Style::new().fg(Color::Black).bg(accent)),
        Span::styled(" location  ", Style::new().fg(dim)),
        Span::styled(format!(" {} ", hint("switch_pane")), Style::new().fg(Color::Black).bg(accent)),
        Span::styled(" pane  ", Style::new().fg(dim)),
        Span::styled(format!(" {} ", hint("cycle_files")), Style::new().fg(Color::Black).bg(accent)),
        Span::styled(" cycle files  ", Style::new().fg(dim)),
        Span::styled(format!(" {} ", hint("home")), Style::new().fg(Color::Black).bg(accent)),
        Span::styled(" home ", Style::new().fg(dim)),
    ]);
    f.render_widget(Paragraph::new(footer_text), footer);
}

const WAVE_CHARS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

fn abbreviate_home(path: &std::path::Path) -> String {
    let s = path.to_string_lossy().to_string();
    if let Some(base) = directories::BaseDirs::new() {
        let home = base.home_dir().to_string_lossy().to_string();
        if let Some(rest) = s.strip_prefix(&home) {
            return format!("~{rest}");
        }
    }
    s
}

fn draw_header(f: &mut Frame, app: &mut App, area: Rect) {
    let accent = app.config.accent();
    let dim = app.config.dim();
    let cat_color = app.config.cat_color();
    let mut run_target: Option<Rect> = None;
    let Some(ed) = app.editor.as_ref() else { return };

    let [loc_a, file_a, clock_a, audio_a] = Layout::horizontal([
        Constraint::Fill(3),
        Constraint::Fill(3),
        Constraint::Length(23),
        Constraint::Length(23),
    ])
    .areas(area);

    // 1. location
    let loc = abbreviate_home(&ed.root);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ⌂ ", Style::new().fg(accent)),
            Span::styled(loc, Style::new().fg(Color::Rgb(200, 200, 200))),
        ])),
        loc_a,
    );

    // 2. file name + extension + the ▶ run button
    let shown = ed.panes.get(ed.active).and_then(|p| p.buf());
    let is_file = shown.map(|b| b.term.is_none()).unwrap_or(false);
    let (name, ext, dirty) = shown
        .map(|b| (b.name(), b.ext(), b.dirty))
        .unwrap_or(("no file open".into(), String::new(), false));
    let mut file_spans = vec![Span::styled("│ ", Style::new().fg(dim))];
    if dirty {
        file_spans.push(Span::styled("● ", Style::new().fg(accent)));
    }
    file_spans.push(Span::styled(name, Style::new().fg(Color::White).add_modifier(Modifier::BOLD)));
    if !ext.is_empty() {
        file_spans.push(Span::styled(format!("  ·{ext}"), Style::new().fg(dim)));
    }
    // Problem counts from the last check, right by the name.
    let (errs, warns) = shown
        .map(|b| {
            let e = b.diags.iter().filter(|d| !d.warning).count();
            (e, b.diags.len() - e)
        })
        .unwrap_or((0, 0));
    if errs > 0 {
        file_spans.push(Span::styled(
            format!("  ✗{errs}"),
            Style::new().fg(Color::Rgb(235, 110, 110)).add_modifier(Modifier::BOLD),
        ));
    }
    if warns > 0 {
        file_spans.push(Span::styled(
            format!("  !{warns}"),
            Style::new().fg(Color::Rgb(229, 192, 92)),
        ));
    }
    if is_file {
        let used: u16 = file_spans.iter().map(|s| s.content.chars().count() as u16).sum();
        let label = " ▶ run ";
        let w = label.chars().count() as u16;
        let x = file_a.x + used + 2;
        if x + w <= file_a.right() {
            file_spans.push(Span::raw("  "));
            file_spans.push(Span::styled(
                label,
                Style::new().fg(Color::Black).bg(accent).add_modifier(Modifier::BOLD),
            ));
            run_target = Some(Rect { x, y: file_a.y, width: w, height: 1 });
        }
    }
    f.render_widget(Paragraph::new(Line::from(file_spans)), file_a);

    // 3. date + time with sun/moon
    let now = Local::now();
    let icon = if (6..18).contains(&now.hour()) { "☀" } else { "☾" };
    let clock = format!("│ {icon} {}", now.format("%a %d %b · %H:%M"));
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(clock, Style::new().fg(Color::Rgb(200, 200, 200))))),
        clock_a,
    );

    // 4. the audio wave + the cat. The wave only moves (and the cat
    // only jiggles) while something actually plays on the machine.
    let playing = app.media_playing();
    let mut wave = String::new();
    for i in 0..7u64 {
        let phase = if playing { app.tick as f32 * 0.35 } else { 0.0 };
        let t = phase + i as f32 * 0.9;
        let level = ((t.sin() * 0.5 + 0.5) * 8.0) as usize;
        wave.push(WAVE_CHARS[level.min(8)]);
    }
    let cursor_x = ed.panes.get(ed.active).and_then(|p| p.buf()).map(|b| b.cx).unwrap_or(0);
    let head = if playing {
        cat::jiggling_cat(app.tick, cursor_x)
    } else {
        format!("{} ", cat::small_cat(app.tick, cursor_x))
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("│ ♪ ", Style::new().fg(dim)),
            Span::styled(wave, Style::new().fg(if playing { accent } else { dim })),
            Span::styled(
                format!(" {head} "),
                Style::new().fg(cat_color).add_modifier(Modifier::BOLD),
            ),
        ]))
        .alignment(Alignment::Right),
        audio_a,
    );
    if let Some(area) = run_target {
        app.mouse_targets.push(crate::app::MouseTarget::RunButton { area });
    }
}

fn draw_panes(f: &mut Frame, app: &mut App, area: Rect) {
    let accent = app.config.accent();
    let dim = app.config.dim();
    let cat_color = app.config.cat_color();
    let dragging = app.drag.is_some();
    let popup_open;
    let overlay_open;
    let n_panes;
    let active;
    let no_tabs;
    let split;
    {
        let Some(ed) = app.editor.as_ref() else { return };
        popup_open = ed.popup.is_some();
        overlay_open = ed.files.is_some() || ed.location.is_some();
        n_panes = ed.panes.len();
        active = ed.active;
        no_tabs = ed.panes.iter().all(|p| p.tabs.is_empty());
        split = ed.split.clamp(20, 80);
    }

    if no_tabs {
        let block = Block::bordered().border_style(Style::new().fg(dim));
        let inner = block.inner(area);
        f.render_widget(block, area);
        let mut lines: Vec<Line> = vec![Line::from("")];
        for row in cat::big_cat(app.tick, app.media_playing()) {
            lines.push(Line::from(Span::styled(row, Style::new().fg(cat_color))));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "no file open yet",
            Style::new().fg(Color::Rgb(200, 200, 200)).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        let term_key = app.config.keys.get("popup_terminal").cloned().unwrap_or_default();
        lines.push(Line::from(Span::styled(
            format!("press {term_key} and type `ls` to browse the project files"),
            Style::new().fg(dim),
        )));
        lines.push(Line::from(Span::styled(
            "pick a file to copy its path, then `open` it",
            Style::new().fg(dim),
        )));
        f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
        return;
    }

    let pane_areas: Vec<Rect> = if n_panes == 1 {
        vec![area]
    } else {
        // The user drags the divider (or alt+←/→) to move this split.
        let [l, r] = Layout::horizontal([
            Constraint::Percentage(split),
            Constraint::Percentage(100 - split),
        ])
        .areas(area);
        vec![l, r]
    };
    app.pane_areas = pane_areas.clone();

    let mut pane_targets: Vec<crate::app::MouseTarget> = Vec::new();
    // The active pane's cursor cell, for hanging the suggestion popup.
    let mut comp_anchor: Option<(u16, u16)> = None;
    let ed = app.editor.as_mut().unwrap();
    let can_split = n_panes == 1 && ed.panes[0].tabs.len() > 1;
    for (i, (pane, pane_area)) in ed.panes.iter_mut().zip(pane_areas.iter()).enumerate() {
        let is_active = i == active;
        let border = if is_active && !popup_open && !overlay_open {
            Style::new().fg(accent)
        } else {
            Style::new().fg(dim)
        };
        let block = Block::bordered().border_style(border);
        let inner = block.inner(*pane_area);
        f.render_widget(block, *pane_area);
        if inner.height == 0 || inner.width < 7 {
            continue;
        }

        // ---- the pane's shown file (all file names live in the rail) ----
        let tab_idx = pane.tab.min(pane.tabs.len().saturating_sub(1));
        let Some(buf) = pane.tabs.get_mut(tab_idx) else { continue };
        // Terminal tabs render their own live view.
        if buf.term.is_some() {
            pane_targets.push(crate::app::MouseTarget::EditorPane { pane: i, area: inner });
            let focused = is_active && !popup_open && !overlay_open;
            draw_term_pane(f, buf, inner, focused, accent, dim);
            continue;
        }
        let [gutter_a, text_a] =
            Layout::horizontal([Constraint::Length(6), Constraint::Min(1)]).areas(inner);
        pane_targets.push(crate::app::MouseTarget::EditorPane { pane: i, area: text_a });

        buf.view_h = text_a.height as usize;
        buf.view_w = text_a.width as usize;
        buf.ensure_visible();

        // line -> is-warning; errors win when a line has both.
        let mut diag_lines: std::collections::BTreeMap<usize, bool> =
            std::collections::BTreeMap::new();
        for d in &buf.diags {
            let e = diag_lines.entry(d.line).or_insert(d.warning);
            if !d.warning {
                *e = false;
            }
        }
        let err_fg = Color::Rgb(235, 110, 110);
        let warn_fg = Color::Rgb(229, 192, 92);

        let h = text_a.height as usize;
        let ext = buf.ext();
        let mut gutter_lines: Vec<Line> = Vec::with_capacity(h);
        let mut text_lines: Vec<Line> = Vec::with_capacity(h);
        for row in 0..h {
            let idx = buf.scroll + row;
            if idx >= buf.lines.len() {
                gutter_lines.push(Line::from(Span::styled("    ~", Style::new().fg(dim))));
                text_lines.push(Line::from(""));
                continue;
            }
            let num_style = if idx == buf.cy && is_active {
                Style::new().fg(accent).add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(dim)
            };
            // Marker column: ● stop point, ✗/! a problem on that line.
            let marker = if buf.breakpoints.contains(&idx) {
                Span::styled("●", Style::new().fg(err_fg))
            } else {
                match diag_lines.get(&idx) {
                    Some(true) => Span::styled("!", Style::new().fg(warn_fg)),
                    Some(false) => Span::styled("✗", Style::new().fg(err_fg)),
                    None => Span::raw(" "),
                }
            };
            gutter_lines.push(Line::from(vec![
                marker,
                Span::styled(format!("{:>4}", idx + 1), num_style),
            ]));
            let mut line = highlight_line(&buf.lines[idx], &ext);
            // Problem lines get a colored wash under the syntax colors.
            if let Some(warning) = diag_lines.get(&idx) {
                let wash =
                    if *warning { Color::Rgb(56, 48, 18) } else { Color::Rgb(64, 24, 24) };
                for s in &mut line.spans {
                    s.style = s.style.bg(wash);
                }
            }
            text_lines.push(line);
        }
        f.render_widget(Paragraph::new(gutter_lines), gutter_a);
        f.render_widget(
            Paragraph::new(text_lines).scroll((0, buf.hscroll as u16)),
            text_a,
        );

        // The cursor line's problem, spelled out along the pane's edge.
        if is_active && text_a.height >= 2 {
            if let Some(d) = buf.diags.iter().find(|d| d.line == buf.cy) {
                let on_last_row =
                    buf.cy.saturating_sub(buf.scroll) as u16 == text_a.height - 1;
                let bar = Rect {
                    x: text_a.x,
                    y: if on_last_row { text_a.y } else { text_a.bottom() - 1 },
                    width: text_a.width,
                    height: 1,
                };
                let fg = if d.warning { warn_fg } else { err_fg };
                let text: String = format!(" {} line {}: {} ", if d.warning { "!" } else { "✗" }, d.line + 1, d.message)
                    .chars()
                    .take(bar.width as usize)
                    .collect();
                f.render_widget(Clear, bar);
                f.render_widget(
                    Paragraph::new(Span::styled(
                        text,
                        Style::new().fg(fg).bg(Color::Rgb(34, 20, 20)),
                    )),
                    bar,
                );
            }
        }

        if is_active && !popup_open && !overlay_open {
            let cx = (buf.cx - buf.hscroll.min(buf.cx)) as u16;
            let cy = (buf.cy - buf.scroll.min(buf.cy)) as u16;
            if cx < text_a.width && cy < text_a.height {
                f.set_cursor_position((text_a.x + cx, text_a.y + cy));
                comp_anchor = Some((text_a.x + cx, text_a.y + cy));
            }
        }
    }
    // The suggestion popup, floating at the cursor.
    if let (Some((ax, ay)), Some(comp)) = (comp_anchor, ed.completion.as_ref()) {
        let maxw = comp.items.iter().map(|s| s.chars().count()).max().unwrap_or(0) as u16;
        let w = (maxw + 4).clamp(18, 44).min(area.width);
        let h = (comp.items.len() as u16 + 2).min(area.height);
        let y = if ay + 1 + h <= area.bottom() { ay + 1 } else { ay.saturating_sub(h) };
        let x = ax.min(area.right().saturating_sub(w)).max(area.x);
        let rect = Rect { x, y: y.max(area.y), width: w, height: h };
        f.render_widget(Clear, rect);
        let block = Block::bordered()
            .border_style(Style::new().fg(accent))
            .title(Span::styled(" ✦ ideas ", Style::new().fg(accent).add_modifier(Modifier::BOLD)))
            .title_bottom(Span::styled(" tab picks · esc ", Style::new().fg(dim)));
        let inner = block.inner(rect);
        f.render_widget(block, rect);
        let lines: Vec<Line> = comp
            .items
            .iter()
            .enumerate()
            .map(|(ci, item)| {
                let style = if ci == comp.selected {
                    Style::new().fg(Color::Black).bg(accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(Color::Rgb(210, 210, 210))
                };
                Line::from(Span::styled(format!(" {item} "), style))
            })
            .collect();
        f.render_widget(Paragraph::new(lines), inner);
    }

    // While dragging over a lone pane, mark the right half as the split target.
    if dragging && can_split {
        let a = pane_areas[0];
        if a.width >= 8 && a.height >= 3 {
            let hint = Rect {
                x: a.x + a.width / 2,
                y: a.y + a.height / 2,
                width: a.width / 2 - 1,
                height: 1,
            };
            f.render_widget(Clear, hint);
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "⇥ drop to split ⇥",
                    Style::new().fg(accent).add_modifier(Modifier::BOLD),
                )))
                .alignment(Alignment::Center),
                hint,
            );
        }
    }
    app.mouse_targets.extend(pane_targets);
}

fn entry_line(entry: &crate::app::FileEntry, selected: bool, expanded: bool, accent: Color, dim: Color) -> Line<'static> {
    let name = entry
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let indent = "  ".repeat(entry.depth);
    let (label, style) = if entry.is_dir {
        // Folders render as minimized "buttons"; press to expand.
        let mark = if expanded { "▾" } else { "▸" };
        (format!("{indent}[{mark} {name}]"), Style::new().fg(accent))
    } else {
        (format!("{indent}  {name}"), Style::new().fg(Color::Rgb(210, 210, 210)))
    };
    let style = if selected {
        Style::new().fg(Color::Black).bg(accent).add_modifier(Modifier::BOLD)
    } else {
        style
    };
    let _ = dim;
    Line::from(Span::styled(label, style))
}

/// vt100's colors -> ratatui's, keeping whatever the program asked for.
fn vt_color(c: vt100::Color, default: Color) -> Color {
    match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// A terminal tab's live view: the emulated PTY screen, cell by cell,
/// so interactive programs render just like in a normal terminal.
fn draw_term_pane(
    f: &mut Frame,
    buf: &mut crate::editor::buffer::Buffer,
    area: Rect,
    focused: bool,
    accent: Color,
    dim: Color,
) {
    let Some(term) = buf.term.as_mut() else { return };
    if area.height < 1 || area.width < 2 {
        return;
    }
    if let Some(err) = &term.error {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled("couldn't start a shell here:", Style::new().fg(dim))),
                Line::from(Span::styled(format!("  {err}"), Style::new().fg(Color::Red))),
            ]),
            area,
        );
        return;
    }
    term.resize(area.height, area.width);
    let alive = term.is_running();

    let mut cursor: Option<(u16, u16)> = None;
    let mut lines: Vec<Line> = Vec::with_capacity(area.height as usize);
    term.with_screen(|screen| {
        for row in 0..area.height {
            // Merge runs of same-styled cells into single spans.
            let mut spans: Vec<Span> = Vec::new();
            let mut run = String::new();
            let mut run_style = Style::new();
            let mut col = 0u16;
            while col < area.width {
                let Some(cell) = screen.cell(row, col) else { break };
                if cell.is_wide_continuation() {
                    col += 1;
                    continue;
                }
                let mut style = Style::new()
                    .fg(vt_color(cell.fgcolor(), Color::Rgb(210, 210, 210)))
                    .bg(vt_color(cell.bgcolor(), Color::Reset));
                if cell.bold() {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if cell.italic() {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                if cell.underline() {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                if cell.inverse() {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                if style != run_style && !run.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut run), run_style));
                }
                run_style = style;
                if cell.has_contents() {
                    run.push_str(&cell.contents());
                } else {
                    run.push(' ');
                }
                col += if cell.is_wide() { 2 } else { 1 };
            }
            if !run.is_empty() {
                spans.push(Span::styled(run, run_style));
            }
            lines.push(Line::from(spans));
        }
        if !screen.hide_cursor() {
            let (r, c) = screen.cursor_position();
            cursor = Some((c, r));
        }
    });
    f.render_widget(Paragraph::new(lines), area);

    let scroll = term.scroll;
    if scroll > 0 {
        let note = format!(" ↑ {scroll} rows back · pgdn returns ");
        let w = (note.chars().count() as u16).min(area.width);
        let tag = Rect { x: area.right().saturating_sub(w), y: area.y, width: w, height: 1 };
        f.render_widget(Clear, tag);
        f.render_widget(
            Paragraph::new(Span::styled(note, Style::new().fg(Color::Black).bg(accent))),
            tag,
        );
    }
    if !alive {
        let note = " shell exited — ctrl+x closes this tab ";
        let w = (note.chars().count() as u16).min(area.width);
        let tag = Rect {
            x: area.x,
            y: area.bottom().saturating_sub(1),
            width: w,
            height: 1,
        };
        f.render_widget(Clear, tag);
        f.render_widget(Paragraph::new(Span::styled(note, Style::new().fg(dim))), tag);
    }
    if focused && scroll == 0 {
        if let Some((cx, cy)) = cursor {
            if cx < area.width && cy < area.height {
                f.set_cursor_position((area.x + cx, area.y + cy));
            }
        }
    }
}

/// The always-there vertical rail on the right edge: one slot per open
/// file, in pane order. Hover a slot to see the file's name, press it
/// to show that file; the `＋` at the end opens the "open here" picker.
fn draw_rail(f: &mut Frame, app: &mut App, area: Rect) {
    let accent = app.config.accent();
    let dim = app.config.dim();
    let hover = app.hover;
    let mut targets: Vec<crate::app::MouseTarget> = Vec::new();
    let mut tooltip: Option<(u16, String)> = None;
    {
        let Some(ed) = app.editor.as_ref() else { return };
        let block = Block::bordered()
            .border_style(Style::new().fg(dim))
            .title(Span::styled("⋮", Style::new().fg(dim)));
        let inner = block.inner(area);
        f.render_widget(block, area);
        if inner.height < 2 {
            return;
        }

        let mut lines: Vec<Line> = Vec::new();
        let mut row: u16 = 0;
        let max_rows = inner.height - 1; // keep a row for the `＋`
        for (pi, pane) in ed.panes.iter().enumerate() {
            for (ti, buf) in pane.tabs.iter().enumerate() {
                if row >= max_rows {
                    break;
                }
                let shown = pane.tab == ti;
                let active = shown && pi == ed.active;
                let initial = buf.name().chars().next().unwrap_or('·');
                let style = if active {
                    Style::new().fg(Color::Black).bg(accent).add_modifier(Modifier::BOLD)
                } else if shown {
                    Style::new().fg(accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(dim)
                };
                let mark = if buf.dirty { "●" } else { " " };
                lines.push(Line::from(Span::styled(format!(" {initial}{mark}"), style)));
                let slot = Rect { x: area.x, y: inner.y + row, width: area.width, height: 1 };
                targets.push(crate::app::MouseTarget::Tab { pane: pi, tab: ti, area: slot });
                if let Some((hx, hy)) = hover {
                    if slot.contains(Position { x: hx, y: hy }) {
                        let side = if pi == 0 { "left" } else { "right" };
                        tooltip = Some((
                            slot.y,
                            format!(
                                " {}{} · {side} ",
                                buf.name(),
                                if buf.dirty { " ●" } else { "" }
                            ),
                        ));
                    }
                }
                row += 1;
            }
        }
        // The `＋` slot: pick a file to open.
        lines.push(Line::from(Span::styled(" ＋", Style::new().fg(accent))));
        let plus = Rect { x: area.x, y: inner.y + row, width: area.width, height: 1 };
        targets.push(crate::app::MouseTarget::SplitZone { area: plus });
        if let Some((hx, hy)) = hover {
            if plus.contains(Position { x: hx, y: hy }) {
                tooltip = Some((plus.y, " open a file ".into()));
            }
        }
        f.render_widget(Paragraph::new(lines), inner);
    }
    // The hovered slot's name, floating just left of the rail.
    if let Some((y, text)) = tooltip {
        let w = (text.chars().count() as u16).min(area.x);
        let rect = Rect { x: area.x.saturating_sub(w), y, width: w, height: 1 };
        f.render_widget(Clear, rect);
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                text,
                Style::new().fg(Color::Black).bg(accent),
            ))),
            rect,
        );
    }
    app.mouse_targets.extend(targets);
}

/// The "open here" picker: a file list on the right; picking a file
/// shows it there, asking which side to replace when both are busy.
fn draw_place(f: &mut Frame, app: &mut App, body: Rect) {
    let accent = app.config.accent();
    let dim = app.config.dim();
    let Some(ed) = app.editor.as_ref() else { return };
    let Some(pl) = ed.place.as_ref() else { return };

    let w = 40.min(body.width);
    let rect = Rect { x: body.right().saturating_sub(w), y: body.y, width: w, height: body.height };
    f.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_style(Style::new().fg(accent))
        .title(Span::styled(" ＋ open here ", Style::new().fg(accent).add_modifier(Modifier::BOLD)));
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    if inner.height < 2 {
        return;
    }

    let [list_a, hint_a] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
    let h = list_a.height as usize;
    let start = scroll_offset(pl.selected, h);
    let mut lines: Vec<Line> = Vec::new();
    for (i, entry) in pl.entries.iter().enumerate().skip(start).take(h) {
        let expanded = pl.expanded.contains(&entry.path);
        lines.push(entry_line(entry, i == pl.selected, expanded, accent, dim));
    }
    if pl.entries.is_empty() {
        lines.push(Line::from(Span::styled("  (empty folder)", Style::new().fg(dim))));
    }
    f.render_widget(Paragraph::new(lines), list_a);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            " enter: put the file there · esc: close",
            Style::new().fg(dim),
        ))),
        hint_a,
    );
    let list_target = crate::app::MouseTarget::PlacePanel { area: list_a, first: start };

    // Both sides busy: ask which file to replace with the picked one.
    let mut side_targets: Vec<crate::app::MouseTarget> = Vec::new();
    if let Some(path) = pl.choosing.as_ref() {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let left_name = ed.panes.first().and_then(|p| p.buf()).map(|b| b.name()).unwrap_or_default();
        let right_name = ed.panes.get(1).and_then(|p| p.buf()).map(|b| b.name()).unwrap_or_default();
        let box_w = (body.width * 2 / 5).clamp(26, 46).min(body.width);
        let box_h = 6.min(body.height);
        let rect = Rect {
            x: body.x + (body.width.saturating_sub(box_w)) / 2,
            y: body.y + (body.height.saturating_sub(box_h)) / 2,
            width: box_w,
            height: box_h,
        };
        f.render_widget(Clear, rect);
        let title = format!(" show {name} where? ");
        let block = Block::bordered()
            .border_style(Style::new().fg(accent))
            .title(Span::styled(title, Style::new().fg(accent).add_modifier(Modifier::BOLD)))
            .title_bottom(Span::styled(" ←/→ choose · enter · esc ", Style::new().fg(dim)));
        let inner = block.inner(rect);
        f.render_widget(block, rect);
        let opt = |label: String, sel: bool| {
            let style = if sel {
                Style::new().fg(Color::Black).bg(accent).add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(Color::Rgb(210, 210, 210))
            };
            Line::from(Span::styled(label, style))
        };
        let mut lines = vec![Line::from("")];
        lines.push(opt(format!("  left — replaces {left_name}  "), pl.side == 0));
        lines.push(opt(format!("  right — replaces {right_name}  "), pl.side == 1));
        f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
        if inner.height >= 3 {
            side_targets.push(crate::app::MouseTarget::PlaceSide {
                side: 0,
                area: Rect { x: inner.x, y: inner.y + 1, width: inner.width, height: 1 },
            });
            side_targets.push(crate::app::MouseTarget::PlaceSide {
                side: 1,
                area: Rect { x: inner.x, y: inner.y + 2, width: inner.width, height: 1 },
            });
        }
    }
    app.mouse_targets.push(list_target);
    app.mouse_targets.extend(side_targets);
}

fn draw_files_panel(f: &mut Frame, app: &mut App, area: Rect) {
    let accent = app.config.accent();
    let dim = app.config.dim();
    let Some(ed) = app.editor.as_ref() else { return };
    let Some(files) = ed.files.as_ref() else { return };

    let block = Block::bordered()
        .border_style(Style::new().fg(accent))
        .title(Span::styled(" files ", Style::new().fg(accent).add_modifier(Modifier::BOLD)));
    let inner = block.inner(area);
    f.render_widget(Clear, area);
    f.render_widget(block, area);
    if inner.height < 2 {
        return;
    }

    let [list_a, hint_a] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);

    let h = list_a.height as usize;
    let start = scroll_offset(files.selected, h);
    let mut lines: Vec<Line> = Vec::new();
    for (i, entry) in files.entries.iter().enumerate().skip(start).take(h) {
        let expanded = files.expanded.contains(&entry.path);
        lines.push(entry_line(entry, i == files.selected, expanded, accent, dim));
    }
    if files.entries.is_empty() {
        lines.push(Line::from(Span::styled("  (empty folder)", Style::new().fg(dim))));
    }
    f.render_widget(Paragraph::new(lines), list_a);
    let hint = if app.config.open_on_click {
        " click/enter: open · esc: close"
    } else {
        " enter: expand / copy path · esc: close"
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(hint, Style::new().fg(dim)))),
        hint_a,
    );
    app.mouse_targets.push(crate::app::MouseTarget::FilesPanel { area: list_a, first: start });
}

fn draw_location(f: &mut Frame, app: &mut App, body: Rect) {
    let accent = app.config.accent();
    let dim = app.config.dim();
    let Some(ed) = app.editor.as_ref() else { return };
    let Some(loc) = ed.location.as_ref() else { return };

    let w = (body.width / 2).clamp(24, 60);
    let h = (body.height * 2 / 3).clamp(6, 24);
    let rect = Rect { x: body.x + 1, y: body.y, width: w.min(body.width), height: h.min(body.height) };
    f.render_widget(Clear, rect);
    let title = format!(" ⌂ {} ", abbreviate_home(&loc.dir));
    let block = Block::bordered()
        .border_style(Style::new().fg(accent))
        .title(Span::styled(title, Style::new().fg(accent).add_modifier(Modifier::BOLD)));
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    if inner.height < 2 {
        return;
    }

    let [list_a, hint_a] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
    let h = list_a.height as usize;
    let start = scroll_offset(loc.selected, h);
    let mut lines: Vec<Line> = Vec::new();
    for (i, entry) in loc.entries.iter().enumerate().skip(start).take(h) {
        lines.push(entry_line(entry, i == loc.selected, false, accent, dim));
    }
    if loc.entries.is_empty() {
        lines.push(Line::from(Span::styled("  (empty folder)", Style::new().fg(dim))));
    }
    f.render_widget(Paragraph::new(lines), list_a);
    let hint = if app.config.open_on_click {
        " click/enter: open · bksp: up · esc"
    } else {
        " enter: open dir / copy path · bksp: up · esc"
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(hint, Style::new().fg(dim)))),
        hint_a,
    );
    app.mouse_targets.push(crate::app::MouseTarget::Location { area: list_a, first: start });
}

/// The Ctrl+Tab switcher: a centered app-switcher style overlay of
/// every open file. Tab / Ctrl+Tab move the highlight, Enter (or a
/// click) swaps the pick into the pane that was last active.
fn draw_switcher(f: &mut Frame, app: &mut App) {
    let accent = app.config.accent();
    let dim = app.config.dim();
    let mut targets: Vec<crate::app::MouseTarget> = Vec::new();
    {
        let Some(ed) = app.editor.as_ref() else { return };
        let Some(sw) = ed.switcher.as_ref() else { return };
        let labels: Vec<(String, bool)> = sw
            .items
            .iter()
            .enumerate()
            .map(|(i, (pi, ti))| {
                let buf = ed.panes.get(*pi).and_then(|p| p.tabs.get(*ti));
                let name = buf.map(|b| b.name()).unwrap_or_default();
                let dirty = buf.map(|b| b.dirty).unwrap_or(false);
                let side = if *pi == 0 { "left" } else { "right" };
                let label =
                    format!(" {}{} · {} ", name, if dirty { "●" } else { "" }, side);
                (label, i == sw.selected)
            })
            .collect();
        let total: u16 = labels
            .iter()
            .map(|(l, _)| l.chars().count() as u16 + 1)
            .sum::<u16>()
            .saturating_sub(1);
        let area = f.area();
        let w = (total + 4).clamp(24, area.width.saturating_sub(2).max(24));
        let h = 5u16.min(area.height);
        let rect = Rect {
            x: area.x + (area.width.saturating_sub(w)) / 2,
            y: area.y + (area.height.saturating_sub(h)) / 2,
            width: w.min(area.width),
            height: h,
        };
        f.render_widget(Clear, rect);
        let block = Block::bordered()
            .border_style(Style::new().fg(accent))
            .title(Span::styled(
                " ⇄ open files ",
                Style::new().fg(accent).add_modifier(Modifier::BOLD),
            ))
            .title_bottom(Span::styled(
                " tab: next · enter: swap here · esc ",
                Style::new().fg(dim),
            ));
        let inner = block.inner(rect);
        f.render_widget(block, rect);
        if inner.height == 0 || labels.is_empty() {
            return;
        }
        let row_y = inner.y + inner.height / 2;

        // Keep the highlighted card in view when the list overflows.
        let width_upto = |start: usize| -> u16 {
            labels[start..=sw.selected.max(start)]
                .iter()
                .map(|(l, _)| l.chars().count() as u16 + 1)
                .sum()
        };
        let mut start = 0usize;
        while start < sw.selected && width_upto(start) > inner.width {
            start += 1;
        }

        let mut x = inner.x + 1;
        let mut spans: Vec<Span> = vec![Span::raw(" ")];
        for (i, (label, sel)) in labels.iter().enumerate().skip(start) {
            let lw = label.chars().count() as u16;
            if x + lw > inner.right() {
                break;
            }
            let style = if *sel {
                Style::new().fg(Color::Black).bg(accent).add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(Color::Rgb(200, 200, 200))
            };
            spans.push(Span::styled(label.clone(), style));
            spans.push(Span::raw(" "));
            targets.push(crate::app::MouseTarget::SwitchItem {
                idx: i,
                area: Rect { x, y: row_y, width: lw, height: 1 },
            });
            x += lw + 1;
        }
        f.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect { x: inner.x, y: row_y, width: inner.width, height: 1 },
        );
    }
    app.mouse_targets.extend(targets);
}

/// The window that appears by itself when a ▶ run fails: the tail of
/// what the program printed, error lines in red, on top of everything.
fn draw_run_report(f: &mut Frame, app: &App) {
    let dim = app.config.dim();
    let Some(ed) = app.editor.as_ref() else { return };
    let Some(rep) = ed.run_report.as_ref() else { return };
    let red = Color::Rgb(235, 110, 110);

    let area = f.area();
    let w = (area.width * 3 / 4).clamp(30, 96).min(area.width);
    let h = (rep.lines.len() as u16 + 4).clamp(7, (area.height * 2 / 3).max(7)).min(area.height);
    let rect = Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_style(Style::new().fg(red))
        .title(Span::styled(
            format!(" ▶ the program failed — exit code {} ", rep.code),
            Style::new().fg(red).add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            " esc closes · the bad lines are marked red in your file ",
            Style::new().fg(dim),
        ));
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    if inner.height == 0 {
        return;
    }

    let show = inner.height as usize;
    let start = rep.lines.len().saturating_sub(show);
    let lines: Vec<Line> = rep.lines[start..]
        .iter()
        .map(|l| {
            let hot = ["error", "Error", "panicked", "Exception", "Traceback", "warning"]
                .iter()
                .any(|k| l.contains(k));
            let style = if hot {
                Style::new().fg(red)
            } else {
                Style::new().fg(Color::Rgb(210, 210, 210))
            };
            Line::from(Span::styled(format!(" {l}"), style))
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_popup_terminal(f: &mut Frame, app: &App) {
    let accent = app.config.accent();
    let dim = app.config.dim();
    let ed: &EditorState = match app.editor.as_ref() {
        Some(e) => e,
        None => return,
    };
    let Some(popup) = ed.popup.as_ref() else { return };

    let rect = centered_rect(72, 55, f.area());
    f.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_style(Style::new().fg(accent))
        .title(Span::styled(" ⌁ silver terminal ", Style::new().fg(accent).add_modifier(Modifier::BOLD)))
        .title_bottom(Span::styled(" `help` for commands · esc to close ", Style::new().fg(dim)));
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    if inner.height < 2 {
        return;
    }

    let [out_a, in_a] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(inner);
    let mut h = out_a.height as usize;
    let total = popup.lines.len();
    let scroll = popup.scroll.min(total.saturating_sub(h));
    if scroll > 0 {
        h = h.saturating_sub(1); // reserve a row for the indicator
    }
    let end = total - scroll;
    let start = end.saturating_sub(h);
    let mut lines: Vec<Line> = popup.lines[start..end]
        .iter()
        .map(|l| Line::from(Span::styled(l.clone(), Style::new().fg(Color::Rgb(200, 200, 200)))))
        .collect();
    if scroll > 0 {
        lines.push(Line::from(Span::styled(
            format!("· · · {scroll} lines below (pgdn / wheel) · · ·"),
            Style::new().fg(dim),
        )));
    }
    f.render_widget(Paragraph::new(lines), out_a);

    let prompt = " » ";
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(prompt, Style::new().fg(accent).add_modifier(Modifier::BOLD)),
            Span::styled(popup.input.clone(), Style::new().fg(Color::White)),
        ])),
        in_a,
    );
    let x = in_a.x + prompt.len() as u16 + popup.input.chars().count() as u16;
    f.set_cursor_position((x.min(in_a.right().saturating_sub(1)), in_a.y));
}
