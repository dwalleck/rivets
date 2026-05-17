# What I learned — post-lcb6 prove-it-prototype iteration

**Date:** 2026-05-17 (PR #65 merge commit `94226ce`)

## The one-sentence finding

The 52 residual phantom cross-crate edges remaining after PR #61's slices 1-3
of rivets-0gom are 100% in the rivets-3d0s class (unique-workspace-match
fallback for trait method names like `len`, `add`, `write`, `Serialize`);
they re-occur identically across runs with zero contribution from lcb6
staleness, which means rivets-0gom's original `tethys.Ce=0` acceptance
criterion cannot be hit without landing rivets-3d0s.

## What was surprising

Before running the probe, I assumed lcb6 staleness would account for some
of the observed phantoms in `after-review-fixes.txt` (e.g. maybe the 16
`tethys→rivets` edges included some stale rows from prior runs that had
re-routed since). The measurement shows that's **not the case**:

| pair                          | polluted DB | clean DB | delta |
|-------------------------------|-------------|----------|-------|
| tethys → rivets-jsonl         | 21          | 23       | +2    |
| tethys → rivets               | 16          | 15       | −1    |
| rivets → rivets-jsonl         | 15          | 15       |  0    |
| rivets-mcp → rivets           |  7          |  7       |  0    |
| rivets → tethys               |  5          |  5       |  0    |
| rivets-mcp → tethys           |  4          |  4       |  0    |
| rivets-jsonl → tethys         |  3          |  3       |  0    |
| rivets-mcp → rivets-jsonl     |  2          |  2       |  0    |

The ±2 deltas are within the noise of code growth (the rivets workspace
has had recent commits adding/removing references; the cross-crate
*structure* is unchanged).

By contrast, **intra-crate dropped substantially**:
- tethys intra: 272 → 201 (−71)
- total file_deps: 465 → 398 (−67)

So lcb6 was holding stale **intra-crate** edges (refs to since-changed
files within the same crate that resolver re-routing eventually moved
elsewhere), but cross-crate phantoms are **regenerated identically on
every index run** — confirming they're live resolver bugs, not stale data.

## What this means for the 0gom→3d0s handoff

- **rivets-0gom slices 1-3 (PR #61) are doing the work they were designed
  to do**: ambiguity violations (multiple cross-crate candidates) are 0.
  Same-crate preference works (intra-crate edges still drop further after
  slice 3 because some `name` collisions are now correctly refused).
- **rivets-0gom's original acceptance criteria** describe the **full**
  resolved state (Ce=0 for tethys and rivets-jsonl), which requires the
  3d0s fix too. The design.md scoped the work to multi-match cases only.
- The residual classifier (`diagnose_residual.py`) confirms: every single
  one of the 52 residual phantoms is in `workspace candidates: 1`
  territory. The textbook 3d0s signature.

## Open question for the user

Two reasonable readings of "rivets-0gom is done":

1. **Design-scope reading:** PR #61's slices 1-3 implemented the design
   that was approved. The remaining work is scoped under rivets-3d0s.
   Close 0gom now; update its acceptance criteria to reflect the
   slice-scope; pivot to 3d0s.

2. **Acceptance-criteria reading:** The original 0gom acceptance criteria
   demand Ce=0, which is not met. Keep 0gom open until 3d0s lands and
   then verify the combined fix achieves Ce=0.

Recommend reading #1 (project pattern: lcb6 closed cleanly when its
narrow fix shipped, residual concerns went to rivets-ml05/dhxo).
