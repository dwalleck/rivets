"""
prove-it-prototype probe for rivets-v465.

Smallest question: for each of the ~95 legitimate cross-crate refs that
the simulation flagged as leaking to unscoped fallback, does the caller
file have ANY `use` statement that plausibly covers the ref — and if so,
why didn't tethys's resolver use it?

Reads tethys's DB directly via stdlib sqlite3. Classifies each leak by
import coverage. The oracle (.rivets-v465/oracle.sh) hand-greps a sample
of leaks for `use` statements to verify the probe's classifications.

Categories:
  A. NO_TARGET_IMPORT     — caller has no `use` mentioning target_crate
  B. GLOB_FROM_TARGET     — caller has `use target_crate::...::*` (glob)
  C. EXPLICIT_NAME_MATCH  — caller has `use ... where symbol_name == ref_name`
  D. PARENT_PATH_IMPORT   — caller has `use target_crate::Module` (ref uses Module::Name)
  E. OTHER                — has target_crate imports but none of the above shapes
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
# ALLOWED ordered-pairs derived from Cargo.toml dep graph
ALLOWED = {
    ("rivets",       "rivets-jsonl"),
    ("rivets-mcp",   "rivets"),
    ("rivets-mcp",   "rivets-jsonl"),
}

def crate_of(path: str) -> str | None:
    for name, root in CRATES.items():
        if path.startswith(root):
            return name
    return None

def crate_module_prefix(crate: str) -> str:
    """The Rust module name for a crate (hyphens become underscores)."""
    return crate.replace("-", "_")

conn = sqlite3.connect(DB)

# All resolved cross-crate refs to method/function/enum_variant (the leak shape)
rows = conn.execute("""
    SELECT r.id, r.reference_name, s.name, s.kind, s.qualified_name,
           r.file_id, f_caller.path, f_target.path
    FROM refs r
    JOIN symbols s ON s.id = r.symbol_id
    JOIN files f_caller ON f_caller.id = r.file_id
    JOIN files f_target ON f_target.id = s.file_id
    WHERE r.symbol_id IS NOT NULL
      AND r.kind = 'call'
      AND s.kind IN ('method', 'function', 'enum_variant')
""").fetchall()

leaks = []
for rid, ref_name, sym_name, sym_kind, qname, file_id, caller, target in rows:
    cc, tc = crate_of(caller), crate_of(target)
    if not cc or not tc or cc == tc:
        continue
    if (cc, tc) not in ALLOWED:
        continue
    leaks.append((rid, ref_name, sym_name, sym_kind, qname, file_id, caller, target, cc, tc))

print(f"Suspected leaks (ALLOWED cross-crate call refs to method/function/enum_variant): {len(leaks)}")
print()

# Classify each leak by the caller file's imports
imports_by_file = defaultdict(list)
for fid, symbol_name, source_module, alias in conn.execute(
    "SELECT file_id, symbol_name, source_module, alias FROM imports"
).fetchall():
    imports_by_file[fid].append((symbol_name, source_module, alias))

def classify(ref_name: str | None, sym_name: str, file_id: int, tc: str) -> str:
    tcm = crate_module_prefix(tc)
    imports = imports_by_file.get(file_id, [])
    target_imports = [i for i in imports if i[1].startswith(tcm + "::") or i[1] == tcm]
    if not target_imports:
        return "A. NO_TARGET_IMPORT"
    # ref_name is what tree-sitter recorded; sym_name is the resolved target's name.
    # For unqualified method/function calls, ref_name often equals sym_name.
    needle = ref_name or sym_name
    glob = any(i[0] == "*" for i in target_imports)
    explicit_match = any(
        (i[2] or i[0]) == needle and i[0] != "*" for i in target_imports
    )
    if explicit_match:
        return "C. EXPLICIT_NAME_MATCH"
    if glob:
        return "B. GLOB_FROM_TARGET"
    # Has target imports but no name match — likely 'use target::Module' and
    # the ref site uses 'Module::Name' (which would actually be ref_kind=type
    # or qualified, not bare call). Or the import is for a different name.
    return "D. PARENT_PATH_OR_OTHER"

verdicts = Counter()
samples = defaultdict(list)
for leak in leaks:
    rid, ref_name, sym_name, sym_kind, qname, file_id, caller, target, cc, tc = leak
    cat = classify(ref_name, sym_name, file_id, tc)
    verdicts[cat] += 1
    samples[cat].append((ref_name or sym_name, sym_kind, caller, qname))

print("=== Classification of leaks ===")
for cat, n in sorted(verdicts.items()):
    print(f"  {n:>4}  {cat}")
print()
print("=== Sample (up to 5 per category) ===")
for cat in sorted(samples.keys()):
    print(f"--- {cat} ---")
    for name, kind, caller, qname in samples[cat][:5]:
        print(f"  name={name!r:<22} sym_kind={kind:<12} caller={caller}  target_qual={qname!r}")
    print()
