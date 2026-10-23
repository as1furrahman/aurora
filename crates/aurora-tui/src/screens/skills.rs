//! Skills tab: one row per catalog skill relevant to the workflow, one column per CLI in the
//! profile, each cell what the install plan will do there. The detail pane explains the
//! selected skill and shows the equivalent CLI commands.

use aurora_core::catalog::{Entry, Kind, Tier};
use aurora_skills::plan::{Action as PlanAction, ActionKind, skill_dir};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Row as TableRow, Table, TableState, Wrap};

use crate::app::{App, Tab};
use crate::ui::Theme;

/// What the plan does for one skill on one CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    Installed,
    Create,
    CreateAfterReview,
    Update,
    Remove,
    Modified,
    Foreign,
    Blocked,
    /// Not wanted for this CLI, but visible to it through a shared directory.
    Leak,
    NotWanted,
}

impl Cell {
    pub const ALL: [Cell; 10] = [
        Cell::Installed,
        Cell::Create,
        Cell::CreateAfterReview,
        Cell::Update,
        Cell::Remove,
        Cell::Modified,
        Cell::Foreign,
        Cell::Blocked,
        Cell::Leak,
        Cell::NotWanted,
    ];

    pub fn glyph(self) -> &'static str {
        match self {
            Cell::Installed => "✓",
            Cell::Create => "+",
            Cell::CreateAfterReview => "?",
            Cell::Update => "↑",
            Cell::Remove => "−",
            Cell::Modified => "≠",
            Cell::Foreign => "!",
            Cell::Blocked => "✗",
            Cell::Leak => "◐",
            Cell::NotWanted => "·",
        }
    }

    pub fn meaning(self) -> &'static str {
        match self {
            Cell::Installed => "installed, unchanged",
            Cell::Create => "will be created",
            Cell::CreateAfterReview => "will be created after your review",
            Cell::Update => "will be updated",
            Cell::Remove => "will be removed (prune)",
            Cell::Modified => "you modified it; Aurora will not touch it",
            Cell::Foreign => "exists but was not installed by Aurora",
            Cell::Blocked => "blocked by the scanner",
            Cell::Leak => "visible through a shared directory",
            Cell::NotWanted => "not wanted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub kind: Kind,
    pub tier: Tier,
    /// One per CLI of the profile, in profile order.
    pub cells: Vec<Cell>,
}

fn skill_name(e: &Entry) -> &str {
    e.path.as_deref().and_then(|p| p.rsplit('/').next()).unwrap_or(&e.id)
}

fn cell_for(app: &App, e: &Entry, cli: &str) -> Cell {
    let (Some(p), Ok(cat)) = (&app.planned, &app.catalog) else { return Cell::NotWanted };
    let target = match e.kind {
        Kind::Rules => cat.target(cli).map(|t| t.rules_file.clone()),
        _ => skill_dir(cli).map(|d| format!("{}/{}", d.rel, skill_name(e))),
    };
    let Some(target) = target else { return Cell::NotWanted };
    let action: Option<&PlanAction> = p.plan.actions.iter().find(|a| a.target == target && a.entry == e.id);
    let mine = action.filter(|a| a.providers.is_empty() || a.providers.iter().any(|x| x == cli));
    match mine {
        Some(a) => match a.kind {
            ActionKind::Create | ActionKind::Update if a.needs_review => Cell::CreateAfterReview,
            ActionKind::Create => Cell::Create,
            ActionKind::Update => Cell::Update,
            ActionKind::Unchanged | ActionKind::Keep => Cell::Installed,
            ActionKind::Remove => Cell::Remove,
            ActionKind::Refuse if a.detail.contains("blocked") => Cell::Blocked,
            ActionKind::Refuse if a.detail.contains("modified") || a.detail.contains("edited") => Cell::Modified,
            ActionKind::Refuse => Cell::Foreign,
        },
        None => {
            let sees = p.plan.visibility.iter().find(|v| v.provider == cli);
            if action.is_some() && sees.is_some_and(|v| v.skills.iter().any(|s| s == skill_name(e))) {
                Cell::Leak
            } else {
                Cell::NotWanted
            }
        }
    }
}

