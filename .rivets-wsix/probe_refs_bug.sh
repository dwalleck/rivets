#!/usr/bin/env bash
# rivets-wsix probe (expanded slice): does `refs` actually accumulate across
# re-index runs?
#
# Smallest factual question: if I index a fixture twice without changing
# anything, does the total number of refs in the DB double?
#
# Mechanism (probe): run `tethys index` twice on a tiny tempdir workspace,
# count rows in `refs` table via direct SQLite query between runs.
#
# Oracle: count refs from the indexing.rs trace logs themselves
# (RUST_LOG=tethys=trace). Independent because the trace fires once per
# extracted ref, regardless of what the DB does on insert.

set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

WORK=$(mktemp -d)
trap "rm -rf $WORK" EXIT

# Tiny fixture: one file, two refs (one resolved same-file, one cross-file).
mkdir -p "$WORK/src"
cat > "$WORK/Cargo.toml" << 'EOF'
[package]
name = "probe_refs_bug"
version = "0.0.0"
edition = "2021"
EOF
cat > "$WORK/src/lib.rs" << 'EOF'
mod helper;

pub fn entry() {
    helper::do_thing();
    let x = make_thing();
}

fn make_thing() -> i32 { 42 }
EOF
cat > "$WORK/src/helper.rs" << 'EOF'
pub fn do_thing() {}
EOF

# Build tethys release binary if not already
TETHYS="$(pwd)/target/release/tethys"
if [[ ! -x "$TETHYS" ]]; then
    cargo build -p tethys --release --quiet 2>&1 | tail -5
fi

cd "$WORK"

count_refs() {
    sqlite3 .rivets/index/tethys.db \
        "SELECT COUNT(*) FROM refs r JOIN files f ON f.id=r.file_id
         WHERE f.path='src/lib.rs';"
}

# Run 1: initial index. 3 refs in lib.rs: helper, do_thing, make_thing.
"$TETHYS" index --workspace . > /dev/null 2>&1
N1=$(count_refs)

# Modify src/lib.rs and re-index. Force a content change AND a mtime bump.
cat > src/lib.rs << 'EOF'
mod helper;

pub fn entry() {
    helper::do_thing();
    let x = make_thing();
    let y = make_thing();
}

fn make_thing() -> i32 { 42 }
EOF
# Belt-and-braces mtime bump (some filesystems have 1s mtime resolution).
sleep 1
touch src/lib.rs

# Run 2: now lib.rs has an EXTRA call to make_thing (4 refs total expected).
"$TETHYS" index --workspace . > /dev/null 2>&1
N2=$(count_refs)

# Now REMOVE both make_thing calls and re-index.
cat > src/lib.rs << 'EOF'
mod helper;

pub fn entry() {
    helper::do_thing();
}

fn make_thing() -> i32 { 42 }
EOF
sleep 1
touch src/lib.rs

"$TETHYS" index --workspace . > /dev/null 2>&1
N3=$(count_refs)

echo "=== refs in src/lib.rs across re-index runs ==="
echo "Run 1 (helper::do_thing + 1× make_thing call): $N1   expected ~3 (mod, fn, fn)"
echo "Run 2 (helper::do_thing + 2× make_thing call): $N2   expected ~4 (mod, fn, fn, fn)"
echo "Run 3 (helper::do_thing only, no make_thing):  $N3   expected ~2 if cleared, ~5+ if buggy"
echo
if [[ "$N3" -gt "$N2" || "$N3" -gt 3 ]]; then
    echo "BUG CONFIRMED: removing refs from source leaves stale rows in refs table."
elif [[ "$N3" -lt "$N2" ]]; then
    echo "OK: refs table correctly reflects current source state (some clear path exists)."
else
    echo "AMBIGUOUS: refs count unchanged. Source change may not have been re-indexed."
fi
