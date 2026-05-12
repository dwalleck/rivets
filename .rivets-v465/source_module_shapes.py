"""Cheapest falsifier for the rivets-v465 fix-design hypothesis:
'path[0] of source_module is enough to identify workspace-crate imports.'

Surveys all rows in the imports table, breaks down by the first segment
of source_module. If most workspace-crate imports start with the
unaliased crate name, the design's `path[0] == workspace_crate_name`
heuristic is sound. If many start with aliased names or weird shapes,
the design needs to handle that complexity in scope.
"""
import sqlite3
from collections import Counter
from pathlib import Path

conn = sqlite3.connect(Path(".rivets/index/tethys.db"))

WORKSPACE_CRATES_AS_MODULES = {"rivets", "rivets_jsonl", "rivets_mcp", "tethys"}

heads = Counter()
samples_by_head: dict = {}
for (source_module,) in conn.execute("SELECT source_module FROM imports").fetchall():
    if not source_module:
        head = "<empty>"
    else:
        head = source_module.split("::", 1)[0]
    heads[head] += 1
    samples_by_head.setdefault(head, []).append(source_module)

# Categorize
ws_known = sum(n for h, n in heads.items() if h in WORKSPACE_CRATES_AS_MODULES)
crate_self_super = sum(n for h, n in heads.items() if h in {"crate", "self", "super"})
external = sum(
    n for h, n in heads.items()
    if h not in WORKSPACE_CRATES_AS_MODULES and h not in {"crate", "self", "super", "<empty>"}
)
empty = heads.get("<empty>", 0)

total = sum(heads.values())
print(f"Total import rows: {total}\n")
print(f"Category breakdown:")
print(f"  {crate_self_super:>5}  crate / self / super (current resolver handles)")
print(f"  {ws_known:>5}  workspace-crate name as path[0] (current resolver: 'External crate - cannot resolve')")
print(f"  {external:>5}  external crate names (correctly returned None)")
print(f"  {empty:>5}  empty source_module")
print()
print(f"=== Top 20 path[0] heads ===")
for h, n in heads.most_common(20):
    bucket = (
        "WORKSPACE" if h in WORKSPACE_CRATES_AS_MODULES
        else "CRATE/SELF/SUPER" if h in {"crate", "self", "super"}
        else "<empty>" if h == "<empty>"
        else "external"
    )
    print(f"  {n:>5}  head={h!r:<25}  [{bucket}]")
print()
print(f"=== Sample of workspace-crate imports (first 8 of each head) ===")
for ws in sorted(WORKSPACE_CRATES_AS_MODULES):
    if ws in samples_by_head:
        print(f"--- {ws} ({heads[ws]} total) ---")
        for s in samples_by_head[ws][:8]:
            print(f"  {s}")
