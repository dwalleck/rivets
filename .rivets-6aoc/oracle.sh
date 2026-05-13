#!/usr/bin/env bash
# rivets-6aoc oracle: independently verify the probe's count using
# grep over source files + filesystem checks (NO sqlite, NO probe code).
#
# Mechanism: for each .rs file in a crate's src/, grep `use crate::X` lines
# (including grouped `use crate::X::{...}`), extract first module segment, and
# check whether <crate>/src/X.rs (or /X/mod.rs) exists AND <workspace_root>/src/X.rs
# does NOT. Each (file, first_segment) pair counted once.
#
# Comparison target: probe.py's "a. migrate" bucket count.

set -eo pipefail
cd "$(dirname "$0")/.."

total_use_lines=0
migrate_pairs=0
declare -a samples

for crate in rivets rivets-jsonl rivets-mcp tethys; do
    src="crates/$crate/src"
    [[ -d "$src" ]] || continue
    # Walk all .rs files in this crate
    while IFS= read -r rs_file; do
        # Find unique first-segments after `crate::` in this file
        # Patterns covered: `use crate::X`, `use crate::X::Y`, `use crate::X::{...}`
        # Also handles indented imports and trailing punctuation
        segments=$( (grep -E 'use[[:space:]]+crate::' "$rs_file" 2>/dev/null \
            | sed -nE 's/.*use[[:space:]]+crate::([A-Za-z_][A-Za-z0-9_]*).*/\1/p' \
            | sort -u) || true )
        # Also: `use crate;` (bare) — count as empty segment (resolves to lib.rs)
        if grep -qE '^\s*use\s+crate\s*;' "$rs_file" 2>/dev/null; then
            segments=$(printf '%s\n%s\n' "$segments" "__CRATE_ROOT__" | sort -u)
        fi
        for seg in $segments; do
            total_use_lines=$((total_use_lines + 1))
            if [[ "$seg" == "__CRATE_ROOT__" ]]; then
                # crate root resolves to lib.rs / main.rs
                if [[ -f "$src/lib.rs" || -f "$src/main.rs" ]] \
                   && [[ ! -f "src/lib.rs" && ! -f "src/main.rs" ]]; then
                    migrate_pairs=$((migrate_pairs + 1))
                fi
            elif [[ -f "$src/$seg.rs" || -f "$src/$seg/mod.rs" ]] \
                && [[ ! -f "src/$seg.rs" && ! -d "src/$seg" ]]; then
                migrate_pairs=$((migrate_pairs + 1))
                if [[ ${#samples[@]} -lt 8 ]]; then
                    rel_file=${rs_file#./}
                    samples+=("$rel_file: use crate::$seg -> $src/$seg")
                fi
            fi
        done
    done < <(find "$src" -name '*.rs' -type f)
done

echo "Oracle: scanned $(find crates/{rivets,rivets-jsonl,rivets-mcp,tethys}/src -name '*.rs' -type f | wc -l) .rs files in workspace src dirs"
echo "Oracle: $total_use_lines distinct (file, first_segment) pairs with use crate::"
echo "Oracle: $migrate_pairs of those would migrate under the fix"
echo ""
echo "Samples:"
if [[ ${#samples[@]} -gt 0 ]]; then
    for s in "${samples[@]}"; do
        echo "  $s"
    done
else
    echo "  (no migrating samples captured)"
fi
