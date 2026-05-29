#!/usr/bin/env bash
# rivets-wsix oracle: independent enumeration of tethys's tables via schema
# DDL, then audit ALL insert sites in crates/tethys/src/ for each table.
#
# Different mechanism from probe.sh:
#   - Probe assumed 1 table per file (db/<table>.rs convention).
#   - Oracle reads actual CREATE TABLE statements from schema.rs, then
#     greps for INSERT INTO <table> across the whole tethys crate.
#   - Catches tables that don't follow the file naming convention
#     (subordinate tables like enum_variants, struct_fields per PR #58).
#
# Output: for each schema-declared table, list every INSERT site found
# and flag UPSERT shape.

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

SCHEMA_FILE="crates/tethys/src/db/schema.rs"

# Extract table names from CREATE TABLE statements (handle quoted, IF NOT EXISTS, etc.)
tables=$(grep -oE "CREATE TABLE (IF NOT EXISTS )?[a-z_]+" "$SCHEMA_FILE" \
         | awk '{print $NF}' | sort -u)

echo "=== Tables declared in $SCHEMA_FILE ==="
echo "$tables"
echo

echo "=== INSERT sites per table ==="
for t in $tables; do
    # Find INSERT INTO <table> ( or INSERT OR REPLACE INTO <table>
    sites=$(grep -rnE "INSERT( OR REPLACE)? INTO ${t}\b" \
            crates/tethys/src --include="*.rs" 2>/dev/null || true)
    if [[ -z "$sites" ]]; then
        printf "%-30s [NO INSERT SITES FOUND]\n" "$t"
        continue
    fi
    while IFS= read -r line; do
        file_line=$(echo "$line" | cut -d: -f1-2)
        sql=$(echo "$line" | cut -d: -f3- | sed 's/^[[:space:]]*//' \
              | cut -c1-100)
        upsert_marker=""
        # Check if the line OR a nearby line (within 5 lines) has UPSERT shape
        loc=$(echo "$line" | cut -d: -f1-2)
        fpath=$(echo "$loc" | cut -d: -f1)
        lnum=$(echo "$loc" | cut -d: -f2)
        end=$((lnum + 5))
        nearby=$(sed -n "${lnum},${end}p" "$fpath" 2>/dev/null || true)
        if echo "$nearby" | grep -qE "ON CONFLICT.*DO UPDATE|INSERT OR REPLACE"; then
            upsert_marker=" [UPSERT]"
        fi
        printf "%-30s %s%s\n" "$t" "$file_line" "$upsert_marker"
    done <<< "$sites"
done

echo
echo "=== clear_all_X function definitions ==="
grep -rnE "fn clear_all_[a-z_]+" crates/tethys/src --include="*.rs" \
    | sed 's|crates/tethys/src/||'
