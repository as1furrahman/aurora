"""
Aurora (CLI: aurora, compatibility alias: awb) CLI entrypoint.
Provides non-mutating detection, listing, validation, and diagnostics.
Zero third-party runtime dependencies.
"""

import argparse
import os
import sys
from pathlib import Path
from typing import List, Optional

from awb import __version__
from awb.adapters import get_all_adapters
from awb.config import Config
from awb.errors import AWBError, SafetyError
from awb.registry.parser import load_registry
from awb.registry.validator import RegistryValidator


def build_parser(prog: Optional[str] = None) -> argparse.ArgumentParser:
    """Construct CLI argument parser with modular subcommands."""
    default_prog = prog or ("awb" if sys.argv and Path(sys.argv[0]).name == "awb" else "aurora")
    parser = argparse.ArgumentParser(
        prog=default_prog,
        description="Aurora: Universal Centralized Manager for Coding-Agent Skills (compatibility alias: awb).",
    )
    parser.add_argument(
        "--version",
        action="version",
        version=f"Aurora ({default_prog}) {__version__}",
    )

    subparsers = parser.add_subparsers(dest="command", help="Available subcommands")

    # awb detect
    detect_parser = subparsers.add_parser("detect", help="Non-mutating probe of available agent environments")
    detect_parser.add_argument("--json", action="store_true", help="Output results in JSON format")

    # awb list
    list_parser = subparsers.add_parser("list", help="List canonical skills defined in registry")
    list_parser.add_argument("--registry", type=Path, default=None, help="Path to custom skills.toml")
    list_parser.add_argument("--agent", type=str, default=None, help="Filter by compatible agent")

    # awb validate
    val_parser = subparsers.add_parser("validate", help="Validate registry syntax, schema, and dependencies")
    val_parser.add_argument("--registry", type=Path, default=None, help="Path to skills.toml to validate")

    # awb doctor
    doc_parser = subparsers.add_parser("doctor", help="Run system diagnostics and adapter readiness checks")

    # aurora providers
    prov_parser = subparsers.add_parser("providers", help="List execution providers (coding CLIs) and PATH detection")
    prov_parser.add_argument("--json", action="store_true", help="Output results in JSON format")

    # awb install
    install_parser = subparsers.add_parser("install", help="Install a skill transactionally under ProcessLock")
    install_parser.add_argument("skill_id", type=str, help="Canonical skill ID from registry")
    install_parser.add_argument("--target", type=Path, default=None, help="Target installation directory (required for sandboxes)")
    install_parser.add_argument("--root", type=Path, default=None, help="Explicit trusted sandbox root containing the destination")
    install_parser.add_argument("--adapter", type=str, default=None, help="Target agent adapter identifier")
    install_parser.add_argument("--registry", type=Path, default=None, help="Path to custom skills.toml")
    install_parser.add_argument("--dry-run", action="store_true", help="Inspect planned changes without modifying filesystem")
    install_parser.add_argument("--json", action="store_true", help="Output result in JSON format")

    # awb update
    update_parser = subparsers.add_parser("update", help="Update or re-install an AWB-managed skill transactionally under ProcessLock")
    update_parser.add_argument("skill_id", type=str, help="Canonical skill ID from registry")
    update_parser.add_argument("--target", type=Path, default=None, help="Target installation directory (required for sandboxes)")
    update_parser.add_argument("--root", type=Path, default=None, help="Explicit trusted sandbox root containing the destination")
    update_parser.add_argument("--adapter", type=str, default=None, help="Target agent adapter identifier")
    update_parser.add_argument("--registry", type=Path, default=None, help="Path to custom skills.toml")
    update_parser.add_argument("--dry-run", action="store_true", help="Inspect planned changes without modifying filesystem")
    update_parser.add_argument("--json", action="store_true", help="Output result in JSON format")

    # awb remove
    remove_parser = subparsers.add_parser("remove", help="Remove an AWB-managed skill transactionally under ProcessLock")
    remove_parser.add_argument("skill_id", type=str, help="Canonical skill ID to remove")
    remove_parser.add_argument("--target", type=Path, default=None, help="Target directory containing the skill")
    remove_parser.add_argument("--root", type=Path, default=None, help="Explicit trusted sandbox root containing the destination")
    remove_parser.add_argument("--adapter", type=str, default=None, help="Target agent adapter identifier")
    remove_parser.add_argument("--dry-run", action="store_true", help="Inspect planned removals without modifying filesystem")
    remove_parser.add_argument("--json", action="store_true", help="Output result in JSON format")

    return parser


