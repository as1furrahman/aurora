//! Staged atomic writes (port of `SafeFS.safe_atomic_write`): stage in the same directory,
//! chmod, fsync, rename over the destination, fsync the directory. A reader never sees a
//! partially written file, and a failure at any step leaves the original untouched and no
//! staging file behind.
//!
//! Every step runs relative to the parent directory descriptor, opened by a component-wise
//! `O_NOFOLLOW` walk (`openat` / `renameat` / `unlinkat`), so a directory swapped for a symlink
//! after validation cannot redirect the write.

use std::fs::File;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rustix::fd::OwnedFd;
use rustix::fs::{AtFlags, FileType, Mode, OFlags};
use sha2::{Digest, Sha256};

use crate::FsError;
use crate::paths::{open_dir_nofollow, validate_path};

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

/// Create a fresh `O_EXCL|O_NOFOLLOW` staging file inside `dir`; returns its name and descriptor.
fn create_staging(dir: &OwnedFd) -> Result<(String, OwnedFd), FsError> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    for _ in 0..64 {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let name =
            format!("{STAGE_PREFIX}{}_{nanos:x}_{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
        match rustix::fs::openat(dir, name.as_str(), flags, Mode::from(0o600)) {
            Ok(fd) => return Ok((name, fd)),
            Err(rustix::io::Errno::EXIST) => continue,
            Err(e) => return Err(FsError::io("create staging file", e)),
        }
    }
    Err(FsError::Descriptor("could not create a unique staging file".into()))
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
    let name = canon.file_name().ok_or_else(|| FsError::Descriptor(format!("'{}' has no name", canon.display())))?;
    if !parent.is_dir() {
        return Err(FsError::Descriptor(format!("Parent directory '{}' does not exist", parent.display())));
    }

    // Pin and verify the parent directory; everything below is relative to it.
    let dir = open_dir_nofollow(parent)?;
    let st = rustix::fs::fstat(&dir).map_err(|e| FsError::io("fstat parent", e))?;
    if FileType::from_raw_mode(st.st_mode) != FileType::Directory || st.st_uid != rustix::process::getuid().as_raw() {
        return Err(FsError::Descriptor(format!("Parent directory '{}' not a user-owned directory", parent.display())));
    }

    let (stage_name, fd) = create_staging(&dir)?;
    let mut staged = File::from(fd);
    let result = (|| -> Result<WriteInfo, FsError> {
        injected(fault, Fault::AfterCreate)?;
        if fault == Some(Fault::DuringWrite) {
            let _ = staged.write_all(&content[..content.len().min(1)]);
            injected(fault, Fault::DuringWrite)?;
        }
        staged.write_all(content).map_err(|e| FsError::io("write staging file", e))?;
        // std takes a u32 mode on every Unix (rustix's Mode is u16 on macOS).
        staged
            .set_permissions(std::os::unix::fs::PermissionsExt::from_mode(mode))
            .map_err(|e| FsError::io("chmod staging file", e))?;
        injected(fault, Fault::BeforeFsync)?;
        staged.sync_all().map_err(|e| FsError::io("fsync staging file", e))?;
        let meta = staged.metadata().map_err(|e| FsError::io("fstat staging file", e))?;
        injected(fault, Fault::BeforeReplace)?;
        rustix::fs::renameat(&dir, stage_name.as_str(), &dir, name)
            .map_err(|e| FsError::io(format!("rename into '{}'", canon.display()), e))?;
        Ok(WriteInfo {
            path: canon.clone(),
            dev: meta.dev(),
            ino: meta.ino(),
            sha256: Sha256::digest(content).iter().map(|b| format!("{b:02x}")).collect(),
            size: content.len() as u64,
        })
    })();
    match result {
        Ok(info) => {
            // Flush the directory entry; best effort, as in Python.
            let _ = rustix::fs::fsync(&dir);
            Ok(info)
        }
        Err(e) => {
            let _ = rustix::fs::unlinkat(&dir, stage_name.as_str(), AtFlags::empty());
            Err(e)
        }
    }
}
