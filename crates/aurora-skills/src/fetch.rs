//! Fetch one skill directory at a pinned commit (R2 Step 2b).
//!
//! `git` runs from an argument list (never a shell) in a hardened environment: no system or
//! global config, no hooks, no credential helper, no terminal prompt, HTTPS only, and a timeout
//! on every step. The checkout is blobless, depth 1 and sparse, so only the declared subpath's
//! files are downloaded. Results are cached by repository, revision and skill path (each checkout
//! is sparse for one path): the checkout is built in a staging directory and renamed into place,
//! so a cache entry is complete or absent.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy)]
pub struct FetchRequest<'a> {
    /// `https://github.com/<owner>/<name>` (or `file:///…` when `allow_file_urls`).
    pub repo: &'a str,
    /// Full 40-character lowercase commit SHA.
    pub rev: &'a str,
    /// Skill directory inside the repository.
    pub path: &'a str,
    pub cache_root: &'a Path,
}

#[derive(Debug, Clone, Copy)]
pub struct FetchOptions {
    /// Limit for each git step.
    pub timeout: Duration,
    /// Allow `file://` repositories (tests and local mirrors only).
    pub allow_file_urls: bool,
}

impl Default for FetchOptions {
    fn default() -> Self {
        FetchOptions { timeout: Duration::from_secs(120), allow_file_urls: false }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("invalid fetch request: {0}")]
    InvalidRequest(String),
    #[error("git {step} failed: {stderr}")]
    Git { step: &'static str, stderr: String },
    #[error("git {step} timed out after {secs}s")]
    Timeout { step: &'static str, secs: u64 },
    #[error("fetched {got}, expected the pinned {expected}")]
    RevMismatch { expected: String, got: String },
    #[error("'{path}' does not exist at {rev}")]
    PathMissing { path: String, rev: String },
    #[error("{0}")]
    Io(String),
}

fn is_full_sha(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_name_part(p: &str) -> bool {
    !p.is_empty() && !p.starts_with(['.', '-']) && p.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

/// Cache key directories for `repo`: `<owner>/<name>` for GitHub, a hash for local mirrors.
fn repo_key(repo: &str, allow_file_urls: bool) -> Result<PathBuf, FetchError> {
    let invalid = |why: &str| Err(FetchError::InvalidRequest(format!("repo '{repo}': {why}")));
    if repo.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return invalid("must not contain whitespace");
    }
    if let Some(rest) = repo.strip_prefix("https://github.com/") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() == 2 && parts.iter().all(|p| is_name_part(p)) {
            return Ok(PathBuf::from(parts[0]).join(parts[1]));
        }
        return invalid("expected https://github.com/<owner>/<name>");
    }
    if let Some(path) = repo.strip_prefix("file://") {
        if !allow_file_urls {
            return invalid("file:// repositories are not allowed");
        }
        if !path.starts_with('/') {
            return invalid("file:// path must be absolute");
        }
        let digest: String = Sha256::digest(repo.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect();
        return Ok(PathBuf::from("local").join(digest));
    }
    invalid("only https://github.com repositories are supported")
}

/// Cache directory name for one skill path: its last component plus a short hash of the whole
/// path, readable and collision-free.
fn path_key(path: &str) -> String {
    let digest: String = Sha256::digest(path.as_bytes()).iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("{}-{digest}", path.rsplit('/').next().unwrap_or(path))
}

fn is_repo_subpath(s: &str) -> bool {
    !s.is_empty()
        && !s.contains('\\')
        && s.split('/').all(|part| !part.is_empty() && part != "." && part != ".." && !part.starts_with('-'))
}

/// Fetch `req.path` at `req.rev` and return the skill directory inside the cache.
pub fn fetch(req: &FetchRequest<'_>, opts: &FetchOptions) -> Result<PathBuf, FetchError> {
    if !is_full_sha(req.rev) {
        return Err(FetchError::InvalidRequest(format!("rev '{}' must be a full 40-character lowercase SHA", req.rev)));
    }
    if !is_repo_subpath(req.path) {
        return Err(FetchError::InvalidRequest(format!(
            "path '{}' must be relative and inside the repository",
            req.path
        )));
    }
    let key = repo_key(req.repo, opts.allow_file_urls)?;
    let parent = req.cache_root.join(&key).join(req.rev);
    let final_dir = parent.join(path_key(req.path));

    if !final_dir.exists() {
        std::fs::create_dir_all(&parent)
            .map_err(|e| FetchError::Io(format!("cannot create {}: {e}", parent.display())))?;
        // Unique per call, not just per process: the TUI fetches on several threads at once.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let staging = parent.join(format!("{}.tmp-{}-{n}", path_key(req.path), std::process::id()));
        std::fs::create_dir(&staging)
            .map_err(|e| FetchError::Io(format!("cannot create {}: {e}", staging.display())))?;
        let result = checkout(req, opts, &staging).and_then(|()| match std::fs::rename(&staging, &final_dir) {
            Ok(()) => Ok(()),
            // Another thread or process finished the same revision first; theirs is equivalent.
            Err(_) if final_dir.exists() => Ok(()),
            Err(e) => Err(FetchError::Io(format!("cannot move checkout into the cache: {e}"))),
        });
        if staging.exists() {
            // Our own staging directory: created by this call just above, under a name no other
            // call uses.
            let _ = std::fs::remove_dir_all(&staging);
        }
        result?;
    }

    let head = git(&final_dir, &["rev-parse", "HEAD"], "rev-parse", opts)?;
    if head != req.rev {
        return Err(FetchError::RevMismatch { expected: req.rev.to_string(), got: head });
    }
    let dir = final_dir.join(req.path);
    if !dir.is_dir() {
        return Err(FetchError::PathMissing { path: req.path.to_string(), rev: req.rev.to_string() });
    }
    Ok(dir)
}

/// Check out `req` into the empty directory `dir`.
fn checkout(req: &FetchRequest<'_>, opts: &FetchOptions, dir: &Path) -> Result<(), FetchError> {
    git(dir, &["init", "-q"], "init", opts)?;
    git(dir, &["sparse-checkout", "set", "--no-cone", &format!("/{}/", req.path)], "sparse-checkout", opts)?;
    git(
        dir,
        &["fetch", "-q", "--no-tags", "--depth", "1", "--filter=blob:none", "--", req.repo, req.rev],
        "fetch",
        opts,
    )?;
    git(dir, &["checkout", "-q", "--detach", "FETCH_HEAD"], "checkout", opts)?;
    Ok(())
}

/// Run `git -C dir <args>` with the hardened configuration; return trimmed stdout.
fn git(dir: &Path, args: &[&str], step: &'static str, opts: &FetchOptions) -> Result<String, FetchError> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir);
    for setting in [
        "core.hooksPath=/dev/null",
        "credential.helper=",
        "core.autocrlf=false",
        "core.symlinks=true",
        "core.fsmonitor=false",
        "advice.detachedHead=false",
        "init.defaultBranch=aurora",
        "fetch.recurseSubmodules=false",
        "submodule.recurse=false",
        "transfer.fsckObjects=true",
        "protocol.allow=never",
        "protocol.https.allow=always",
    ] {
        cmd.arg("-c").arg(setting);
    }
    if opts.allow_file_urls {
        cmd.arg("-c").arg("protocol.file.allow=always");
    }
    cmd.args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .env("SSH_ASKPASS", "")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for proxy in ["HTTPS_PROXY", "https_proxy", "NO_PROXY", "no_proxy", "SSL_CERT_FILE", "SSL_CERT_DIR"] {
        if let Some(v) = std::env::var_os(proxy) {
            cmd.env(proxy, v);
        }
    }
    let child = cmd.spawn().map_err(|e| FetchError::Io(format!("cannot run git: {e}")))?;
    let (status, stdout, stderr) = wait(child, opts.timeout, step)?;
    if !status.success() {
        let stderr = stderr.trim().lines().take(5).collect::<Vec<_>>().join(" / ");
        return Err(FetchError::Git { step, stderr });
    }
    Ok(stdout.trim().to_string())
}

fn wait(
    mut child: Child,
    timeout: Duration,
    step: &'static str,
) -> Result<(std::process::ExitStatus, String, String), FetchError> {
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_string(&mut text);
            }
            text
        })
    };
    let out = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let deadline = Instant::now() + timeout;
    let status = loop {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(FetchError::Timeout { step, secs: timeout.as_secs() });
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(FetchError::Io(format!("waiting for git: {e}"))),
        }
    };
    Ok((status, out.join().unwrap_or_default(), err.join().unwrap_or_default()))
}
