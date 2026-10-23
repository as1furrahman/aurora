//! `aurora skills plan`: what installing a workflow's recommended skills into a project would do.
//! A dry run: it fetches into the cache and reads the project without following symlinks, but
//! writes nothing to the project.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anstream::println;
use aurora_core::catalog::{Catalog, RecommendRequest, Recommendation, builtin_catalog, parse_catalog_str};
use aurora_core::presets::{PRESETS, get_preset};
use aurora_core::workflow::{WorkflowDefinition, parse_workflow_bytes, parse_workflow_str};
use aurora_skills::apply::{ApplyError, ApplyInput, apply, undo};
use aurora_skills::overlay::strip_frontmatter_keys;
use aurora_skills::plan::{
    ActionKind, Gate, InstallPlan, Lock, Observed, PlanInput, PreparedRules, PreparedSkill, SourcePin, plan,
};
use aurora_skills::scan::{Readiness, readiness, scan};
use aurora_skills::skill::{listing_chars, parse_skill};
use aurora_skills::tree::{Limits, SkillTree, collect};
use clap::Args;
use serde_json::json;

use crate::skills::{Pin, error, fetch_checked, pin_for_entry, resolve_cache};
use crate::style::{BAD, DIM, HEAD, OK, WARN, paint};

pub const LOCK_FILE: &str = "aurora.lock";
const MAX_RULES_FILE_BYTES: u64 = 1 << 20;

#[derive(Debug, Args)]
pub struct PlanArgs {
    /// Workflow definition (.toml); or use --preset
    #[arg(required_unless_present = "preset", conflicts_with = "preset")]
    file: Option<PathBuf>,
    /// Built-in preset name (see `aurora workflow presets`)
    #[arg(long)]
    preset: Option<String>,
    /// CLI profile: solo, pair or trio (default: from the number of --clis, else pair)
    #[arg(long)]
    profile: Option<String>,
    /// CLIs filling the profile's slots, comma-separated (e.g. claude,codex)
    #[arg(long, value_delimiter = ',')]
    clis: Vec<String>,
    /// Include an opt-in entry (repeatable)
    #[arg(long, value_name = "ENTRY")]
    with: Vec<String>,
    /// Project directory to install into (default: the current directory)
    #[arg(long, default_value = ".")]
    project: PathBuf,
    /// Keep skill hooks instead of removing them (they run commands without asking)
    #[arg(long)]
    keep_hooks: bool,
    /// Remove Aurora-installed skills that are no longer wanted
    #[arg(long)]
    prune: bool,
    /// Custom catalog file (default: the built-in catalog)
    #[arg(long, value_name = "FILE")]
    catalog: Option<PathBuf>,
    /// Cache directory (default: $AURORA_CACHE_DIR, else $XDG_CACHE_HOME/aurora/skills, else ~/.cache/aurora/skills)
    #[arg(long, value_name = "DIR")]
    cache: Option<PathBuf>,
    /// Serve https://github.com/<owner>/<name> from DIR/<owner>/<name> (an offline mirror)
    #[arg(long, value_name = "DIR")]
    mirror: Option<PathBuf>,
    /// Output in JSON format
    #[arg(long)]
    json: bool,
}

/// Everything a plan was computed from, so `apply` can execute exactly it.
pub(crate) struct Planned {
    pub project: PathBuf,
    pub recommendation: Recommendation,
    pub plan: InstallPlan,
    /// Entry -> the tree to install (overlays applied).
    pub contents: BTreeMap<String, SkillTree>,
    /// Entry -> files a human must read before it is installed.
    pub review_files: BTreeMap<String, Vec<String>>,
    pub warnings: Vec<String>,
}

