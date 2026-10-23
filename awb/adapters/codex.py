"""
OpenAI Codex CLI adapter.
Guarantees strict protection of ~/.codex/skills/.system/ internal skills.
"""

import shutil
from pathlib import Path
from typing import List

from awb.errors import AdapterError
from .base import BaseAdapter, ConformanceReport


class CodexCLIAdapter(BaseAdapter):
    """Adapter for OpenAI Codex CLI skill discovery (~/.codex/skills/)."""

    SYSTEM_DIR_NAME = ".system"

    def __init__(self, target_path: Path):
        self._target_path = Path(target_path)
        # Defense-in-depth: Target path must never point directly to or reside inside .system/
        canon = self._target_path.resolve()
        if canon.name == self.SYSTEM_DIR_NAME or self.SYSTEM_DIR_NAME in canon.parts:
            raise AdapterError(
                f"Codex adapter target path cannot be or reside within '{self.SYSTEM_DIR_NAME}': '{target_path}'"
            )

    @property
    def name(self) -> str:
        return "codex-cli"

    @property
    def display_name(self) -> str:
        return "OpenAI Codex CLI (~/.codex/skills)"

    @property
    def target_type(self) -> str:
        return "cli"

    @property
    def discovery_path(self) -> Path:
        return self._target_path

    def is_detected(self) -> bool:
        """Non-mutating probe: checks binary in PATH or ~/.codex directory existence."""
        has_binary = shutil.which("codex") is not None
        has_dir = self._target_path.parent.is_dir()
        return has_binary or has_dir

    def list_installed_skills(self) -> List[str]:
        """
        List user skills in ~/.codex/skills/ without mutating filesystem.
        Invariants:
        1. Strictly ignores .system/ and hidden files.
        2. Strictly rejects symlinked skill entries.
        3. Strictly rejects entries resolving inside .system/ or outside target root.
        """
        if not self._target_path.is_dir():
            return []
        skills: List[str] = []
        canon_root = self._target_path.resolve()
        canon_system = (self._target_path / self.SYSTEM_DIR_NAME).resolve()

        try:
            for entry in sorted(self._target_path.iterdir()):
                # Exclude hidden entries
                if entry.name.startswith("."):
                    continue

                # Reject symlinked skill directories (prevents aliasing and directory escapes)
                if entry.is_symlink():
                    continue

                # Must be a regular directory
                if not entry.is_dir():
                    continue

                # Boundary and .system isolation check on resolved path
                try:
                    canon_entry = entry.resolve()
                except OSError:
                    continue

                if canon_entry == canon_system or canon_system in canon_entry.parents:
                    continue

                if canon_entry != canon_root and canon_root not in canon_entry.parents:
                    continue

                # Verify entrypoint: SKILL.md must be a regular file, not a symlink
                skill_md = entry / "SKILL.md"
                if skill_md.is_file() and not skill_md.is_symlink():
                    skills.append(entry.name)
        except OSError:
            pass
        return skills

    def validate_conformance(self, sandbox_root: Path) -> ConformanceReport:
        """
        Verify Codex adapter structure and explicit .system/ protection boundary.
        """
        checks: List[str] = []
        errors: List[str] = []

        checks.append("Verify sandbox directory")
        if not sandbox_root.is_dir():
            errors.append(f"Sandbox root '{sandbox_root}' is not a directory")

        # 1. Simulate presence of .system/ in sandbox
        simulated_system = sandbox_root / self.SYSTEM_DIR_NAME
        simulated_system.mkdir(parents=True, exist_ok=True)
        (simulated_system / "SKILL.md").write_text("system-skill", encoding="utf-8")

        # 2. Simulate malicious symlink pointing to .system/
        symlink_alias = sandbox_root / "sneaky_alias"
        if not symlink_alias.exists():
            try:
                symlink_alias.symlink_to(simulated_system)
            except OSError:
                pass

        # 3. Test list_installed_skills isolation
        temp_adapter = CodexCLIAdapter(sandbox_root)
        installed = temp_adapter.list_installed_skills()

        checks.append("Verify .system exclusion invariant")
        if self.SYSTEM_DIR_NAME in installed or any(s.startswith(".") for s in installed):
            errors.append(f"CRITICAL SAFETY VIOLATION: '{self.SYSTEM_DIR_NAME}' exposed in installed skills")

        checks.append("Verify symlink alias rejection invariant")
        if "sneaky_alias" in installed:
            errors.append("CRITICAL SAFETY VIOLATION: Symlink alias to .system was accepted as installed skill")

        # 4. Test direct initialization rejection against .system
        checks.append("Verify direct .system path initialization rejection")
        try:
            CodexCLIAdapter(simulated_system)
            errors.append("CRITICAL SAFETY VIOLATION: CodexCLIAdapter accepted target path inside .system")
        except AdapterError:
            pass

        return ConformanceReport(
            adapter_name=self.name,
            target_path=self._target_path,
            passed=(len(errors) == 0),
            checks=checks,
            errors=errors,
        )
