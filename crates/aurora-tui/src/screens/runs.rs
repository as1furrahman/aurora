//! Runs tab: evidence-gated workflow runs under the run root. The list shows each run's
//! progress; Enter opens its stage pipeline, where a stage's handoff can be read, its evidence
//! submitted through a checkbox form of its acceptance criteria, and approval recorded.

use aurora_core::workflow::StageState;
use aurora_fs::engine::{StageStatus, StatusReport};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Row as TableRow, Table, TableState, Wrap};

use crate::app::{App, Tab};
use crate::ui::Theme;

/// Glyph and word for a stage state.
pub fn state_label(s: StageState) -> (&'static str, &'static str) {
    match s {
        StageState::Pending => ("·", "pending"),
        StageState::Ready => ("○", "ready"),
        StageState::Running => ("◔", "running"),
        StageState::Passed => ("✓", "passed"),
        StageState::Failed => ("✗", "failed"),
        StageState::Blocked => ("■", "blocked"),
        StageState::Skipped => ("–", "skipped"),
        StageState::NotApplicable => ("–", "n/a"),
    }
}

/// Passed (or not applicable) stages over all stages.
pub fn progress(r: &StatusReport) -> String {
    let done = r.stages.iter().filter(|s| matches!(s.state, StageState::Passed | StageState::NotApplicable)).count();
    format!("{done}/{}", r.stages.len())
}

fn run_state(r: &StatusReport) -> String {
    if r.complete {
        "complete ✓".into()
    } else if !r.drift.is_empty() {
        "evidence changed".into()
    } else if !r.escalations.is_empty() {
        "escalated".into()
    } else if !r.awaiting_approval.is_empty() {
        "awaiting approval".into()
    } else {
        "in progress".into()
    }
}

/// The open run's status, if a run is open and readable.
pub fn open_run(app: &App) -> Option<&StatusReport> {
    app.run_open.and_then(|i| app.runs.get(i)).and_then(|r| r.status.as_ref().ok())
}

/// The selected stage of the open run.
pub fn selected_stage(app: &App) -> Option<&StageStatus> {
    open_run(app).and_then(|r| r.stages.get(app.stage_cursor))
}

fn stage_note(s: &StageStatus) -> String {
    if s.awaiting_approval {
        "awaiting approval".into()
    } else if s.escalated {
        "escalated".into()
    } else if !s.blocked_reasons.is_empty() {
        s.blocked_reasons.join("; ")
    } else if !s.waiting_on.is_empty() {
        format!("waits for {}", s.waiting_on.join(", "))
    } else if !s.missing.is_empty() && s.state == StageState::Failed {
        format!("missing {}", s.missing.join(", "))
    } else {
        String::new()
    }
}

pub fn render_list(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let Some(root) = &app.opts.run_root else {
        let text = "No run root set.\n\nStart Aurora with --run-root DIR (e.g. `aurora tui --run-root ~/aurora-runs`) to see \
                    workflow runs here. Runs are created with `aurora workflow start`.";
        f.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), area);
        return;
    };
    if let Some(e) = &app.runs_error {
        f.render_widget(Paragraph::new(format!("{e}\n\nPress r to reload.")).wrap(Wrap { trim: true }), area);
        return;
    }
    if let Some(r) = open_run(app) {
        let [head, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(3)]).areas(area);
        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    format!("run {} · {} · {} · {}", r.run_id, r.workflow.id, progress(r), run_state(r)),
                    theme.accent(),
                )),
                Line::from(Span::styled("Esc back to runs", theme.dim())),
            ]),
            head,
        );
        let rows: Vec<TableRow> = r
            .stages
            .iter()
            .map(|s| {
                let (g, w) = state_label(s.state);
                TableRow::new(vec![
                    format!("{g} {w}"),
                    s.id.clone(),
                    s.role.clone(),
                    s.provider.clone(),
                    format!("{}/{}", s.attempts, s.max_attempts),
                    stage_note(s),
                ])
            })
            .collect();
        let table = Table::new(
            rows,
            [
                Constraint::Length(10),
                Constraint::Length(14),
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Length(5),
                Constraint::Min(10),
            ],
        )
        .header(TableRow::new(vec!["state", "stage", "role", "CLI", "try", "note"]).style(theme.accent()))
        .row_highlight_style(theme.focused().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");
        let mut state = TableState::default().with_selected(Some(app.stage_cursor));
        f.render_stateful_widget(table, body, &mut state);
        return;
    }
    if app.runs.is_empty() {
        let text = format!(
            "No runs under {}.\n\nStart one with `aurora workflow start <workflow>.toml --run-root {}`.",
            root.display(),
            root.display()
        );
        f.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), area);
        return;
    }
    let rows: Vec<TableRow> = app
        .runs
        .iter()
        .map(|run| {
            let name = run.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            match &run.status {
                Ok(r) => TableRow::new(vec![name, r.workflow.id.clone(), progress(r), run_state(r)]),
                Err(e) => TableRow::new(vec![name, "?".into(), "?".into(), format!("unreadable: {e}")]),
            }
        })
        .collect();
    let table =
        Table::new(rows, [Constraint::Min(16), Constraint::Length(16), Constraint::Length(7), Constraint::Min(12)])
            .header(TableRow::new(vec!["run", "workflow", "passed", "state"]).style(theme.accent()))
            .row_highlight_style(theme.focused().add_modifier(Modifier::REVERSED))
            .highlight_symbol("▶ ");
    let mut state = TableState::default().with_selected(Some(app.cursor[Tab::Runs.index()]));
    f.render_stateful_widget(table, area, &mut state);
}

