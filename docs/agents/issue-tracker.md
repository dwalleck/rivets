# Issue tracker: Rivets

Issues and planning artifacts for this repo live in Rivets. The checked-in `.rivets/issues.jsonl` file is the source of truth. Use the connected Rivets MCP tools when available; otherwise use the `rivets` CLI. Never edit the JSONL store by hand.

## Conventions

- **Initialize MCP context**: call `set_context` with the repository root before other Rivets MCP operations.
- **Create an issue**: use `create`, or `rivets create --title "..." --kind <kind> --priority <0-4>`.
- **Read an issue**: use `show`, or `rivets show <issue-id>`.
- **List issues**: use `list`, or `rivets list`, with status, label, kind, priority, or assignee filters as needed.
- **Find unblocked work**: use `ready`, or `rivets ready`.
- **Update an issue**: use `update`, or `rivets update <issue-id>`. Prefer atomic label add/remove operations over replacing the full label set.
- **Record discussion or resolution**: Rivets has no authored comment history. Append dated entries to `notes`, preserving existing notes. Workflows that require reporter-reply detection need human interpretation.
- **Close or reopen**: use `close` / `reopen`, or the corresponding CLI commands, with a concise reason.
- **Add a dependency**: the first issue depends on the second. Use `dep`, or `rivets dep add <dependent> <dependency> --type <blocks|related|parent-child|discovered-from>`.
- **Remove a dependency or delete an issue**: use the CLI when the MCP surface does not expose the required mutation.

## When a skill says "publish to the issue tracker"

Create a Rivets issue.

## When a skill says "fetch the relevant ticket"

Load it with `show` so its description, notes, labels, dependencies, and dependents are available. Rivets has no canonical issue URL; use the title as the human-facing name and keep the issue id adjacent as the lookup key.

## Wayfinding operations

Used by `/wayfinder`. The map is one Rivets epic and its tickets are child issues.

- **Map**: create an `epic` labelled `wayfinder:map`. Store Destination, Notes, Decisions-so-far, Not-yet-specified, and Out-of-scope in its description.
- **Child ticket**: create a `task` labelled `wayfinder:<type>`, with its question in the description. Add a `parent-child` dependency from the ticket to the map: the child is the dependent; the map is the dependency.
- **Blocking**: add a `blocks` dependency from the blocked ticket to its blocker: the blocked ticket is the dependent; the blocker is the dependency.
- **Frontier query**: get unblocked issues with `ready`, then retain the map's open, unassigned children by inspecting parent-child relationships. Use the tracker result order because Rivets has no explicit sub-issue order.
- **Claim**: as the first write, set the ticket assignee to the driving developer. The assignee is the claim; status alone is not a claim.
- **Resolve**: append the answer to ticket notes, close the ticket with a concise reason, then append a one-line title-first context pointer with the issue id to the map's Decisions-so-far.
- **Concurrent edits**: reload the map and affected tickets immediately before updating them. Preserve unrelated labels, notes, description sections, and dependencies.
