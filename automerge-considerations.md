# Automerge Considerations for Rivets

**Status**: Recommendation  
**Date**: 2025-01-20  
**Context**: Storage backend selection for issue tracking

## Summary

Automerge is a powerful CRDT library, but it solves a different problem than what Rivets primarily needs. For git-based async collaboration between developers, simpler solutions provide better tradeoffs.

## What Automerge Does Well

Automerge excels at **real-time concurrent access**:

- Multiple processes editing the same document simultaneously
- Automatic conflict resolution without user intervention
- Offline-first with seamless sync when reconnected
- Sub-second merge latency between live peers

These capabilities matter for:
- Collaborative text editors (Google Docs-style)
- Real-time multiplayer applications
- Live dashboards with multiple writers
- Distributed databases without coordination

## Why Automerge May Be Overkill for Rivets

### 1. The Actual Access Pattern

Rivets' primary use case is **async collaboration via git**:

- Developer A creates/edits issues on their machine
- Developer B does the same on their machine
- Changes merge when branches merge in git
- Concurrent edits to the *same issue* are rare

This is fundamentally different from real-time concurrent access. Git already handles the transport and merge orchestration—we just need a storage format that merges cleanly.

### 2. Binary Format vs Git Diff/Merge

Automerge uses a compact binary format. While efficient for sync, this creates friction with git:

| Aspect | Automerge Binary | Text-based Formats |
|--------|------------------|-------------------|
| `git diff` | Opaque, shows binary changed | Line-by-line changes visible |
| `git merge` | Requires custom merge driver | Standard 3-way merge works |
| `git blame` | Not possible | Works per-line |
| Code review | Cannot review changes in PR | Full visibility |
| Manual editing | Requires tooling | Any text editor works |

A custom git merge driver can make Automerge files mergeable, but reviewers still can't see *what* changed in a PR without additional tooling.

### 3. Complexity Budget

Automerge introduces:

- New dependency (~500KB WASM or native binary)
- CRDT semantics to understand (last-writer-wins, list CRDTs, text CRDTs)
- Sync protocol if real-time features are added later
- Custom merge driver configuration for all contributors
- Potential for "spooky" merges that surprise users

For a project where concurrent same-document edits are rare, this complexity doesn't pay for itself.

### 4. CRDT Merge Semantics Can Surprise Users

Consider concurrent title edits:

```
Agent A: "Fix bug" → "Fix critical bug"
Agent B: "Fix bug" → "Resolve bug"
```

A text CRDT might produce: `"Fix criticalResolve bug"` (character-by-character merge)

For document bodies this is usually fine. For short fields like titles, it produces garbage. You'd need to configure per-field merge strategies, adding complexity.

## When Automerge Would Be Right for Rivets

Consider Automerge if requirements change to include:

1. **Live sync between running agents**: Multiple MCP servers editing issues simultaneously with sub-second sync
2. **Real-time team dashboard**: Browser-based UI where multiple team members see instant updates
3. **Mobile/offline client**: Native app that syncs when connectivity returns
4. **P2P collaboration**: Direct device-to-device sync without central git server

If these become requirements, Automerge is well-suited. The current design doc (`automerge-storage.md`) provides a reasonable starting point.

## Recommended Alternative: File-Per-Issue

For git-based async collaboration, a simpler pattern works better:

```
.rivets/
├── config.yaml
└── issues/
    ├── rivets-a3f8.rivet
    ├── rivets-b2c9.rivet
    └── rivets-x9k2.rivet
```

### Why This Works

| Scenario | Behavior |
|----------|----------|
| Dev A adds issue, Dev B adds issue | Different files, always merges cleanly |
| Dev A edits issue X, Dev B edits issue Y | Different files, always merges cleanly |
| Dev A edits issue X, Dev B edits issue X | Single file conflict, easy to resolve |
| Dev A deletes issue X, Dev B edits issue X | Git shows modify/delete conflict clearly |

### Additional Benefits

- **Atomic history**: `git log -- .rivets/issues/rivets-a3f8.rivet` shows one issue's history
- **Partial operations**: Can cherry-pick or revert individual issues
- **Sparse checkout**: Large projects can clone subset of issues
- **No special tooling**: Works with standard git, any text editor
- **Reviewable PRs**: Changes visible in diff view

### Format Options

1. **JSON**: Zero parsing effort via serde, but multiline strings are ugly in diffs
2. **YAML**: Better multiline handling, still widely supported
3. **Custom DSL (.rivet)**: Optimized for the domain, best diffs, requires parser

The `.rivet` format (see `rivets-format` crate) is designed specifically for git-friendly diffs while remaining human-readable.

## Migration Path

If real-time requirements emerge later:

1. Keep file-per-issue as the git-committed format
2. Add Automerge as a **runtime sync layer** between live processes
3. Serialize back to `.rivet` files for git commits
4. This gives both git reviewability AND real-time sync

This hybrid approach avoids committing binary files while enabling real-time features.

## Decision Framework

Use this checklist when evaluating storage approaches:

```
□ Do multiple processes need to edit the SAME document simultaneously?
  → Yes: Consider Automerge
  → No: File-per-issue is simpler

□ Is sub-second sync latency required?
  → Yes: Consider Automerge
  → No: Git push/pull is sufficient

□ Must changes be reviewable in GitHub/GitLab PRs?
  → Yes: Use text-based format
  → No: Binary formats acceptable

□ Do contributors need to edit issues without special tooling?
  → Yes: Use text-based format
  → No: Binary formats acceptable

□ Is the primary collaboration model git branches + PRs?
  → Yes: Optimize for git merge behavior
  → No: Optimize for real-time sync
```

## Conclusion

Automerge is excellent technology solving real problems. For Rivets' current requirements—git-based async collaboration with occasional merges—it adds complexity without proportional benefit. 

The file-per-issue pattern with a human-readable format (JSON, YAML, or `.rivet`) provides:
- Simpler implementation
- Better git integration
- Full PR reviewability
- No special tooling requirements

Reserve Automerge for if/when real-time sync becomes a requirement, and consider a hybrid approach that keeps text files as the git-committed source of truth.

## References

- [Automerge](https://automerge.org/) - CRDT library
- [Local-first software](https://www.inkandswitch.com/local-first/) - Ink & Switch
- [Bruno API Client](https://www.usebruno.com/) - Example of file-per-request pattern
- [CRDTs: The Hard Parts](https://www.youtube.com/watch?v=x7drE24geUw) - Martin Kleppmann
