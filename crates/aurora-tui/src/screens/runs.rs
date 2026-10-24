//! Tasks page: your work split into steps. Each step is done by one AI tool: open a step, copy
//! its instructions into that tool, then tick its checklist here. Aurora only accepts a step when
//! the files it asked for really exist.

use aurora_fs::engine::{StageStatus, StatusReport};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Row as TableRow, Table, TableState, Wrap};

use crate::app::{App, Tab};
use crate::ui::Theme;
use crate::words;

/// Glyph and words for a step's state.
pub fn state_label(s: aurora_core::workflow::StageState) -> (&'static str, &'static str) {
    words::step_state(s)
}

/// "2 of 5 done".
pub fn progress(r: &StatusReport) -> String {
    use aurora_core::workflow::StageState;
    let done = r.stages.iter().filter(|s| matches!(s.state, StageState::Passed | StageState::NotApplicable)).count();
    format!("{done} of {} done", r.stages.len())
}

fn task_state(r: &StatusReport) -> String {
    if r.complete {
        "finished ✓".into()
    } else if !r.drift.is_empty() {
        "a finished step's files changed".into()
    } else if !r.escalations.is_empty() {
        "needs a person to look".into()
    } else if !r.awaiting_approval.is_empty() {
        "waiting for your OK".into()
    } else {
        "in progress".into()
    }
}

/// The open task's status, if a task is open and readable.
pub fn open_run(app: &App) -> Option<&StatusReport> {
    app.run_open.and_then(|i| app.runs.get(i)).and_then(|r| r.status.as_ref().ok())
}

/// The selected step of the open task.
pub fn selected_stage(app: &App) -> Option<&StageStatus> {
    open_run(app).and_then(|r| r.stages.get(app.stage_cursor))
}

fn step_note(s: &StageStatus) -> String {
    if s.awaiting_approval {
        "waiting for your OK (A)".into()
    } else if s.escalated {
        "too many tries: needs a person".into()
    } else if !s.blocked_reasons.is_empty() {
        "stuck: see details".into()
    } else if !s.waiting_on.is_empty() {
        let w: Vec<String> = s.waiting_on.iter().map(|x| words::step(x)).collect();
        format!("waits for {}", w.join(", "))
    } else if !s.missing.is_empty() && s.state == aurora_core::workflow::StageState::Failed {
        format!(
            "missing {}",
            s.missing.iter().map(|m| m.rsplit('/').next().unwrap_or(m)).collect::<Vec<_>>().join(", ")
        )
    } else {
        String::new()
    }
}

fn intro(f: &mut Frame, area: Rect, first: &str, second: &str, theme: Theme) {
    f.render_widget(
        Paragraph::new(vec![Line::raw(first.to_string()), Line::from(Span::styled(second.to_string(), theme.dim()))])
            .wrap(Wrap { trim: true }),
        area,
    );
}

