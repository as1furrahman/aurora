//! Directory-tree primitives for installing skills (R2 Step 4a).
//!
//! All work is relative to a [`Root`] descriptor. Relative paths are plain components (no `..`,
//! no absolute paths), every component is opened `O_NOFOLLOW|O_DIRECTORY`, files are created
//! `O_EXCL|O_NOFOLLOW`, and renames use `RENAME_NOREPLACE`, so nothing is ever written through
//! a symlink or over an existing destination. Trees are built in a staging directory beside the
//! destination and renamed into place: a destination is either complete or absent.

use std::io::Write;
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rustix::fs::{AtFlags, FileType, Mode, OFlags, RenameFlags};
use rustix::io::Errno;

use crate::FsError;
use crate::paths::{absolute, open_dir_nofollow};

/// Staging-directory prefix for trees being written.
pub const STAGE_PREFIX: &str = ".aurora_stage_";
static STAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

const DIR_FLAGS: OFlags = OFlags::RDONLY.union(OFlags::DIRECTORY).union(OFlags::NOFOLLOW).union(OFlags::CLOEXEC);

/// A file to create inside a new tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFile {
    /// `/`-separated path relative to the tree.
    pub path: String,
    pub content: Vec<u8>,
    pub executable: bool,
}

/// What is at a relative path, without following a final symlink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Absent,
    Dir,
    File,
    Symlink,
    Other,
}

/// An opened directory that all operations are relative to.
#[derive(Debug)]
pub struct Root {
    fd: OwnedFd,
    path: PathBuf,
}

fn refuse(what: &str, e: Errno) -> FsError {
    if e == Errno::LOOP || e == Errno::NOTDIR {
        FsError::SymlinkAncestor(format!("'{what}' is a symlink or not a directory"))
    } else {
        FsError::Descriptor(format!("'{what}': {e}"))
    }
}

/// Split a relative path into checked components (`""` is the root itself).
fn components(rel: &str) -> Result<Vec<&str>, FsError> {
    if rel.is_empty() {
        return Ok(Vec::new());
    }
    let parts: Vec<&str> = rel.split('/').collect();
    if rel.starts_with('/') || parts.iter().any(|p| p.is_empty() || *p == "." || *p == ".." || p.contains('\0')) {
        return Err(FsError::PathTraversal(format!("'{rel}' must be a plain relative path")));
    }
    Ok(parts)
}

fn split_parent(rel: &str) -> Result<(Vec<&str>, &str), FsError> {
    let mut parts = components(rel)?;
    let name = parts.pop().ok_or_else(|| FsError::PathTraversal("an empty path has no name".into()))?;
    Ok((parts, name))
}

impl Root {
    /// Open `path` as a root. Its ancestors are canonicalized (they are the user's environment);
    /// the directory itself must not be a symlink.
    pub fn open(path: &Path) -> Result<Root, FsError> {
        let abs = absolute(path);
        let (Some(parent), Some(name)) = (abs.parent(), abs.file_name()) else {
            return Err(FsError::Descriptor(format!("'{}' cannot be a root", abs.display())));
        };
        let parent =
            std::fs::canonicalize(parent).map_err(|e| FsError::io(format!("resolve {}", parent.display()), e))?;
        let parent_fd = open_dir_nofollow(&parent)?;
        let fd = rustix::fs::openat(&parent_fd, name, DIR_FLAGS, Mode::empty())
            .map_err(|e| refuse(&abs.display().to_string(), e))?;
        Ok(Root { fd, path: parent.join(name) })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Open `parts` below the root one component at a time; on failure, the failing prefix and errno.
    fn walk_raw(&self, parts: &[&str], create: bool) -> Result<OwnedFd, (String, Errno)> {
        let mut fd = rustix::fs::openat(&self.fd, ".", DIR_FLAGS, Mode::empty()).map_err(|e| (".".to_string(), e))?;
        for (i, part) in parts.iter().enumerate() {
            let shown = || parts[..=i].join("/");
            if create {
                match rustix::fs::mkdirat(&fd, *part, Mode::from_raw_mode(0o755)) {
                    Ok(()) | Err(Errno::EXIST) => {}
                    Err(e) => return Err((shown(), e)),
                }
            }
            fd = rustix::fs::openat(&fd, *part, DIR_FLAGS, Mode::empty()).map_err(|e| (shown(), e))?;
        }
        Ok(fd)
    }

    fn walk(&self, parts: &[&str], create: bool) -> Result<OwnedFd, FsError> {
        self.walk_raw(parts, create).map_err(|(what, e)| refuse(&what, e))
    }

    /// `mkdir -p rel`, refusing any component that is a symlink or not a directory.
    pub fn ensure_dir(&self, rel: &str) -> Result<(), FsError> {
        self.walk(&components(rel)?, true).map(drop)
    }

    /// What is at `rel` (a missing parent means `Absent`; a symlinked parent is an error).
    pub fn entry(&self, rel: &str) -> Result<Entry, FsError> {
        let (parents, name) = split_parent(rel)?;
        let dir = match self.walk_raw(&parents, false) {
            Ok(fd) => fd,
            Err((_, Errno::NOENT)) => return Ok(Entry::Absent),
            Err((what, e)) => return Err(refuse(&what, e)),
        };
        match rustix::fs::statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW) {
            Err(Errno::NOENT) => Ok(Entry::Absent),
            Err(e) => Err(FsError::Descriptor(format!("stat '{rel}': {e}"))),
            Ok(st) => Ok(match FileType::from_raw_mode(st.st_mode) {
                FileType::Directory => Entry::Dir,
                FileType::RegularFile => Entry::File,
                FileType::Symlink => Entry::Symlink,
                _ => Entry::Other,
            }),
        }
    }

