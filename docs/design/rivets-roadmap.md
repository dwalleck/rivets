# Rivets Differentiation Roadmap

## Vision

Transform Rivets from a Beads port into a **code-aware, multi-interface issue tracker** that leverages Rust's unique ecosystem strengths.

**Tagline**: "Work tracking that understands your code"

## Core Differentiators

| Feature | Crate | Description |
|---------|-------|-------------|
| CRDT Storage | automerge | Multi-agent concurrent access |
| Code Intelligence | tree-sitter | TODO scanning, issue-code linking |
| Rich TUI | ratatui | Interactive Kanban, graphs |
| Git Integration | git2 | Commit-issue linking, branch awareness |
| Desktop App | tauri | Native GUI sharing core library |

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    rivets (core library)                │
│  - Domain models (Issue, Dependency, etc.)              │
│  - IssueStorage trait + implementations                 │
│  - Code intelligence engine (tree-sitter)               │
│  - Git integration (git2)                               │
├──────────┬──────────┬──────────────────┬────────────────┤
│   CLI    │   TUI    │   Tauri Desktop  │   MCP Server   │
│  clap    │ ratatui  │  tauri + dioxus  │     rmcp       │
│          │          │  or leptos       │                │
└──────────┴──────────┴──────────────────┴────────────────┘
```

## Implementation Phases

### Phase 1: Foundation Enhancements (Current Sprint)

#### 1.1 Automerge Storage Backend
**Status**: Design complete (docs/design/automerge-storage.md)
**Epic**: rivets-5vz

- New crate: `rivets-automerge`
- IssueDocument wrapper for AutoCommit
- AutomergeStorage implementing IssueStorage trait
- JSONL export mirroring for human readability
- Git merge driver for .automerge files

#### 1.2 Enhanced CLI with Fuzzy Search
**Crate**: nucleo

```rust
// Example: fuzzy issue lookup
rv show lgn  // matches "login-bug", "logging-feature", etc.
rv dep add --from $(rv fuzzy "auth") --to $(rv fuzzy "login")
```

- Add `nucleo` dependency for fuzzy matching
- Implement fuzzy ID completion in all commands
- Add `rv fuzzy <query>` subcommand for scripting

---

### Phase 2: Code Intelligence

#### 2.1 Tree-sitter Integration
**Crate**: tree-sitter, tree-sitter-{rust,python,typescript,...}

New crate: `rivets-code`

```rust
pub struct CodeScanner {
    parsers: HashMap<String, Parser>,  // Extension → parser
}

impl CodeScanner {
    /// Scan file for TODO/FIXME comments
    pub fn scan_todos(&self, path: &Path) -> Vec<TodoComment>;

    /// Find references to issue IDs in code
    pub fn find_issue_refs(&self, path: &Path, prefix: &str) -> Vec<IssueRef>;
}
```

Features:
- `rv scan` - Scan codebase for TODOs, create/link issues
- `rv affected <file>` - Find issues related to a file
- `rv orphans` - Detect issues referencing deleted code
- Auto-update issue locations when code moves

#### 2.2 Git Integration
**Crate**: git2

```rust
pub struct GitContext {
    repo: Repository,
}

impl GitContext {
    /// Get current branch name
    pub fn current_branch(&self) -> Option<String>;

    /// Find commits mentioning an issue ID
    pub fn commits_for_issue(&self, id: &IssueId) -> Vec<Commit>;

    /// Auto-link: scan recent commits for issue IDs
    pub fn auto_link_commits(&self, prefix: &str) -> Vec<(Commit, IssueId)>;
}
```

Features:
- `rv log <issue-id>` - Show commits related to issue
- `rv branch <issue-id>` - Create branch named after issue
- Commit message hooks suggesting issue references
- `rv pr <issue-id>` - Generate PR description from issue

---

### Phase 3: Rich TUI

#### 3.1 Interactive Terminal UI
**Crate**: ratatui, crossterm

**Access**: `rv tui` subcommand (integrated into main binary)

```
┌─ Rivets ─────────────────────────────────────────────┐
│ [Ready] [In Progress] [Blocked] [Closed]   Q:quit   │
├──────────────────────────────────────────────────────┤
│ ┌─ Ready (5) ────────┐ ┌─ In Progress (2) ─────────┐│
│ │ ● rivets-067  P1   │ │ ● rivets-5vz  P1         ││
│ │   Create crate...  │ │   Automerge epic...      ││
│ │ ● rivets-81c  P1   │ │                          ││
│ │   IssueDocument... │ └────────────────────────────┘│
│ │ ● rivets-rg8  P1   │ ┌─ Blocked (1) ─────────────┐│
│ │   AutomergeStor... │ │ ● rivets-xyz  P2         ││
│ └────────────────────┘ │   Blocked by: rivets-5vz ││
│                        └────────────────────────────┘│
├──────────────────────────────────────────────────────┤
│ [j/k] Navigate  [Enter] View  [c] Create  [/] Search│
└──────────────────────────────────────────────────────┘
```

Features:
- Kanban board view (configurable columns)
- Dependency graph visualization (ASCII)
- Vim-style navigation (j/k/g/G)
- Inline editing
- Real-time filtering

---

### Phase 4: Tauri Desktop App

#### 4.1 Desktop Application
**Crates**: tauri, dioxus

New crate: `rivets-desktop`

Architecture:
```
┌─────────────────────────────────────┐
│         Tauri Shell                 │
│  ┌───────────────────────────────┐  │
│  │      Web UI (Dioxus)          │  │
│  │   - Reactive components       │  │
│  │   - Tailwind CSS              │  │
│  └───────────────────────────────┘  │
│              ▲                      │
│              │ IPC (Tauri commands) │
│              ▼                      │
│  ┌───────────────────────────────┐  │
│  │      Rust Backend             │  │
│  │   - rivets core library       │  │
│  │   - Native file access        │  │
│  │   - Git operations            │  │
│  └───────────────────────────────┘  │
└─────────────────────────────────────┘
```

Tauri Commands (exposed to frontend):
```rust
#[tauri::command]
async fn list_issues(filter: IssueFilter) -> Result<Vec<Issue>, Error>;