pub fn render_detail(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let mut lines: Vec<Line> = Vec::new();
    if let (Some(r), Some(s)) = (open_run(app), selected_stage(app)) {
        let (g, w) = state_label(s.state);
        lines.push(Line::from(Span::styled(format!("{} · {}", s.id, s.role), theme.accent())));
        lines.push(Line::raw(format!(
            "state    {g} {w}{}",
            if s.awaiting_approval { " · awaiting approval" } else { "" }
        )));
        lines.push(Line::raw(format!("CLI      {}", s.provider)));
        lines.push(Line::raw(format!("attempts {}/{}", s.attempts, s.max_attempts)));
        if !s.missing.is_empty() {
            lines.push(Line::raw(format!("missing  {}", s.missing.join(", "))));
        }
        if !s.waiting_on.is_empty() {
            lines.push(Line::raw(format!("waits    {}", s.waiting_on.join(", "))));
        }
        for b in &s.blocked_reasons {
            lines.push(Line::raw(format!("blocked  {b}")));
        }
        if let Some(h) = &s.handoff {
            lines.push(Line::raw(format!("handoff  {}/{h}", r.run_dir)));
        }
        if r.concerns.contains(&s.id) {
            lines.push(Line::raw("note     passed with concerns (carried into later handoffs)"));
        }
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "Keys: Enter read handoff · s submit · A approve · y copy path · R resume",
            theme.dim(),
        )));
        lines.push(Line::from(Span::styled("Equivalent commands", theme.dim())));
        lines.push(Line::raw(format!("  aurora workflow status --run {}", r.run_dir)));
        lines.push(Line::raw(format!(
            "  aurora workflow submit --run {} --stage {} --met <criterion>",
            r.run_dir, s.id
        )));
        lines.push(Line::raw(format!("  aurora workflow approve --run {} --stage {} --by <name>", r.run_dir, s.id)));
    } else if let Some(run) = app.runs.get(app.cursor[Tab::Runs.index()]) {
        match &run.status {
            Ok(r) => {
                lines.push(Line::from(Span::styled(format!("run {}", r.run_id), theme.accent())));
                lines.push(Line::raw(format!("workflow {} v{} · mode {}", r.workflow.id, r.workflow.version, r.mode)));
                lines.push(Line::raw(format!("progress {} · {}", progress(r), run_state(r))));
                lines.push(Line::raw(format!("dir      {}", r.run_dir)));
                for (label, list) in [
                    ("awaiting", &r.awaiting_approval),
                    ("escalated", &r.escalations),
                    ("concerns", &r.concerns),
                    ("waived", &r.waived),
                    ("reduced", &r.reduced_independence),
                ] {
                    if !list.is_empty() {
                        lines.push(Line::raw(format!("{label:<9}{}", list.join(", "))));
                    }
                }
                for d in &r.drift {
                    lines.push(Line::raw(format!("drift    {} {}: {}", d.stage, d.path, d.problem)));
                }
                for w in &r.warnings {
                    lines.push(Line::raw(format!("! {w}")));
                }
                lines.push(Line::raw(""));
                lines.push(Line::from(Span::styled("Enter opens the stage pipeline · r reloads", theme.dim())));
            }
            Err(e) => lines.push(Line::raw(format!("cannot read this run: {e}"))),
        }
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}
