# Rivets Issue Tracking

Rivets coordinates concerns, work, and decisions within a Workspace by tracking their lifecycle and relationships.

## Language

### Core concepts

**Workspace**:
The boundary that owns a collection of Issues and their shared identity namespace. A Workspace may coincide with a source repository but does not require one.
_Avoid_: Project, repository (unless specifically discussing source control)

**Issue**:
An enduring record of something worth tracking. It may represent work, a request, a decision, or a grouping of other Issues.
_Avoid_: Ticket, work item (too narrow); task except when naming the Task kind

### Issue kinds

**Issue Kind**:
The single current classification of what an Issue represents: Bug, Feature, Task, Epic, or Chore. Kind follows present understanding and may change when the Issue is reclassified.
_Avoid_: Issue type, category

**Bug**:
An Issue recording observed behavior that violates intended behavior.
_Avoid_: Feature request

**Feature**:
An Issue recording a desired addition or change to intended capability.
_Avoid_: Bug, Chore

**Task**:
An Issue recording actionable work that is not itself a defect, capability change, maintenance outcome, or grouping.
_Avoid_: Issue as a general synonym

**Epic**:
An Issue that owns child Issues to group and decompose a larger concern. An Epic is not directly actionable.
_Avoid_: Parent task, milestone

**Chore**:
An Issue recording maintenance that preserves intended behavior.
_Avoid_: Feature, Task when the work is specifically maintenance

### Issue record

**Issue ID**:
A stable identifier unique within a Workspace and retained for the lifetime of an Issue.
_Avoid_: Ticket number, issue number

**Note**:
An immutable, timestamped entry that records a finding, justification, or other context as an Issue evolves. Notes form an append-only history; adding a Note never rewrites an earlier Note.
_Avoid_: Comment (conversational), mutable notes field, log entry

**Description**:
The current account of what an Issue concerns and why it matters. It may be refined as understanding changes; history belongs in Notes.
_Avoid_: Note, Design, immutable original report

**Design**:
The current intended approach for resolving an Issue. It explains how the concern will be addressed, distinct from the what and why in the Description.
_Avoid_: Description, Note

**Acceptance Criteria**:
Observable conditions under which an Issue can be considered successfully completed. They need not be satisfied when an Issue is Closed for another outcome.
_Avoid_: Closure reason, Description

**Associated Resource**:
A typed reference from an Issue to relevant information or an artifact. Each entry identifies a target and role, may provide a human-readable label, and belongs to a mutable curated index with no effect on workflow or readiness.
_Avoid_: External Reference, associated URL, attachment, historical log

**Resource Role**:
The reason an Associated Resource matters to its Issue. Implementation delivers work; Documentation explains; Evidence supports a finding or decision; Successor continues the concern elsewhere; Reference is the generic fallback.
_Avoid_: Resource type, URL kind, provider

**Resource Target**:
The location of an Associated Resource, represented explicitly as either a Web URL or a Workspace Path. A Web URL is absolute; a Workspace Path is relative to its Workspace root and cannot escape that boundary.
_Avoid_: Untyped locator, absolute file path, file URL

### Work coordination

**Workflow State**:
Open means not yet started, In Progress means actively being worked, and Closed means no further work is currently planned regardless of outcome. Workflow State does not encode whether dependencies prevent work.
_Avoid_: Status, Blocked status

**Blocked**:
A condition in which unresolved blocking relationships prevent work on an Issue. Blockedness ends when those relationships are resolved.
_Avoid_: Blocked status, manually blocked

**Priority**:
An Issue's scheduling urgency relative to other Issues, ranked from P0 (most urgent) through P4 (least urgent). Priority influences ordering but does not determine readiness.
_Avoid_: Severity, impact, Workflow State

**Assignee**:
The sole person or agent responsible for an Issue's next action. Assignment claims a Ready Issue from others, may precede In Progress, and does not itself change Workflow State.
_Avoid_: Owner, collaborator, current worker

**Label**:
A Workspace-defined classification applied to an Issue for grouping and filtering. Rivets gives a Label no intrinsic lifecycle or dependency meaning, though integrations may interpret agreed label conventions.
_Avoid_: Issue Kind, Workflow State

**Ready**:
A condition of an Open Issue that is not Blocked. Assignment does not change readiness; an assigned Open Issue remains Ready for its assignee.
_Avoid_: Unfinished, active

### Relationships

**Issue Relationship**:
A typed connection between two Issues. Each relationship kind has its own direction and effect; only a Blocking Dependency can prevent an Issue from being Ready.
_Avoid_: Dependency as an umbrella term, link

**Blocking Dependency**:
A directed Issue Relationship from a dependent Issue to a prerequisite Issue. The dependent is Blocked until the prerequisite is Closed.
_Avoid_: “A blocks B” when A is the dependent; say “A depends on B” or “A is blocked by B”


**Parentage**:
A directed Issue Relationship in which an Epic owns a child Issue for grouping and decomposition. A child has at most one parent, only Epics may be parents, Parentage does not affect readiness, and an Epic cannot close before all its children are Closed.
_Avoid_: Dependency, membership

**Related Association**:
A symmetric, non-blocking Issue Relationship indicating useful context between two Issues. Neither Issue owns or depends on the other.
_Avoid_: Dependency, duplicate

**Discovery Origin**:
A directed, non-blocking Issue Relationship from an Issue to the Issue whose work surfaced it. It records provenance, not causation or dependency.
_Avoid_: Cause, Blocking Dependency

