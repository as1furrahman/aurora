"""
Data schemas for canonical skill registry.
"""

from dataclasses import dataclass, field
from typing import Dict, List, Optional


@dataclass(frozen=True)
class SkillDefinition:
    """Canonical representation of a skill entry in registry/skills.toml."""

    id: str
    name: str
    version: str
    description: str
    dependencies: List[str] = field(default_factory=list)
    compatible_agents: List[str] = field(default_factory=list)
    triggers: List[str] = field(default_factory=list)
    files: List[str] = field(default_factory=list)
    category: Optional[str] = None


@dataclass(frozen=True)
class RegistryManifest:
    """Canonical registry manifest encompassing all registered skills."""

    schema_version: str
    updated_at: str
    skills: Dict[str, SkillDefinition] = field(default_factory=dict)

    def get_skill(self, skill_id: str) -> Optional[SkillDefinition]:
        return self.skills.get(skill_id)

    def skill_count(self) -> int:
        return len(self.skills)
