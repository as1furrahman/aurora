//! Mandatory cross-process mutation lock (port of `awb/safety/locking.py`).
//!
//! Uses `flock` (std `File::try_lock` on Unix), the same mechanism as the Python
//! implementation, so Rust and Python processes exclude each other. The lock file is never
//! unlinked or recreated (SEC-01/SEC-08): the kernel releases a dead holder's lock, and
//! after acquiring we verify the locked descriptor is still the file at the path.

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustix::fs::{Mode, OFlags};

use crate::FsError;
use crate::paths::{is_within, resolve};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const RETRY: Duration = Duration::from_millis(50);

/// Held lock; released (metadata cleared, flock dropped) on drop.
#[derive(Debug)]
pub struct ProcessLock {
    path: PathBuf,
    file: Option<File>,
}

fn hostname() -> String {
    rustix::system::uname().nodename().to_string_lossy().into_owned()
}

impl ProcessLock {
    pub fn acquire(lock_path: &Path, allowed_roots: &[&Path], timeout: Duration) -> Result<ProcessLock, FsError> {
        // Resolve the parent only: canonicalising the whole path would follow a symlinked lock
        // file to its target before O_NOFOLLOW could refuse it.
        let name = lock_path
            .file_name()
            .ok_or_else(|| FsError::Lock(format!("Lock path '{}' has no file name", lock_path.display())))?;
        let path = resolve(lock_path.parent().unwrap_or(Path::new("."))).join(name);
        if !allowed_roots.is_empty() && !allowed_roots.iter().any(|r| is_within(&path, &resolve(r))) {
            let roots: Vec<String> = allowed_roots.iter().map(|r| format!("'{}'", resolve(r).display())).collect();
            return Err(FsError::PathTraversal(format!(
                "Lock file '{}' is outside allowed roots: [{}]",
                path.display(),
                roots.join(", ")
            )));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| FsError::io(format!("create '{}'", parent.display()), e))?;
        }

        let start = Instant::now();
        loop {
            let fd = match rustix::fs::open(
                &path,
                OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from(0o600),
            ) {
                Ok(fd) => fd,
                Err(e) if e == rustix::io::Errno::LOOP => {
                    return Err(FsError::Lock(format!(
                        "Lock file '{}' is a symlink; refusing to lock",
                        path.display()
                    )));
                }
                Err(e) => {
                    if start.elapsed() >= timeout {
                        return Err(FsError::LockTimeout(format!(
                            "Timed out opening lock file '{}': {e}",
                            path.display()
                        )));
                    }
                    sleep(RETRY);
                    continue;
                }
            };
            let file = File::from(fd);
            match file.try_lock() {
                Ok(()) => {}
                Err(std::fs::TryLockError::WouldBlock) => {
                    if start.elapsed() >= timeout {
                        return Err(FsError::LockTimeout(format!(
                            "Timed out after {:.1}s waiting for lock on '{}'{}",
                            timeout.as_secs_f64(),
                            path.display(),
                            holder(&path)
                        )));
                    }
                    drop(file);
                    sleep(RETRY);
                    continue;
                }
                Err(std::fs::TryLockError::Error(e)) => {
                    return Err(FsError::Lock(format!("Failed to acquire lock on '{}': {e}", path.display())));
                }
            }

            // Re-verify the locked descriptor is the file currently at the path.
            let same = match (file.metadata(), std::fs::symlink_metadata(&path)) {
                (Ok(fd_meta), Ok(path_meta)) => fd_meta.ino() == path_meta.ino() && fd_meta.dev() == path_meta.dev(),
                _ => false,
            };
            if !same {
                let _ = file.unlock();
                drop(file);
                sleep(Duration::from_millis(20));
                continue;
            }

            let mut lock = ProcessLock { path, file: Some(file) };
            lock.write_metadata();
            return Ok(lock);
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn write_metadata(&mut self) {
        let Some(file) = self.file.as_mut() else { return };
        let meta = serde_json::json!({
            "pid": std::process::id(),
            "uid": rustix::process::getuid().as_raw(),
            "host": hostname(),
            "acquired_at": SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0),
        });
        let _ = file.set_len(0);
        let _ = file.seek(SeekFrom::Start(0));
        let _ = file.write_all(meta.to_string().as_bytes());
        let _ = file.sync_all();
    }
}

/// " (held by PID n on host)" from the lock file's metadata, if readable.
fn holder(path: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(path) else { return String::new() };
    let Ok(meta) = serde_json::from_str::<serde_json::Value>(&text) else { return String::new() };
    match (meta.get("pid"), meta.get("host").and_then(|h| h.as_str())) {
        (Some(pid), Some(host)) => format!(" (held by PID {pid} on {host})"),
        _ => String::new(),
    }
}

impl Drop for ProcessLock {
    fn drop(&mut self) {
        if let Some(file) = self.file.take() {
            let _ = file.set_len(0);
            let _ = file.sync_all();
            let _ = file.unlock();
        }
    }
}
