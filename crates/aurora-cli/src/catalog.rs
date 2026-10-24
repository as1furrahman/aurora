//! `aurora catalog list|show|recommend`: the curated skill catalog. Read-only: nothing is
//! fetched or installed.

use std::path::PathBuf;

use anstream::{eprintln, println};
use aurora_core::catalog::{Catalog, Entry, Kind, RecommendRequest, Recommendation, builtin_catalog};
use aurora_core::presets::{PRESETS, get_preset};
use aurora_core::workflow::{WorkflowDefinition, parse_workflow_bytes, parse_workflow_str};
use clap::Args;
use serde_json::json;

use crate::style::{BAD, DIM, HEAD, WARN, paint};

#[derive(Debug, Args)]
pub struct RecommendArgs {
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
    /// Output in JSON format
    #[arg(long)]
    json: bool,
}

fn error(message: impl std::fmt::Display) -> u8 {
    eprintln!("{} {message}", paint(BAD, "Error:"));
    1
}

fn print_json(value: &impl serde::Serialize) {
    println!("{}", serde_json::to_string_pretty(value).unwrap_or_default());
}

fn load() -> Result<Catalog, u8> {
    builtin_catalog().map_err(error)
}

fn entry_json(cat: &Catalog, e: &Entry) -> serde_json::Value {
    let src = cat.source(&e.source);
    json!({
        "id": e.id,
        "kind": e.kind,
        "tier": e.tier,
        "trust": src.map(|s| s.trust),
        "roles": e.roles,
        "stages": e.stages,
        "repo": src.map(|s| s.repo.as_str()),
        "rev": src.map(|s| s.rev.as_str()),
        "path": e.path,
        "license": cat.license_of(e),
        "flags": e.flags,
        "conflicts_with": e.conflicts_with,
        "description_chars": e.description_chars,
        "why": e.why,
    })
}

fn or_all(items: &[String]) -> String {
    if items.is_empty() { "all".to_string() } else { items.join(", ") }
}

pub fn list(as_json: bool) -> u8 {
    let cat = match load() {
        Ok(cat) => cat,
        Err(code) => return code,
    };
    if as_json {
        print_json(&cat.entries().iter().map(|e| entry_json(&cat, e)).collect::<Vec<_>>());
        return 0;
    }
    println!("{}", paint(HEAD, format!("Aurora — Skill Catalog (verified {})", cat.verified_on())));
    println!("{}", "-".repeat(65));
    println!(" {:<32} {:<10} {:<9} {:<10} roles", "entry", "kind", "tier", "trust");
    for e in cat.entries() {
        let trust = cat.source(&e.source).map(|s| s.trust.to_string()).unwrap_or_default();
        let trust = if trust == "community" { paint(WARN, format!("{trust:<10}")) } else { format!("{trust:<10}") };
        println!(" {:<32} {:<10} {:<9} {trust} {}", e.id, e.kind.to_string(), e.tier.to_string(), or_all(&e.roles));
    }
    println!("\n{}", paint(DIM, "Details: aurora catalog show <entry>. Nothing is installed by these commands."));
    0
}

pub fn show(id: &str, as_json: bool) -> u8 {
    let cat = match load() {
        Ok(cat) => cat,
        Err(code) => return code,
    };
    let Some(e) = cat.get(id) else {
        return error(format!("unknown catalog entry '{id}' (see `aurora catalog list`)"));
    };
    if as_json {
        print_json(&entry_json(&cat, e));
        return 0;
    }
    let src = cat.source(&e.source);
    println!(
        "{}",
        paint(
            HEAD,
            format!("{} ({} · {} · {})", e.id, e.kind, e.tier, src.map(|s| s.trust.to_string()).unwrap_or_default())
        )
    );
    println!("  {}", e.why);
    if let Some(s) = src {
        // A browsable link to exactly the pinned files.
        match &e.path {
            Some(path) => println!("  source:  {}/tree/{}/{path}", s.repo, s.rev),
            None => println!("  source:  {} (rev {})", s.repo, s.rev),
        }
        if let Some(n) = &s.note {
            println!("  note:    {n}");
        }
    }
    println!("  license: {}", cat.license_of(e));
    println!("  roles:   {}   stages: {}", or_all(&e.roles), or_all(&e.stages));
    if !e.flags.is_empty() {
        let flags: Vec<String> = e.flags.iter().map(ToString::to_string).collect();
        println!("  flags:   {}", paint(WARN, flags.join(", ")));
    }
    if !e.conflicts_with.is_empty() {
        println!("  conflicts with: {}", e.conflicts_with.join(", "));
    }
    if matches!(e.kind, Kind::Skill | Kind::Rules) {
        println!("  description: {} characters in each CLI's skill listing", e.description_chars);
    } else {
        println!("  {}", paint(DIM, "Not installed by Aurora: set it up with its own tooling if you adopt it."));
    }
    0
}

fn workflow(args: &RecommendArgs) -> Result<WorkflowDefinition, String> {
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

pub fn recommend(args: &RecommendArgs) -> u8 {
    let cat = match load() {
        Ok(cat) => cat,
        Err(code) => return code,
    };
    let wf = match workflow(args) {
        Ok(wf) => wf,
        Err(e) => return error(e),
    };
    let profile = args.profile.clone().unwrap_or_else(|| {
        match args.clis.len() {
            1 => "solo",
            3 => "trio",
            _ => "pair",
        }
        .to_string()
    });
    let req = RecommendRequest { profile, clis: args.clis.clone(), with: args.with.clone() };
    let rec = match cat.recommend(&wf, &req) {
        Ok(rec) => rec,
        Err(e) => return error(e),
    };
    if args.json {
        print_json(&rec);
    } else {
        print_recommendation(&cat, &rec);
    }
    0
}

fn print_recommendation(cat: &Catalog, rec: &Recommendation) {
    let profile = cat.profile(&rec.profile).map(|p| p.description.as_str()).unwrap_or_default();
    println!(
        "{}",
        paint(
            HEAD,
            format!("Aurora — Skills for '{}' ({} profile: {})", rec.workflow, rec.profile, rec.clis.join(" + "))
        )
    );
    println!("  {}", paint(DIM, profile));
    println!("{}", "-".repeat(65));
    println!("Always-on rules:");
    for r in &rec.rules {
        let files: Vec<String> = r.files.iter().map(|(cli, file)| format!("{cli} → {file}")).collect();
        println!("  {}: {}", r.entry, files.join(", "));
    }
    println!("\nStages:");
    for s in &rec.stages {
        println!("  {:<14} {:<12} {:<12} {}", s.stage, s.role, s.provider, s.skills.join(", "));
    }
    println!("\nSkill listing budget per CLI:");
    for p in &rec.providers {
        let line = format!(
            "  {:<12} {:>2} skills, ~{} of {} chars",
            p.provider,
            p.skills.len(),
            p.listing_chars,
            p.budget_chars
        );
        println!("{}", if p.listing_chars > p.budget_chars { paint(BAD, line) } else { line });
    }
    if !rec.warnings.is_empty() {
        println!("\nWarnings:");
        for w in &rec.warnings {
            println!("  {} {w}", paint(WARN, "!"));
        }
    }
    println!("\n{}", paint(DIM, "Recommendation only: nothing was fetched or installed."));
}
