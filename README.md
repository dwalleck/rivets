# Rivets

[![CI](https://github.com/dwalleck/rivets/actions/workflows/ci.yml/badge.svg)](https://github.com/dwalleck/rivets/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/dwalleck/rivets/branch/main/graph/badge.svg)](https://codecov.io/gh/dwalleck/rivets)
[![Crates.io](https://img.shields.io/crates/v/rivets.svg)](https://crates.io/crates/rivets)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

A fast, Git-friendly issue tracker that lives in your repository.

Rivets stores issues as JSONL files alongside your code—no external services, no context switching, no sync problems. Track bugs, features, and tasks with the same workflow you use for code.

## Features

- **Git-native** — Issues live in your repo, branch with your code, merge with your PRs
- **Fast** — Built in Rust for instant responses, even with thousands of issues
- **Dependency tracking** — Model blockers and relationships between issues
- **AI-ready** — MCP server for seamless integration with AI coding assistants
- **Scriptable** — JSON output mode for automation and custom tooling
- **Human-readable** — JSONL storage you can grep, diff, and edit directly

## Installation

```bash
cargo install rivets
```

## Quick Start

```bash
# Initialize in your project
rivets init

# Create an issue
rivets create --title "Add user authentication" --type feature

# See what's ready to work on
rivets ready

# Start working on an issue
rivets update RIVETS-1 --status in_progress

# Mark it done
rivets close RIVETS-1
```

## Usage

### Managing Issues

```bash
rivets create --title "Fix login bug" --type bug --priority 1
rivets list                          # List all open issues
rivets list --status in_progress     # Filter by status
rivets show RIVETS-1                 # View issue details
rivets update RIVETS-1 --priority 2  # Update fields
rivets close RIVETS-1 --reason "Fixed in commit abc123"
```

### Dependencies

```bash
rivets dep RIVETS-2 --blocks RIVETS-1    # RIVETS-2 blocks RIVETS-1
rivets blocked                            # Show all blocked issues
rivets ready                              # Show issues with no blockers
```

### Labels

```bash
rivets label add RIVETS-1 urgent backend
rivets label remove RIVETS-1 urgent
rivets list --label backend
```

### JSON Output

All commands support `--json` for scripting:

```bash
rivets list --json | jq '.[] | select(.priority == 1)'
```

## Project Structure

This workspace contains three crates:

| Crate | Description |
|-------|-------------|
| `rivets` | CLI and core library |
| `rivets-jsonl` | General-purpose JSONL library |
| `rivets-mcp` | MCP server for AI assistant integration |

## Development

### Prerequisites

- Rust 1.70+

### Building and Testing

```bash
cargo build              # Build all crates
cargo test               # Run tests
cargo run -p rivets -- --help
```

### Code Quality

Pre-commit hooks enforce formatting, linting, and tests. Run manually with:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

### Commit Convention

This project uses [Conventional Commits](https://www.conventionalcommits.org/):

```
feat(cli): add export command
fix(storage): handle empty files gracefully
docs: update installation instructions
```

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feat/amazing-feature`)
3. Make your changes with tests
4. Ensure all quality checks pass
5. Submit a pull request

For maintainers, see [Publishing](#publishing) for release procedures.

<details>
<summary><h3>Publishing</h3></summary>

Publish crates in dependency order:

```bash
cargo publish -p rivets-jsonl
# Wait for indexing...
cargo publish -p rivets
# Wait for indexing...
cargo publish -p rivets-mcp
```

Generate changelog: `git cliff --unreleased --bump --prepend CHANGELOG.md`

</details>

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
