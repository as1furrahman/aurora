//! Install planning and application for a project: the workflow and CLI profile pick the skills,
//! every skill is fetched and verified against its pin, overlays are applied, the project is
//! observed without following symlinks, and the pure planner decides. Apply and undo go through
//! the journaled engine in `aurora_skills::apply`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use aurora_core::catalog::{RecommendRequest, Recommendation};
use aurora_core::presets::{PRESETS, get_preset};
use aurora_core::workflow::{WorkflowDefinition, parse_workflow_bytes, parse_workflow_str};
use aurora_skills::apply::{ApplyError, ApplyInput, OpRecord, UndoReport, apply, undo};
use aurora_skills::overlay::strip_frontmatter_keys;
use aurora_skills::plan::{
    ActionKind, Gate, InstallPlan, Lock, Observed, PlanInput, PreparedRules, PreparedSkill, SourcePin, plan,
};
use aurora_skills::scan::{Readiness, readiness, scan};
use aurora_skills::skill::{listing_chars, parse_skill};
use aurora_skills::tree::{Limits, SkillTree, collect};

use crate::fetch::{Pin, fetch_checked, load_catalog, pin_for_entry, resolve_cache};

pub const LOCK_FILE: &str = "aurora.lock";
const MAX_RULES_FILE_BYTES: u64 = 1 << 20;

/// Where the workflow comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowSource {
    Preset(String),
    File(PathBuf),
}

/// What to plan: the same inputs as `aurora skills plan`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRequest {
    pub workflow: WorkflowSource,
    /// `solo`, `pair` or `trio`; `None` picks one from the number of CLIs.
    pub profile: Option<String>,
    pub clis: Vec<String>,
    /// Opt-in entries to include.
    pub with: Vec<String>,
    pub project: PathBuf,
    pub keep_hooks: bool,
    pub prune: bool,
    pub catalog: Option<PathBuf>,
    pub cache: Option<PathBuf>,
    pub mirror: Option<PathBuf>,
}

/// Everything a plan was computed from, so applying executes exactly it.
#[derive(Debug, Clone)]
pub struct Planned {
    pub project: PathBuf,
    pub recommendation: Recommendation,
    pub plan: InstallPlan,
    /// Entry -> the tree to install (overlays applied).
    pub contents: BTreeMap<String, SkillTree>,
    /// Entry -> the verified upstream copy in the cache (for reading before review).
    pub sources: BTreeMap<String, PathBuf>,
    /// Entry -> files a human must read before it is installed.
    pub review_files: BTreeMap<String, Vec<String>>,
    pub warnings: Vec<String>,
}

impl Planned {
    /// Plan warnings followed by recommendation and preparation warnings, without duplicates.
    pub fn all_warnings(&self) -> Vec<String> {
        let mut all = self.warnings.clone();
        for w in &self.plan.warnings {
            if !all.contains(w) {
                all.push(w.clone());
            }
        }
        all
    }

