//! Aurora's terminal UI (R3). A front end over the tested engines: every action maps to an
//! `aurora` CLI command, and nothing here mutates a project except through `aurora-app`.
//!
//! Design: `docs/design/tui-vision.md`. Structure (gitui/serie pattern): one [`app::App`] model,
//! one [`app::Action`] enum with pure `update` transitions, a single key table
//! ([`keymap`]) that also generates the footer and help, and [`ui::render`] as a pure function
//! of the model.

pub mod app;
pub mod keymap;
pub mod ui;

use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyEventKind};

pub use app::TuiOptions;

/// Run the TUI until the user quits. The terminal is restored on return and on panic.
pub fn run(mut opts: TuiOptions) -> std::io::Result<()> {
    if let Ok(abs) = std::fs::canonicalize(&opts.project) {
        opts.project = abs;
    }
    let mut terminal = ratatui::init();
    let result = (|| -> std::io::Result<()> {
        let mut app = app::App::new(opts);
        while !app.quit {
            terminal.draw(|f| ui::render(f, &app))?;
            if event::poll(Duration::from_millis(100))?
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                app.press(key);
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}
