//! Run directory and run-record persistence (ADR-0003; port of `awb/workflow/runs.py`).
//!
//! Every write is boundary-checked against the run directory and staged atomically; records
//! are read through the no-follow reader; record updates happen under the run's
//! `.awb.lock`. Layout: `<run-root>/<run-id>/{run.json, workflow.toml, handoff/, artifacts/}`.

use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

use aurora_core::workflow::record::{RECORD_FILE, RunRecord};

use crate::FsError;
use crate::atomic::atomic_write;
use crate::boundaries::{BoundaryPolicy, validate_security_preconditions};
use crate::lock::{DEFAULT_TIMEOUT, ProcessLock};
use crate::read::{ReadInfo, safe_read};

pub const LOCK_FILE: &str = ".awb.lock";
pub const RECORD_MAX_BYTES: u64 = 8 * 1024 * 1024;
pub const EVIDENCE_MAX_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// Invalid run operation (bad run root/id, wrong stage state, usage error).
    #[error("{0}")]
    Run(String),
    /// `run.json` is missing, unreadable, or violates the v1 schema.
    #[error("{0}")]
    Record(String),
    /// The pinned workflow or accepted evidence no longer matches its recorded hash.
    #[error("{0}")]
    Integrity(String),
    #[error(transparent)]
    Fs(#[from] FsError),
}

/// `^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$`, whole string, and not `.`/`..`.
pub fn validate_run_id(run_id: &str) -> Result<(), RunError> {
    let bytes = run_id.as_bytes();
    let ok = !bytes.is_empty()
        && bytes.len() <= 128
        && bytes[0].is_ascii_alphanumeric()
        && bytes.iter().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if !ok || run_id == "." || run_id == ".." {
        return Err(RunError::Run(format!(
            "Invalid run id '{run_id}': use letters, digits, '.', '_' or '-', starting alphanumeric"
        )));
    }
    Ok(())
}

/// Boundary-checked run-directory store.
#[derive(Debug, Clone)]
pub struct RunStore {
    pub policy: BoundaryPolicy,
}

impl RunStore {
    pub fn host() -> Self {
        RunStore { policy: BoundaryPolicy::host() }
    }

    pub fn with_policy(policy: BoundaryPolicy) -> Self {
        RunStore { policy }
    }

    fn check(&self, target: &Path, root: &Path) -> Result<PathBuf, RunError> {
        Ok(validate_security_preconditions(target, &[root], false, &self.policy)?)
    }

    /// Create `<run_root>/<run_id>` (mode 0700). The root must already exist; an existing run
    /// directory is never reused.
    pub fn create_run_dir(&self, run_root: &Path, run_id: &str) -> Result<PathBuf, RunError> {
        validate_run_id(run_id)?;
        if !run_root.is_dir() {
            return Err(RunError::Run(format!(
                "Run root '{}' does not exist or is not a directory; create it explicitly first",
                run_root.display()
            )));
        }
        let root = run_root.canonicalize().map_err(|e| FsError::io(format!("resolve '{}'", run_root.display()), e))?;
        let run_dir = root.join(run_id);
        self.check(&run_dir, &root)?;
        match DirBuilder::new().mode(0o700).create(&run_dir) {
            Ok(()) => Ok(run_dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(RunError::Run(format!(
                "Run directory '{}' already exists; refusing to reuse it",
                run_dir.display()
            ))),
            Err(e) => Err(FsError::io(format!("create '{}'", run_dir.display()), e).into()),
        }
    }

    /// Create a directory inside `run_dir`, boundary-checking each level.
    pub fn ensure_dir(&self, run_dir: &Path, rel: &str) -> Result<PathBuf, RunError> {
        let mut path = run_dir.to_path_buf();
        for part in Path::new(rel).components() {
            path.push(part);
            self.check(&path, run_dir)?;
            if !path.is_dir() {
                DirBuilder::new()
                    .mode(0o700)
                    .create(&path)
                    .map_err(|e| FsError::io(format!("create '{}'", path.display()), e))?;
            }
        }
        Ok(path)
    }

    pub fn write_file(&self, run_dir: &Path, rel: &str, content: &[u8]) -> Result<(), RunError> {
        let dest = run_dir.join(rel);
        self.check(&dest, run_dir)?;
        atomic_write(&dest, content, &[run_dir], 0o600)?;
        Ok(())
    }

    pub fn read_file(&self, run_dir: &Path, rel: &str, max_size: u64) -> Result<ReadInfo, RunError> {
        Ok(safe_read(&run_dir.join(rel), &[run_dir], max_size)?)
    }

    pub fn lock(&self, run_dir: &Path) -> Result<ProcessLock, RunError> {
        Ok(ProcessLock::acquire(&run_dir.join(LOCK_FILE), &[run_dir], DEFAULT_TIMEOUT)?)
    }

    pub fn save_record(&self, run_dir: &Path, record: &RunRecord) -> Result<(), RunError> {
        self.write_file(run_dir, RECORD_FILE, record.to_json().as_bytes())
    }

    pub fn load_record(&self, run_dir: &Path) -> Result<RunRecord, RunError> {
        let run_dir = run_dir.canonicalize().unwrap_or_else(|_| run_dir.to_path_buf());
        if !run_dir.join(RECORD_FILE).is_file() {
            return Err(RunError::Record(format!(
                "'{}' is not an Aurora run directory (missing {RECORD_FILE})",
                run_dir.display()
            )));
        }
        let info = self.read_file(&run_dir, RECORD_FILE, RECORD_MAX_BYTES)?;
        RunRecord::from_json(&info.content).map_err(|e| RunError::Record(e.to_string()))
    }
}
