# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**Use `rivets ready` to see available work, `rivets list` to see all issues.**

## Project Overview

Rivets is a Rust implementation of the Beads project tracking system - a CLI tool for AI-native issue tracking with dependency graphs, stored as JSONL files alongside code.

## Work Tracking

This project uses rivets for issue tracking (dogfooding our own tool). Issues are stored in `.rivets/issues.jsonl`. The `.rivets/` directory is checked into git (issues travel with the repo).

### Quick Reference

```bash
rivets ready              # Show issues ready to work on (no blockers)
rivets list               # List all open issues
rivets list --status closed --limit 10  # Recent closed issues
rivets show <id>          # Full issue details with dependencies
rivets stats              # Project statistics
rivets blocked            # Show issues blocked by dependencies
```

### Working on Issues

```bash
# 1. Find work
rivets ready --limit 5

# 2. View issue details
rivets show rivets-xyz

# 3. Update status when starting
rivets update rivets-xyz --status in_progress

# 4. Close when done
rivets close rivets-xyz --reason "Implemented in commit abc123"
```

### Creating Issues

```bash
# Basic issue
rivets create --title "Fix the bug" --type bug --priority 2

# Full issue with design notes
rivets create \
  --title "Add new feature" \
  --type feature \
  --priority 2 \
  --description "Detailed description here" \
  --design "Implementation approach" \
  --acceptance "- [ ] Criterion 1\n- [ ] Criterion 2"
```

### Managing Dependencies

```bash
# Add dependency (issue-a blocks issue-b)
rivets dep add issue-a issue-b --type blocks

# View dependency tree
rivets dep tree issue-id

# See what's blocking an issue
rivets show issue-id  # Shows dependencies section
```

## Development Commands

```bash
cargo build              # Build all crates
cargo test               # Run all tests (~1400 tests)
cargo clippy             # Lint (pedantic mode enabled)
cargo fmt --check        # Check formatting
cargo fmt                # Auto-format
cargo run -- <subcommand>  # Run rivets CLI
```

### Workspace Lints

- `unsafe_code = "forbid"` - No unsafe code anywhere
- `clippy::pedantic = "warn"` - Pedantic lints enabled workspace-wide

### Crate-specific

```bash
cargo test -p rivets-jsonl   # JSONL library tests only
cargo test -p rivets         # Core + CLI tests only
cargo test -p tethys         # Code intelligence tests only
cargo test -p rivets-mcp     # MCP server tests only
```

## Commit Message Convention

This project uses [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).

### Format

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

### Types

- `feat`: New feature (MINOR version bump)
- `fix`: Bug fix (PATCH version bump)
- `docs`: Documentation changes
- `style`: Code style (formatting, semicolons, etc.)
- `refactor`: Code refactoring
- `perf`: Performance improvements
- `test`: Adding or updating tests
- `build`: Build system or dependencies
- `ci`: CI/CD configuration
- `chore`: Maintenance tasks

### Suggested Scopes (Optional)

Scopes must be lowercase. Common scopes:

- `cli`: CLI commands and interface
- `storage`: Storage layer and persistence
- `mcp`: MCP server functionality
- `jsonl`: JSONL library
- `tethys`: Code intelligence engine

### Breaking Changes

Use `!` after type or add `BREAKING CHANGE:` footer for MAJOR version bumps.

### Examples

```
feat(cli): add export command
fix(storage): handle empty files gracefully
docs: update installation instructions
feat!: redesign API (breaking change)
refactor(mcp): simplify tool registration
```

## Architecture

Cargo workspace with 4 crates:

| Crate | Type | Purpose |
|-------|------|---------|
| `rivets` | bin + lib | Core issue tracker: domain model, storage, CLI, commands |
| `rivets-jsonl` | lib | JSONL file format library (atomic writes, streaming reads, queries) |
| `rivets-mcp` | bin + lib | MCP server exposing rivets tools to AI assistants |
| `tethys` | bin + lib | Code intelligence engine (symbol graphs, LSP integration, dependency analysis) |

### Key directories in `rivets` crate

- `src/cli/` - Clap command definitions
- `src/commands/` - Command implementations
- `src/domain/` - Core domain types (Issue, Priority, Status, etc.)
- `src/storage/` - Persistence layer (JSONL-backed)
- `src/output/` - CLI output formatting

## Extended Guidelines (from GitHub Awesome Copilot)

### Self-Explanatory Code Commenting

**Core Principle**: Write code that speaks for itself. Comment only when necessary to explain WHY, not WHAT.

**Comments to Avoid**:

- **Obvious Comments**: Don't state what the code clearly shows ("Initialize counter to zero", "Increment counter by one")
- **Redundant Comments**: Avoid repeating the code's meaning in prose form
- **Outdated Comments**: Never let documentation drift from actual implementation

