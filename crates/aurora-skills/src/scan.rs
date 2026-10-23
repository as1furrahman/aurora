//! Static skill scanner (R2 Step 2a). A signal, not a gate on its own: Trail of Bits bypassed five
//! skill scanners in 2026, so a clean verdict never replaces reading the files.
//!
//! - `Block`: cannot be installed, even with review. Things a skill never needs and attacks use:
//!   symlinks, special files, compiled bytecode and binaries, archives, newline padding that
//!   pushes a payload out of view, and anything over the size limits.
//! - `Review`: legitimate but powerful; a human must read the files: scripts, executables,
//!   hooks, `` !`cmd` `` injection, broad `allowed-tools`, pipe-to-shell commands (rustup and uv
//!   installers and security skills quoting attacks use them too), encoded blobs, download links.

use crate::skill::{parse_skill, shell_injections};
use crate::tree::{FileEntry, SkillTree};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Review,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    Clean,
    NeedsReview,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Finding {
    pub rule: &'static str,
    pub level: Level,
    /// Path relative to the skill root.
    pub path: String,
    /// 1-based line, when the finding is in text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ScanReport {
    pub verdict: Verdict,
    pub findings: Vec<Finding>,
}

/// Blank-line run that counts as padding (published bypass: ~100,000 newlines before a payload).
const PADDING_LINES: usize = 1_000;
const LONG_LINE_CHARS: usize = 10_000;
const BLOB_CHARS: usize = 2_000;

const SCRIPT_EXTENSIONS: [&str; 16] =
    ["sh", "bash", "zsh", "fish", "py", "js", "mjs", "cjs", "ts", "rb", "pl", "ps1", "bat", "cmd", "php", "lua"];
const BYTECODE_EXTENSIONS: [&str; 4] = ["pyc", "pyo", "class", "wasm"];
const BINARY_EXTENSIONS: [&str; 6] = ["exe", "dll", "so", "dylib", "bin", "scr"];
const ARCHIVE_EXTENSIONS: [&str; 17] = [
    "zip", "tar", "gz", "tgz", "bz2", "xz", "zst", "7z", "rar", "jar", "whl", "docx", "xlsx", "pptx", "odt", "ods",
    "odp",
];
const IMAGE_EXTENSIONS: [&str; 7] = ["png", "jpg", "jpeg", "gif", "webp", "ico", "bmp"];
const DOWNLOAD_EXTENSIONS: [&str; 12] =
    [".sh", ".ps1", ".exe", ".bin", ".dmg", ".pkg", ".msi", ".bat", ".cmd", ".scr", ".appimage", ".command"];
const DOWNLOADERS: [&str; 6] = ["curl", "wget", "iwr", "irm", "invoke-webrequest", "invoke-restmethod"];
const SHELLS: [&str; 13] = [
    "sh",
    "bash",
    "zsh",
    "dash",
    "ksh",
    "fish",
    "iex",
    "invoke-expression",
    "python",
    "python3",
    "node",
    "perl",
    "ruby",
];

fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()).unwrap_or_default()
}

fn magic(bytes: &[u8]) -> Option<&'static str> {
    const BINARY: [&[u8]; 6] =
        [b"\x7fELF", b"MZ", b"\xfe\xed\xfa\xce", b"\xfe\xed\xfa\xcf", b"\xcf\xfa\xed\xfe", b"\xca\xfe\xba\xbe"];
    const ARCHIVE: [&[u8]; 5] = [b"PK\x03\x04", b"\x1f\x8b", b"7z\xbc\xaf", b"Rar!", b"BZh"];
    if BINARY.iter().any(|m| bytes.starts_with(m)) {
        Some("binary")
    } else if ARCHIVE.iter().any(|m| bytes.starts_with(m)) {
        Some("archive")
    } else {
        None
    }
}

/// A downloader given a target and piped into an interpreter, or an interpreter fed a download
/// through process or command substitution: `curl … | sh`, `iwr … | iex`, `bash <(curl …)`.
/// Markdown table rows and target-less prose (`` `curl | sh` lines ``) do not count.
fn pipe_to_shell(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.len() > 1 && trimmed.starts_with('|') && trimmed.ends_with('|') {
        return false;
    }
    let lower = line.to_ascii_lowercase();
    let words = |s: &str| -> Vec<String> {
        s.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == '"' || c == '\'' || c == '`' || c == '$')
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    };
    // A downloader plus a target: a URL or a host-like `name.tld/path` word.
    let has_downloader = |s: &str| {
        let w = words(s);
        w.iter().any(|w| DOWNLOADERS.contains(&w.as_str()))
            && w.iter().any(|w| w.contains("://") || (w.contains('.') && w.contains('/')))
    };
    let is_shell = |w: &str| SHELLS.contains(&w.rsplit('/').next().unwrap_or(w));
    let segments: Vec<&str> = lower.split('|').collect();
    for pair in segments.windows(2) {
        if has_downloader(pair[0]) {
            let next: Vec<String> = words(pair[1]).into_iter().filter(|w| w != "sudo" && !w.starts_with('-')).collect();
            if next.first().is_some_and(|w| is_shell(w)) {
                return true;
            }
        }
    }
    for opener in ["<(", "$(", "`"] {
        if let Some(at) = lower.find(opener)
            && has_downloader(&lower[at..])
            && words(&lower[..at]).iter().any(|w| is_shell(w) || w == "source" || w == "eval")
        {
            return true;
        }
    }
    false
}

