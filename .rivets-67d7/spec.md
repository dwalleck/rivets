# Spec: Canonical general Update

## Request (verbatim)
> claim and implement rivets-67d7

## What this is
Restrict general Update to its canonical mutable fields. Intent-owned state moves through dedicated operations, while CLI batches retain per-target partial-success behavior.

## Roles
- CLI operator: changes Issues through terminal commands and receives batch successes/failures.
- MCP caller: changes one Issue per tool call and receives equivalent domain results/errors.
- Library caller: submits a validated nonempty update through shared storage.

## Behavior

### Update supplied fields
- Given: an existing Issue and a nonempty canonical update.
- When: a CLI operator or MCP caller submits that update.
- Then: supplied fields change under shared validation; omitted fields remain unchanged; successful mutation is persisted.

### Reject empty requests
- Given: an existing Issue and a request with no supplied mutable fields.
- When: either adapter attempts Update.
- Then: it fails without changing the Issue timestamp or persisted bytes.

### Preserve dedicated ownership
- Given: an Issue with Assignment, Labels, Notes, Workflow State, and relationships.
- When: general Update succeeds.
- Then: those intent-owned values remain unchanged; dedicated operations remain available to change them under their existing invariants.

### Preserve batch semantics
- Given: an ordered CLI batch containing successful and failing targets.
- When: the batch runs.
- Then: its per-target domain effects and errors equal repeated single-target MCP calls in order, without cross-target rollback.

## Success criteria
- Update accepts only the agreed field set: checked through real CLI parsing, registered MCP invocation, and storage behavior.
- Empty request rejection leaves timestamps and persisted bytes identical: checked by before/after snapshots with a valid-update positive control.
- Dedicated operations remain reachable and preserve lifecycle/Assignment invariants: checked through both adapters.
- Mixed batches match repeated single-target calls: checked against independently prepared expected Issue records.

## Out of scope
Permanent non-goals for this change: redesigning JSONL persistence, changing Workspace locking, inventing a new Priority scale, changing existing descriptive-text clearing behavior, or adding MCP batch tools. These would change unrelated contracts.
Full inventory-wide schema modernization is separate work in rivets-mmqc; inventory-wide parity harness is rivets-y4vw. This change still proves its own affected contracts.

## Related issues
Native tracker has no text-search option; `list --label interface-parity --limit 100`, `show` and local record search located relevant contracts.
- rivets-67d7: requested acceptance criteria; no Parent specification recorded.
- rivets-8rj9: implemented Claim/Release and Assignment side effects of lifecycle transitions.
- rivets-5mlg: governing Workflow State/Assignment specification, referenced by rivets-8rj9.
- rivets-mmqc: wider schema modernization; not a reason to leave changed Update inputs permissive.
- rivets-y4vw: wider shared parity suite; this change supplies focused behavioral evidence.
- rivets-ragt and rivets-g3t7: completed canonical Issue ID and Label prerequisites.

## Resolved field behavior

### Canonical field set
- **Given**: an existing Issue in any canonical Workflow State.
- **When**: a caller supplies title, description, Priority P0-P4, Kind, design, or acceptance criteria through general Update.
- **Then**: shared validation accepts valid supplied values and changes only those fields plus the update timestamp; excluded intent-owned fields cannot be supplied through general Update.

### Omission and explicit empty text
- **Given**: an existing Issue with descriptive text.
- **When**: a caller omits a field or passes null for an optional MCP field.
- **Then**: that field remains unchanged and does not count toward a nonempty request.
- **Given**: the same Issue.
- **When**: a caller supplies an empty description, design, or acceptance criteria.
- **Then**: it is an explicit text update under existing semantics; an empty title instead fails shared validation without mutation.

### Repeated supplied values
- **Given**: an existing Issue and a valid supplied value equal to its current field value.
- **When**: Update is called again with that value.
- **Then**: it remains a nonempty successful Update with existing timestamp behavior, not an empty-request rejection.

## Decisions

| Question | Decision | Rationale | Implication |
|---|---|---|---|
| Should Priority remain in general Update? | Retain Priority P0-P4 | Requester: "retain priority in general update". Priority is absent from the exhaustive intent-owned removal list; removing it would strand its only mutation path. | Allowed fields are title, description, Priority, Kind, design, and acceptance criteria |
| What does omitted text mean? | Preserve unchanged | Existing Update semantics | Missing/None is not a clear operation |
| What does explicitly empty descriptive text mean? | Preserve current acceptance of empty description/design/acceptance text | Existing storage semantics; changing clearing behavior is not requested | Do not conflate supplied empty text with absent update |
| Can title be empty? | No; retain shared title validation | Existing CLI/storage invariant | Rejection before mutation |
| May removal of update status/notes eliminate existing capabilities? | No | rivets-67d7 requires dedicated intents, not feature deletion | Design must provide separate lifecycle and CLI Append Note entry points |
| What happens to batch partial failures? | Preserve ordered per-target success/failure | Explicit rivets-67d7 acceptance and ADR-0006 | No all-or-nothing batch transaction |
| Concurrency and Workspace isolation? | Retain existing mutation lock and per-Workspace storage | Existing storage contract and ADR-0005 | No new concurrency or persistence mechanism |
| Can ordinary Update change Assignment? | No; retain Claim/Release plus existing lifecycle side effects | rivets-8rj9 and ADR-0005 | Close still clears Assignment; reopening is unassigned |
| Empty set / missing fields? | Reject requests with no supplied allowed fields before mutation; CLI requires at least one target | rivets-67d7 | No timestamp or persisted-byte changes on empty rejection |
| Null fields? | Preserve existing omitted-field semantics; null optional fields do not count as supplied changes | Existing Option-based adapter contract | A request containing only null optional fields is empty |
| Maximum scale? | Preserve existing supported Workspace and batch sizes; no new cap or parallel execution | No scale expansion requested | Design verifies existing whole-Workspace persistence and ordered batches without extra per-target scans |
| Permission denied / authentication? | Propagate existing filesystem/Workspace access errors; no new authentication model | Local Workspace adapters | An access failure must not be reported as mutation success |
| Retries and idempotency? | A supplied valid field is a nonempty update even if equal to its current value; preserve existing successful-update timestamp behavior | Existing Update contract | Empty rejection is not a change-detection feature |
| Closed or soft-deleted records? | Preserve descriptive updates to Closed Issues; soft deletion is N/A because no such record state exists | Canonical Workflow State | Do not add a lifecycle precondition to descriptive Update |
| Workspace tenancy? | Use the existing selected Workspace; never cross Workspace storage | Existing CLI/MCP context contract | IDs are resolved within the selected Workspace |
| Time zones / DST? | Preserve system UTC timestamps; no calendar arithmetic is introduced | Existing timestamp contract | Empty rejection compares the exact stored timestamp |
| Replication lag / cache invalidation? | No replication mechanism; preserve existing persistent-adapter reload and invalidation semantics | Existing local storage architecture | No new cache or synchronization policy |

## Specification sign-off
All scope questions are resolved and the complete specification is approved. Dedicated lifecycle/Note interface placement remains subject to design approval.

## Approval
Priority decision approved verbatim: "retain priority in general update"
Date: 2026-09-06
Requester approval (verbatim): "yes, I agree"
Date: 2026-09-06
