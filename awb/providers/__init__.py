"""
awb.providers - Execution-provider contract for coding CLIs (ADR-0002, ADR-0005).
Milestone 1 supports manual handoff only; no provider process is ever spawned.
"""

from .base import ExecutionProvider, ProviderDetection, ProviderNotEnabledError, ProviderSpec
from .registry import PROVIDERS, get_provider

__all__ = [
    "ExecutionProvider",
    "ProviderDetection",
    "ProviderNotEnabledError",
    "ProviderSpec",
    "PROVIDERS",
    "get_provider",
]
