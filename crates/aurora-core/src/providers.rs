//! Execution providers (coding CLIs) as static specifications.
//!
//! Port of `awb/providers/{base,registry}.py`. Providers know how a CLI is invoked, never
//! where it discovers skills (that is the skill-storage side). Milestone R1 supports manual
//! handoff only; nothing here spawns a process. Facts verified 2026-10-23; sources in
//! `docs_urls` and `docs/architecture/provider-adapter-contract.md`.

use serde::{Deserialize, Serialize};

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
    /// How a user or agent invokes an installed skill by name (`{}` is the name); empty when
    /// the CLI has no documented syntax.
    pub skill_invocation: &'static str,
    pub run_hint: &'static str,
    pub documented_headless: &'static str,
    /// Permission-bypass flags Aurora must never emit.
    pub forbidden_flags: &'static [&'static str],
    pub docs_urls: &'static [&'static str],
    pub verified_on: &'static str,
    pub invocation_modes: &'static [&'static str],
    pub headless_enabled: bool,
    /// How strongly this CLI can enforce each capability a role may require (after the AWS
    /// CLI Agent Orchestrator tool-restriction table; sources per entry).
    pub enforcement: &'static [Enforcement],
}

impl ProviderSpec {
    /// How to invoke skill `name` on this CLI, formatted for a handoff.
    pub fn invoke_skill(&self, name: &str) -> String {
        if self.skill_invocation.is_empty() {
            format!("`{name}` (ask for this skill by name)")
        } else {
            format!("`{}`", self.skill_invocation.replace("{}", name))
        }
    }
}

/// Capabilities a role may require (`requires = [...]`).
pub const CAPABILITIES: [&str; 2] = ["read_only", "tool_restriction"];

/// `hard`: the CLI itself enforces it. `soft`: only a prompt instruction; Aurora cannot verify it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strength {
    Hard,
    Soft,
}

impl std::fmt::Display for Strength {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Strength::Hard => "hard",
            Strength::Soft => "soft",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Enforcement {
    pub capability: &'static str,
    pub strength: Strength,
    /// Instruction for starting the CLI in the restricted mode (manual handoff).
    pub how: &'static str,
    pub source: &'static str,
}

/// Known providers, sorted by id.
pub const PROVIDERS: [ProviderSpec; 4] = [
    ProviderSpec {
        id: "antigravity",
        display_name: "Google Antigravity CLI",
        binary: "agy",
        skill_adapters: &["antigravity-cli", "antigravity-workspace"],
        skill_invocation: "",
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
        enforcement: &[
            Enforcement {
                capability: "read_only",
                strength: Strength::Soft,
                how: "Start `agy --mode plan` and tell it not to modify the workspace; Aurora could not verify that plan mode blocks writes.",
                source: "https://antigravity.google/docs/cli/reference/",
            },
            Enforcement {
                capability: "tool_restriction",
                strength: Strength::Soft,
                how: "Add deny rules for the restricted tools in ~/.gemini/antigravity-cli/settings.json (permissions.deny) and state the restriction in the prompt; Aurora cannot verify them.",
                source: "https://antigravity.google/docs/cli/commands/permissions/",
            },
        ],
    },
    ProviderSpec {
        id: "claude",
        display_name: "Claude Code",
        binary: "claude",
        skill_adapters: &["claude-code"],
        skill_invocation: "/{}",
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
        enforcement: &[
            Enforcement {
                capability: "read_only",
                strength: Strength::Hard,
                how: "Start `claude --permission-mode plan` (plan mode cannot edit files); save its output into the stage's output files yourself.",
                source: "https://code.claude.com/docs/en/permissions",
            },
            Enforcement {
                capability: "tool_restriction",
                strength: Strength::Hard,
                how: "Start `claude --disallowedTools <tools>` to deny the restricted tools.",
                source: "https://code.claude.com/docs/en/cli-reference",
            },
        ],
    },
    ProviderSpec {
        id: "codex",
        display_name: "OpenAI Codex CLI",
        binary: "codex",
        skill_adapters: &["codex-cli"],
        skill_invocation: "${}",
        run_hint: "Start `codex` interactively in the project directory and paste this handoff (everything above).",
        documented_headless: "codex exec <prompt> [--json] [-C <dir>] (default sandbox: read-only)",
        forbidden_flags: &[
            "--dangerously-bypass-approvals-and-sandbox",
            "--yolo",
            "danger-full-access",
            "--ask-for-approval never",
            "--dangerously-bypass-hook-trust",
        ],
        docs_urls: &[
            "https://learn.chatgpt.com/docs/non-interactive-mode",
            "https://learn.chatgpt.com/docs/developer-commands?surface=cli",
            "https://learn.chatgpt.com/docs/build-skills",
        ],
        verified_on: "2026-10-23",
        invocation_modes: &[MANUAL_HANDOFF],
        headless_enabled: false,
        enforcement: &[
            Enforcement {
                capability: "read_only",
                strength: Strength::Hard,
                how: "Start `codex --sandbox read-only` (the sandbox blocks writes); save its output into the stage's output files yourself.",
                source: "https://learn.chatgpt.com/docs/developer-commands?surface=cli",
            },
            Enforcement {
                capability: "tool_restriction",
                strength: Strength::Soft,
                how: "Codex has no per-tool deny flag; state the restriction in the prompt.",
                source: "https://github.com/awslabs/cli-agent-orchestrator/blob/main/docs/tool-restrictions.md",
            },
        ],
    },
    // Facts verified against the installed CLI (`hermes --help`, `hermes chat --help`,
    // Hermes Agent v0.22.5) on 2026-10-24.
    ProviderSpec {
        id: "hermes",
        display_name: "Hermes Agent",
        binary: "hermes",
        skill_adapters: &["hermes"],
        skill_invocation: "/{}",
        run_hint: "Start `hermes` interactively in the project directory and paste this handoff (everything above).",
        documented_headless: "hermes chat --query-file <file> --oneshot -Q [--in <dir>] [-t <toolsets>]",
        forbidden_flags: &["--yolo", "--accept-hooks"],
        docs_urls: &[
            "https://hermes-agent.nousresearch.com/docs/getting-started/quickstart",
            "https://github.com/NousResearch/hermes-agent",
        ],
        verified_on: "2026-10-24",
        invocation_modes: &[MANUAL_HANDOFF],
        headless_enabled: false,
        enforcement: &[
            Enforcement {
                capability: "read_only",
                strength: Strength::Soft,
                how: "Hermes has no read-only mode; enable only non-writing toolsets with `hermes -t <toolsets>` and state the restriction in the prompt.",
                source: "https://hermes-agent.nousresearch.com/docs/getting-started/quickstart",
            },
            Enforcement {
                capability: "tool_restriction",
                strength: Strength::Hard,
                how: "Start `hermes -t <toolsets>` to enable only the allowed toolsets.",
                source: "https://hermes-agent.nousresearch.com/docs/getting-started/quickstart",
            },
        ],
    },
];

/// Known provider ids, sorted.
pub const PROVIDER_IDS: [&str; 4] = ["antigravity", "claude", "codex", "hermes"];

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

    pub fn enforcement(&self, capability: &str) -> Option<&'static Enforcement> {
        self.enforcement.iter().find(|e| e.capability == capability)
    }

    /// Direct execution is out of scope for Milestone R1.
    pub fn execute(&self) -> Result<(), ProviderNotEnabled> {
        Err(ProviderNotEnabled { provider: self.id.to_string() })
    }
}
