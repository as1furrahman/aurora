//! Mutation preconditions: protected projects, global agent skill directories, sensitive
//! system paths, trusted roots, and parent ownership/permissions. Port of
//! `awb/safety/boundaries.py`. The policy is a value (not globals) so tests can inject
//! fake locations instead of touching the real home directory.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::FsError;
use crate::paths::{is_within, resolve, validate_path};

#[derive(Debug, Clone)]
pub struct BoundaryPolicy {
    pub home: PathBuf,
    /// Must never be targeted, contained, or contain a target (SEC-04).
    pub protected_projects: Vec<PathBuf>,
    /// Real agent skill directories; blocked unless `allow_global` (Gate C).
    pub global_agent_dirs: Vec<PathBuf>,
    pub sensitive: Vec<PathBuf>,
    /// Roots too broad to serve as trusted boundaries (the home directory is also refused).
    pub broad_roots: Vec<PathBuf>,
}

impl BoundaryPolicy {
    /// Policy for the current user (`$HOME`).
    pub fn host() -> Self {
        BoundaryPolicy::for_home(&std::env::home_dir().unwrap_or_else(|| PathBuf::from("/nonexistent-home")))
    }

    /// Policy for an explicit home directory. The protected-project list matches the Python
    /// implementation (it is host-specific; see the Stage 0 audit, finding F4).
    pub fn for_home(home: &Path) -> Self {
        BoundaryPolicy {
            home: home.to_path_buf(),
            protected_projects: vec![
                PathBuf::from("/home/asif/projects/token-efficient-workflow"),
                PathBuf::from("/home/asif/projects/dwl"),
                PathBuf::from("/home/asif/projects/dwl-native-bar-lab"),
            ],
            global_agent_dirs: [
                ".codex/skills",
                ".gemini/antigravity-cli/skills",
                ".gemini/config/skills",
                ".claude/skills",
                ".agents/skills",
            ]
            .iter()
            .map(|d| home.join(d))
            .collect(),
            sensitive: vec![
                home.join(".ssh"),
                home.join(".gnupg"),
                PathBuf::from("/etc"),
                PathBuf::from("/root"),
                PathBuf::from("/boot"),
                PathBuf::from("/sys"),
                PathBuf::from("/proc"),
            ],
            broad_roots: vec![PathBuf::from("/"), PathBuf::from("/home")],
        }
    }
}

fn related(a: &Path, b: &Path) -> bool {
    is_within(a, b) || is_within(b, a)
}

/// Validate filesystem preconditions before any mutation. Returns the canonical target.
pub fn validate_security_preconditions(
    target: &Path,
    allowed_roots: &[&Path],
    allow_global: bool,
    policy: &BoundaryPolicy,
) -> Result<PathBuf, FsError> {
    let resolved = resolve(target);
    let shown = target.display();

    // 1. Never `.system/` (vendor-managed skills).
    let has_system = |p: &Path| p.components().any(|c| c.as_os_str() == ".system");
    if has_system(&resolved) || has_system(target) {
        return Err(FsError::Safety(format!("Target path cannot be or reside within protected '.system/': '{shown}'")));
    }

    // 2. Protected projects (SEC-04).
    for prot in &policy.protected_projects {
        let canon = resolve(prot);
        if related(&resolved, &canon) {
            return Err(FsError::Safety(format!(
                "Target path '{shown}' conflicts with protected project directory '{}'. Target cannot equal, \
                 contain, or reside within protected projects.",
                canon.display()
            )));
        }
    }

    // 3. Real global agent skill directories (Gate C).
    if !allow_global {
        for dir in &policy.global_agent_dirs {
            if related(&resolved, &resolve(dir)) {
                return Err(FsError::GlobalMutationBlocked(format!(
                    "Direct mutation of global agent directory '{}' is blocked pending separate operational \
                     authorization. Use an isolated test directory or sandbox.",
                    resolved.display()
                )));
            }
        }
    }

    // 4. Sensitive system locations.
    for sens in &policy.sensitive {
        let canon = resolve(sens);
        if related(&resolved, &canon) {
            return Err(FsError::Safety(format!(
                "Target path '{shown}' conflicts with sensitive system location '{}'.",
                canon.display()
            )));
        }
    }

    // 5. Trusted roots (SEC-04).
    if allowed_roots.is_empty() {
        return Err(FsError::SecurityPrecondition(
            "No trusted sandbox root provided. Allowed roots list cannot be empty.".into(),
        ));
    }
    let home = resolve(&policy.home);
    let mut canon_roots = Vec::new();
    for root in allowed_roots {
        let canon = resolve(root);
        if policy.broad_roots.iter().any(|b| resolve(b) == canon) {
            return Err(FsError::Safety(format!(
                "Insecure boundary: allowed root '{}' is overly broad. Roots must be specific sandbox or project \
                 directories.",
                canon.display()
            )));
        }
        if canon == home && !allow_global {
            return Err(FsError::Safety(format!(
                "Insecure boundary: user home '{}' is overly broad as an allowed root. Roots must be specific \
                 sandbox directories.",
                canon.display()
            )));
        }
        for prot in &policy.protected_projects {
            let canon_prot = resolve(prot);
            if related(&canon, &canon_prot) {
                return Err(FsError::Safety(format!(
                    "Insecure boundary: allowed root '{}' conflicts with protected project directory '{}'.",
                    canon.display(),
                    canon_prot.display()
                )));
            }
        }
        canon_roots.push(canon);
    }

    // 6. Containment, traversal and ancestor symlinks.
    let root_refs: Vec<&Path> = canon_roots.iter().map(PathBuf::as_path).collect();
    let canon_target = validate_path(target, &root_refs, false)?;

    // 7. Ownership and permissions of the deepest existing ancestor (SEC-07). A directory
    // writable by group or others must carry the sticky bit.
    let mut current = canon_target.as_path();
    while !current.exists() {
        match current.parent() {
            Some(parent) => current = parent,
            None => break,
        }
    }
    if let Ok(meta) = std::fs::metadata(current) {
        let uid = rustix::process::getuid().as_raw();
        if meta.uid() != uid {
            return Err(FsError::SecurityPrecondition(format!(
                "Insecure ownership: target parent '{}' is owned by UID {}, expected current user UID {uid}",
                current.display(),
                meta.uid()
            )));
        }
        let mode = meta.mode();
        let world = mode & 0o002 != 0;
        let group = mode & 0o020 != 0;
        if (world || group) && mode & 0o1000 == 0 {
            return Err(FsError::SecurityPrecondition(format!(
                "Insecure permissions: target directory '{}' is {} without sticky bit (mode {mode:#o}). Parent \
                 directories must not be writable by group or others unless the sticky bit (0o1000) is set.",
                current.display(),
                if world { "world-writable" } else { "group-writable" }
            )));
        }
    }
    Ok(canon_target)
}
