"""
Configuration and environment management for Aurora (compatibility alias: AWB).
"""

import os
from dataclasses import dataclass
from pathlib import Path
from typing import Optional


@dataclass(frozen=True)
class Config:
    """Immutable workbench configuration with environment variable overrides."""

    project_root: Path
    registry_path: Path
    antigravity_cli_path: Path
    antigravity_workspace_path: Path
    antigravity_ide_path: Path
    codex_cli_path: Path

    @classmethod
    def load(cls, custom_root: Optional[Path] = None) -> "Config":
        """Load configuration from environment (AURORA_* with AWB_* fallback) or defaults."""
        root = custom_root or Path(__file__).resolve().parent.parent
        user_home = Path.home()

        registry_env = os.environ.get("AURORA_REGISTRY_PATH") or os.environ.get("AWB_REGISTRY_PATH")
        registry_path = Path(registry_env) if registry_env else (root / "registry" / "skills.toml")

        ag_cli_env = os.environ.get("AURORA_ANTIGRAVITY_CLI_PATH") or os.environ.get("AWB_ANTIGRAVITY_CLI_PATH")
        ag_cli_path = Path(ag_cli_env) if ag_cli_env else (user_home / ".gemini" / "antigravity-cli" / "skills")

        ag_ws_env = os.environ.get("AURORA_ANTIGRAVITY_WORKSPACE_PATH") or os.environ.get("AWB_ANTIGRAVITY_WORKSPACE_PATH")
        ag_ws_path = Path(ag_ws_env) if ag_ws_env else (Path.cwd() / ".agents" / "skills")

        ag_ide_env = os.environ.get("AURORA_ANTIGRAVITY_IDE_PATH") or os.environ.get("AWB_ANTIGRAVITY_IDE_PATH")
        ag_ide_path = Path(ag_ide_env) if ag_ide_env else (user_home / ".gemini" / "config" / "skills")

        codex_env = os.environ.get("AURORA_CODEX_CLI_PATH") or os.environ.get("AWB_CODEX_CLI_PATH")
        codex_path = Path(codex_env) if codex_env else (user_home / ".codex" / "skills")

        return cls(
            project_root=root,
            registry_path=registry_path,
            antigravity_cli_path=ag_cli_path,
            antigravity_workspace_path=ag_ws_path,
            antigravity_ide_path=ag_ide_path,
            codex_cli_path=codex_path,
        )
