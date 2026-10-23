//! R3 Step 2: the Skills tab. A skills × CLIs matrix from the real install plan, a detail pane
//! with provenance and the equivalent CLI command, and opt-in toggles that re-plan.

mod common;

use aurora_tui::app::{App, Command, Msg};
use aurora_tui::screens::skills::{Cell, rows};
use common::{fixture, press, screen};
use ratatui::crossterm::event::KeyCode;

fn loaded(f: &common::Fixture) -> App {
    let mut app = App::new(f.options());
    let Some(Command::Plan { generation, request }) = app.take_command() else { panic!("startup queues a plan") };
    assert_eq!(request, f.request(&[]));
    app.receive(Msg::Planned { generation, result: aurora_app::install::plan_install(&request) });
    app
}

#[test]
fn startup_queues_a_plan_and_shows_progress() {
    let f = fixture();
    let mut app = App::new(f.options());
    assert!(matches!(app.take_command(), Some(Command::Plan { .. })));
    assert!(app.take_command().is_none(), "one plan at a time");
    assert!(screen(&app, 110, 24).contains("planning"), "a spinner line while the plan runs");
}

#[test]
fn the_matrix_shows_each_skill_per_cli() {
    let f = fixture();
    let app = loaded(&f);
    let r = rows(&app);
    let cell = |id: &str, cli: usize| r.iter().find(|x| x.id == id).unwrap_or_else(|| panic!("row {id}")).cells[cli];
    // pair: column 0 claude, column 1 codex
    assert_eq!((cell("demo", 0), cell("demo", 1)), (Cell::Create, Cell::Create), "baseline goes to both");
    assert_eq!((cell("checker", 0), cell("checker", 1)), (Cell::NotWanted, Cell::Create), "reviewer runs on codex");
    assert_eq!(cell("scripted", 0), Cell::CreateAfterReview, "a bundled script needs review");
    assert_eq!(
        (cell("house-rules", 0), cell("house-rules", 1)),
        (Cell::Create, Cell::Create),
        "rules into CLAUDE.md and AGENTS.md"
    );
    assert_eq!((cell("extra", 0), cell("extra", 1)), (Cell::NotWanted, Cell::NotWanted), "opt-in is off by default");
    let s = screen(&app, 110, 24);
    for needle in ["claude", "codex", "demo", "checker", "budget"] {
        assert!(s.contains(needle), "missing {needle}:\n{s}");
    }
    snapshot!("matrix", s);
}

#[test]
fn the_detail_pane_explains_the_selected_skill() {
    let f = fixture();
    let mut app = loaded(&f);
    let at = rows(&app).iter().position(|r| r.id == "checker").expect("checker");
    app.cursor[0] = at;
    let s = screen(&app, 130, 30);
    for needle in [
        "checker",
        "skill · stage · curated",
        "Why checker.",
        "allowed-tools removed",
        "codex",
        ".agents/skills/checker",
        "aurora catalog show checker",
    ] {
        assert!(s.contains(needle), "missing {needle}:\n{s}");
    }
}

#[test]
fn filtering_narrows_the_rows() {
    let f = fixture();
    let mut app = loaded(&f);
    press(&mut app, &[KeyCode::Char('/'), KeyCode::Char('c'), KeyCode::Char('h'), KeyCode::Char('e')]);
    let ids: Vec<String> = rows(&app).into_iter().map(|r| r.id).collect();
    assert_eq!(ids, ["checker"]);
}

#[test]
fn toggling_an_opt_in_entry_replans_with_it() {
    let f = fixture();
    let mut app = loaded(&f);
    app.cursor[0] = rows(&app).iter().position(|r| r.id == "extra").expect("extra");
    press(&mut app, &[KeyCode::Char('i')]);
    let Some(Command::Plan { generation, request }) = app.take_command() else { panic!("re-plan") };
    assert_eq!(request.with, ["extra"]);
    app.receive(Msg::Planned { generation, result: aurora_app::install::plan_install(&request) });
    let extra = rows(&app).into_iter().find(|r| r.id == "extra").expect("extra");
    assert_eq!(extra.cells[1], Cell::Create, "the tester role runs on codex");

    // A stale result from an older request is ignored.
    press(&mut app, &[KeyCode::Char('i')]);
    let Some(Command::Plan { generation: newer, request }) = app.take_command() else { panic!("re-plan") };
    assert!(request.with.is_empty());
    app.receive(Msg::Planned { generation: newer - 1, result: Err("stale".into()) });
    assert!(!screen(&app, 110, 24).contains("stale"));
}

#[test]
fn toggling_a_non_opt_in_row_does_nothing() {
    let f = fixture();
    let mut app = loaded(&f);
    app.cursor[0] = rows(&app).iter().position(|r| r.id == "demo").expect("demo");
    press(&mut app, &[KeyCode::Char('i')]);
    assert!(app.take_command().is_none());
}

#[test]
fn foreign_skills_in_the_project_show_as_foreign() {
    let f = fixture();
    let dir = f.project.join(".claude/skills/demo");
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join("SKILL.md"), "---\nname: demo\ndescription: mine\n---\n").expect("write");
    let app = loaded(&f);
    let demo = rows(&app).into_iter().find(|r| r.id == "demo").expect("demo");
    assert_eq!(demo.cells[0], Cell::Foreign);
}

#[test]
fn a_plan_error_is_shown_with_a_retry_key() {
    let f = fixture();
    let mut app = App::new(f.options());
    let Some(Command::Plan { generation, .. }) = app.take_command() else { panic!("plan") };
    app.receive(Msg::Planned { generation, result: Err("cannot fetch https://github.com/x: offline".into()) });
    let s = screen(&app, 110, 24);
    assert!(s.contains("cannot fetch") && s.contains("r retry"), "{s}");
    press(&mut app, &[KeyCode::Char('r')]);
    assert!(matches!(app.take_command(), Some(Command::Plan { .. })));
}

#[test]
fn bottom_clamps_to_the_last_row() {
    let f = fixture();
    let mut app = loaded(&f);
    press(&mut app, &[KeyCode::Char('G')]);
    assert_eq!(app.cursor[0], rows(&app).len() - 1);
}

#[test]
fn skills_in_a_shared_directory_show_as_leaks() {
    // codex builds and antigravity reviews: the reviewer-only `checker` lands in the shared
    // .agents/skills, where codex loads it too.
    let f = fixture();
    let opts = aurora_tui::app::TuiOptions { clis: vec!["codex".into(), "antigravity".into()], ..f.options() };
    let mut app = App::new(opts);
    let Some(Command::Plan { generation, request }) = app.take_command() else { panic!("plan") };
    app.receive(Msg::Planned { generation, result: aurora_app::install::plan_install(&request) });
    let checker = rows(&app).into_iter().find(|r| r.id == "checker").expect("checker");
    assert_eq!(checker.cells, [Cell::Leak, Cell::Create]);
}
