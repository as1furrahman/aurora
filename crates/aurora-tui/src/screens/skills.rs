//! Skills page: which skills each AI tool gets for this kind of task. One row per skill, one
//! column per AI tool; each cell says what will happen there. The details pane explains the
//! selected skill in plain words, with the technical facts under "More details".

use aurora_core::catalog::{Entry, Kind, Tier};
use aurora_skills::plan::{Action as PlanAction, ActionKind, skill_dir};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Row as TableRow, Table, TableState, Wrap};

use crate::app::{App, Tab};
use crate::ui::Theme;
use crate::words;

/// What the plan does for one skill on one AI tool.
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
    /// Not meant for this tool, but it can see it through a shared folder.
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

    /// The full sentence used in the details pane.
    pub fn meaning(self) -> &'static str {
        match self {
            Cell::Installed => "added and up to date",
            Cell::Create => "will be added",
            Cell::CreateAfterReview => "will be added once you've checked it",
            Cell::Update => "will be updated to a newer version",
            Cell::Remove => "will be removed (no longer needed)",
            Cell::Modified => "you changed it, so Aurora leaves it alone",
            Cell::Foreign => "already here and not added by Aurora, so it's left alone",
            Cell::Blocked => "blocked by Aurora's safety check",
            Cell::Leak => "not meant for this tool, but it can see it (shared folder)",
            Cell::NotWanted => "not used",
        }
    }

    /// The short word used in the legend.
    pub fn short(self) -> &'static str {
        match self {
            Cell::Installed => "added",
            Cell::Create => "will add",
            Cell::CreateAfterReview => "check first",
            Cell::Update => "will update",
            Cell::Remove => "will remove",
            Cell::Modified => "you changed it",
            Cell::Foreign => "yours, left alone",
            Cell::Blocked => "blocked",
            Cell::Leak => "shared",
            Cell::NotWanted => "not used",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub kind: Kind,
    pub tier: Tier,
    /// One per AI tool in use, in order.
    pub cells: Vec<Cell>,
}

fn skill_name(e: &Entry) -> &str {
    e.path.as_deref().and_then(|p| p.rsplit('/').next()).unwrap_or(&e.id)
}

fn target_of(app: &App, e: &Entry, cli: &str) -> Option<String> {
    let Ok(cat) = &app.catalog else { return None };
    match e.kind {
        Kind::Rules => cat.target(cli).map(|t| t.rules_file.clone()),
        _ => skill_dir(cli).map(|d| format!("{}/{}", d.rel, skill_name(e))),
    }
}

fn cell_for(app: &App, e: &Entry, cli: &str) -> Cell {
    let Some(p) = &app.planned else { return Cell::NotWanted };
    let Some(target) = target_of(app, e, cli) else { return Cell::NotWanted };
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

/// Rows for the current plan and search.
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
    let foot_lines = p.plan.visibility.len() as u16 + 4;
    let [intro_area, table_area, foot_area] =
        Layout::vertical([Constraint::Length(2), Constraint::Min(3), Constraint::Length(foot_lines)]).areas(area);
    f.render_widget(
        Paragraph::new(vec![
            Line::raw(format!(
                "Skills teach your AI tools how to do one kind of work well. These are the ones for “{}”.",
                words::task_type(&app.opts.preset)
            )),
            Line::from(Span::styled("Pick a row to see what it does. Add them on page 3.", theme.dim())),
        ])
        .wrap(Wrap { trim: true }),
        intro_area,
    );

    let mut header = vec![Span::raw("Skill")];
    header.extend(clis.iter().map(|c| Span::raw(words::tool(c).to_string())));
    let body: Vec<TableRow> = rows
        .iter()
        .map(|r| {
            let tag = match (r.kind, r.tier) {
                (Kind::Rules, _) => " (always on)",
                (_, Tier::OptIn) => " (optional)",
                _ => "",
            };
            let mut cells = vec![format!("{}{tag}", r.id)];
            cells.extend(r.cells.iter().map(|c| format!("  {}", c.glyph())));
            TableRow::new(cells)
        })
        .collect();
    let mut widths = vec![Constraint::Min(20)];
    widths.extend(clis.iter().map(|_| Constraint::Length(13)));
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
    let legend: Vec<String> = shown.iter().map(|c| format!("{} {}", c.glyph(), c.short())).collect();
    let mut lines = vec![
        Line::from(Span::styled(legend[..3].join("   "), theme.dim())),
        Line::from(Span::styled(legend[3..6].join("   "), theme.dim())),
        Line::from(Span::styled(legend[6..].join("   "), theme.dim())),
        Line::from(Span::styled("Room left in each tool's memory for skill descriptions:", theme.dim())),
    ];
    for v in &p.plan.visibility {
        let width = 12usize;
        let filled =
            if v.budget_chars == 0 { width } else { (v.listing_chars * width).div_ceil(v.budget_chars).min(width) };
        lines.push(Line::raw(format!(
            "  {:<12} ▕{}{}▏ {} skills · {}",
            words::tool(&v.provider),
            "█".repeat(filled),
            "░".repeat(width - filled),
            v.skills.len(),
            words::room(v.listing_chars, v.budget_chars)
        )));
    }
    f.render_widget(Paragraph::new(lines), foot_area);
}