fn broad_allowed_tools(value: &serde_json::Value) -> Option<String> {
    let tokens: Vec<String> = match value {
        serde_json::Value::String(s) => s.split([' ', ',']).filter(|t| !t.is_empty()).map(str::to_string).collect(),
        serde_json::Value::Array(items) => items.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    };
    tokens.into_iter().find(|t| matches!(t.as_str(), "Bash" | "Bash(*)" | "Bash(:*)" | "*"))
}

fn scan_text(f: &FileEntry, text: &str, out: &mut Vec<Finding>) {
    let mut push = |rule, level, line: usize, detail: String| {
        if !out.iter().any(|x| x.rule == rule && x.path == f.path) {
            out.push(Finding { rule, level, path: f.path.clone(), line: Some(line), detail });
        }
    };
    let mut blank_run = 0usize;
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run == PADDING_LINES {
                push("padding", Level::Block, n, format!("{PADDING_LINES}+ consecutive blank lines can hide content"));
            }
            continue;
        }
        blank_run = 0;
        if line.chars().count() > LONG_LINE_CHARS {
            push("long-line", Level::Review, n, format!("line longer than {LONG_LINE_CHARS} characters"));
        }
        if pipe_to_shell(line) {
            push(
                "pipe-to-shell",
                Level::Review,
                n,
                format!("downloads and executes: {}", line.trim().chars().take(100).collect::<String>()),
            );
        }
        let mut run = 0usize;
        for c in line.chars() {
            if c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=') {
                run += 1;
                if run == BLOB_CHARS {
                    push("encoded-blob", Level::Review, n, format!("{BLOB_CHARS}+ characters of base64-like data"));
                }
            } else {
                run = 0;
            }
        }
        for word in line.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '<' | '>' | '"' | '\'' | '`')) {
            let lower = word.trim_end_matches(['.', ',', ';', ':']).to_ascii_lowercase();
            if (lower.starts_with("http://") || lower.starts_with("https://"))
                && DOWNLOAD_EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
            {
                push("download-url", Level::Review, n, format!("links an executable download: {word}"));
            }
        }
    }
}

