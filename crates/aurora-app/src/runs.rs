//! Workflow runs under a run root, for the TUI's Runs tab: listing, status, acceptance criteria,
//! handoff text, and submit/approve/resume through the evidence-gated engine in `aurora-fs`.

use std::path::{Path, PathBuf};

use aurora_core::workflow::record::{RECORD_FILE, WORKFLOW_FILE};
use aurora_core::workflow::{PlanOptions, parse_workflow_bytes};
use aurora_fs::dirs::{Entry, Root};
use aurora_fs::engine::{ApproveResult, Engine, StatusReport, SubmitResult};
use aurora_fs::runs::{RunStore, validate_run_id};
use indexmap::IndexMap;

const MAX_TEXT_BYTES: u64 = 1 << 20;

/// One run directory and its status (or why it could not be read).
#[derive(Debug, Clone)]
pub struct RunSummary {
    pub dir: PathBuf,
    pub status: Result<StatusReport, String>,
}

/// Every run directory (one holding `run.json`) directly under `root`, newest name last.
pub fn list_runs(root: &Path) -> Result<Vec<RunSummary>, String> {
    // No tasks folder yet (the project's own .aurora/runs before the first task) means no tasks.
    if std::fs::symlink_metadata(root).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
        return Ok(Vec::new());
    }
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

/// Where a project's tasks live when no run root is given: `<project>/.aurora/runs`.
pub fn default_run_root(project: &Path) -> PathBuf {
    project.join(".aurora").join("runs")
}

/// Move a project-local task into `.aurora/archive/runs` without deleting or replacing anything.
/// Custom run roots are intentionally rejected: their archive location must be chosen explicitly
/// by a future API rather than silently moving project data across filesystems.
pub fn archive_task(run_dir: &Path, project: &Path) -> Result<PathBuf, String> {
    let project =
        std::fs::canonicalize(project).map_err(|e| format!("cannot resolve project {}: {e}", project.display()))?;
    let run_root = default_run_root(&project);
    let run_root = std::fs::canonicalize(&run_root)
        .map_err(|e| format!("cannot resolve task folder {}: {e}", run_root.display()))?;
    let run_dir =
        std::fs::canonicalize(run_dir).map_err(|e| format!("cannot resolve task {}: {e}", run_dir.display()))?;
    let id = run_dir
        .strip_prefix(&run_root)
        .ok()
        .filter(|rel| rel.components().count() == 1)
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .ok_or_else(|| "only tasks directly inside this project's .aurora/runs folder can be archived".to_string())?;
    validate_run_id(id).map_err(|e| e.to_string())?;

    let store = RunStore::host();
    let _lock = store.lock(&run_dir).map_err(|e| e.to_string())?;
    store.load_record(&run_dir).map_err(|e| e.to_string())?;

    let aurora_root = Root::open(&project.join(".aurora")).map_err(|e| e.to_string())?;
    if aurora_root.entry(&format!("runs/{id}")).map_err(|e| e.to_string())? != Entry::Dir {
        return Err(format!("task '{}' is not a regular run directory", run_dir.display()));
    }
    aurora_root.ensure_dir("archive/runs").map_err(|e| e.to_string())?;
    aurora_root.rename(&format!("runs/{id}"), &format!("archive/runs/{id}")).map_err(|e| e.to_string())?;
    Ok(project.join(".aurora/archive/runs").join(id))
}

/// Restore an archived project-local task to `.aurora/runs` without replacing an active task.
pub fn restore_task(id: &str, project: &Path) -> Result<PathBuf, String> {
    validate_run_id(id).map_err(|e| e.to_string())?;
    let project =
        std::fs::canonicalize(project).map_err(|e| format!("cannot resolve project {}: {e}", project.display()))?;
    let aurora_root = Root::open(&project.join(".aurora")).map_err(|e| e.to_string())?;
    if aurora_root.entry(&format!("archive/runs/{id}")).map_err(|e| e.to_string())? != Entry::Dir {
        return Err(format!("no archived task named '{id}'"));
    }
    aurora_root.ensure_dir("runs").map_err(|e| e.to_string())?;
    if aurora_root.entry(&format!("runs/{id}")).map_err(|e| e.to_string())? != Entry::Absent {
        return Err(format!("an active task named '{id}' already exists; nothing was replaced"));
    }

    let archived_dir = project.join(".aurora/archive/runs").join(id);
    let store = RunStore::host();
    let _lock = store.lock(&archived_dir).map_err(|e| e.to_string())?;
    store.load_record(&archived_dir).map_err(|e| e.to_string())?;
    aurora_root.rename(&format!("archive/runs/{id}"), &format!("runs/{id}")).map_err(|e| e.to_string())?;
    Ok(project.join(".aurora/runs").join(id))
}

/// Which AI tool does each stage, and which catalog skills its handoff names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileRoutes {
    pub routes: IndexMap<String, String>,
    pub stage_skills: IndexMap<String, Vec<String>>,
}

