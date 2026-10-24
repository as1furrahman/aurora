//! R3.1: the TUI speaks plainly. Engine words (CLI, provider, stage, handoff, overlay, ...) may
//! appear only under a "More details" heading, never in the lists, headers, footers or the top of
//! a details pane.

mod common;

use aurora_tui::app::App;
use common::{assert_plain, fixture, press, screen};
use ratatui::crossterm::event::KeyCode;

fn loaded(f: &common::Fixture) -> App {
    let mut app = App::new(f.options());
    while let Some(cmd) = app.take_command() {
        app.receive(aurora_tui::execute(cmd));
    }
    app
}

#[test]
fn every_page_and_details_pane_speaks_plainly() {
    let f = fixture();
    let mut app = loaded(&f);
    for (tab, name) in [('1', "Skills"), ('2', "Who does what"), ('3', "Add to project"), ('4', "Tasks")] {
        press(&mut app, &[KeyCode::Char(tab)]);
        for row in 0..8 {
            assert_plain(&screen(&app, 130, 34), &format!("{name} page, row {row}"));
            press(&mut app, &[KeyCode::Char('j')]);
        }
    }
}

#[test]
fn help_and_dialogs_speak_plainly() {
    let f = fixture();
    let mut app = loaded(&f);
    press(&mut app, &[KeyCode::Char('?')]);
    assert_plain(&screen(&app, 110, 60), "help");
    press(&mut app, &[KeyCode::Esc, KeyCode::Char('3'), KeyCode::Char('u')]);
    assert_plain(&screen(&app, 110, 30), "undo question");
}

#[test]
fn footer_labels_are_a_key_and_a_plain_verb() {
    for b in aurora_tui::keymap::bindings() {
        let (key, words) = b.label.split_once(' ').expect("key then words");
        let words = words.trim_start_matches(|c: char| !c.is_alphabetic()); // "[ ] pages": the key is "[ ]"
        assert!(!key.is_empty() && !words.is_empty(), "{:?}", b.label);
        assert!(
            words.chars().all(|c| c.is_lowercase() || " /-".contains(c) || c == 'O' || c == 'K'),
            "plain lower-case words: {:?}",
            b.label
        );
    }
}
