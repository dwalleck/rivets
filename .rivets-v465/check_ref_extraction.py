"""For the 6 'one same-crate' leaks, show exactly what tethys extracted
into the refs table — particularly reference_name (which determines
whether is_qualified='true' triggers get_symbol_by_qualified_name path).
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
    SELECT r.id, r.reference_name, r.kind AS ref_kind, r.line, r.column,
           s.name, s.kind AS sym_kind, s.qualified_name,
           f_caller.path, f_target.path
    FROM refs r
    JOIN symbols s ON s.id = r.symbol_id
    JOIN files f_caller ON f_caller.id = r.file_id
    JOIN files f_target ON f_target.id = s.file_id
    WHERE r.symbol_id IS NOT NULL
      AND r.kind = 'call'
      AND s.kind IN ('method', 'function', 'enum_variant')
""").fetchall()

print("Inspecting 'one same-crate' leaks (ones with a unique caller-crate same-name candidate):\n")
seen = set()
for rid, ref_name, ref_kind, line, col, sym_name, sym_kind, qname, caller, target in rows:
    cc, tc = crate_of(caller), crate_of(target)
    if not cc or not tc or cc == tc or (cc, tc) not in ALLOWED:
        continue
    needle = ref_name or sym_name
    same_crate_count = conn.execute(
        "SELECT COUNT(*) FROM symbols s JOIN files f ON f.id = s.file_id WHERE s.name = ? AND f.path LIKE ?",
        (needle, CRATES[cc] + "%"),
    ).fetchone()[0]
    if same_crate_count != 1:
        continue
    key = (needle, caller)
    if key in seen:
        continue
    seen.add(key)
    has_colons = "::" in (ref_name or "")
    print(f"--- ref id={rid} {caller}:{line}:{col} ---")
    print(f"  reference_name (as extracted): {ref_name!r}")
    print(f"  is_qualified (contains '::'):  {has_colons}")
    print(f"  resolved to:  {target}  qualified_name={qname!r}")
    print()
