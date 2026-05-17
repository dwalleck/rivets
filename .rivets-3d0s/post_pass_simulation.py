"""
post-pass audit simulation for rivets-3d0s (v2 design: audit-after-resolution).

Reads tethys's refs + symbols tables. Simulates the post-pass audit:
demote (set symbol_id=NULL) any cross-crate ref whose (ref_kind, sym_kind)
is incompatible per the design matrix. Same-crate refs are exempted (C3
guard); non-filtering ref_kinds (Import, Inherit, Construct, FieldAccess,
Unknown) are exempted (C5 guard).

Outputs per-claim verifiable counts. Critically: this simulation is a
faithful predictor of the implementation because the post-pass audit
operates on already-resolved refs and only nullifies symbol_id — the
prior design's un-ambiguation dynamic (narrowing during resolution
creating new unique matches) does not apply here by construction.

Usage:
    cd <rivets-workspace-with-.rivets/index/tethys.db>
    python3 .rivets-3d0s/post_pass_simulation.py
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

TYPE_ALLOW = {"struct", "class", "enum", "trait", "interface", "type_alias"}
CALL_ALLOW = {"function", "method", "macro"}
FILTER_KINDS = {"type", "call"}

# Oracle (transcribed from oracle.sh output on current main)
ALLOWED_PAIRS = {("rivets", "rivets-jsonl"), ("rivets-mcp", "rivets")}
MISMATCH_PAIRS = {("tethys", "rivets"), ("tethys", "rivets-jsonl")}
# All other ordered pairs are FORBIDDEN per oracle (cargo dep absent + no use stmt)

def crate_of(path):
    for name, root in CRATES.items():
        if path.startswith(root):
            return name
    return None

def oracle_class(caller, target):
    pair = (caller, target)
    if pair in ALLOWED_PAIRS:
        return "ALLOWED"
    if pair in MISMATCH_PAIRS:
        return "MISMATCH"
    return "FORBIDDEN"

conn = sqlite3.connect(DB)

# Fetch all resolved refs + their target symbol's file
rows = conn.execute("""
    SELECT
        r.id, r.kind, s.kind, f_caller.path, f_target.path, r.file_id, s.file_id
    FROM refs r
    JOIN symbols s     ON s.id = r.symbol_id
    JOIN files f_caller ON f_caller.id = r.file_id
    JOIN files f_target ON f_target.id = s.file_id
""").fetchall()

# Pre-audit file_deps edges (for C7 baseline)
edge_rows = conn.execute("""
    SELECT f1.path, f2.path
    FROM file_deps d
    JOIN files f1 ON f1.id = d.from_file_id
    JOIN files f2 ON f2.id = d.to_file_id