fn load_catalog(path: Option<&Path>) -> Result<Catalog, String> {
    match path {
        None => builtin_catalog().map_err(|e| e.to_string()),
        Some(p) => {
            let text = std::fs::read_to_string(p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
            parse_catalog_str(&text).map_err(|e| format!("{}: {e}", p.display()))
        }
    }
}

fn load_workflow(args: &PlanArgs) -> Result<WorkflowDefinition, String> {
    if let Some(name) = &args.preset {
        let names: Vec<&str> = PRESETS.iter().map(|p| p.name).collect();
        let preset = get_preset(name)
            .ok_or_else(|| format!("unknown preset '{name}'; expected one of: {}", names.join(", ")))?;
        return parse_workflow_str(preset.toml, None, preset.name).map_err(|e| e.to_string());
    }
    let file = args.file.as_ref().ok_or("a workflow file or --preset is required")?;
    let raw = std::fs::read(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    parse_workflow_bytes(&raw, None, &file.display().to_string()).map_err(|e| e.to_string())
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

fn read_lock(project: &Path) -> Result<Option<Lock>, String> {
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

/// Compute the plan for `args` (used by `plan` and, in Step 4, `apply`).
pub(crate) fn compute(args: &PlanArgs) -> Result<Planned, String> {
    let catalog = load_catalog(args.catalog.as_deref())?;
    let wf = load_workflow(args)?;
    let profile = args.profile.clone().unwrap_or_else(|| {
        match args.clis.len() {
            1 => "solo",
            3 => "trio",
            _ => "pair",
        }
        .to_string()
    });
    let req = RecommendRequest { profile, clis: args.clis.clone(), with: args.with.clone() };
    let rec = catalog.recommend(&wf, &req).map_err(|e| e.to_string())?;
    let cache = resolve_cache(args.cache.as_deref())?;
    let project =
        std::fs::canonicalize(&args.project).map_err(|e| format!("project {}: {e}", args.project.display()))?;
    let mut warnings = rec.warnings.clone();

    // Entry -> CLIs that want it.
    let mut wanted: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for load in &rec.providers {
        for id in &load.skills {
            wanted.entry(id.clone()).or_default().push(load.provider.clone());
        }
    }

    let strip: Vec<&str> = if args.keep_hooks { vec!["allowed-tools"] } else { vec!["allowed-tools", "hooks"] };
    let mut skills = Vec::new();
    let mut contents = BTreeMap::new();
    let mut review_files = BTreeMap::new();
    for (id, providers) in &wanted {
        let pin: Pin = pin_for_entry(&catalog, id)?;
        let fetched = fetch_checked(&pin, &cache, args.mirror.as_deref())?;
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
    }

    let mut rules = Vec::new();
    let mut observed_files = BTreeMap::new();
    for r in &rec.rules {
        let pin = pin_for_entry(&catalog, &r.entry)?;
        let fetched = fetch_checked(&pin, &cache, args.mirror.as_deref())?;
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
        prune: args.prune,
    });
    Ok(Planned { project, recommendation: rec, plan: result, contents, review_files, warnings })
}

pub fn run_plan(args: &PlanArgs) -> u8 {
    let planned = match compute(args) {
        Ok(p) => p,
        Err(e) => return error(e),
    };
    let Planned { project, recommendation: rec, plan: p, warnings: pre, .. } = &planned;
    let mut warnings = pre.clone();
    warnings.extend(p.warnings.iter().cloned());
    if args.json {
        let out = json!({
            "project": project.display().to_string(),
            "profile": rec.profile,
            "clis": rec.clis,
            "actions": p.actions,
            "visibility": p.visibility,
            "warnings": warnings,
            "lock": p.lock,
            "changed": p.changed,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }
    print_plan(project, rec, p, &warnings);
    println!("\n{}", paint(DIM, "Dry run: nothing was written to the project."));
    0
}

pub(crate) fn print_plan(project: &Path, rec: &Recommendation, p: &InstallPlan, warnings: &[String]) {
    println!(
        "{}",
        paint(
            HEAD,
            format!("Aurora — Install plan: {} ({}: {})", project.display(), rec.profile, rec.clis.join(" + "))
        )
    );
    println!("{}", "-".repeat(65));
    for a in &p.actions {
        let kind = match a.kind {
            ActionKind::Create => paint(OK, "create   "),
            ActionKind::Update => paint(OK, "update   "),
            ActionKind::Unchanged => paint(DIM, "unchanged"),
            ActionKind::Keep => paint(DIM, "keep     "),
            ActionKind::Remove => paint(WARN, "remove   "),
            ActionKind::Refuse => paint(BAD, "refuse   "),
        };
        let who = if a.providers.is_empty() { String::new() } else { format!(" [{}]", a.providers.join(", ")) };
        let review = if a.needs_review { paint(WARN, "  needs your review") } else { String::new() };
        let detail = if a.detail.is_empty() { String::new() } else { format!("  {}", a.detail) };
        println!("  {kind} {:<46} {}{who}{review}{detail}", a.target, a.entry);
    }
    let overlays: Vec<(&String, &Vec<String>)> =
        p.lock.skills.values().filter(|s| !s.overlays.is_empty()).map(|s| (&s.entry, &s.overlays)).collect();
    if !overlays.is_empty() {
        println!("\nOverlays (the only edits Aurora makes to upstream files):");
        let mut seen = Vec::new();
        for (entry, keys) in overlays {
            if seen.contains(&entry) {
                continue;
            }
            seen.push(entry);
            for key in keys {
                let why = if key == "hooks" {
                    "hooks run commands without asking; --keep-hooks keeps them"
                } else {
                    "pre-approved tools; the CLI asks you as usual"
                };
                println!("  {entry}: {key} removed ({why})");
            }
        }
    }
    println!("\nWhat each CLI will load from this project:");
    for v in &p.visibility {
        let line = format!(
            "  {:<12} {:>2} skills, ~{} of {} chars: {}",
            v.provider,
            v.skills.len(),
            v.listing_chars,
            v.budget_chars,
            v.skills.join(", ")
        );
        println!("{}", if v.listing_chars > v.budget_chars { paint(BAD, line) } else { line });
    }
    if !warnings.is_empty() {
        println!("\nWarnings:");
        for w in warnings {
            println!("  {} {w}", paint(WARN, "!"));
        }
    }
    let lock_state = if !p.changed { "unchanged" } else { "would be written" };
    println!(
        "\n{LOCK_FILE}: {lock_state} ({} installed copies, {} rule blocks)",
        p.lock.skills.len(),
        p.lock.rules.values().map(BTreeMap::len).sum::<usize>()
    );
}

#[derive(Debug, Args)]
pub struct ApplyArgs {
    #[command(flatten)]
    plan: PlanArgs,
    /// Apply without asking (required when not running in a terminal)
    #[arg(long)]
    yes: bool,
    /// Confirm you read the flagged files of this entry (repeatable)
    #[arg(long, value_name = "ENTRY")]
    reviewed: Vec<String>,
}

fn op_id() -> String {
    let secs =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    format!("{}-{}", aurora_core::util::format_utc(secs).replace(':', ""), std::process::id())
}

fn confirm(question: &str) -> bool {
    use std::io::{BufRead, IsTerminal, Write};
    if !std::io::stdin().is_terminal() {
        return false;
    }
    print!("{question} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer).is_ok() && matches!(answer.trim(), "y" | "Y" | "yes")
}

pub fn run_apply(args: &ApplyArgs) -> u8 {
    let planned = match compute(&args.plan) {
        Ok(p) => p,
        Err(e) => return error(e),
    };
    let Planned { project, recommendation: rec, plan: p, contents, review_files, warnings: pre } = &planned;
    let policy = aurora_fs::boundaries::BoundaryPolicy::host();
    if let Err(e) =
        aurora_fs::boundaries::validate_security_preconditions(project, &[project.as_path()], false, &policy)
    {
        return error(format!("refusing to install into {}: {e}", project.display()));
    }
    let mut warnings = pre.clone();
    warnings.extend(p.warnings.iter().cloned());
    if !args.plan.json {
        print_plan(project, rec, p, &warnings);
    }
    let unreviewed: Vec<String> = p
        .actions
        .iter()
        .filter(|a| a.needs_review && !args.reviewed.contains(&a.entry))
        .map(|a| a.entry.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if !unreviewed.is_empty() {
        let list: Vec<String> = unreviewed
            .iter()
            .map(|e| format!("{e} (read: {})", review_files.get(e).map(|f| f.join(", ")).unwrap_or_default()))
            .collect();
        let flags: Vec<String> = unreviewed.iter().map(|e| format!("--reviewed {e}")).collect();
        return error(format!(
            "these skills need your review before they are installed: {}. Read the files in the cache \
             (`aurora skills fetch <entry>` shows where), then rerun with {}",
            list.join("; "),
            flags.join(" ")
        ));
    }
    if !p.changed {
        println!("\nNothing to do: the project already matches the plan.");
        return 0;
    }
    let changes = p
        .actions
        .iter()
        .filter(|a| matches!(a.kind, ActionKind::Create | ActionKind::Update | ActionKind::Remove))
        .count();
    if !args.yes && !confirm(&format!("\nApply {changes} changes to {}?", project.display())) {
        return error("not applied: confirm in a terminal, or pass --yes");
    }
    let id = op_id();
    let input = ApplyInput { project, plan: p, contents, op_id: &id, fault_before_step: None };
    match apply(&input) {
        Ok(record) => {
            if args.plan.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &json!({"project": project.display().to_string(), "operation": record})
                    )
                    .unwrap_or_default()
                );
            } else {
                println!("\n{} {changes} changes (operation {id}).", paint(OK, "Applied"));
                println!("  Undo: aurora skills undo --project {}", project.display());
            }
            0
        }
        Err(e @ ApplyError::Pending(_)) => error(e),
        Err(e) => error(format!("not applied: {e}")),
    }
}

#[derive(Debug, Args)]
pub struct UndoArgs {
    /// Project directory (default: the current directory)
    #[arg(long, default_value = ".")]
    project: PathBuf,
    /// Operation id (default: the most recent one)
    #[arg(long, value_name = "ID")]
    op: Option<String>,
    /// Output in JSON format
    #[arg(long)]
    json: bool,
}

pub fn run_undo(args: &UndoArgs) -> u8 {
    let project = match std::fs::canonicalize(&args.project) {
        Ok(p) => p,
        Err(e) => return error(format!("project {}: {e}", args.project.display())),
    };
    match undo(&project, args.op.as_deref()) {
        Ok(report) => {
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
                return 0;
            }
            println!("{} {} items from operation {}:", paint(OK, "Restored"), report.restored.len(), report.op_id);
            for r in &report.restored {
                println!("  {r}");
            }
            if !report.skipped.is_empty() {
                println!("\nSkipped (changed since the operation; left as they are):");
                for s in &report.skipped {
                    println!("  {} {s}", paint(WARN, "!"));
                }
            }
            println!(
                "\n{}",
                paint(
                    DIM,
                    format!(
                        "Removed items were moved aside to {}/.aurora/ops/{}/, not deleted.",
                        project.display(),
                        report.op_id
                    )
                )
            );
            0
        }
        Err(e) => error(e),
    }
}
