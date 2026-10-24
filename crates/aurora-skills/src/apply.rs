//! Applying an install plan, and undoing it (R2 Step 4b).
//!
//! - Write-ahead journal: `.aurora/ops/<id>/op.json` is written before the first change and
//!   after every step, so an interrupted operation is visible and blocks further applies until
//!   `undo` recovers it.
//! - Nothing is deleted: a replaced or pruned copy is moved into `.aurora/ops/<id>/backup/`,
//!   and undo moves what it takes away into `.aurora/ops/<id>/undone/`.
//! - Every step re-checks its precondition (destination absent, or still the expected hash)
//!   immediately before acting; a failure rolls back the steps already done.
//! - Undo reverses a step only while the destination still holds what the operation wrote;
//!   anything changed since is skipped and reported.
//! - All writes go through `aurora-fs` (fd-relative, no-follow); applies are serialised with the
//!   persistent `ProcessLock`.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use aurora_fs::FsError;
use aurora_fs::atomic::atomic_write;
use aurora_fs::dirs::{Entry, NewFile, Root};
use aurora_fs::lock::ProcessLock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::plan::{ActionKind, InstallPlan, render_rules_file};
use crate::tree::{Limits, SkillTree, collect};

pub const OPS_DIR: &str = ".aurora/ops";
const LOCK_PATH: &str = ".aurora/lock";
const LOCK_FILE: &str = "aurora.lock";
const OP_VERSION: u32 = 1;
const MAX_TEXT_BYTES: u64 = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpStatus {
    Prepared,
    Committed,
    RolledBack,
    Undone,
}

/// One reversible change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Step {
    /// Create a skill directory.
    Place { target: String, entry: String, installed_sha256: String },
    /// Move the old copy to `backup`, then place the new one.
    Replace { target: String, entry: String, old_sha256: String, installed_sha256: String, backup: String },
    /// Move the copy to `backup`.
    Remove { target: String, sha256: String, backup: String },
    /// Write a file (rules file or lock); its previous content, if any, is kept in `backup`.
    WriteFile { target: String, old_sha256: Option<String>, new_sha256: String, backup: Option<String> },
}

impl Step {
    pub fn target(&self) -> &str {
        match self {
            Step::Place { target, .. }
            | Step::Replace { target, .. }
            | Step::Remove { target, .. }
            | Step::WriteFile { target, .. } => target,
        }
    }
}

/// The journal of one operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpRecord {
    pub version: u32,
    pub id: String,
    pub status: OpStatus,
    pub steps: Vec<Step>,
    /// Steps completed.
    pub done: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    #[error("{0}")]
    Fs(#[from] FsError),
    #[error("operation {0} did not finish; run `aurora skills undo` to recover before applying again")]
    Pending(String),
    #[error("operation {0} was already undone or rolled back")]
    AlreadyUndone(String),
    #[error("no operation to undo")]
    NoOperation,
    #[error("'{target}' {reason}; nothing was changed (steps already done were rolled back)")]
    Precondition { target: String, reason: String },
    #[error("injected fault before step {0}")]
    InjectedFault(usize),
    #[error("{0}")]
    Journal(String),
}

#[derive(Debug)]
pub struct ApplyInput<'a> {
    /// Canonical project directory.
    pub project: &'a Path,
    pub plan: &'a InstallPlan,
    /// Entry -> the tree to install (overlays applied).
    pub contents: &'a BTreeMap<String, SkillTree>,
    /// Operation id: letters, digits, `-` and `_`.
    pub op_id: &'a str,
    /// Test seam: stop as if the process died before this step.
    pub fault_before_step: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UndoReport {
    pub op_id: String,
    pub restored: Vec<String>,
    pub skipped: Vec<String>,
}

fn sha_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

struct Ctx<'a> {
    root: Root,
    project: &'a Path,
}

impl Ctx<'_> {
    fn abs(&self, rel: &str) -> std::path::PathBuf {
        self.project.join(rel)
    }

    fn dir_hash(&self, rel: &str) -> Result<Option<String>, ApplyError> {
        Ok(match self.root.entry(rel)? {
            Entry::Absent => None,
            Entry::Dir => Some(
                collect(&self.abs(rel), &Limits::default())
                    .map_err(|e| ApplyError::Journal(e.to_string()))?
                    .content_hash(),
            ),
            _ => Some("not-a-directory".into()),
        })
    }

    fn read_file(&self, rel: &str) -> Result<Option<Vec<u8>>, ApplyError> {
        match self.root.entry(rel)? {
            Entry::Absent => Ok(None),
            Entry::File => {
                Ok(Some(aurora_fs::read::safe_read(&self.abs(rel), &[self.project], MAX_TEXT_BYTES)?.content))
            }
            _ => Err(ApplyError::Precondition { target: rel.to_string(), reason: "is not a regular file".into() }),
        }
    }

    fn write_file(&self, rel: &str, content: &[u8], mode: u32) -> Result<(), ApplyError> {
        atomic_write(&self.abs(rel), content, &[self.project], mode)?;
        Ok(())
    }

    fn save(&self, record: &OpRecord) -> Result<(), ApplyError> {
        let text = serde_json::to_string_pretty(record).map_err(|e| ApplyError::Journal(e.to_string()))?;
        self.write_file(&format!("{OPS_DIR}/{}/op.json", record.id), format!("{text}\n").as_bytes(), 0o600)
    }

    fn set_aside(&self, op: &str, i: usize, rel: &str) -> Result<(), ApplyError> {
        let dir = format!("{OPS_DIR}/{op}/undone");
        self.root.ensure_dir(&dir)?;
        self.root.rename(rel, &format!("{dir}/{i}"))?;
        Ok(())
    }
}

