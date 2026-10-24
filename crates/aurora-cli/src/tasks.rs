//! Project-local task listing and recoverable archiving.

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use anstream::{eprintln, println};
use clap::Args;
use serde_json::json;

use crate::style::{BAD, HEAD, OK, paint};

#[derive(Debug, Args)]
pub struct ProjectArgs {
    /// Project directory (default: the current directory)
    #[arg(long, default_value = ".")]
    project: PathBuf,
    /// Output in JSON format
    #[arg(long)]
    json: bool,
    /// List archived tasks instead of active tasks
    #[arg(long)]
    archived: bool,
}

#[derive(Debug, Args)]
pub struct ArchiveArgs {
    /// Task id shown by `aurora task list`
    id: String,
    /// Project directory (default: the current directory)
    #[arg(long, default_value = ".")]
    project: PathBuf,
    /// Confirm archiving without an interactive prompt
    #[arg(long)]
    yes: bool,
}

#[derive(Debug, Args)]
pub struct RestoreArgs {
    /// Task id shown by `aurora task list` before archiving
    id: String,
    /// Project directory (default: the current directory)
    #[arg(long, default_value = ".")]
    project: PathBuf,
}

fn error(message: impl std::fmt::Display) -> u8 {
    eprintln!("{} {message}", paint(BAD, "Error:"));
    1
}

fn resolve_project(project: &Path) -> Result<PathBuf, String> {
    std::fs::canonicalize(project).map_err(|e| format!("cannot resolve project {}: {e}", project.display()))
}

fn status_json(run: &aurora_app::runs::RunSummary) -> serde_json::Value {
    let id = run.dir.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    match &run.status {
        Ok(status) => json!({
            "id": id,
            "workflow": status.workflow.id,
            "complete": status.complete,
            "stages": status.stages.len(),
            "finished_stages": status.stages.iter().filter(|s| {
                matches!(s.state, aurora_core::workflow::StageState::Passed | aurora_core::workflow::StageState::NotApplicable)
            }).count(),
            "status": if status.complete { "finished" } else { "in progress" },
        }),
        Err(reason) => json!({"id": id, "status": "can't be read", "error": reason}),
    }
}

pub fn list(args: &ProjectArgs) -> u8 {
    let project = match resolve_project(&args.project) {
        Ok(project) => project,
        Err(e) => return error(e),
    };
    let root =
        if args.archived { project.join(".aurora/archive/runs") } else { aurora_app::runs::default_run_root(&project) };
    let runs = match aurora_app::runs::list_runs(&root) {
        Ok(runs) => runs,
        Err(e) => return error(e),
    };
    if args.json {
        let rows: Vec<_> = runs.iter().map(status_json).collect();
        println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_default());
        return 0;
    }
    if runs.is_empty() {
        let kind = if args.archived { "archived" } else { "active" };
        println!("No {kind} tasks in {}.", root.display());
        return 0;
    }
    let kind = if args.archived { "Archived" } else { "Active" };
    println!("{}", paint(HEAD, format!("Aurora — {kind} tasks in {}", root.display())));
    println!("{:<24} {:<18} {:<14} STATUS", "ID", "KIND", "PROGRESS");
    for run in &runs {
        let id = run.dir.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        match &run.status {
            Ok(status) => {
                let finished = status
                    .stages
                    .iter()
                    .filter(|s| {
                        matches!(
                            s.state,
                            aurora_core::workflow::StageState::Passed
                                | aurora_core::workflow::StageState::NotApplicable
                        )
                    })
                    .count();
                let state = if status.complete { "finished" } else { "in progress" };
                println!("{id:<24} {:<18} {finished:>2}/{:<11} {state}", status.workflow.id, status.stages.len());
            }
            Err(_) => println!("{id:<24} {:<18} {:<14} can't be read", "?", "?"),
        }
    }
    0
}

fn confirm(id: &str) -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }
    print!("Archive task {id}? Its files will be kept in .aurora/archive/runs/{id}. [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer).is_ok() && matches!(answer.trim(), "y" | "Y" | "yes")
}

pub fn archive(args: &ArchiveArgs) -> u8 {
    let project = match resolve_project(&args.project) {
        Ok(project) => project,
        Err(e) => return error(e),
    };
    let root = aurora_app::runs::default_run_root(&project);
    let runs = match aurora_app::runs::list_runs(&root) {
        Ok(runs) => runs,
        Err(e) => return error(e),
    };
    let Some(run) = runs.iter().find(|run| run.dir.file_name().is_some_and(|name| name.to_string_lossy() == args.id))
    else {
        return error(format!("no active task named '{}' (see `aurora task list`)", args.id));
    };
    if !args.yes && !confirm(&args.id) {
        return error("not archived: confirm in a terminal, or pass --yes");
    }
    match aurora_app::runs::archive_task(&run.dir, &project) {
        Ok(path) => {
            println!("{} task {}. Saved in {}", paint(OK, "Archived"), args.id, path.display());
            0
        }
        Err(e) => error(e),
    }
}

pub fn restore(args: &RestoreArgs) -> u8 {
    let project = match resolve_project(&args.project) {
        Ok(project) => project,
        Err(e) => return error(e),
    };
    match aurora_app::runs::restore_task(&args.id, &project) {
        Ok(path) => {
            println!("{} task {}. Restored to {}", paint(OK, "Restored"), args.id, path.display());
            0
        }
        Err(e) => error(e),
    }
}