    /// Entries that need review and are not in `reviewed`, sorted.
    pub fn unreviewed(&self, reviewed: &[String]) -> Vec<String> {
        self.plan
            .actions
            .iter()
            .filter(|a| a.needs_review && !reviewed.contains(&a.entry))
            .map(|a| a.entry.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Number of creates, updates and removals.
    pub fn change_count(&self) -> usize {
        self.plan
            .actions
            .iter()
            .filter(|a| matches!(a.kind, ActionKind::Create | ActionKind::Update | ActionKind::Remove))
            .count()
    }
}

/// `profile`, or the profile matching the number of CLIs (1 solo, 3 trio, otherwise pair).
pub fn default_profile(profile: Option<&str>, clis: usize) -> String {
    profile.map(str::to_string).unwrap_or_else(|| {
        match clis {
            1 => "solo",
            3 => "trio",
            _ => "pair",
        }
        .to_string()
    })
}

pub fn load_workflow(source: &WorkflowSource) -> Result<WorkflowDefinition, String> {
    match source {
        WorkflowSource::Preset(name) => {
            let names: Vec<&str> = PRESETS.iter().map(|p| p.name).collect();
            let preset = get_preset(name)
                .ok_or_else(|| format!("unknown preset '{name}'; expected one of: {}", names.join(", ")))?;
            parse_workflow_str(preset.toml, None, preset.name).map_err(|e| e.to_string())
        }
        WorkflowSource::File(file) => {
            let raw = std::fs::read(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
            parse_workflow_bytes(&raw, None, &file.display().to_string()).map_err(|e| e.to_string())
        }
    }
}

fn observe_dir(project: &Path, target: &str) -> Observed {
    let path = project.join(target);
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Observed::Absent,
        Ok(m) if m.is_dir() => match collect(&path, &Limits::default()) {
            Ok(tree) => Observed::Dir { sha256: tree.content_hash() },
            Err(_) => Observed::NotADir,
        },
        _ => Observed::NotADir,
    }
}

/// The project's `aurora.lock`, if any.
pub fn read_lock(project: &Path) -> Result<Option<Lock>, String> {
    let path = project.join(LOCK_FILE);
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
        Ok(_) => {
            let info = aurora_fs::read::safe_read(&path, &[project], MAX_RULES_FILE_BYTES)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let text = String::from_utf8(info.content).map_err(|_| format!("{} is not UTF-8", path.display()))?;
            Lock::from_json(&text).map(Some).map_err(|e| format!("{}: {e}", path.display()))
        }
    }
}

/// Replace SKILL.md in `tree` with `text` (an overlay result).
fn with_skill_md(tree: &SkillTree, text: &str) -> SkillTree {
    let mut out = tree.clone();
    if let Some(f) = out.files.iter_mut().find(|f| f.path == "SKILL.md") {
        f.content = Some(text.as_bytes().to_vec());
        f.size = text.len() as u64;
    }
    out
}

fn gate(r: &Readiness) -> Gate {
    match r {
        Readiness::Ready => Gate::Ready,
        Readiness::Reviewed(note) => Gate::Reviewed(note.clone()),
        Readiness::NeedsReview(paths) => Gate::NeedsReview(paths.clone()),
        Readiness::Blocked(rules) => Gate::Blocked(rules.clone()),
        Readiness::IntegrityMismatch { .. } => Gate::Blocked(vec!["integrity".into()]),
    }
}

fn skill_md(tree: &SkillTree) -> Result<String, String> {
    let bytes = tree.file("SKILL.md").and_then(|f| f.content.clone()).ok_or("SKILL.md is missing or unreadable")?;
    String::from_utf8(bytes).map_err(|_| "SKILL.md is not UTF-8".to_string())
}

/// Compute the install plan for `req`. Fetches into the cache; writes nothing to the project.
pub fn plan_install(req: &InstallRequest) -> Result<Planned, String> {
    let catalog = load_catalog(req.catalog.as_deref())?;
    let wf = load_workflow(&req.workflow)?;
    let profile = default_profile(req.profile.as_deref(), req.clis.len());
    let rreq = RecommendRequest { profile, clis: req.clis.clone(), with: req.with.clone() };
    let rec = catalog.recommend(&wf, &rreq).map_err(|e| e.to_string())?;
    let cache = resolve_cache(req.cache.as_deref())?;
    let project = std::fs::canonicalize(&req.project).map_err(|e| format!("project {}: {e}", req.project.display()))?;
    let mut warnings = rec.warnings.clone();

    // Entry -> CLIs that want it.
    let mut wanted: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for load in &rec.providers {
        for id in &load.skills {
            wanted.entry(id.clone()).or_default().push(load.provider.clone());
        }
    }

    let strip: Vec<&str> = if req.keep_hooks { vec!["allowed-tools"] } else { vec!["allowed-tools", "hooks"] };
    let mut skills = Vec::new();
    let mut contents = BTreeMap::new();
    let mut sources = BTreeMap::new();
    let mut review_files = BTreeMap::new();
    for (id, providers) in &wanted {
        let pin: Pin = pin_for_entry(&catalog, id)?;
        let fetched = fetch_checked(&pin, &cache, req.mirror.as_deref())?;
        if let Readiness::IntegrityMismatch { expected, got } = &fetched.readiness {
            return Err(format!(
                "'{id}': fetched content {got} differs from the catalog pin {expected}; refusing to plan"
            ));
        }
        let original = skill_md(&fetched.tree).map_err(|e| format!("'{id}': {e}"))?;
        let (text, overlays) = strip_frontmatter_keys(&original, &strip).map_err(|e| format!("'{id}': {e}"))?;
        let installed = with_skill_md(&fetched.tree, &text);
        // Integrity was checked on the upstream copy; the review gate judges what is installed,
        // so findings an overlay removed (allowed-tools, hooks) no longer need review. A catalog
        // review note covers the verified upstream content, which overlays only reduce.
        let installed_gate =
            readiness(&installed, &scan(&installed), Some(&installed.content_hash()), pin.review.clone());
        if let Readiness::NeedsReview(paths) = &installed_gate {
            review_files.insert(id.clone(), paths.clone());
        }
        let name = pin.path.rsplit('/').next().unwrap_or(id).to_string();
        let parsed = parse_skill(&text, &name).map_err(|e| format!("'{id}': {e}"))?;
        let listing = rec
            .clis
            .iter()
            .map(|cli| (cli.clone(), listing_chars(&parsed, cli, catalog.target(cli).and_then(|t| t.per_skill_chars))))
            .collect();
        skills.push(PreparedSkill {
            entry: id.clone(),
            name,
            source: SourcePin {
                repo: pin.repo.clone(),
                rev: pin.rev.clone(),
                path: pin.path.clone(),
                sha256: fetched.tree.content_hash(),
            },
            installed_sha256: installed.content_hash(),
            overlays,
            gate: gate(&installed_gate),
            listing_chars: listing,
            providers: providers.clone(),
        });
        contents.insert(id.clone(), installed);
        sources.insert(id.clone(), fetched.dir.clone());
    }

    let mut rules = Vec::new();
    let mut observed_files = BTreeMap::new();
    for r in &rec.rules {
        let pin = pin_for_entry(&catalog, &r.entry)?;
        let fetched = fetch_checked(&pin, &cache, req.mirror.as_deref())?;
        match &fetched.readiness {
            Readiness::Ready | Readiness::Reviewed(_) => {}
            other => {
                warnings.push(format!("rules '{}' are not ready ({other:?}); skipped", r.entry));
                continue;
            }
        }
        let text = skill_md(&fetched.tree).map_err(|e| format!("'{}': {e}", r.entry))?;
        let body = parse_skill(&text, &r.entry).map_err(|e| format!("'{}': {e}", r.entry))?.body;
        let mut files = Vec::new();
        for file in r.files.values() {
            if files.contains(file) {
                continue;
            }
            let path = project.join(file);
            match std::fs::symlink_metadata(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    observed_files.insert(file.clone(), None);
                    files.push(file.clone());
                }
                Ok(m) if m.file_type().is_symlink() => {
                    warnings.push(format!(
                        "{file} is a symlink; Aurora does not write through it (add the rules to its target)"
                    ));
                }
                Ok(m) if m.is_file() => {
                    let info = aurora_fs::read::safe_read(&path, &[project.as_path()], MAX_RULES_FILE_BYTES)
                        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
                    let content = String::from_utf8(info.content).map_err(|_| format!("{file} is not UTF-8"))?;
                    observed_files.insert(file.clone(), Some(content));
                    files.push(file.clone());
                }
                _ => warnings.push(format!("{file} is not a regular file; skipped")),
            }
        }
        if !files.is_empty() {
            rules.push(PreparedRules {
                entry: r.entry.clone(),
                source: SourcePin { repo: pin.repo, rev: pin.rev, path: pin.path, sha256: fetched.tree.content_hash() },
                text: body,
                files,
            });
        }
    }

