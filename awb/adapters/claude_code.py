"""
Claude Code skill-storage adapter.
Read-only discovery of personal skills (~/.claude/skills/<name>/SKILL.md).
Source: https://code.claude.com/docs/en/skills (accessed 2026-10-23).
"""

import shutil
from pathlib import Path
from typing import List

from .base import BaseAdapter, ConformanceReport


class ClaudeCodeSkillAdapter(BaseAdapter):
    """Adapter for Claude Code personal skill discovery (~/.claude/skills/)."""

    def __init__(self, target_path: Path):
        self._target_path = Path(target_path)

    @property
    def name(self) -> str:
        return "claude-code"

    @property
    def display_name(self) -> str:
        return "Claude Code (~/.claude/skills)"

    @property
    def target_type(self) -> str:
        return "cli"

    @property
    def discovery_path(self) -> Path:
        return self._target_path

    def is_detected(self) -> bool:
        """Non-mutating probe: binary in PATH or skills directory present."""
        return shutil.which("claude") is not None or self._target_path.is_dir()

    def list_installed_skills(self) -> List[str]:
        """
        List skills without mutating the filesystem.
        Skips hidden entries, symlinks, non-directories, entries resolving outside
        the root, and directories lacking a regular-file SKILL.md.
        """
        if not self._target_path.is_dir():
            return []
        skills: List[str] = []
        canon_root = self._target_path.resolve()
        try:
            for entry in sorted(self._target_path.iterdir()):
                if entry.name.startswith(".") or entry.is_symlink() or not entry.is_dir():
                    continue
                try:
                    canon_entry = entry.resolve()
                except OSError:
                    continue
                if canon_root not in canon_entry.parents:
                    continue
                skill_md = entry / "SKILL.md"
                if skill_md.is_file() and not skill_md.is_symlink():
                    skills.append(entry.name)
        except OSError:
            pass
        return skills

    def validate_conformance(self, sandbox_root: Path) -> ConformanceReport:
        """Verify discovery invariants using fixtures created only inside sandbox_root."""
        checks: List[str] = []
        errors: List[str] = []

        checks.append("Verify sandbox directory")
        if not sandbox_root.is_dir():
            errors.append(f"Sandbox root '{sandbox_root}' is not a directory")
            return ConformanceReport(self.name, self._target_path, False, checks, errors)

        fixture = sandbox_root / "claude_skills"
        (fixture / "valid").mkdir(parents=True, exist_ok=True)
        (fixture / "valid" / "SKILL.md").write_text("---\ndescription: d\n---\n", encoding="utf-8")
        (fixture / ".hidden").mkdir(exist_ok=True)
        (fixture / ".hidden" / "SKILL.md").write_text("x", encoding="utf-8")
        alias = fixture / "alias"
        if not alias.exists():
            try:
                alias.symlink_to(fixture / "valid")
            except OSError:
                pass

        installed = ClaudeCodeSkillAdapter(fixture).list_installed_skills()

        checks.append("Verify valid skill discovered")
        if installed != ["valid"]:
            errors.append(f"Expected ['valid'], discovered {installed}")

        return ConformanceReport(
            adapter_name=self.name,
            target_path=self._target_path,
            passed=(len(errors) == 0),
            checks=checks,
            errors=errors,
        )