/// Stage routes and stage skills for `wf` under a catalog profile: what `--profile`/`--clis` do
/// on `aurora workflow plan|start`. Without a profile it follows the number of tools.
pub fn profile_routes(
    catalog: &aurora_core::catalog::Catalog,
    wf: &aurora_core::workflow::WorkflowDefinition,
    profile: Option<&str>,
    clis: &[String],
    with: &[String],
) -> Result<ProfileRoutes, String> {
    let req = aurora_core::catalog::RecommendRequest {
        profile: crate::install::default_profile(profile, clis.len()),
        clis: clis.to_vec(),
        with: with.to_vec(),
    };
    let rec = catalog.recommend(wf, &req).map_err(|e| e.to_string())?;
    let mut out = ProfileRoutes::default();
    for s in rec.stages {
        out.routes.insert(s.stage.clone(), s.provider);
        out.stage_skills.insert(s.stage, s.skills);
    }
    Ok(out)
}

/// A new task: a workflow preset run in a project with a set of AI tools.
#[derive(Debug, Clone)]
pub struct TaskRequest {
    pub project: PathBuf,
    pub run_root: PathBuf,
    pub preset: String,
    pub profile: Option<String>,
    pub clis: Vec<String>,
    pub with: Vec<String>,
    pub catalog: Option<PathBuf>,
}

/// Where a task's workflow definition is kept in the project.
pub fn task_workflow(project: &Path, preset: &str) -> PathBuf {
    project.join(".aurora").join("workflows").join(format!("{preset}.toml"))
}