fn parent_and_name(rel: &str) -> (&str, &str) {
    rel.rsplit_once('/').unwrap_or(("", rel))
}

fn valid_op_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn files_of(tree: &SkillTree, entry: &str) -> Result<Vec<NewFile>, ApplyError> {
    tree.files
        .iter()
        .map(|f| {
            let content =
                f.content.clone().ok_or_else(|| ApplyError::Journal(format!("'{entry}': {} was not read", f.path)))?;
            Ok(NewFile { path: f.path.clone(), content, executable: f.executable })
        })
        .collect()
}

/// Journal steps plus the new contents of the files they write.
type StepPlan = (Vec<Step>, BTreeMap<String, Vec<u8>>);

/// Translate the plan into journal steps and the new file contents they write.
fn steps_for(ctx: &Ctx<'_>, input: &ApplyInput<'_>) -> Result<StepPlan, ApplyError> {
    let id = input.op_id;
    let mut steps = Vec::new();
    let mut new_files = BTreeMap::new();
    let backup = |i: usize| format!("{OPS_DIR}/{id}/backup/{i}");
    let lock_for = |target: &str| input.plan.lock.skills.get(target);

    let mut rule_edits: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for a in &input.plan.actions {
        let is_rules = input.plan.rule_blocks.contains_key(&a.entry) && !a.target.contains('/');
        match (a.kind, is_rules) {
            (ActionKind::Create | ActionKind::Update, true) => {
                rule_edits.entry(a.target.as_str()).or_default().push(&a.entry)
            }
            (ActionKind::Create, false) => {
                let installed = lock_for(&a.target).map(|l| l.installed_sha256.clone()).unwrap_or_default();
                steps.push(Step::Place {
                    target: a.target.clone(),
                    entry: a.entry.clone(),
                    installed_sha256: installed,
                });
            }
            (ActionKind::Update, false) => {
                let installed = lock_for(&a.target).map(|l| l.installed_sha256.clone()).unwrap_or_default();
                let old = ctx.dir_hash(&a.target)?.unwrap_or_default();
                let b = backup(steps.len());
                steps.push(Step::Replace {
                    target: a.target.clone(),
                    entry: a.entry.clone(),
                    old_sha256: old,
                    installed_sha256: installed,
                    backup: b,
                });
            }
            (ActionKind::Remove, _) => {
                let sha = ctx.dir_hash(&a.target)?.unwrap_or_default();
                let b = backup(steps.len());
                steps.push(Step::Remove { target: a.target.clone(), sha256: sha, backup: b });
            }
            _ => {}
        }
    }
    let mut file_step = |steps: &mut Vec<Step>, target: &str, old: Option<Vec<u8>>, new: Vec<u8>| {
        let b = old.as_ref().map(|_| backup(steps.len()));
        steps.push(Step::WriteFile {
            target: target.to_string(),
            old_sha256: old.as_deref().map(sha_hex),
            new_sha256: sha_hex(&new),
            backup: b,
        });
        new_files.insert(target.to_string(), new);
    };
    for (file, entries) in rule_edits {
        let old = ctx.read_file(file)?;
        let mut text = match &old {
            Some(bytes) => Some(
                String::from_utf8(bytes.clone()).map_err(|_| ApplyError::Journal(format!("{file} is not UTF-8")))?,
            ),
            None => None,
        };
        for entry in entries {
            let block = &input.plan.rule_blocks[entry];
            text = Some(
                render_rules_file(text.as_deref(), entry, block)
                    .map_err(|reason| ApplyError::Precondition { target: file.to_string(), reason })?,
            );
        }
        file_step(&mut steps, file, old, text.unwrap_or_default().into_bytes());
    }
    let old_lock = ctx.read_file(LOCK_FILE)?;
    let new_lock = input.plan.lock.to_json().into_bytes();
    if old_lock.as_deref() != Some(new_lock.as_slice()) {
        file_step(&mut steps, LOCK_FILE, old_lock, new_lock);
    }
    Ok((steps, new_files))
}

