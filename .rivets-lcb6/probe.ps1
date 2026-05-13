# rivets-lcb6 probe: measure file_deps idempotency on the rivets workspace.
#
# CAVEAT: This probe runs `tethys index` (no --rebuild) on the rivets workspace
# itself with NO file changes between runs. In that scenario the indexer's
# mtime/size staleness check correctly short-circuits — `insert_file_dependency`
# is never invoked, so the UPSERT-accumulation bug doesn't manifest here.
#
# This probe therefore verifies the no-change path is idempotent (a property
# we don't want to regress). The actual bug — stale row PERSISTS after a file
# is modified to remove a `use` — needs a fixture where files DO change. That
# scenario lives in the regression test at crates/tethys/tests/file_deps_idempotency.rs.
#
# Why keep this probe: it's the simplest empirical check that the indexer
# pipeline doesn't accidentally trigger a re-insert on unchanged files. A
# post-fix regression that always wipes file_deps and then fails to repopulate
# from cached state (e.g., if rivets-bxom incremental landed wrong) would
# show up here as row count dropping to zero or something pathological.
#
# Run from repo root. Idempotent: does not modify workspace state.

$ErrorActionPreference = 'Stop'
$db = ".\.rivets\index\tethys.db"
$snapshot = ".\.rivets-lcb6\probe-snapshot.txt"

if (-not (Test-Path $db)) {
    Write-Host "DB not found at $db. Run 'tethys index' first."
    exit 1
}

function Get-FileDepsCount {
    [int]$count = & sqlite3 $db "SELECT COUNT(*) FROM file_deps;"
    return $count
}

$before = Get-FileDepsCount
Write-Host "file_deps rows before re-index: $before"

# Re-index without --rebuild (this is the buggy path)
& .\target\release\tethys.exe index | Out-Null

$after1 = Get-FileDepsCount
Write-Host "file_deps rows after 1st re-index: $after1"

& .\target\release\tethys.exe index | Out-Null

$after2 = Get-FileDepsCount
Write-Host "file_deps rows after 2nd re-index: $after2"

$delta1 = $after1 - $before
$delta2 = $after2 - $after1

"baseline = $before" | Out-File $snapshot
"after_reindex_1 = $after1 (delta $delta1)" | Out-File $snapshot -Append
"after_reindex_2 = $after2 (delta $delta2)" | Out-File $snapshot -Append

if ($delta1 -eq 0 -and $delta2 -eq 0) {
    Write-Host "PASS: file_deps is idempotent across runs (post-fix behavior)."
    exit 0
} else {
    Write-Host "FAIL: file_deps accumulates across runs (pre-fix behavior). delta1=$delta1 delta2=$delta2"
    exit 2
}
