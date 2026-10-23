//! Execution providers (coding CLIs) as static specifications.
//!
//! Port of `awb/providers/{base,registry}.py`. Providers know how a CLI is invoked, never
//! where it discovers skills (that is the skill-storage side). Milestone R1 supports manual
//! handoff only; nothing here spawns a process. Facts verified 2026-10-23; sources in
//! `docs_urls` and `docs/architecture/provider-adapter-contract.md`.

use crate::util::py_list;

pub const MANUAL_HANDOFF: &str = "manual-handoff";

/// Immutable description of a coding CLI as an execution tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderSpec {
    pub id: &'static str,
    pub display_name: &'static str,
    pub binary: &'static str,
    /// Skill-storage adapter ids whose skills this CLI can load.
    pub skill_adapters: &'static [&'static str],
    pub run_hint: &'static str,
    pub documented_headless: &'static str,
    /// Permission-bypass flags Aurora must never emit.
    pub forbidden_flags: &'static [&'static str],
    pub docs_urls: &'static [&'static str],
    pub verified_on: &'static str,
    pub invocation_modes: &'static [&'static str],
    pub headless_enabled: bool,
}

/// Known providers, sorted by id.
pub const PROVIDERS: [ProviderSpec; 3] = [
    ProviderSpec {
        id: "antigravity",
        display_name: "Google Antigravity CLI",
        binary: "agy",
        skill_adapters: &["antigravity-cli", "antigravity-workspace"],
        run_hint: "Start `agy` interactively in the project directory and paste this handoff (everything above).",
        documented_headless: "agy -p <prompt> [--output-format text|json|stream-json]",
        forbidden_flags: &["--dangerously-skip-permissions"],
        docs_urls: &[
            "https://antigravity.google/docs/cli/headless/",
            "https://antigravity.google/docs/cli/reference/",
            "https://antigravity.google/docs/skills",
        ],
        verified_on: "2026-10-23",
        invocation_modes: &[MANUAL_HANDOFF],
        headless_enabled: false,
    },
    ProviderSpec {
        id: "claude",
        display_name: "Claude Code",
        binary: "claude",
        skill_adapters: &["claude-code"],
        run_hint: "Start `claude` interactively in the project directory and paste this handoff (everything above).",
        documented_headless: "claude -p <prompt> [--output-format text|json|stream-json]",
        forbidden_flags: &[
            "--dangerously-skip-permissions",
            "--allow-dangerously-skip-permissions",
            "bypassPermissions",
        ],
        docs_urls: &[
            "https://code.claude.com/docs/en/headless",
            "https://code.claude.com/docs/en/permissions",
            "https://code.claude.com/docs/en/skills",
        ],
        verified_on: "2026-10-23",
        invocation_modes: &[MANUAL_HANDOFF],
        headless_enabled: false,
    },
    ProviderSpec {
        id: "codex",
        display_name: "OpenAI Codex CLI",
        binary: "codex",
        skill_adapters: &["codex-cli"],
        run_hint: "Start `codex` interactively in the project directory and paste this handoff (everything above).",
        documented_headless: "codex exec <prompt> [--json] [-C <dir>] (default sandbox: read-only)",
        forbidden_flags: &[
            "--dangerously-bypass-approvals-and-sandbox",
            "--yolo",
            "danger-full-access",
            "--ask-for-approval never",
        ],
        docs_urls: &[
            "https://learn.chatgpt.com/docs/non-interactive-mode",
            "https://learn.chatgpt.com/docs/developer-commands?surface=cli",
            "https://learn.chatgpt.com/docs/build-skills",
        ],
        verified_on: "2026-10-23",
        invocation_modes: &[MANUAL_HANDOFF],
        headless_enabled: false,
    },
];

/// Known provider ids, sorted.
pub const PROVIDER_IDS: [&str; 3] = ["antigravity", "claude", "codex"];

pub fn is_known_provider(id: &str) -> bool {
    PROVIDER_IDS.contains(&id)
}

/// Raised when direct provider execution is requested but not enabled (ADR-0005).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "Direct execution via '{provider}' is not enabled; supported modes: ['manual-handoff']. Use the manual handoff bundle."
)]
pub struct ProviderNotEnabled {
    pub provider: String,
}

/// Unknown provider id.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Unknown provider '{0}'. Known providers: {known}", known = py_list(&PROVIDER_IDS))]
pub struct UnknownProvider(pub String);

pub fn get_provider(id: &str) -> Result<&'static ProviderSpec, UnknownProvider> {
    PROVIDERS.iter().find(|p| p.id == id).ok_or_else(|| UnknownProvider(id.to_string()))
}

impl ProviderSpec {
    /// Append provider-specific manual run instructions to a role prompt. Pure.
    pub fn build_handoff(&self, prompt: &str) -> String {
        format!(
            "{}\n\n## How to run this stage with {}\n\n{}\n\nUse the CLI's normal permission prompts. Do not use flags \
             that bypass approvals or sandboxing.\n",
            prompt.trim_end(),
            self.display_name,
            self.run_hint
        )
    }

    /// Direct execution is out of scope for Milestone R1.
    pub fn execute(&self) -> Result<(), ProviderNotEnabled> {
        Err(ProviderNotEnabled { provider: self.id.to_string() })
    }
}