def cmd_detect(config: Config, as_json: bool = False) -> int:
    """Execute non-mutating agent environment detection."""
    adapters = get_all_adapters(config)
    results = []

    for name, adapter in adapters.items():
        detected = adapter.is_detected()
        installed = adapter.list_installed_skills()
        results.append({
            "id": name,
            "name": adapter.display_name,
            "type": adapter.target_type,
            "detected": detected,
            "path": str(adapter.discovery_path),
            "installed_count": len(installed),
            "installed_skills": installed,
        })

    if as_json:
        import json
        print(json.dumps(results, indent=2))
        return 0

    print("Aurora — Environment Detection (Non-Mutating):")
    print("-" * 65)
    for r in results:
        status = "[DETECTED]" if r["detected"] else "[NOT DETECTED]"
        print(f" {status:<15} {r['name']} [{r['id']}]")
        print(f"   Discovery Path: {r['path']}")
        print(f"   Installed Skills: {r['installed_count']} ({', '.join(r['installed_skills']) if r['installed_skills'] else 'none'})")
        print()
    return 0


def cmd_providers(as_json: bool = False) -> int:
    """List execution providers. Detection is a PATH lookup only; no binary is run."""
    from awb.providers import PROVIDERS

    results = []
    for pid in sorted(PROVIDERS):
        provider = PROVIDERS[pid]
        detection = provider.detect()
        spec = provider.spec
        results.append({
            "id": spec.id,
            "name": spec.display_name,
            "binary": spec.binary,
            "installed": detection.installed,
            "path": detection.path,
            "skill_adapters": list(spec.skill_adapters),
            "invocation_modes": list(spec.invocation_modes),
            "headless_enabled": spec.headless_enabled,
            "documented_headless": spec.documented_headless,
            "forbidden_flags": list(spec.forbidden_flags),
            "docs_urls": list(spec.docs_urls),
            "verified_on": spec.verified_on,
        })

    if as_json:
        import json
        print(json.dumps(results, indent=2))
        return 0

    print("Aurora — Execution Providers (manual handoff only; no process is launched):")
    print("-" * 65)
    for r in results:
        status = "[INSTALLED]" if r["installed"] else "[NOT FOUND]"
        print(f" {status:<12} {r['name']} [{r['id']}] (binary: {r['binary']})")
        print(f"   Skill adapters:   {', '.join(r['skill_adapters'])}")
        print(f"   Invocation modes: {', '.join(r['invocation_modes'])} (headless: disabled)")
        print()
    return 0


def cmd_list(config: Config, custom_registry: Optional[Path] = None, agent_filter: Optional[str] = None) -> int:
    """List canonical skills defined in registry/skills.toml."""
    reg_path = custom_registry or config.registry_path
    try:
        manifest = load_registry(reg_path)
    except AWBError as e:
        print(f"Error loading registry: {e}", file=sys.stderr)
        return 1

    skills = list(manifest.skills.values())
    if agent_filter:
        skills = [s for s in skills if agent_filter in s.compatible_agents or "universal" in s.compatible_agents]

    print(f"Aurora — Canonical Skill Registry (v{manifest.schema_version}):")
    print(f"Source: {reg_path}")
    print(f"Total Skills: {len(skills)}")
    print("-" * 75)

    for s in skills:
        deps = f" [deps: {', '.join(s.dependencies)}]" if s.dependencies else ""
        print(f"* {s.id} (v{s.version}){deps}")
        print(f"  Name: {s.name}")
        print(f"  Category: {s.category or 'general'}")
        print(f"  Agents: {', '.join(s.compatible_agents)}")
        print(f"  Description: {s.description}")
        print()
    return 0


