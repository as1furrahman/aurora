//! R3 Step 6: mouse support and themes. Mouse handling is a pure transition like keys.

mod common;

use aurora_tui::app::{App, Focus, Tab, TuiOptions};
use aurora_tui::ui::Theme;
use common::{fixture, press, screen};
use ratatui::crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::Color;

const AREA: Rect = Rect { x: 0, y: 0, width: 120, height: 30 };

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE }
}

fn click(app: &mut App, column: u16, row: u16) {
    app.mouse(mouse(MouseEventKind::Down(MouseButton::Left), column, row), AREA);
}

fn loaded(f: &common::Fixture) -> App {
    let mut app = App::new(f.options());
    while let Some(cmd) = app.take_command() {
        app.receive(aurora_tui::execute(cmd));
    }
    app
}

#[test]
fn clicking_a_tab_title_switches_tab() {
    let f = fixture();
    let mut app = loaded(&f);
    let s = screen(&app, AREA.width, AREA.height);
    let tabs_line = s.lines().nth(1).expect("tab row");
    for (title, tab) in [
        ("2 Who does what", Tab::Roles),
        ("4 Tasks", Tab::Runs),
        ("3 Add to project", Tab::Install),
        ("1 Skills", Tab::Skills),
    ] {
        let col = tabs_line.find(title).expect("title on screen");
        // Column in characters, not bytes: the divider is a multi-byte glyph.
        let col = tabs_line[..col].chars().count() as u16 - 1;
        click(&mut app, col + 2, 1);
        assert_eq!(app.tab, tab, "clicking {title}");
    }
}

#[test]
fn the_wheel_moves_the_selection_and_scrolls_popups() {
    let f = fixture();
    let mut app = loaded(&f);
    app.mouse(mouse(MouseEventKind::ScrollDown, 10, 10), AREA);
    app.mouse(mouse(MouseEventKind::ScrollDown, 10, 10), AREA);
    assert_eq!(app.cursor[0], 2);
    app.mouse(mouse(MouseEventKind::ScrollUp, 10, 10), AREA);
    assert_eq!(app.cursor[0], 1);
    press(&mut app, &[KeyCode::Char('?')]);
    app.mouse(mouse(MouseEventKind::ScrollDown, 10, 10), AREA);
    assert_eq!(app.help_scroll(), 1, "the wheel scrolls the open popup, not the list");
    assert_eq!(app.cursor[0], 1);
}

#[test]
fn clicking_a_pane_focuses_it_and_a_row_selects_it() {
    let f = fixture();
    let mut app = loaded(&f);
    click(&mut app, 100, 10);
    assert_eq!(app.focus, Focus::Detail);
    // Body starts at row 2; inside the border the table header is row 3, data from row 4.
    click(&mut app, 10, 6);
    assert_eq!(app.focus, Focus::List);
    assert_eq!(app.cursor[0], 2, "the third data row");
}

#[test]
fn clicks_are_ignored_while_a_popup_is_open() {
    let f = fixture();
    let mut app = loaded(&f);
    press(&mut app, &[KeyCode::Char('?')]);
    click(&mut app, 30, 1);
    assert_eq!(app.tab, Tab::Skills);
    assert!(app.help_open());
}

#[test]
fn high_contrast_and_no_color_themes() {
    let hc = Theme { no_color: false, high_contrast: true };
    assert_eq!(hc.accent().fg, Some(Color::Yellow));
    let plain = Theme { no_color: true, high_contrast: false };
    assert_eq!(plain.accent().fg, None, "NO_COLOR keeps emphasis but drops colour");
    let opts = TuiOptions { high_contrast: true, ..TuiOptions::default() };
    assert!(opts.high_contrast);
}
