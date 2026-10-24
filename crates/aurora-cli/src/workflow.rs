//! `aurora workflow validate|show|plan` (read-only workflow commands).

use anstream::{eprintln, println};
use aurora_core::registry::Registry;
use aurora_core::workflow::{
    Plan, PlanError, PlanOptions, WorkflowDefinition, WorkflowValidationError, build_plan, describe,
    parse_workflow_bytes,
};
use indexmap::IndexMap;
use serde_json::json;

use crate::style::{BAD, HEAD, OK, WARN, paint};
use crate::{DefinitionArgs, RoutingArgs, providers, registry};

pub const EXIT_BLOCKED: u8 = 3;

enum LoadError {
    Registry(String),
    Definition(WorkflowValidationError),
}

fn load(def: &DefinitionArgs) -> Result<(WorkflowDefinition, Registry), LoadError> {
    let reg = registry::load(def.registry.as_deref()).map_err(LoadError::Registry)?;
    let source = def.file.display().to_string();
    let raw = std::fs::read(&def.file).map_err(|e| {
        LoadError::Definition(WorkflowValidationError {
            errors: vec![format!("cannot read file: {e}")],
            source_name: source.clone(),
        })
    })?;
    let wf = parse_workflow_bytes(&raw, Some(&reg), &source).map_err(LoadError::Definition)?;
    Ok((wf, reg))
}

fn error(message: impl std::fmt::Display) -> u8 {
    eprintln!("{} {message}", paint(BAD, "Error:"));
    1
}

fn print_json(value: &impl serde::Serialize) {
    println!("{}", serde_json::to_string_pretty(value).unwrap_or_default());
}

pub fn validate(def: &DefinitionArgs) -> u8 {
    match load(def) {
        Err(LoadError::Registry(e)) => error(e),
        Err(LoadError::Definition(e)) => {
            if def.json {
                print_json(&json!({"valid": false, "source": e.source_name, "errors": e.errors}));
            } else {
                println!("{} {}", paint(BAD, "INVALID:"), e.source_name);
                for err in &e.errors {
                    println!("  - {err}");
                }
            }
            1
        }
        Ok((wf, _)) => {
            if def.json {
                print_json(&json!({
                    "valid": true, "id": wf.id, "version": wf.version, "sha256": wf.sha256, "warnings": wf.warnings,
                }));
            } else {
                println!(
                    "{} {} v{} ({} roles, {} stages)",
                    paint(OK, "VALID:"),
                    wf.id,
                    wf.version,
                    wf.roles.len(),
                    wf.stages.len()
                );
                println!("  sha256: {}", wf.sha256);
                for w in &wf.warnings {
                    println!("  {} {w}", paint(WARN, "[warning]"));
                }
            }
            0
        }
    }
}

pub fn show(def: &DefinitionArgs) -> u8 {
    let wf = match load(def) {
        Ok((wf, _)) => wf,
        Err(LoadError::Registry(e)) => return error(e),
        Err(LoadError::Definition(e)) => return error(e),
    };
    let data = describe(&wf);
    if def.json {
        print_json(&data);
        return 0;
    }
    println!(
        "{}",
        paint(
            HEAD,
            format!("Aurora — Workflow: {} [{} v{}] (assurance: {})", wf.name, wf.id, wf.version, wf.assurance)
        )
    );
    println!("{}", "-".repeat(65));
    println!("Roles (provider-neutral):");
    for (rid, r) in &data.roles {
        println!("  {rid}: {}", r.responsibility);
        println!(
            "    required skills: {}; recommended: {}",
            or_none(&r.required_skills),
            or_none(&r.recommended_skills)
        );
    }
    println!("\nStages:");
    for s in &data.stages {
        let flags: Vec<&str> = [("optional", s.optional), ("requires_approval", s.requires_approval)]
            .into_iter()
            .filter_map(|(name, on)| on.then_some(name))
            .collect();
        let deps = if s.depends_on.is_empty() { "-".to_string() } else { s.depends_on.join(", ") };
        let suffix = if flags.is_empty() { String::new() } else { format!(" ({})", flags.join(", ")) };
        println!("  {} [role: {}] depends_on: {deps}{suffix}", s.id, s.role);
        if !s.independent_of.is_empty() {
            println!("    independent_of: {}", s.independent_of.join(", "));
        }
        println!("    outputs: {}; max_attempts: {} -> {}", s.outputs.join(", "), s.max_attempts, s.on_exhausted);
        for c in &s.acceptance {
            println!("    gate {}: {} [evidence: {}]", c.id, c.description, c.evidence.join(", "));
        }
    }
    println!("\nRouting (separate from roles):");
    for (rid, rr) in &data.routing.roles {
        println!("  {rid}: eligible {}, preferred {}", list_repr(&rr.eligible), rr.preferred);
    }
    for f in &data.routing.fallback {
        println!("  fallback {}: {} -> {} on {}", f.role, f.from, f.to, list_repr(&f.on));
    }
    0
}

