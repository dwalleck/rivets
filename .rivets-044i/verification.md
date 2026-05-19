# rivets-044i slice 3 verification

Post-fix measurements on the rivets workspace.

## Indexing wall time

| metric | pre-fix | post-fix | delta |
|---|---|---|---|
| files | 123 | 123 | – |
| symbols | 2647 | 2651 | +4 (new test file symbols) |
| references | 22190 | 22263 | +73 (new test file refs) |
| wall time | 23.25s | 20.81s | **−10%** |
| `time real` | 23.394s | 20.939s | **−10%** |

Budget: ≤ 1.5× baseline. **PASS** — post-fix is actually faster (within
noise). The new path is gated behind every existing fallback, so it
adds work only on the refs that would have stayed unresolved anyway.

## Phantom rate (rivets-3d0s regression fence, claim 7)

| metric | pre-fix | post-fix |
|---|---|---|
| cross-crate edges | 8 | 9 |
| corroborated | 8 | 9 |
| phantom edges | 0 | **0** |
| phantom rate | 0.00% | **0.00%** |
| FORBIDDEN-pair edges | 0 | 0 |

**PASS.** The one extra cross-crate edge is corroborated by an import,
consistent with the K-hybrid filter's behavior.

CI fence: `cargo nextest run -p tethys --test file_deps_corroboration`
all 2 tests pass.

## 0gom Section 3 ambiguity (claim 8)

| metric | pre-fix | post-fix |
|---|---|---|
| refs resolved across crates | 326 | 330 |
| ambiguity violations | 0 | **0** |

**PASS.** Four new cross-crate resolutions, zero new ambiguity violations.

CI fence: `cargo nextest run -p tethys --test resolver_routing` all 4 tests pass.

## Resolve coverage delta (informational)

| metric | pre-fix | post-fix | delta |
|---|---|---|---|
| total refs | 22190 | 22263 | +73 (new test refs added in slices 1+2) |
| resolved | 6756 | 6837 | **+81** |
| unresolved total | 15434 | 15426 | −8 |
| unresolved qualified | 1558 | 1493 | **−65** |

So qualified-ref resolution improved by 65 refs on the rivets workspace
itself (+ several refs in the new test files that also got resolved
through the new path). The +81 resolved count exceeds the −65
unresolved-qualified delta because the new test files added some refs
that resolved purely via Pass 1 (same-file).

## Final integration

All probes, falsifiers, and regression fences pass against the
post-fix binary:

- `pass2_qualified_paths.rs` (8 tests, all pass — claims 1, 2, 3, 4, 5 + 3 added by
  the round-1 review-decisions commit `cc6dd0c`: `longest_prefix_wins_over_shorter`
  pins the loop-direction invariant, `prefix_resolves_but_tail_missing_stays_unresolved`
  pins the tail-miss branch, `self_and_super_paths_resolve_via_as_written` pins the
  `matches!` gate for `self::*` / `super::*` paths)
- `pass2_no_imports.rs` (1 test, passes — claim 6)
- `file_deps_corroboration.rs` (2 tests, pass — claim 7)
- `resolver_routing.rs` (4 tests, pass — claim 8)
- Full `cargo nextest run -p tethys` (636 tests, all pass post round-1 additions)
- Phantom-rate probe (claim 7 oracle) — 0.00%
- 0gom Section 3 probe (claim 8 oracle) — 0 violations
- Indexing wall time (claim 1 budget) — −10% vs baseline