fn precondition(ctx: &Ctx<'_>, step: &Step) -> Result<(), ApplyError> {
    let fail =
        |reason: &str| Err(ApplyError::Precondition { target: step.target().to_string(), reason: reason.into() });
    match step {
        Step::Place { target, .. } => {
            if ctx.root.entry(target)? != Entry::Absent {
                return fail("appeared after the plan was made");
            }
        }
        Step::Replace { target, old_sha256, .. } | Step::Remove { target, sha256: old_sha256, .. } => {
            if ctx.dir_hash(target)?.as_deref() != Some(old_sha256.as_str()) {
                return fail("changed after the plan was made");
            }
        }
        Step::WriteFile { target, old_sha256, .. } => {
            if ctx.read_file(target)?.as_deref().map(sha_hex) != *old_sha256 {
                return fail("changed after the plan was made");
            }
        }
    }
    Ok(())
}

fn execute(
    ctx: &Ctx<'_>,
    input: &ApplyInput<'_>,
    step: &Step,
    new_files: &BTreeMap<String, Vec<u8>>,
) -> Result<(), ApplyError> {
    let place = |target: &str, entry: &str, installed: &str| -> Result<(), ApplyError> {
        let tree = input.contents.get(entry).ok_or_else(|| ApplyError::Journal(format!("no content for '{entry}'")))?;
        let (parent, name) = parent_and_name(target);
        ctx.root.ensure_dir(parent)?;
        ctx.root.write_tree(parent, name, &files_of(tree, entry)?)?;
        if ctx.dir_hash(target)?.as_deref() != Some(installed) {
            return Err(ApplyError::Journal(format!("'{target}' does not have the planned content after writing")));
        }
        Ok(())
    };
    match step {
        Step::Place { target, entry, installed_sha256 } => place(target, entry, installed_sha256),
        Step::Replace { target, entry, installed_sha256, backup, .. } => {
            ctx.root.ensure_dir(parent_and_name(backup).0)?;
            ctx.root.rename(target, backup)?;
            place(target, entry, installed_sha256)
        }
        Step::Remove { target, backup, .. } => {
            ctx.root.ensure_dir(parent_and_name(backup).0)?;
            ctx.root.rename(target, backup).map_err(Into::into)
        }
        Step::WriteFile { target, backup, .. } => {
            if let Some(b) = backup {
                let old = ctx.read_file(target)?.unwrap_or_default();
                ctx.root.ensure_dir(parent_and_name(b).0)?;
                ctx.write_file(b, &old, 0o600)?;
            }
            let content =
                new_files.get(target).ok_or_else(|| ApplyError::Journal(format!("no content for {target}")))?;
            ctx.write_file(target, content, 0o644)
        }
    }
}

/// Reverse `record.steps[..record.done]`, newest first; returns (restored, skipped).
fn reverse(ctx: &Ctx<'_>, record: &OpRecord) -> Result<(Vec<String>, Vec<String>), ApplyError> {
    let mut restored = Vec::new();
    let mut skipped = Vec::new();
    for (i, step) in record.steps[..record.done].iter().enumerate().rev() {
        let target = step.target().to_string();
        let changed = |skipped: &mut Vec<String>, why: &str| skipped.push(format!("{target}: {why}; left as it is"));
        match step {
            Step::Place { installed_sha256, .. } => match ctx.dir_hash(&target)? {
                None => restored.push(target.clone()),
                Some(h) if h == *installed_sha256 => {
                    ctx.set_aside(&record.id, i, &target)?;
                    restored.push(target.clone());
                }
                Some(_) => changed(&mut skipped, "changed since it was installed"),
            },
            Step::Replace { installed_sha256, backup, .. } => match ctx.dir_hash(&target)? {
                Some(h) if h != *installed_sha256 => changed(&mut skipped, "changed since it was updated"),
                current => {
                    if current.is_some() {
                        ctx.set_aside(&record.id, i, &target)?;
                    }
                    ctx.root.rename(backup, &target)?;
                    restored.push(target.clone());
                }
            },
            Step::Remove { backup, .. } => {
                if ctx.root.entry(&target)? == Entry::Absent {
                    ctx.root.rename(backup, &target)?;
                    restored.push(target.clone());
                } else {
                    changed(&mut skipped, "something now exists there");
                }
            }
            Step::WriteFile { new_sha256, backup, .. } => {
                let current = ctx.read_file(&target)?;
                if current.as_deref().map(sha_hex).as_deref() != Some(new_sha256.as_str()) {
                    changed(&mut skipped, "changed since it was written");
                    continue;
                }
                match backup {
                    Some(b) => {
                        let old = ctx.read_file(b)?.unwrap_or_default();
                        ctx.write_file(&target, &old, 0o644)?;
                    }
                    None => ctx.set_aside(&record.id, i, &target)?,
                }
                restored.push(target.clone());
            }
        }
    }
    Ok((restored, skipped))
}

