//! R3 Step 5: the Runs tab over real evidence-gated runs (started with the R1 engine).
//! Submit uses a checkbox form of the stage's acceptance criteria; approve asks for a name.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use aurora_core::workflow::PlanOptions;
use aurora_fs::engine::Engine;
use aurora_tui::app::{App, Command, Tab};
use aurora_tui::execute;
use common::{assert_plain, fixture, press, screen};
use ratatui::crossterm::event::KeyCode;

const MINI: &str = r#"schema_version = 1

[workflow]
id = "mini"
version = "1.0.0"
name = "Mini"

[roles.tester]
responsibility = "Check the result."

[[stages]]
id = "check"
role = "tester"
outputs = ["result.md"]
requires_approval = true

  [[stages.acceptance]]
  id = "done"
  description = "result.md records the check."
  evidence = ["result.md"]

[routing]
allow_override = true

[routing.roles.tester]
eligible = ["claude", "codex"]
preferred = "claude"
"#;

/// Start a run of `toml` (or the bugfix preset) under `root` with id `id`.
fn start(root: &Path, toml: &str, id: &str) -> PathBuf {
    let def = root.join(format!("{id}.toml"));
    fs::write(&def, toml).expect("write definition");
    let started =
        Engine::host().start_run(&def, root, None, &PlanOptions::default(), Some(id), None).expect("start run");
    started.run_dir
}

fn bugfix_toml() -> String {
    aurora_core::presets::get_preset("bugfix").expect("bugfix").toml.to_string()
}

/// Run queued commands as the event loop would.
fn drain(app: &mut App) {
    while let Some(cmd) = app.take_command() {
        app.receive(execute(cmd));
    }
}

fn runs_app(f: &common::Fixture, root: &Path) -> App {
    let mut app = App::new(aurora_tui::app::TuiOptions { run_root: Some(root.to_path_buf()), ..f.options() });
    drain(&mut app);
    press(&mut app, &[KeyCode::Char('4')]);
    drain(&mut app); // opening the Tasks page reads the task list
    assert_eq!(app.tab, Tab::Runs);
    app
}

fn runs_root() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = fs::canonicalize(tmp.path()).expect("canonical");
    (tmp, root)
}

#[test]
fn without_a_run_root_tasks_live_in_the_project() {
    let f = fixture();
    let mut app = App::new(f.options());
    drain(&mut app);
    press(&mut app, &[KeyCode::Char('4')]);
    let s = screen(&app, 120, 24);
    assert!(s.contains("No tasks yet") && s.contains("Press n"), "{s}");
    assert!(!f.project.join(".aurora").exists(), "looking writes nothing");
}

#[test]
fn n_starts_a_task_after_a_yes_and_opens_it() {
    let f = fixture();
    let mut app = App::new(f.options());
    drain(&mut app);
    press(&mut app, &[KeyCode::Char('4'), KeyCode::Char('n')]);
    let s = screen(&app, 120, 30);
    assert!(s.contains("Start a new task") && s.contains("Fix a bug") && s.contains("Claude Code"), "{s}");
    assert!(!f.project.join(".aurora").exists(), "nothing before the yes");

    press(&mut app, &[KeyCode::Char('y')]);
    drain(&mut app);
    assert!(aurora_app::runs::task_workflow(&f.project, "bugfix").is_file(), "the workflow is kept in the project");
    let runs = aurora_app::runs::list_runs(&aurora_app::runs::default_run_root(&f.project)).expect("runs");
    assert_eq!(runs.len(), 1);
    let report = runs[0].status.as_ref().expect("a readable run");
    let tool = |id: &str| report.stages.iter().find(|s| s.id == id).map(|s| s.provider.clone());
    assert_eq!(tool("fix").as_deref(), Some("claude"), "the builder slot");
    assert_eq!(tool("review").as_deref(), Some("codex"), "the reviewer slot: a real second opinion");
    assert_eq!(app.run_open, Some(0), "the new task opens");
    let s = screen(&app, 120, 30);
    assert!(s.contains("1. Reproduce") && s.contains("in progress"), "{s}");
}

#[test]
fn n_then_no_writes_nothing() {
    let f = fixture();
    let mut app = App::new(f.options());
    drain(&mut app);
    press(&mut app, &[KeyCode::Char('4'), KeyCode::Char('n'), KeyCode::Char('n')]);
    drain(&mut app);
    assert!(app.popup().is_none());
    assert!(!f.project.join(".aurora").exists());
}