/// Rows for the current plan and filter.
pub fn rows(app: &App) -> Vec<Row> {
    let (Some(p), Ok(cat)) = (&app.planned, &app.catalog) else { return Vec::new() };
    let rec = &p.recommendation;
    let filter = app.filter().to_lowercase();
    cat.entries()
        .iter()
        .filter(|e| matches!(e.kind, Kind::Skill | Kind::Rules))
        .filter(|e| {
            rec.stages.iter().any(|s| s.skills.contains(&e.id))
                || rec.rules.iter().any(|r| r.entry == e.id)
                || rec.stages.iter().any(|s| e.serves(&s.role, &s.stage))
        })
        .filter(|e| filter.is_empty() || e.id.to_lowercase().contains(&filter))
        .map(|e| Row {
            id: e.id.clone(),
            kind: e.kind,
            tier: e.tier,
            cells: rec.clis.iter().map(|cli| cell_for(app, e, cli)).collect(),
        })
        .collect()
}

pub fn render_list(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let Some(p) = &app.planned else { return };
    let rows = rows(app);
    let clis = &p.recommendation.clis;
    let budget_lines = p.plan.visibility.len() as u16 + 3;
    let [table_area, budget_area] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(budget_lines)]).areas(area);

    let mut header = vec![Span::raw("skill")];
    header.extend(clis.iter().map(|c| Span::raw(c.clone())));
    let body: Vec<TableRow> = rows
        .iter()
        .map(|r| {
            let tag = match (r.kind, r.tier) {
                (Kind::Rules, _) => " (rules)",
                (_, Tier::OptIn) => " (opt-in)",
                _ => "",
            };
            let mut cells = vec![format!("{}{tag}", r.id)];
            cells.extend(r.cells.iter().map(|c| format!("  {}", c.glyph())));
            TableRow::new(cells)
        })
        .collect();
    let mut widths = vec![Constraint::Min(20)];
    widths.extend(clis.iter().map(|_| Constraint::Length(12)));
    let table = Table::new(body, widths)
        .header(TableRow::new(header).style(theme.accent()))
        .row_highlight_style(theme.focused().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");
    let mut state = TableState::default().with_selected(Some(app.cursor[Tab::Skills.index()]));
    f.render_stateful_widget(table, table_area, &mut state);

    let shown = [
        Cell::Installed,
        Cell::Create,
        Cell::CreateAfterReview,
        Cell::Update,
        Cell::Modified,
        Cell::Foreign,
        Cell::Leak,
        Cell::NotWanted,
    ];
    let legend: Vec<String> = shown.iter().map(|c| format!("{} {}", c.glyph(), short_meaning(*c))).collect();
    let mut lines = vec![
        Line::from(Span::styled(legend[..4].join("  "), theme.dim())),
        Line::from(Span::styled(legend[4..].join("  "), theme.dim())),
        Line::from(Span::styled("listing budget (what each CLI will load):", theme.dim())),
    ];
    for v in &p.plan.visibility {
        let width = 12usize;
        let filled =
            if v.budget_chars == 0 { width } else { (v.listing_chars * width).div_ceil(v.budget_chars).min(width) };
        lines.push(Line::raw(format!(
            "  {:<11} ▕{}{}▏ {}/{} · {} skills",
            v.provider,
            "█".repeat(filled),
            "░".repeat(width - filled),
            v.listing_chars,
            v.budget_chars,
            v.skills.len()
        )));
    }
    f.render_widget(Paragraph::new(lines), budget_area);
}