**Comments Worth Writing**:

- **Complex Business Logic**: Clarify non-obvious calculations or domain-specific rules
- **Algorithm Choices**: Explain why you selected a particular algorithm
  - Example: "Using Floyd-Warshall for all-pairs shortest paths because we need distances between all nodes"
- **Regex Patterns**: Describe what complex regular expressions match in plain language
- **API Constraints**: Document external limitations
  - Example: "GitHub API rate limit: 5000 requests/hour for authenticated users"

**Decision Framework** (before commenting):

1. Is the code self-explanatory?
2. Would better naming eliminate the need?
3. Does this explain WHY, not WHAT?
4. Will future maintainers benefit?

**Special Cases**:

- **Public APIs**: Use structured documentation (rustdoc `///`, JSDoc `/**`)
- **Constants**: Explain reasoning ("Based on network reliability studies")
- **Annotations**: Use standard markers: TODO, FIXME, HACK, NOTE, WARNING, PERF, SECURITY, BUG, REFACTOR, DEPRECATED

**Anti-Patterns**:

- Don't comment out code; use version control instead
- Never maintain change history in comments
- Avoid decorative divider lines

---

### Rust - Extended Guidelines (GitHub Awesome Copilot)

**Overview**: Follow idiomatic Rust practices based on The Rust Book, Rust API Guidelines, RFC 430, and community standards.

**General Instructions**:

- Prioritize readability, safety, and maintainability throughout
- Leverage strong typing and Rust's ownership system for memory safety
- Decompose complex functions into smaller, manageable units
- Include explanations for algorithm-related code
- Handle errors gracefully using `Result<T, E>` with meaningful messages
- Document external dependencies and their purposes
- Follow RFC 430 naming conventions consistently
- Ensure code compiles without warnings

**Ownership, Borrowing, and Lifetimes**:

- Prefer borrowing (`&T`) over cloning unless ownership transfer is necessary
- Use `&mut T` when modifying borrowed data
- Explicitly annotate lifetimes when the compiler cannot infer them
- Use `Rc<T>` for single-threaded reference counting; `Arc<T>` for thread-safe scenarios
- Use `RefCell<T>` for interior mutability in single-threaded contexts; `Mutex<T>` or `RwLock<T>` for multi-threaded

**Patterns to Follow**:

- Use modules (`mod`) and public interfaces (`pub`) for encapsulation
- Handle errors properly with `?`, `match`, or `if let`
- Employ `serde` for serialization and `thiserror`/`anyhow` for custom errors
- Implement traits to abstract services or dependencies
- Structure async code using `async/await` with `tokio` or `async-std`
- Prefer enums over flags for type safety
- Use builders for complex object creation
- Separate binary and library code for testability
- Use `rayon` for data parallelism
- Prefer iterators over index-based loops
- Use `&str` instead of `String` for function parameters when ownership isn't needed
- Favor borrowing and zero-copy operations

### Rust Best Practices Skill

**For detailed Rust patterns and code review, load the `rust-best-practices` skill.**

This skill provides 28 rules covering:
- Error handling (Option/Result patterns, expect vs unwrap)
- File I/O safety (atomic writes, TOCTOU avoidance)
- Type safety (enums, newtypes, validation)
- Performance (loop optimization, zero-copy limits)
- CLI development (clap, exit codes, signal handling, config files)
- Common footguns (borrow checker, Path edge cases)

**When reviewing Rust code**, apply these patterns in addition to project-specific rules above.

**Deep-dive files** (load when encountering specific issues):
- `error-handling.md` - Option patterns, Path footguns
- `file-io.md` - Atomic writes, tempfile testing
- `type-safety.md` - Constants, enums, newtypes
- `performance.md` - Loop optimization
- `cli-development.md` - clap, signals, config
- `common-footguns.md` - TOCTOU, borrow checker

### Testing with rstest

