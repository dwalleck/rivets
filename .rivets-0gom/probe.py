"""
prove-it-prototype probe for rivets-0gom.

Reads tethys's file_deps table directly via stdlib sqlite3 (independent of
tethys's own resolver code path). Classifies edges as cross-crate or
intra-crate, by workspace crate.

Outputs three sections:

  1. CROSS-CRATE EDGES   — primary table for claims 3 and 4
                            (FORBIDDEN pairs should be zero, ALLOWED pairs
                             should remain non-zero)
  2. INTRA-CRATE EDGES   — claim 5
                            (counts before/after fix should be identical;
                             save a snapshot, diff after fix)
  3. AMBIGUITY CHECK     — claim 6
                            (lists references resolved to a cross-crate symbol
                             when the caller's crate has NO same-name symbol AND
                             multiple cross-crate candidates exist — these are
                             the "should have returned None" cases)

The oracle (.rivets-0gom/oracle.sh) is independent ground truth for sections
1 and 2 (built from grep + Cargo.toml).
"""
import sqlite3
import sys
from collections import Counter, defaultdict
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

def crate_of(path: str) -> str | None:
    for name, root in CRATES.items():
        if path.startswith(root):
            return name
    return None

conn = sqlite3.connect(DB)

# === Section 1: cross-crate file_deps ===
edge_rows = conn.execute("""
    SELECT f1.path, f2.path
    FROM file_deps d
    JOIN files f1 ON f1.id = d.from_file_id
    JOIN files f2 ON f2.id = d.to_file_id
""").fetchall()

cross = Counter()
intra = Counter()
for src, tgt in edge_rows:
    a, b = crate_of(src), crate_of(tgt)
    if a and b:
        if a == b:
            intra[a] += 1
        else:
            cross[(a, b)] += 1

print(f"total file_deps rows: {len(edge_rows)}")
print()
print(f"=== Section 1: CROSS-CRATE EDGES (claims 3, 4) ===")
print(f"distinct cross-crate (from, to) pairs: {sum(cross.values())}")
print()
print(f"{'FROM':<14} {'TO':<14} {'EDGES':>6}")
for (a, b), n in sorted(cross.items(), key=lambda kv: (-kv[1], kv[0])):
    print(f"{a:<14} {b:<14} {n:>6}")

# === Section 2: intra-crate file_deps ===
print()
print(f"=== Section 2: INTRA-CRATE EDGES (claim 5: unchanged before/after fix) ===")
print(f"{'CRATE':<14} {'EDGES':>6}")
for crate, n in sorted(intra.items(), key=lambda kv: (-kv[1], kv[0])):
    print(f"{crate:<14} {n:>6}")

# === Section 3: ambiguity check (claim 6) ===
# A ref is "ambiguity-violated" if:
#   - the resolver picked a cross-crate target
#   - the caller's crate has NO symbol with that name
#   - the workspace has ≥ 2 cross-crate symbols with that name
print()
print(f"=== Section 3: AMBIGUITY CHECK (claim 6: should-have-been-None cases) ===")

# For each resolved ref pointing to a cross-crate symbol, get the symbol name
# and the caller's file path. Then count candidates by crate.
ref_rows = conn.execute("""
    SELECT
        s.name        AS sym_name,
        f_caller.path AS caller_path,
        f_target.path AS target_path
    FROM refs r
    JOIN symbols s     ON s.id = r.symbol_id
    JOIN files f_caller ON f_caller.id = r.file_id
    JOIN files f_target ON f_target.id = s.file_id
    WHERE r.symbol_id IS NOT NULL
""").fetchall()

# Group by symbol name to know which names have which-crate candidates available.
name_to_crates = defaultdict(set)
all_symbol_rows = conn.execute("""
    SELECT s.name, f.path FROM symbols s JOIN files f ON f.id = s.file_id
""").fetchall()
for name, path in all_symbol_rows:
    c = crate_of(path)
    if c:
        name_to_crates[name].add(c)

ambiguity_violations = []
for sym_name, caller_path, target_path in ref_rows:
    caller_crate = crate_of(caller_path)
    target_crate = crate_of(target_path)
    if not caller_crate or not target_crate or caller_crate == target_crate:
        continue
    candidate_crates = name_to_crates.get(sym_name, set())
    has_same_crate = caller_crate in candidate_crates
    cross_crate_count = len(candidate_crates - {caller_crate})
    # Violation: resolved to cross-crate AND no same-crate option AND ≥ 2 cross-crate candidates
    if not has_same_crate and cross_crate_count >= 2:
        ambiguity_violations.append((sym_name, caller_crate, target_crate, cross_crate_count))

violation_counts = Counter((v[0], v[1]) for v in ambiguity_violations)
cross_crate_refs = sum(
    1
    for _, caller_path, target_path in ref_rows
    if (cc := crate_of(caller_path)) is not None
    and (tc := crate_of(target_path)) is not None
    and cc != tc
)
print(f"refs resolved across crates: {cross_crate_refs}")
print(f"ambiguity violations (claim 6): {len(ambiguity_violations)}")
if violation_counts:
    print()
    print(f"  {'SYMBOL':<20} {'CALLER':<14} {'COUNT':>6}")
    for (name, caller), n in violation_counts.most_common(15):
        print(f"  {name:<20} {caller:<14} {n:>6}")
