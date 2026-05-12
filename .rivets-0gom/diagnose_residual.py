"""
After slice 3, FORBIDDEN pairs still have 52 phantom edges across 6 pairs.
Section 3 ambiguity count is 0 (so these aren't ambiguity violations).
That means each residual phantom is a UNIQUE-WORKSPACE-MATCH fallback:

  - Caller's crate has no same-name symbol
  - The workspace has exactly ONE cross-crate symbol with that name
  - Resolver returns it
  - But the caller doesn't actually import the target crate

Print the per-pair symbol-name distribution so we can see what's hitting
the unique-fallback path.
"""
import sqlite3
from collections import Counter

DB = ".rivets/index/tethys.db"
PAIRS = [
    ("tethys",       "rivets"),
    ("tethys",       "rivets-jsonl"),
    ("rivets",       "tethys"),
    ("rivets-mcp",   "tethys"),
    ("rivets-jsonl", "tethys"),
    ("rivets-mcp",   "rivets-jsonl"),
]
CRATES = {
    "rivets":       "crates/rivets/",
    "rivets-jsonl": "crates/rivets-jsonl/",
    "rivets-mcp":   "crates/rivets-mcp/",
    "tethys":       "crates/tethys/",
}

conn = sqlite3.connect(DB)
for caller, target in PAIRS:
    rows = conn.execute("""
        SELECT s.name, f_target.path
        FROM refs r
        JOIN symbols s ON s.id = r.symbol_id
        JOIN files f_caller ON f_caller.id = r.file_id
        JOIN files f_target ON f_target.id = s.file_id
        WHERE r.symbol_id IS NOT NULL
          AND f_caller.path LIKE ? || '%'
          AND f_target.path LIKE ? || '%'
    """, (CRATES[caller], CRATES[target])).fetchall()
    if not rows:
        continue
    print(f"=== {caller} -> {target}: {len(rows)} refs ===")
    sym_counts = Counter(name for name, _ in rows)
    for sym, n in sym_counts.most_common(10):
        # How many candidates does this symbol have workspace-wide?
        candidates = conn.execute(
            "SELECT COUNT(*) FROM symbols WHERE name = ?", (sym,)
        ).fetchone()[0]
        print(f"  {n:>4}  {sym:<25} (workspace candidates: {candidates})")
    print()
