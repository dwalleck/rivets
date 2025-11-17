# Rivets CLI

A Rust-based issue tracking system with JSONL storage.

## Overview

Rivets is a command-line issue tracking system that stores data in human-readable JSONL format. This makes it easy to version control your issues alongside your code and integrate with other tools.

## Installation

```bash
cargo install rivets
```

## Usage

### Initialize a repository

```bash
rivets init
```

### Create an issue

```bash
rivets create
```

### List issues

```bash
rivets list
```

### Show issue details

```bash
rivets show <issue-id>
```

### Update an issue

```bash
rivets update <issue-id>
```

## Features

- Fast, efficient Rust implementation
- Human-readable JSONL storage format
- Git-friendly (easy to version control)
- Simple, intuitive CLI interface
- Dependency tracking between issues

## License

Licensed under either of MIT or Apache-2.0 at your option.
