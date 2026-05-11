"""
prove-it-prototype probe for rivets-0gom.

Reads tethys's file_deps table directly via stdlib sqlite3 (independent of
tethys's own resolver code path). Classifies each edge by the workspace crate
of its source and target files. Prints the cross-crate edge counts.

The oracle (probe.oracle.sh) computes the same counts a different way: by
parsing each crate's Cargo.toml [dependencies] and grepping each crate's
source for `use <other_crate>::` statements.

Probe and oracle should agree. If they disagree, the resolver is wrong.
"""
import sqlite3
import sys
from collections import Counter
from pathlib import Path

DB = Path(".rivets/index/tethys.db")
if not DB.exists():
    sys.exit(f"missing: {DB} (run `tethys index` first)")

# Workspace crates and their source roots (relative to workspace root).
CRATES = {
    "rivets":       "crates/rivets/",
    "rivets-jsonl": "crates/rivets-jsonl/",
    "rivets-mcp":   "crates/rivets-mcp/",
    "tethys":       "crates/tethys/",
}

def crate_of(path: str) -> str | None:
    for name, root in CRATES.items():
        if path.startswith(root):
            return name
    return None

conn = sqlite3.connect(DB)
rows = conn.execute("""
    SELECT f1.path, f2.path
    FROM file_deps d
    JOIN files f1 ON f1.id = d.from_file_id
    JOIN files f2 ON f2.id = d.to_file_id
""").fetchall()

cross = Counter()
for src, tgt in rows:
    a, b = crate_of(src), crate_of(tgt)
    if a and b and a != b:
        cross[(a, b)] += 1

print(f"total file_deps rows: {len(rows)}")
print(f"distinct cross-crate (from, to) pairs: {sum(cross.values())}")
print()
print(f"{'FROM':<14} {'TO':<14} {'EDGES':>6}")
for (a, b), n in sorted(cross.items(), key=lambda kv: (-kv[1], kv[0])):
    print(f"{a:<14} {b:<14} {n:>6}")
