"""
Execution-provider contract. Separate from skill-storage adapters (awb.adapters):
providers know how a CLI is invoked, never where it discovers skills.
"""

import shutil
from dataclasses import dataclass
from pathlib import Path
from typing import Optional, Tuple

from awb.errors import AWBError

MANUAL_HANDOFF = "manual-handoff"


class ProviderNotEnabledError(AWBError):
    """Raised when direct provider execution is requested but not enabled (ADR-0005)."""
    pass


@dataclass(frozen=True)
class ProviderSpec:
    """Immutable description of a coding CLI as an execution tool."""

    id: str
    display_name: str
    binary: str
    skill_adapters: Tuple[str, ...]
    run_hint: str
    documented_headless: str
    forbidden_flags: Tuple[str, ...]
    docs_urls: Tuple[str, ...]
    verified_on: str
    invocation_modes: Tuple[str, ...] = (MANUAL_HANDOFF,)
    headless_enabled: bool = False


@dataclass(frozen=True)
class ProviderDetection:
    """Result of a read-only PATH probe. Version is never probed (no process is run)."""

    provider_id: str
    installed: bool
    path: Optional[str]
    version: Optional[str] = None


class ExecutionProvider:
    """Execution contract: detect, render handoff, and (not yet) execute."""

    def __init__(self, spec: ProviderSpec):
        self.spec = spec

    @property
    def id(self) -> str:
        return self.spec.id

    def detect(self) -> ProviderDetection:
        """Read-only PATH lookup; never runs the binary."""
        path = shutil.which(self.spec.binary)
        return ProviderDetection(provider_id=self.spec.id, installed=path is not None, path=path)

    def build_handoff(self, prompt: str) -> str:
        """Append provider-specific manual run instructions to a role prompt. Pure."""
        return (
            f"{prompt.rstrip()}\n\n"
            f"## How to run this stage with {self.spec.display_name}\n\n"
            f"{self.spec.run_hint}\n\n"
            "Use the CLI's normal permission prompts. Do not use flags that bypass approvals or sandboxing.\n"
        )

    def execute(self, prompt: str, cwd: Path) -> None:
        """Direct execution is out of scope for Milestone 1."""
        raise ProviderNotEnabledError(
            f"Direct execution via '{self.spec.id}' is not enabled; supported modes: "
            f"{list(self.spec.invocation_modes)}. Use the manual handoff bundle."
        )
