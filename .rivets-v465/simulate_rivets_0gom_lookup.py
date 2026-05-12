"""Simulate rivets-0gom's search_symbol_by_name_in_path_prefix exactly,
for each of the 6 'one same-crate' leaks. Confirm the query DOES return
the expected same-crate symbol — which means tethys's resolver should
have used it, and the bug is elsewhere (not in the SQL).
"""
import sqlite3
from pathlib import Path

conn = sqlite3.connect(Path(".rivets/index/tethys.db"))

cases = [
    ("Io",             "crates/rivets-mcp/"),
    ("IssueNotFound",  "crates/rivets-mcp/"),
]

for name, prefix in cases:
    like = prefix + "%"
    # Exact mirror of search_symbol_by_name_in_path_prefix's SQL.
    rows = conn.execute(
        """SELECT s.id, s.name, s.qualified_name, s.kind, f.path, s.line
           FROM symbols s
           JOIN files f ON f.id = s.file_id
           WHERE s.name = ?1
             AND s.file_id IN (SELECT id FROM files WHERE path LIKE ?2)
           LIMIT 2""",
        (name, like),
    ).fetchall()
    print(f"--- search_symbol_by_name_in_path_prefix({name!r}, {prefix!r}) ---")
    if not rows:
        print(f"  EMPTY result set (would explain why same-crate scoping fell through)")
    elif len(rows) == 1:
        sid, n, qn, kind, path, line = rows[0]
        print(f"  UNIQUE: {path}:{line}  qualified_name={qn!r}  kind={kind}")
        print(f"  -> rivets-0gom should resolve to this; tethys did NOT. Bug is upstream of the SQL.")
    else:
        print(f"  AMBIGUOUS: {len(rows)} candidates")
        for sid, n, qn, kind, path, line in rows:
            print(f"    {path}:{line}  qualified_name={qn!r}  kind={kind}")
    print()
