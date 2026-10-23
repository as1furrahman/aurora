"""
Deterministic registry validator, schema enforcer, and dependency graph resolver.
"""

import re
from typing import Dict, List, Set

from awb.errors import DependencyError, DuplicateSkillError, ValidationError
from .schema import RegistryManifest, SkillDefinition

SEMVER_PATTERN = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?$")
SKILL_ID_PATTERN = re.compile(r"^[a-z0-9]+(-[a-z0-9]+)*$")
ALLOWED_AGENTS = {"antigravity-cli", "antigravity-workspace", "antigravity-ide", "codex-cli", "claude-code", "universal"}


class RegistryValidator:
    """Enforces strict schema rules, non-duplication, and dependency acyclicity."""

    @classmethod
    def validate_skill(cls, skill: SkillDefinition) -> None:
        """Validate individual skill definition attributes."""
        if not skill.id or not isinstance(skill.id, str):
            raise ValidationError("Skill 'id' must be a non-empty string")

        if not SKILL_ID_PATTERN.match(skill.id):
            raise ValidationError(
                f"Skill id '{skill.id}' must be lowercase alphanumeric with hyphens (e.g. 'token-efficient-workflow')"
            )

        if not skill.name or not isinstance(skill.name, str):
            raise ValidationError(f"Skill '{skill.id}' must have a non-empty 'name' string")

        if not skill.version or not isinstance(skill.version, str):
            raise ValidationError(f"Skill '{skill.id}' must have a 'version' string")

        if not SEMVER_PATTERN.match(skill.version):
            raise ValidationError(
                f"Skill '{skill.id}' version '{skill.version}' is not valid semantic versioning (SemVer)"
            )

        if not skill.description or not isinstance(skill.description, str):
            raise ValidationError(f"Skill '{skill.id}' must have a non-empty 'description' string")

        if not isinstance(skill.dependencies, list) or not all(isinstance(d, str) for d in skill.dependencies):
            raise ValidationError(f"Skill '{skill.id}' dependencies must be a list of string skill IDs")

        if skill.id in skill.dependencies:
            raise DependencyError(f"Skill '{skill.id}' cannot depend on itself")

        if not isinstance(skill.compatible_agents, list) or not all(isinstance(a, str) for a in skill.compatible_agents):
            raise ValidationError(f"Skill '{skill.id}' compatible_agents must be a list of strings")

        for agent in skill.compatible_agents:
            if agent not in ALLOWED_AGENTS:
                raise ValidationError(
                    f"Skill '{skill.id}' declares unrecognized agent '{agent}'. Allowed: {sorted(ALLOWED_AGENTS)}"
                )

    @classmethod
    def validate_manifest(cls, manifest: RegistryManifest, raw_skill_ids: List[str]) -> None:
        """
        Validate complete registry manifest:
        - Check for duplicate IDs in raw source
        - Validate each skill definition
        - Check for missing dependencies
        - Detect dependency cycles
        """
        # 1. Duplicate ID check
        seen_ids: Set[str] = set()
        for s_id in raw_skill_ids:
            if s_id in seen_ids:
                raise DuplicateSkillError(f"Duplicate skill ID detected in registry: '{s_id}'")
            seen_ids.add(s_id)

        # 2. Individual skill validation
        for skill in manifest.skills.values():
            cls.validate_skill(skill)

        # 3. Missing dependency check
        for skill in manifest.skills.values():
            for dep_id in skill.dependencies:
                if dep_id not in manifest.skills:
                    raise DependencyError(
                        f"Skill '{skill.id}' declares dependency '{dep_id}' which does not exist in registry"
                    )

        # 4. Dependency cycle detection using DFS
        cls.detect_cycles(manifest.skills)

    @classmethod
    def detect_cycles(cls, skills: Dict[str, SkillDefinition]) -> None:
        """Detect any directed dependency cycle using 3-color DFS graph traversal."""
        # 0 = unvisited, 1 = visiting (in recursion stack), 2 = visited
        visited: Dict[str, int] = {k: 0 for k in skills}
        path: List[str] = []

        def dfs(node: str) -> None:
            visited[node] = 1
            path.append(node)

            for dep in skills[node].dependencies:
                if dep not in skills:
                    continue
                if visited[dep] == 1:
                    cycle = " -> ".join(path[path.index(dep):] + [dep])
                    raise DependencyError(f"Dependency cycle detected in registry: {cycle}")
                if visited[dep] == 0:
                    dfs(dep)

            path.pop()
            visited[node] = 2

        for skill_id in skills:
            if visited[skill_id] == 0:
                dfs(skill_id)

    @classmethod
    def resolve_install_order(
        cls, skill_ids: List[str], skills: Dict[str, SkillDefinition]
    ) -> List[str]:
        """
        Compute deterministic topological installation order for requested skills,
        including all transitive dependencies.
        """
        order: List[str] = []
        visited: Set[str] = set()
        visiting: Set[str] = set()

        def visit(s_id: str) -> None:
            if s_id in visiting:
                raise DependencyError(f"Dependency cycle detected during resolution: '{s_id}'")
            if s_id not in visited:
                if s_id not in skills:
                    raise DependencyError(f"Unresolved dependency '{s_id}'")
                visiting.add(s_id)
                for dep in sorted(skills[s_id].dependencies):
                    visit(dep)
                visiting.remove(s_id)
                visited.add(s_id)
                order.append(s_id)

        for target in sorted(skill_ids):
            visit(target)

        return order
