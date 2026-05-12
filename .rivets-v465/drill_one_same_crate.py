"""For the 6 'one same-crate' leaks, show the same-crate symbol's location
so we can investigate why rivets-0gom's same-crate scoping didn't catch it.
"""
import sqlite3
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

seen = set()
for ref_name, sym_name, sym_kind, qname, caller, target in rows:
    cc, tc = crate_of(caller), crate_of(target)
    if not cc or not tc or cc == tc or (cc, tc) not in ALLOWED:
        continue
    needle = ref_name or sym_name
    same_crate = conn.execute(
        """SELECT s.qualified_name, f.path, s.line, s.kind
           FROM symbols s JOIN files f ON f.id = s.file_id
           WHERE s.name = ? AND f.path LIKE ?""",
        (needle, CRATES[cc] + "%"),
    ).fetchall()
    if len(same_crate) != 1:
        continue
    key = (needle, caller)
    if key in seen:
        continue
    seen.add(key)
    print(f"=== ref name={needle!r}  in {caller} ===")
    print(f"  resolved to (cross-crate): {target}  qualified_name={qname!r}  kind={sym_kind}")
    sc_qname, sc_path, sc_line, sc_kind = same_crate[0]
    print(f"  same-crate candidate:     {sc_path}:{sc_line}  qualified_name={sc_qname!r}  kind={sc_kind}")
    print()