#[tauri::command]
async fn create_issue(new_issue: NewIssue) -> Result<Issue, Error>;

#[tauri::command]
async fn get_dependency_graph() -> Result<GraphData, Error>;
```

Features:
- Native desktop app (Windows, macOS, Linux)
- Drag-and-drop Kanban board
- Interactive dependency graph (vis.js or d3)
- System tray integration
- Keyboard shortcuts matching TUI
- Auto-sync with file system

---

### Phase 5: Advanced Features

#### 5.1 Full-Text Search
**Crate**: tantivy

```rust
pub struct IssueIndex {
    index: tantivy::Index,
}

impl IssueIndex {
    /// Search issues by content
    pub fn search(&self, query: &str) -> Vec<(IssueId, f32)>;

    /// Reindex all issues
    pub fn reindex(&mut self, storage: &dyn IssueStorage);
}
```

- `rv search "authentication bug"` - Full-text search
- Ranked results by relevance
- Index stored alongside issues

#### 5.2 Semantic Search (Optional)
**Crates**: candle or ort (ONNX Runtime)

- Local embedding generation (no API calls)
- "Find issues similar to this one"
- Requires downloading a small model (~50MB)

---

## New Crate Structure

```
crates/
├── rivets/              # Core library + CLI
├── rivets-jsonl/        # JSONL utilities
├── rivets-mcp/          # MCP server
├── rivets-automerge/    # NEW: Automerge storage
├── rivets-code/         # NEW: Code intelligence
├── rivets-tui/          # NEW: Terminal UI
└── rivets-desktop/      # NEW: Tauri app
```

## Key Dependencies to Add

```toml
# Phase 1
automerge = "0.5"
nucleo = "0.5"

# Phase 2
tree-sitter = "0.24"
tree-sitter-rust = "0.23"
tree-sitter-python = "0.23"
tree-sitter-typescript = "0.23"
tree-sitter-go = "0.23"
tree-sitter-c-sharp = "0.23"
git2 = "0.19"

# Phase 3
ratatui = "0.29"
crossterm = "0.28"

# Phase 4
tauri = "2.0"
dioxus = "0.6"

