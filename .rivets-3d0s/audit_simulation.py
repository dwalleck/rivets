"""
Falsifiable-design cheapest falsifier for rivets-3d0s.

Simulates the proposed kind-compatibility audit rules against the CURRENT
DB (no code changes required). For each phantom cross-crate resolved ref,
classifies as DEMOTE (would be set to symbol_id=NULL by audit) or SURVIVE
(audit rules don't catch it).

Per-claim verdicts at the bottom. If any claim fails, the design is wrong
and needs revision before any code is written.

Claims tested:
  C1: All ref_kind=type phantoms are demoted by audit.
      Falsifier: count surviving ref_kind=type phantoms. >0 means C1 false.
  C2: All ref_kind=call phantoms with sym_kind in NON_CALLABLE are demoted.
      Falsifier: count surviving call/non-callable phantoms. >0 means C2 false.
  C3: Audit reduces FORBIDDEN-pair phantom count by >= 50%.
      Falsifier: (demoted / total) < 0.50 means C3 false.
  C4: No same-crate-resolved ref is demoted by audit.
      Falsifier: count same-crate refs that would be demoted. >0 means C4 false.
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
FORBIDDEN = {
    ("tethys",       "rivets"),
    ("tethys",       "rivets-jsonl"),
    ("rivets",       "tethys"),
    ("rivets-mcp",   "tethys"),
    ("rivets-jsonl", "tethys"),
    ("rivets-mcp",   "rivets-jsonl"),
}

# Compatibility matrix
TYPE_KINDS = {"struct", "enum", "trait", "type_alias", "union"}
CALL_KINDS = {"function", "method"}
NON_CALLABLE_FOR_CALL = {"struct_field", "module", "enum_variant"}

def crate_of(path: str) -> str | None:
    for name, root in CRATES.items():
        if path.startswith(root):
            return name
    return None

def audit_would_demote(ref_kind: str, sym_kind: str,
                       caller_crate: str | None, target_crate: str | None,
                       extended: bool = False) -> bool:
    """The proposed audit's rule. Returns True if the audit would set
    symbol_id=NULL for this ref.

    Same-crate exemption: variant constructors / field accesses / module-
    qualified calls within a crate produce ref_kind/sym_kind combinations
    that look incompatible on paper but are legitimate. The audit only
    targets cross-crate fallback-resolved refs.

    When `extended=True`, the rule additionally demotes cross-crate
    `ref_kind=call` matches to `sym_kind=method` or `sym_kind=function`
    (Option A — addressing the un-ambiguation drift discovered during
    checkpointed-build of the base rule).
    """
    if caller_crate is None or target_crate is None:
        return False
    if caller_crate == target_crate:
        return False  # Same-crate: never demote
    if ref_kind == "type":
        return sym_kind not in TYPE_KINDS
    if ref_kind == "call":
        if sym_kind in NON_CALLABLE_FOR_CALL:
            return True
        if extended and sym_kind in {"method", "function"}:
            return True
    return False

conn = sqlite3.connect(DB)
all_resolved = conn.execute("""
    SELECT s.kind AS sym_kind, r.kind AS ref_kind,
           f_caller.path, f_target.path, s.name
    FROM refs r
    JOIN symbols s ON s.id = r.symbol_id
    JOIN files f_caller ON f_caller.id = r.file_id
    JOIN files f_target ON f_target.id = s.file_id
    WHERE r.symbol_id IS NOT NULL
