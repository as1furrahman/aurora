//! "Add to project" page: exactly what Aurora will add or change in this project, what needs a
//! quick check by you first, and add/undo behind a yes-or-no question that defaults to no.

use aurora_skills::plan::{Action as PlanAction, ActionKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Row as TableRow, Table, TableState, Wrap};

use crate::app::{App, Tab};
use crate::ui::Theme;
use crate::words;

/// The plan's actions for the current search.
pub fn rows(app: &App) -> Vec<PlanAction> {
    let Some(p) = &app.planned else { return Vec::new() };
    let filter = app.filter().to_lowercase();
    p.plan
        .actions
        .iter()
        .filter(|a| {
            filter.is_empty() || a.target.to_lowercase().contains(&filter) || a.entry.to_lowercase().contains(&filter)
        })
        .cloned()
        .collect()
}

/// Glyph and plain words for what happens to one item.
pub fn kind_label(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::Create => "+ will add",
        ActionKind::Update => "↑ will update",
        ActionKind::Unchanged => "✓ already added",
        ActionKind::Keep => "· kept",
        ActionKind::Remove => "− will remove",
        ActionKind::Refuse => "✗ skipped",
    }
}

/// What happens to one item, with "check first" taking the place of "will add" until you've
/// marked a flagged skill as checked.
fn happens(app: &App, a: &PlanAction) -> &'static str {
    if a.needs_review && !app.reviewed.contains(&a.entry) && matches!(a.kind, ActionKind::Create | ActionKind::Update) {
        "? check first"
    } else {
        kind_label(a.kind)
    }
}

/// Who an item is for, in plain words. Rules files name the tools that read them.
fn for_whom(app: &App, a: &PlanAction) -> String {
    if !a.providers.is_empty() {
        return words::tools(&a.providers);
    }
    let (Some(p), Ok(cat)) = (&app.planned, &app.catalog) else { return String::new() };
    let readers: Vec<String> = p
        .recommendation
        .clis
        .iter()
        .filter(|c| cat.target(c).is_some_and(|t| t.rules_file == a.target))
        .cloned()
        .collect();
    words::tools(&readers)
}

pub fn render_list(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let Some(p) = &app.planned else { return };
    let rows = rows(app);
    let [intro_area, table_area, summary_area] =
        Layout::vertical([Constraint::Length(2), Constraint::Min(3), Constraint::Length(3)]).areas(area);
    f.render_widget(
        Paragraph::new(vec![
            Line::raw("This is exactly what Aurora will add to your project. Nothing changes until you say yes."),
            Line::from(Span::styled("Everything can be undone with u.", theme.dim())),
        ])
        .wrap(Wrap { trim: true }),
        intro_area,
    );
    let body: Vec<TableRow> = rows
        .iter()
        .map(|a| TableRow::new(vec![happens(app, a).to_string(), a.entry.clone(), for_whom(app, a)]))
        .collect();
    let table = Table::new(body, [Constraint::Length(15), Constraint::Fill(3), Constraint::Fill(2)])
        .header(TableRow::new(vec!["What happens", "Skill", "For"]).style(theme.accent()))
        .row_highlight_style(theme.focused().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");
    let mut state = TableState::default().with_selected(Some(app.cursor[Tab::Install.index()]));
    f.render_stateful_widget(table, table_area, &mut state);

    let unreviewed = p.unreviewed(&app.reviewed);
    let n = p.change_count();
    let mut lines = vec![if !p.plan.changed {
        Line::raw("Your project already has everything for this task. Nothing to add.")
    } else if unreviewed.is_empty() {
        Line::from(Span::styled(format!("Ready: press a to add these {n} things to your project."), theme.accent()))
    } else {
        Line::raw(format!(
            "Before adding, please check: {}. Select it, press Enter to read it, then r.",
            unreviewed.join(", ")
        ))
    }];
    if let Some(last) = &app.last_result {
        lines.push(Line::raw(format!("Last change: {last}")));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), summary_area);
}

pub fn render_detail(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let rows = rows(app);
    let (Some(a), Some(p)) = (rows.get(app.cursor[Tab::Install.index()]), &app.planned) else { return };
    let label = |s: &str| Span::styled(format!("{s:<10}"), theme.accent());
    let mut lines = vec![
        Line::from(Span::styled(a.entry.clone(), theme.accent())),
        Line::raw(""),
        Line::from(vec![
            label("Happens"),
            Span::raw(kind_label(a.kind).trim_start_matches(|c: char| !c.is_alphabetic()).to_string()),
        ]),
        Line::from(vec![label("Where"), Span::raw(a.target.clone())]),
    ];
    let whom = for_whom(app, a);
    if !whom.is_empty() {
        lines.push(Line::from(vec![label("For"), Span::raw(whom)]));
    }
    if !a.detail.is_empty() {
        lines.push(Line::from(vec![label("Why"), Span::raw(words::reason(&a.detail))]));
    }
    if let Some(l) = p.plan.lock.skills.get(&a.target)
        && l.overlays.iter().any(|o| o == "allowed-tools")
    {
        lines.push(Line::from(vec![
            label("Safety"),
            Span::raw("Aurora switches off this skill's permission to run commands without asking you."),
        ]));
    }
    if a.needs_review {
        lines.push(Line::raw(""));
        let files = p.review_files.get(&a.entry).map(|f| f.join(", ")).unwrap_or_default();
        let state = if app.reviewed.contains(&a.entry) { "You've marked it as checked ✓" } else { "Not checked yet" };
        lines.push(Line::from(vec![
            label("Check it"),
            Span::raw(format!("Press Enter to read {files}, then r if it looks fine. {state}.")),
        ]));
    }
    let warnings: Vec<String> = p.all_warnings().iter().map(|w| words::warning(w)).collect();
    if !warnings.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled("Good to know", theme.accent())));
        for w in warnings.iter().take(5) {
            lines.push(Line::raw(format!("• {w}")));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("More details", theme.dim())));
    let dim = |s: String| Line::from(Span::styled(s, theme.dim()));
    if let Some(l) = p.plan.lock.skills.get(&a.target) {
        lines.push(dim(format!("  source {}@{} {}", l.repo, &l.rev[..12.min(l.rev.len())], l.path)));
        if !l.overlays.is_empty() {
            lines.push(dim(format!("  overlays {} removed", l.overlays.join(", "))));
        }
    }
    if !a.detail.is_empty() {
        lines.push(dim(format!("  plan: {}", a.detail)));
    }
    lines.push(dim("  Aurora records what it added in aurora.lock and keeps backups in .aurora/".to_string()));
    let mut apply =
        format!("aurora skills apply --preset {} --project {}", app.opts.preset, app.opts.project.display());
    if !app.opts.clis.is_empty() {
        apply.push_str(&format!(" --clis {}", app.opts.clis.join(",")));
    }
    for w in &app.with {
        apply.push_str(&format!(" --with {w}"));
    }
    for r in &app.reviewed {
        apply.push_str(&format!(" --reviewed {r}"));
    }
    lines.push(dim(format!("  {apply}")));
    lines.push(dim(format!("  aurora skills undo --project {}", app.opts.project.display())));
    f.render_widget(Paragraph::new(crate::ui::hang(lines, area.width)).wrap(Wrap { trim: false }), area);
}
