"""
Tests for the skill-storage vs execution-provider separation (ADR-0002),
the Claude Code skill-storage adapter, and Claude/Agents global-path hardening.
"""

import io
import json
import os
import re
import tempfile
import unittest
from contextlib import redirect_stdout
from dataclasses import FrozenInstanceError
from pathlib import Path
from unittest import mock

from awb.adapters import ClaudeCodeSkillAdapter, SkillStorageAdapter, get_all_adapters
from awb.cli import main
from awb.config import Config
from awb.errors import GlobalMutationBlockedError
from awb.providers import (
    PROVIDERS,
    ExecutionProvider,
    ProviderNotEnabledError,
    get_provider,
)
from awb.registry.schema import SkillDefinition
from awb.registry.validator import RegistryValidator
from awb.safety.boundaries import GLOBAL_AGENT_SKILL_PATHS, validate_security_preconditions

REPO_ROOT = Path(__file__).resolve().parent.parent


class TestClaudeCodeSkillAdapter(unittest.TestCase):
    """Read-only discovery of Claude Code personal skills."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(prefix="awb_test_claude_")
        self.root = Path(self.temp_dir.name) / "skills"
        self.root.mkdir()

    def tearDown(self):
        self.temp_dir.cleanup()

    def _skill(self, name: str, with_md: bool = True) -> Path:
        d = self.root / name
        d.mkdir()
        if with_md:
            (d / "SKILL.md").write_text("---\nname: x\ndescription: y\n---\n", encoding="utf-8")
        return d

    def test_lists_only_regular_skill_directories(self):
        self._skill("alpha")
        self._skill("beta")
        self._skill("no-entry", with_md=False)
        self._skill(".hidden")
        self._skill(".system")
        (self.root / "loose-file.md").write_text("x", encoding="utf-8")
        outside = Path(self.temp_dir.name) / "outside"
        outside.mkdir()
        (outside / "SKILL.md").write_text("x", encoding="utf-8")
        (self.root / "linked").symlink_to(outside)

        adapter = ClaudeCodeSkillAdapter(self.root)
        self.assertEqual(adapter.list_installed_skills(), ["alpha", "beta"])

    def test_rejects_symlinked_skill_md(self):
        d = self._skill("sneaky", with_md=False)
        real = Path(self.temp_dir.name) / "real.md"
        real.write_text("x", encoding="utf-8")
        (d / "SKILL.md").symlink_to(real)
        self.assertEqual(ClaudeCodeSkillAdapter(self.root).list_installed_skills(), [])

    def test_missing_directory_lists_nothing(self):
        adapter = ClaudeCodeSkillAdapter(self.root / "absent")
        self.assertEqual(adapter.list_installed_skills(), [])

    def test_identity(self):
        adapter = ClaudeCodeSkillAdapter(self.root)
        self.assertEqual(adapter.name, "claude-code")
        self.assertEqual(adapter.target_type, "cli")
        self.assertEqual(adapter.discovery_path, self.root)
        self.assertIsInstance(adapter, SkillStorageAdapter)

    def test_detection_is_read_only_probe(self):
        with mock.patch("shutil.which", return_value=None):
            self.assertFalse(ClaudeCodeSkillAdapter(self.root / "a" / "skills").is_detected())
            self.assertTrue(ClaudeCodeSkillAdapter(self.root).is_detected())

    def test_conformance_writes_only_inside_sandbox(self):
        sandbox = Path(self.temp_dir.name) / "sandbox"
        sandbox.mkdir()
        report = ClaudeCodeSkillAdapter(self.root).validate_conformance(sandbox)
        self.assertTrue(report.passed, report.errors)
        self.assertEqual(sorted(p.name for p in self.root.iterdir()), [])

    def test_registered_with_configured_path(self):
        with mock.patch.dict(os.environ, {"AURORA_CLAUDE_SKILLS_PATH": str(self.root)}):
            adapters = get_all_adapters(Config.load())
        self.assertIn("claude-code", adapters)
        self.assertEqual(adapters["claude-code"].discovery_path, self.root)

    def test_config_env_precedence(self):
        env = {"AURORA_CLAUDE_SKILLS_PATH": "/tmp/aurora-a", "AWB_CLAUDE_SKILLS_PATH": "/tmp/awb-b"}
        with mock.patch.dict(os.environ, env):
            self.assertEqual(Config.load().claude_skills_path, Path("/tmp/aurora-a"))
        with mock.patch.dict(os.environ, {"AWB_CLAUDE_SKILLS_PATH": "/tmp/awb-b"}):
            os.environ.pop("AURORA_CLAUDE_SKILLS_PATH", None)
            self.assertEqual(Config.load().claude_skills_path, Path("/tmp/awb-b"))
        with mock.patch.dict(os.environ, {}, clear=False):
            os.environ.pop("AURORA_CLAUDE_SKILLS_PATH", None)
            os.environ.pop("AWB_CLAUDE_SKILLS_PATH", None)
            self.assertEqual(Config.load().claude_skills_path, Path.home() / ".claude" / "skills")

    def test_registry_accepts_claude_code_agent(self):
        skill = SkillDefinition(
            id="x", name="X", version="1.0.0", description="d", compatible_agents=["claude-code"]
        )
        RegistryValidator.validate_skill(skill)


class TestGlobalPathHardening(unittest.TestCase):
    """Claude Code and shared Agents user skill dirs are blocked like other global dirs."""

    def test_claude_and_agents_dirs_are_in_global_block_list(self):
        home = Path.home()
        self.assertIn(home / ".claude" / "skills", GLOBAL_AGENT_SKILL_PATHS)
        self.assertIn(home / ".agents" / "skills", GLOBAL_AGENT_SKILL_PATHS)

    def test_mutation_targets_are_rejected(self):
        home = Path.home()
        for target in (home / ".claude" / "skills", home / ".claude" / "skills" / "x",
                       home / ".agents" / "skills"):
            with self.assertRaises(GlobalMutationBlockedError):
                validate_security_preconditions(target, [home / ".claude"], allow_global=False)

    def test_cli_install_into_claude_adapter_is_blocked(self):
        # --dry-run: if the guard ever regresses, this fails without writing to the live directory.
        f = io.StringIO()
        with redirect_stdout(f), mock.patch("sys.stderr", f):
            code = main(["install", "principles", "--adapter", "claude-code", "--dry-run",
                         "--root", str(Path.home() / ".claude")])
        self.assertEqual(code, 1)
        self.assertIn("blocked", f.getvalue())


class TestExecutionProviders(unittest.TestCase):
    """Execution providers: manual-handoff only, no process spawning, no bypass flags."""

    def test_known_providers(self):
        self.assertEqual(sorted(PROVIDERS), ["antigravity", "claude", "codex"])
        self.assertEqual(get_provider("claude").spec.binary, "claude")
        self.assertEqual(get_provider("codex").spec.binary, "codex")
        self.assertEqual(get_provider("antigravity").spec.binary, "agy")

    def test_unknown_provider_fails_clearly(self):
        with self.assertRaises(KeyError) as ctx:
            get_provider("gpt-cli")
        self.assertIn("gpt-cli", str(ctx.exception))
        self.assertIn("claude", str(ctx.exception))

    def test_specs_are_immutable_and_manual_only(self):
        for provider in PROVIDERS.values():
            spec = provider.spec
            self.assertEqual(spec.invocation_modes, ("manual-handoff",))
            self.assertFalse(spec.headless_enabled)
            self.assertTrue(spec.forbidden_flags)
            self.assertTrue(spec.docs_urls)
            with self.assertRaises(FrozenInstanceError):
                spec.headless_enabled = True  # type: ignore[misc]

    def test_provider_skill_adapters_exist(self):
        adapter_ids = set(get_all_adapters(Config.load()))
        for provider in PROVIDERS.values():
            self.assertTrue(provider.spec.skill_adapters)
            self.assertLessEqual(set(provider.spec.skill_adapters), adapter_ids)

    def test_contracts_are_structurally_separate(self):
        for provider in PROVIDERS.values():
            self.assertFalse(hasattr(provider, "discovery_path"))
            self.assertFalse(hasattr(provider, "list_installed_skills"))
        for adapter in get_all_adapters(Config.load()).values():
            self.assertFalse(hasattr(adapter, "execute"))
            self.assertFalse(hasattr(adapter, "build_handoff"))
            self.assertNotIsInstance(adapter, ExecutionProvider)

    def test_execute_is_not_enabled(self):
        for provider in PROVIDERS.values():
            with self.assertRaises(ProviderNotEnabledError):
                provider.execute(prompt="x", cwd=Path("/tmp"))

    def test_detect_never_spawns_processes(self):
        boom = mock.Mock(side_effect=AssertionError("process spawned"))
        with mock.patch("subprocess.Popen", boom), mock.patch("subprocess.run", boom), \
                mock.patch("os.system", boom), \
                mock.patch("shutil.which", side_effect=lambda b: f"/usr/bin/{b}" if b == "claude" else None):
            results = {pid: p.detect() for pid, p in PROVIDERS.items()}
        self.assertTrue(results["claude"].installed)
        self.assertEqual(results["claude"].path, "/usr/bin/claude")
        self.assertFalse(results["codex"].installed)
        self.assertIsNone(results["codex"].version)
        boom.assert_not_called()

    def test_handoff_text_never_contains_forbidden_flags(self):
        for provider in PROVIDERS.values():
            text = provider.build_handoff("Do the planning.")
            self.assertIn("Do the planning.", text)
            for flag in provider.spec.forbidden_flags:
                self.assertNotIn(flag, text)

    def test_new_packages_do_not_import_subprocess(self):
        pattern = re.compile(r"^\s*(import|from)\s+subprocess\b", re.MULTILINE)
        for pkg in ("providers", "workflow"):
            pkg_dir = REPO_ROOT / "awb" / pkg
            if not pkg_dir.is_dir():
                continue
            for src in pkg_dir.rglob("*.py"):
                self.assertIsNone(pattern.search(src.read_text(encoding="utf-8")), str(src))

    def test_cli_providers_json(self):
        f = io.StringIO()
        with redirect_stdout(f), mock.patch("shutil.which", return_value=None):
            code = main(["providers", "--json"])
        self.assertEqual(code, 0)
        data = json.loads(f.getvalue())
        self.assertEqual([p["id"] for p in data], ["antigravity", "claude", "codex"])
        for p in data:
            self.assertFalse(p["installed"])
            self.assertFalse(p["headless_enabled"])
            self.assertEqual(p["invocation_modes"], ["manual-handoff"])


if __name__ == "__main__":
    unittest.main()
