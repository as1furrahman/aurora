//! Install tab: the plan's actions (what `aurora skills apply` would do), the review state of
//! each skill that needs one, and apply/undo behind a confirm dialog that defaults to No.

use aurora_skills::plan::{Action as PlanAction, ActionKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Row as TableRow, Table, TableState, Wrap};

use crate::app::{App, Tab};
use crate::ui::Theme;

/// The plan's actions for the current filter.
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

fn kind_label(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::Create => "+ create",
        ActionKind::Update => "↑ update",
        ActionKind::Unchanged => "✓ unchanged",
        ActionKind::Keep => "· keep",
        ActionKind::Remove => "− remove",
        ActionKind::Refuse => "✗ refuse",
    }
}

fn review_note(app: &App, a: &PlanAction) -> &'static str {
    if !a.needs_review {
        ""
    } else if app.reviewed.contains(&a.entry) {
        "reviewed ✓"
    } else {
        "needs review"
    }
}

pub fn render_list(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let Some(p) = &app.planned else { return };
    let rows = rows(app);
    let [table_area, summary_area] = Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).areas(area);
    let body: Vec<TableRow> = rows
        .iter()
        .map(|a| {
            TableRow::new(vec![
                kind_label(a.kind).to_string(),
                a.target.clone(),
                a.entry.clone(),
                review_note(app, a).to_string(),
            ])
        })
        .collect();
    let table =
        Table::new(body, [Constraint::Length(12), Constraint::Min(24), Constraint::Length(22), Constraint::Length(13)])
            .header(TableRow::new(vec!["action", "target", "entry", "review"]).style(theme.accent()))
            .row_highlight_style(theme.focused().add_modifier(Modifier::REVERSED))
            .highlight_symbol("▶ ");
    let mut state = TableState::default().with_selected(Some(app.cursor[Tab::Install.index()]));
    f.render_stateful_widget(table, table_area, &mut state);

    let unreviewed = p.unreviewed(&app.reviewed);
    let lock = if p.plan.changed { "aurora.lock would be written" } else { "aurora.lock unchanged" };
    let mut lines =
        vec![Line::raw(format!("{} changes · {lock} · {} warnings", p.change_count(), p.all_warnings().len()))];
    lines.push(if unreviewed.is_empty() {
        Line::from(Span::styled("ready: press a to apply (you will be asked first)", theme.dim()))
    } else {
        Line::raw(format!("needs your review first: {}", unreviewed.join(", ")))
    });
    if let Some(last) = &app.last_result {
        lines.push(Line::raw(format!("last: {last}")));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), summary_area);
}

pub fn render_detail(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let rows = rows(app);
    let (Some(a), Some(p)) = (rows.get(app.cursor[Tab::Install.index()]), &app.planned) else { return };
    let mut lines = vec![
        Line::from(Span::styled(a.target.clone(), theme.accent())),
        Line::raw(format!("{} · entry {}", kind_label(a.kind), a.entry)),
    ];
    if !a.providers.is_empty() {
        lines.push(Line::raw(format!("for      {}", a.providers.join(", "))));
    }
    if !a.detail.is_empty() {
        lines.push(Line::raw(format!("why      {}", a.detail)));
    }
    if let Some(l) = p.plan.lock.skills.get(&a.target) {
        lines.push(Line::raw(format!("source   {}@{} {}", l.repo, &l.rev[..12.min(l.rev.len())], l.path)));
        if !l.overlays.is_empty() {
            lines.push(Line::raw(format!("overlays {} removed", l.overlays.join(", "))));
        }
    }
    if a.needs_review {
        lines.push(Line::raw(""));
        let files = p.review_files.get(&a.entry).map(|f| f.join(", ")).unwrap_or_default();
        lines.push(Line::raw(format!("review   read {files} (Enter), then mark it with r — {}", review_note(app, a))));
    }
    let warnings = p.all_warnings();
    if !warnings.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled("Plan warnings", theme.accent())));
        for w in warnings.iter().take(6) {
            lines.push(Line::raw(format!("! {w}")));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled("Equivalent commands", theme.dim())));
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
    lines.push(Line::raw(format!("  {apply}")));
    lines.push(Line::raw(format!("  aurora skills undo --project {}", app.opts.project.display())));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}
