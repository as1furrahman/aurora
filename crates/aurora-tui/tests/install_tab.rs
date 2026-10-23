//! R3 Step 4: the Install tab. Review flagged files, apply behind a confirm that defaults to No,
//! undo; apply and undo run through the same aurora-app functions as `aurora skills apply/undo`.

mod common;

use aurora_tui::app::{App, Command, Msg, Tab};
use aurora_tui::execute;
use aurora_tui::screens::install::rows;
use common::{fixture, press, screen};
use ratatui::crossterm::event::KeyCode;

/// Run every queued command (as the event loop would) and feed back the results.
fn drain(app: &mut App) -> Vec<&'static str> {
    let mut ran = Vec::new();
    while let Some(cmd) = app.take_command() {
        ran.push(match &cmd {
            Command::Plan { .. } => "plan",
            Command::Apply { .. } => "apply",
            Command::Undo { .. } => "undo",
        });
        app.receive(execute(cmd));
    }
    ran
}

fn on_install(f: &common::Fixture) -> App {
    let mut app = App::new(f.options());
    drain(&mut app);
    press(&mut app, &[KeyCode::Char('3')]);
    assert_eq!(app.tab, Tab::Install);
    app
}

fn select(app: &mut App, target: &str) {
    app.cursor[Tab::Install.index()] = rows(app).iter().position(|a| a.target == target).expect("action");
}

#[test]
fn the_plan_lists_every_action() {
    let f = fixture();
    let app = on_install(&f);
    let targets: Vec<String> = rows(&app).into_iter().map(|a| a.target).collect();
    for t in [
        ".claude/skills/demo",
        ".agents/skills/demo",
        ".agents/skills/checker",
        ".claude/skills/scripted",
        "CLAUDE.md",
        "AGENTS.md",
    ] {
        assert!(targets.iter().any(|x| x == t), "missing {t}: {targets:?}");
    }
    let s = screen(&app, 130, 30);
    snapshot!("plan", s);
}

#[test]
fn apply_refuses_until_flagged_skills_are_reviewed() {
    let f = fixture();
    let mut app = on_install(&f);
    press(&mut app, &[KeyCode::Char('a')]);
    assert!(app.take_command().is_none());
    assert!(screen(&app, 130, 30).contains("review scripted first"), "{}", screen(&app, 130, 30));
}

#[test]
fn enter_shows_the_flagged_files_from_the_verified_copy() {
    let f = fixture();
    let mut app = on_install(&f);
    select(&mut app, ".claude/skills/scripted");
    press(&mut app, &[KeyCode::Enter]);
    let s = screen(&app, 130, 30);
    assert!(s.contains("run.sh") && s.contains("echo hi"), "{s}");
    press(&mut app, &[KeyCode::Esc]);
    assert!(!screen(&app, 130, 30).contains("echo hi"));
}

#[test]
fn the_confirm_dialog_defaults_to_no() {
    let f = fixture();
    let mut app = on_install(&f);
    select(&mut app, ".claude/skills/scripted");
    press(&mut app, &[KeyCode::Char('r'), KeyCode::Char('a')]);
    let s = screen(&app, 130, 30);
    assert!(s.contains("Apply") && s.contains(".claude/skills/demo") && s.contains("y apply"), "{s}");
    press(&mut app, &[KeyCode::Enter]);
    assert!(app.take_command().is_none(), "Enter means No");
    press(&mut app, &[KeyCode::Char('a'), KeyCode::Char('n')]);
    assert!(app.take_command().is_none());
    press(&mut app, &[KeyCode::Char('a'), KeyCode::Esc]);
    assert!(app.take_command().is_none());
}

#[test]
fn apply_then_undo_round_trips_through_the_engine() {
    let f = fixture();
    let mut app = on_install(&f);
    select(&mut app, ".claude/skills/scripted");
    press(&mut app, &[KeyCode::Char('r'), KeyCode::Char('a'), KeyCode::Char('y')]);
    assert_eq!(drain(&mut app), ["apply", "plan"], "apply, then refresh the plan");
    assert!(f.project.join(".claude/skills/demo/SKILL.md").is_file());
    assert!(f.project.join("CLAUDE.md").is_file());
    let s = screen(&app, 130, 30);
    assert!(s.contains("Applied") && s.contains("u undoes"), "{s}");
    assert!(rows(&app).iter().all(|a| a.kind == aurora_skills::plan::ActionKind::Unchanged), "re-plan is unchanged");

    press(&mut app, &[KeyCode::Char('a')]);
    assert!(app.take_command().is_none());
    assert!(screen(&app, 130, 30).contains("Nothing to do"));

    press(&mut app, &[KeyCode::Char('u'), KeyCode::Char('y')]);
    assert_eq!(drain(&mut app), ["undo", "plan"]);
    assert!(!f.project.join("CLAUDE.md").exists() && !f.project.join(".claude/skills/demo").exists());
    assert!(screen(&app, 130, 30).contains("Restored"));
    for _ in 0..200 {
        app.tick();
    }
    let s = screen(&app, 130, 30);
    assert!(s.contains("last: Restored"), "the outcome stays visible after the toast fades:\n{s}");
}

#[test]
fn one_operation_at_a_time() {
    let f = fixture();
    let mut app = on_install(&f);
    select(&mut app, ".claude/skills/scripted");
    press(&mut app, &[KeyCode::Char('r'), KeyCode::Char('a'), KeyCode::Char('y')]);
    let first = app.take_command().expect("apply queued");
    assert!(matches!(first, Command::Apply { .. }));
    press(&mut app, &[KeyCode::Char('u')]);
    assert!(app.take_command().is_none(), "no undo while the apply runs");
    assert!(screen(&app, 130, 30).contains("busy"));
    app.receive(execute(first));
}

#[test]
fn a_failed_apply_is_reported_not_hidden() {
    let f = fixture();
    let mut app = on_install(&f);
    app.receive(Msg::Applied(Err("refusing to install into /: protected".into())));
    assert!(screen(&app, 130, 30).contains("refusing to install"));
}

#[test]
fn the_worker_rechecks_the_review_gate() {
    // Defence in depth: even a hand-built Apply command cannot skip review.
    let f = fixture();
    let planned = f.planned(&[]);
    let msg = execute(Command::Apply { planned: Box::new(planned), reviewed: vec![] });
    let Msg::Applied(Err(e)) = msg else { panic!("must refuse: {msg:?}") };
    assert!(e.contains("scripted") && e.contains("review"), "{e}");
    assert!(!f.project.join("CLAUDE.md").exists(), "nothing was written");
}