fn used_by(app: &App, e: &Entry) -> String {
    let Some(p) = &app.planned else { return String::new() };
    let jobs: Vec<String> = p
        .recommendation
        .stages
        .iter()
        .filter(|s| s.skills.contains(&e.id))
        .map(|s| format!("{} ({})", words::tool(&s.provider), words::job(&s.role)))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if jobs.is_empty() { "every AI tool, all the time".into() } else { jobs.join(", ") }
}

pub fn render_detail(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let rows = rows(app);
    let (Some(row), Some(p), Ok(cat)) = (rows.get(app.cursor[Tab::Skills.index()]), &app.planned, &app.catalog) else {
        let lines: Vec<Line> = legend().lines().map(|l| Line::from(Span::styled(l.to_string(), theme.dim()))).collect();
        f.render_widget(Paragraph::new(crate::ui::hang(lines, area.width)), area);
        return;
    };
    let Some(e) = cat.get(&row.id) else { return };
    let src = cat.source(&e.source);
    let label = |s: &str| Span::styled(format!("{s:<14}"), theme.accent());
    let mut lines = vec![Line::from(Span::styled(e.id.clone(), theme.accent())), Line::raw("")];
    lines.push(Line::from(vec![label("What it does"), Span::raw(e.why.clone())]));
    lines.push(Line::from(vec![label("Used by"), Span::raw(used_by(app, e))]));
    let kind = match (e.kind, e.tier) {
        (Kind::Rules, _) => "Always-on rules, written into each tool's instructions file.",
        (_, Tier::OptIn) => "Optional: press i to add or remove it.",
        (_, Tier::Baseline) => "Recommended for every task.",
        _ => "Recommended for this kind of task.",
    };
    lines.push(Line::from(vec![label("Type"), Span::raw(kind)]));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("Each AI tool", theme.accent())));
    for (cli, cell) in p.recommendation.clis.iter().zip(&row.cells) {
        let place = match target_of(app, e, cli) {
            Some(t) if *cell != Cell::NotWanted => format!(" → {t}"),
            _ => String::new(),
        };
        lines.push(Line::raw(format!("  {:<12} {} {}{place}", words::tool(cli), cell.glyph(), cell.meaning())));
    }
    lines.push(Line::raw(""));
    let safety = match (p.review_files.get(&e.id), &e.reviewed, src.map(|s| s.trust.to_string())) {
        (Some(files), _, Some(t)) if t == "community" => {
            format!("From the community. Read it before adding: {} (page 3, Enter).", files.join(", "))
        }
        (Some(files), _, _) => format!("Has parts worth a look before adding: {} (page 3, Enter).", files.join(", ")),
        (None, Some(_), _) => "Checked by Aurora's maintainers; its helper files were read.".into(),
        (None, None, _) => "Nothing that needs checking.".into(),
    };
    lines.push(Line::from(vec![label("Safety"), Span::raw(safety)]));
    let switched_off =
        p.plan.lock.skills.values().any(|l| l.entry == e.id && l.overlays.iter().any(|o| o == "allowed-tools"));
    if switched_off {
        lines.push(Line::from(vec![
            label(""),
            Span::raw("Aurora switches off its permission to run commands without asking you."),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("More details", theme.dim())));
    let dim = |s: String| Line::from(Span::styled(s, theme.dim()));
    lines.push(dim(format!(
        "  kind {} · tier {} · trust {}",
        e.kind,
        e.tier,
        src.map(|s| s.trust.to_string()).unwrap_or_default()
    )));
    if let Some(s) = src {
        let link = match &e.path {
            Some(path) => format!("{}/tree/{}/{path}", s.repo, s.rev),
            None => s.repo.clone(),
        };
        lines.push(dim(format!("  source {link}")));
    }
    lines.push(dim(format!("  licence {}", cat.license_of(e))));
    if !e.flags.is_empty() {
        let flags: Vec<String> = e.flags.iter().map(ToString::to_string).collect();
        lines.push(dim(format!("  flags {}", flags.join(", "))));
    }
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
        lines.push(dim(format!("  overlays {}", overlays.join(", "))));
    }
    for cli in &p.recommendation.clis {
        if let Some(t) = cat.target(cli)
            && e.kind == Kind::Skill
        {
            lines.push(dim(format!("  ~{} listing chars on {}", e.listing_cost(t), words::tool(cli))));
        }
    }
    lines.push(dim(format!("  aurora catalog show {}", e.id)));
    if e.kind == Kind::Skill {
        lines.push(dim(format!("  aurora skills fetch {}", e.id)));
    }
    lines.push(dim(format!("  {}", plan_command(app))));
    f.render_widget(Paragraph::new(crate::ui::hang(lines, area.width)).wrap(Wrap { trim: false }), area);
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

fn legend() -> String {
    let mut s = String::from("What the symbols mean\n");
    for c in Cell::ALL {
        s.push_str(&format!("  {}  {}\n", c.glyph(), c.meaning()));
    }
    s
}
