"""
Unit tests for awb CLI commands.
"""

import io
import sys
import unittest
from contextlib import redirect_stdout
from pathlib import Path

from awb.cli import main


class TestCLI(unittest.TestCase):
    """Test suite for awb command-line interface."""

    def test_cli_version(self):
        """CLI --version prints version and exits cleanly."""
        with self.assertRaises(SystemExit) as ctx:
            main(["--version"])
        self.assertEqual(ctx.exception.code, 0)

    def test_cli_help(self):
        """CLI --help prints usage and exits cleanly."""
        with self.assertRaises(SystemExit) as ctx:
            main(["--help"])
        self.assertEqual(ctx.exception.code, 0)

    def test_cmd_detect(self):
        """aurora detect executes non-mutating detection and returns code 0."""
        f = io.StringIO()
        with redirect_stdout(f):
            code = main(["detect"])
        self.assertEqual(code, 0)
        output = f.getvalue()
        self.assertIn("Aurora — Environment Detection", output)
        self.assertIn("antigravity-cli", output)
        self.assertIn("codex-cli", output)

    def test_cmd_detect_json(self):
        """awb detect --json outputs valid JSON."""
        import json
        f = io.StringIO()
        with redirect_stdout(f):
            code = main(["detect", "--json"])
        self.assertEqual(code, 0)
        data = json.loads(f.getvalue())
        self.assertIsInstance(data, list)
        self.assertEqual(len(data), 4)

    def test_cmd_list(self):
        """awb list displays canonical skills from registry/skills.toml."""
        f = io.StringIO()
        with redirect_stdout(f):
            code = main(["list"])
        self.assertEqual(code, 0)
        output = f.getvalue()
        self.assertIn("Canonical Skill Registry", output)
        self.assertIn("principles", output)
        self.assertIn("token-efficient-workflow", output)

    def test_cmd_validate(self):
        """awb validate verifies canonical registry and prints resolution order."""
        f = io.StringIO()
        with redirect_stdout(f):
            code = main(["validate"])
        self.assertEqual(code, 0)
        output = f.getvalue()
        self.assertIn("VALIDATION PASSED", output)
        self.assertIn("Topological Resolution Order", output)

    def test_cmd_doctor(self):
        """awb doctor performs diagnostics without error."""
        f = io.StringIO()
        with redirect_stdout(f):
            code = main(["doctor"])
        self.assertEqual(code, 0)
        output = f.getvalue()
        self.assertIn("Aurora — System Diagnostics (Doctor)", output)
        self.assertIn("Python Runtime", output)
        self.assertIn("tomllib available", output)

    def test_cmd_install_dry_run(self):
        """awb install --dry-run prints plan and leaves target unmodified when trusted root is provided."""
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            tmp_p = Path(tmp)
            f = io.StringIO()
            with redirect_stdout(f):
                code = main(["install", "principles", "--target", str(tmp_p), "--root", str(tmp_p), "--dry-run"])
            self.assertEqual(code, 0)
            output = f.getvalue()
            self.assertIn("Skill Installation Plan (Dry Run)", output)
            self.assertIn("principles", output)
            self.assertFalse((tmp_p / "principles").exists())

    def test_cmd_install_and_remove_lifecycle(self):
        """awb install and awb remove full lifecycle via CLI in isolated sandbox with explicit root."""
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            tmp_p = Path(tmp)
            # 1. Install
            f_inst = io.StringIO()
            with redirect_stdout(f_inst):
                code_inst = main(["install", "principles", "--target", str(tmp_p), "--root", str(tmp_p)])
            self.assertEqual(code_inst, 0)
            self.assertIn("Skill Installation Successful", f_inst.getvalue())
            self.assertTrue((tmp_p / "principles" / "SKILL.md").is_file())

            # 2. Remove dry-run
            f_rem_dry = io.StringIO()
            with redirect_stdout(f_rem_dry):
                code_rem_dry = main(["remove", "principles", "--target", str(tmp_p), "--root", str(tmp_p), "--dry-run"])
            self.assertEqual(code_rem_dry, 0)
            self.assertIn("Skill Removal Plan (Dry Run)", f_rem_dry.getvalue())
            self.assertTrue((tmp_p / "principles" / "SKILL.md").is_file())

            # 3. Remove execution
            f_rem = io.StringIO()
            with redirect_stdout(f_rem):
                code_rem = main(["remove", "principles", "--target", str(tmp_p), "--root", str(tmp_p)])
            self.assertEqual(code_rem, 0)
            self.assertIn("successfully removed", f_rem.getvalue())
            self.assertFalse((tmp_p / "principles").exists())

    def test_cmd_install_destination_required(self):
        """awb install without --target or --adapter fails with clear error."""
        f_err = io.StringIO()
        with io.StringIO() as f_out:
            with redirect_stdout(f_out):
                code = main(["install", "principles"])
        self.assertEqual(code, 1)

    def test_cmd_install_fails_without_explicit_root(self):
        """SEC-04: awb install with --target but without --root fails closed."""
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            tmp_p = Path(tmp)
            code = main(["install", "principles", "--target", str(tmp_p)])
            self.assertEqual(code, 1)

    def test_cmd_install_rejects_protected_project_targets(self):
        """SEC-04: awb install targeting protected project directories is rejected."""
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            tmp_p = Path(tmp)
            # Attempt to target protected project directory
            code = main([
                "install", "principles",
                "--target", "/home/asif/projects/token-efficient-workflow",
                "--root", str(tmp_p),
            ])
            self.assertEqual(code, 1)

    def test_cmd_install_global_blocked(self):
        """awb install targeting global adapter fails closed with GlobalMutationBlockedError."""
        f_err = io.StringIO()
        with redirect_stdout(f_err):
            code = main(["install", "principles", "--adapter", "codex-cli", "--root", str(Path.home())])
        self.assertEqual(code, 1)

    def test_canonical_aurora_and_compatibility_awb_alias(self):
        """Verify that CLI functions identically under both canonical 'aurora' and alias 'awb'."""
        f_aurora = io.StringIO()
        with redirect_stdout(f_aurora):
            code_aurora = main(["detect"], prog="aurora")
        self.assertEqual(code_aurora, 0)
        out_aurora = f_aurora.getvalue()
        self.assertIn("Aurora — Environment Detection", out_aurora)

        f_awb = io.StringIO()
        with redirect_stdout(f_awb):
            code_awb = main(["detect"], prog="awb")
        self.assertEqual(code_awb, 0)
        out_awb = f_awb.getvalue()
        self.assertIn("Aurora — Environment Detection", out_awb)

    def test_cli_help_for_both_aliases(self):
        """CLI --help prints appropriate prog name for both aurora and awb."""
        f_aurora = io.StringIO()
        with redirect_stdout(f_aurora):
            with self.assertRaises(SystemExit):
                main(["--help"], prog="aurora")
        self.assertIn("usage: aurora", f_aurora.getvalue())

        f_awb = io.StringIO()
        with redirect_stdout(f_awb):
            with self.assertRaises(SystemExit):
                main(["--help"], prog="awb")
        self.assertIn("usage: awb", f_awb.getvalue())


if __name__ == "__main__":
    unittest.main()
