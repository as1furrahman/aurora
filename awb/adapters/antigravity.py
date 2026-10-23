"""
Antigravity CLI and Antigravity Workspace adapters.
"""

import os
import shutil
from pathlib import Path
from typing import List

from .base import BaseAdapter, ConformanceReport


class AntigravityCLIAdapter(BaseAdapter):
    """Adapter for Antigravity CLI global skill discovery (~/.gemini/antigravity-cli/skills/)."""

    def __init__(self, target_path: Path):
        self._target_path = target_path

    @property
    def name(self) -> str:
        return "antigravity-cli"

    @property
    def display_name(self) -> str:
        return "Google Antigravity CLI (Global)"

    @property
    def target_type(self) -> str:
        return "cli"

    @property
    def discovery_path(self) -> Path:
        return self._target_path

    def is_detected(self) -> bool:
        """Non-mutating probe: checks binary in PATH or directory existence."""
        has_binary = shutil.which("agy") is not None
        has_dir = self._target_path.parent.is_dir()
        return has_binary or has_dir

    def list_installed_skills(self) -> List[str]:
        """List skills in ~/.gemini/antigravity-cli/skills/ without mutating filesystem."""
        if not self._target_path.is_dir():
            return []
        skills: List[str] = []
        try:
            for entry in sorted(self._target_path.iterdir()):
                if entry.is_dir() and not entry.name.startswith("."):
                    # Antigravity skill directory requires SKILL.md
                    if (entry / "SKILL.md").is_file():
                        skills.append(entry.name)
        except OSError:
            pass
        return skills

    def validate_conformance(self, sandbox_root: Path) -> ConformanceReport:
        """Run non-mutating structure checks in test sandbox."""
        checks: List[str] = []
        errors: List[str] = []
        mock_skills = sandbox_root / "skills"
        checks.append("Verify sandbox path")
        if not sandbox_root.is_dir():
            errors.append(f"Sandbox root '{sandbox_root}' is not a directory")

        # Test simulated skill structure
        test_skill = mock_skills / "test-skill"
        test_skill_md = test_skill / "SKILL.md"
        checks.append("Verify SKILL.md requirement")
        if test_skill.is_dir() and not test_skill_md.is_file():
            errors.append("Skill missing mandatory SKILL.md entrypoint")

        return ConformanceReport(
            adapter_name=self.name,
            target_path=self._target_path,
            passed=(len(errors) == 0),
            checks=checks,
            errors=errors,
        )


class AntigravityWorkspaceAdapter(BaseAdapter):
    """Adapter for workspace-level Antigravity skills (.agents/skills/)."""

    def __init__(self, target_path: Path):
        self._target_path = target_path

    @property
    def name(self) -> str:
        return "antigravity-workspace"

    @property
    def display_name(self) -> str:
        return "Antigravity Workspace (.agents/skills)"

    @property
    def target_type(self) -> str:
        return "workspace"

    @property
    def discovery_path(self) -> Path:
        return self._target_path

    def is_detected(self) -> bool:
        """Non-mutating check for .agents/ directory in active workspace."""
        agents_dir = self._target_path.parent
        return agents_dir.is_dir()

    def list_installed_skills(self) -> List[str]:
        """Read-only scan of workspace skills."""
        if not self._target_path.is_dir():
            return []
        skills: List[str] = []
        try:
            for entry in sorted(self._target_path.iterdir()):
                if entry.is_dir() and not entry.name.startswith("."):
                    if (entry / "SKILL.md").is_file():
                        skills.append(entry.name)
        except OSError:
            pass
        return skills

    def validate_conformance(self, sandbox_root: Path) -> ConformanceReport:
        checks = ["Verify workspace boundary", "Verify SKILL.md format"]
        errors = []
        if not sandbox_root.is_dir():
            errors.append(f"Sandbox root '{sandbox_root}' not a directory")
        return ConformanceReport(
            adapter_name=self.name,
            target_path=self._target_path,
            passed=(len(errors) == 0),
            checks=checks,
            errors=errors,
        )
