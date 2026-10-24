//! "Who does what" page: each step of the task, the job it needs, and which AI tool does it.
//! Any tool can do any job: `p` gives the selected job to another tool, `P` changes how many
//! tools you use, `w` the kind of task.

use aurora_core::providers::{Strength, get_provider};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Row as TableRow, Table, TableState, Wrap};

use crate::app::{App, Tab};
use crate::ui::Theme;
use crate::words;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageRow {
    pub stage: String,
    pub role: String,
    pub cli: String,
    pub skills: Vec<String>,
}

/// Step rows for the current plan and search.
pub fn rows(app: &App) -> Vec<StageRow> {
    let Some(p) = &app.planned else { return Vec::new() };
    let filter = app.filter().to_lowercase();
    p.recommendation
        .stages
        .iter()
        .filter(|s| {
            filter.is_empty()
                || [
                    s.stage.as_str(),
                    s.role.as_str(),
                    s.provider.as_str(),
                    words::job(&s.role),
                    words::tool(&s.provider),
                ]
                .iter()
                .any(|v| v.to_lowercase().contains(&filter))
        })
        .map(|s| StageRow {
            stage: s.stage.clone(),
            role: s.role.clone(),
            cli: s.provider.clone(),
            skills: s.skills.clone(),
        })
        .collect()
}

/// "Claude Code + Codex", the tools in use.
pub fn profile_summary(app: &App) -> Option<String> {
    app.planned.as_ref().map(|p| words::tools(&p.recommendation.clis))
}

pub fn render_list(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let Some(p) = &app.planned else { return };
    let rows = rows(app);
    let warnings: Vec<String> = p.recommendation.warnings.iter().map(|w| words::warning(w)).collect();
    let warn_lines = (warnings.len() as u16 * 2).min(area.height / 3);
    let [intro_area, table_area, warn_area] =
        Layout::vertical([Constraint::Length(2), Constraint::Min(3), Constraint::Length(warn_lines)]).areas(area);
    f.render_widget(
        Paragraph::new(vec![
            Line::raw("Each step of the task is one job, done by one AI tool."),
            Line::from(Span::styled("p gives the selected job to another tool · P uses 1, 2 or 3 tools", theme.dim())),
        ])
        .wrap(Wrap { trim: true }),
        intro_area,
    );
    let body: Vec<TableRow> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let n = r.skills.len();
            TableRow::new(vec![
                format!("{}. {}", i + 1, words::step(&r.stage)),
                words::job(&r.role).to_string(),
                words::tool(&r.cli).to_string(),
                format!("{n} skill{}", if n == 1 { "" } else { "s" }),
            ])
        })
        .collect();
    let table =
        Table::new(body, [Constraint::Min(16), Constraint::Length(17), Constraint::Length(13), Constraint::Length(9)])
            .header(TableRow::new(vec!["Step", "Job", "AI tool", "Skills"]).style(theme.accent()))
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
    let tool = words::tool(&row.cli);
    let label = |s: &str| Span::styled(format!("{s:<14}"), theme.accent());
    let mut lines = vec![
        Line::from(Span::styled(
            format!("{} · the {}", words::step(&row.stage), words::job(&row.role)),
            theme.accent(),
        )),
        Line::raw(""),
        Line::from(vec![
            label("The job"),
            Span::raw(format!("The {} {}.", words::job(&row.role), words::job_explained(&row.role))),
        ]),
        Line::from(vec![label("Done by"), Span::raw(tool.to_string())]),
    ];
    // In a profile, jobs share AI tools: changing this tool moves its partners too.
    if let Ok(cat) = &app.catalog
        && let Some(pr) = cat.profile(&p.recommendation.profile)
        && let Some(slot) = pr.roles.get(&row.role)
    {
        let mut mates: Vec<&str> = p
            .recommendation
            .stages
            .iter()
            .filter(|s| s.role != row.role && pr.roles.get(&s.role) == Some(slot))
            .map(|s| words::job(&s.role))
            .collect();
        mates.dedup();
        if !mates.is_empty() {
            lines.push(Line::from(vec![
                label("Also"),
                Span::raw(format!("{tool} is also the {}; pressing p moves those jobs too.", mates.join(" and "))),
            ]));
        }
    }
    if let Some(s) = stage
        && !s.independent_of.is_empty()
    {
        let others: Vec<String> = s.independent_of.iter().map(|o| words::step(o)).collect();
        lines.push(Line::from(vec![
            label("Second opinion"),
            Span::raw(format!("Best done by a different tool than {}, so mistakes get caught.", others.join(", "))),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(format!("Skills for this step (type these in {tool})"), theme.accent())));
    for s in &row.skills {
        let how = provider.map(|p| p.invoke_skill(s)).unwrap_or_else(|| s.clone());
        lines.push(Line::raw(format!("  {how}")));
    }
    if let Some(r) = role
        && !r.requires.is_empty()
    {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled("Keeping it safe", theme.accent())));
        for cap in &r.requires {
            let what = if cap == "read_only" { "not change any files" } else { "only use the tools it needs" };
            let line = match provider.and_then(|p| p.enforcement(cap)) {
                Some(e) if e.strength == Strength::Hard => {
                    format!("  This job should {what}. {tool} can be locked to that (see More details).")
                }
                Some(_) => {
                    format!("  This job should {what}. {tool} can only be asked to, not forced, so check its work.")
                }
                None => format!("  This job should {what}. {tool} can't enforce it, so the instructions ask it to."),
            };
            lines.push(Line::raw(line));
        }
    }
    let mine: Vec<String> = p
        .recommendation
        .warnings
        .iter()
        .filter(|w| w.contains(&format!("'{}'", row.stage)))
        .map(|w| words::warning(w))
        .collect();
    if !mine.is_empty() {
        lines.push(Line::raw(""));
        for w in mine {
            lines.push(Line::raw(format!("! {w}")));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("More details", theme.dim())));
    let dim = |s: String| Line::from(Span::styled(s, theme.dim()));
    lines.push(dim(format!("  stage {} · role {} · provider {}", row.stage, row.role, row.cli)));
    if let Some(r) = role {
        lines.push(dim(format!("  responsibility: {}", r.responsibility)));
    }
    if let Some(r) = role {
        for cap in &r.requires {
            match provider.and_then(|p| p.enforcement(cap)) {
                Some(e) => lines.push(dim(format!("  requires {cap}: {} · {}", e.strength, e.how))),
                None => lines.push(dim(format!("  requires {cap}: none"))),
            }
        }
    }
    let clis = p.recommendation.clis.join(",");
    lines.push(dim(format!("  aurora catalog recommend --preset {} --clis {clis}", app.opts.preset)));
    lines.push(dim(format!(
        "  aurora workflow init {0} && aurora workflow plan {0}.toml --clis {clis}",
        app.opts.preset
    )));
    f.render_widget(Paragraph::new(crate::ui::hang(lines, area.width)).wrap(Wrap { trim: false }), area);
}