pub fn render_detail(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let rows = rows(app);
    let (Some(row), Some(p), Ok(cat)) = (rows.get(app.cursor[Tab::Skills.index()]), &app.planned, &app.catalog) else {
        f.render_widget(Paragraph::new(Span::styled(legend(), theme.dim())).wrap(Wrap { trim: false }), area);
        return;
    };
    let Some(e) = cat.get(&row.id) else { return };
    let src = cat.source(&e.source);
    let mut lines = vec![
        Line::from(Span::styled(e.id.clone(), theme.accent())),
        Line::raw(format!("{} · {} · {}", e.kind, e.tier, src.map(|s| s.trust.to_string()).unwrap_or_default())),
        Line::raw(e.why.clone()),
        Line::raw(""),
    ];
    if let Some(s) = src {
        let link = match &e.path {
            Some(path) => format!("{}/tree/{}/{path}", s.repo, s.rev),
            None => s.repo.clone(),
        };
        lines.push(Line::raw(format!("source   {link}")));
    }
    lines.push(Line::raw(format!("licence  {}", cat.license_of(e))));
    let all = |v: &[String]| if v.is_empty() { "all".to_string() } else { v.join(", ") };
    lines.push(Line::raw(format!("roles    {}   stages: {}", all(&e.roles), all(&e.stages))));
    if !e.flags.is_empty() {
        let flags: Vec<String> = e.flags.iter().map(ToString::to_string).collect();
        lines.push(Line::raw(format!("flags    {}", flags.join(", "))));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("Per CLI", theme.accent())));
    for (cli, cell) in p.recommendation.clis.iter().zip(&row.cells) {
        let target = match e.kind {
            Kind::Rules => cat.target(cli).map(|t| t.rules_file.clone()).unwrap_or_default(),
            _ => skill_dir(cli).map(|d| format!("{}/{}", d.rel, skill_name(e))).unwrap_or_default(),
        };
        let cost = cat.target(cli).map(|t| e.listing_cost(t)).unwrap_or(0);
        let place = if *cell == Cell::NotWanted { String::new() } else { format!(" · {target}") };
        let listing = if e.kind == Kind::Skill && *cell != Cell::NotWanted {
            format!(" · ~{cost} chars")
        } else {
            String::new()
        };
        lines.push(Line::raw(format!("  {:<11} {} {}{place}{listing}", cli, cell.glyph(), short_meaning(*cell))));
    }
    lines.push(Line::raw(""));
    let review = match (p.review_files.get(&e.id), &e.reviewed) {
        (Some(files), _) => format!("needs your review: read {}", files.join(", ")),
        (None, Some(note)) => format!("reviewed for the catalog: {note}"),
        (None, None) => "no review needed".to_string(),
    };
    lines.push(Line::raw(format!("review   {review}")));
    let overlays: Vec<String> = p
        .plan
        .lock
        .skills
        .values()
        .filter(|l| l.entry == e.id)
        .flat_map(|l| l.overlays.iter().map(|k| format!("{k} removed")))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if !overlays.is_empty() {
        lines.push(Line::raw(format!("overlays {}", overlays.join(", "))));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("Equivalent commands", theme.dim())));
    lines.push(Line::raw(format!("  aurora catalog show {}", e.id)));
    if e.kind == Kind::Skill {
        lines.push(Line::raw(format!("  aurora skills fetch {}", e.id)));
    }
    lines.push(Line::raw(format!("  {}", plan_command(app))));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

/// The `aurora skills plan` command for the current settings.
pub fn plan_command(app: &App) -> String {
    let mut cmd = format!("aurora skills plan --preset {} --project {}", app.opts.preset, app.opts.project.display());
    if !app.opts.clis.is_empty() {
        cmd.push_str(&format!(" --clis {}", app.opts.clis.join(",")));
    }
    for w in &app.with {
        cmd.push_str(&format!(" --with {w}"));
    }
    cmd
}

fn short_meaning(c: Cell) -> &'static str {
    match c {
        Cell::Installed => "installed",
        Cell::Create => "create",
        Cell::CreateAfterReview => "after review",
        Cell::Update => "update",
        Cell::Remove => "remove",
        Cell::Modified => "modified",
        Cell::Foreign => "foreign",
        Cell::Blocked => "blocked",
        Cell::Leak => "shared dir",
        Cell::NotWanted => "not wanted",
    }
}

fn legend() -> String {
    let mut s = String::from("Legend\n");
    for c in Cell::ALL {
        s.push_str(&format!("  {}  {}\n", c.glyph(), c.meaning()));
    }
    s
}
