"""
Unit tests for canonical TOML registry parsing, validation, and dependency resolution.
"""

import unittest
from pathlib import Path

from awb.errors import (
    DependencyError,
    DuplicateSkillError,
    RegistryParseError,
    ValidationError,
)
from awb.registry.parser import load_registry, parse_registry_string
from awb.registry.schema import RegistryManifest, SkillDefinition
from awb.registry.validator import RegistryValidator


class TestRegistry(unittest.TestCase):
    """Test suite for awb.registry."""

    def test_canonical_skills_toml_loads_successfully(self):
        """Verify that the project's canonical registry/skills.toml is completely valid."""
        repo_root = Path(__file__).resolve().parent.parent
        reg_file = repo_root / "registry" / "skills.toml"
        self.assertTrue(reg_file.is_file())

        manifest = load_registry(reg_file)
        self.assertIsInstance(manifest, RegistryManifest)
        self.assertEqual(manifest.schema_version, "1.0.0")
        self.assertGreaterEqual(manifest.skill_count(), 4)

        # Check expected skills are present
        self.assertIn("principles", manifest.skills)
        self.assertIn("task-router", manifest.skills)
        self.assertIn("token-efficient-workflow", manifest.skills)
        self.assertIn("verification-guard", manifest.skills)

    def test_rejects_duplicate_skill_id(self):
        """Reject TOML when duplicate skill IDs exist."""
        bad_toml = """
        [registry]
        version = "1.0.0"
        updated_at = "2026-10-23"

        [[skills]]
        id = "my-skill"
        name = "Skill One"
        version = "1.0.0"
        description = "First instance"

        [[skills]]
        id = "my-skill"
        name = "Skill Two"
        version = "1.0.0"
        description = "Second instance duplicate"
        """
        with self.assertRaises(DuplicateSkillError) as ctx:
            parse_registry_string(bad_toml)
        self.assertIn("Duplicate skill ID detected", str(ctx.exception))

    def test_rejects_invalid_semver(self):
        """Reject skill with invalid SemVer version string."""
        bad_toml = """
        [registry]
        version = "1.0.0"
        updated_at = "2026-10-23"

        [[skills]]
        id = "bad-ver"
        name = "Bad Version Skill"
        version = "v1.0"
        description = "Invalid SemVer"
        """
        with self.assertRaises(ValidationError) as ctx:
            parse_registry_string(bad_toml)
        self.assertIn("not valid semantic versioning", str(ctx.exception))

    def test_rejects_invalid_skill_id(self):
        """Reject skill with uppercase or underscore in id."""
        bad_toml = """
        [registry]
        version = "1.0.0"
        updated_at = "2026-10-23"

        [[skills]]
        id = "Bad_Skill_Id"
        name = "Bad ID"
        version = "1.0.0"
        description = "Invalid ID format"
        """
        with self.assertRaises(ValidationError) as ctx:
            parse_registry_string(bad_toml)
        self.assertIn("must be lowercase alphanumeric with hyphens", str(ctx.exception))

    def test_rejects_self_dependency(self):
        """Reject skill that declares dependency on itself."""
        bad_toml = """
        [registry]
        version = "1.0.0"
        updated_at = "2026-10-23"

        [[skills]]
        id = "self-dep"
        name = "Self Dependent"
        version = "1.0.0"
        description = "Self dependency"
        dependencies = ["self-dep"]
        """
        with self.assertRaises(DependencyError) as ctx:
            parse_registry_string(bad_toml)
        self.assertIn("cannot depend on itself", str(ctx.exception))

    def test_rejects_missing_dependency(self):
        """Reject skill declaring dependency on nonexistent skill."""
        bad_toml = """
        [registry]
        version = "1.0.0"
        updated_at = "2026-10-23"

        [[skills]]
        id = "has-missing-dep"
        name = "Missing Dependency"
        version = "1.0.0"
        description = "Missing dep"
        dependencies = ["non-existent-skill"]
        """
        with self.assertRaises(DependencyError) as ctx:
            parse_registry_string(bad_toml)
        self.assertIn("does not exist in registry", str(ctx.exception))

    def test_rejects_circular_dependency(self):
        """Reject direct and indirect circular dependencies."""
        bad_toml = """
        [registry]
        version = "1.0.0"
        updated_at = "2026-10-23"

        [[skills]]
        id = "cycle-a"
        name = "Cycle A"
        version = "1.0.0"
        description = "A depends on B"
        dependencies = ["cycle-b"]

        [[skills]]
        id = "cycle-b"
        name = "Cycle B"
        version = "1.0.0"
        description = "B depends on C"
        dependencies = ["cycle-c"]

        [[skills]]
        id = "cycle-c"
        name = "Cycle C"
        version = "1.0.0"
        description = "C depends on A"
        dependencies = ["cycle-a"]
        """
        with self.assertRaises(DependencyError) as ctx:
            parse_registry_string(bad_toml)
        self.assertIn("Dependency cycle detected", str(ctx.exception))

    def test_topological_install_order_resolution(self):
        """Verify deterministic resolution of dependencies in topological order."""
        valid_toml = """
        [registry]
        version = "1.0.0"
        updated_at = "2026-10-23"

        [[skills]]
        id = "core"
        name = "Core"
        version = "1.0.0"
        description = "Base"
        dependencies = []

        [[skills]]
        id = "router"
        name = "Router"
        version = "1.0.0"
        description = "Router depends on core"
        dependencies = ["core"]

        [[skills]]
        id = "workflow"
        name = "Workflow"
        version = "1.0.0"
        description = "Workflow depends on router and core"
        dependencies = ["core", "router"]
        """
        manifest = parse_registry_string(valid_toml)
        order = RegistryValidator.resolve_install_order(["workflow"], manifest.skills)
        self.assertEqual(order, ["core", "router", "workflow"])


    def test_rejects_empty_or_whitespace_registry(self):
        """Empty or whitespace-only registry content must fail closed."""
        with self.assertRaises(RegistryParseError) as ctx:
            parse_registry_string("")
        self.assertIn("Registry content is empty", str(ctx.exception))

        with self.assertRaises(RegistryParseError) as ctx:
            parse_registry_string("   \n\t   \n")
        self.assertIn("Registry content is empty", str(ctx.exception))

    def test_rejects_missing_registry_section(self):
        """Missing mandatory [registry] section must fail validation."""
        toml_no_reg = """
        [[skills]]
        id = "test"
        name = "Test"
        version = "1.0.0"
        description = "Test"
        """
        with self.assertRaises(ValidationError) as ctx:
            parse_registry_string(toml_no_reg)
        self.assertIn("Missing mandatory [registry] section", str(ctx.exception))

    def test_rejects_missing_skills_array(self):
        """Missing mandatory [[skills]] array must fail validation."""
        toml_no_skills = """
        [registry]
        version = "1.0.0"
        updated_at = "2026-10-23"
        """
        with self.assertRaises(ValidationError) as ctx:
            parse_registry_string(toml_no_skills)
        self.assertIn("Missing mandatory [[skills]] array", str(ctx.exception))

    def test_rejects_missing_registry_version(self):
        """[registry] missing version string must fail validation."""
        toml_no_ver = """
        [registry]
        updated_at = "2026-10-23"

        [[skills]]
        id = "test"
        name = "Test"
        version = "1.0.0"
        description = "Test"
        """
        with self.assertRaises(ValidationError) as ctx:
            parse_registry_string(toml_no_ver)
        self.assertIn("must declare a string 'version'", str(ctx.exception))


    def test_rejects_missing_updated_at(self):
        """[registry] missing updated_at string must fail validation."""
        toml_no_updated = """
        [registry]
        version = "1.0.0"

        [[skills]]
        id = "test"
        name = "Test"
        version = "1.0.0"
        description = "Test"
        """
        with self.assertRaises(ValidationError) as ctx:
            parse_registry_string(toml_no_updated)
        self.assertIn("must declare a non-empty string 'updated_at'", str(ctx.exception))

    def test_rejects_empty_or_whitespace_updated_at(self):
        """[registry] with empty or whitespace-only updated_at must fail validation."""
        toml_empty = """
        [registry]
        version = "1.0.0"
        updated_at = ""

        [[skills]]
        id = "test"
        name = "Test"
        version = "1.0.0"
        description = "Test"
        """
        with self.assertRaises(ValidationError) as ctx:
            parse_registry_string(toml_empty)
        self.assertIn("must declare a non-empty string 'updated_at'", str(ctx.exception))

        toml_ws = """
        [registry]
        version = "1.0.0"
        updated_at = "   \t  "

        [[skills]]
        id = "test"
        name = "Test"
        version = "1.0.0"
        description = "Test"
        """
        with self.assertRaises(ValidationError) as ctx:
            parse_registry_string(toml_ws)
        self.assertIn("must declare a non-empty string 'updated_at'", str(ctx.exception))


if __name__ == "__main__":
    unittest.main()
