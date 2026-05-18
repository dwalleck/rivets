"""
ycaq acceptance-criterion-#6 measurement probe.

Question: on the rivets workspace post-PR-67 (K-hybrid filter), is the
phantom-edge rate < 1%?

A cross-crate file_deps edge is "phantom" if the source file has no `use`
import into the target file's crate. The K-hybrid filter at
`crates/tethys/src/db/call_edges.rs::populate_file_deps_from_call_edges`
is supposed to drop all such edges before insertion. Post-fix, the phantom
rate should be ~0%.

This probe is INDEPENDENT of the filter code path — it queries the imports
table and file_deps table directly via stdlib sqlite3, classifies each
cross-crate file_deps edge as corroborated/phantom, and reports the rate.
That makes it a fair oracle: if the filter has a bug, the probe will see
phantoms even though the filter intended to drop them.

Layered against the FORBIDDEN-pair (Cargo-dep-graph) classification from
`.rivets-3d0s/probe.py`: a phantom is necessarily a FORBIDDEN-pair edge,
but a FORBIDDEN-pair edge could in principle be corroborated by an import
(though Cargo wouldn't allow it to compile — that case shouldn't exist in
a buildable workspace).
"""
import sqlite3
import sys
from collections import Counter
from pathlib import Path

DB = Path(".rivets/index/tethys.db")
if not DB.exists():
    sys.exit(f"missing: {DB} (run `tethys index --rebuild` first)")

CRATES = {
    "rivets":       "crates/rivets/",
    "rivets-jsonl": "crates/rivets-jsonl/",
    "rivets-mcp":   "crates/rivets-mcp/",
    "tethys":       "crates/tethys/",
}
# Cargo dep-graph-forbidden ordered pairs (no Cargo.toml edge exists).
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

def normalize_crate(seg: str) -> str:
    """Match the K-hybrid filter's `_` -> `-` normalization for first-segment matching."""
    return seg.replace("_", "-")

conn = sqlite3.connect(DB)

# === Pull all file_deps with file paths ===
fd_rows = conn.execute("""
    SELECT d.from_file_id, d.to_file_id, d.ref_count, f1.path, f2.path
    FROM file_deps d
    JOIN files f1 ON f1.id = d.from_file_id
    JOIN files f2 ON f2.id = d.to_file_id
""").fetchall()

# === Pull all imports (file_id -> set of first-segment crate-like names) ===
imp_rows = conn.execute("SELECT file_id, source_module FROM imports").fetchall()
imports_per_file: dict[int, set[str]] = {}
for fid, source_module in imp_rows:
    if not source_module:
        continue
    # First segment of e.g. `rivets_jsonl::Storage` or `tethys::db::Index`
    first = source_module.split("::", 1)[0].split(".", 1)[0]
    imports_per_file.setdefault(fid, set()).add(normalize_crate(first))

# === Classify each file_deps edge ===
total = 0
intra = 0
cross_total = 0
cross_corroborated = 0
cross_phantom = 0
cross_orphan = 0    # caller or callee outside any known crate
forbidden_pair_edges = 0
forbidden_pair_corroborated = 0
phantom_examples = []

for from_fid, to_fid, ref_count, from_path, to_path in fd_rows:
    total += 1
    fc = crate_of(from_path)
    tc = crate_of(to_path)
    if fc is None or tc is None:
        cross_orphan += 1
        continue
    if fc == tc:
        intra += 1
        continue
    cross_total += 1
    pair_forbidden = (fc, tc) in FORBIDDEN
    if pair_forbidden:
        forbidden_pair_edges += 1
    # Corroboration check: does the source file import the target crate?
    src_imports = imports_per_file.get(from_fid, set())
    is_corroborated = normalize_crate(tc) in src_imports
    if is_corroborated:
        cross_corroborated += 1
        if pair_forbidden:
            forbidden_pair_corroborated += 1
    else:
        cross_phantom += 1
        if len(phantom_examples) < 10:
            phantom_examples.append((from_path, to_path, fc, tc, ref_count))

# === Report ===
print("=" * 70)
print("ycaq acceptance #6 measurement (post-PR-67 K-hybrid)")
print("=" * 70)
print(f"total file_deps rows:        {total}")
print(f"  intra-crate:               {intra}")
print(f"  cross-crate (both known):  {cross_total}")
print(f"  orphan (unknown crate):    {cross_orphan}")
print()
print(f"=== Cross-crate corroboration (the ycaq #6 metric) ===")
print(f"  cross-crate edges:          {cross_total}")
print(f"  corroborated by import:     {cross_corroborated}")
print(f"  PHANTOM (no import):        {cross_phantom}")
if cross_total > 0:
    rate = 100.0 * cross_phantom / cross_total
    print(f"  phantom rate:               {rate:.2f}%  (target: < 1%)")
    if rate < 1.0:
        print(f"  -> ycaq #6 PASS")
    else:
        print(f"  -> ycaq #6 FAIL")
else:
    print(f"  (no cross-crate edges; vacuous pass)")
print()
print(f"=== FORBIDDEN-pair check (Cargo dep-graph violations) ===")
print(f"  FORBIDDEN-pair edges:       {forbidden_pair_edges}  (rivets-3d0s threshold: <= 5)")
print(f"    of those, corroborated:   {forbidden_pair_corroborated}  (should be 0 in a buildable workspace)")
print()
if phantom_examples:
    print(f"=== Sample phantoms (first {len(phantom_examples)}) ===")
    for from_path, to_path, fc, tc, rc in phantom_examples:
        print(f"  [{fc} -> {tc}]  rc={rc}  {from_path}  ->  {to_path}")
