"""
prove-it-prototype probe for rivets-6aoc + rivets-34tv.

Smallest factual question:
  How many `crate::*` imports in the rivets workspace would resolve correctly
  under the proposed fix (crate_root = importing file's own crate src/) but
  fail to resolve under the current code (crate_root = workspace_root/src/)?

Mechanism: read the imports table from tethys.db, simulate `resolve_module_path`'s
file-existence logic twice (once with the broken root, once with the per-file
crate's root), classify each import into one of four buckets, and dump samples.

Oracle (.rivets-6aoc/oracle.sh) independently counts `use crate::X` statements
by greping source files and checking the same filesystem invariant. Probe is
DB-driven; oracle is source-driven — different mechanisms, same target answer.
"""

import sqlite3
import sys
from collections import Counter
from pathlib import Path

REPO = Path(".").resolve()
DB = REPO / ".rivets" / "index" / "tethys.db"
if not DB.exists():
    sys.exit(f"missing: {DB} (run `cargo run -p tethys -- index` first)")

CRATES = {
    "rivets":       "crates/rivets",
    "rivets-jsonl": "crates/rivets-jsonl",
    "rivets-mcp":   "crates/rivets-mcp",
    "tethys":       "crates/tethys",
}


def crate_of(rel_path: str) -> str | None:
    for name, root in CRATES.items():
        if rel_path == root or rel_path.startswith(root + "/"):
            return name
    return None


def resolve_via_root(crate_root: Path, remainder: str) -> Path | None:
    """Mirror `resolve_crate_path` / `resolve_as_module`: try `<root>/p/q.rs`,
    then `<root>/p/q/mod.rs`. For empty remainder, fall back to lib.rs/main.rs."""
    if not remainder:
        for f in ("lib.rs", "main.rs"):
            p = crate_root / f
            if p.exists():
                return p
        return None
    parts = remainder.split("::")
    base = crate_root.joinpath(*parts)
    for cand in (base.with_suffix(".rs"), base / "mod.rs"):
        if cand.exists():
            return cand
    return None


conn = sqlite3.connect(DB)
rows = conn.execute(
    """
    SELECT i.source_module, i.symbol_name, f.id, f.path
    FROM imports i JOIN files f ON f.id = i.file_id
    WHERE i.source_module = 'crate' OR i.source_module LIKE 'crate::%'
    """
).fetchall()

ws_src = REPO / "src"  # current hardcoded crate_root (does NOT exist for rivets workspace)
buckets = Counter()  # per-symbol import-row counts
migrate_samples = []
# For oracle agreement: dedupe by (file_id, first_segment_after_crate::)
migrate_first_segments: set[tuple[int, str]] = set()
all_first_segments: set[tuple[int, str]] = set()

for source_module, symbol_name, file_id, file_path in rows:
    crate = crate_of(file_path)
    if not crate:
        buckets["e. file outside known crate"] += 1
        continue
    correct_src = REPO / CRATES[crate] / "src"
    remainder = source_module[len("crate"):].lstrip(":")
    first_segment = remainder.split("::", 1)[0] if remainder else "__CRATE_ROOT__"
    all_first_segments.add((file_id, first_segment))
    fix_path = resolve_via_root(correct_src, remainder)
    bug_path = resolve_via_root(ws_src, remainder)
    if fix_path and not bug_path:
        buckets["a. migrate (fix resolves, bug doesn't)"] += 1
        migrate_samples.append((source_module, symbol_name, file_path, fix_path))
        migrate_first_segments.add((file_id, first_segment))
    elif fix_path and bug_path:
        buckets["b. both resolve (fix and bug see same file)"] += 1
    elif not fix_path and not bug_path:
        buckets["c. both fail (likely points at non-module symbol)"] += 1
    else:
        buckets["d. bug-only (suspicious; should not happen)"] += 1

print(f"crate::* imports total (per-symbol rows): {len(rows)}")
for label, n in sorted(buckets.items()):
    print(f"  {n:>4}  {label}")

print()
print(f"=== Oracle-comparable unit: distinct (file, first_segment) pairs ===")
print(f"  All:     {len(all_first_segments)}")
print(f"  Migrate: {len(migrate_first_segments)}")

print()
migrate_count = buckets["a. migrate (fix resolves, bug doesn't)"]
print(f"=== Sample migrating per-symbol imports (up to 10 of {migrate_count}) ===")
for sm, sn, fp, target in migrate_samples[:10]:
    rel = target.relative_to(REPO).as_posix()
    print(f"  source_module={sm!r:<35} symbol={sn!r:<25} in {fp:<55} -> {rel}")
