Summary

  The documentation covers 11 major sections:

  1. Protocol Fundamentals - CRDTs, convergence guarantees, actor model
  2. Data Model & Types - Maps, Lists, Text, Scalars, Counters
  3. Conflict Resolution - Automatic merging vs true conflicts, detection patterns
  4. Sync Protocol - Manual and streaming sync methods
  5. Rust Library API - Core types, document lifecycle, all operations with code
  6. Best Practices - Document structure, field type selection, save strategies
  7. Anti-Patterns & Gotchas - 5 anti-patterns, 5 gotchas with fixes
  8. Performance Characteristics - Automerge 3.0 improvements, complexity, guidelines
  9. Type Mapping for Rivets - Complete field-by-field recommendation table
  10. Implementation Examples - Storage backend, CRUD operations, sync code
  11. References - Official docs, crate links, papers, talks

  Key highlights that should inform the rivets-automerge implementation:
  - Use AutoCommit (not Automerge) for simpler API
  - Use Text type for title/description/notes (character-level merge)
  - Use Scalar for status/priority (LWW is correct)
  - Use List for labels/dependencies (all additions preserved)
  - Thread-safe with Arc<RwLock<AutoCommit>>
  - Never use Counters for auto-increment IDs (use UUIDs)