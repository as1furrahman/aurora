"""
Unit and conformance tests for agent adapters.
Verifies non-mutating discovery, sandboxed conformance, and .system/ protection.
"""

import tempfile
import unittest
from pathlib import Path

from awb.adapters import (
    AntigravityCLIAdapter,
    AntigravityIDEAdapter,
    AntigravityWorkspaceAdapter,
    CodexCLIAdapter,
    get_all_adapters,
)
from awb.config import Config


class TestAdapters(unittest.TestCase):
    """Test suite for awb.adapters."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(prefix="awb_test_adapters_")
        self.sandbox = Path(self.temp_dir.name)

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_adapter_paths_are_strictly_distinct(self):
        """Ensure all adapter discovery paths are distinct and non-overlapping."""
        cfg = Config.load()
        adapters = get_all_adapters(cfg)

        paths = [str(a.discovery_path) for a in adapters.values()]
        self.assertEqual(len(paths), len(set(paths)), "Adapter paths must be unique")

        # Distinct check between Antigravity CLI and IDE
        ag_cli_path = adapters["antigravity-cli"].discovery_path
        ag_ide_path = adapters["antigravity-ide"].discovery_path
        self.assertNotEqual(ag_cli_path, ag_ide_path)
        self.assertIn("antigravity-cli", str(ag_cli_path))
        self.assertIn("config", str(ag_ide_path))

    def test_codex_adapter_strictly_protects_system_directory(self):
        """Codex adapter must never list or expose .system/ internal skills."""
        codex_skills_dir = self.sandbox / "codex_skills"
        codex_skills_dir.mkdir(parents=True, exist_ok=True)

        # Create .system directory with internal files
        sys_dir = codex_skills_dir / ".system"
        sys_dir.mkdir(parents=True, exist_ok=True)
        (sys_dir / "SKILL.md").write_text("# System internal skill", encoding="utf-8")

        # Create legitimate user skill
        user_skill = codex_skills_dir / "user-custom-skill"
        user_skill.mkdir(parents=True, exist_ok=True)
        (user_skill / "SKILL.md").write_text("# User skill", encoding="utf-8")

        adapter = CodexCLIAdapter(codex_skills_dir)
        installed = adapter.list_installed_skills()

        self.assertIn("user-custom-skill", installed)
        self.assertNotIn(".system", installed)
        self.assertFalse(any(s.startswith(".") for s in installed))

    def test_codex_adapter_rejects_symlink_aliases_to_system(self):
        """Reject symlinked skill entries aliasing .system via direct or relative traversal."""
        codex_skills_dir = self.sandbox / "codex_symlink_skills"
        codex_skills_dir.mkdir(parents=True, exist_ok=True)

        sys_dir = codex_skills_dir / ".system"
        sys_dir.mkdir(parents=True, exist_ok=True)
        (sys_dir / "SKILL.md").write_text("# Internal", encoding="utf-8")

        # 1. Direct symlink alias
        (codex_skills_dir / "direct_alias").symlink_to(sys_dir)

        # 2. Relative traversal symlink
        (codex_skills_dir / "rel_alias").symlink_to(Path("../codex_symlink_skills/.system"))

        # 3. Legitimate skill
        legit = codex_skills_dir / "legit-skill"
        legit.mkdir(parents=True, exist_ok=True)
        (legit / "SKILL.md").write_text("# Legit", encoding="utf-8")

        adapter = CodexCLIAdapter(codex_skills_dir)
        installed = adapter.list_installed_skills()

        self.assertEqual(installed, ["legit-skill"])
        self.assertNotIn("direct_alias", installed)
        self.assertNotIn("rel_alias", installed)

    def test_codex_adapter_rejects_target_path_inside_system(self):
        """Adapter initialization must fail closed if target path points to or inside .system."""
        from awb.errors import AdapterError

        codex_root = self.sandbox / "codex_root"
        sys_dir = codex_root / ".system"
        sys_sub = sys_dir / "nested"
        sys_sub.mkdir(parents=True, exist_ok=True)

        with self.assertRaises(AdapterError):
            CodexCLIAdapter(sys_dir)

        with self.assertRaises(AdapterError):
            CodexCLIAdapter(sys_sub)

    def test_antigravity_cli_discovery_in_sandbox(self):
        """Antigravity CLI adapter discovers skills with valid SKILL.md."""
        ag_dir = self.sandbox / "ag_skills"
        ag_dir.mkdir(parents=True, exist_ok=True)

        # Valid skill
        valid = ag_dir / "valid-skill"
        valid.mkdir(parents=True, exist_ok=True)
        (valid / "SKILL.md").write_text("# Valid Skill", encoding="utf-8")

        # Incomplete skill (no SKILL.md)
        incomplete = ag_dir / "incomplete-skill"
        incomplete.mkdir(parents=True, exist_ok=True)

        adapter = AntigravityCLIAdapter(ag_dir)
        installed = adapter.list_installed_skills()

        self.assertIn("valid-skill", installed)
        self.assertNotIn("incomplete-skill", installed)

    def test_conformance_suite_all_adapters(self):
        """Run non-mutating conformance tests for all adapters against isolated sandboxes."""
        cfg = Config.load()
        adapters = get_all_adapters(cfg)

        for name, adapter in adapters.items():
            sb = self.sandbox / name
            sb.mkdir(parents=True, exist_ok=True)
            report = adapter.validate_conformance(sb)
            self.assertTrue(report.passed, f"Adapter {name} conformance failed: {report.errors}")
            self.assertGreaterEqual(len(report.checks), 1)


if __name__ == "__main__":
    unittest.main()