/// Scan a collected skill.
pub fn scan(tree: &SkillTree) -> ScanReport {
    let mut findings = Vec::new();
    let mut block = |rule, path: &str, detail: String| {
        findings.push(Finding { rule, level: Level::Block, path: path.to_string(), line: None, detail });
    };
    for p in &tree.symlinks {
        block("symlink", p, "symlinks are never followed or installed".into());
    }
    for p in &tree.specials {
        block("special-file", p, "FIFOs, sockets and devices are not skill content".into());
    }
    for p in &tree.too_deep {
        block("too-deep", p, "directory nesting exceeds the limit".into());
    }
    if tree.too_many_files {
        block("too-many-files", "", "more files than the limit; the rest were not read".into());
    }
    if tree.too_large {
        block("too-large", "", "total size exceeds the limit; the rest were not read".into());
    }

    match tree.file("SKILL.md").and_then(|f| f.content.as_deref()) {
        None => block("skill-md", "SKILL.md", "SKILL.md is missing or unreadable".into()),
        Some(bytes) => match std::str::from_utf8(bytes)
            .map_err(|e| e.to_string())
            .and_then(|t| parse_skill(t, "").map_err(|e| e.to_string()))
        {
            Err(e) => block("skill-md", "SKILL.md", format!("SKILL.md does not parse: {e}")),
            Ok(skill) => {
                let mut review = |rule, line, detail: String| {
                    findings.push(Finding { rule, level: Level::Review, path: "SKILL.md".into(), line, detail });
                };
                if skill.frontmatter.contains_key("hooks") {
                    review("hooks", None, "declares session hooks that run commands on Claude Code events".into());
                }
                for inj in shell_injections(&skill.body) {
                    review(
                        "shell-injection",
                        None,
                        format!("runs on load without a prompt (body line {}): {}", inj.line, inj.snippet),
                    );
                }
                if let Some(tool) = skill.frontmatter.get("allowed-tools").and_then(broad_allowed_tools) {
                    review("broad-allowed-tools", None, format!("pre-approves '{tool}' for every command"));
                }
            }
        },
    }

    for f in &tree.files {
        let ext = extension(&f.path);
        let push = |findings: &mut Vec<Finding>, rule, level, detail: String| {
            findings.push(Finding { rule, level, path: f.path.clone(), line: None, detail });
        };
        let Some(bytes) = f.content.as_deref() else {
            // Unread because of the total limit: already reported once as `too-large`.
            if f.size > tree.max_file_bytes {
                push(
                    &mut findings,
                    "file-too-large",
                    Level::Block,
                    format!("{} bytes; the limit is {}", f.size, tree.max_file_bytes),
                );
            }
            continue;
        };
        if BYTECODE_EXTENSIONS.contains(&ext.as_str()) || f.path.split('/').any(|c| c == "__pycache__") {
            push(&mut findings, "bytecode", Level::Block, "compiled bytecode cannot be reviewed".into());
            continue;
        }
        match magic(bytes) {
            Some("binary") => {
                push(&mut findings, "binary", Level::Block, "native executable".into());
                continue;
            }
            Some(_) => {
                push(&mut findings, "archive", Level::Block, "archives can hide files from review".into());
                continue;
            }
            None => {}
        }
        if BINARY_EXTENSIONS.contains(&ext.as_str()) {
            push(&mut findings, "binary", Level::Block, "native executable".into());
            continue;
        }
        if ARCHIVE_EXTENSIONS.contains(&ext.as_str()) {
            push(&mut findings, "archive", Level::Block, "archives can hide files from review".into());
            continue;
        }
        let is_script = SCRIPT_EXTENSIONS.contains(&ext.as_str()) || bytes.starts_with(b"#!");
        if is_script {
            push(&mut findings, "script", Level::Review, "bundled script: read it before installing".into());
        } else if f.executable {
            push(&mut findings, "executable", Level::Review, "has the executable bit".into());
        }
        if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
            continue;
        }
        match std::str::from_utf8(bytes) {
            Ok(text) => scan_text(f, text, &mut findings),
            Err(_) => push(&mut findings, "non-text", Level::Review, "not UTF-8 text and not a known image".into()),
        }
    }

    let verdict = match findings.iter().map(|f| f.level).max() {
        None => Verdict::Clean,
        Some(Level::Review) => Verdict::NeedsReview,
        Some(Level::Block) => Verdict::Blocked,
    };
    ScanReport { verdict, findings }
}

/// What a human review covers for this entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewState {
    /// No recorded review.
    None,
    /// The catalog records a review of the pinned content (curated or official sources).
    Catalog(String),
    /// A community source: never pre-reviewed by the catalog.
    Community,
}

/// Whether a fetched skill may be installed, and what stands in the way.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case", tag = "state", content = "detail")]
pub enum Readiness {
    Ready,
    /// Needs review, already covered by the catalog's review of this exact content.
    Reviewed(String),
    /// A human must read these files (or, for community entries, the whole skill) first.
    NeedsReview(Vec<String>),
    /// Cannot be installed; the rules that block it.
    Blocked(Vec<String>),
    /// The content is not what the catalog pinned.
    IntegrityMismatch {
        expected: String,
        got: String,
    },
}

/// Decide readiness. Precedence: integrity, then blocking findings, then review.
pub fn readiness(
    tree: &SkillTree,
    report: &ScanReport,
    expected_sha256: Option<&str>,
    review: ReviewState,
) -> Readiness {
    let got = tree.content_hash();
    if let Some(expected) = expected_sha256
        && expected != got
    {
        return Readiness::IntegrityMismatch { expected: expected.to_string(), got };
    }
    if report.verdict == Verdict::Blocked {
        let mut rules: Vec<String> =
            report.findings.iter().filter(|f| f.level == Level::Block).map(|f| f.rule.to_string()).collect();
        rules.dedup();
        return Readiness::Blocked(rules);
    }
    let mut flagged: Vec<String> = report.findings.iter().map(|f| f.path.clone()).collect();
    flagged.sort();
    flagged.dedup();
    match review {
        ReviewState::Community => Readiness::NeedsReview(if flagged.is_empty() {
            tree.files.iter().map(|f| f.path.clone()).collect()
        } else {
            flagged
        }),
        _ if report.verdict == Verdict::Clean => Readiness::Ready,
        ReviewState::Catalog(note) if expected_sha256.is_some() => Readiness::Reviewed(note),
        _ => Readiness::NeedsReview(flagged),
    }
}