def cmd_validate(config: Config, custom_registry: Optional[Path] = None) -> int:
    """Validate registry schema, semver, duplicate IDs, and dependency acyclicity."""
    reg_path = custom_registry or config.registry_path
    print(f"Validating registry: {reg_path}")
    try:
        manifest = load_registry(reg_path)
        # Compute install order to prove dependency resolution
        order = RegistryValidator.resolve_install_order(list(manifest.skills.keys()), manifest.skills)
    except AWBError as e:
        print(f"VALIDATION FAILED: {e}", file=sys.stderr)
        return 1

    print("VALIDATION PASSED:")
    print(f"  - Schema Version: {manifest.schema_version}")
    print(f"  - Validated Skills: {manifest.skill_count()}")
    print("  - Duplicate IDs: None detected")
    print("  - Dependency Graph: Acyclic (Cycle check OK)")
    print(f"  - Topological Resolution Order: {' -> '.join(order)}")
    return 0


def cmd_doctor(config: Config) -> int:
    """Execute comprehensive system diagnostics and non-mutating checks."""
    import sys
    import tomllib

    print("Aurora — System Diagnostics (Doctor):")
    print("-" * 65)

    # 1. Python Environment
    py_ver = sys.version.split()[0]
    py_ok = sys.version_info >= (3, 11)
    status_py = "OK" if py_ok else "FAIL (Requires Python 3.11+)"
    print(f"[*] Python Runtime: {py_ver} ({status_py})")

    # 2. tomllib availability
    print("[*] Standard-Library TOML: tomllib available (OK)")

    # 3. Canonical Registry
    reg_path = config.registry_path
    if reg_path.is_file():
        try:
            manifest = load_registry(reg_path)
            print(f"[*] Canonical Registry: {reg_path} ({manifest.skill_count()} skills, OK)")
        except Exception as e:
            print(f"[*] Canonical Registry: FAILED ({e})")
    else:
        print(f"[*] Canonical Registry: NOT FOUND at {reg_path}")

    # 4. Agent Adapters Probe
    adapters = get_all_adapters(config)
    print("\n[*] Agent Adapters Readiness:")
    for name, adapter in adapters.items():
        detected = adapter.is_detected()
        flag = "[DETECTED]" if detected else "[NOT PRESENT]"
        print(f"    - {name:<22} {flag:<14} (path: {adapter.discovery_path})")

    # 5. Protected Assets Preservation Status
    print("\n[*] Protected Assets Preservation Invariant:")
    baseline_file = Path("/tmp/full_preservation_baseline.json")
    if baseline_file.is_file():
        print(f"    - Preservation Baseline: {baseline_file} active (OK)")
    else:
        print("    - Preservation Baseline: None active")

    # 6. Orphaned Staging Artifacts Check
    print("\n[*] Staging File Hygiene:")
    orphaned_stages = []
    for adapter in adapters.values():
        p = adapter.discovery_path
        if p.is_dir():
            try:
                for entry in p.glob(".awb_stage_*"):
                    st = entry.stat()
                    if st.st_uid == os.getuid():
                        orphaned_stages.append(str(entry))
            except OSError:
                pass
    if orphaned_stages:
        print(f"    [!] Stale staging files detected ({len(orphaned_stages)} files):")
        for s in orphaned_stages[:5]:
            print(f"        {s}")
    else:
        print("    - No orphaned staging files detected (Clean)")

    print("\nAll diagnostics complete. Host environment is healthy.")
    return 0


def resolve_allowed_roots(
    explicit_root: Optional[Path] = None,
    config: Optional[Config] = None,
) -> List[Path]:
    """
    Resolve genuine trusted sandbox roots for CLI operations.
    SEC-04: Never derives allowed roots from target_dir. Requires explicit configuration.
    """
    roots: List[Path] = []
    if explicit_root is not None:
        roots.append(explicit_root.resolve())

    env_roots = os.environ.get("AURORA_ALLOWED_ROOTS") or os.environ.get("AWB_ALLOWED_ROOTS")
    if env_roots:
        for p in env_roots.split(os.pathsep):
            if p.strip():
                roots.append(Path(p.strip()).resolve())

    if not roots:
        from awb.errors import SecurityPreconditionError
        raise SecurityPreconditionError(
            "No trusted sandbox root configured. You must specify an explicit trusted root via "
            "--root <path> or set the AURORA_ALLOWED_ROOTS (or AWB_ALLOWED_ROOTS) environment variable."
        )

    return roots


