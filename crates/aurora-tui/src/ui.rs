//! Rendering: a pure function of `&App`. Glyphs and words carry every meaning; colour only adds
//! emphasis (and is dropped entirely under `NO_COLOR`).

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap};

use crate::app::{App, Focus, Popup, Tab};
use crate::keymap::{Binding, Ctx, bindings, footer_for};

pub const MIN_WIDTH: u16 = 60;
pub const MIN_HEIGHT: u16 = 12;
/// Below this width the list and detail panes stack into one.
pub const SPLIT_WIDTH: u16 = 100;

/// Styles, or plain text when colour is off.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub no_color: bool,
    pub high_contrast: bool,
}

impl Theme {
    fn pick(self, colored: Style) -> Style {
        if self.no_color { Style::default() } else { colored }
    }
    fn hue(self) -> Color {
        if self.high_contrast { Color::Yellow } else { Color::Cyan }
    }
    pub fn accent(self) -> Style {
        self.pick(Style::default().fg(self.hue())).add_modifier(Modifier::BOLD)
    }
    pub fn dim(self) -> Style {
        // Dimmed text is hard to read in high contrast; keep it plain there.
        if self.high_contrast { Style::default() } else { self.pick(Style::default().add_modifier(Modifier::DIM)) }
    }
    pub fn focused(self) -> Style {
        let s = self.pick(Style::default().fg(self.hue()));
        if self.high_contrast { s.add_modifier(Modifier::BOLD) } else { s }
    }
}

pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();
    let theme = Theme { no_color: app.opts.no_color, high_contrast: app.opts.high_contrast };
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let msg = format!(
            "Terminal too small: {}×{} (Aurora needs at least {MIN_WIDTH}×{MIN_HEIGHT}). Resize, or press q to quit.",
            area.width, area.height
        );
        f.render_widget(Paragraph::new(msg).wrap(Wrap { trim: true }), area);
        return;
    }
    let [header, tabs, body, foot] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)])
            .areas(area);

    let title = format!(
        "Aurora · {} · {} · {}",
        app.opts.project.display(),
        app.opts.preset,
        crate::screens::roles::profile_summary(app).unwrap_or_else(|| app
            .opts
            .profile
            .clone()
            .unwrap_or_else(|| "pair".into()))
    );
    let status = if app.loading {
        const SPIN: [&str; 4] = ["⠋", "⠙", "⠸", "⠴"];
        format!("   {} planning…", SPIN[(app.ticks % 4) as usize])
    } else if app.plan_error.is_some() {
        "   ✗ plan failed (r retry)".to_string()
    } else {
        String::new()
    };
    f.render_widget(Paragraph::new(Line::from(vec![Span::styled(title, theme.accent()), Span::raw(status)])), header);

    let titles: Vec<String> = Tab::ALL.iter().enumerate().map(|(i, t)| format!("{} {}", i + 1, t.title())).collect();
    f.render_widget(
        Tabs::new(titles)
            .select(app.tab.index())
            .highlight_style(theme.accent().add_modifier(Modifier::REVERSED))
            .divider("│"),
        tabs,
    );

    render_body(f, app, body, theme);
    render_footer(f, app, foot, theme);
    match app.popup() {
        Some(Popup::Help { .. }) => render_help(f, app, area, theme),
        Some(Popup::Confirm { title, lines, .. }) => render_confirm(f, area, title, lines, theme),
        Some(Popup::Viewer { title, text, scroll }) => render_viewer(f, area, title, text, *scroll, theme),
        Some(Popup::Submit { stage, items, cursor, .. }) => render_submit(f, area, stage, items, *cursor, theme),
        Some(Popup::Input { prompt, text, .. }) => render_input(f, area, prompt, text, theme),
        None => {}
    }
}

