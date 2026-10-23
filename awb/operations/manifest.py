"""
Skill installation manifest (.awb-manifest.json) model, serialization,
and hash-based integrity and content-drift verification.

NOTE ON SECURITY INVARIANT (SEC-06):
The manifest records SHA-256 digests and file metadata for local self-consistency
and accidental drift detection. It is NOT cryptographically signed or tamper-proof
against malicious local processes with write access to the directory.
Zero third-party dependencies.
"""

import hashlib
import json
import os
from dataclasses import asdict, dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

from awb.errors import OwnershipError

MANIFEST_FILENAME = ".awb-manifest.json"
MANIFEST_VERSION = "1.0.0"


@dataclass
class SkillManifest:
    """Metadata recorded inside an AWB-managed skill directory."""

    manifest_version: str
    awb_managed: bool
    skill_id: str
    version: str
    installed_at: str
    source_registry: str
    files: Dict[str, Dict[str, Any]]  # rel_path -> {sha256, size, mode}

    def to_dict(self) -> Dict[str, Any]:
        return asdict(self)

    def to_json(self) -> str:
        return json.dumps(self.to_dict(), indent=2)

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "SkillManifest":
        if not isinstance(data, dict):
            raise OwnershipError("Manifest content is not a valid JSON object")
        if not data.get("awb_managed", False):
            raise OwnershipError("Directory manifest does not have awb_managed=true")
        if "skill_id" not in data or not data["skill_id"]:
            raise OwnershipError("Manifest missing valid skill_id")

        return cls(
            manifest_version=str(data.get("manifest_version", MANIFEST_VERSION)),
            awb_managed=bool(data.get("awb_managed", True)),
            skill_id=str(data["skill_id"]),
            version=str(data.get("version", "1.0.0")),
            installed_at=str(data.get("installed_at", "")),
            source_registry=str(data.get("source_registry", "")),
            files=dict(data.get("files", {})),
        )

    @classmethod
    def from_json(cls, json_str: str) -> "SkillManifest":
        try:
            data = json.loads(json_str)
        except json.JSONDecodeError as e:
            raise OwnershipError(f"Corrupted or invalid JSON manifest: {e}") from e
        return cls.from_dict(data)


@dataclass
class IntegrityReport:
    """Outcome of skill directory integrity and ownership verification."""

    valid: bool
    missing_files: List[str] = field(default_factory=list)
    modified_files: List[str] = field(default_factory=list)
    extraneous_files: List[str] = field(default_factory=list)
    symlinks_detected: List[str] = field(default_factory=list)
    errors: List[str] = field(default_factory=list)


def verify_skill_integrity(
    skill_dir: Path,
    expected_skill_id: Optional[str] = None,
) -> Tuple[bool, Optional[SkillManifest], IntegrityReport]:
    """
    Verify that a skill directory is demonstrably owned by AWB and has not been modified.
    Rejects directories with missing manifests, content drift, symlinks, or extraneous files.
    """
    path = skill_dir.resolve()
    report = IntegrityReport(valid=True)

    if not path.is_dir():
        report.valid = False
        report.errors.append(f"Target path '{skill_dir}' is not a directory")
        return False, None, report

    manifest_file = path / MANIFEST_FILENAME
    if not manifest_file.is_file():
        report.valid = False
        report.errors.append(
            f"Missing '{MANIFEST_FILENAME}' in '{skill_dir}'. Directory is not owned or managed by AWB."
        )
        return False, None, report

    if manifest_file.is_symlink():
        report.valid = False
        report.symlinks_detected.append(MANIFEST_FILENAME)
        report.errors.append(f"Manifest '{MANIFEST_FILENAME}' is a symlink; rejected for safety.")
        return False, None, report

    try:
        manifest_text = manifest_file.read_text(encoding="utf-8")
        manifest = SkillManifest.from_json(manifest_text)
    except Exception as e:
        report.valid = False
        report.errors.append(f"Failed to read/parse manifest: {e}")
        return False, None, report

    if expected_skill_id and manifest.skill_id != expected_skill_id:
        report.valid = False
        report.errors.append(
            f"Skill ID mismatch: directory manifest reports '{manifest.skill_id}', expected '{expected_skill_id}'"
        )
        return False, manifest, report

    # Collect actual files on disk
    disk_files: Dict[str, Path] = {}
    for root, _, filenames in os.walk(path):
        for fname in filenames:
            full_p = Path(root) / fname
            rel_p = str(full_p.relative_to(path))
            disk_files[rel_p] = full_p

    # Check for symlinks in skill directory
    for rel_p, full_p in disk_files.items():
        if full_p.is_symlink():
            report.symlinks_detected.append(rel_p)

    if report.symlinks_detected:
        report.valid = False
        report.errors.append(f"Symlinks detected in skill directory: {report.symlinks_detected}")

    # Check expected files against manifest
    for rel_p, meta in manifest.files.items():
        if rel_p not in disk_files:
            report.missing_files.append(rel_p)
            continue

        full_p = disk_files[rel_p]
        try:
            with open(full_p, "rb") as f:
                content = f.read()
            h = hashlib.sha256(content).hexdigest()
            expected_h = meta.get("sha256")
            if expected_h and h != expected_h:
                report.modified_files.append(rel_p)
        except OSError as e:
            report.errors.append(f"Failed to inspect file '{rel_p}': {e}")

    # Check for extraneous files not declared in manifest
    ignored_special = {MANIFEST_FILENAME, ".awb.lock", ".awb.journal.json"}
    for rel_p in disk_files:
        if rel_p not in ignored_special and rel_p not in manifest.files:
            report.extraneous_files.append(rel_p)

    if report.missing_files:
        report.valid = False
        report.errors.append(f"Missing expected files: {report.missing_files}")

    if report.modified_files:
        report.valid = False
        report.errors.append(f"Modified files (hash drift): {report.modified_files}")

    if report.extraneous_files:
        report.valid = False
        report.errors.append(
            f"Extraneous unmanaged files detected in skill directory: {report.extraneous_files}"
        )

    return report.valid, manifest, report