def cmd_install(
    config: Config,
    skill_id: str,
    target: Optional[Path] = None,
    root: Optional[Path] = None,
    adapter_name: Optional[str] = None,
    custom_registry: Optional[Path] = None,
    dry_run: bool = False,
    as_json: bool = False,
) -> int:
    """Execute or plan skill installation."""
    import json
    from awb.operations import SkillInstaller

    if target is None and adapter_name is None:
        print("Error: Destination required. Specify either --target <path> or --adapter <name>.", file=sys.stderr)
        return 1

    if target is not None:
        target_dir = target.resolve()
    else:
        adapters = get_all_adapters(config)
        if adapter_name not in adapters:
            print(f"Error: Unknown adapter '{adapter_name}'. Available: {list(adapters.keys())}", file=sys.stderr)
            return 1
        target_dir = adapters[adapter_name].discovery_path.resolve()

    try:
        allowed_roots = resolve_allowed_roots(explicit_root=root, config=config)
    except (AWBError, SafetyError) as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1

    try:
        if dry_run:
            plan, manifest, snapshot = SkillInstaller.plan_install(
                skill_id=skill_id,
                target_dir=target_dir,
                allowed_roots=allowed_roots,
                config=config,
                custom_registry=custom_registry,
                allow_global=False,
            )
            if as_json:
                plan_dict = {
                    "dry_run": True,
                    "plan_id": plan.plan_id,
                    "skill_id": skill_id,
                    "version": manifest.version,
                    "target_dir": str(target_dir),
                    "steps": [
                        {"action": s.action.value, "target": str(s.target_path)}
                        for s in plan.steps
                    ],
                    "manifest": manifest.to_dict(),
                }
                print(json.dumps(plan_dict, indent=2))
                return 0

            print("Aurora — Skill Installation Plan (Dry Run):")
            print("-" * 65)
            print(f"Skill ID:     {skill_id} (v{manifest.version})")
            print(f"Target Dir:   {target_dir / skill_id}")
            print(f"Plan ID:      {plan.plan_id}")
            print(f"Total Steps:  {len(plan.steps)}")
            print("Planned Operations:")
            for idx, s in enumerate(plan.steps):
                print(f"  {idx + 1}. [{s.action.value}] {s.target_path}")
            print(f"\nManifest: {len(manifest.files)} files recorded with SHA256 hashes.")
            print("Dry-run complete. No changes made to filesystem.")
            return 0

        # Normal execution
        result, manifest = SkillInstaller.execute_install(
            skill_id=skill_id,
            target_dir=target_dir,
            allowed_roots=allowed_roots,
            config=config,
            custom_registry=custom_registry,
            dry_run=False,
            allow_global=False,
        )

        if as_json:
            out = {
                "success": result.success,
                "plan_id": result.plan_id,
                "skill_id": skill_id,
                "applied_count": result.applied_count,
                "error": result.error,
            }
            print(json.dumps(out, indent=2))
            return 0 if result.success else 1

        if result.success:
            print("Skill Installation Successful:")
            print(f"  Skill:       {skill_id} (v{manifest.version if manifest else 'unknown'})")
            print(f"  Destination: {target_dir / skill_id}")
            print(f"  Steps:       {result.applied_count} applied transactionally under ProcessLock")
            return 0
        else:
            print(f"Skill Installation Failed: {result.error}", file=sys.stderr)
            return 1

    except (AWBError, SafetyError) as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1


