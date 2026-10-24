//! Mandatory run locking. The lock file persists across releases (SEC-01/SEC-08: never unlink or
//! recreate it).

use std::fs;
use std::time::{Duration, Instant};

use aurora_fs::FsError;
use aurora_fs::lock::ProcessLock;

#[test]
fn basic_acquire_and_release_keeps_the_lock_file() {
    let t = tempfile::tempdir().expect("tempdir");
    let path = t.path().join(".awb.lock");
    {
        let _guard = ProcessLock::acquire(&path, &[t.path()], Duration::from_secs(2)).expect("acquire");
        let meta: serde_json::Value = serde_json::from_slice(&fs::read(&path).expect("read")).expect("json");
        assert_eq!(meta["pid"], std::process::id());
        assert!(meta["host"].is_string() && meta["acquired_at"].is_number());
    }
    assert!(path.is_file(), "lock file must persist");
    assert_eq!(fs::read(&path).expect("read").len(), 0, "metadata cleared on release");
    let _again = ProcessLock::acquire(&path, &[t.path()], Duration::from_secs(2)).expect("re-acquire");
}

#[test]
fn contested_lock_times_out() {
    let t = tempfile::tempdir().expect("tempdir");
    let path = t.path().join(".awb.lock");
    let _held = ProcessLock::acquire(&path, &[t.path()], Duration::from_secs(2)).expect("first");
    let start = Instant::now();
    let err = ProcessLock::acquire(&path, &[t.path()], Duration::from_millis(300)).expect_err("contested");
    assert!(matches!(err, FsError::LockTimeout(_)), "{err}");
    assert!(err.to_string().contains(&std::process::id().to_string()), "names the holder: {err}");
    assert!(start.elapsed() >= Duration::from_millis(300));
}

#[test]
fn lock_enforces_path_boundaries() {
    let t = tempfile::tempdir().expect("tempdir");
    let inside = t.path().join("inside");
    fs::create_dir(&inside).expect("mkdir");
    let err = ProcessLock::acquire(&t.path().join(".awb.lock"), &[inside.as_path()], Duration::from_millis(100))
        .expect_err("outside root");
    assert!(matches!(err, FsError::PathTraversal(_)));
}

#[test]
fn lock_refuses_a_symlinked_lock_file() {
    let t = tempfile::tempdir().expect("tempdir");
    fs::write(t.path().join("target"), "").expect("write");
    std::os::unix::fs::symlink(t.path().join("target"), t.path().join(".awb.lock")).expect("symlink");
    assert!(ProcessLock::acquire(&t.path().join(".awb.lock"), &[t.path()], Duration::from_millis(200)).is_err());
}