# Phase 5 (optional)
tantivy = "0.22"
```

## Success Metrics

1. **Concurrent Access**: Multiple agents can work without conflicts
2. **Code Awareness**: TODOs auto-linked, dead issues detected
3. **UX Excellence**: <100ms response for all operations
4. **Multi-Platform**: CLI, TUI, Desktop all functional
5. **Beads Compatibility**: Can import/export Beads JSONL format

## Decisions Made

1. **Tauri Frontend**: Dioxus (React-like API, excellent docs, hot-reloading)

2. **Tree-sitter Languages**: Rust, Python, TypeScript/JavaScript, Go, C#

3. **TUI Integration**: Subcommand (`rv tui`) - simpler distribution, single binary

## Critical Files for Implementation

- `crates/rivets/src/storage/mod.rs` - Storage trait
- `crates/rivets/src/domain/mod.rs` - Domain models
- `crates/rivets/src/cli/` - CLI structure
- `docs/design/automerge-storage.md` - Automerge design
- `Cargo.toml` - Workspace dependencies

---

## Ecosystem Notes: Catalyst & Skills Synergies

### The Broader Vision

Rivets, Catalyst, and Claude-Skills-Supercharged form layers of an AI development platform:

| Layer | Project | Purpose |
|-------|---------|---------|
| Work Context | Rivets | WHAT you're working on (issues, deps, code links) |
| Context Flow | Catalyst | HOW context flows (hooks, file analysis, agents) |
| Knowledge | Skills | WHAT knowledge applies (domain guidance) |

### Identified Synergies

1. **Issue-Driven Skill Activation**
   - `rv start <issue>` exports context (labels, type, related files)
   - Catalyst hook reads context, adds to skill matching
   - Skills pre-load based on issue metadata, not just prompt

2. **Dev Docs ↔ Issues Bidirectional Sync**
   - Catalyst's 3-file pattern maps to Issue fields:
     - plan.md → issue.design
     - context.md → issue.notes
     - tasks.md → issue.acceptance_criteria
   - Rivets becomes persistent store for dev docs

3. **Agent Orchestration via Dependencies**
   - Issues can have `assigned_agent` field
   - Completion of blocking issues triggers agent dispatch
   - Agents update issue notes with findings

4. **Unified Desktop Cockpit**
   - Tauri app shows: issues + skills + agents + file activity
   - Single view of entire development context

5. **Shared CRDT Persistence**
   - Automerge could store: issues + session state + skill state
   - All conflict-free, all syncable between agents

### Integration Strategy: Loose Coupling First

Rather than deep integration, define protocols:

1. **WorkContext schema** - JSON Rivets exports for current issue
2. **Catalyst reads optionally** - If WorkContext exists, use it
3. **Skills reference labels** - Issue labels map to skill triggers
4. **Desktop aggregates** - Reads from all sources, doesn't require all

This approach:
- Ships Rivets independently now
- Adds integration points incrementally
- Validates synergies before coupling
- Keeps optionality open

### Future: Unified Platform?

If synergies prove valuable, could evolve into unified platform:
- Shared Automerge document for all state
- Single installation/configuration
- Marketed as "AI Development Platform"

Decision deferred until integration points validated.

---

## Deep Dive: Integration Protocols

### WorkContext Protocol Schema

```json
{
  "version": "1.0",
  "timestamp": "2025-11-30T12:00:00Z",
  "active_issue": {
    "id": "rivets-5vz",
    "title": "Automerge CRDT Storage",
    "type": "epic",
    "status": "in_progress",
    "priority": 1,
    "labels": ["rust", "database", "crdt"],
    "design_summary": "First 200 chars of design field...",
    "acceptance_criteria": ["Implements IssueStorage", "Multiple agents work", "..."]
  },
  "related_files": ["crates/rivets-automerge/src/lib.rs"],
  "blocking_issues": [],
  "suggested_skills": ["rust-best-practices", "database-patterns"]
}
```

- **Location**: `.rivets/work-context.json`
- **Written**: On `rv start <issue>` or `rv update --status in_progress`
- **Cleared**: On `rv close <issue>` or `rv stop`
- **Consumers**: Catalyst hooks, skill-activation-prompt

### Automerge Document Architecture

**Federated documents (recommended over monolithic):**

| Document | Contents | Lifecycle | Git-tracked |
|----------|----------|-----------|-------------|
| `platform.automerge` | Issues, dependencies, work_context | Persistent | Yes |
| `session.automerge` | Skills loaded, file tracking | Per-session | No (.gitignore) |

**Rationale**: Separates durable (issues) from transient (session), shared from user-specific.

### Tauri Desktop Data Flow

```
┌─────────────────────────────────────────────────────────────┐
│  rivets-desktop (Tauri + Dioxus)                            │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  DataAggregator (Rust backend)                      │   │
│  │  ├── issues      ← rivets crate (direct link)       │   │
│  │  ├── work_ctx    ← .rivets/work-context.json        │   │
│  │  ├── skills      ← .claude/skills/skill-rules.json  │   │
│  │  └── file_track  ← .claude/hooks/state/*            │   │
│  └─────────────────────────────────────────────────────┘   │
│                         ↓                                   │
│  ┌─────────────────────────────────────────────────────┐   │
│  │  UnifiedState (exposed via Tauri commands)          │   │
│  │  - get_unified_state() → full dashboard data        │   │
│  │  - start_issue(id) → activates work context         │   │
│  │  - activate_skill(name) → triggers skill load       │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

### JSONL Role in Unified Architecture

**Transition: Primary Storage → Export Format**

```
Phase 1 (Current):
  .rivets/issues.jsonl  ← PRIMARY storage

Phase 2 (Dual-write):
  .rivets/platform.automerge  ← PRIMARY (CRDT)
  .rivets/issues.jsonl        ← AUTO-EXPORT on save()

Phase 3+ (Stable):
  Automerge primary, JSONL always available for:
  - Human inspection (`cat issues.jsonl | jq`)
  - Git diff review
  - Beads import/export compatibility
  - Debugging
```

**JSONL is never removed** - it transforms from storage to view.
