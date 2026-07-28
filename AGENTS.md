# AGENTS.md - AI Assistant Documentation for Rivets

> This file provides context for AI assistants working with the Rivets codebase. Start here when helping with development tasks.

## Project Overview

**Rivets** is a fast, Git-friendly issue tracker built in Rust. It stores issues as JSONL files alongside your code, enabling seamless version control integration without external services.

### Key Facts

- **Language**: Rust 2021 Edition
- **Minimum Rust Version**: 1.70+
- **License**: MIT OR Apache-2.0
- **Repository**: github.com/dwalleck/rivets
- **Total LOC**: ~52,000 across 4 crates

### Crate Structure

| Crate | Purpose | Lines |
|-------|---------|-------|
| `rivets` | CLI and core issue tracking | ~7,000 |
| `rivets-jsonl` | JSONL library | ~4,000 |
| `rivets-mcp` | MCP server for AI assistants | ~3,000 |
| `tethys` | Code intelligence cache | ~38,000 |

## Quick Reference

### Starting Agent Work

1. Run `rivets ready --type task --label ready-for-agent`. The Task filter matters: published specification Features also carry `ready-for-agent` but are context, not executable slices.
2. Choose only an Open, unblocked, unassigned Task from that frontier. Never start an arbitrary story copied from a specification; native blocking relationships on the published Tasks define safe execution order.
3. Run `rivets show <task-id>`, then read its **Parent** specification, `CONTEXT.md`, and the ADRs named by the specification before editing code.
4. Claim before work as the first write: set the Assignee and move the Task to In Progress. Recheck the Assignee immediately before claiming.
5. Current Assignment updates are not cross-process atomic until **Claim and release Ready Issues atomically** (`rivets-8rj9`) lands. Do not race two agents for the same Task; coordinate claims externally in the meantime.
6. Close the Task only after its acceptance criteria and specified behavioral seams pass. Closing a blocker automatically advances the next Tasks into the Ready frontier.

Treat `rivets ready --type task --label ready-for-agent` as the sole frontier source of truth; never hardcode a current frontier in documentation because closing blockers changes it.

### Common Tasks

| Task | Location | Key Types/Functions |
|------|----------|---------------------|
| Add CLI command | `crates/rivets/src/cli/` | `Commands` enum, `execute_*` functions |
| Add storage backend | `crates/rivets/src/storage/` | `IssueStorage` trait |
| Add MCP tool | `crates/rivets-mcp/src/tools.rs` | `Tools` impl |
| Add domain type | `crates/rivets/src/domain/` | `Issue`, `IssueStatus`, etc. |
| Modify JSONL handling | `crates/rivets-jsonl/src/` | `JsonlReader`, `JsonlWriter` |

### File Layout

```
/home/dwalleck/repos/rivets/
├── crates/
│   ├── rivets/                      # Main CLI app
│   │   ├── src/
│   │   │   ├── cli/                 # Argument parsing (clap)
│   │   │   ├── commands/            # Command implementations
│   │   │   ├── domain/              # Issue, IssueStatus, etc.
│   │   │   ├── storage/             # Storage trait + backends
│   │   │   ├── app.rs               # App config
│   │   │   ├── lib.rs               # Public API
│   │   │   └── main.rs              # Binary entrypoint
│   │   └── tests/                   # Integration tests
│   ├── rivets-jsonl/                # JSONL library
│   │   └── src/
│   │       ├── reader.rs            # Async JSONL reading
│   │       ├── writer.rs            # Async JSONL writing
│   │       ├── atomic.rs            # Atomic file operations
│   │       └── warning.rs           # Warning collection
│   ├── rivets-mcp/                  # MCP server
│   │   └── src/
│   │       ├── tools.rs             # MCP tool implementations
│   │       ├── server.rs            # MCP protocol
│   │       ├── context.rs           # Workspace context
│   │       └── models.rs            # MCP types
│   └── tethys/                      # Code intelligence
│       └── src/
│           ├── lib.rs               # Main Tethys interface
│           ├── db/                  # SQLite database
│           ├── languages/           # Rust + C# support
│           └── lsp/                 # LSP integration
├── .agents/summary/                 # Detailed documentation
│   ├── index.md                     # Start here for docs
│   ├── architecture.md              # System design
│   ├── components.md                # Module details
│   ├── interfaces.md                # APIs and CLI
│   ├── data_models.md               # Type definitions
│   ├── workflows.md                 # Process flows
│   ├── dependencies.md              # External deps
│   └── review_notes.md              # Quality notes
├── Cargo.toml                       # Workspace manifest
└── README.md                        # User-facing overview
```

