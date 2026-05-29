#!/usr/bin/env bash
# rivets-wsix probe slice 3: do refs with in_symbol_id IS NULL accumulate
# across re-index runs? These are file-scope refs (mod declarations, top-level
# type annotations) not contained in any function/struct/impl, so they don't
# cascade-delete via the symbol DELETE that catches in-function refs.

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

WORK=$(mktemp -d)
trap "rm -rf $WORK" EXIT

mkdir -p "$WORK/src"
cat > "$WORK/Cargo.toml" << 'EOF'
[package]
name = "probe_null_in_symbol"
version = "0.0.0"
edition = "2021"
EOF

# Run 1: file with mod declarations and use statements (file-scope refs).
cat > "$WORK/src/lib.rs" << 'EOF'
mod helper_a;
mod helper_b;
EOF
cat > "$WORK/src/helper_a.rs" << 'EOF'
pub fn do_a() {}
EOF
cat > "$WORK/src/helper_b.rs" << 'EOF'
pub fn do_b() {}
EOF

TETHYS="$(pwd)/target/release/tethys"
cd "$WORK"

count_null_refs() {
    sqlite3 .rivets/index/tethys.db \
        "SELECT COUNT(*) FROM refs r JOIN files f ON f.id=r.file_id
         WHERE f.path='src/lib.rs' AND r.in_symbol_id IS NULL;"
}
count_total_lib_refs() {
    sqlite3 .rivets/index/tethys.db \
        "SELECT COUNT(*) FROM refs r JOIN files f ON f.id=r.file_id
         WHERE f.path='src/lib.rs';"
}

"$TETHYS" index --workspace . > /dev/null 2>&1
N1_NULL=$(count_null_refs)
N1_TOTAL=$(count_total_lib_refs)

# Run 2: REMOVE mod helper_b; (delete one of the two mod-level refs).
cat > src/lib.rs << 'EOF'
mod helper_a;
EOF
sleep 1
touch src/lib.rs

"$TETHYS" index --workspace . > /dev/null 2>&1
N2_NULL=$(count_null_refs)
N2_TOTAL=$(count_total_lib_refs)

# Run 3: change to a DIFFERENT module name entirely.
cat > src/lib.rs << 'EOF'
mod helper_a;
mod helper_c;
EOF
cat > src/helper_c.rs << 'EOF'
pub fn do_c() {}
EOF
sleep 1
touch src/lib.rs

"$TETHYS" index --workspace . > /dev/null 2>&1
N3_NULL=$(count_null_refs)
N3_TOTAL=$(count_total_lib_refs)

echo "=== src/lib.rs refs (null in_symbol_id only) across re-index runs ==="
echo "Run 1 (mod helper_a + mod helper_b):  null=$N1_NULL  total=$N1_TOTAL  expected null≥2"
echo "Run 2 (only mod helper_a):            null=$N2_NULL  total=$N2_TOTAL  expected null=1 if cleared, 2 if buggy"
echo "Run 3 (mod helper_a + mod helper_c):  null=$N3_NULL  total=$N3_TOTAL  expected null=2 if cleared, ≥3 if buggy"
echo
echo "=== dump of in_symbol_id NULL refs in src/lib.rs after run 3 ==="
sqlite3 .rivets/index/tethys.db \
    "SELECT r.reference_name, s.name AS symbol_name
     FROM refs r JOIN files f ON f.id=r.file_id
     LEFT JOIN symbols s ON s.id=r.symbol_id
     WHERE f.path='src/lib.rs' AND r.in_symbol_id IS NULL;"
echo

if [[ "$N2_NULL" -gt 1 || "$N3_NULL" -gt 2 ]]; then
    echo "BUG CONFIRMED: file-scope refs (in_symbol_id IS NULL) persist after source removal."
else
    echo "OK: file-scope refs also reflect current source state."
fi