fn or_none(items: &[String]) -> String {
    if items.is_empty() { "none".to_string() } else { items.join(", ") }
}

fn list_repr(items: &[String]) -> String {
    aurora_core::util::list_repr(items)
}

/// Parse repeated `--route STAGE=PROVIDER` values, keeping their order.
pub fn parse_routes(values: &[String]) -> Result<IndexMap<String, String>, PlanError> {
    let mut routes = IndexMap::new();
    for item in values {
        let (stage, provider) = item
            .split_once('=')
            .filter(|(s, p)| !s.is_empty() && !p.is_empty())
            .ok_or_else(|| PlanError(format!("--route expects STAGE=PROVIDER, got '{item}'")))?;
        if routes.contains_key(stage) {
            return Err(PlanError(format!("--route given twice for stage '{stage}'")));
        }
        routes.insert(stage.to_string(), provider.to_string());
    }
    Ok(routes)
}

/// Load the definition and build its plan with the CLI's routing options.
pub fn plan_from_args(def: &DefinitionArgs, routing: &RoutingArgs) -> Result<(WorkflowDefinition, Plan), String> {
    let (wf, reg) = match load(def) {
        Ok(loaded) => loaded,
        Err(LoadError::Registry(e)) => return Err(e),
        Err(LoadError::Definition(e)) => return Err(e.to_string()),
    };
    let detected = (!routing.no_detect).then(providers::detect_all);
    let opts = plan_options(&wf, routing, detected.as_ref())?;
    let plan = build_plan(&wf, Some(&reg), &opts).map_err(|e| e.to_string())?;
    Ok((wf, plan))
}

/// Planning options from the CLI routing flags. With `--profile`/`--clis`, every stage is
/// routed through the catalog profile and its recommended skills are attached.
pub fn plan_options<'a>(
    wf: &WorkflowDefinition,
    routing: &'a RoutingArgs,
    detected: Option<&'a aurora_core::workflow::Detection>,
) -> Result<PlanOptions<'a>, String> {
    let mut opts = PlanOptions {
        provider_override: routing.provider.as_deref(),
        routes: parse_routes(&routing.route).map_err(|e| e.to_string())?,
        detected,
        ..Default::default()
    };
    if routing.profile.is_none() && routing.clis.is_empty() {
        if !routing.with.is_empty() {
            return Err("--with needs --profile or --clis".into());
        }
        return Ok(opts);
    }
    let catalog = aurora_core::catalog::builtin_catalog().map_err(|e| e.to_string())?;
    let profile =
        aurora_app::runs::profile_routes(&catalog, wf, routing.profile.as_deref(), &routing.clis, &routing.with)?;
    opts.routes.extend(profile.routes);
    opts.stage_skills = profile.stage_skills;
    Ok(opts)
}

pub fn plan(def: &DefinitionArgs, routing: &RoutingArgs) -> u8 {
    let plan = match plan_from_args(def, routing) {
        Ok((_, plan)) => plan,
        Err(e) => return error(e),
    };
    if def.json {
        print_json(&plan);
    } else {
        print_plan(&plan);
    }
    if plan.blocked { EXIT_BLOCKED } else { 0 }
}

