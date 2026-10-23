//! Catalog entries to verified, scanned skill trees.

use std::path::{Path, PathBuf};

use aurora_core::catalog::{Catalog, Kind, Trust, builtin_catalog, parse_catalog_str};
use aurora_skills::fetch::{FetchOptions, FetchRequest, fetch};
use aurora_skills::scan::{Readiness, ReviewState, ScanReport, readiness, scan};
use aurora_skills::tree::{Limits, SkillTree, collect};

/// The built-in catalog, or a custom one from `path` (validated the same way).
pub fn load_catalog(path: Option<&Path>) -> Result<Catalog, String> {
    match path {
        None => builtin_catalog().map_err(|e| e.to_string()),
        Some(p) => {
            let text = std::fs::read_to_string(p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
            parse_catalog_str(&text).map_err(|e| format!("{}: {e}", p.display()))
        }
    }
}

/// The fetch cache: `explicit`, else `$AURORA_CACHE_DIR`, `$XDG_CACHE_HOME/aurora/skills` or
/// `~/.cache/aurora/skills`.
pub fn resolve_cache(explicit: Option<&Path>) -> Result<PathBuf, String> {
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

/// Where a catalog entry is pinned, and what review covers it.
#[derive(Debug, Clone)]
pub struct Pin {
    pub repo: String,
    pub rev: String,
    pub path: String,
    pub expected: Option<String>,
    pub review: ReviewState,
}

/// Resolve an installable catalog entry to its pin.
pub fn pin_for_entry(catalog: &Catalog, id: &str) -> Result<Pin, String> {
    let e = catalog.get(id).ok_or_else(|| format!("unknown catalog entry '{id}' (see `aurora catalog list`)"))?;
    let src = catalog.source(&e.source).ok_or_else(|| format!("entry '{id}' has no source"))?;
    if matches!(e.kind, Kind::Framework | Kind::Infra) {
        return Err(format!("'{id}' is a {} and is not installed by Aurora; see {}", e.kind, src.repo));
    }
    let review = match (src.trust, &e.reviewed) {
        (Trust::Community, _) => ReviewState::Community,
        (_, Some(note)) => ReviewState::Catalog(note.clone()),
        (_, None) => ReviewState::None,
    };
    Ok(Pin {
        repo: src.repo.clone(),
        rev: src.rev.clone(),
        path: e.path.clone().unwrap_or_default(),
        expected: e.sha256.clone(),
        review,
    })
}

/// A fetched skill with its scan and readiness.
#[derive(Debug, Clone)]
pub struct Fetched {
    pub dir: PathBuf,
    pub tree: SkillTree,
    pub report: ScanReport,
    pub readiness: Readiness,
}

/// Fetch `pin` (through `mirror` if given), collect it without following symlinks, scan it and
/// decide readiness against the pinned hash.
pub fn fetch_checked(pin: &Pin, cache: &Path, mirror: Option<&Path>) -> Result<Fetched, String> {
    let mut opts = FetchOptions::default();
    let mut url = pin.repo.clone();
    if let Some(m) = mirror {
        let rest = pin
            .repo
            .strip_prefix("https://github.com/")
            .ok_or_else(|| format!("--mirror only maps https://github.com repositories, not '{}'", pin.repo))?;
        url = format!("file://{}/{rest}", aurora_fs::paths::absolute(m).display());
        opts.allow_file_urls = true;
    }
    let req = FetchRequest { repo: &url, rev: &pin.rev, path: &pin.path, cache_root: cache };
    let dir = fetch(&req, &opts).map_err(|e| format!("cannot fetch {}@{} {}: {e}", pin.repo, pin.rev, pin.path))?;
    let tree = collect(&dir, &Limits::default()).map_err(|e| e.to_string())?;
    let report = scan(&tree);
    let readiness = readiness(&tree, &report, pin.expected.as_deref(), pin.review.clone());
    Ok(Fetched { dir, tree, report, readiness })
}