#[test]
fn runs_are_listed_and_open_into_a_stage_pipeline() {
    let f = fixture();
    let (_t, root) = runs_root();
    start(&root, &bugfix_toml(), "r1");
    let mut app = runs_app(&f, &root);
    let s = screen(&app, 130, 28);
    assert!(s.contains("r1") && s.contains("Fix a bug") && s.contains("0 of 5 done"), "{s}");
    press(&mut app, &[KeyCode::Enter]);
    // The engine marks a stage RUNNING once its handoff is issued.
    let s = screen(&app, 130, 28);
    for needle in ["Reproduce", "Root cause", "Review", "Accept", "in progress", "waiting"] {
        assert!(s.contains(needle), "missing {needle}:\n{s}");
    }
    press(&mut app, &[KeyCode::Enter]);
    drain(&mut app);
    assert!(screen(&app, 130, 28).contains("Aurora handoff"), "Enter on a stage opens its handoff");
    press(&mut app, &[KeyCode::Esc, KeyCode::Esc]);
    assert!(screen(&app, 130, 28).contains("0 of 5 done"), "Esc returns to the task list");
}

#[test]
fn d_archives_only_after_confirmation_and_keeps_the_task_files() {
    let f = fixture();
    let run_root = aurora_app::runs::default_run_root(&f.project);
    fs::create_dir_all(&run_root).expect("project task folder");
    let run = start(&run_root, &bugfix_toml(), "keep-me");
    let mut app = runs_app(&f, &run_root);

    press(&mut app, &[KeyCode::Char('d')]);
    let confirm = screen(&app, 130, 28);
    assert!(confirm.contains("Archive task keep-me?") && confirm.contains("Nothing is deleted"), "{confirm}");
    assert!(run.join("run.json").is_file(), "asking does not move the task");

    press(&mut app, &[KeyCode::Char('n')]);
    assert!(app.popup().is_none());
    assert!(run.join("run.json").is_file(), "No cancels the archive");

    press(&mut app, &[KeyCode::Char('d'), KeyCode::Char('y')]);
    let Some(Command::ArchiveTask { run_dir, project }) = app.take_command() else { panic!("archive queued") };
    assert_eq!(run_dir, run);
    assert_eq!(project, f.project);
    app.receive(execute(Command::ArchiveTask { run_dir, project }));
    drain(&mut app);

    let archived = f.project.join(".aurora/archive/runs/keep-me");
    assert!(!run.exists());
    assert!(archived.join("run.json").is_file(), "the task is preserved");
    assert!(app.runs.is_empty(), "archived task leaves the active list");
    assert!(screen(&app, 130, 28).contains("Task archived"));
}

#[test]
fn search_filters_tasks_and_steps_without_breaking_open() {
    let f = fixture();
    let (_t, root) = runs_root();
    start(&root, &bugfix_toml(), "first");
    start(&root, &bugfix_toml(), "second");
    let mut app = runs_app(&f, &root);

    press(&mut app, &[KeyCode::Char('/'), KeyCode::Char('s'), KeyCode::Char('e'), KeyCode::Char('c'), KeyCode::Enter]);
    assert_eq!(aurora_tui::screens::runs::visible_runs(&app), [1]);
    let s = screen(&app, 130, 28);
    assert!(s.contains("second") && !s.contains("first"), "{s}");
    press(&mut app, &[KeyCode::Enter]);
    assert_eq!(aurora_tui::screens::runs::open_run(&app).map(|r| r.run_id.as_str()), Some("second"));

    press(&mut app, &[KeyCode::Esc]);
    press(&mut app, &[KeyCode::Char('/'), KeyCode::Char('r'), KeyCode::Char('e'), KeyCode::Char('v'), KeyCode::Enter]);
    assert_eq!(aurora_tui::screens::runs::visible_stages(&app), [3, 4]);
    assert_eq!(aurora_tui::screens::runs::selected_stage(&app).map(|s| s.id.as_str()), Some("review"));
}

fn open_submit(app: &mut App) {
    press(app, &[KeyCode::Char('s')]);
    drain(app);
}

