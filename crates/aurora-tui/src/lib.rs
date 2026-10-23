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
pub mod words;

use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind};

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
                return Err(format!("please check {} first", unreviewed.join(", ")));
            }
            let id = new_op_id();
            apply_planned(&planned, &id).map_err(|e| e.to_string())?;
            Ok(format!("Added {} things to your project. Press u to undo.", planned.change_count()))
        })()),
        Command::Undo { project } => Msg::Undone(undo_project(&project, None)),
        Command::LoadRuns { root } => Msg::Runs(aurora_app::runs::list_runs(&root)),
        Command::ReadText { title, run_dir, rel } => {
            Msg::Text { title, result: aurora_app::runs::read_run_file(&run_dir, &rel) }
        }
        Command::LoadCriteria { run_dir, stage } => {
            let result = aurora_app::runs::criteria(&run_dir, &stage);
            Msg::Criteria { run_dir, stage, result }
        }
        Command::Submit { run_dir, stage, met } => {
            let result = aurora_app::runs::submit(&run_dir, &stage, &met);
            Msg::Submitted { stage, result }
        }
        Command::Approve { run_dir, stage, by } => {
            let result = aurora_app::runs::approve(&run_dir, &stage, &by);
            Msg::Approved { stage, result }
        }
        Command::Resume { run_dir } => Msg::Resumed(Box::new(aurora_app::runs::resume(&run_dir))),
        // The event loop writes the clipboard sequence itself; here there is no terminal.
        Command::Copy { text } => Msg::Notice(format!("Copied: {text}")),
    }
}

/// Base64 for the OSC 52 clipboard sequence.
fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(A[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
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
    // Mouse: wheel, tab and row clicks. Hold Shift to select text in most terminals.
    let _ = ratatui::crossterm::execute!(std::io::stdout(), EnableMouseCapture);
    let result = (|| -> std::io::Result<()> {
        let mut app = app::App::new(opts);
        let (tx, rx) = channel();
        while !app.quit {
            while let Some(cmd) = app.take_command() {
                if let Command::Copy { text } = &cmd {
                    // OSC 52: the terminal (or tmux, ssh) puts the text on the clipboard.
                    use std::io::Write;
                    let mut out = std::io::stdout();
                    let _ = write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes()));
                    let _ = out.flush();
                }
                spawn(cmd, tx.clone());
            }
            while let Ok(msg) = rx.try_recv() {
                app.receive(msg);
            }
            app.tick();
            terminal.draw(|f| ui::render(f, &app))?;
            if event::poll(Duration::from_millis(100))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => app.press(key),
                    Event::Mouse(m) => {
                        let size = terminal.size()?;
                        app.mouse(m, ratatui::layout::Rect::new(0, 0, size.width, size.height));
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    })();
    let _ = ratatui::crossterm::execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}
