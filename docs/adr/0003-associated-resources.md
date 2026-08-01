# Model related material as Associated Resources

Rivets replaces the singular, untyped External Reference concept with a plural collection of typed Associated Resources. Each resource has an explicit Resource Target—either an absolute Web URL or a Workspace-relative Path—a standard Resource Role of Implementation, Documentation, Evidence, Successor, or Reference, and an optional human-readable label. We chose this model over URL-only and untyped-string alternatives because Issues need portable links to both remote and local context, while agents need to understand why each target matters without inferring meaning from its syntax or provider.

## Consequences

Associated Resources form a mutable curated index: they may be added, corrected, relabeled, or removed, while historically significant changes belong in Notes. Resource associations do not affect workflow or readiness. The current `external_ref: Option<String>` representation must migrate to a plural resource collection; legacy values that are neither valid Web URLs nor Workspace Paths require an explicit migration decision rather than silent reinterpretation.

As implemented (issue_record.rs), the migration is: a truly empty `external_ref` carries no context and is dropped; an absolute Web URL becomes a Reference resource; any other non-empty value is preserved visibly in a migration Note. Because Note content rejects control characters, when a value contains any terminal-unsafe control character it is escaped (Rust `escape_default`) in the Note, and the escape pass also escapes backslashes at the same time; a backslash-only value otherwise passes through unchanged. A value that loads in the future is never silently reinterpreted as a resource target.