pub fn render_list(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let [intro_area, body] = Layout::vertical([Constraint::Length(2), Constraint::Min(3)]).areas(area);
    let Some(root) = &app.opts.run_root else {
        intro(f, intro_area, "A task is your work split into steps, each done by one AI tool.", "", theme);
        let text = "No tasks folder chosen yet.\n\nStart Aurora with --run-root and a folder, for example:\n  aurora tui --run-root ~/aurora-tasks\n\nTasks are created with `aurora workflow start`.";
        f.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), body);
        return;
    };
    if let Some(e) = &app.runs_error {
        intro(f, intro_area, "Aurora couldn't read the tasks folder.", "Press r to try again.", theme);
        f.render_widget(Paragraph::new(e.clone()).wrap(Wrap { trim: true }), body);
        return;
    }
    if let Some(r) = open_run(app) {
        intro(
            f,
            intro_area,
            &format!("Task {} · {} · {} · {}", r.run_id, words::task_type(&r.workflow.id), progress(r), task_state(r)),
            "Open a step with Enter to see its instructions · Esc goes back to all tasks",
            theme,
        );
        let rows: Vec<TableRow> = r
            .stages
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let (g, w) = state_label(s.state);
                TableRow::new(vec![
                    format!("{}. {}", i + 1, words::step(&s.id)),
                    words::job(&s.role).to_string(),
                    words::tool(&s.provider).to_string(),
                    format!("{g} {w}"),
                    step_note(s),
                ])
            })
            .collect();
        let table = Table::new(
            rows,
            [
                Constraint::Length(16),
                Constraint::Length(17),
                Constraint::Length(12),
                Constraint::Length(15),
                Constraint::Min(10),
            ],
        )
        .header(TableRow::new(vec!["Step", "Job", "AI tool", "Status", ""]).style(theme.accent()))
        .row_highlight_style(theme.focused().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▶ ");
        let mut state = TableState::default().with_selected(Some(app.stage_cursor));
        f.render_stateful_widget(table, body, &mut state);
        return;
    }
    intro(
        f,
        intro_area,
        "A task is your work split into steps, each done by one AI tool.",
        "Pick a task and press Enter to see its steps.",
        theme,
    );
    if app.runs.is_empty() {
        let tools = crate::screens::roles::profile_summary(app).unwrap_or_else(|| "your AI tools".into());
        let lines = vec![
            Line::raw("No tasks yet."),
            Line::raw(""),
            Line::from(vec![
                Span::styled("Press n", theme.accent()),
                Span::raw(format!(
                    " to start one: “{}” with {tools}. Aurora asks before it saves anything.",
                    words::task_type(&app.opts.preset)
                )),
            ]),
            Line::raw(""),
            Line::from(Span::styled(format!("Tasks are kept in {}", root.display()), theme.dim())),
        ];
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), body);
        return;
    }
    let rows: Vec<TableRow> = app
        .runs
        .iter()
        .map(|run| {
            let name = run.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            match &run.status {
                Ok(r) => {
                    TableRow::new(vec![name, words::task_type(&r.workflow.id).to_string(), progress(r), task_state(r)])
                }
                Err(_) => TableRow::new(vec![name, "?".into(), "?".into(), "can't be read".into()]),
            }
        })
        .collect();
    let table =
        Table::new(rows, [Constraint::Min(14), Constraint::Length(22), Constraint::Length(12), Constraint::Min(12)])
            .header(TableRow::new(vec!["Task", "Kind", "Progress", "Status"]).style(theme.accent()))
            .row_highlight_style(theme.focused().add_modifier(Modifier::REVERSED))
            .highlight_symbol("▶ ");
    let mut state = TableState::default().with_selected(Some(app.cursor[Tab::Runs.index()]));
    f.render_stateful_widget(table, body, &mut state);
}

