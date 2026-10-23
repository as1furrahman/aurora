"""
Base adapter interface and conformance model for agent skills.
Zero third-party dependencies.
"""

from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from pathlib import Path
from typing import List, Optional


@dataclass(frozen=True)
class ConformanceReport:
    """Outcome of non-mutating adapter conformance validation."""

    adapter_name: str
    target_path: Path
    passed: bool
    checks: List[str] = field(default_factory=list)
    errors: List[str] = field(default_factory=list)


class BaseAdapter(ABC):
    """Abstract base class for all Agent Workbench adapters."""

    @property
    @abstractmethod
    def name(self) -> str:
        """Unique internal adapter identifier (e.g. 'antigravity-cli')."""
        pass

    @property
    @abstractmethod
    def display_name(self) -> str:
        """Human-readable adapter name."""
        pass

    @property
    @abstractmethod
    def target_type(self) -> str:
        """Type of adapter: 'cli', 'workspace', or 'ide'."""
        pass

    @property
    @abstractmethod
    def discovery_path(self) -> Path:
        """Canonical discovery path for skills managed by this agent."""
        pass

    @abstractmethod
    def is_detected(self) -> bool:
        """Non-mutating probe determining whether the agent/CLI environment is present."""
        pass

    @abstractmethod
    def list_installed_skills(self) -> List[str]:
        """Non-mutating read-only discovery of installed skill IDs."""
        pass

    @abstractmethod
    def validate_conformance(self, sandbox_root: Path) -> ConformanceReport:
        """
        Validate adapter structure, discovery invariants, and safety boundaries
        within an isolated non-mutating sandbox directory.
        """
        pass
