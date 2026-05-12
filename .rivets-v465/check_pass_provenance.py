"""Check Pass-1 vs Pass-2 provenance for each leak.

reference_name = NULL  -> resolved at extraction time (Pass 1, tree-sitter)
reference_name != NULL -> Pass 2 saw it and resolved (via imports or fallback)

This is the closest thing to provenance tracking we have without code changes.
"""
import sqlite3
from collections import Counter
from pathlib import Path

conn = sqlite3.connect(Path(".rivets/index/tethys.db"))
CRATES = {
    "rivets":       "crates/rivets/",
    "rivets-jsonl": "crates/rivets-jsonl/",
    "rivets-mcp":   "crates/rivets-mcp/",
    "tethys":       "crates/tethys/",
}
ALLOWED = {("rivets", "rivets-jsonl"), ("rivets-mcp", "rivets"), ("rivets-mcp", "rivets-jsonl")}

def crate_of(path):
    for name, root in CRATES.items():
        if path.startswith(root):
            return name
    return None

rows = conn.execute("""
    SELECT r.reference_name, s.name, s.kind, s.qualified_name,
           f_caller.path, f_target.path
    FROM refs r
    JOIN symbols s ON s.id = r.symbol_id
    JOIN files f_caller ON f_caller.id = r.file_id
    JOIN files f_target ON f_target.id = s.file_id
    WHERE r.symbol_id IS NOT NULL
      AND r.kind = 'call'
      AND s.kind IN ('method', 'function', 'enum_variant')
""").fetchall()

verdict = Counter()
for ref_name, sym_name, sym_kind, qname, caller, target in rows:
    cc, tc = crate_of(caller), crate_of(target)
    if not cc or not tc or cc == tc or (cc, tc) not in ALLOWED:
        continue
    if ref_name is None:
        verdict[f"Pass 1 (reference_name=NULL)  sym_kind={sym_kind}"] += 1
    else:
        verdict[f"Pass 2 (reference_name set)   sym_kind={sym_kind}"] += 1

print(f"Total leaks: {sum(verdict.values())}\n")
print("=== Provenance attribution ===")
for cat, n in sorted(verdict.items()):
    print(f"  {n:>4}  {cat}")

# Same for the FORBIDDEN-pair phantoms (sanity check)
print()
print("=== Same check on FORBIDDEN-pair phantoms (rivets-3d0s territory) ===")
FORBIDDEN = {
    ("tethys",       "rivets"), ("tethys",       "rivets-jsonl"),
    ("rivets",       "tethys"), ("rivets-mcp",   "tethys"),
    ("rivets-jsonl", "tethys"), ("rivets-mcp",   "rivets-jsonl"),
}
phantom_verdict = Counter()
for ref_name, sym_name, sym_kind, qname, caller, target in conn.execute("""
    SELECT r.reference_name, s.name, s.kind, s.qualified_name,
           f_caller.path, f_target.path
    FROM refs r
    JOIN symbols s ON s.id = r.symbol_id
    JOIN files f_caller ON f_caller.id = r.file_id
    JOIN files f_target ON f_target.id = s.file_id
    WHERE r.symbol_id IS NOT NULL
""").fetchall():
    cc, tc = crate_of(caller), crate_of(target)
    if not cc or not tc or cc == tc or (cc, tc) not in FORBIDDEN:
        continue
    if ref_name is None:
        phantom_verdict[f"Pass 1 (reference_name=NULL)  sym_kind={sym_kind}"] += 1
    else:
        phantom_verdict[f"Pass 2 (reference_name set)   sym_kind={sym_kind}"] += 1

for cat, n in sorted(phantom_verdict.items()):
    print(f"  {n:>4}  {cat}")
