//! Roles tab: stage -> role -> CLI -> skills for the current workflow and CLI profile. Any CLI
//! may take any role: `p` hands the selected role to the next free CLI, `P` switches the
//! profile, `w` the workflow. The detail pane shows how each skill is invoked on that CLI and
//! how strongly the CLI enforces what the role requires.

use aurora_core::providers::get_provider;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Row as TableRow, Table, TableState, Wrap};

use crate::app::{App, Tab};
use crate::ui::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageRow {
    pub stage: String,
    pub role: String,
    pub cli: String,
    pub skills: Vec<String>,
}

/// Stage rows for the current plan and filter.
pub fn rows(app: &App) -> Vec<StageRow> {
    let Some(p) = &app.planned else { return Vec::new() };
    let filter = app.filter().to_lowercase();
    p.recommendation
        .stages
        .iter()
        .filter(|s| {
            filter.is_empty() || [&s.stage, &s.role, &s.provider].iter().any(|v| v.to_lowercase().contains(&filter))
        })
        .map(|s| StageRow {
            stage: s.stage.clone(),
            role: s.role.clone(),
            cli: s.provider.clone(),
            skills: s.skills.clone(),
        })
        .collect()
}

/// The profile in effect and the CLIs filling its slots.
pub fn profile_summary(app: &App) -> Option<String> {
    app.planned.as_ref().map(|p| format!("{} ({})", p.recommendation.profile, p.recommendation.clis.join(" + ")))
}

pub fn render_list(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let Some(p) = &app.planned else { return };
    let rows = rows(app);
    let warnings = &p.recommendation.warnings;
    let warn_lines = (warnings.len() as u16 * 2).min(area.height / 2);
    let [table_area, warn_area] = Layout::vertical([Constraint::Min(3), Constraint::Length(warn_lines)]).areas(area);
    let body: Vec<TableRow> = rows
        .iter()
        .map(|r| {
            TableRow::new(vec![
                r.stage.clone(),
                r.role.clone(),
                r.cli.clone(),
                format!("{} skill{}", r.skills.len(), if r.skills.len() == 1 { "" } else { "s" }),
            ])
        })
        .collect();
    let table =
        Table::new(body, [Constraint::Min(14), Constraint::Length(13), Constraint::Length(13), Constraint::Length(10)])
            .header(TableRow::new(vec!["stage", "role", "CLI", "skills"]).style(theme.accent()))
            .row_highlight_style(theme.focused().add_modifier(Modifier::REVERSED))
            .highlight_symbol("▶ ");
    let mut state = TableState::default().with_selected(Some(app.cursor[Tab::Roles.index()]));
    f.render_stateful_widget(table, table_area, &mut state);
    let lines: Vec<Line> = warnings.iter().map(|w| Line::raw(format!("! {w}"))).collect();
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), warn_area);
}

pub fn render_detail(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let rows = rows(app);
    let (Some(row), Some(p)) = (rows.get(app.cursor[Tab::Roles.index()]), &app.planned) else { return };
    let provider = get_provider(&row.cli).ok();
    let role = app.workflow.as_ref().ok().and_then(|wf| wf.role(&row.role));
    let stage = app.workflow.as_ref().ok().and_then(|wf| wf.stage(&row.stage));
    let mut lines = vec![
        Line::from(Span::styled(format!("{} · {}", row.stage, row.role), theme.accent())),
        Line::raw(role.map(|r| r.responsibility.clone()).unwrap_or_default()),
        Line::raw(""),
        Line::raw(format!(
            "CLI      {} ({})",
            provider.map(|p| p.display_name).unwrap_or(&row.cli),
            profile_summary(app).unwrap_or_default()
        )),
    ];
    // In a profile, roles share CLI slots: changing this CLI moves its slot-mates too.
    if let Ok(cat) = &app.catalog
        && let Some(pr) = cat.profile(&p.recommendation.profile)
        && let Some(slot) = pr.roles.get(&row.role)
    {
        let mut mates: Vec<&str> = p
            .recommendation
            .stages
            .iter()
            .filter(|s| s.role != row.role && pr.roles.get(&s.role) == Some(slot))
            .map(|s| s.role.as_str())
            .collect();
        mates.dedup();
        let shared =
            if mates.is_empty() { String::new() } else { format!("; this CLI also plays {}", mates.join(", ")) };
        lines.push(Line::raw(format!("slot     {slot} of {}{shared}", p.recommendation.profile)));
    }
    if let Some(s) = stage
        && !s.independent_of.is_empty()
    {
        lines.push(Line::raw(format!("must be independent of: {}", s.independent_of.join(", "))));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("Skills for this stage", theme.accent())));
    for s in &row.skills {
        let how = provider.map(|p| p.invoke_skill(s)).unwrap_or_else(|| s.clone());
        lines.push(Line::raw(format!("  {how}")));
    }
    if let Some(r) = role
        && !r.requires.is_empty()
    {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled("Safeguards this role requires", theme.accent())));
        for cap in &r.requires {
            let line = match provider.and_then(|p| p.enforcement(cap)) {
                Some(e) => format!("  {cap}: {} — {}", e.strength, e.how),
                None => format!("  {cap}: none — {} cannot enforce it; the handoff only asks", row.cli),
            };
            lines.push(Line::raw(line));
        }
    }
    let mine: Vec<&String> =
        p.recommendation.warnings.iter().filter(|w| w.contains(&format!("'{}'", row.stage))).collect();
    if !mine.is_empty() {
        lines.push(Line::raw(""));
        for w in mine {
            lines.push(Line::raw(format!("! {w}")));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("Keys: p another CLI for this role · P profile · w workflow", theme.dim())));
    lines.push(Line::from(Span::styled("Equivalent commands", theme.dim())));
    let clis = p.recommendation.clis.join(",");
    lines.push(Line::raw(format!("  aurora catalog recommend --preset {} --clis {clis}", app.opts.preset)));
    lines.push(Line::raw(format!(
        "  aurora workflow init {0} && aurora workflow plan {0}.toml --clis {clis}",
        app.opts.preset
    )));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}
