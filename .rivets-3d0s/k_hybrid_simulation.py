"""
K-hybrid (imports-corroborated calls) simulation for rivets-3d0s v3 design.

Kirograph-inspired: cross-crate file_deps edges should only exist when the
source file has explicit imports into the target file's crate. This
sidesteps the phantom-edge problem by construction — phantom call
resolutions remain in call_edges, but they're filtered out during
file_deps aggregation if the caller never imported the target crate.

Rules:
  1. Intra-crate call edge -> always include
  2. Cross-crate call edge + source file has import into target crate -> include
  3. Cross-crate call edge + source file has NO import into target crate -> DROP
  4. Orphan (caller or target not in any known crate) -> include (conservative)

Outputs per-claim verifiable counts. Compares pre/post file_deps by pair.

Usage:
    cd <rivets-workspace>
    python3 .rivets-3d0s/k_hybrid_simulation.py
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

# Mapping from crate name to its Rust-namespace prefix (`rivets-mcp` -> `rivets_mcp`).
# Imports' source_module uses the rust-namespace form.
RUST_NAME = {
    "rivets":       "rivets",
    "rivets-jsonl": "rivets_jsonl",
    "rivets-mcp":   "rivets_mcp",
    "tethys":       "tethys",
}

# Oracle (from .rivets-0gom/oracle.sh)
ALLOWED_PAIRS = {("rivets", "rivets-jsonl"), ("rivets-mcp", "rivets")}
MISMATCH_PAIRS = {("tethys", "rivets"), ("tethys", "rivets-jsonl")}

def crate_of(path):
    """Return the crate name a file belongs to.

    For Cargo-known crates, returns the canonical crate name.
    For orphan files (not under any Cargo.toml-known root), returns a
    'pseudo-crate' name derived from the top-level directory. This makes
    intra-orphan-dir refs intra-pseudo-crate (kept) and cross-pseudo-crate
    refs subject to import corroboration.
    """
    for name, root in CRATES.items():
        if path.startswith(root):
            return name
    # Orphan: derive pseudo-crate from top-level dir
    top = path.split('/', 1)[0] if '/' in path else path
    return f"orphan:{top}" if top else None

def oracle_class(caller, target):
    pair = (caller, target)
    if pair in ALLOWED_PAIRS:
        return "ALLOWED"
    if pair in MISMATCH_PAIRS:
        return "MISMATCH"
    return "FORBIDDEN"

conn = sqlite3.connect(DB)

# --- Pre-fix snapshot: current file_deps edge counts by crate-pair ---
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

# --- Imports table: which (file_id, target_crate) pairs exist? ---
imports_rows = conn.execute("""
    SELECT i.file_id, f.path, i.source_module
    FROM imports i
    JOIN files f ON f.id = i.file_id
""").fetchall()

# For each (caller_file_id, target_crate), True if caller imports something from target_crate
imports_per_file_crate = defaultdict(set)  # file_id -> set of crate_name imported from
for file_id, caller_path, source_module in imports_rows:
    caller_crate = crate_of(caller_path)
    # Parse source_module first segment
    first_seg = source_module.split('::')[0] if '::' in source_module else source_module
    # Match against rust-name -> crate-name
    for crate_name, rust_name in RUST_NAME.items():
        if first_seg == rust_name:
            imports_per_file_crate[file_id].add(crate_name)
            break

# --- Call edges aggregation simulation ---
# Replicate tethys's `populate_file_deps_from_call_edges` semantics, but with
# the K-hybrid filter applied.
call_edge_rows = conn.execute("""
    SELECT s1.file_id, s2.file_id, ce.call_count
    FROM call_edges ce
    JOIN symbols s1 ON ce.caller_symbol_id = s1.id
    JOIN symbols s2 ON ce.callee_symbol_id = s2.id
    WHERE s1.file_id != s2.file_id
