"""For each NO_TARGET_IMPORT leak, check whether the caller's OWN crate has
a same-name symbol that rivets-0gom's same-crate scoping should have picked.

If multiple same-name symbols exist in the caller's crate, same-crate scoping
refuses (rivets-0gom slice 3) and falls through to unscoped — that's how
these "leaks" happen. The fix isn't import-resolver; it's same-crate
ambiguity disambiguation.

If only one same-name symbol exists in the caller's crate, same-crate scoping
should have picked it. Why didn't it?
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
examples = {"zero_same_crate": [], "one_same_crate": [], "many_same_crate": []}
for ref_name, sym_name, sym_kind, qname, caller, target in rows:
    cc, tc = crate_of(caller), crate_of(target)
    if not cc or not tc or cc == tc or (cc, tc) not in ALLOWED:
        continue
    needle = ref_name or sym_name
    # Count same-crate symbols with this name
    same_crate_count = conn.execute(
        """SELECT COUNT(*) FROM symbols s
           JOIN files f ON f.id = s.file_id
           WHERE s.name = ? AND f.path LIKE ?""",
        (needle, CRATES[cc] + "%"),
    ).fetchone()[0]
    if same_crate_count == 0:
        verdict["A. ZERO same-crate symbol (legitimate cross-crate, just no import)"] += 1
        if len(examples["zero_same_crate"]) < 5:
            examples["zero_same_crate"].append((needle, sym_kind, caller, target))
    elif same_crate_count == 1:
        verdict["B. ONE same-crate symbol (rivets-0gom scoping should have found it -- BUG)"] += 1
        if len(examples["one_same_crate"]) < 5:
            examples["one_same_crate"].append((needle, sym_kind, caller, target))
    else:
        verdict[f"C. {same_crate_count}+ same-crate symbols (ambiguity refusal in rivets-0gom slice 3)"] += 1
        if len(examples["many_same_crate"]) < 5:
            examples["many_same_crate"].append((needle, sym_kind, caller, target, same_crate_count))

total = sum(verdict.values())
print(f"Total leaks examined: {total}")
print()
print("=== Why same-crate scoping didn't catch each leak ===")
for cat, n in sorted(verdict.items()):
    print(f"  {n:>4}  {cat}")
print()
for kind, exs in examples.items():
    print(f"--- {kind} ---")
    for ex in exs:
        print(f"  {ex}")
    print()
