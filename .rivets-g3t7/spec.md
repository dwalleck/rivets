# Spec: Canonical Label grammar

## Request (verbatim)
> claim and implement rivets-93t7

The requested identifier does not exist; the immediately preceding frontier answer named `rivets-g3t7`, **Enforce one canonical Label grammar across CLI MCP and storage**, which was claimed as the intended task.

## What this is

Rivets will represent Issue Labels with one canonical domain value and enforce its grammar at CLI, MCP, JSONL, and storage seams. Equivalent operations will accept and reject the same spellings while preserving existing idempotent mutation and deterministic listing behavior.

## Roles

- **CLI operator**: creates, filters, and labels Issues through command arguments and sees clap validation errors before storage access.
- **MCP client**: sends structured tool inputs and receives JSON-RPC `invalid_params` with the same domain error meaning as the CLI operator.
- **Workspace maintainer**: loads and saves Git-backed JSONL Issue records and needs canonical Label spellings preserved without silent corruption.

## Behavior

### Parse a canonical Label
- **Given**: an exact 1-50 byte spelling matching `[a-z0-9]+(?:[-_][a-z0-9]+)*`.
- **When**: a CLI, MCP, JSONL, or storage seam receives it as an Issue Label.
- **Then**: Rivets constructs one canonical Label whose displayed, JSON, and persisted spelling is byte-identical.

### Reject a noncanonical Label
- **Given**: an empty, overlength, uppercase, whitespace/control/Unicode-containing, invalid-endpoint, invalid-character, or adjacent same/mixed-separator spelling.
- **When**: a CLI or MCP create, compatibility update, list/ready filter, add, or remove input receives it.
- **Then**: the adapter rejects it before query or mutation with the same typed domain error meaning; MCP classifies it as JSON-RPC `invalid_params`.

### Use canonical Labels throughout Issue behavior
- **Given**: canonical Label values on an Issue or filter.
- **When**: an Issue is created, compatibility-updated, filtered, labeled, unlabelled, serialized, or listed.
- **Then**: no storage path reinterprets a raw Label string; Issue-level insertion order remains stable, duplicate add and absent remove are no-ops, remove deletes matching associations, and list-all is sorted and deduplicated.

### Load noncanonical persisted Labels
- **Given**: a JSONL Issue record containing a noncanonical Label.
- **When**: a Workspace is loaded.
- **Then**: Rivets emits a visible Issue-specific invalid-data warning, omits that Issue from the loaded store, and the existing partial-load guard refuses later writes; it never guesses a replacement spelling.

### Normalize this repository's known tracker data
- **Given**: the 15 known Issues carrying uppercase or dotted noncanonical Labels in this repository.
- **When**: this change updates the tracker data before enforcing strict JSONL parsing.
- **Then**: the reviewed replacements are explicit (`DRY` → `dry`, `M-DOCUMENTED-MAGIC` → `m-documented-magic`, `M-LOG-STRUCTURED` → `m-log-structured`, and `*.rs` → `*-rs`), every other Label on each Issue is preserved, and the repository contains zero noncanonical persisted Labels.

## Success criteria

- **Binary / structural**: `Label` has a private string field and every public construction path is fallible, checked by compiler-enforced caller migration and domain grammar tests.
- **Binary / structural**: CLI, MCP, JSONL, storage, create/update, filter, add, and remove paths carry `Label` rather than unconstrained Issue-Label strings, checked by affected crate compilation plus cross-adapter behavioral tests.
- **Binary / structural**: every invalid grammar class returns a typed `LabelError`, checked by a parameterized domain matrix and CLI/MCP equivalence fence.
- **Binary / structural**: canonical persisted Labels round-trip byte-identically, checked through the JSONL compatibility loader and a second save.
- **Binary / structural**: a noncanonical persisted Label yields an Issue-specific invalid-data warning and activates the partial-load write guard, checked by a resilient-loader integration test.
- **Binary / structural**: this repository has zero noncanonical persisted Labels after the explicit 15-Issue cleanup, checked by a JSONL label audit using the canonical regular expression.
- **Binary / structural**: duplicate add and absent remove do not change `updated_at`; list-all remains sorted/deduplicated, checked through storage and adapter integration tests.
- **Quantitative**: parser accepts at most 50 bytes and performs one linear scan plus one success allocation, measured by the 50-byte boundary fixture and implementation inspection.
- **Binary / structural**: `canonical-label-input`, Add Label, and Remove Label become conformant only while named behavioral evidence exists, checked by the parity registry contract and renderer check.

## Out of scope

This change does NOT include Resource Labels (human-readable Associated Resource metadata), removing Labels from general MCP Update (owned by `rivets-67d7`), changing CLI batch semantics, changing Issue-level Label insertion order, adding custom Label namespaces, or inventing a lossy automatic migration for arbitrary noncanonical persisted Labels.

## Related issues

- `rivets-g3t7`: owning canonical Label parity task.
- `rivets-67d7`: removes Label replacement from general Update after this canonical value lands; this change validates the temporary compatibility input but does not retain it permanently.
- `rivets-ragt`: prior canonical Issue ID parsing used by Label Add/Remove/List and adopted as adapter error/parity precedent.

## Decisions

| Question | Decision | Rationale | Implication |
|---|---|---|---|
| What is the canonical grammar? | 1-50 ASCII bytes matching `[a-z0-9]+(?:[-_][a-z0-9]+)*`. | `CONTEXT.md`, ADR-0006 registry rule, and owning ticket. | No trimming or case folding; mixed adjacent separators are rejected. |
| Are Resource Labels included? | No. | Resource Labels are human-readable metadata with a distinct domain type and grammar. | Only Issue Labels migrate to `Label`. |
| What happens for empty, null, or missing collections/filters? | Empty/missing collections remain empty; an absent optional filter means no Label filter; an explicitly empty Label is rejected. | Existing adapter defaults plus canonical grammar. | Defaults do not synthesize Labels. |
| What is the maximum supported Label size? | 50 ASCII bytes inclusive. | Canonical glossary. | 51 bytes is a typed error. |
| How do concurrent mutations behave? | Existing storage serialization/locking remains unchanged. | No new concurrency behavior is requested. | Label typing does not add a second mutation path. |
| How do partial CLI batches behave? | Existing per-Issue partial success remains loop-equivalent to repeated MCP calls. | ADR-0006 adapter mechanics. | One invalid Label is rejected before the batch; per-Issue storage failures remain partial. |
| Are add/remove retries idempotent? | Yes; duplicate add and absent remove remain no-ops without timestamp change. | Owning ticket and current storage contract. | Typed Label equality replaces string equality. |
| Do Closed Issues accept Label changes? | Preserve current behavior. | Workflow behavior is outside this grammar cutover. | No new status guard. |
| Permission, authentication, soft deletion, multi-tenancy, time-zone/DST, replication lag, cache invalidation | N/A — local Workspace storage has no auth/tenant/soft-delete/replica/cache/time behavior in this path. | These dimensions cannot affect Label grammar. | No additional behavior. |
| How are existing noncanonical persisted Labels handled? | Strict rejection plus explicit cleanup of this repository's 15 known records. | Requester selected **Strict + explicit cleanup**; guessed universal normalization is lossy and collision-prone. | JSONL conversion returns visible Issue-specific invalid data; this repository uses reviewed lowercase/dot-to-hyphen replacements before strict loading lands. |

## Approval

Requester approval (verbatim): "Approve spec"
Date: 2026-08-29