fn open(project: &Path) -> Result<(Root, ProcessLock), ApplyError> {
    let root = Root::open(project)?;
    root.ensure_dir(OPS_DIR)?;
    let lock = ProcessLock::acquire(&project.join(LOCK_PATH), &[project], Duration::from_secs(10))?;
    Ok((root, lock))
}

fn load_ops(ctx: &Ctx<'_>) -> Result<Vec<OpRecord>, ApplyError> {
    let dir = ctx.abs(OPS_DIR);
    let mut ops = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else { return Ok(ops) };
    let mut ids: Vec<String> =
        entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| valid_op_id(n)).collect();
    ids.sort();
    for id in ids {
        let rel = format!("{OPS_DIR}/{id}/op.json");
        if let Some(bytes) = ctx.read_file(&rel)? {
            let record: OpRecord =
                serde_json::from_slice(&bytes).map_err(|e| ApplyError::Journal(format!("{rel}: {e}")))?;
            if record.version != OP_VERSION || record.id != id {
                return Err(ApplyError::Journal(format!("{rel} is not an Aurora operation record")));
            }
            ops.push(record);
        }
    }
    Ok(ops)
}

/// The id of an operation that did not finish, if any.
pub fn pending_op(project: &Path) -> Result<Option<String>, ApplyError> {
    let ctx = Ctx { root: Root::open(project)?, project };
    Ok(load_ops(&ctx)?.into_iter().find(|o| o.status == OpStatus::Prepared).map(|o| o.id))
}

/// Execute the plan. Refused, unchanged and kept actions are not executed.
pub fn apply(input: &ApplyInput<'_>) -> Result<OpRecord, ApplyError> {
    if !valid_op_id(input.op_id) {
        return Err(ApplyError::Journal(format!("invalid operation id '{}'", input.op_id)));
    }
    let (root, _lock) = open(input.project)?;
    let ctx = Ctx { root, project: input.project };
    if let Some(p) = load_ops(&ctx)?.into_iter().find(|o| o.status == OpStatus::Prepared) {
        return Err(ApplyError::Pending(p.id));
    }
    if ctx.root.entry(&format!("{OPS_DIR}/{}", input.op_id))? != Entry::Absent {
        return Err(ApplyError::Journal(format!("operation {} already exists", input.op_id)));
    }
    let (steps, new_files) = steps_for(&ctx, input)?;
    let mut record =
        OpRecord { version: OP_VERSION, id: input.op_id.to_string(), status: OpStatus::Prepared, steps, done: 0 };
    if record.steps.is_empty() {
        record.status = OpStatus::Committed;
        return Ok(record);
    }
    ctx.root.ensure_dir(&format!("{OPS_DIR}/{}", record.id))?;
    ctx.save(&record)?;
    for i in 0..record.steps.len() {
        if input.fault_before_step == Some(i) {
            return Err(ApplyError::InjectedFault(i));
        }
        let step = record.steps[i].clone();
        if let Err(e) = precondition(&ctx, &step).and_then(|()| execute(&ctx, input, &step, &new_files)) {
            reverse(&ctx, &record)?;
            record.status = OpStatus::RolledBack;
            ctx.save(&record)?;
            return Err(e);
        }
        record.done = i + 1;
        ctx.save(&record)?;
    }
    record.status = OpStatus::Committed;
    ctx.save(&record)?;
    Ok(record)
}

/// Undo `op_id`, or the most recent operation that is committed or unfinished.
pub fn undo(project: &Path, op_id: Option<&str>) -> Result<UndoReport, ApplyError> {
    let (root, _lock) = open(project)?;
    let ctx = Ctx { root, project };
    let ops = load_ops(&ctx)?;
    let mut record = match op_id {
        Some(id) => {
            ops.into_iter().find(|o| o.id == id).ok_or_else(|| ApplyError::Journal(format!("no operation {id}")))?
        }
        None => ops
            .into_iter()
            .rev()
            .find(|o| matches!(o.status, OpStatus::Committed | OpStatus::Prepared))
            .ok_or(ApplyError::NoOperation)?,
    };
    if matches!(record.status, OpStatus::Undone | OpStatus::RolledBack) {
        return Err(ApplyError::AlreadyUndone(record.id));
    }
    let (restored, skipped) = reverse(&ctx, &record)?;
    record.status = OpStatus::Undone;
    ctx.save(&record)?;
    Ok(UndoReport { op_id: record.id, restored, skipped })
}
