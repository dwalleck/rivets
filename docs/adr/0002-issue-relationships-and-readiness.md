# Separate workflow, readiness, and issue relationships

Rivets separates an Issue's lifecycle from whether work can start: Workflow State is Open, In Progress, or Closed; Blocked is derived only from explicit Blocking Dependencies; and Ready means Open and unblocked. Issue Relationship is the umbrella for distinct semantics: a Blocking Dependency points from a dependent to its prerequisite, Parentage gives an Issue one Epic parent without propagating blockedness, Related Association is symmetric, and Discovery Origin is directed provenance. We chose explicit concepts over a generic dependency graph and a Blocked workflow state because the generic model conflated lifecycle, work eligibility, hierarchy, provenance, and association.

## Consequences

An Assignee exclusively claims the next action but may be assigned before work enters In Progress. An Epic cannot close while it has non-Closed children. The current implementation must be aligned: remove the Blocked workflow state, exclude In Progress Issues from Ready, stop propagating blockedness through Parentage, enforce one Epic parent, and expose Related Associations symmetrically.