    let previous = read_lock(&project)?;
    let mut observed_dirs = BTreeMap::new();
    let mut targets: Vec<String> = previous.iter().flat_map(|l| l.skills.keys().cloned()).collect();
    for s in &skills {
        for p in &s.providers {
            if let Some(dir) = aurora_skills::plan::skill_dir(p) {
                targets.push(format!("{}/{}", dir.rel, s.name));
            }
        }
    }
    for t in targets {
        let seen = observe_dir(&project, &t);
        observed_dirs.insert(t, seen);
    }
    let budgets = rec.clis.iter().filter_map(|c| catalog.target(c).map(|t| (c.clone(), t.listing_chars))).collect();
    let result = plan(&PlanInput {
        profile: &rec.profile,
        clis: &rec.clis,
        skills: &skills,
        rules: &rules,
        previous: previous.as_ref(),
        observed_dirs: &observed_dirs,
        observed_files: &observed_files,
        budgets: &budgets,
        prune: req.prune,
    });
    Ok(Planned { project, recommendation: rec, plan: result, contents, sources, review_files, warnings })
}

/// Refuse projects the boundary policy protects (home, broad roots, global agent dirs, sensitive paths).
pub fn check_project(project: &Path) -> Result<(), String> {
    let policy = aurora_fs::boundaries::BoundaryPolicy::host();
    aurora_fs::boundaries::validate_security_preconditions(project, &[project], false, &policy)
        .map(drop)
        .map_err(|e| format!("refusing to install into {}: {e}", project.display()))
}

/// A sortable operation id: UTC time plus the process id.
pub fn new_op_id() -> String {
    let secs =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    format!("{}-{}", aurora_core::util::format_utc(secs).replace(':', ""), std::process::id())
}

/// Apply `planned` as operation `op_id`. Callers check the project and the review gate first.
pub fn apply_planned(planned: &Planned, op_id: &str) -> Result<OpRecord, ApplyError> {
    apply(&ApplyInput {
        project: &planned.project,
        plan: &planned.plan,
        contents: &planned.contents,
        op_id,
        fault_before_step: None,
    })
}

/// Undo the latest operation in `project`, or `op_id`.
pub fn undo_project(project: &Path, op_id: Option<&str>) -> Result<UndoReport, String> {
    let project = std::fs::canonicalize(project).map_err(|e| format!("project {}: {e}", project.display()))?;
    undo(&project, op_id).map_err(|e| e.to_string())
}