This project uses [rstest](https://docs.rs/rstest) for parameterized testing and [proptest](https://docs.rs/proptest) for property-based testing where exhaustive case enumeration isn't practical. Use rstest when you have multiple test cases that share the same test logic.

**When to use rstest:**

- Multiple similar tests that only differ in input/expected values
- Testing the same behavior with different configurations
- Verifying boundary conditions across multiple values

**Key features:**

- `#[rstest]` - Marks a test as parameterized
- `#[case(...)]` - Defines individual test cases with named variants
- `#[values(...)]` - Creates matrix tests across multiple values
- `#[fixture]` - Defines reusable test fixtures

**Example - Using `#[case]` for discrete test cases:**

```rust
use rstest::rstest;

#[rstest]
#[case::simple("hello", 5)]
#[case::empty("", 0)]
#[case::unicode("日本語", 3)]
#[test]
fn test_char_count(#[case] input: &str, #[case] expected: usize) {
    assert_eq!(input.chars().count(), expected);
}
```

**Example - Using `#[values]` for value ranges:**

```rust
use rstest::rstest;

#[rstest]
fn test_valid_priority(#[values(1, 2, 3, 4, 5)] priority: u8) {
    assert!(priority >= 1 && priority <= 5);
}
```

**Guidelines:**

- Name cases descriptively with `#[case::name(...)]` for clear test output
- Prefer `#[case]` when test cases have different expected behaviors
- Prefer `#[values]` when testing the same assertion across a range
- Don't force rstest on tests that don't benefit from parameterization
- Works with `#[tokio::test]` for async tests (place `#[rstest]` before `#[tokio::test]`)

### Test Design Patterns

Beyond parameterization with rstest, follow these patterns for robust test coverage:

**Roundtrip Tests (Parse/Serialize Symmetry)**

When a type has both serialization (`as_str()`, `to_string()`) and deserialization (`parse()`, `from_str()`), verify they're inverses:

```rust
#[test]
fn reference_kind_roundtrip() {
    let variants = [
        ReferenceKind::Import,
        ReferenceKind::Call,
        ReferenceKind::Type,
    ];
    for kind in variants {
        assert_eq!(
            ReferenceKind::parse(kind.as_str()),
            Some(kind),
            "roundtrip failed for {kind:?}"
        );
    }
}
```

**Invariant Tests (Contract Verification)**

When documentation or API design promises invariants, write tests that verify them:

```rust
#[test]
fn file_count_equals_language_sum() {
    // Invariant: file_count == sum(files_by_language) + skipped_unknown
    let stats = tethys.get_stats().expect("get_stats failed");
    let language_sum: usize = stats.files_by_language.values().sum();
    assert_eq!(
        stats.file_count,
        language_sum + stats.skipped_unknown_languages,
        "file_count should equal sum of language counts + skipped"
    );
}
```

**Descriptive Assertions in Tests**

Use `.expect("descriptive message")` instead of `.unwrap()` in tests for clearer failure output:

```rust
// Good - failure message explains context
let result = parser.parse(input).expect("parser should handle valid input");
let file = tethys.get_file_by_path(&path).expect("file should exist after indexing");

// Avoid - failures give no context
let result = parser.parse(input).unwrap();
```

**Test Categories to Consider:**

- **Roundtrip**: `parse(serialize(x)) == x` for all serializable types
- **Invariant**: Document promises hold under all conditions
- **Boundary**: Edge cases (empty, max, special characters)
- **Error paths**: Invalid inputs return appropriate errors
- **State transitions**: Operations produce expected state changes

### Structured Logging with tracing

This project uses the [`tracing`](https://docs.rs/tracing) crate for structured logging. Always use structured fields instead of string interpolation.

**Do this (structured):**

```rust
tracing::warn!(
    error = %reload_err,
    issue_id = %id,
    "Failed to reload after save error"
);

tracing::info!(
    workspace = %path.display(),
    issue_count = count,
    "Loaded workspace"
);

tracing::debug!(
    operation = "update",
    issue_id = %id,
    fields_changed = ?changed_fields,
    "Issue updated"
);
```

**Don't do this (string interpolation):**

```rust
// BAD - loses structured data
tracing::warn!("Failed to reload after save error: {}", reload_err);
tracing::info!("Loaded {} issues from {}", count, path.display());
```

**Field formatting:**

- `%value` - Use `Display` trait (for user-friendly output)
- `?value` - Use `Debug` trait (for developer debugging)
- `value` - Use directly if it implements `tracing::Value`

**Log levels:**

- `error!` - Unrecoverable errors, operation failed
- `warn!` - Recoverable issues, degraded operation
- `info!` - Significant events (startup, shutdown, major operations)
- `debug!` - Detailed diagnostic information
- `trace!` - Very verbose, step-by-step execution

**When to use logging vs user output:**

- Use `tracing::*` for **internal diagnostics** (debugging, monitoring, troubleshooting)
- Use `println!/eprintln!` for **user-facing output** (command results, error messages shown to users)

## ⚠️ CRITICAL: Before Making ANY Code Changes

**MANDATORY**: Always consult project guidelines before:

- Writing any code
- Making any modifications
- Implementing any features
- Creating any tests

Key guidelines to follow:

- Required Test-Driven Development workflow
- Documentation standards
- Code quality requirements
- Step-by-step implementation process
- Verification checklists

**SPECIAL ATTENTION**: If working as part of a multi-agent team:

1. You MUST follow parallel development workflows
2. You MUST create branches and show ALL command outputs
3. You MUST run verification scripts and show their output
4. You MUST create progress tracking files

**NEVER** proceed with implementation without following established guidelines.
