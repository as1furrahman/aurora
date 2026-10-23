//! No-follow, non-blocking, bounded reads (port of `SafeFS.safe_open_read`).
//!
//! The parent directory is opened `O_DIRECTORY|O_NOFOLLOW` and verified, then the file is
//! opened relative to it with `O_NOFOLLOW|O_NONBLOCK` so a symlink is refused and a FIFO can
//! never block the reader. Only user-owned regular files within the size bound are read.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use rustix::fs::{FileType, Mode, OFlags};
use sha2::{Digest, Sha256};

use crate::FsError;
use crate::paths::validate_path;

#[derive(Debug, Clone)]
pub struct ReadInfo {
    pub path: PathBuf,
    pub content: Vec<u8>,
    pub size: u64,
    pub sha256: String,
    pub dev: u64,
    pub ino: u64,
    pub mode: u32,
}

pub fn safe_read(target: &Path, allowed_roots: &[&Path], max_size: u64) -> Result<ReadInfo, FsError> {
    let canon = validate_path(target, allowed_roots, false)?;
    let parent = canon.parent().ok_or_else(|| FsError::Descriptor(format!("'{}' has no parent", canon.display())))?;
    let name = canon.file_name().ok_or_else(|| FsError::Descriptor(format!("'{}' has no name", canon.display())))?;
    let uid = rustix::process::getuid().as_raw();

    let dir = rustix::fs::open(
        parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| FsError::Descriptor(format!("Cannot open parent directory '{}': {e}", parent.display())))?;
    let st_dir = rustix::fs::fstat(&dir).map_err(|e| FsError::io("fstat parent", e))?;
    if FileType::from_raw_mode(st_dir.st_mode) != FileType::Directory {
        return Err(FsError::Descriptor(format!("Parent '{}' descriptor is not a directory", parent.display())));
    }
    if st_dir.st_uid != uid {
        return Err(FsError::Descriptor(format!(
            "Parent '{}' owner UID mismatch (expected {uid}, got {})",
            parent.display(),
            st_dir.st_uid
        )));
    }

    let fd = rustix::fs::openat(
        &dir,
        name,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| {
        if e == rustix::io::Errno::LOOP {
            FsError::SymlinkAncestor(format!("Target file '{}' is a symlink: {e}", name.to_string_lossy()))
        } else {
            FsError::Descriptor(format!("Cannot open file '{}': {e}", canon.display()))
        }
    })?;
    let st = rustix::fs::fstat(&fd).map_err(|e| FsError::io("fstat file", e))?;
    match FileType::from_raw_mode(st.st_mode) {
        FileType::RegularFile => {}
        FileType::Fifo => {
            return Err(FsError::Fifo(format!("FIFO detected at '{}'; refused to read", canon.display())));
        }
        _ => {
            return Err(FsError::Fifo(format!(
                "Non-regular file node (mode {:#o}) at '{}'",
                st.st_mode,
                canon.display()
            )));
        }
    }
    if st.st_uid != uid {
        return Err(FsError::Descriptor(format!(
            "File '{}' owner mismatch (expected {uid}, got {})",
            canon.display(),
            st.st_uid
        )));
    }
    let declared = u64::try_from(st.st_size).unwrap_or(u64::MAX);
    if declared > max_size {
        return Err(FsError::Descriptor(format!(
            "File '{}' exceeds maximum size bound ({declared} > {max_size} bytes)",
            canon.display()
        )));
    }

    // Bounded read: never trust the size recorded by fstat alone.
    let mut content = Vec::new();
    File::from(fd)
        .take(max_size + 1)
        .read_to_end(&mut content)
        .map_err(|e| FsError::io(format!("read '{}'", canon.display()), e))?;
    if content.len() as u64 > max_size {
        return Err(FsError::Descriptor(format!(
            "File '{}' streamed bytes exceeded max size limit ({max_size})",
            canon.display()
        )));
    }
    let sha256 = Sha256::digest(&content).iter().map(|b| format!("{b:02x}")).collect();
    Ok(ReadInfo {
        path: canon,
        size: content.len() as u64,
        content,
        sha256,
        dev: st.st_dev,
        ino: st.st_ino,
        mode: st.st_mode,
    })
}
