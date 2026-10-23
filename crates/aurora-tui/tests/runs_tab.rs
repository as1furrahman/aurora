//! R3 Step 5: the Runs tab over real evidence-gated runs (started with the R1 engine).
//! Submit uses a checkbox form of the stage's acceptance criteria; approve asks for a name.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use aurora_core::workflow::PlanOptions;
use aurora_fs::engine::Engine;
use aurora_tui::app::{App, Command, Tab};
use aurora_tui::execute;
use common::{fixture, press, screen};
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
    assert_eq!(app.tab, Tab::Runs);
    app
}

fn runs_root() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = fs::canonicalize(tmp.path()).expect("canonical");
    (tmp, root)
}

#[test]
fn without_a_run_root_the_tab_says_how_to_set_one() {
    let f = fixture();
    let mut app = App::new(f.options());
    drain(&mut app);
    press(&mut app, &[KeyCode::Char('4')]);
    assert!(screen(&app, 120, 24).contains("--run-root"), "{}", screen(&app, 120, 24));
}

#[test]
fn runs_are_listed_and_open_into_a_stage_pipeline() {
    let f = fixture();
    let (_t, root) = runs_root();
    start(&root, &bugfix_toml(), "r1");
    let mut app = runs_app(&f, &root);
    let s = screen(&app, 130, 28);
    assert!(s.contains("r1") && s.contains("bugfix") && s.contains("0/5"), "{s}");
    press(&mut app, &[KeyCode::Enter]);
    // The engine marks a stage RUNNING once its handoff is issued.
    let s = screen(&app, 130, 28);
    for needle in ["reproduce", "root-cause", "review", "accept", "running", "pending"] {
        assert!(s.contains(needle), "missing {needle}:\n{s}");
    }
    press(&mut app, &[KeyCode::Enter]);
    drain(&mut app);
    assert!(screen(&app, 130, 28).contains("Aurora handoff"), "Enter on a stage opens its handoff");
    press(&mut app, &[KeyCode::Esc, KeyCode::Esc]);
    assert!(screen(&app, 130, 28).contains("0/5"), "Esc returns to the run list");
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
    assert!(s.contains("[ ] behaviour-recorded") && s.contains("[ ] reproduced"), "{s}");
    press(&mut app, &[KeyCode::Char(' '), KeyCode::Enter]);
    let Some(Command::Submit { met, .. }) = app.take_command() else { panic!("submit queued") };
    assert_eq!(met, ["behaviour-recorded"]);
    // The outputs do not exist yet: the gate fails, whatever was claimed.
    app.receive(execute(Command::Submit { run_dir: run.clone(), stage: "reproduce".into(), met }));
    drain(&mut app);
    assert!(screen(&app, 130, 28).contains("FAILED"), "{}", screen(&app, 130, 28));

    let out = run.join("artifacts/reproduce");
    fs::create_dir_all(&out).expect("mkdir");
    fs::write(out.join("bugfix.md"), "current, expected, unchanged\n").expect("write");
    fs::write(out.join("repro-test.md"), "failing test\n").expect("write");
    open_submit(&mut app);
    press(&mut app, &[KeyCode::Char(' '), KeyCode::Char('j'), KeyCode::Char(' '), KeyCode::Enter]);
    drain(&mut app);
    let s = screen(&app, 130, 28);
    assert!(s.contains("PASSED"), "{s}");
    assert!(s.contains("1/5"), "the pipeline reloads: {s}");
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
    assert!(screen(&app, 130, 28).contains("awaiting approval"), "{}", screen(&app, 130, 28));

    press(&mut app, &[KeyCode::Char('A')]);
    assert!(screen(&app, 130, 28).contains("Approve check as"), "{}", screen(&app, 130, 28));
    for c in "maintainer".chars() {
        press(&mut app, &[KeyCode::Char(c)]);
    }
    press(&mut app, &[KeyCode::Enter]);
    let Some(Command::Approve { by, stage, .. }) = app.take_command() else { panic!("approve queued") };
    assert_eq!((by.as_str(), stage.as_str()), ("maintainer", "check"));
    app.receive(execute(Command::Approve { run_dir: run.clone(), stage, by }));
    drain(&mut app);
    let s = screen(&app, 130, 28);
    assert!(s.contains("complete") && s.contains("1/1"), "{s}");
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
