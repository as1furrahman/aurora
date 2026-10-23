"""
Static provider specifications. Facts verified 2026-10-23; sources in docs_urls and
docs/architecture/provider-adapter-contract.md.
"""

from typing import Dict

from .base import ExecutionProvider, ProviderSpec

_SPECS = (
    ProviderSpec(
        id="antigravity",
        display_name="Google Antigravity CLI",
        binary="agy",
        skill_adapters=("antigravity-cli", "antigravity-workspace"),
        run_hint="Start `agy` interactively in the project directory and paste the prompt below.",
        documented_headless="agy -p <prompt> [--output-format text|json|stream-json]",
        forbidden_flags=("--dangerously-skip-permissions",),
        docs_urls=(
            "https://antigravity.google/docs/cli/headless/",
            "https://antigravity.google/docs/cli/reference/",
            "https://antigravity.google/docs/skills",
        ),
        verified_on="2026-10-23",
    ),
    ProviderSpec(
        id="claude",
        display_name="Claude Code",
        binary="claude",
        skill_adapters=("claude-code",),
        run_hint="Start `claude` interactively in the project directory and paste the prompt below.",
        documented_headless="claude -p <prompt> [--output-format text|json|stream-json]",
        forbidden_flags=(
            "--dangerously-skip-permissions",
            "--allow-dangerously-skip-permissions",
            "bypassPermissions",
        ),
        docs_urls=(
            "https://code.claude.com/docs/en/headless",
            "https://code.claude.com/docs/en/permissions",
            "https://code.claude.com/docs/en/skills",
        ),
        verified_on="2026-10-23",
    ),
    ProviderSpec(
        id="codex",
        display_name="OpenAI Codex CLI",
        binary="codex",
        skill_adapters=("codex-cli",),
        run_hint="Start `codex` interactively in the project directory and paste the prompt below.",
        documented_headless="codex exec <prompt> [--json] [-C <dir>] (default sandbox: read-only)",
        forbidden_flags=(
            "--dangerously-bypass-approvals-and-sandbox",
            "--yolo",
            "danger-full-access",
            "--ask-for-approval never",
        ),
        docs_urls=(
            "https://learn.chatgpt.com/docs/non-interactive-mode",
            "https://learn.chatgpt.com/docs/developer-commands?surface=cli",
            "https://learn.chatgpt.com/docs/build-skills",
        ),
        verified_on="2026-10-23",
    ),
)

PROVIDERS: Dict[str, ExecutionProvider] = {spec.id: ExecutionProvider(spec) for spec in _SPECS}


def get_provider(provider_id: str) -> ExecutionProvider:
    """Return a provider by id, failing clearly for unknown ids."""
    if provider_id not in PROVIDERS:
        raise KeyError(f"Unknown provider '{provider_id}'. Known providers: {sorted(PROVIDERS)}")
    return PROVIDERS[provider_id]