pub fn print_plan(plan: &Plan) {
    let mode = match &plan.provider_override {
        Some(p) => format!("{}, provider={p}", plan.mode),
        None => plan.mode.clone(),
    };
    println!(
        "{}",
        paint(HEAD, format!("Aurora — Workflow Plan: {} v{} (mode: {mode})", plan.workflow.id, plan.workflow.version))
    );
    println!("{}", "-".repeat(65));
    println!(" {:>2}  {:<14} {:<12} {:<12} {:<10} status", "#", "stage", "role", "provider", "source");
    for (i, s) in plan.stages.iter().enumerate() {
        let status = if s.is_blocked() { paint(BAD, "BLOCKED") } else { "assignable".to_string() };
        println!(" {:>2}  {:<14} {:<12} {:<12} {:<10} {status}", i + 1, s.id, s.role, s.provider, s.assignment_source);
    }
    for s in &plan.stages {
        for step in &s.fallback_trail {
            println!("  [fallback] {}: {} -> {} ({})", s.id, step.from, step.to, step.reason);
        }
        for e in &s.independence {
            println!("  [independence] {} vs {}: {}", s.id, e.of, e.status);
        }
        for reason in &s.blocked_reasons {
            println!("  {} {}: {reason}", paint(BAD, "[BLOCKED]"), s.id);
            if reason.starts_with("incompatible") {
                let adapter = aurora_core::providers::get_provider(&s.provider)
                    .map(|p| p.skill_adapters[0])
                    .unwrap_or(&s.provider);
                println!(
                    "    fix: route the stage to a CLI that can load the skill (--route {}=<cli>), or declare \
                     `{adapter}` in the skill's compatible_agents in a team registry (--registry FILE)",
                    s.id
                );
            }
        }
        for w in &s.warnings {
            println!("  {} {}: {w}", paint(WARN, "[warning]"), s.id);
        }
    }
    for w in &plan.warnings {
        println!("  {} {w}", paint(WARN, "[warning]"));
    }
}

pub fn presets(as_json: bool) -> u8 {
    use aurora_core::presets::PRESETS;
    let reg = match registry::load(None) {
        Ok(reg) => reg,
        Err(e) => return error(e),
    };
    let mut rows = Vec::new();
    for preset in &PRESETS {
        let wf = match aurora_core::workflow::parse_workflow_str(preset.toml, Some(&reg), preset.name) {
            Ok(wf) => wf,
            Err(e) => return error(e),
        };
        rows.push(json!({
            "name": preset.name,
            "summary": preset.summary,
            "assurance": wf.assurance,
            "roles": wf.roles.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
            "stages": wf.stages.iter().map(|s| s.id.clone()).collect::<Vec<_>>(),
        }));
    }
    if as_json {
        print_json(&rows);
        return 0;
    }
    println!("{}", paint(HEAD, "Aurora — Built-in workflow presets"));
    println!("{}", "-".repeat(65));
    for row in &rows {
        let stages: Vec<&str> = row["stages"].as_array().into_iter().flatten().filter_map(|v| v.as_str()).collect();
        println!("  {:<16} {}", row["name"].as_str().unwrap_or_default(), row["summary"].as_str().unwrap_or_default());
        println!("  {:<16} stages: {}", "", stages.join(" -> "));
    }
    println!("\nCreate an editable copy: aurora workflow init <preset> [--output FILE]");
    0
}

pub fn init(name: &str, output: Option<&std::path::Path>) -> u8 {
    use aurora_core::presets::{PRESETS, get_preset};
    use std::io::Write;
    let Some(preset) = get_preset(name) else {
        let known: Vec<&str> = PRESETS.iter().map(|p| p.name).collect();
        return error(format!("Unknown preset '{name}'. Known presets: {}", known.join(", ")));
    };
    let path = output.map(std::path::Path::to_path_buf).unwrap_or_else(|| format!("{name}.toml").into());
    // create_new: refuse to overwrite an existing file (and never follow a pre-existing symlink).
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&path);
    let mut file = match file {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return error(format!("'{}' already exists; choose another --output", path.display()));
        }
        Err(e) => return error(format!("cannot create '{}': {e}", path.display())),
    };
    if let Err(e) = file.write_all(preset.toml.as_bytes()).and_then(|()| file.sync_all()) {
        return error(format!("cannot write '{}': {e}", path.display()));
    }
    println!("{} {} -> {}", paint(OK, "Wrote preset"), preset.name, path.display());
    println!(
        "Next: aurora workflow plan {}   (then: aurora workflow start {} --run-root <dir>)",
        path.display(),
        path.display()
    );
    0
}