fn render_submit(
    f: &mut Frame,
    area: Rect,
    stage: &str,
    items: &[(String, String, bool)],
    cursor: usize,
    theme: Theme,
) {
    let mut lines = vec![
        Line::raw("Check each criterion whose evidence you saved. The gate decides: outputs must exist"),
        Line::raw("and be non-empty, and only checked criteria count."),
        Line::raw(""),
    ];
    for (i, (id, desc, checked)) in items.iter().enumerate() {
        let mark = if *checked { "[x]" } else { "[ ]" };
        let pointer = if i == cursor { "▶ " } else { "  " };
        let style = if i == cursor { theme.focused().add_modifier(Modifier::REVERSED) } else { Style::default() };
        lines.push(Line::from(Span::styled(format!("{pointer}{mark} {id}"), style)));
        lines.push(Line::raw(format!("        {desc}")));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("Space check · j/k move · Enter submit · Esc cancel", theme.dim())));
    let h = (lines.len() as u16 + 2).min(area.height.saturating_sub(2));
    let rect = centered(area, area.width.saturating_sub(8).min(100), h);
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default().borders(Borders::ALL).title(format!(" Submit {stage} ")).border_style(theme.focused()),
        ),
        rect,
    );
}

fn render_input(f: &mut Frame, area: Rect, prompt: &str, text: &str, theme: Theme) {
    let lines = vec![
        Line::raw(prompt.to_string()),
        Line::from(Span::styled(format!("> {text}▏"), theme.accent())),
        Line::from(Span::styled("Enter ok · Esc cancel", theme.dim())),
    ];
    let rect = centered(area, area.width.saturating_sub(8).min(80), 5);
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).border_style(theme.focused())),
        rect,
    );
}

fn render_confirm(f: &mut Frame, area: Rect, title: &str, lines: &[String], theme: Theme) {
    let mut text: Vec<Line> = lines.iter().map(|l| Line::raw(l.clone())).collect();
    text.push(Line::raw(""));
    text.push(Line::from(vec![
        Span::styled("y", theme.accent()),
        Span::raw(" apply    "),
        Span::styled("n", theme.accent()),
        Span::raw(" / Esc / Enter cancel (the default is No)"),
    ]));
    let h = (text.len() as u16 + 2).min(area.height.saturating_sub(2));
    let rect = centered(area, area.width.saturating_sub(8).min(96), h);
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(title.to_string()).border_style(theme.focused())),
        rect,
    );
}

fn render_viewer(f: &mut Frame, area: Rect, title: &str, text: &str, scroll: u16, theme: Theme) {
    let rect = centered(area, area.width.saturating_sub(4), area.height.saturating_sub(2));
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(text.to_string())
            .scroll((scroll, 0))
            .block(Block::default().borders(Borders::ALL).title(title.to_string()).border_style(theme.focused())),
        rect,
    );
}

fn pane(title: &str, focused: bool, theme: Theme) -> Block<'static> {
    let marker = if focused { "▶ " } else { "  " };
    Block::default().borders(Borders::ALL).title(format!("{marker}{title}")).border_style(if focused {
        theme.focused()
    } else {
        theme.dim()
    })
}

fn render_body(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let (list_title, detail_title) = match app.tab {
        Tab::Skills => ("Skills × CLIs", "Skill"),
        Tab::Roles => ("Stages and roles", "Stage"),
        Tab::Install => ("Install plan", "Action"),
        Tab::Runs => ("Runs", "Run"),
    };
    let (left, right) = if area.width < SPLIT_WIDTH {
        match app.focus {
            Focus::List => (Some(area), None),
            Focus::Detail => (None, Some(area)),
        }
    } else {
        let [l, r] = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);
        (Some(l), Some(r))
    };
    if let Some(l) = left {
        let block = pane(list_title, app.focus == Focus::List, theme);
        let inner = block.inner(l);
        f.render_widget(block, l);
        render_list(f, app, inner, theme);
    }
    if let Some(r) = right {
        let block = pane(detail_title, app.focus == Focus::Detail, theme);
        let inner = block.inner(r);
        f.render_widget(block, r);
        render_detail(f, app, inner, theme);
    }
}

fn render_list(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    if let Some(e) = app.plan_error.as_ref().filter(|_| app.tab != Tab::Runs) {
        let text = format!("The install plan failed:\n\n{e}\n\nPress r to retry.");
        f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), area);
        return;
    }
    match app.tab {
        Tab::Skills if app.planned.is_some() => crate::screens::skills::render_list(f, app, area, theme),
        Tab::Roles if app.planned.is_some() => crate::screens::roles::render_list(f, app, area, theme),
        Tab::Install if app.planned.is_some() => crate::screens::install::render_list(f, app, area, theme),
        Tab::Runs => crate::screens::runs::render_list(f, app, area, theme),
        Tab::Skills | Tab::Roles | Tab::Install => {
            f.render_widget(Paragraph::new(Span::styled("Planning the install…", theme.dim())), area)
        }
    }
}