## Development Patterns

### Adding a New CLI Command

1. **Add command variant** to `Commands` enum in `crates/rivets/src/cli/mod.rs`:
   ```rust
   #[derive(Subcommand, Debug, Clone)]
   enum Commands {
       /// Command description
       MyCommand(MyCommandArgs),
   }
   ```

2. **Create args struct** in `crates/rivets/src/cli/args.rs`:
   ```rust
   #[derive(Parser, Debug)]
   pub struct MyCommandArgs {
       // Add arguments here
   }
   ```

3. **Implement handler** in `crates/rivets/src/cli/execute.rs`:
   ```rust
   pub async fn execute_my_command(app: &mut App, args: &MyCommandArgs) -> Result<()> {
       // Implementation
   }
   ```

4. **Wire up dispatch** in `Cli::execute()` in `crates/rivets/src/cli/mod.rs`:
   ```rust
   Some(Commands::MyCommand(args)) => execute_my_command(app, args).await,
   ```

### Adding a Storage Backend

1. **Create implementation** in `crates/rivets/src/storage/`:
   ```rust
   #[async_trait]
   impl IssueStorage for MyBackend {
       async fn create(&mut self, issue: NewIssue) -> Result<Issue> { ... }
       async fn get(&self, id: &IssueId) -> Result<Option<Issue>> { ... }
       // ... implement all trait methods
   }
   ```

2. **Update factory** in `create_storage()` in `crates/rivets/src/storage/mod.rs`:
   ```rust
   match backend {
       StorageBackend::MyBackend(config) => {
           let inner = MyBackend::new(config)?;
           Ok(Box::new(inner))
       }
       // ... other backends
   }
   ```

### Adding an MCP Tool

1. **Add tool signature** in `crates/rivets-mcp/src/tools.rs`:
   ```rust
   #[tool]
   async fn my_tool(param: ParamType) -> Result<ResponseType> {
       // Implementation
   }
   ```

2. **Register in router** in `crates/rivets-mcp/src/server.rs`:
   ```rust
   impl RivetsMcpServer {
       fn router(&self) -> Router {
           Router::new()
               .method("my_tool", ToolHandler::new(Self::my_tool))
               // ... other tools
       }
   }
   ```

### Adding or Changing Domain Behavior

1. Read `CONTEXT.md` and the relevant ADRs before naming or shaping a domain concept.
2. Put invariants in domain types with private fields and fallible constructors. Do not add another public data bag plus a detached `validate()` function.
3. Parse CLI, MCP, and persisted inputs into domain values at their adapter seams. Core modules should not repeatedly interpret raw strings, paths, numbers, or JSON.
4. Expose intent-named transitions that preserve aggregate invariants; avoid generic setters for state, Assignment, relationships, Notes, and Associated Resources.
5. Test the behavior through the highest stable interface, then add narrow domain tests only for invariants that external seams cannot isolate clearly.

The detailed rules below are mandatory for new code and should guide refactors of touched legacy code.

## Rust Modeling and Module Design

These rules are adapted from the strongest shared guidance in the Tethys and Cyril Rust repositories. Optimize for a strong domain model, deep modules, and compile-time exclusion of invalid states.

### Make invalid states unrepresentable

- **Parse, do not merely validate.** Convert untrusted CLI, MCP, and JSONL data into domain values once at an adapter seam. A free `validate_x(&T)` function that callers can bypass usually signals a missing newtype or enum.
- **Use private fields and smart constructors.** If a value has an invariant, callers must not be able to construct it with a struct literal and validate later. Implement `TryFrom`, `FromStr`, or an intent-named fallible constructor that returns a typed error.
- **Use enums for mutually exclusive states.** Prefer one exhaustive enum over combinations of booleans or loosely coordinated `Option` fields. Put data needed by one state on that enum variant when practical.
- **Use newtypes for constrained primitives.** IDs, non-empty text, validated labels, web URLs, Workspace-relative paths, priorities, and similar concepts should not travel through the core as interchangeable `String`, `PathBuf`, or integer values.
- **Encode relationship roles in types.** Prefer explicit fields such as dependent/prerequisite, child/parent, and discovered/source over generic `from`, `to`, and stringly typed kind fields. Symmetric and directed relationships must not share an interface that makes direction ambiguous.
- **Make valid transitions the only mutation path.** Domain aggregates own state changes through methods such as append, claim, release, reclassify, attach, move, close, and reopen. Each operation checks all affected invariants atomically and returns a typed domain error on rejection.
- **Keep compatibility weakness at the edge.** Legacy or permissive persisted records may use optional and untyped fields, but they are adapter DTOs. Convert them once into the strong canonical model; never let migration shapes become the domain model.
- **Do not reach for typestate automatically.** Use typestate only when it materially shrinks the callable interface. A runtime enum with exhaustive transitions is often clearer for persisted workflow state.

