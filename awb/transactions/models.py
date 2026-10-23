"""
Transaction models and mutation step definitions.
Zero third-party runtime dependencies.
"""

from dataclasses import dataclass, field
from enum import Enum
from pathlib import Path
from typing import Any, Dict, List, Optional, Union


class MutationAction(Enum):
    """Supported atomic mutation actions."""
    WRITE_FILE = "write_file"
    UNLINK_FILE = "unlink_file"
    CREATE_DIR = "create_dir"
    REMOVE_DIR = "remove_dir"


@dataclass(frozen=True)
class MutationStep:
    """Individual atomic filesystem mutation step."""
    action: MutationAction
    target_path: Path
    content: Optional[Union[bytes, str]] = None
    expected_dev: Optional[int] = None
    expected_ino: Optional[int] = None
    expected_sha: Optional[str] = None
    mode: int = 0o600


@dataclass(frozen=True)
class MutationPlan:
    """Pre-computed deterministic plan encompassing an atomic set of mutation steps."""
    plan_id: str
    target_dir: Path
    steps: List[MutationStep] = field(default_factory=list)


@dataclass(frozen=True)
class TransactionResult:
    """Outcome of transactional mutation execution."""
    success: bool
    plan_id: str
    applied_count: int = 0
    rolled_back: bool = False
    error: Optional[str] = None
    journal_path: Optional[Path] = None
