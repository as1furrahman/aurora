//! Safe collection of a skill directory (R2 Step 2a) and its timestamp-free content hash.
//!
//! The skill root's ancestors are canonicalized (they are the user's environment; on Fedora
//! Atomic `/home` itself is a symlink), then the root and everything below it are opened
//! fd-relative with `O_NOFOLLOW`: a symlink is recorded, never followed; FIFOs and devices are
//! recorded, never opened; files over the size limit are recorded, never read.

use std::io::Read;
use std::os::fd::OwnedFd;
use std::path::Path;

use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags};
use sha2::{Digest, Sha256};

/// Collection limits; anything beyond them is reported by the scanner as `Blocked`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        // The largest catalogued skill (trailofbits/sharp-edges) is ~30 files and well under 1 MiB.
        Limits { max_files: 200, max_file_bytes: 1 << 20, max_total_bytes: 5 << 20 }
    }
}

const MAX_DEPTH: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Path relative to the skill root, `/`-separated.
    pub path: String,
    pub size: u64,
    pub executable: bool,
    /// `None` when the file was over a limit and deliberately not read.
    pub content: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SkillTree {
    /// Regular files, sorted by path.
    pub files: Vec<FileEntry>,
    pub symlinks: Vec<String>,
    /// FIFOs, sockets and devices.
    pub specials: Vec<String>,
    /// Directories nested deeper than the depth limit (not entered).
    pub too_deep: Vec<String>,
    pub max_file_bytes: u64,
    /// More regular files than `max_files`; the rest were not collected.
    pub too_many_files: bool,
    /// Reading stopped once `max_total_bytes` was reached.
    pub too_large: bool,
}

impl SkillTree {
    pub fn file(&self, path: &str) -> Option<&FileEntry> {
        self.files.iter().find(|f| f.path == path)
    }

    /// Lowercase hex SHA-256 over every entry, sorted by path: `path NUL kind NUL sha256(content)`.
    /// Independent of timestamps, ownership and directory location.
    pub fn content_hash(&self) -> String {
        let mut lines: Vec<String> = self
            .files
            .iter()
            .map(|f| {
                let kind = if f.executable { "x" } else { "f" };
                let digest = match &f.content {
                    Some(bytes) => hex(&Sha256::digest(bytes)),
                    None => format!("unread:{}", f.size),
                };
                format!("{}\0{kind}\0{digest}\n", f.path)
            })
            .chain(self.symlinks.iter().map(|p| format!("{p}\0l\0\n")))
            .chain(self.specials.iter().map(|p| format!("{p}\0s\0\n")))
            .collect();
        lines.sort();
        hex(&Sha256::digest(lines.concat().as_bytes()))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, thiserror::Error)]
pub enum TreeError {
    #[error("cannot open skill directory '{path}': {reason}")]
    Open { path: String, reason: String },
    #[error("cannot read '{path}': {reason}")]
    Read { path: String, reason: String },
}

fn open_root(dir: &Path) -> Result<OwnedFd, TreeError> {
    let fail = |reason: String| TreeError::Open { path: dir.display().to_string(), reason };
    let abs = aurora_fs::paths::absolute(dir);
    let (Some(parent), Some(name)) = (abs.parent(), abs.file_name()) else {
        return Err(fail("not a directory below a parent".into()));
    };
    let parent = std::fs::canonicalize(parent).map_err(|e| fail(e.to_string()))?;
    let parent_fd = aurora_fs::paths::open_dir_nofollow(&parent).map_err(|e| fail(e.to_string()))?;
    rustix::fs::openat(
        &parent_fd,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| {
        if e == rustix::io::Errno::LOOP || e == rustix::io::Errno::NOTDIR {
            fail("the skill directory is a symlink or not a directory".into())
        } else {
            fail(e.to_string())
        }
    })
}

/// Collect the skill rooted at `dir`.
pub fn collect(dir: &Path, limits: &Limits) -> Result<SkillTree, TreeError> {
    let root = open_root(dir)?;
    let mut tree = SkillTree { max_file_bytes: limits.max_file_bytes, ..SkillTree::default() };
    let mut total = 0u64;
    walk(&root, "", 0, limits, &mut tree, &mut total)?;
    tree.files.sort_by(|a, b| a.path.cmp(&b.path));
    tree.symlinks.sort();
    tree.specials.sort();
    Ok(tree)
}

fn walk(
    dir: &OwnedFd,
    prefix: &str,
    depth: usize,
    limits: &Limits,
    tree: &mut SkillTree,
    total: &mut u64,
) -> Result<(), TreeError> {
    let read_err = |path: &str, e: rustix::io::Errno| TreeError::Read { path: path.to_string(), reason: e.to_string() };
    let mut names = Vec::new();
    for entry in Dir::read_from(dir).map_err(|e| read_err(prefix, e))? {
        let entry = entry.map_err(|e| read_err(prefix, e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name != "." && name != ".." {
            names.push(name);
        }
    }
    names.sort();
    for name in names {
        let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let st = rustix::fs::statat(dir, name.as_str(), AtFlags::SYMLINK_NOFOLLOW).map_err(|e| read_err(&rel, e))?;
        match FileType::from_raw_mode(st.st_mode) {
            FileType::Symlink => tree.symlinks.push(rel),
            FileType::Directory if depth + 1 >= MAX_DEPTH => tree.too_deep.push(rel),
            FileType::Directory => {
                let sub = rustix::fs::openat(
                    dir,
                    name.as_str(),
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|e| read_err(&rel, e))?;
                walk(&sub, &rel, depth + 1, limits, tree, total)?;
            }
            FileType::RegularFile => {
                if tree.files.len() >= limits.max_files {
                    tree.too_many_files = true;
                    continue;
                }
                let size = st.st_size.max(0) as u64;
                let executable = st.st_mode & 0o111 != 0;
                let content = if size > limits.max_file_bytes || *total + size > limits.max_total_bytes {
                    tree.too_large |= size <= limits.max_file_bytes;
                    None
                } else {
                    *total += size;
                    Some(read_file(dir, &name, &rel, limits.max_file_bytes)?)
                };
                tree.files.push(FileEntry { path: rel, size, executable, content });
            }
            _ => tree.specials.push(rel),
        }
    }
    Ok(())
}

fn read_file(dir: &OwnedFd, name: &str, rel: &str, max: u64) -> Result<Vec<u8>, TreeError> {
    let err = |reason: String| TreeError::Read { path: rel.to_string(), reason };
    let fd = rustix::fs::openat(
        dir,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| err(e.to_string()))?;
    let st = rustix::fs::fstat(&fd).map_err(|e| err(e.to_string()))?;
    if FileType::from_raw_mode(st.st_mode) != FileType::RegularFile {
        return Err(err("changed type while being read".into()));
    }
    let mut bytes = Vec::new();
    std::fs::File::from(fd).take(max + 1).read_to_end(&mut bytes).map_err(|e| err(e.to_string()))?;
    if bytes.len() as u64 > max {
        return Err(err("grew past the size limit while being read".into()));
    }
    Ok(bytes)
}
