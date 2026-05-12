"""
Independent oracle for the rivets-3d0s probe.

Walks workspace source via stdlib (os.walk + re) — does NOT use tethys's
extractor or DB. For each phantom-name+kind tuple claimed by the probe,
classify the actual workspace context by regex.

The probe asks "what is the resolved-target SymbolKind for the 174 phantom
cross-crate refs?" The oracle answers "where in workspace source is this
name actually defined, and as what?" If the oracle finds a real definition
that matches the probe's claimed kind, they agree. If the oracle finds
NO real definition, tethys is fabricating the symbol — that's a different
bug than what the probe says.
"""
import re
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path("crates")
if not ROOT.exists():
    sys.exit("run from repo root (crates/ not found)")

# (name, probe-claimed sym_kind) — top phantoms from probe.py output
QUERIES = [
    ("len",         "method"),
    ("children",    "struct_field"),
    ("display",     "module"),
    ("Tree",        "enum_variant"),
    ("write",       "method"),
    ("Serialize",   "enum_variant"),
    ("Deserialize", "enum_variant"),
    ("Parser",      "enum_variant"),
]

# Regex patterns matched line-by-line. Each maps to an oracle-inferred kind.
def classify_line(name: str, line: str) -> str | None:
    if re.match(rf"\s*(pub(\([^)]+\))?\s+)?mod\s+{name}\b", line):
        return "module"
    if re.match(rf"\s*(pub(\([^)]+\))?\s+)?(struct|union)\s+{name}\b", line):
        return "struct"
    if re.match(rf"\s*(pub(\([^)]+\))?\s+)?enum\s+{name}\b", line):
        return "enum"
    if re.match(rf"\s*(pub(\([^)]+\))?\s+)?trait\s+{name}\b", line):
        return "trait"
    # Methods: pub fn NAME(...) inside an impl block — we approximate by
    # "fn NAME(" at any indent, and rely on companion impl-context check.
    if re.search(rf"\bfn\s+{name}\s*[(<]", line):
        return "function_or_method"
    # Enum variant (heuristic): NAME, or NAME(...), or NAME { ... } in an
    # indented context. We require it be inside an enum {} block — done
    # below by tracking enum context, not per-line.
    return None

def walk_workspace():
    """Yield (file_path, in_enum, in_impl, line_no, line) tuples."""
    for path in ROOT.rglob("*.rs"):
        in_enum_depth = 0
        in_impl_depth = 0
        brace_depth = 0
        in_enum_start = []
        in_impl_start = []
        with open(path, encoding="utf-8", errors="replace") as fh:
            for line_no, line in enumerate(fh, start=1):
                # Crude brace tracking. Not Rust-correct, good enough for files
                # that don't have braces in string literals across multiple lines.
                if re.match(r"\s*(pub(\([^)]+\))?\s+)?enum\s+\w+", line):
                    in_enum_start.append(brace_depth)
                if re.match(r"\s*impl(<[^>]+>)?(\s+\w+(\s+for\s+\w+)?)?\b", line):
                    in_impl_start.append(brace_depth)
                opens = line.count("{")
                closes = line.count("}")
                # Yield BEFORE updating brace depth so this line's context is
                # determined by enclosing braces, not braces on this line.
                yield (
                    str(path),
                    any(d < brace_depth for d in in_enum_start),
                    any(d < brace_depth for d in in_impl_start),
                    line_no,
                    line.rstrip("\n"),
                )
                brace_depth += opens - closes
                in_enum_start = [d for d in in_enum_start if d < brace_depth]
                in_impl_start = [d for d in in_impl_start if d < brace_depth]

print("=== Oracle: workspace-definition contexts for phantom names ===\n")
found = defaultdict(list)
for path, in_enum, in_impl, lno, line in walk_workspace():
    for name, _ in QUERIES:
        kind = classify_line(name, line)
        if kind:
            if kind == "function_or_method":
                kind = "method" if in_impl else "function"
            found[name].append((kind, path, lno, line.strip()))
        # Enum variant pattern (heuristic): exactly the name, possibly with
        # (...) tuple-payload or { ... } struct-payload, inside enum context.
        if in_enum and re.match(rf"\s*{name}\s*[,({{=]", line):
            found[name].append(("enum_variant", path, lno, line.strip()))
        # Struct field: `pub? NAME: T,` inside a struct body. Heuristic by
        # brace-depth and the line shape; the trailing `:` distinguishes it
        # from variants. We don't require in_struct context (which we don't
        # track) — false positives are OK since the verdict only checks
        # whether the claimed kind appears, not whether it's exclusive.
        if re.match(rf"\s*(pub(\([^)]+\))?\s+)?{name}\s*:\s*[^,;]", line):
            found[name].append(("struct_field", path, lno, line.strip()))

for name, probe_kind in QUERIES:
    matches = found.get(name, [])
    print(f"--- name={name!r}  probe_kind={probe_kind} ---")
    if not matches:
        print(f"  ORACLE: no workspace definition found")
    else:
        kinds = sorted(set(m[0] for m in matches))
        # Verdict
        if probe_kind in kinds:
            verdict = "AGREES with probe"
        elif "function" in kinds and probe_kind == "method":
            verdict = "PARTIAL (function found but probe says method)"
        else:
            verdict = f"DISAGREES (oracle found {kinds}, probe says {probe_kind})"
        print(f"  ORACLE kinds: {kinds}  -> {verdict}")
        for kind, path, lno, snippet in matches[:3]:
            print(f"    {path}:{lno}  [{kind}]  {snippet[:100]}")
    print()