Concrete Rivets examples:

- `WorkflowState` contains `Open`, `InProgress`, and `Closed`; dependency-derived Blocked is not another persisted variant.
- `ResourceTarget` distinguishes a validated Web URL from a validated Workspace Path.
- `BlockingDependency` names its dependent and prerequisite; callers never infer direction from positional arguments.
- `Note` construction assigns its timestamp and rejects empty content; existing Notes expose no mutation path.

### Exhaustive matching is a maintenance guardrail

- Match every domain, protocol, persistence, and error enum variant explicitly. Avoid `_ =>` catch-alls where adding a variant should force a conscious decision.
- When translating between adapter and domain enums, make the compiler identify every call site affected by a new variant.
- If unknown external values must be tolerated, isolate that fallback in the adapter's compatibility type and return a typed warning or error before domain conversion.

### Keep errors typed and causal

- Map external errors at the adapter seam. Serde, I/O, URL parsing, and MCP errors must not leak through a domain interface.
- Prefer dedicated error variants over `reason: String` sentinels whenever a caller may branch on the cause.
- Preserve source chains with `#[source]` or `#[from]`; flatten errors to display strings only at the outermost CLI or MCP response boundary.
- New production code must not introduce `.unwrap()` or `.expect()` for recoverable states. Use `?`, exhaustive `match`, or a typed invariant error. Tests may use `expect` when it improves failure diagnostics.

### Build deep modules at deliberate seams

- A **Module** earns its place by hiding substantial behavior behind a small **Interface**. The interface includes invariants, ordering, errors, and performance expectations—not only Rust signatures.
- CLI, MCP, and JSONL are **Adapters**. They translate at a **Seam** and delegate; domain decisions must not be reimplemented in each adapter.
- Put each formula, invariant, migration rule, and transition in one owning module. If deleting a module would merely spread the same logic across callers, the module has useful depth.
- The interface is the primary test surface. Prefer a small number of high behavioral seams over tests that reach through the interface into implementation details.
- Do not create a trait for hypothetical variation. One adapter is a concrete implementation; a trait seam becomes justified when a second real adapter exists or an existing interface already requires it.
- Enforce load-bearing seams with architectural tests when ordinary review could miss leakage. A seam test should forbid specific dependency directions or adapter knowledge, not inspect arbitrary source formatting.

### Keep documentation synchronized

- `CONTEXT.md` is the canonical domain glossary; ADRs record why load-bearing decisions were made. AGENTS.md records current engineering and navigation rules.
- Update these files in the same change that alters what they describe. A stale invariant is worse than no documentation because agents treat it as fact.
- If implementation intentionally lags an accepted domain decision, say so explicitly and link the governing ADR or Rivets Issue rather than documenting the legacy behavior as canonical.

## Key Concepts

`CONTEXT.md` is authoritative for domain meaning. The implementation is intentionally behind several accepted decisions; do not infer the canonical model from legacy type or command names. ADR-0001, ADR-0002, and ADR-0003 record the governing decisions, with implementation specified by **Evolve Issue records with Notes, Associated Resources, and mutable Kinds** (`rivets-wb0q`) and **Align Workflow State, readiness, relationships, and Assignment claims** (`rivets-5mlg`).

### Workflow and Readiness

- **Workflow State** is Open, In Progress, or Closed.
- **Blocked** is derived only from unresolved explicit Blocking Dependencies; it is not a Workflow State.
- **Ready** means Open and not Blocked. In Progress and Closed Issues are not Ready.
- **Assignment** is an exclusive claim on the next action. An assigned Open Issue remains Ready for its Assignee but is not available to others.

### Issue Relationships

| Relationship | Direction | Blocks Ready? |
|--------------|-----------|---------------|
| **Blocking Dependency** | dependent → prerequisite | Yes, while the prerequisite is not Closed |
| **Parentage** | child → one Epic parent | No |
| **Related Association** | symmetric | No |
| **Discovery Origin** | discovered Issue → source Issue | No |