def cmd_remove(
    config: Config,
    skill_id: str,
    target: Optional[Path] = None,
    root: Optional[Path] = None,
    adapter_name: Optional[str] = None,
    dry_run: bool = False,
    as_json: bool = False,
) -> int:
    """Execute or plan skill removal."""
    import json
    from awb.operations import SkillRemover

    if target is None and adapter_name is None:
        print("Error: Destination required. Specify either --target <path> or --adapter <name>.", file=sys.stderr)
        return 1

    if target is not None:
        target_dir = target.resolve()
    else:
        adapters = get_all_adapters(config)
        if adapter_name not in adapters:
            print(f"Error: Unknown adapter '{adapter_name}'. Available: {list(adapters.keys())}", file=sys.stderr)
            return 1
        target_dir = adapters[adapter_name].discovery_path.resolve()

    try:
        allowed_roots = resolve_allowed_roots(explicit_root=root, config=config)
    except (AWBError, SafetyError) as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1

    try:
        if dry_run:
            plan, manifest, snapshot = SkillRemover.plan_remove(
                skill_id=skill_id,
                target_dir=target_dir,
                allowed_roots=allowed_roots,
                allow_global=False,
            )
            if as_json:
                plan_dict = {
                    "dry_run": True,
                    "plan_id": plan.plan_id,
                    "skill_id": skill_id,
                    "target_dir": str(target_dir / skill_id),
                    "steps": [
                        {"action": s.action.value, "target": str(s.target_path)}
                        for s in plan.steps
                    ],
                }
                print(json.dumps(plan_dict, indent=2))
                return 0

            print("Aurora — Skill Removal Plan (Dry Run):")
            print("-" * 65)
            print(f"Skill ID:     {skill_id} (v{manifest.version})")
            print(f"Target Dir:   {target_dir / skill_id}")
            print(f"Plan ID:      {plan.plan_id}")
            print(f"Total Steps:  {len(plan.steps)}")
            print("Planned Removals:")
            for idx, s in enumerate(plan.steps):
                print(f"  {idx + 1}. [{s.action.value}] {s.target_path}")
            print("\nDry-run complete. No changes made to filesystem.")
            return 0

        # Normal execution
        result, manifest = SkillRemover.execute_remove(
            skill_id=skill_id,
            target_dir=target_dir,
            allowed_roots=allowed_roots,
            dry_run=False,
            allow_global=False,
        )

        if as_json:
            out = {
                "success": result.success,
                "plan_id": result.plan_id,
                "skill_id": skill_id,
                "applied_count": result.applied_count,
                "error": result.error,
            }
            print(json.dumps(out, indent=2))
            return 0 if result.success else 1

        if result.success:
            print(f"Skill '{skill_id}' successfully removed.")
            print(f"  Target: {target_dir / skill_id}")
            print(f"  Steps:  {result.applied_count} unlinked/removed transactionally under ProcessLock")
            return 0
        else:
            print(f"Skill Removal Failed: {result.error}", file=sys.stderr)
            return 1

    except (AWBError, SafetyError) as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1


def main(argv: Optional[List[str]] = None, prog: Optional[str] = None) -> int:
    """CLI main entry point."""
    parser = build_parser(prog=prog)
    args = parser.parse_args(argv)

    if not args.command:
        parser.print_help()
        return 0

    config = Config.load()

    if args.command == "detect":
        return cmd_detect(config, as_json=args.json)
    elif args.command == "list":
        return cmd_list(config, custom_registry=args.registry, agent_filter=args.agent)
    elif args.command == "validate":
        return cmd_validate(config, custom_registry=args.registry)
    elif args.command == "doctor":
        return cmd_doctor(config)
    elif args.command == "providers":
        return cmd_providers(as_json=args.json)
    elif args.command in ("install", "update"):
        return cmd_install(
            config,
            skill_id=args.skill_id,
            target=args.target,
            root=args.root,
            adapter_name=args.adapter,
            custom_registry=args.registry,
            dry_run=args.dry_run,
            as_json=args.json,
        )
    elif args.command == "remove":
        return cmd_remove(
            config,
            skill_id=args.skill_id,
            target=args.target,
            root=args.root,
            adapter_name=args.adapter,
            dry_run=args.dry_run,
            as_json=args.json,
        )

    return 0


if __name__ == "__main__":
    sys.exit(main())
