#!/usr/bin/env bash
# rivets-wsix probe: enumerate UPSERT-shaped writes in crates/tethys/src/db/
# and pair each table with whether it has a clear_all_* function defined.
#
# Mechanism: regex grep over .rs source. Tables come from the file name
# (db/<table>.rs convention); UPSERT shape detected by SQL pattern in source.
#
# Output columns:
#   FILE | HAS_UPSERT | HAS_CLEAR_FN | CLEAR_FN_NAME

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

DB_DIR="crates/tethys/src/db"

for f in "$DB_DIR"/*.rs; do
    base=$(basename "$f" .rs)
    [[ "$base" == "mod" ]] && continue

    has_upsert="no"
    if grep -qE "INSERT[[:space:]]+OR[[:space:]]+REPLACE|ON[[:space:]]+CONFLICT.*DO[[:space:]]+UPDATE" "$f"; then
        has_upsert="yes"
    fi

    clear_fn=$(grep -oE "fn clear_all_[a-z_]+" "$f" | head -1 || true)
    has_clear="no"
    if [[ -n "$clear_fn" ]]; then
        has_clear="yes"
    fi

    printf '%-30s upsert=%-4s clear_fn=%-4s %s\n' "$base" "$has_upsert" "$has_clear" "${clear_fn:--}"
done
