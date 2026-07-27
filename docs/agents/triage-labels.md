# Triage Labels

The skills speak in terms of five canonical triage roles. This file maps those roles to the actual label strings used in this repo's issue tracker.

| Label in mattpocock/skills | Label in our tracker | Meaning |
| --- | --- | --- |
| `needs-triage` | `needs-triage` | Maintainer needs to evaluate this issue |
| `needs-info` | `needs-info` | Waiting on reporter for more information |
| `ready-for-agent` | `ready-for-agent` | Fully specified, ready for an AFK agent |
| `ready-for-human` | `ready-for-human` | Requires human implementation |
| `wontfix` | `wontfix` | Will not be actioned |

When a skill mentions a role, use the corresponding tracker label from this table.

## Local extensions

These have no mattpocock/skills counterpart. Skills that speak the five canonical
roles will never emit them, so apply them *alongside* a canonical label rather
than instead of one.

| Label | Meaning |
| --- | --- |
| `needs-grilling` | Blocked on a design decision with known options; resolve with a `/grilling` session, not by picking one silently |

The `needs-*` / `ready-for-*` prefixes are a blocked/unblocked axis, orthogonal to
who acts: `needs-*` means something must happen before work can start,
`ready-for-*` means someone can pick it up now. Keep that split when adding labels.