""").fetchall()
pre_edges_by_pair = Counter()
for src, tgt in edge_rows:
    a, b = crate_of(src), crate_of(tgt)
    if a and b and a != b:
        pre_edges_by_pair[(a, b)] += 1

# Simulate per-ref demotion
stats = {
    "total_resolved": 0,
    "candidate_for_demotion": 0,  # before guards
    "demoted_after_guards": 0,
    "saved_by_C3_same_crate": 0,
    "saved_by_orphan": 0,
    "skipped_C4_kind_compatible": 0,
    "skipped_C5_other_ref_kind": 0,
}
demoted_by_ref_kind = Counter()
demoted_by_pair_class = Counter()
# (file pair) -> refs surviving (count); used to predict edge survival
surviving_refs_per_file_pair = Counter()
total_refs_per_file_pair = Counter()

for ref_id, ref_kind, sym_kind, caller_path, target_path, caller_fid, target_fid in rows:
    stats["total_resolved"] += 1
    file_pair = (caller_fid, target_fid)
    total_refs_per_file_pair[file_pair] += 1

    # Classify ref_kind for the matrix
    raw_demote_candidate = (
        (ref_kind == "type" and sym_kind not in TYPE_ALLOW)
        or (ref_kind == "call" and sym_kind not in CALL_ALLOW)
    )
    if not raw_demote_candidate:
        # Either kind-compatible or non-filter ref_kind
        if ref_kind in FILTER_KINDS:
            stats["skipped_C4_kind_compatible"] += 1
        else:
            stats["skipped_C5_other_ref_kind"] += 1
        surviving_refs_per_file_pair[file_pair] += 1
        continue

    stats["candidate_for_demotion"] += 1
    caller_crate = crate_of(caller_path)
    target_crate = crate_of(target_path)

    # Orphan guard (caller or target outside any known crate)
    if caller_crate is None or target_crate is None:
        stats["saved_by_orphan"] += 1
        surviving_refs_per_file_pair[file_pair] += 1
        continue

    # C3 same-crate guard
    if caller_crate == target_crate:
        stats["saved_by_C3_same_crate"] += 1
        surviving_refs_per_file_pair[file_pair] += 1
        continue

    # Survives all guards — demote
    stats["demoted_after_guards"] += 1
    demoted_by_ref_kind[ref_kind] += 1
    demoted_by_pair_class[oracle_class(caller_crate, target_crate)] += 1
    # NOT incrementing surviving_refs_per_file_pair — this ref is demoted

# Predict post-audit edge survival (an edge survives iff at least 1 ref survives for that file_pair)
predicted_post_audit_edges_by_pair = Counter()
for file_pair, total in total_refs_per_file_pair.items():
    survivors = surviving_refs_per_file_pair.get(file_pair, 0)
    if survivors > 0:
        # Edge survives. Map file pair to crate pair.
        # Re-fetch caller/target file path to crate-map them.
        caller_fid, target_fid = file_pair
        # (No need to re-query; we have the data in `rows` but let's keep it simple)
        pass

# Cleaner edge prediction: bucket file pairs by crate-pair and count survivors
file_pair_to_crate_pair = {}
for _, _, _, caller_path, target_path, caller_fid, target_fid in rows:
    a, b = crate_of(caller_path), crate_of(target_path)
    if a and b and a != b:
        file_pair_to_crate_pair[(caller_fid, target_fid)] = (a, b)

predicted_post_edges = Counter()
for file_pair, crate_pair in file_pair_to_crate_pair.items():
    if surviving_refs_per_file_pair.get(file_pair, 0) > 0:
        predicted_post_edges[crate_pair] += 1

# Output
print(f"=== INPUT ===")
print(f"  Total resolved refs (with target file): {stats['total_resolved']}")
print()
print(f"=== AUDIT DECISIONS ===")
print(f"  C4 skip (kind-compatible, no audit needed):  {stats['skipped_C4_kind_compatible']:>5}")
print(f"  C5 skip (Import/Inherit/Construct/etc):      {stats['skipped_C5_other_ref_kind']:>5}")
print(f"  Candidate for demotion (Type|Call + incompat): {stats['candidate_for_demotion']:>5}")
print(f"    saved by C3 (same-crate guard):              {stats['saved_by_C3_same_crate']:>5}")
print(f"    saved by orphan-file guard:                  {stats['saved_by_orphan']:>5}")
print(f"    DEMOTED after all guards:                    {stats['demoted_after_guards']:>5}")
print()
print(f"=== DEMOTED BY REF_KIND ===")
for k, n in demoted_by_ref_kind.most_common():
    print(f"  {k:<10} {n:>5}")
print()
print(f"=== DEMOTED BY ORACLE CLASS (verifies design intent) ===")
for cls, n in demoted_by_pair_class.most_common():
    print(f"  {cls:<10} {n:>5}")
print()
print(f"=== PRE/POST EDGE PREDICTION BY CRATE-PAIR ===")
print(f"  {'FROM':<14} {'TO':<14} {'pre':>5} {'post':>5} {'delta':>6}  {'class':<10}")
all_pairs = sorted(set(pre_edges_by_pair.keys()) | set(predicted_post_edges.keys()))
total_pre = 0
total_post = 0
forbidden_pre = 0
forbidden_post = 0
for (a, b) in all_pairs:
    pre = pre_edges_by_pair.get((a, b), 0)
    post = predicted_post_edges.get((a, b), 0)
    delta = post - pre
    cls = oracle_class(a, b)
    total_pre += pre
    total_post += post
    if cls == "FORBIDDEN":
        forbidden_pre += pre
        forbidden_post += post
    print(f"  {a:<14} {b:<14} {pre:>5} {post:>5} {delta:>+6}  {cls}")
print()
print(f"  TOTAL cross-crate edges:  pre={total_pre} post={total_post}  delta={total_post-total_pre:+d}")
print(f"    (C8 check: delta MUST be <= 0 — audit cannot ADD edges)")
print(f"  FORBIDDEN-pair edges:     pre={forbidden_pre} post={forbidden_post}  delta={forbidden_post-forbidden_pre:+d}")
if forbidden_pre > 0:
    reduction_pct = (forbidden_pre - forbidden_post) / forbidden_pre * 100
    print(f"    (C7 check: ≥50% reduction required; actual={reduction_pct:.1f}%)")