""").fetchall()

# Bucket: phantom (cross-crate FORBIDDEN), legitimate-cross-crate, same-crate
phantoms = []
legit_cross = []
same_crate = []
for sym_kind, ref_kind, caller, target, name in all_resolved:
    cc = crate_of(caller)
    tc = crate_of(target)
    if not cc or not tc:
        continue
    record = (sym_kind, ref_kind, cc, tc, name)
    if cc == tc:
        same_crate.append(record)
    elif (cc, tc) in FORBIDDEN:
        phantoms.append(record)
    else:
        legit_cross.append(record)

print(f"Total resolved refs (crate-attributable): "
      f"{len(phantoms) + len(legit_cross) + len(same_crate)}")
print(f"  phantoms (FORBIDDEN cross-crate): {len(phantoms)}")
print(f"  legitimate cross-crate (ALLOWED pairs): {len(legit_cross)}")
print(f"  same-crate: {len(same_crate)}")
print()

# Apply simulated audit. r is (sym_kind, ref_kind, caller_crate, target_crate, name)
# Toggle EXTENDED to probe Option A (demote call->method/function cross-crate).
EXTENDED = True
print(f"[audit mode: {'EXTENDED (Option A)' if EXTENDED else 'BASE'}]")
print()

def demote(r):
    return audit_would_demote(r[1], r[0], r[2], r[3], extended=EXTENDED)

phantom_demoted = [r for r in phantoms if demote(r)]
phantom_survived = [r for r in phantoms if not demote(r)]
legit_demoted = [r for r in legit_cross if demote(r)]
same_crate_demoted = [r for r in same_crate if demote(r)]

print("=== Simulated audit verdict ===")
print(f"  phantoms demoted:     {len(phantom_demoted)} / {len(phantoms)}")
print(f"  phantoms surviving:   {len(phantom_survived)}")
print(f"  legit-cross demoted:  {len(legit_demoted)}  (false positives in ALLOWED pairs)")
print(f"  same-crate demoted:   {len(same_crate_demoted)}  (CLAIM 4 false-positive guard)")
print()

# C1: type-position phantom survival
type_survivors = [r for r in phantom_survived if r[1] == "type"]
print(f"--- C1: ALL ref_kind=type phantoms demoted ---")
if len(type_survivors) == 0:
    print(f"  PASS (0 type-position phantoms survive)")
else:
    print(f"  FAIL ({len(type_survivors)} type-position phantoms survive):")
    for r in type_survivors[:5]:
        print(f"    {r}")

# C2: call/non-callable phantom survival
non_callable_survivors = [
    r for r in phantom_survived
    if r[1] == "call" and r[0] in NON_CALLABLE_FOR_CALL
]
print(f"--- C2: ALL ref_kind=call phantoms with non-callable sym_kind demoted ---")
if len(non_callable_survivors) == 0:
    print(f"  PASS (0 call/non-callable phantoms survive)")
else:
    print(f"  FAIL ({len(non_callable_survivors)} survive):")
    for r in non_callable_survivors[:5]:
        print(f"    {r}")

# C3: overall reduction >= 50%
reduction_pct = 100.0 * len(phantom_demoted) / max(1, len(phantoms))
print(f"--- C3: Audit reduces FORBIDDEN-pair phantoms by >= 50% ---")
if reduction_pct >= 50.0:
    print(f"  PASS ({len(phantom_demoted)}/{len(phantoms)} = {reduction_pct:.1f}%)")
else:
    print(f"  FAIL ({reduction_pct:.1f}% reduction)")

# C4: no same-crate ref demoted
print(f"--- C4: No same-crate-resolved ref demoted ---")
if len(same_crate_demoted) == 0:
    print(f"  PASS (0 same-crate refs demoted)")
else:
    print(f"  FAIL ({len(same_crate_demoted)} same-crate refs demoted):")
    by_kind = Counter((r[0], r[1]) for r in same_crate_demoted)
    for (sym_kind, ref_kind), n in by_kind.most_common(5):
        print(f"    {n:>4}  sym_kind={sym_kind:<12} ref_kind={ref_kind}")

# Bonus diagnostics
print()
print(f"=== Breakdown of phantoms that SURVIVE the audit ===")
survivor_by_kind = Counter((r[0], r[1]) for r in phantom_survived)
for (sym_kind, ref_kind), n in survivor_by_kind.most_common():
    print(f"  {n:>4}  sym_kind={sym_kind:<12} ref_kind={ref_kind}")
print()
print(f"=== Legitimate cross-crate refs that would be demoted (false positives) ===")
legit_by_kind = Counter((r[0], r[1]) for r in legit_demoted)
for (sym_kind, ref_kind), n in legit_by_kind.most_common(10):
    print(f"  {n:>4}  sym_kind={sym_kind:<12} ref_kind={ref_kind}")
