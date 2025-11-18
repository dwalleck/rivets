# Rivets

[![CI](https://github.com/dwalleck/rivets/actions/workflows/ci.yml/badge.svg)](https://github.com/dwalleck/rivets/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/dwalleck/rivets/branch/main/graph/badge.svg)](https://codecov.io/gh/dwalleck/rivets)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)
[![Crates.io](https://img.shields.io/crates/v/rivets.svg)](https://crates.io/crates/rivets)

A high-performance, Rust-based issue tracking system using JSONL storage.

## Overview

Rivets is a modern issue tracking system written in Rust that provides fast, efficient project management capabilities. It uses JSONL (JSON Lines) format for data storage, making it human-readable, version-control friendly, and easily scriptable.

## Project Structure

This workspace contains two crates:

- **rivets-jsonl**: A general-purpose JSONL library for efficient reading, writing, streaming, and querying of JSON Lines data
- **rivets**: The CLI application for issue tracking built on top of rivets-jsonl

## Installation

```bash
cargo install rivets
```

## Development

### Building

```bash
cargo build
```

### Testing

```bash
cargo test
```

### Running

```bash
cargo run --package rivets -- --help
```

### Quality Gates

This project uses pre-commit hooks to maintain code quality. The following checks run automatically before each commit:

1. **Code Formatting** (`cargo fmt -- --check`): Ensures all code follows Rust formatting standards
2. **Linting** (`cargo clippy`): Catches common mistakes and enforces best practices
3. **Tests** (`cargo test`): Ensures all tests pass

To manually run all quality checks:

```bash
cargo fmt -- --check  # Check formatting
cargo clippy --all-targets --all-features -- -D warnings  # Run linter
cargo test  # Run tests
```

To fix formatting issues:

```bash
cargo fmt
```

**Note**: The pre-commit hook is automatically installed in `.git/hooks/pre-commit`. If you need to bypass it (not recommended), use `git commit --no-verify`.

## License

Licensed under either of:

- MIT license
- Apache License, Version 2.0

at your option.

## Contributing

Contributions are welcome! Please see our contribution guidelines for more information.
