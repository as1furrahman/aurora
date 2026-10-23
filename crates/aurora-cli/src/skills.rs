//! `aurora skills inspect <dir>`: parse a SKILL.md, check it against the Agent Skills spec, and
//! report how it behaves on each CLI. `aurora skills scan <dir>`: hash every file and run the
//! static scanner. Both are read-only and never follow symlinks.

use std::collections::BTreeMap;
use std::path::Path;

use anstream::{eprintln, println};
use aurora_core::catalog::builtin_catalog;
use aurora_core::providers::PROVIDER_IDS;
use aurora_skills::scan::{Level, Verdict, scan as scan_tree};
use aurora_skills::skill::{
    MAX_SKILL_MD_BYTES, Severity, Support, check_spec, listing_chars, parse_skill, shell_injections, support,
};
use aurora_skills::tree::{Limits, collect};
use serde_json::json;

use crate::style::{BAD, DIM, HEAD, OK, WARN, paint};

fn error(message: impl std::fmt::Display) -> u8 {
    eprintln!("{} {message}", paint(BAD, "Error:"));
    1
}

pub fn inspect(dir: &Path, as_json: bool) -> u8 {
    let dir = aurora_fs::paths::absolute(dir);
    let file = dir.join("SKILL.md");
    let dir_name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let raw = match aurora_fs::read::safe_read(&file, &[dir.as_path()], MAX_SKILL_MD_BYTES as u64) {
        Ok(info) => info.content,
        Err(e) => return error(format!("cannot read {}: {e}", file.display())),
    };
    let Ok(text) = String::from_utf8(raw) else {
        return error(format!("{} is not valid UTF-8", file.display()));
    };
    let skill = match parse_skill(&text, &dir_name) {
        Ok(skill) => skill,
        Err(e) => return error(format!("{}: {e}", file.display())),
    };
    let catalog = match builtin_catalog() {
        Ok(cat) => cat,
        Err(e) => return error(e),
    };

    let issues = check_spec(&skill, &dir_name);
    let injections = shell_injections(&skill.body);
    let per_cli: BTreeMap<&str, (Support, usize)> = PROVIDER_IDS
        .iter()
        .map(|p| {
            let cap = catalog.target(p).and_then(|t| t.per_skill_chars);
            (*p, (support(&skill, p), listing_chars(&skill, p, cap)))
        })
        .collect();
    let failed = issues.iter().any(|i| i.severity == Severity::Error);

    if as_json {
        let out = json!({
            "name": skill.name,
            "dir": dir.display().to_string(),
            "description_chars": skill.description.chars().count(),
            "repaired": skill.repaired,
            "frontmatter_keys": skill.frontmatter.keys().collect::<Vec<_>>(),
            "issues": issues,
            "injections": injections,
            "support": per_cli.iter().map(|(p, (s, _))| (p.to_string(), json!(s))).collect::<serde_json::Map<_, _>>(),
            "listing_chars": per_cli.iter().map(|(p, (_, n))| (p.to_string(), json!(n))).collect::<serde_json::Map<_, _>>(),
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return u8::from(failed);
    }

    println!("{}", paint(HEAD, format!("{} ({})", skill.name, dir.display())));
    println!("  description: {} characters", skill.description.chars().count());
    if skill.repaired {
        println!("  {}", paint(WARN, "frontmatter needed repair (unquoted ': ' in a value); it parses, but quote it"));
    }
    println!("\nAgent Skills spec:");
    if issues.is_empty() {
        println!("  {} no issues", paint(OK, "✓"));
    }
    for i in &issues {
        let tag = match i.severity {
            Severity::Error => paint(BAD, "error  "),
            Severity::Warning => paint(WARN, "warning"),
        };
        println!("  {tag} {}", i.message);
    }
    println!("\nPer CLI (listing = characters added to the always-loaded skill list):");
    for (p, (s, n)) in &per_cli {
        match s {
            Support::Native => println!("  {p:<12} {:<10} listing ~{n}", paint(OK, "native")),
            Support::Degraded(reasons) => {
                println!("  {p:<12} {:<10} listing ~{n}", paint(WARN, "degraded"));
                for r in reasons {
                    println!("      - {r}");
                }
            }
        }
    }
    if !injections.is_empty() {
        println!("\nContext injections (Claude Code runs these when the skill loads, without a permission prompt):");
        for inj in &injections {
            println!("  line {}: {}", inj.line, inj.snippet);
        }
    }
    println!("\n{}", paint(DIM, "Inspection only: nothing was installed or executed."));
    u8::from(failed)
}

pub const EXIT_SCAN_BLOCKED: u8 = 3;
pub const EXIT_SCAN_REVIEW: u8 = 4;

pub fn scan(dir: &Path, as_json: bool) -> u8 {
    let tree = match collect(dir, &Limits::default()) {
        Ok(tree) => tree,
        Err(e) => return error(e),
    };
    let report = scan_tree(&tree);
    let hash = tree.content_hash();
    let code = match report.verdict {
        Verdict::Clean => 0,
        Verdict::NeedsReview => EXIT_SCAN_REVIEW,
        Verdict::Blocked => EXIT_SCAN_BLOCKED,
    };
    if as_json {
        let out = json!({
            "dir": aurora_fs::paths::absolute(dir).display().to_string(),
            "verdict": report.verdict,
            "sha256": hash,
            "files": tree.files.len(),
            "findings": report.findings,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return code;
    }
    let verdict = match report.verdict {
        Verdict::Clean => paint(OK, "clean"),
        Verdict::NeedsReview => paint(WARN, "needs review"),
        Verdict::Blocked => paint(BAD, "blocked"),
    };
    println!(
        "{}",
        paint(HEAD, format!("Scan: {} ({} files)", aurora_fs::paths::absolute(dir).display(), tree.files.len()))
    );
    println!("  verdict: {verdict}");
    println!("  sha256:  {hash}");
    for f in &report.findings {
        let tag = match f.level {
            Level::Block => paint(BAD, "block "),
            Level::Review => paint(WARN, "review"),
        };
        let at = match f.line {
            Some(line) => format!("{}:{line}", f.path),
            None => f.path.clone(),
        };
        println!("  {tag} {:<20} {at}  {}", f.rule, f.detail);
    }
    let note = match report.verdict {
        Verdict::Clean => "No findings. A clean scan is a signal, not a guarantee: read unfamiliar skills before use.",
        Verdict::NeedsReview => "Read every flagged file before installing; Aurora asks you to confirm the review.",
        Verdict::Blocked => "Aurora will not install this skill; blocked findings cannot be overridden.",
    };
    println!("\n{}", paint(DIM, note));
    code
}
