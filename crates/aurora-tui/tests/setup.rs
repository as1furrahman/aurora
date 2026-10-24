//! R3.1 Step 1: the guided setup. On first launch in a project (no aurora.lock) Aurora asks which
//! AI tools you use, what you're working on, and shows what it will add. Detection is injected.

mod common;

use aurora_tui::app::{App, Command, Popup, Tab, TuiOptions};
use aurora_tui::setup::Step;
use common::{assert_plain, fixture, key, press, screen};
use ratatui::crossterm::event::KeyCode;

fn first_launch(f: &common::Fixture, installed: &[&str]) -> App {
    let opts = TuiOptions { setup: true, installed: installed.iter().map(|s| s.to_string()).collect(), ..f.options() };
    App::new(opts)
}

fn step(app: &App) -> Option<Step> {
    match app.popup() {
        Some(Popup::Setup(s)) => Some(s.step),
        _ => None,
    }
}

fn ticked(app: &App) -> Vec<String> {
    match app.popup() {
        Some(Popup::Setup(s)) => s.chosen(),
        _ => Vec::new(),
    }
}

/// Run queued work until the plan arrives.
fn settle(app: &mut App) {
    while let Some(cmd) = app.take_command() {
        app.receive(aurora_tui::execute(cmd));
    }
}

#[test]
fn first_launch_asks_which_tools_with_installed_ones_ticked() {
    let f = fixture();
    let app = first_launch(&f, &["claude", "hermes"]);
    assert_eq!(step(&app), Some(Step::Tools));
    assert_eq!(ticked(&app), ["claude", "hermes"]);
    let s = screen(&app, 100, 30);
    for needle in
        ["Which AI tools do you use?", "[x] Claude Code", "[ ] Codex", "[x] Hermes", "(installed)", "step 1 of 3"]
    {
        assert!(s.contains(needle), "missing {needle:?}:\n{s}");
    }
}

#[test]
fn without_setup_the_app_opens_straight_on_the_pages() {
    let f = fixture();
    assert_eq!(step(&App::new(f.options())), None, "tests and returning users skip it");
}

#[test]
fn nothing_installed_means_nothing_ticked_and_a_friendly_note() {
    let f = fixture();
    let app = first_launch(&f, &[]);
    assert!(ticked(&app).is_empty());
    assert!(screen(&app, 100, 30).contains("None of these were found"), "{}", screen(&app, 100, 30));
}

#[test]
fn space_ticks_and_enter_wants_one_to_three_tools() {
    let f = fixture();
    let mut app = first_launch(&f, &[]);
    press(&mut app, &[KeyCode::Enter]);
    assert_eq!(step(&app), Some(Step::Tools), "zero tools: stay");
    assert!(screen(&app, 100, 30).contains("Pick at least one"));
    for _ in 0..4 {
        press(&mut app, &[KeyCode::Char(' '), KeyCode::Down]);
    }
    assert_eq!(ticked(&app).len(), 4);
    press(&mut app, &[KeyCode::Enter]);
    assert_eq!(step(&app), Some(Step::Tools), "four tools: stay");
    assert!(screen(&app, 100, 30).contains("up to 3"));
    press(&mut app, &[KeyCode::Up, KeyCode::Char(' '), KeyCode::Enter]);
    assert_eq!(step(&app), Some(Step::Task));
}

#[test]
fn the_task_type_and_tools_become_the_plan() {
    let f = fixture();
    let mut app = first_launch(&f, &["claude", "codex"]);
    settle(&mut app);
    press(&mut app, &[KeyCode::Enter]);
    let s = screen(&app, 100, 30);
    for needle in ["What are you working on?", "Fix a bug", "Build a new feature", "Explore an idea", "step 2 of 3"] {
        assert!(s.contains(needle), "missing {needle:?}:\n{s}");
    }
    press(&mut app, &[KeyCode::Down, KeyCode::Enter]);
    assert_eq!(step(&app), Some(Step::Preview));
    assert_eq!(app.opts.preset, "feature");
    assert_eq!(app.opts.clis, ["claude", "codex"]);
    assert_eq!(app.opts.profile, None, "the profile follows the number of tools");
    let Some(Command::Plan { request, .. }) = app.take_command() else { panic!("a new plan") };
    assert_eq!(request.clis, ["claude", "codex"]);
}

#[test]
fn the_preview_says_what_will_be_added_and_enter_opens_add_to_project() {
    let f = fixture();
    let mut app = first_launch(&f, &["claude", "codex"]);
    press(&mut app, &[KeyCode::Enter, KeyCode::Enter]);
    settle(&mut app);
    let s = screen(&app, 110, 32);
    for needle in [
        "Here's what Aurora will add",
        "Claude Code",
        "Codex",
        "demo",
        "CLAUDE.md",
        "Check first",
        "scripted",
        "step 3 of 3",
    ] {
        assert!(s.contains(needle), "missing {needle:?}:\n{s}");
    }
    press(&mut app, &[KeyCode::Enter]);
    assert_eq!(step(&app), None, "setup closes");
    assert_eq!(app.tab, Tab::Install, "straight to the review and add page");
    assert!(!f.project.join("CLAUDE.md").exists(), "nothing is written without a yes");
}

#[test]
fn backspace_goes_back_and_esc_means_not_now() {
    let f = fixture();
    let mut app = first_launch(&f, &["claude"]);
    press(&mut app, &[KeyCode::Enter]);
    assert_eq!(step(&app), Some(Step::Task));
    press(&mut app, &[KeyCode::Backspace]);
    assert_eq!(step(&app), Some(Step::Tools));
    app.press(key(KeyCode::Esc));
    assert_eq!(step(&app), None, "not now");
    assert_eq!(app.tab, Tab::Skills);
}

#[test]
fn t_reopens_setup_with_the_current_tools() {
    let f = fixture();
    let mut app = App::new(TuiOptions { installed: vec!["claude".into()], ..f.options() });
    settle(&mut app);
    press(&mut app, &[KeyCode::Char('t')]);
    assert_eq!(step(&app), Some(Step::Tools));
    assert_eq!(ticked(&app), ["claude", "codex"], "the tools in use, not just the installed ones");
}

#[test]
fn the_three_setup_screens() {
    let f = fixture();
    let mut app = first_launch(&f, &["claude", "codex"]);
    settle(&mut app);
    assert_plain(&screen(&app, 100, 30), "setup step 1");
    snapshot!("setup_tools", screen(&app, 100, 30));
    press(&mut app, &[KeyCode::Enter]);
    assert_plain(&screen(&app, 100, 30), "setup step 2");
    snapshot!("setup_task", screen(&app, 100, 30));
    press(&mut app, &[KeyCode::Enter]);
    settle(&mut app);
    assert_plain(&screen(&app, 100, 30), "setup step 3");
    snapshot!("setup_preview", screen(&app, 100, 30));
}
