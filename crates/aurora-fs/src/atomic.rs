//! Staged atomic writes (port of `SafeFS.safe_atomic_write`): stage in the same directory,
//! chmod, fsync, rename over the destination, fsync the directory. A reader never sees a
//! partially written file, and a failure at any step leaves the original untouched and no
//! staging file behind.

use std::fs::{File, Permissions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use rustix::fs::{FileType, Mode, OFlags};
use sha2::{Digest, Sha256};

use crate::FsError;
use crate::paths::validate_path;

/// Staging-file prefix, shared with the Python implementation (gitignored as `.awb_stage_*`).
pub const STAGE_PREFIX: &str = ".awb_stage_";

/// Failure-injection points, the test seam used by the Python suite's `_fault_at`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    AfterCreate,
    DuringWrite,
    BeforeFsync,
    BeforeReplace,
}

#[derive(Debug, Clone)]
pub struct WriteInfo {
    pub path: PathBuf,
    pub dev: u64,
    pub ino: u64,
    pub sha256: String,
    pub size: u64,
}

pub fn atomic_write(dest: &Path, content: &[u8], allowed_roots: &[&Path], mode: u32) -> Result<WriteInfo, FsError> {
    atomic_write_with_fault(dest, content, allowed_roots, mode, None)
}

fn injected(fault: Option<Fault>, at: Fault) -> Result<(), FsError> {
    if fault == Some(at) {
        return Err(FsError::io(format!("Fault injection: {at:?}"), std::io::Error::other("injected fault")));
    }
    Ok(())
}

#[doc(hidden)]
pub fn atomic_write_with_fault(
    dest: &Path,
    content: &[u8],
    allowed_roots: &[&Path],
    mode: u32,
    fault: Option<Fault>,
) -> Result<WriteInfo, FsError> {
    let canon = validate_path(dest, allowed_roots, false)?;
    let parent = canon.parent().ok_or_else(|| FsError::Descriptor(format!("'{}' has no parent", canon.display())))?;
    if !parent.is_dir() {
        return Err(FsError::Descriptor(format!("Parent directory '{}' does not exist", parent.display())));
    }

    // Pin and verify the parent directory before staging into it.
    let dir_fd = rustix::fs::open(
        parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| FsError::Descriptor(format!("Cannot open parent directory '{}': {e}", parent.display())))?;
    let st = rustix::fs::fstat(&dir_fd).map_err(|e| FsError::io("fstat parent", e))?;
    if FileType::from_raw_mode(st.st_mode) != FileType::Directory || st.st_uid != rustix::process::getuid().as_raw() {
        return Err(FsError::Descriptor(format!("Parent directory '{}' not a user-owned directory", parent.display())));
    }

    // NamedTempFile is created O_EXCL in the same directory (same filesystem) and is removed on
    // drop unless persisted, which covers every failure path below.
    let mut staged = tempfile::Builder::new()
        .prefix(STAGE_PREFIX)
        .tempfile_in(parent)
        .map_err(|e| FsError::io(format!("create staging file in '{}'", parent.display()), e))?;
    injected(fault, Fault::AfterCreate)?;
    if fault == Some(Fault::DuringWrite) {
        let _ = staged.write_all(&content[..content.len().min(1)]);
        injected(fault, Fault::DuringWrite)?;
    }
    staged.write_all(content).map_err(|e| FsError::io("write staging file", e))?;
    staged.as_file().set_permissions(Permissions::from_mode(mode)).map_err(|e| FsError::io("chmod staging file", e))?;
    injected(fault, Fault::BeforeFsync)?;
    staged.as_file().sync_all().map_err(|e| FsError::io("fsync staging file", e))?;
    let meta = staged.as_file().metadata().map_err(|e| FsError::io("fstat staging file", e))?;
    injected(fault, Fault::BeforeReplace)?;
    staged.persist(&canon).map_err(|e| FsError::io(format!("rename into '{}'", canon.display()), e.error))?;

    // Flush the directory entry; best effort, as in Python.
    if let Ok(dir) = File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(WriteInfo {
        path: canon,
        dev: meta.dev(),
        ino: meta.ino(),
        sha256: Sha256::digest(content).iter().map(|b| format!("{b:02x}")).collect(),
        size: content.len() as u64,
    })
}