Parentage never propagates blockedness. Related is one symmetric association, not two directed dependencies. The current generic `DependencyType` model, `dep` commands, and parent-blocking graph behavior are legacy implementation details governed by ADR-0002.

### Issue Record

- **Issue Kind** is the mutable classification Bug, Feature, Task, Epic, or Chore. “Issue Type” is legacy vocabulary.
- **Notes** are immutable timestamped entries in an append-only history. Legacy singular Note strings are accepted only by the JSONL compatibility loader and migrate to canonical Note arrays.
- **Associated Resources** are typed, mutable references to Web URLs or Workspace Paths; the current singular External Reference is legacy.

### Storage Adapters

- **In-memory**: Runtime representation for Issue and relationship operations.
- **JSONL**: Persistent Git-friendly representation and compatibility-loading seam.

### Ready Ordering

Hybrid, Priority, and Oldest policies order Issues only after the Ready predicate is satisfied. A sort policy never decides whether an Issue is Ready.

## Error Handling

Current code uses `thiserror` error enums with `anyhow` only at outer application boundaries. The example below is an illustrative legacy shape; new branchable failures require dedicated typed variants and preserved source chains under the rules above.

```rust
#[derive(Error, Debug)]
pub enum Error {
    #[error("Issue not found: {0}")]
    IssueNotFound(IssueId),
    #[error("Circular dependency detected")]
    CircularDependency,
    #[error("Storage error: {0}")]
    Storage(String),
}
```

### Common Errors

| Error | Cause | Resolution |
|-------|-------|------------|
| `IssueNotFound` | ID doesn't exist | Verify ID with `rivets list` |
| `CircularDependency` | Would create cycle | Restructure dependencies |
| `HasDependents` | Issue has blockers | Remove dependencies first |

## Testing

### Unit Tests

```bash
cargo test -p rivets           # Core crate
cargo test -p rivets-jsonl     # JSONL library
cargo test -p rivets-mcp       # MCP server
cargo test -p tethys           # Code intelligence
```

### Integration Tests

```bash
cargo test --test cli_tests        # CLI integration
cargo test --test in_memory_storage # Storage tests
```

### Test Utilities

```rust
// Mock storage for testing
use rivets::storage::MockStorage;

// In-memory storage for full functionality
use rivets::storage::{create_storage, StorageBackend};
```

## Building and Running

### Development Build

```bash
cargo build              # All crates
cargo build -p rivets    # Just rivets CLI
cargo build -p rivets-mcp # Just MCP server
```

### Release Build

```bash
cargo build --release
```

### Run CLI

```bash
cargo run -p rivets -- --help
./target/release/rivets --help
```

### Run MCP Server

```bash
cargo run -p rivets-mcp
```

## Code Quality

### Formatting

```bash
cargo fmt
cargo fmt --check
```

### Linting

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

### All Checks

```bash
cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test
```

## Useful Commands

### Find where a type is used

```bash
# Using ripgrep
rg "IssueStatus" --type rust
```

### Find all tests for a module

```bash
rg "#\[test\]" crates/rivets/src/domain/mod.rs
```

### Check test coverage

```bash
cargo tarpaulin --out Html
```

## Documentation

### Generate Documentation

```bash
cargo doc --no-deps --open
```

### This Documentation

The `.agents/summary/` directory contains detailed documentation:

- `index.md` - Start here, contains metadata for all files
- `architecture.md` - System design and patterns
- `components.md` - Module-level documentation
- `interfaces.md` - CLI, library, and MCP APIs
- `data_models.md` - Type definitions
- `workflows.md` - Process flows
- `dependencies.md` - External dependencies

## Contributing

1. Fork the repository
2. Create a feature branch: `git checkout -b feat/amazing-feature`
3. Make changes with tests
4. Run quality checks: `cargo fmt --check && cargo clippy ... && cargo test`
5. Submit a pull request

See [CONTRIBUTING.md](./CONTRIBUTING.md) for detailed guidelines.

## Additional Resources

- [README.md](./README.md) - User-facing overview
- [CONTRIBUTING.md](./CONTRIBUTING.md) - Development guidelines
- [Cargo.toml](./Cargo.toml) - Workspace configuration
- Individual crate `Cargo.toml` files for specific dependencies

---

**For AI Assistants**: When working on this codebase, start with `.agents/summary/index.md` for navigation guidance. The index.md file contains sufficient metadata to understand which documentation files contain relevant information for specific types of questions.
