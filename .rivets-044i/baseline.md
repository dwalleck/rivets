# rivets-044i pre-fix baselines

Captured on branch `feat/rivets-044i-qualified-paths` @ origin/main (no
code changes yet).

## Indexing wall time (single run, release build)
```
$ ./target/release/tethys.exe index
Indexed 123 files, found 2647 symbols, 22190 references
Duration: 23.25s
real    0m23.394s
```

## Phantom rate (rivets-3d0s regression fence)
- cross-crate edges: 8
- corroborated: 8
- phantom: 0
- phantom rate: 0.00%

## 0gom Section 3 ambiguity (claim 8 regression fence)
- refs resolved across crates: 326
- ambiguity violations: 0

## Resolve coverage (informational)
- total refs: 22190
- resolved: 6756 (30.45%)
- unresolved total: 15434
- unresolved qualified (contains `::`): 1558  ← upper bound on refs reachable by the new fallback