fn render_detail(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    match app.tab {
        Tab::Skills => crate::screens::skills::render_detail(f, app, area, theme),
        Tab::Roles => crate::screens::roles::render_detail(f, app, area, theme),
        Tab::Install => crate::screens::install::render_detail(f, app, area, theme),
        Tab::Runs => crate::screens::runs::render_detail(f, app, area, theme),
    }
}

fn key_spans(binds: &[&Binding], theme: Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (i, b) in binds.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        let (key, word) = b.label.split_once(' ').unwrap_or((b.label, ""));
        spans.push(Span::styled(key.to_string(), theme.accent()));
        spans.push(Span::raw(format!(" {word}")));
    }
    spans
}

fn render_footer(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let ctx = app.ctx();
    let mut spans = Vec::new();
    if let Some(msg) = app.toast() {
        spans.push(Span::styled(format!("▌ {msg}"), theme.accent()));
        spans.push(Span::raw("   "));
    } else if let Some(b) = app.busy {
        spans.push(Span::styled(format!("▌ {b}…"), theme.accent()));
        spans.push(Span::raw("   "));
    }
    if ctx == Ctx::Filter {
        spans.push(Span::styled(format!("/{}▏", app.filter()), theme.accent()));
        spans.push(Span::raw("   "));
    }
    let ctxs: Vec<Ctx> = if ctx == Ctx::Normal { vec![app.tab.ctx(), Ctx::Normal] } else { vec![ctx] };
    spans.extend(key_spans(&footer_for(&ctxs), theme));
    if ctx == Ctx::Normal && !app.filter().is_empty() {
        spans.push(Span::raw("   "));
        spans.push(Span::styled(format!("/{}", app.filter()), theme.accent()));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// A centred rectangle of at most `w`×`h` inside `area`.
pub fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

/// The help overlay's lines: every binding, the current tab's first, grouped by context.
pub fn help_lines(app: &App) -> Vec<(Option<&'static str>, String)> {
    let mut order = vec![
        app.tab.ctx(),
        Ctx::Normal,
        Ctx::Filter,
        Ctx::Help,
        Ctx::Confirm,
        Ctx::Viewer,
        Ctx::Form,
        Ctx::Input,
        Ctx::Global,
    ];
    for tab in Tab::ALL {
        if !order.contains(&tab.ctx()) {
            order.push(tab.ctx());
        }
    }
    let mut out = Vec::new();
    for ctx in order {
        out.push((Some(ctx.title()), String::new()));
        let mut seen: Vec<&str> = Vec::new();
        for b in bindings().iter().filter(|b| b.ctx == ctx) {
            if seen.contains(&b.help) {
                continue;
            }
            seen.push(b.help);
            let keys: Vec<String> = bindings()
                .iter()
                .filter(|o| o.ctx == ctx && o.help == b.help)
                .flat_map(|o| o.keys.iter().map(|(code, mods)| key_name(*code, *mods)))
                .collect();
            out.push((None, format!("  {:<18} {}", keys.join(" "), b.help)));
        }
        out.push((None, String::new()));
    }
    out
}

fn render_help(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let lines: Vec<Line> = help_lines(app)
        .into_iter()
        .map(|(title, text)| match title {
            Some(t) => Line::from(Span::styled(t, theme.accent())),
            None => Line::raw(text),
        })
        .collect();
    let rect = centered(area, area.width.saturating_sub(4).min(100), area.height.saturating_sub(2));
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((app.help_scroll(), 0))
            .block(Block::default().borders(Borders::ALL).title(" Keys (Esc closes) ").border_style(theme.focused())),
        rect,
    );
}

fn key_name(code: ratatui::crossterm::event::KeyCode, mods: ratatui::crossterm::event::KeyModifiers) -> String {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    let base = match code {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::BackTab => "S-Tab".into(),
        KeyCode::Down => "↓".into(),
        KeyCode::Up => "↑".into(),
        KeyCode::Home => "Home".into(),
        KeyCode::End => "End".into(),
        KeyCode::PageDown => "PgDn".into(),
        KeyCode::PageUp => "PgUp".into(),
        KeyCode::Esc => "Esc".into(),
        KeyCode::Enter => "Enter".into(),
        other => format!("{other:?}"),
    };
    if mods.contains(KeyModifiers::CONTROL) { format!("^{}", base.to_uppercase()) } else { base }
}
