//! Workflow runs under a run root, for the TUI's Runs tab: listing, status, acceptance criteria,
//! handoff text, and submit/approve/resume through the evidence-gated engine in `aurora-fs`.

use std::path::{Path, PathBuf};

use aurora_core::workflow::parse_workflow_bytes;
use aurora_core::workflow::record::{RECORD_FILE, WORKFLOW_FILE};
use aurora_fs::engine::{ApproveResult, Engine, StatusReport, SubmitResult};

const MAX_TEXT_BYTES: u64 = 1 << 20;

/// One run directory and its status (or why it could not be read).
#[derive(Debug, Clone)]
pub struct RunSummary {
    pub dir: PathBuf,
    pub status: Result<StatusReport, String>,
}

/// Every run directory (one holding `run.json`) directly under `root`, newest name last.
pub fn list_runs(root: &Path) -> Result<Vec<RunSummary>, String> {
    let entries = std::fs::read_dir(root).map_err(|e| format!("cannot read run root {}: {e}", root.display()))?;
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir()) && p.join(RECORD_FILE).is_file())
        .collect();
    dirs.sort();
    let engine = Engine::host();
    Ok(dirs
        .into_iter()
        .map(|dir| {
            let status = engine.status(&dir).map_err(|e| e.to_string());
            RunSummary { dir, status }
        })
        .collect())
}

/// A stage's acceptance criteria from the run's pinned workflow: (id, description).
pub fn criteria(run_dir: &Path, stage: &str) -> Result<Vec<(String, String)>, String> {
    let path = run_dir.join(WORKFLOW_FILE);
    let info = aurora_fs::read::safe_read(&path, &[run_dir], MAX_TEXT_BYTES)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let wf = parse_workflow_bytes(&info.content, None, &path.display().to_string()).map_err(|e| e.to_string())?;
    let s = wf.stage(stage).ok_or_else(|| format!("no stage '{stage}' in {}", path.display()))?;
    Ok(s.acceptance.iter().map(|c| (c.id.clone(), c.description.clone())).collect())
}

/// The text of a run-relative file (a handoff bundle), read without following symlinks.
pub fn read_run_file(run_dir: &Path, rel: &str) -> Result<String, String> {
    let path = run_dir.join(rel);
    let info = aurora_fs::read::safe_read(&path, &[run_dir], MAX_TEXT_BYTES)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok(String::from_utf8_lossy(&info.content).into_owned())
}

pub fn submit(run_dir: &Path, stage: &str, met: &[String]) -> Result<SubmitResult, String> {
    Engine::host().submit(run_dir, stage, met, false, None).map_err(|e| e.to_string())
}

pub fn approve(run_dir: &Path, stage: &str, by: &str) -> Result<ApproveResult, String> {
    Engine::host().approve(run_dir, stage, by, None).map_err(|e| e.to_string())
}

pub fn resume(run_dir: &Path) -> Result<StatusReport, String> {
    Engine::host().resume(run_dir, None).map_err(|e| e.to_string())
}
