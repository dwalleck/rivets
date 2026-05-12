"""
Diff: probe's migrating (file, first_segment) set vs oracle's.

Used during prove-it-prototype to debug a 7-pair discrepancy between
probe (112) and oracle (105). Not part of either canonical artifact —
this is a debugging utility.
"""

import re
import sqlite3
import subprocess
import sys
from pathlib import Path

REPO = Path(".").resolve()
DB = REPO / ".rivets/index/tethys.db"
CRATES = {
    "rivets":       "crates/rivets",
    "rivets-jsonl": "crates/rivets-jsonl",
    "rivets-mcp":   "crates/rivets-mcp",
    "tethys":       "crates/tethys",
}

def crate_of(rel_path: str) -> str | None:
    for name, root in CRATES.items():
        if rel_path == root or rel_path.startswith(root + "/"):
            return name
    return None

def resolves(crate_root: Path, remainder: str) -> bool:
    if not remainder:
        return (crate_root / "lib.rs").exists() or (crate_root / "main.rs").exists()
    parts = remainder.split("::")
    base = crate_root.joinpath(*parts)
    return base.with_suffix(".rs").exists() or (base / "mod.rs").exists()

# Probe set
conn = sqlite3.connect(DB)
probe_set: set[tuple[str, str]] = set()  # (file_path, first_segment)
for source_module, symbol_name, file_path in conn.execute(
    "SELECT i.source_module, i.symbol_name, f.path FROM imports i JOIN files f ON f.id=i.file_id "
    "WHERE i.source_module='crate' OR i.source_module LIKE 'crate::%'"
):
    crate = crate_of(file_path)
    if not crate:
        continue
    remainder = source_module[len("crate"):].lstrip(":")
    first_segment = remainder.split("::", 1)[0] if remainder else "__CRATE_ROOT__"
    if resolves(REPO / CRATES[crate] / "src", remainder) and not resolves(REPO / "src", remainder):
        probe_set.add((file_path, first_segment))

# Oracle set
oracle_set: set[tuple[str, str]] = set()
for crate_name, crate_root in CRATES.items():
    src = REPO / crate_root / "src"
    if not src.is_dir():
        continue
    for rs_file in src.rglob("*.rs"):
        rel = rs_file.relative_to(REPO).as_posix()
        text = rs_file.read_text(encoding="utf-8", errors="replace")
        # Mirror oracle.sh regex: `use[\s]+crate::([A-Za-z_][A-Za-z0-9_]*)`
        segs = set(re.findall(r"use\s+crate::([A-Za-z_][A-Za-z0-9_]*)", text))
        # And bare `use crate;`
        if re.search(r"^\s*use\s+crate\s*;", text, re.MULTILINE):
            segs.add("__CRATE_ROOT__")
        for seg in segs:
            if seg == "__CRATE_ROOT__":
                if (src / "lib.rs").exists() or (src / "main.rs").exists():
                    oracle_set.add((rel, seg))
            elif (src / f"{seg}.rs").exists() or (src / seg / "mod.rs").exists():
                # Also check NOT in ws root
                if not (REPO / "src" / f"{seg}.rs").exists() and not (REPO / "src" / seg).is_dir():
                    oracle_set.add((rel, seg))

print(f"Probe set:  {len(probe_set)}")
print(f"Oracle set: {len(oracle_set)}")
print(f"Common:     {len(probe_set & oracle_set)}")
print(f"In probe but not oracle: {len(probe_set - oracle_set)}")
for f, s in sorted(probe_set - oracle_set):
    print(f"  {f}  ::  {s}")
print(f"In oracle but not probe: {len(oracle_set - probe_set)}")
for f, s in sorted(oracle_set - probe_set):
    print(f"  {f}  ::  {s}")
