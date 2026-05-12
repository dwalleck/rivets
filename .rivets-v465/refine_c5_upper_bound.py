"""For each of the 105 ALLOWED-pair leak refs, ask: does the caller file
have an explicit import (symbol_name or alias) whose name matches the
ref's name, AND is that import's source_module workspace-crate-resolvable?

This tells us the upper bound for what the current slice-1+2 fix can
catch. If the answer is ~6, the design's C5/C6 thresholds were just
wildly off; we accept the smaller delta. If the answer is much higher
(say 50), there's a downstream resolver bug preventing the migration.

The probe is independent of the resolver code: it just compares strings
between the refs and imports tables.
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
WORKSPACE_HEADS = {"rivets", "rivets_jsonl", "rivets_mcp", "tethys", "crate", "self", "super"}

def crate_of(path):
    for name, root in CRATES.items():
        if path.startswith(root):
            return name
    return None

# All cross-crate resolved call-refs to method/function/enum_variant in ALLOWED pairs
# (the leak population)
rows = conn.execute("""
    SELECT r.id, r.reference_name, s.name, s.kind, r.file_id, f_caller.path, f_target.path
    FROM refs r
    JOIN symbols s ON s.id = r.symbol_id
    JOIN files f_caller ON f_caller.id = r.file_id
    JOIN files f_target ON f_target.id = s.file_id
    WHERE r.symbol_id IS NOT NULL
      AND r.kind = 'call'
      AND s.kind IN ('method', 'function', 'enum_variant')
""").fetchall()

leaks = []
for rid, ref_name, sym_name, sym_kind, file_id, caller, target in rows:
    cc, tc = crate_of(caller), crate_of(target)
    if not cc or not tc or cc == tc or (cc, tc) not in ALLOWED:
        continue
    needle = ref_name or sym_name
    leaks.append((needle, sym_kind, file_id, caller, tc))

# For each leak, find caller's imports where (symbol_name or alias) == ref_name
# and source_module's path[0] is workspace-resolvable.
verdict = Counter()
example_by_cat = {}

def first_seg(s: str) -> str:
    return s.split("::", 1)[0] if s else ""

for needle, sym_kind, file_id, caller, tc in leaks:
    imports = conn.execute(
        "SELECT symbol_name, source_module, alias FROM imports WHERE file_id = ?",
        (file_id,),
    ).fetchall()
    # Find imports where alias or symbol_name equals the ref name
    matching = [
        (sn, sm, al) for sn, sm, al in imports
        if (al == needle) or (al is None and sn == needle)
    ]
    if not matching:
        cat = "NO_NAME_MATCHING_IMPORT (method-on-imported-type pattern, glob, or no import at all)"
    else:
        # At least one import has the matching name. Is its source_module resolvable
        # by the new resolve_module_path?
        resolvable = [
            (sn, sm, al) for sn, sm, al in matching
            if first_seg(sm) in WORKSPACE_HEADS
        ]
        if resolvable:
            cat = "FIX_SHOULD_CATCH (named import with workspace-crate source_module)"
        else:
            cat = "NAME_MATCHES_BUT_SOURCE_NOT_RESOLVABLE (e.g., empty source_module, external)"
    verdict[cat] += 1
    if cat not in example_by_cat:
        example_by_cat[cat] = []
    if len(example_by_cat[cat]) < 4:
        example_by_cat[cat].append((needle, sym_kind, caller, matching))

print(f"Total leaks examined: {sum(verdict.values())}")
print()
print("=== Upper bound for slice-1+2 fix coverage ===")
for cat, n in sorted(verdict.items()):
    print(f"  {n:>4}  {cat}")
print()
for cat, exs in example_by_cat.items():
    print(f"--- {cat} (sample) ---")
    for needle, sym_kind, caller, matching in exs:
        match_repr = ", ".join(
            f"{sn}<-{sm}" + (f" as {al}" if al else "")
            for sn, sm, al in matching[:2]
        ) if matching else "<no name-matching import>"
        print(f"  name={needle!r:<22} sym_kind={sym_kind:<12} {caller}")
        print(f"    imports matching name: [{match_repr}]")
    print()
