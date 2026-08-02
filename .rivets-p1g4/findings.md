# Prove-it-prototype findings (rivets-p1g4)

Probe: `.rivets-p1g4/probe.py` (runs against `target/debug/rivets` built from
current `main` and against this repository as the workspace root).

## Smallest question

What normalization rule should Workspace Path targets use, and does the
existing persistence substrate keep resource identity and ordering stable
across removal/update and process restart?

## Oracle

- Section A: coreutils `realpath -m` (uutils 0.8.0) — purely lexical
  canonicalization, no filesystem access, independent of the probe's
  component-stack implementation.
- Section B: raw `.rivets/issues.jsonl` parsed with the Python stdlib
  (not the rivets binary); CLI output compared item-by-item to the file.
- Section C: hand-count of the edited JSONL (`r1`,`r3` remain; next id is
  `max+1 = r4`).

## Result: probe and oracle agree (20/20 normalization cases; CLI == file on
ids, order, and target shape; ids/order stable after simulated removal and
update; sequence continues at `r4`).

## What I learned (not obvious before the probe ran)

1. **Normalization and validation policy are separate claims.**
   `realpath -m` accepts whitespace-only (`'   '`) and control-character
   (`'un\tdir/x'`) paths as legal filenames — it cannot express the domain's
   rejection of those. The rejection is policy with clear precedent in this
   codebase (`WebUrl` trims leading C0/space; `ResourceLabel`, `NoteContent`,
   `ResourceId` reject control characters). The design must keep
   "accepted inputs normalize exactly like `realpath -m`" and
   "policy rejects X" as distinct claims with distinct falsifiers.

2. **The persistence substrate already guarantees id stability and sequence
   monotonicity across removal.** `Issue::rehydrate_resources` preserves
   persisted ids verbatim and sets `next_resource_id` to `max+1` of the
   loaded sequence. Removing a middle resource and reloading keeps `r1`,`r3`
   in order, and the next add gets `r4` — never a reused id. The domain
   remove/update operations inherit this for free.

3. **In-bounds parent traversal must normalize, not reject.** `docs/../src`
   normalizes to `src` (realpath agrees). This matters for duplicate
   detection: `docs/../src` and `src` with the same role must be the same
   target. Only traversal that *escapes* the root (`../x`, `a/../../b`) is
   rejected, and `a/..` / `.` (normalizing to the root itself) is rejected
   as empty-after-normalization.

4. **JSONL resources array order is the insertion order and survives
   restart exactly** — the CLI and a fresh process reload both match the
   raw file byte-for-byte on ids and targets (`{"type":"web","url":...}`).

## Implications for the design

- `WorkspacePath` newtype: lexical normalization implemented with a
  component stack; rejects absolute, escape, empty-after-normalization,
  control chars, whitespace-only. Equality on the normalized form.
- Duplicate detection: `ResourceTarget` equality derives from normalized
  `WorkspacePath`/`WebUrl` — exact target-role duplicates invalid, distinct
  roles on the same target allowed (already the case for Web).
- Remove/update: `Issue` methods keyed by `ResourceId`, positional
  preservation comes from `Vec` splice; no reidentification because ids
  are persisted, not positional.
