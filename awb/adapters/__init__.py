"""
awb.adapters - Multi-agent skill adapters for Antigravity and Codex.
"""

from typing import Dict

from awb.config import Config
from .antigravity import AntigravityCLIAdapter, AntigravityWorkspaceAdapter
from .antigravity_ide import AntigravityIDEAdapter
from .base import BaseAdapter, ConformanceReport
from .codex import CodexCLIAdapter

__all__ = [
    "BaseAdapter",
    "ConformanceReport",
    "AntigravityCLIAdapter",
    "AntigravityWorkspaceAdapter",
    "CodexCLIAdapter",
    "AntigravityIDEAdapter",
    "get_all_adapters",
]


def get_all_adapters(config: Config) -> Dict[str, BaseAdapter]:
    """Instantiate and return all supported agent adapters."""
    return {
        "antigravity-cli": AntigravityCLIAdapter(config.antigravity_cli_path),
        "antigravity-workspace": AntigravityWorkspaceAdapter(config.antigravity_workspace_path),
        "codex-cli": CodexCLIAdapter(config.codex_cli_path),
        "antigravity-ide": AntigravityIDEAdapter(config.antigravity_ide_path),
    }
