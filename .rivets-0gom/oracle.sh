#!/usr/bin/env bash
# Oracle for rivets-0gom.
#
# Independent of tethys's resolver. Computes the expected "cross-crate
# dependency exists" matrix from two sources:
#   1. Each crate's Cargo.toml [dependencies] section (workspace-internal only)
#   2. `grep "^use <other_crate>::"` in each crate's source tree
#
# Print: for each ordered pair (A, B), whether A is permitted to have ANY
# file-level dep edges into B.
#
# Probe and oracle must agree:
#   - If oracle says "not permitted" and probe says count > 0  ->  PHANTOM EDGES
#   - If oracle says "permitted"     and probe says count == 0 ->  also a smell
#   - Otherwise: consistent (numbers may differ; presence/absence must not)

set -euo pipefail

CRATES=(rivets rivets-jsonl rivets-mcp tethys)
CARGO=(rivets/Cargo.toml rivets-jsonl/Cargo.toml rivets-mcp/Cargo.toml tethys/Cargo.toml)

declare -A USE_NAME=(
  [rivets]=rivets
  [rivets-jsonl]=rivets_jsonl
  [rivets-mcp]=rivets_mcp
  [tethys]=tethys
)

has_cargo_dep() {
  local from="$1" to="$2"
  local manifest="crates/${from}/Cargo.toml"
  # Look for either `to = ...` or `to.workspace = true` in the manifest.
  # Be conservative: any occurrence of `^to\s*=` or `^to\.` under any
  # [dependencies] section counts.
  grep -E "^${to}(\s*=|\.)" "${manifest}" >/dev/null 2>&1
}

has_use_statement() {
  local from="$1" to="$2"
  local rust_name="${USE_NAME[$to]}"
  # Search the crate's src/ for `use <rust_name>::` or `<rust_name>::` references.
  # First form is the import; second catches inline-qualified references.
  grep -rE "(^|[^a-zA-Z0-9_])${rust_name}::" "crates/${from}/src" >/dev/null 2>&1
}

printf "%-14s %-14s %-9s %-9s %s\n" "FROM" "TO" "CARGO" "USE_STMT" "VERDICT"
for from in "${CRATES[@]}"; do
  for to in "${CRATES[@]}"; do
    [[ "$from" == "$to" ]] && continue
    if has_cargo_dep "$from" "$to"; then cargo="yes"; else cargo="no"; fi
    if has_use_statement "$from" "$to"; then use_stmt="yes"; else use_stmt="no"; fi
    if [[ "$cargo" == "yes" && "$use_stmt" == "yes" ]]; then
      verdict="ALLOWED"
    elif [[ "$cargo" == "no" && "$use_stmt" == "no" ]]; then
      verdict="FORBIDDEN (zero edges expected)"
    else
      verdict="MISMATCH ($cargo cargo, $use_stmt use)"
    fi
    printf "%-14s %-14s %-9s %-9s %s\n" "$from" "$to" "$cargo" "$use_stmt" "$verdict"
  done
done
