//! R3 Step 1: the TUI skeleton. Tabs, focus, filter, help and quit are pure `App` transitions;
//! rendering is snapshot-tested on ratatui's TestBackend (text only, so colour never matters).

use aurora_tui::app::{App, Focus, Tab, TuiOptions};
use aurora_tui::keymap::{Ctx, bindings, footer};
use aurora_tui::ui::render;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn app() -> App {
    App::new(TuiOptions { project: "/work/demo".into(), no_color: true, ..TuiOptions::default() })
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn press(app: &mut App, codes: &[KeyCode]) {
    for c in codes {
        app.press(key(*c));
    }
}

fn screen(app: &App, w: u16, h: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(w, h)).expect("terminal");
    term.draw(|f| render(f, app)).expect("draw");
    term.backend().to_string()
}

#[test]
fn starts_on_skills_with_header_tabs_and_footer() {
    let a = app();
    assert_eq!(a.tab, Tab::Skills);
    let s = screen(&a, 100, 24);
    for needle in ["Aurora · demo", "1 Skills", "2 Who does what", "3 Add to project", "4 Tasks", "? help", "q quit"] {
        assert!(s.contains(needle), "missing {needle:?}:\n{s}");
    }
    insta::assert_snapshot!("initial", s);
}

#[test]
fn number_keys_and_brackets_switch_tabs() {
    let mut a = app();
    press(&mut a, &[KeyCode::Char('3')]);
    assert_eq!(a.tab, Tab::Install);
    press(&mut a, &[KeyCode::Char(']')]);
    assert_eq!(a.tab, Tab::Runs);
    press(&mut a, &[KeyCode::Char(']')]);
    assert_eq!(a.tab, Tab::Skills, "wraps around");
    press(&mut a, &[KeyCode::Char('[')]);
    assert_eq!(a.tab, Tab::Runs);
}

#[test]
fn tab_moves_focus_between_list_and_detail() {
    let mut a = app();
    assert_eq!(a.focus, Focus::List);
    press(&mut a, &[KeyCode::Tab]);
    assert_eq!(a.focus, Focus::Detail);
    a.press(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(a.focus, Focus::List);
}

#[test]
fn help_overlay_lists_every_binding_and_esc_closes_it() {
    let mut a = app();
    press(&mut a, &[KeyCode::Char('?')]);
    assert!(a.help_open());
    let text: String =
        aurora_tui::ui::help_lines(&a).into_iter().map(|(t, l)| format!("{}{l}\n", t.unwrap_or(""))).collect();
    for b in bindings() {
        assert!(text.contains(b.help), "help is missing {:?}:\n{text}", b.help);
    }
    let s = screen(&a, 100, 40);
    assert!(s.contains("On the Skills page"), "the current tab's keys come first:\n{s}");
    insta::assert_snapshot!("help", s);
    press(&mut a, &[KeyCode::Char('q')]);
    assert!(!a.help_open() && !a.quit, "q closes a popup instead of quitting");
    press(&mut a, &[KeyCode::Char('?'), KeyCode::Esc]);
    assert!(!a.help_open());
}

#[test]
fn filter_edits_live_then_enter_keeps_and_esc_clears() {
    let mut a = app();
    press(&mut a, &[KeyCode::Char('/'), KeyCode::Char('r'), KeyCode::Char('e'), KeyCode::Char('x')]);
    assert!(a.filter_editing());
    press(&mut a, &[KeyCode::Backspace]);
    assert_eq!(a.filter(), "re");
    assert!(!a.quit, "typing q in a filter does not quit");
    press(&mut a, &[KeyCode::Char('q'), KeyCode::Enter]);
    assert!(!a.filter_editing() && a.filter() == "req" && !a.quit);
    assert!(screen(&a, 100, 24).contains("/req"), "the kept filter is visible");
    press(&mut a, &[KeyCode::Esc]);
    assert_eq!(a.filter(), "", "Esc clears a kept filter first");
    press(&mut a, &[KeyCode::Char('/'), KeyCode::Char('z'), KeyCode::Esc]);
    assert!(!a.filter_editing() && a.filter().is_empty());
}

#[test]
fn q_quits_from_the_top_level_and_ctrl_c_always_quits() {
    let mut a = app();
    press(&mut a, &[KeyCode::Char('q')]);
    assert!(a.quit);
    let mut b = app();
    press(&mut b, &[KeyCode::Char('?')]);
    b.press(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(b.quit);
}

#[test]
fn a_small_terminal_shows_a_notice_instead_of_a_broken_layout() {
    let s = screen(&app(), 50, 10);
    assert!(s.contains("too small"), "{s}");
}

#[test]
fn the_footer_is_generated_from_the_keymap() {
    let keys: Vec<&str> = footer(Ctx::Normal).iter().map(|b| b.label).collect();
    assert!(keys.contains(&"? help") && keys.contains(&"q quit") && keys.contains(&"/ search"), "{keys:?}");
    assert!(footer(Ctx::Help).iter().any(|b| b.label == "Esc close"));
}
