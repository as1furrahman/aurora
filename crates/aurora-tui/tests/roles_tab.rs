//! R3 Step 3: the Roles tab. Stage -> role -> CLI -> skills, with one-key changes of the
//! workflow (w), the profile (P) and the CLI playing a role (p); every change re-plans.

mod common;

use aurora_tui::app::{App, Command, Msg, Tab};
use aurora_tui::screens::roles::rows;
use common::{fixture, press, screen};
use ratatui::crossterm::event::KeyCode;

fn run_plan(app: &mut App) -> aurora_app::install::InstallRequest {
    let Some(Command::Plan { generation, request }) = app.take_command() else { panic!("a plan was queued") };
    app.receive(Msg::Planned { generation, result: Box::new(aurora_app::install::plan_install(&request)) });
    request
}

fn on_roles(f: &common::Fixture) -> App {
    let mut app = App::new(f.options());
    run_plan(&mut app);
    press(&mut app, &[KeyCode::Char('2')]);
    assert_eq!(app.tab, Tab::Roles);
    app
}

#[test]
fn the_board_lists_every_stage_with_its_cli_and_skills() {
    let f = fixture();
    let app = on_roles(&f);
    let r = rows(&app);
    let stages: Vec<(&str, &str, &str)> =
        r.iter().map(|s| (s.stage.as_str(), s.role.as_str(), s.cli.as_str())).collect();
    assert_eq!(
        stages,
        [
            ("reproduce", "tester", "codex"),
            ("root-cause", "implementer", "claude"),
            ("fix", "implementer", "claude"),
            ("review", "reviewer", "codex"),
            ("accept", "tester", "codex"),
        ]
    );
    assert!(r[3].skills.contains(&"checker".to_string()));
    let s = screen(&app, 130, 30);
    snapshot!("board", s);
}

#[test]
fn the_detail_shows_invocation_syntax_and_enforcement() {
    let f = fixture();
    let mut app = on_roles(&f);
    app.cursor[Tab::Roles.index()] = 3; // review, on codex
    let s = screen(&app, 140, 34);
    for needle in [
        "review",
        "reviewer",
        "Codex",
        "`$checker`",
        "read_only",
        "hard",
        "is also the checker",
        "aurora catalog recommend --preset bugfix",
    ] {
        assert!(s.contains(needle), "missing {needle:?}:\n{s}");
    }
}

#[test]
fn p_gives_the_selected_role_to_the_next_free_cli() {
    let f = fixture();
    let mut app = on_roles(&f);
    app.cursor[Tab::Roles.index()] = 3; // reviewer slot = codex
    press(&mut app, &[KeyCode::Char('p')]);
    let req = run_plan(&mut app);
    assert_eq!(req.clis, ["claude", "hermes"], "codex -> hermes; claude keeps the builder slot");
    let review = rows(&app).into_iter().find(|s| s.stage == "review").expect("review");
    assert_eq!(review.cli, "hermes");
}

#[test]
fn shift_p_cycles_the_profile_and_resets_the_clis() {
    let f = fixture();
    let mut app = on_roles(&f);
    press(&mut app, &[KeyCode::Char('P')]);
    let req = run_plan(&mut app);
    assert_eq!(req.profile.as_deref(), Some("solo"), "the fixture catalog has solo and pair; pair wraps to solo");
    assert!(req.clis.is_empty(), "the profile's own CLIs");
    assert!(rows(&app).iter().all(|s| s.cli == "claude"));
    let s = screen(&app, 130, 30);
    assert!(s.contains("! Review and Fix are both done by Claude Code"), "solo loses independent review:\n{s}");
}

#[test]
fn w_cycles_the_workflow_preset() {
    let f = fixture();
    let mut app = on_roles(&f);
    press(&mut app, &[KeyCode::Char('w')]);
    let req = run_plan(&mut app);
    assert_eq!(req.workflow, aurora_app::install::WorkflowSource::Preset("refactor".into()));
    assert_eq!(rows(&app).first().map(|s| s.stage.as_str()), Some("baseline"));
    assert!(screen(&app, 130, 30).contains("refactor"));
}

#[test]
fn role_keys_do_nothing_on_other_tabs() {
    let f = fixture();
    let mut app = App::new(f.options());
    run_plan(&mut app);
    press(&mut app, &[KeyCode::Char('p'), KeyCode::Char('w')]);
    assert!(app.take_command().is_none(), "p and w belong to the Roles tab");
}

#[test]
fn quick_presses_build_on_the_previous_press_not_the_last_plan() {
    // Each P or p asks for a new plan; pressing again before it arrives must not repeat the
    // same step (the smoke test caught P P P landing on the wrong profile).
    let f = fixture();
    let mut app = on_roles(&f);
    press(&mut app, &[KeyCode::Char('P'), KeyCode::Char('P')]);
    assert_eq!(app.opts.profile.as_deref(), Some("pair"), "pair -> solo -> pair, with no plan in between");
    let mut app = on_roles(&f);
    app.cursor[Tab::Roles.index()] = 3; // the reviewer slot, on codex
    press(&mut app, &[KeyCode::Char('p'), KeyCode::Char('p')]);
    assert_eq!(app.opts.clis, ["claude", "antigravity"], "codex -> hermes -> antigravity (claude is taken)");
}
