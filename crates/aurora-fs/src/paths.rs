//! Path canonicalisation and boundary containment (port of `SafeFS.validate_path`).

use std::path::{Component, Path, PathBuf};

use crate::FsError;

/// `~` / `~/…` expansion against `$HOME`, as Python's `os.path.expanduser`.
pub fn expand_tilde(p: &Path) -> PathBuf {
    let Ok(rest) = p.strip_prefix("~") else { return p.to_path_buf() };
    match std::env::home_dir() {
        Some(home) => home.join(rest),
        None => p.to_path_buf(),
    }
}

/// Absolute, lexically normalised path (`os.path.abspath`): resolves `.` and `..` without
/// touching the filesystem.
pub fn absolute(p: &Path) -> PathBuf {
    let p = expand_tilde(p);
    let joined = if p.is_absolute() { p } else { std::env::current_dir().map(|cwd| cwd.join(&p)).unwrap_or(p) };
    let mut out = PathBuf::from("/");
    for comp in joined.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(part) => out.push(part),
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
        }
    }
    out
}

/// `os.path.realpath` for paths that may not exist yet: canonicalise the longest existing
/// prefix (resolving symlinks) and append the remaining components unchanged.
pub fn resolve(p: &Path) -> PathBuf {
    let abs = absolute(p);
    let mut existing = abs.clone();
    let mut tail = Vec::new();
    loop {
        if let Ok(canon) = existing.canonicalize() {
            let mut out = canon;
            for part in tail.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (existing.file_name().map(|n| n.to_os_string()), existing.parent().map(Path::to_path_buf)) {
            (Some(name), Some(parent)) => {
                tail.push(name);
                existing = parent;
            }
            _ => return abs,
        }
    }
}

/// True when `path` equals `root` or lies beneath it (component-wise, so `/a/run-x` is not
/// inside `/a/run`).
pub fn is_within(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
}

fn is_symlink(p: &Path) -> bool {
    p.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink())
}

/// Validate that `target` resolves strictly within one of `allowed_roots`, rejecting
/// traversal escapes and (unless `allow_symlinks`) symlinked leaves or ancestors up to the root.
/// Returns the canonical target.
pub fn validate_path(target: &Path, allowed_roots: &[&Path], allow_symlinks: bool) -> Result<PathBuf, FsError> {
    let abs_target = absolute(target);
    let canon_target = resolve(&abs_target);
    let canon_roots: Vec<PathBuf> = allowed_roots.iter().map(|r| resolve(r)).collect();

    if !canon_roots.iter().any(|root| is_within(&canon_target, root)) {
        let roots: Vec<String> = canon_roots.iter().map(|r| format!("'{}'", r.display())).collect();
        return Err(FsError::PathTraversal(format!(
            "Path traversal rejected: '{}' resolves to '{}' which is outside allowed boundaries: [{}]",
            target.display(),
            canon_target.display(),
            roots.join(", ")
        )));
    }

    if !allow_symlinks {
        if is_symlink(&abs_target) {
            return Err(FsError::SymlinkAncestor(format!(
                "Target path is an unauthorized symlink: '{}'",
                abs_target.display()
            )));
        }
        let mut current = abs_target.as_path();
        while let Some(parent) = current.parent() {
            if is_symlink(parent) {
                return Err(FsError::SymlinkAncestor(format!(
                    "Ancestor directory is an unauthorized symlink: '{}' (target: '{}')",
                    parent.display(),
                    target.display()
                )));
            }
            // Bounded traversal: stop at the canonical allowed root.
            if canon_roots.iter().any(|r| r == parent) {
                break;
            }
            current = parent;
        }
    }
    Ok(canon_target)
}

/// Open an absolute directory by walking it from `/` one component at a time with
/// `O_DIRECTORY|O_NOFOLLOW`, so no component — intermediate or final — can be a symlink, even
/// one swapped in after validation. Operations then run relative to the returned descriptor.
pub fn open_dir_nofollow(dir: &Path) -> Result<rustix::fd::OwnedFd, FsError> {
    use rustix::fs::{Mode, OFlags};
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let refuse = |e: rustix::io::Errno| {
        if e == rustix::io::Errno::LOOP || e == rustix::io::Errno::NOTDIR {
            FsError::SymlinkAncestor(format!("'{}' has a symlinked or non-directory component: {e}", dir.display()))
        } else {
            FsError::Descriptor(format!("Cannot open directory '{}': {e}", dir.display()))
        }
    };
    if !dir.is_absolute() {
        return Err(FsError::Descriptor(format!("'{}' must be absolute", dir.display())));
    }
    let mut fd = rustix::fs::open("/", flags, Mode::empty()).map_err(refuse)?;
    for comp in dir.components() {
        match comp {
            Component::Normal(part) => fd = rustix::fs::openat(&fd, part, flags, Mode::empty()).map_err(refuse)?,
            Component::RootDir => {}
            _ => return Err(FsError::Descriptor(format!("'{}' is not normalised", dir.display()))),
        }
    }
    Ok(fd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_normalisation() {
        assert_eq!(absolute(Path::new("/a/b/../c/./d")), PathBuf::from("/a/c/d"));
        assert_eq!(absolute(Path::new("/../..")), PathBuf::from("/"));
    }

    #[test]
    fn containment_is_component_wise() {
        assert!(is_within(Path::new("/x/run/a"), Path::new("/x/run")));
        assert!(is_within(Path::new("/x/run"), Path::new("/x/run")));
        assert!(!is_within(Path::new("/x/run-evil"), Path::new("/x/run")));
    }

    #[test]
    fn resolve_handles_missing_tails() {
        let t = std::env::temp_dir().canonicalize().expect("tmp");
        assert_eq!(resolve(&t.join("no/such/path")), t.join("no/such/path"));
    }
}