pub fn render_detail(f: &mut Frame, app: &App, area: Rect, theme: Theme) {
    let label = |s: &str| Span::styled(format!("{s:<12}"), theme.accent());
    let dim = |s: String| Line::from(Span::styled(s, theme.dim()));
    let mut lines: Vec<Line> = Vec::new();
    if let (Some(r), Some(s)) = (open_run(app), selected_stage(app)) {
        let (g, w) = state_label(s.state);
        let tool = words::tool(&s.provider);
        let n = r.stages.iter().position(|x| x.id == s.id).map_or(0, |i| i + 1);
        lines.push(Line::from(Span::styled(
            format!("Step {n} of {}: {}", r.stages.len(), words::step(&s.id)),
            theme.accent(),
        )));
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            label("Status"),
            Span::raw(format!("{g} {w}{}", if s.awaiting_approval { ", waiting for your OK" } else { "" })),
        ]));
        lines.push(Line::from(vec![
            label("Job"),
            Span::raw(format!("the {}, who {}", words::job(&s.role), words::job_explained(&s.role))),
        ]));
        lines.push(Line::from(vec![label("AI tool"), Span::raw(tool.to_string())]));
        lines.push(Line::from(vec![label("Tries"), Span::raw(format!("{} of {}", s.attempts, s.max_attempts))]));
        if !s.waiting_on.is_empty() {
            let w: Vec<String> = s.waiting_on.iter().map(|x| words::step(x)).collect();
            lines.push(Line::from(vec![label("Waits for"), Span::raw(w.join(", "))]));
        }
        if !s.missing.is_empty() && s.state == aurora_core::workflow::StageState::Failed {
            let files: Vec<&str> = s.missing.iter().map(|m| m.rsplit('/').next().unwrap_or(m)).collect();
            lines.push(Line::from(vec![
                label("Missing"),
                Span::raw(format!("{} (save it in the step's folder, then tick again)", files.join(", "))),
            ]));
        }
        if r.concerns.contains(&s.id) {
            lines.push(Line::from(vec![label("Note"), Span::raw("Done, with concerns the next steps will see.")]));
        }
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled("What to do", theme.accent())));
        lines.push(Line::raw(format!("  1. Press Enter to see the instructions, and paste them into {tool}.")));
        lines.push(Line::raw("  2. When it's finished, press s and tick the checklist."));
        if s.awaiting_approval {
            lines.push(Line::raw("  3. This step needs your OK: press A and type your name."));
        }
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled("More details", theme.dim())));
        if let Some(h) = &s.handoff {
            lines.push(dim(format!("  instructions file {}/{h}", r.run_dir)));
        }
        for b in &s.blocked_reasons {
            lines.push(dim(format!("  blocked: {b}")));
        }
        lines.push(dim(format!("  aurora workflow status --run {}", r.run_dir)));
        lines.push(dim(format!("  aurora workflow submit --run {} --stage {} --met <item>", r.run_dir, s.id)));
        lines.push(dim(format!("  aurora workflow approve --run {} --stage {} --by <name>", r.run_dir, s.id)));
    } else if let Some(run) = app.runs.get(app.cursor[Tab::Runs.index()]) {
        match &run.status {
            Ok(r) => {
                lines.push(Line::from(Span::styled(format!("Task {}", r.run_id), theme.accent())));
                lines.push(Line::raw(""));
                lines.push(Line::from(vec![label("Kind"), Span::raw(words::task_type(&r.workflow.id).to_string())]));
                lines.push(Line::from(vec![
                    label("Steps"),
                    Span::raw(words::task_type_explained(&r.workflow.id).to_string()),
                ]));
                lines.push(Line::from(vec![
                    label("Progress"),
                    Span::raw(format!("{} · {}", progress(r), task_state(r))),
                ]));
                if !r.awaiting_approval.is_empty() {
                    let w: Vec<String> = r.awaiting_approval.iter().map(|x| words::step(x)).collect();
                    lines.push(Line::from(vec![label("Your OK"), Span::raw(format!("needed for {}", w.join(", ")))]));
                }
                for d in &r.drift {
                    lines.push(Line::from(vec![
                        label("Changed"),
                        Span::raw(format!(
                            "{} of {} changed after it was done",
                            d.path.rsplit('/').next().unwrap_or(&d.path),
                            words::step(&d.stage)
                        )),
                    ]));
                }
                lines.push(Line::raw(""));
                lines.push(Line::from(Span::styled("Press Enter to see its steps.", theme.dim())));
                lines.push(Line::raw(""));
                lines.push(Line::from(Span::styled("More details", theme.dim())));
                lines.push(dim(format!("  workflow {} v{} · mode {}", r.workflow.id, r.workflow.version, r.mode)));
                lines.push(dim(format!("  folder {}", r.run_dir)));
                for (label, list) in [
                    ("escalated", &r.escalations),
                    ("concerns", &r.concerns),
                    ("waived", &r.waived),
                    ("reduced independence", &r.reduced_independence),
                ] {
                    if !list.is_empty() {
                        lines.push(dim(format!("  {label}: {}", list.join(", "))));
                    }
                }
                for w in &r.warnings {
                    lines.push(dim(format!("  ! {w}")));
                }
            }
            Err(e) => lines.push(Line::raw(format!("This task can't be read: {e}"))),
        }
    }
    f.render_widget(Paragraph::new(crate::ui::hang(lines, area.width)).wrap(Wrap { trim: false }), area);
}
