"""
Deterministic TOML registry parser using Python standard library tomllib.
"""

import sys
import tomllib
from pathlib import Path
from typing import Any, Dict, List, Union

from awb.errors import RegistryParseError, ValidationError
from .schema import RegistryManifest, SkillDefinition
from .validator import RegistryValidator, SEMVER_PATTERN


def parse_registry_string(content: str) -> RegistryManifest:
    """Parse a TOML registry string into a validated RegistryManifest."""
    if not content or not content.strip():
        raise RegistryParseError("Registry content is empty; cannot parse empty registry")

    try:
        data = tomllib.loads(content)
    except tomllib.TOMLDecodeError as e:
        raise RegistryParseError(f"Failed to parse registry TOML: {e}") from e

    if not isinstance(data, dict):
        raise ValidationError("Registry root must be a TOML table")

    # [registry] is strictly mandatory
    if "registry" not in data:
        raise ValidationError("Missing mandatory [registry] section")

    reg_meta = data["registry"]
    if not isinstance(reg_meta, dict):
        raise ValidationError("[registry] section must be a table")

    if "version" not in reg_meta or not isinstance(reg_meta["version"], str):
        raise ValidationError("[registry] section must declare a string 'version'")

    schema_version = reg_meta["version"]
    if not SEMVER_PATTERN.match(schema_version):
        raise ValidationError(f"[registry] version '{schema_version}' is not valid semantic versioning (SemVer)")

    if "updated_at" not in reg_meta or not isinstance(reg_meta["updated_at"], str) or not reg_meta["updated_at"].strip():
        raise ValidationError("[registry] section must declare a non-empty string 'updated_at'")

    updated_at = reg_meta["updated_at"].strip()

    # [[skills]] is strictly mandatory
    if "skills" not in data:
        raise ValidationError("Missing mandatory [[skills]] array in registry")

    raw_skills = data["skills"]
    if not isinstance(raw_skills, list):
        raise ValidationError("[[skills]] must be a list of skill tables")

    raw_ids: List[str] = []
    skills_map: Dict[str, SkillDefinition] = {}

    for idx, item in enumerate(raw_skills):
        if not isinstance(item, dict):
            raise ValidationError(f"Skill entry #{idx} is not a valid table")

        skill_id = item.get("id", "")
        raw_ids.append(skill_id)

        skill = SkillDefinition(
            id=skill_id,
            name=item.get("name", ""),
            version=item.get("version", ""),
            description=item.get("description", ""),
            dependencies=item.get("dependencies", []),
            compatible_agents=item.get("compatible_agents", []),
            triggers=item.get("triggers", []),
            files=item.get("files", []),
            category=item.get("category"),
        )
        skills_map[skill_id] = skill

    manifest = RegistryManifest(
        schema_version=schema_version,
        updated_at=updated_at,
        skills=skills_map,
    )

    # Validate schema, duplicate IDs, and dependency acyclicity
    RegistryValidator.validate_manifest(manifest, raw_ids)

    return manifest


def load_registry(path: Union[str, Path]) -> RegistryManifest:
    """Load and parse registry file from disk."""
    p = Path(path)
    if not p.is_file():
        raise RegistryParseError(f"Registry file does not exist: '{p}'")

    try:
        content = p.read_text(encoding="utf-8")
    except OSError as e:
        raise RegistryParseError(f"Cannot read registry file '{p}': {e}") from e

    return parse_registry_string(content)