#[test]
fn the_submit_form_is_a_checkbox_list_and_the_gate_decides() {
    let f = fixture();
    let (_t, root) = runs_root();
    let run = start(&root, &bugfix_toml(), "r1");
    let mut app = runs_app(&f, &root);
    press(&mut app, &[KeyCode::Enter]);
    open_submit(&mut app);
    let s = screen(&app, 130, 28);
    assert!(s.contains("[ ] ") && s.contains("(behaviour-recorded)") && s.contains("(reproduced)"), "{s}");
    press(&mut app, &[KeyCode::Char(' '), KeyCode::Enter]);
    let Some(Command::Submit { met, .. }) = app.take_command() else { panic!("submit queued") };
    assert_eq!(met, ["behaviour-recorded"]);
    // The outputs do not exist yet: the gate fails, whatever was claimed.
    app.receive(execute(Command::Submit { run_dir: run.clone(), stage: "reproduce".into(), met }));
    drain(&mut app);
    assert!(screen(&app, 130, 28).contains("not done yet"), "{}", screen(&app, 130, 28));

    let out = run.join("artifacts/reproduce");
    fs::create_dir_all(&out).expect("mkdir");
    fs::write(out.join("bugfix.md"), "current, expected, unchanged\n").expect("write");
    fs::write(out.join("repro-test.md"), "failing test\n").expect("write");
    open_submit(&mut app);
    press(&mut app, &[KeyCode::Char(' '), KeyCode::Char('j'), KeyCode::Char(' '), KeyCode::Enter]);
    drain(&mut app);
    let s = screen(&app, 130, 28);
    assert!(s.contains("✓ done"), "{s}");
    assert!(s.contains("1 of 5 done"), "the steps reload: {s}");
}

#[test]
fn esc_cancels_the_form_without_submitting() {
    let f = fixture();
    let (_t, root) = runs_root();
    start(&root, &bugfix_toml(), "r1");
    let mut app = runs_app(&f, &root);
    press(&mut app, &[KeyCode::Enter]);
    open_submit(&mut app);
    press(&mut app, &[KeyCode::Char(' '), KeyCode::Esc]);
    assert!(app.take_command().is_none());
}

#[test]
fn approval_asks_for_a_name_and_completes_the_run() {
    let f = fixture();
    let (_t, root) = runs_root();
    let run = start(&root, MINI, "m1");
    fs::create_dir_all(run.join("artifacts/check")).expect("mkdir");
    fs::write(run.join("artifacts/check/result.md"), "checked\n").expect("write");
    let mut app = runs_app(&f, &root);
    press(&mut app, &[KeyCode::Enter]);
    open_submit(&mut app);
    press(&mut app, &[KeyCode::Char(' '), KeyCode::Enter]);
    drain(&mut app);
    assert!(screen(&app, 130, 28).contains("waiting for your OK"), "{}", screen(&app, 130, 28));

    press(&mut app, &[KeyCode::Char('A')]);
    assert!(screen(&app, 130, 28).contains("Give your OK for Check"), "{}", screen(&app, 130, 28));
    for c in "maintainer".chars() {
        press(&mut app, &[KeyCode::Char(c)]);
    }
    press(&mut app, &[KeyCode::Enter]);
    let Some(Command::Approve { by, stage, .. }) = app.take_command() else { panic!("approve queued") };
    assert_eq!((by.as_str(), stage.as_str()), ("maintainer", "check"));
    app.receive(execute(Command::Approve { run_dir: run.clone(), stage, by }));
    drain(&mut app);
    let s = screen(&app, 130, 28);
    assert!(s.contains("finished ✓") && s.contains("1 of 1 done"), "{s}");
}

#[test]
fn an_empty_name_does_not_approve() {
    let f = fixture();
    let (_t, root) = runs_root();
    start(&root, MINI, "m1");
    let mut app = runs_app(&f, &root);
    press(&mut app, &[KeyCode::Enter, KeyCode::Char('A'), KeyCode::Enter]);
    assert!(app.take_command().is_none(), "Enter with an empty name");
}

#[test]
fn tasks_and_steps_speak_plainly() {
    let f = fixture();
    let (_tmp, root) = runs_root();
    start(&root, &bugfix_toml(), "r1");
    let mut app = runs_app(&f, &root);
    assert_plain(&screen(&app, 130, 30), "task list");
    press(&mut app, &[KeyCode::Enter]);
    for step in 0..5 {
        assert_plain(&screen(&app, 130, 30), &format!("step {step}"));
        press(&mut app, &[KeyCode::Char('j')]);
    }
    press(&mut app, &[KeyCode::Char('g'), KeyCode::Char('s')]);
    assert_plain(&screen(&app, 130, 30), "checklist");
}
