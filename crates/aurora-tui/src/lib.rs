//! Aurora's terminal UI (R3). A front end over the tested engines: every action maps to an
//! `aurora` CLI command, and nothing here mutates a project except through `aurora-app`.
//!
//! Design: `docs/design/tui-vision.md`. Structure (gitui/serie pattern): one [`app::App`] model,
//! one [`app::Action`] enum with pure `update` transitions, a single key table
//! ([`keymap`]) that also generates the footer and help, and [`ui::render`] as a pure function
//! of the model.

pub mod app;
pub mod keymap;
pub mod screens;
pub mod ui;

use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyEventKind};

use crate::app::{Command, Msg};

/// Do the work of `cmd` (blocking) and describe the outcome. Apply re-checks the project
/// boundary and the review gate, as `aurora skills apply` does, before touching anything.
pub fn execute(cmd: Command) -> Msg {
    use aurora_app::install::{apply_planned, check_project, new_op_id, plan_install, undo_project};
    match cmd {
        Command::Plan { generation, request } => Msg::Planned { generation, result: Box::new(plan_install(&request)) },
        Command::Apply { planned, reviewed } => Msg::Applied((|| {
            check_project(&planned.project)?;
            let unreviewed = planned.unreviewed(&reviewed);
            if !unreviewed.is_empty() {
                return Err(format!("{} still need review", unreviewed.join(", ")));
            }
            let id = new_op_id();
            apply_planned(&planned, &id).map_err(|e| e.to_string())?;
            Ok(format!("Applied {} changes (operation {id}). u undoes.", planned.change_count()))
        })()),
        Command::Undo { project } => Msg::Undone(undo_project(&project, None)),
    }
}

/// Run `cmd` on a worker thread; its result comes back over `tx`.
fn spawn(cmd: Command, tx: Sender<Msg>) {
    std::thread::spawn(move || {
        // The UI may have quit; nothing to report to.
        let _ = tx.send(execute(cmd));
    });
}

pub use app::TuiOptions;

/// Run the TUI until the user quits. The terminal is restored on return and on panic.
pub fn run(mut opts: TuiOptions) -> std::io::Result<()> {
    if let Ok(abs) = std::fs::canonicalize(&opts.project) {
        opts.project = abs;
    }
    let mut terminal = ratatui::init();
    let result = (|| -> std::io::Result<()> {
        let mut app = app::App::new(opts);
        let (tx, rx) = channel();
        while !app.quit {
            while let Some(cmd) = app.take_command() {
                spawn(cmd, tx.clone());
            }
            while let Ok(msg) = rx.try_recv() {
                app.receive(msg);
            }
            app.tick();
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
