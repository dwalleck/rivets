"""
prove-it-prototype probe for rivets-3d0s.

Smallest question: for the ~52 residual phantom cross-crate resolved refs
that survived rivets-0gom, what SymbolKind is each resolved target? This
gates the fix shape:

  - Mostly Method / Field kinds (impl-item pollution): fix at symbol-insert
    or search-by-name (filter impl items from fallback search).
  - Mostly Function / Struct kinds: the issue's "impl items polluting the
    symbol table" hypothesis is wrong; need to think harder before designing.

Reads tethys's DB directly via stdlib sqlite3 (independent of the resolver
code path). Oracle is a shell pipeline (see oracle.sh) that hand-classifies
the top phantom-name targets by greping the workspace for their definition
site.
"""
import sqlite3
import sys
from collections import Counter
from pathlib import Path

DB = Path(".rivets/index/tethys.db")
if not DB.exists():
    sys.exit(f"missing: {DB} (run `tethys index` first)")

CRATES = {
    "rivets":       "crates/rivets/",
    "rivets-jsonl": "crates/rivets-jsonl/",
    "rivets-mcp":   "crates/rivets-mcp/",
    "tethys":       "crates/tethys/",
}
# FORBIDDEN ordered-pairs from diagnose_residual.py: pairs with no
# Cargo.toml dependency edge. Any cross-crate edge in these pairs is phantom.
FORBIDDEN = {
    ("tethys",       "rivets"),
    ("tethys",       "rivets-jsonl"),
    ("rivets",       "tethys"),
    ("rivets-mcp",   "tethys"),
    ("rivets-jsonl", "tethys"),
    ("rivets-mcp",   "rivets-jsonl"),
}

def crate_of(path: str) -> str | None:
    for name, root in CRATES.items():
        if path.startswith(root):
            return name
    return None

conn = sqlite3.connect(DB)
rows = conn.execute("""
    SELECT s.name, s.kind, s.qualified_name, s.parent_symbol_id,
           f_caller.path, f_target.path, r.kind AS ref_kind
    FROM refs r
    JOIN symbols s ON s.id = r.symbol_id
    JOIN files f_caller ON f_caller.id = r.file_id
    JOIN files f_target ON f_target.id = s.file_id
    WHERE r.symbol_id IS NOT NULL
""").fetchall()

phantoms = []
for name, kind, qual, parent, caller, target, ref_kind in rows:
    cc, tc = crate_of(caller), crate_of(target)
    if cc and tc and cc != tc and (cc, tc) in FORBIDDEN:
        phantoms.append((name, kind, qual, parent, cc, tc, ref_kind))

print(f"Phantom cross-crate resolved refs (FORBIDDEN pairs): {len(phantoms)}")
print()
print("=== Resolved-target SymbolKind distribution ===")
for kind, n in Counter(k for _, k, _, _, _, _, _ in phantoms).most_common():
    print(f"  {n:>4}  {kind}")
print()
print("=== Reference-site kind distribution (call / field / type / ...) ===")
for rk, n in Counter(rk for _, _, _, _, _, _, rk in phantoms).most_common():
    print(f"  {n:>4}  {rk}")
print()
print("=== Top 15 (name, sym_kind, ref_kind) by frequency ===")
by_triple = Counter((n, k, rk) for n, k, _, _, _, _, rk in phantoms)
for (name, kind, ref_kind), n in by_triple.most_common(15):
    print(f"  {n:>4}  name={name!r:<22} sym_kind={kind:<12} ref_kind={ref_kind}")