""").fetchall()

# Build (caller_file_id, target_file_id) -> aggregated ref_count from call edges
call_derived_file_pairs = Counter()
for caller_fid, target_fid, call_count in call_edge_rows:
    call_derived_file_pairs[(caller_fid, target_fid)] += call_count

# Build file_id -> path for crate lookup
files_rows = conn.execute("SELECT id, path FROM files").fetchall()
file_id_to_path = {fid: path for fid, path in files_rows}

# Apply K-hybrid filter
stats = {
    "call_derived_total_file_pairs": len(call_derived_file_pairs),
    "intra_crate_kept": 0,
    "cross_crate_kept_corroborated": 0,
    "cross_crate_dropped_no_import": 0,
    "orphan_kept": 0,
}
predicted_call_file_pairs_kept = set()  # file_pair set that survives filter
predicted_file_dep_edges_by_crate_pair = Counter()  # post-filter cross-crate edges

for (caller_fid, target_fid), _ in call_derived_file_pairs.items():
    caller_path = file_id_to_path.get(caller_fid, "")
    target_path = file_id_to_path.get(target_fid, "")
    caller_crate = crate_of(caller_path)
    target_crate = crate_of(target_path)

    if caller_crate is None or target_crate is None:
        # Truly path-less (no parent dir) — extremely rare; keep conservatively
        stats["orphan_kept"] += 1
        predicted_call_file_pairs_kept.add((caller_fid, target_fid))
        continue

    if caller_crate == target_crate:
        stats["intra_crate_kept"] += 1
        predicted_call_file_pairs_kept.add((caller_fid, target_fid))
        continue

    # Cross-crate (incl. orphan-pseudo-crate): check import corroboration
    has_corroborating_import = target_crate in imports_per_file_crate.get(caller_fid, set())
    if has_corroborating_import:
        stats["cross_crate_kept_corroborated"] += 1
        predicted_call_file_pairs_kept.add((caller_fid, target_fid))
        predicted_file_dep_edges_by_crate_pair[(caller_crate, target_crate)] += 1
    else:
        stats["cross_crate_dropped_no_import"] += 1
        # NOT added to kept set

# Also need to add imports-derived file_deps (which still happen during indexing)
# For accurate post-fix prediction: imports-derived edges come from indexing.rs's
# insert_file_dependency calls, separate from populate_file_deps_from_call_edges.
# These edges should be UNCHANGED by our fix.
# To predict the FINAL post-fix file_deps state, we need: imports-derived + filtered-calls-derived.
# For simplicity, assume the current `pre_edges_by_pair` minus the predicted call-derived
# drops = post state. This is conservative — some call-derived edges might also be
# import-derived (no double-count).

# Better: replicate the upsert logic. In tethys, both imports-derived and call-derived
# inserts hit file_deps with UPSERT. After our filter:
#   post_edge_exists(A,B) = (any imports-derived edge A->B exists) OR
#                          (some kept call-derived call_edges from A to B exists at file-pair granularity
#                           that map to crate-pair A_crate->B_crate)

# For simplicity, compute predicted_post_edges_by_crate_pair as:
# UNION of (imports-derived crate-pairs) and (post-filter call-derived crate-pairs)
# But we don't have a clean separation of imports-derived. As a proxy: any current edge
# whose call-derived portion was kept OR which has an imports-derived contribution
# should remain. Since we can't perfectly distinguish, use a conservative estimate:
# - For pairs where ALL their file-pairs are filtered out, AND the pair has no
#   imports-only-derived edges (which is rare), the pair drops.

# Compute: for each crate-pair, count file-pairs that survive (whether intra or cross)
post_edges_by_pair_from_calls = Counter()
for (caller_fid, target_fid) in predicted_call_file_pairs_kept:
    caller_path = file_id_to_path.get(caller_fid, "")
    target_path = file_id_to_path.get(target_fid, "")
    a, b = crate_of(caller_path), crate_of(target_path)
    if a and b and a != b:
        post_edges_by_pair_from_calls[(a, b)] += 1

# Also count imports-derived edges. An import in file F with source_module starting
# with rust_name X creates a file_dep edge from F to whichever file in crate X the
# import resolved to. We don't have that resolution in the imports table directly,
# but we can approximate: an imports-derived edge exists from file F to crate X if
# F imports from X (we have that map).
post_edges_by_pair_from_imports = Counter()
imports_derived_pair_count_estimate = Counter()  # (caller_crate, target_crate) -> # of files in caller_crate that import target_crate
for file_id, target_crates in imports_per_file_crate.items():
    caller_path = file_id_to_path.get(file_id, "")
    caller_crate = crate_of(caller_path)
    if caller_crate is None:
        continue
    for target_crate in target_crates:
        if target_crate != caller_crate:
            # An import-derived file_dep edge from this file to SOME file in target crate exists
            # Upper bound: 1 edge per (file, target_crate) — but in practice imports may resolve
            # to multiple files. For prediction purposes, count this as 1 edge contribution.
            imports_derived_pair_count_estimate[(caller_crate, target_crate)] += 1

# Final post-fix prediction (conservative): UNION of imports-derived and call-derived (filtered)
predicted_post_edges = Counter()
all_pairs = set(post_edges_by_pair_from_calls.keys()) | set(imports_derived_pair_count_estimate.keys())
for pair in all_pairs:
    # An edge exists in post state if either imports OR filtered-calls produces it
    call_count = post_edges_by_pair_from_calls.get(pair, 0)
    import_count = imports_derived_pair_count_estimate.get(pair, 0)
    # Approximate union: just take the max as upper bound
    # (real upsert dedups by (from_file, to_file), can't compute exactly here)
    predicted_post_edges[pair] = max(call_count, import_count)

# === OUTPUT ===
print(f"=== K-HYBRID SIMULATION (rivets-3d0s v3) ===")
print()
print(f"=== INPUT ===")
print(f"  call-edge file-pair groups (current): {stats['call_derived_total_file_pairs']}")
print(f"  files with imports tracked:           {len(imports_per_file_crate)}")
print()
print(f"=== FILTER DECISIONS ===")
print(f"  intra-crate kept (C3):                  {stats['intra_crate_kept']:>5}")
print(f"  cross-crate kept (import corroborated): {stats['cross_crate_kept_corroborated']:>5}")
print(f"  cross-crate DROPPED (no import):        {stats['cross_crate_dropped_no_import']:>5}")
print(f"  orphan kept (conservative):             {stats['orphan_kept']:>5}")
print()
print(f"=== PRE/POST CROSS-CRATE EDGES BY PAIR ===")
print(f"  {'FROM':<14} {'TO':<14} {'pre':>5} {'post':>5} {'delta':>6}  {'class':<10}")
total_pre = 0
total_post = 0
forbidden_pre = 0
forbidden_post = 0
allowed_pre = 0
allowed_post = 0
all_pairs_sorted = sorted(set(pre_edges_by_pair.keys()) | set(predicted_post_edges.keys()))
for (a, b) in all_pairs_sorted:
    pre = pre_edges_by_pair.get((a, b), 0)
    post = predicted_post_edges.get((a, b), 0)
    delta = post - pre
    cls = oracle_class(a, b)
    total_pre += pre
    total_post += post
    if cls == "FORBIDDEN":
        forbidden_pre += pre
        forbidden_post += post
    elif cls == "ALLOWED":
        allowed_pre += pre
        allowed_post += post
    print(f"  {a:<14} {b:<14} {pre:>5} {post:>5} {delta:>+6}  {cls}")
print()
print(f"  TOTAL cross-crate edges: pre={total_pre} post={total_post} delta={total_post-total_pre:+d}")
print(f"  FORBIDDEN-pair edges:    pre={forbidden_pre} post={forbidden_post} delta={forbidden_post-forbidden_pre:+d}")
print(f"    (target: post <= 5 per rivets-3d0s acceptance)")
print(f"  ALLOWED-pair edges:      pre={allowed_pre} post={allowed_post} delta={allowed_post-allowed_pre:+d}")
print(f"    (must not decrease meaningfully — design claim C6)")
