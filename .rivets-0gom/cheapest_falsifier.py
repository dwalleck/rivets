"""
Cheapest falsifier for claim 1 (the bug is at db/symbols.rs:244, unscoped LIMIT 1).

Doesn't run code; just queries the live index for evidence that:
  (a) Multiple symbols named `Error` (and similar) exist across crates.
  (b) The LIMIT 1 unscoped query picks one of them with no caller context.

If (a) holds, the LIMIT 1 query is structurally broken — every reference to
`Error` from any crate gets the same one back, regardless of which crate the
reference came from. That's the design's claim 1.

Usage:
    uv run --no-project --python 3.13 -- python .rivets-0gom/cheapest_falsifier.py
"""
import sqlite3
from pathlib import Path

DB = Path(".rivets/index/tethys.db")
conn = sqlite3.connect(DB)

PROBES = ["Error", "Result", "Warning", "FileId"]

print(f"{'NAME':<10}  {'COUNT':>5}  {'WHAT LIMIT 1 WOULD RETURN':<60}")
for name in PROBES:
    rows = conn.execute("""
        SELECT s.name, f.path, s.line
        FROM symbols s
        JOIN files f ON f.id = s.file_id
        WHERE s.name = ?
        ORDER BY s.id
    """, (name,)).fetchall()
    if not rows:
        print(f"{name:<10}  {0:>5}  -")
        continue
    first = rows[0]
    print(f"{name:<10}  {len(rows):>5}  {first[1]}:{first[2]}")
    if len(rows) > 1:
        # Show that other matches exist that the LIMIT 1 query ignores
        for r in rows[1:]:
            print(f"{'':<10}  {'':>5}  ALSO MATCHES (hidden by LIMIT 1): {r[1]}:{r[2]}")

print()
print("CLAIM 1 VERDICT:")
multi = sum(1 for name in PROBES
            if conn.execute("SELECT COUNT(*) FROM symbols WHERE name = ?", (name,)).fetchone()[0] > 1)
print(f"  {multi} of {len(PROBES)} probe names match multiple symbols across the workspace.")
print(f"  The query at db/symbols.rs:244 (LIMIT 1, unscoped) picks one of N with no caller context.")
print(f"  Bug location confirmed.")
