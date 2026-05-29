# Related issues — rivets-wsix

Tracker scan (5-min cap per prove-it-prototype step 0):

## Direct ancestors

- **rivets-lcb6** (closed, PR #65) — the *parent* fix. Discovered file_deps was UPSERT-only and never cleared, causing stale resolver edges to accumulate. Established the `clear_all_X` pattern and applied it to `file_deps`. The wsix audit asks: "what other tables have this shape?"
- **rivets-zoi3** (open, P2) — also surfaced during PR #65 review. Asks for *expanded test coverage* of the file_deps clear pattern (4 specific tests). Adjacent but different work; wsix is breadth-first (which tables), zoi3 is depth-first (one table, many fixtures).

## Sibling / related

- **rivets-dhxo** (open, P3) — orphan file_deps re-insertion in streaming-mode indexing. Different bug class (orphan file_ids producing phantom edges *from* deleted source files), but lives in the same "file_deps coherence" problem space. Not displaced by wsix.

## Closed but informative

- **rivets-itz7** (closed, P1) — added imports indexing for Rust + C#.
- **rivets-lxbg** (closed, P1) — added imports table to schema.

Both are about *creating* the imports table; neither audited its write pattern for the UPSERT-stale-row bug class. The imports table is a wsix candidate (it appears in my broader grep).

## No prior art found for

The wsix-specific question — "which UPSERT-only tables in `crates/tethys/src/db/` are missing a `clear_all_X` call from `index_with_options`" — has no existing issue. wsix is the right tracker entry; no displacement.
