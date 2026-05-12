"""
Slice 5 measurement script for rivets-6aoc.

Reads tethys.db and emits a JSON snapshot of counts that bear on claims
C5 (Pass-2-imports resolution count), C6 (total resolved count), and
C7 (timing — separate hyperfine step).

Usage:
    python .rivets-6aoc/measure.py [label]

`label` is included in the output JSON to disambiguate snapshots (e.g.,
"pre-fix-with-fallback", "post-fix-without-fallback"). Defaults to
"unlabeled".

Run pre-fix and post-fix, save outputs to .rivets-6aoc/measure-*.json,
then diff with `diff` or jq.
"""

import json
import sqlite3
import sys
from pathlib import Path

REPO = Path(".").resolve()
DB = REPO / ".rivets" / "index" / "tethys.db"

if not DB.exists():
    sys.exit(f"missing: {DB} (run `cargo run -p tethys --release -- index` first)")

label = sys.argv[1] if len(sys.argv) > 1 else "unlabeled"
conn = sqlite3.connect(DB)

CRATES = {
    "rivets":       "crates/rivets",
    "rivets-jsonl": "crates/rivets-jsonl",
    "rivets-mcp":   "crates/rivets-mcp",
    "tethys":       "crates/tethys",
}


def crate_of(path: str) -> str | None:
    for n, r in CRATES.items():
        if path == r or path.startswith(r + "/"):
            return n
    return None


def scalar(query: str, *args) -> int:
    return conn.execute(query, args).fetchone()[0]


total_refs = scalar("SELECT COUNT(*) FROM refs")
resolved_refs = scalar("SELECT COUNT(*) FROM refs WHERE symbol_id IS NOT NULL")
unresolved_refs = scalar("SELECT COUNT(*) FROM refs WHERE symbol_id IS NULL")

# Cross-crate resolved refs (file_id and symbol_id's file in different crates).
# This is the count Pass-2-imports for crate::* paths should *not* increase
# (those are intra-crate); it's a sanity check that nothing regressed.
cross_crate_resolved = 0
intra_crate_resolved = 0
for caller_path, target_path in conn.execute(
    """
    SELECT f_caller.path, f_target.path
    FROM refs r
    JOIN symbols s ON s.id = r.symbol_id
    JOIN files f_caller ON f_caller.id = r.file_id
    JOIN files f_target ON f_target.id = s.file_id
    WHERE r.symbol_id IS NOT NULL
    """
):
    cc, tc = crate_of(caller_path), crate_of(target_path)
    if cc and tc:
        if cc == tc:
            intra_crate_resolved += 1
        else:
            cross_crate_resolved += 1

# file_deps edges: how many distinct (file, target_file) pairs exist?
file_deps_total = scalar("SELECT COUNT(*) FROM file_deps")
# file_deps within the same crate vs across crates
file_deps_intra = 0
file_deps_cross = 0
for from_path, to_path in conn.execute(
    """
    SELECT f1.path, f2.path FROM file_deps d
    JOIN files f1 ON f1.id = d.from_file_id
    JOIN files f2 ON f2.id = d.to_file_id
    """
):
    fc, tc = crate_of(from_path), crate_of(to_path)
    if fc and tc:
        if fc == tc:
            file_deps_intra += 1
        else:
            file_deps_cross += 1

# Pass-2 import provenance proxy: refs whose source_module is still
# non-NULL (unresolved) and starts with `crate::`. Pre-fix, many `crate::*`
# refs failed Pass-2-imports and fell back to fallback (which clears
# reference_name on resolve). Post-fix, fewer should remain unresolved.
unresolved_crate_refs = scalar(
    "SELECT COUNT(*) FROM refs WHERE symbol_id IS NULL AND "
    "(reference_name LIKE 'crate::%' OR reference_name = 'crate')"
)

snapshot = {
    "label": label,
    "total_refs": total_refs,
    "resolved_refs": resolved_refs,
    "unresolved_refs": unresolved_refs,
    "intra_crate_resolved": intra_crate_resolved,
    "cross_crate_resolved": cross_crate_resolved,
    "file_deps_total": file_deps_total,
    "file_deps_intra_crate": file_deps_intra,
    "file_deps_cross_crate": file_deps_cross,
    "unresolved_crate_refs": unresolved_crate_refs,
}

print(json.dumps(snapshot, indent=2))
