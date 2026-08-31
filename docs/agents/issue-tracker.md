# Issue tracker: Rivets

Issues and planning artifacts for this repo live in Rivets. The checked-in `.rivets/issues.jsonl` file is the source of truth. Use the connected Rivets MCP tools when available; otherwise use the `rivets` CLI. Never edit the JSONL store by hand.

Before adding or changing a CLI command, MCP tool, argument, default, validation rule, ordering rule, error, or result shape, read the [CLI and MCP Interface Parity](../cli-mcp-parity.md) contract. Update its registry and rendered reference in the same change.

## Conventions

- **Initialize MCP context**: call `set_context` with the repository root before other Rivets MCP operations.
- **Create an issue**: use `create`, or `rivets create --title "..." --kind <kind> --priority <0-4>`.
- **Read an issue**: use `show`, or `rivets show <issue-id>`.
- **List issues**: use `list`, or `rivets list`, with status, label, kind, priority, or assignee filters as needed.
- **Find Ready work**: use `ready`, or `rivets ready`. Omitted Assignment selectors return only unassigned Open Issues without unresolved direct Blocking Dependencies; use `--assignee` or `--all-assignees` when that visibility is intentional.
- **Update an issue**: use `update`, or `rivets update <issue-id>`, for ordinary fields and Workflow State. Assignment changes use `claim` / `release`; labels use atomic add/remove operations.
- **Claim or release responsibility**: use MCP `claim` / `release`, or `rivets claim <issue-id> --assignee <name>` / `rivets release <issue-id> --assignee <name>`. Never emulate either intent with a read followed by a general update.
- **Record discussion or resolution**: Rivets has no authored comment history. Use MCP `add_note` or CLI `rivets update <issue-id> --notes "..."`; each operation appends one immutable, system-timestamped Note.
- **Close or reopen**: use `close` / `reopen`, or the corresponding CLI commands, with a concise reason.
- **Add a Blocking Dependency**: the dependent Issue depends on the prerequisite. Use MCP `blocking_dependency_add`, or `rivets blocking-dependency add --dependent <issue> --prerequisite <issue>`.
- **Inspect or remove Blocking Dependencies**: use the matching MCP `blocking_dependency_list` / `blocking_dependency_tree` / `blocking_dependency_remove` tools or CLI `blocking-dependency` subcommands. Never reverse dependent and prerequisite wording.

## When a skill says "publish to the issue tracker"

Create a Rivets issue.

## When a skill says "fetch the relevant ticket"

Load it with `show` so its description, notes, labels, dependencies, and dependents are available. Rivets has no canonical issue URL; use the title as the human-facing name and keep the issue id adjacent as the lookup key.

## Wayfinding operations

Used by `/wayfinder`. The map is one Rivets Epic; until canonical Parentage
lands in `rivets-qcje`, map membership is represented by `wayfinder:<type>`
labels rather than a relationship.

- **Map**: create an `epic` labelled `wayfinder:map`. Store Destination, Notes, Decisions-so-far, Not-yet-specified, and Out-of-scope in its description.
- **Child ticket**: create a `task` labelled `wayfinder:<type>`, with its question in the description. Do not create a legacy `parent-child` dependency; canonical set/clear/move/show Parentage is tracked at `rivets-qcje`.
- **Blocking**: use MCP `blocking_dependency_add`, or `rivets blocking-dependency add --dependent <blocked-ticket> --prerequisite <blocker>`.
- **Frontier query**: query `ready` with the map's `wayfinder:<type>` label. The default Assignment selector already restricts the result to unassigned Ready Issues. Use the tracker result order because Rivets has no explicit sub-issue order.
- **Claim**: as the first write, atomically claim the ticket for the driving developer. The Assignment is the claim; status alone is not a claim.
- **Resolve**: append the answer with `add_note`, close the ticket with a concise reason, then append a one-line title-first context pointer with the issue id to the map's Decisions-so-far.
- **Concurrent edits**: reload the map and affected tickets immediately before updating them. Preserve unrelated labels, notes, description sections, and dependencies.
