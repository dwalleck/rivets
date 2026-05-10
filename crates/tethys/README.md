# tethys

Code intelligence from the command line.

Tethys indexes your source code using [tree-sitter](https://tree-sitter.github.io/) and provides fast queries for symbols, references, call graphs, and dependency analysis. It's designed for CI integration and local development workflows.

## Installation

```bash
cargo install tethys
```

## Quick Start

```bash
# Index the current workspace
tethys index

# Search for symbols
tethys search AuthService

# Find all callers of a function
tethys callers "AuthService::authenticate"

# Analyze impact of changes to a file
tethys impact src/auth/mod.rs

# Find tests affected by changed files (great for CI)
tethys affected-tests src/auth/login.rs src/auth/session.rs --names-only

# View per-crate coupling metrics
tethys coupling

# Sort alphabetically
tethys coupling --sort name

# Drill into one package
tethys coupling --package my-crate

# JSON for tooling
tethys coupling --json
```

## Commands

| Command | Description |
|---------|-------------|
| `affected-tests` | Find tests affected by file changes |
| `callers` | Show callers of a symbol (with `--transitive` for call chains) |
| `coupling` | Per-crate coupling metrics (Ca, Ce, instability) |
| `cycles` | Detect circular dependencies |
| `impact` | Analyze impact of changes to a file or symbol |
| `index` | Index source files in the workspace |
| `panic-points` | Find `.unwrap()` and `.expect()` calls |
| `reachable` | Analyze symbol reachability (forward/backward traversal) |
| `search` | Search for symbols by name |
| `stats` | Show index statistics |

## Language Support

- Rust
- C#

## LSP Integration

For enhanced reference resolution, tethys can integrate with language servers:

```bash
# Index with rust-analyzer support
tethys index --lsp

# Use LSP for caller analysis
tethys callers "MyStruct::method" --lsp
```

## CI Integration

The `affected-tests` command outputs test names suitable for filtering test runs:

```bash
# Get affected test names for changed files
TESTS=$(tethys affected-tests $(git diff --name-only main) --names-only)

# Run only affected tests
cargo test $TESTS
```

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE) or [MIT license](../../LICENSE-MIT) at your option.