/// Start a task: write the preset to `.aurora/workflows/<preset>.toml` unless a file is already
/// there (then it is yours and is used as it is), and start a run under the run root, each step
/// routed to its AI tool through the profile. Returns the run directory.
pub fn start_task(req: &TaskRequest) -> Result<PathBuf, String> {
    use aurora_fs::dirs::{Entry, Root};
    crate::install::check_project(&req.project)?;
    let preset =
        aurora_core::presets::get_preset(&req.preset).ok_or_else(|| format!("unknown task type '{}'", req.preset))?;
    let root = Root::open(&req.project).map_err(|e| e.to_string())?;
    root.ensure_dir(".aurora/workflows").map_err(|e| e.to_string())?;
    let rel = format!(".aurora/workflows/{}.toml", preset.name);
    let file = task_workflow(&req.project, preset.name);
    match root.entry(&rel).map_err(|e| e.to_string())? {
        Entry::Absent => {
            aurora_fs::atomic::atomic_write(&file, preset.toml.as_bytes(), &[req.project.as_path()], 0o644)
                .map_err(|e| e.to_string())?;
        }
        Entry::File => {}
        _ => return Err(format!("{} is not a regular file", file.display())),
    }
    let reg = crate::registry::load(None)?;
    let raw = std::fs::read(&file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let wf = aurora_core::workflow::parse_workflow_bytes(&raw, Some(&reg), &file.display().to_string())
        .map_err(|e| e.to_string())?;
    let catalog = crate::fetch::load_catalog(req.catalog.as_deref())?;
    let ProfileRoutes { routes, stage_skills } =
        profile_routes(&catalog, &wf, req.profile.as_deref(), &req.clis, &req.with)?;
    let opts = PlanOptions { routes, stage_skills, ..Default::default() };
    if req.run_root.starts_with(&req.project) {
        let rel = req.run_root.strip_prefix(&req.project).map_err(|e| e.to_string())?;
        root.ensure_dir(&rel.to_string_lossy()).map_err(|e| e.to_string())?;
    } else {
        std::fs::create_dir_all(&req.run_root).map_err(|e| format!("cannot create {}: {e}", req.run_root.display()))?;
    }
    // The engine's own id format, made unique: two tasks started in the same second both work.
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let base = format!("{}-{}", wf.id, aurora_core::util::format_utc(secs).replace(['-', ':'], ""));
    let id = (1..100)
        .map(|n| if n == 1 { base.clone() } else { format!("{base}-{n}") })
        .find(|id| std::fs::symlink_metadata(req.run_root.join(id)).is_err())
        .ok_or_else(|| format!("too many tasks started this second in {}", req.run_root.display()))?;
    let started = Engine::host()
        .start_run(&file, &req.run_root, Some(&reg), &opts, Some(&id), None)
        .map_err(|e| e.to_string())?;
    Ok(started.run_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().expect("tmp");
        let dir = tmp.path().join("project");
        std::fs::create_dir(&dir).expect("project");
        let dir = std::fs::canonicalize(dir).expect("canonical");
        (tmp, dir)
    }

    fn request(project: &Path) -> TaskRequest {
        TaskRequest {
            project: project.to_path_buf(),
            run_root: default_run_root(project),
            preset: "bugfix".into(),
            profile: None,
            clis: vec!["claude".into(), "codex".into()],
            with: Vec::new(),
            catalog: None,
        }
    }

    #[test]
    fn a_task_writes_the_preset_once_and_starts_a_run() {
        let (_tmp, dir) = project();
        let run = start_task(&request(&dir)).expect("started");
        assert!(run.starts_with(default_run_root(&dir)));
        assert!(task_workflow(&dir, "bugfix").is_file());
        let status = Engine::host().status(&run).expect("status");
        let review = status.stages.iter().find(|s| s.id == "review").expect("review");
        assert_eq!(review.provider, "codex", "pair: the reviewer is the second tool");
        start_task(&request(&dir)).expect("a second task reuses the workflow");
        assert_eq!(list_runs(&default_run_root(&dir)).expect("runs").len(), 2);
    }

    #[test]
    fn archiving_moves_a_task_out_of_the_active_list_without_deleting_it() {
        let (_tmp, dir) = project();
        let run = start_task(&request(&dir)).expect("started");
        let run_file = run.join(RECORD_FILE);

        let archived = archive_task(&run, &dir).expect("archived");

        assert_eq!(archived, dir.join(".aurora/archive/runs").join(run.file_name().expect("id")));
        assert!(!run.exists(), "the task leaves the active folder");
        assert!(archived.join(RECORD_FILE).is_file(), "the task remains recoverable");
        assert!(run_file != archived.join(RECORD_FILE));
        assert!(list_runs(&default_run_root(&dir)).expect("active tasks").is_empty());
        assert_eq!(
            Engine::host().status(&archived).expect("archived run remains readable").run_id,
            archived.file_name().expect("archive id").to_string_lossy()
        );
    }

    #[test]
    fn archiving_rejects_custom_roots_and_never_replaces_an_archive() {
        let (_tmp, dir) = project();
        let run = start_task(&request(&dir)).expect("started");
        let elsewhere = dir.join("elsewhere");
        std::fs::create_dir(&elsewhere).expect("custom root");
        let error = archive_task(&elsewhere, &dir).expect_err("not a task run");
        assert!(error.contains(".aurora/runs"), "{error}");

        let archive = dir.join(".aurora/archive/runs").join(run.file_name().expect("id"));
        std::fs::create_dir_all(&archive).expect("archive collision");
        let error = archive_task(&run, &dir).expect_err("destination collision");
        assert!(error.contains("cannot move"), "{error}");
        assert!(run.join(RECORD_FILE).is_file(), "source stays intact on collision");
    }

    #[test]
    fn restoring_moves_an_archived_task_back_to_the_active_list() {
        let (_tmp, dir) = project();
        let run = start_task(&request(&dir)).expect("started");
        let id = run.file_name().expect("id").to_string_lossy().into_owned();
        let archived = archive_task(&run, &dir).expect("archived");

        let restored = restore_task(&id, &dir).expect("restored");

        assert_eq!(restored, run);
        assert!(!archived.exists());
        assert!(restored.join(RECORD_FILE).is_file());
        assert_eq!(list_runs(&default_run_root(&dir)).expect("active tasks").len(), 1);
    }

    #[test]
    fn restoring_never_replaces_an_active_task() {
        let (_tmp, dir) = project();
        let run = start_task(&request(&dir)).expect("started");
        let id = run.file_name().expect("id").to_string_lossy().into_owned();
        let archived = archive_task(&run, &dir).expect("archived");
        let active = start_task(&request(&dir)).expect("active task with distinct id");
        std::fs::rename(&active, &run).expect("recreate the colliding active task path");

        let error = restore_task(&id, &dir).expect_err("collision must fail");

        assert!(error.contains("already exists"), "{error}");
        assert!(archived.join(RECORD_FILE).is_file());
        assert!(run.join(RECORD_FILE).is_file());
    }

    #[test]
    fn an_edited_workflow_is_kept() {
        let (_tmp, dir) = project();
        let file = task_workflow(&dir, "bugfix");
        std::fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
        let mine = format!("{}\n# my own notes\n", aurora_core::presets::get_preset("bugfix").expect("preset").toml);
        std::fs::write(&file, &mine).expect("write");
        start_task(&request(&dir)).expect("started");
        assert_eq!(std::fs::read_to_string(&file).expect("read"), mine);
    }

    #[test]
    fn a_symlinked_aurora_folder_is_refused() {
        let (tmp, dir) = project();
        let elsewhere = tmp.path().join("elsewhere");
        std::fs::create_dir(&elsewhere).expect("elsewhere");
        std::os::unix::fs::symlink(&elsewhere, dir.join(".aurora")).expect("symlink");
        assert!(start_task(&request(&dir)).is_err());
        assert_eq!(std::fs::read_dir(&elsewhere).expect("read").count(), 0, "nothing written through the link");
    }

    #[test]
    fn a_missing_task_folder_lists_no_tasks() {
        let (_tmp, dir) = project();
        assert!(list_runs(&default_run_root(&dir)).expect("ok").is_empty());
    }
}
