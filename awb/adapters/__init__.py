"""
awb.adapters - Skill-storage adapters (Antigravity, Codex, Claude Code).
Execution concerns live in awb.providers (ADR-0002).
"""

from typing import Dict

from awb.config import Config
from .antigravity import AntigravityCLIAdapter, AntigravityWorkspaceAdapter
from .antigravity_ide import AntigravityIDEAdapter
from .base import BaseAdapter, ConformanceReport
from .claude_code import ClaudeCodeSkillAdapter
from .codex import CodexCLIAdapter

__all__ = [
    "BaseAdapter",
    "SkillStorageAdapter",
    "ClaudeCodeSkillAdapter",
    "ConformanceReport",
    "AntigravityCLIAdapter",
    "AntigravityWorkspaceAdapter",
    "CodexCLIAdapter",
    "AntigravityIDEAdapter",
    "get_all_adapters",
]

# Skill-storage contract name used by the architecture docs; BaseAdapter kept for compatibility.
SkillStorageAdapter = BaseAdapter


def get_all_adapters(config: Config) -> Dict[str, BaseAdapter]:
    """Instantiate and return all supported agent adapters."""
    return {
        "antigravity-cli": AntigravityCLIAdapter(config.antigravity_cli_path),
        "antigravity-workspace": AntigravityWorkspaceAdapter(config.antigravity_workspace_path),
        "codex-cli": CodexCLIAdapter(config.codex_cli_path),
        "antigravity-ide": AntigravityIDEAdapter(config.antigravity_ide_path),
        "claude-code": ClaudeCodeSkillAdapter(config.claude_skills_path),
    }
