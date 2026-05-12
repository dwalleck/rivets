"""
Cheapest falsifier for falsifiable-design claim C3:
  For every crate in the rivets workspace, the proposed `CrateInfo::src_root()`
  logic (lib_path.parent() joined with crate path, falling back to path/src
  when lib_path is None) produces a path that exists on disk.

Mechanism: parse each crate's Cargo.toml independently (no tethys code), apply
the proposed src_root() logic, check the directory exists.

Independent oracle (printed alongside): a direct filesystem walk listing each
crate's src/ directory. If the proposed logic disagrees with what's on disk
for any crate, C3 is FALSE and the design needs revision.
"""

import sys
import tomllib
from pathlib import Path

REPO = Path(".").resolve()
WORKSPACE_TOML = REPO / "Cargo.toml"

if not WORKSPACE_TOML.exists():
    sys.exit(f"missing: {WORKSPACE_TOML}")

# Read workspace members from Cargo.toml
ws = tomllib.loads(WORKSPACE_TOML.read_text(encoding="utf-8"))
members_pattern = ws.get("workspace", {}).get("members", [])
# Expand simple glob "crates/*"
member_dirs: list[Path] = []
for pat in members_pattern:
    if "*" in pat:
        member_dirs.extend(sorted(p for p in REPO.glob(pat) if p.is_dir()))
    else:
        member_dirs.append(REPO / pat)


def proposed_src_root(crate_dir: Path) -> tuple[Path, str]:
    """Mirror the proposed CrateInfo::src_root() logic.

    Returns (computed_path, derivation).
    """
    manifest = crate_dir / "Cargo.toml"
    if not manifest.exists():
        return crate_dir / "src", "no Cargo.toml; fallback to path/src"
    data = tomllib.loads(manifest.read_text(encoding="utf-8"))
    lib = data.get("lib", {})
    # CrateInfo.lib_path mirrors cargo's [lib] path or default src/lib.rs
    if "path" in lib:
        lib_path = Path(lib["path"])  # relative to crate_dir
    elif (crate_dir / "src" / "lib.rs").exists():
        lib_path = Path("src/lib.rs")
    else:
        # Bin-only crate: lib_path is None; src_root falls back to path/src
        return crate_dir / "src", "bin-only; fallback to path/src"
    parent = lib_path.parent
    if str(parent) in ("", "."):
        # lib_path is just "lib.rs" with no parent dir
        return crate_dir, f"lib_path={lib_path}; parent empty"
    return crate_dir / parent, f"lib_path={lib_path}; parent={parent}"


print("=== Claim C3 falsifier: per-crate src_root() result vs filesystem ===\n")
all_ok = True
for crate_dir in member_dirs:
    computed, why = proposed_src_root(crate_dir)
    exists = computed.is_dir()
    rel = computed.relative_to(REPO).as_posix() if computed.is_absolute() else str(computed)
    status = "OK" if exists else "FAIL"
    if not exists:
        all_ok = False
    print(f"  [{status}] {crate_dir.name:<15} -> {rel}")
    print(f"         derivation: {why}")
    # Independent oracle: list actual on-disk src/ contents
    if exists:
        sample = sorted(computed.glob("*"))[:3]
        oracle_view = ", ".join(p.name for p in sample) or "(empty)"
        print(f"         on-disk:    {oracle_view}{'...' if len(list(computed.glob('*'))) > 3 else ''}")
    print()

print()
print(f"Result: {'PASS' if all_ok else 'FAIL'}")
print()
if all_ok:
    print("C3 survives its cheapest attempt at falsification.")
    print("Design may proceed to claims C1, C2, C4-C7.")
else:
    print("C3 FAILED: proposed src_root() logic does not match disk for at least one crate.")
    print("Design must be revised before approval.")
    sys.exit(1)
