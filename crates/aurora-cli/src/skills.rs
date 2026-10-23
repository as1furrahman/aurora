//! `aurora skills inspect <dir>`: parse a SKILL.md, check it against the Agent Skills spec, and
//! report how it behaves on each CLI. `aurora skills scan <dir>`: hash every file and run the
//! static scanner. `aurora skills fetch`: download one skill at its pinned commit into the cache,
//! check it against the catalog's hash and decide whether it is ready to install. None of these
//! install anything, and none follow symlinks.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anstream::{eprintln, println};
use aurora_core::catalog::{Kind, Trust, builtin_catalog};
use aurora_core::providers::PROVIDER_IDS;
use aurora_skills::fetch::{FetchOptions, FetchRequest, fetch as fetch_skill};
use aurora_skills::scan::{Finding, Level, Readiness, ReviewState, Verdict, readiness, scan as scan_tree};
use aurora_skills::skill::{
    MAX_SKILL_MD_BYTES, Severity, Support, check_spec, listing_chars, parse_skill, shell_injections, support,
};
use aurora_skills::tree::{Limits, collect};
use clap::Args;
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
    print_findings(&report.findings);
    let note = match report.verdict {
        Verdict::Clean => "No findings. A clean scan is a signal, not a guarantee: read unfamiliar skills before use.",
        Verdict::NeedsReview => "Read every flagged file before installing; Aurora asks you to confirm the review.",
        Verdict::Blocked => "Aurora will not install this skill; blocked findings cannot be overridden.",
    };
    println!("\n{}", paint(DIM, note));
    code
}

fn print_findings(findings: &[Finding]) {
    for f in findings {
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
}

#[derive(Debug, Args)]
pub struct FetchArgs {
    /// Catalog entry id (or give --repo, --rev and --path)
    #[arg(required_unless_present = "repo", conflicts_with_all = ["repo", "rev", "path"])]
    entry: Option<String>,
    /// Repository, https://github.com/<owner>/<name>
    #[arg(long, requires_all = ["rev", "path"])]
    repo: Option<String>,
    /// Full commit SHA
    #[arg(long, requires = "repo")]
    rev: Option<String>,
    /// Skill directory inside the repository
    #[arg(long, requires = "repo")]
    path: Option<String>,
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

fn cache_dir(explicit: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(dir) = explicit {
        return Ok(aurora_fs::paths::absolute(dir));
    }
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(dir) = var("AURORA_CACHE_DIR") {
        return Ok(dir);
    }
    if let Some(dir) = var("XDG_CACHE_HOME") {
        return Ok(dir.join("aurora/skills"));
    }
    var("HOME")
        .map(|h| h.join(".cache/aurora/skills"))
        .ok_or_else(|| "cannot find a cache directory; pass --cache".into())
}

pub fn fetch(args: &FetchArgs) -> u8 {
    let catalog = match builtin_catalog() {
        Ok(cat) => cat,
        Err(e) => return error(e),
    };
    let (repo, rev, path, expected, review) = match &args.entry {
        Some(id) => {
            let Some(e) = catalog.get(id) else {
                return error(format!("unknown catalog entry '{id}' (see `aurora catalog list`)"));
            };
            let Some(src) = catalog.source(&e.source) else {
                return error(format!("entry '{id}' has no source"));
            };
            if matches!(e.kind, Kind::Framework | Kind::Infra) {
                return error(format!("'{id}' is a {} and is not installed by Aurora; see {}", e.kind, src.repo));
            }
            let review = match (src.trust, &e.reviewed) {
                (Trust::Community, _) => ReviewState::Community,
                (_, Some(note)) => ReviewState::Catalog(note.clone()),
                (_, None) => ReviewState::None,
            };
            (src.repo.clone(), src.rev.clone(), e.path.clone().unwrap_or_default(), e.sha256.clone(), review)
        }
        None => (
            args.repo.clone().unwrap_or_default(),
            args.rev.clone().unwrap_or_default(),
            args.path.clone().unwrap_or_default(),
            None,
            ReviewState::None,
        ),
    };
    let cache = match cache_dir(args.cache.as_deref()) {
        Ok(dir) => dir,
        Err(e) => return error(e),
    };
    let mut opts = FetchOptions::default();
    let mut url = repo.clone();
    if let Some(m) = &args.mirror {
        let Some(rest) = repo.strip_prefix("https://github.com/") else {
            return error(format!("--mirror only maps https://github.com repositories, not '{repo}'"));
        };
        url = format!("file://{}/{rest}", aurora_fs::paths::absolute(m).display());
        opts.allow_file_urls = true;
    }
    let req = FetchRequest { repo: &url, rev: &rev, path: &path, cache_root: &cache };
    let dir = match fetch_skill(&req, &opts) {
        Ok(dir) => dir,
        Err(e) => return error(format!("cannot fetch {repo}@{rev} {path}: {e}")),
    };
    let tree = match collect(&dir, &Limits::default()) {
        Ok(tree) => tree,
        Err(e) => return error(e),
    };
    let report = scan_tree(&tree);
    let hash = tree.content_hash();
    let ready = readiness(&tree, &report, expected.as_deref(), review);
    let code = match &ready {
        Readiness::Ready | Readiness::Reviewed(_) => 0,
        Readiness::NeedsReview(_) => EXIT_SCAN_REVIEW,
        Readiness::Blocked(_) => EXIT_SCAN_BLOCKED,
        Readiness::IntegrityMismatch { .. } => 1,
    };

    if args.json {
        let out = json!({
            "entry": args.entry,
            "repo": repo,
            "rev": rev,
            "path": path,
            "dir": dir.display().to_string(),
            "sha256": hash,
            "expected_sha256": expected,
            "verdict": report.verdict,
            "findings": report.findings,
            "readiness": ready,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return code;
    }
    let name = args.entry.clone().unwrap_or_else(|| path.clone());
    println!("{}", paint(HEAD, format!("Fetched {name}: {repo}@{} {path}", &rev[..12.min(rev.len())])));
    println!("  cache:   {}", dir.display());
    let pin = match &expected {
        Some(e) if *e == hash => paint(OK, "matches the catalog pin"),
        Some(_) => paint(BAD, "DIFFERS from the catalog pin"),
        None => paint(DIM, "not pinned in the catalog"),
    };
    println!("  sha256:  {hash} ({pin})");
    print_findings(&report.findings);
    let line = match &ready {
        Readiness::Ready => paint(OK, "ready to install"),
        Readiness::Reviewed(note) => {
            paint(OK, format!("ready to install; flagged files were reviewed for the catalog: {note}"))
        }
        Readiness::NeedsReview(paths) => {
            paint(WARN, format!("needs review: read {} before installing", paths.join(", ")))
        }
        Readiness::Blocked(rules) => paint(BAD, format!("blocked ({}); Aurora will not install it", rules.join(", "))),
        Readiness::IntegrityMismatch { expected, .. } => {
            paint(BAD, format!("integrity failure: the catalog pinned {expected}; nothing will be installed"))
        }
    };
    println!("\n  {line}");
    code
}