    /// Create the directory `parent_rel/name` holding `files`, atomically: it is staged beside
    /// the destination and renamed into place, and an existing destination is never replaced.
    pub fn write_tree(&self, parent_rel: &str, name: &str, files: &[NewFile]) -> Result<(), FsError> {
        if components(name)?.len() != 1 {
            return Err(FsError::PathTraversal(format!("'{name}' must be a single path component")));
        }
        for f in files {
            components(&f.path)?;
        }
        let parent = self.walk(&components(parent_rel)?, false)?;
        if !matches!(rustix::fs::statat(&parent, name, AtFlags::SYMLINK_NOFOLLOW), Err(Errno::NOENT)) {
            return Err(FsError::Safety(format!("'{parent_rel}/{name}' already exists; it is not replaced")));
        }
        let stage =
            format!("{STAGE_PREFIX}{name}_{}_{}", std::process::id(), STAGE_COUNTER.fetch_add(1, Ordering::Relaxed));
        rustix::fs::mkdirat(&parent, stage.as_str(), Mode::from_raw_mode(0o755))
            .map_err(|e| FsError::Descriptor(format!("mkdir staging: {e}")))?;
        let result = fill(&parent, &stage, files).and_then(|()| {
            rustix::fs::renameat_with(&parent, stage.as_str(), &parent, name, RenameFlags::NOREPLACE)
                .map_err(|e| FsError::Safety(format!("cannot place '{parent_rel}/{name}': {e}")))?;
            rustix::fs::fsync(&parent).map_err(|e| FsError::io("fsync parent", e))
        });
        if result.is_err() {
            // Our own staging directory, created above.
            let _ = remove_tree(&parent, &stage);
        }
        result
    }

    /// Move `from` to `to` inside the root; `to` must not exist.
    pub fn rename(&self, from: &str, to: &str) -> Result<(), FsError> {
        let (fp, fname) = split_parent(from)?;
        let (tp, tname) = split_parent(to)?;
        let from_dir = self.walk(&fp, false)?;
        let to_dir = self.walk(&tp, false)?;
        rustix::fs::renameat_with(&from_dir, fname, &to_dir, tname, RenameFlags::NOREPLACE)
            .map_err(|e| FsError::Safety(format!("cannot move '{from}' to '{to}': {e}")))?;
        rustix::fs::fsync(&to_dir).map_err(|e| FsError::io("fsync", e))?;
        rustix::fs::fsync(&from_dir).map_err(|e| FsError::io("fsync", e))
    }
}

fn fill(parent: &OwnedFd, stage: &str, files: &[NewFile]) -> Result<(), FsError> {
    let root = rustix::fs::openat(parent, stage, DIR_FLAGS, Mode::empty()).map_err(|e| refuse(stage, e))?;
    for f in files {
        let (dirs, name) = split_parent(&f.path)?;
        let mut dir = rustix::fs::openat(&root, ".", DIR_FLAGS, Mode::empty()).map_err(|e| refuse(stage, e))?;
        for d in dirs {
            match rustix::fs::mkdirat(&dir, d, Mode::from_raw_mode(0o755)) {
                Ok(()) | Err(Errno::EXIST) => {}
                Err(e) => return Err(FsError::Descriptor(format!("mkdir '{}': {e}", f.path))),
            }
            dir = rustix::fs::openat(&dir, d, DIR_FLAGS, Mode::empty()).map_err(|e| refuse(&f.path, e))?;
        }
        let mode = if f.executable { 0o755 } else { 0o644 };
        let fd = rustix::fs::openat(
            &dir,
            name,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(mode),
        )
        .map_err(|e| FsError::Descriptor(format!("create '{}': {e}", f.path)))?;
        rustix::fs::fchmod(&fd, Mode::from_raw_mode(mode)).map_err(|e| FsError::io("fchmod", e))?;
        let mut file = std::fs::File::from(fd);
        file.write_all(&f.content).map_err(|e| FsError::io(format!("write '{}'", f.path), e))?;
        file.sync_all().map_err(|e| FsError::io(format!("fsync '{}'", f.path), e))?;
    }
    rustix::fs::fsync(&root).map_err(|e| FsError::io("fsync staging", e))
}

/// Remove a directory tree Aurora created itself (a failed staging directory), fd-relative and
/// without following symlinks.
fn remove_tree(parent: &OwnedFd, name: &str) -> Result<(), FsError> {
    let dir = rustix::fs::openat(parent, name, DIR_FLAGS, Mode::empty()).map_err(|e| refuse(name, e))?;
    let mut names = Vec::new();
    for entry in rustix::fs::Dir::read_from(&dir).map_err(|e| FsError::io("readdir", e))? {
        let entry = entry.map_err(|e| FsError::io("readdir", e))?;
        let n = entry.file_name().to_string_lossy().into_owned();
        if n != "." && n != ".." {
            names.push(n);
        }
    }
    for n in names {
        let st = rustix::fs::statat(&dir, n.as_str(), AtFlags::SYMLINK_NOFOLLOW).map_err(|e| FsError::io("stat", e))?;
        if FileType::from_raw_mode(st.st_mode) == FileType::Directory {
            remove_tree(&dir, &n)?;
        } else {
            rustix::fs::unlinkat(&dir, n.as_str(), AtFlags::empty()).map_err(|e| FsError::io("unlink", e))?;
        }
    }
    rustix::fs::unlinkat(parent, name, AtFlags::REMOVEDIR).map_err(|e| FsError::io("rmdir", e))
}
