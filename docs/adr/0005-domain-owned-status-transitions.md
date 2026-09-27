# The domain owns Workflow State and Assignment transitions

Workflow State and Assignment invariants live in the domain and are applied
atomically by the storage seam. `IssueStatus::validate_transition` and
`Issue::apply_status_transition` own the status matrix and its Assignment side
effects; `IssueStorage::claim` and `IssueStorage::release` own Assignment
compare-and-set behavior. Throughout, `status` is the code-and-wire name
(`IssueStatus`, the `status` field) for the glossary's Workflow State
(`CONTEXT.md`).

This seam prevents adapter drift. CLI, MCP, and future adapters delegate to the
same mutation: claim requires an Open, unblocked Issue; repeated claim by the
same owner is an unchanged success; another owner receives a typed Already
Claimed error; release requires the exact owner and Open state but permits an
Open, blocked Issue. Entering In Progress requires an assignee, returning to
Open retains it, closing clears it, and reopening creates an unassigned Open
Issue.

`IssueStorage::transition` accepts a `LifecycleAction`: Start, ReturnToOpen,
Close, or Reopen. ReturnToOpen requires In Progress; Reopen requires Closed.
Storage checks Parentage and commits the state change and optional reason Note
under one lock. General Update accepts a validated, nonempty `IssueUpdate`
containing only title, description, Priority, Kind, design, and acceptance criteria.

Lifecycle transitions preserve Created and use one event timestamp for Updated,
entry into the current Closed period, and an optional reason Note. Closing sets
`closed_at` to the new event instant even after an earlier closure; reopening
clears it. Ordinary edits while Closed preserve the closure instant. Rejected
transitions leave state, timestamps and history unchanged. Timestamp semantics
are governed by the approved resolution on `rivets-c6ql`; the lifecycle slice is
implemented by `rivets-1nhb`, independently of the remaining date-filter work.

Existing recorded closures remain accepted legacy approximations and missing
closures remain unknown. Read-time compatibility loading does not backfill
history or rewrite stale closure values on non-Closed records; future lifecycle
transitions establish current-period semantics.

## Consequences

Adapters must not read an Issue and then implement Assignment with a generic
update. That read-then-write sequence is not atomic and allows two claimants to
overwrite one another. They call `claim` or `release`; persistent adapters hold
the Workspace mutation lock across load, compare-and-set, and atomic JSONL
save.

Every wrapping error preserves the typed domain cause. CLI and MCP may format
the error at their outer boundaries, but neither matches error strings nor
reimplements the transition matrix.

Persisted legacy records that violate the canonical matrix are repaired at the
JSONL compatibility seam. In Progress without an assignee migrates to Open;
Closed with an assignee clears Assignment. Each repair appends an immutable
migration Note and emits a load warning. Canonical records are unchanged on a
second load/save cycle.
