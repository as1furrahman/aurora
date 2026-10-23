//! Rendering: a pure function of `&App`. Glyphs and words carry every meaning; colour only adds
//! emphasis (and is dropped entirely under `NO_COLOR`).

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap};

use crate::app::{App, Focus, Tab};
use crate::keymap::{Binding, Ctx, bindings, footer};

pub const MIN_WIDTH: u16 = 60;
pub const MIN_HEIGHT: u16 = 12;
/// Below this width the list and detail panes stack into one.
pub const SPLIT_WIDTH: u16 = 100;

/// Styles, or plain text when colour is off.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub no_color: bool,
}

impl Theme {
    fn pick(self, colored: Style) -> Style {
        if self.no_color { Style::default() } else { colored }
    }
    pub fn accent(self) -> Style {
        self.pick(Style::default().fg(Color::Cyan)).add_modifier(Modifier::BOLD)
    }
    pub fn dim(self) -> Style {
        self.pick(Style::default().add_modifier(Modifier::DIM))
    }
    pub fn focused(self) -> Style {
        self.pick(Style::default().fg(Color::Cyan))
    }
}

pub fn render(f: &mut Frame, app: &App) {
    let area = f.area();
    let theme = Theme { no_color: app.opts.no_color };
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
        app.opts.profile.as_deref().unwrap_or("pair")
    );
    f.render_widget(Paragraph::new(Line::from(Span::styled(title, theme.accent()))), header);

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
    if app.help_open() {
        render_help(f, app, area, theme);
    }
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
    let empty = Paragraph::new(Span::styled("Nothing loaded yet.", theme.dim()));
    if area.width < SPLIT_WIDTH {
        let (title, focused) = match app.focus {
            Focus::List => (list_title, true),
            Focus::Detail => (detail_title, true),
        };
        f.render_widget(empty.block(pane(title, focused, theme)), area);
        return;
    }
    let [left, right] = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);
    f.render_widget(empty.clone().block(pane(list_title, app.focus == Focus::List, theme)), left);
    f.render_widget(empty.block(pane(detail_title, app.focus == Focus::Detail, theme)), right);
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
    if ctx == Ctx::Filter {
        spans.push(Span::styled(format!("/{}▏", app.filter()), theme.accent()));
        spans.push(Span::raw("   "));
    }
    spans.extend(key_spans(&footer(ctx), theme));
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

fn render_help(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let mut lines: Vec<Line> = Vec::new();
    for ctx in [Ctx::Normal, Ctx::Filter, Ctx::Help, Ctx::Global] {
        lines.push(Line::from(Span::styled(ctx.title(), theme.accent())));
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
            lines.push(Line::from(vec![Span::raw(format!("  {:<18} ", keys.join(" "))), Span::raw(b.help)]));
        }
        lines.push(Line::raw(""));
    }
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
