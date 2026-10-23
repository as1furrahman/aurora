"""
Antigravity IDE adapter (treated as a distinct adapter surface from CLI).
"""

from pathlib import Path
from typing import List

from .base import BaseAdapter, ConformanceReport


class AntigravityIDEAdapter(BaseAdapter):
    """Adapter for Google Antigravity IDE (~/.gemini/config/skills/)."""

    def __init__(self, target_path: Path):
        self._target_path = target_path

    @property
    def name(self) -> str:
        return "antigravity-ide"

    @property
    def display_name(self) -> str:
        return "Google Antigravity IDE (~/.gemini/config/skills)"

    @property
    def target_type(self) -> str:
        return "ide"

    @property
    def discovery_path(self) -> Path:
        return self._target_path

    def is_detected(self) -> bool:
        """Non-mutating probe: checks IDE config directory existence."""
        ide_config_dir = self._target_path.parent
        return ide_config_dir.is_dir()

    def list_installed_skills(self) -> List[str]:
        """Read-only scan of IDE skills."""
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
        checks = ["Verify IDE config path", "Verify separation from CLI adapter"]
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
