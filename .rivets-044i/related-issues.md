# Related issues (5-min tracker scan)

Scan performed 2026-05-18 via `rivets list` against keywords "qualified",
"resolve", "import-less", "dn35", "ycaq".

## Direct ancestors / context

- **rivets-dn35** (closed PR #69): removed the `imports.is_empty()`
  short-circuit in `resolve_refs_for_file`. 044i was filed at dn35's close as
  the residual gap. The `pass2_no_imports.rs` regression fence covers the
  unqualified-fallback shape; the qualified path is what 044i extends.
- **rivets-ycaq** (parent): resolver-correctness containment epic. 044i is
  the named follow-up coverage gap.
- **rivets-0gom** (closed): un-ambiguation drift. The risk callout in 044i's
  design notes flags this — any new resolution path could re-introduce
  phantom resolutions analogous to 0gom. Mitigation: K-hybrid file_deps
  filter (PR #67, rivets-3d0s).
- **rivets-3d0s** (closed): K-hybrid file_deps phantom-rate floor. AC #3
  requires it stay at 0.00% — we already have a probe at
  `.rivets-ycaq/probe_phantom_rate.py` (per memory note).

## Adjacent / not blocking

- **rivets-6aoc / rivets-34tv**: hardcoded `src/` join bugs in resolver
  paths. Not blocking 044i (different file paths) but worth noting in case
  the fix touches `compute_dependencies*`.
- **rivets-714v**: `--lsp` multi-crate path bug. 044i is Pass-2 only; LSP
  pass is separate.

## Conclusion

No prior art for the qualified-path fix itself. 044i is the canonical issue.
