# Rivets

[![CI](https://github.com/dwalleck/rivets/actions/workflows/ci.yml/badge.svg)](https://github.com/dwalleck/rivets/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/dwalleck/rivets/branch/main/graph/badge.svg)](https://codecov.io/gh/dwalleck/rivets)
[![Crates.io](https://img.shields.io/crates/v/rivets.svg)](https://crates.io/crates/rivets)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

A fast, Git-friendly issue tracker that lives in your repository.

Rivets stores issues as JSONL files alongside your code—no external services, no context switching, no sync problems. Track bugs, features, and tasks with the same workflow you use for code.

## Features

- **Git-native** — Issues live in your repo, branch with your code, merge with your PRs
- **Fast** — Built in Rust with an in-memory query engine over Git-friendly persistence
- **Dependency tracking** — Model blockers and relationships between issues
- **Associated Resources** — Attach typed Web links and Workspace Paths with stable IDs and semantic roles
- **AI-ready** — MCP server for seamless integration with AI coding assistants
- **Scriptable** — JSON output mode for automation and custom tooling
- **Human-readable** — JSONL storage you can grep, diff, and edit directly

## Installation

```bash
cargo install rivets
```

## Quick Start

```bash
# Initialize in your project (issue IDs use this prefix)
rivets init --prefix demo

# Create an issue and capture its generated ID
ID=$(rivets create --title "Add user authentication" --kind feature | sed 's/^Created issue: //')

# See what's ready to work on
rivets ready

# Atomically claim it, then start active work
rivets claim "$ID" --assignee "$USER"
rivets update "$ID" --status in_progress

# Mark it done
rivets close "$ID"
```

## Command Overview

| Command | Purpose |
|---------|---------|
| `init` | Initialize a repository (`.rivets/` and `config.yaml`); `--prefix <name>` sets the ID prefix |
| `info` | Repository info: database path, prefix, and summary counts |
| `create` | Create an issue (`--title`, `--kind`, `--priority`, `--assignee`, `--labels`, repeatable `--prerequisite`, `--design`, `--acceptance`, `--notes`) |
| `list` | List issues; filter with `--status`, `--priority`, `--kind`, `--assignee`, `--label`; `--sort` and `--limit` |
| `show` | Show one or more issues with their Blocking prerequisites/dependents and resources |
| `update` | Update status, Kind, design, acceptance criteria, or append a Note; Assignment uses `claim`/`release`, labels use `label` |
| `claim` | Atomically assign one Open, unblocked Issue (`<issue-id> --assignee <name>`) |
| `release` | Atomically unassign one Open Issue from its exact owner (`<issue-id> --assignee <name>`) |
| `close` | Close one or more issues, optionally `--reason` |
| `reopen` | Reopen a closed issue, optionally `--reason` |
| `delete` | Delete an issue permanently (`--force` skips the confirmation prompt) |
| `ready` | Open Issues without unresolved direct Blocking Dependencies; defaults to unassigned, with `--assignee` and `--all-assignees` selectors |
| `blocked` | Issues with direct Blocking Dependencies to non-Closed prerequisites, along with those prerequisites |
| `blocking-dependency` | Blocking Dependencies: `add`/`remove --dependent <id> --prerequisite <id>`, `list --dependent|--prerequisite <id>`, `tree --dependent <id> [--depth N]` |
| `related` | Symmetric Related Associations: `add`/`remove --issue <id> --related <id>`, `list --issue <id>` |
| `discovery` | Directed Discovery Origins: `add`/`remove --discovered <id> --source <id>`, `list --discovered <id>` |
| `parent` | Parentage: `set`/`move --child <id> --parent <epic-id>`, `clear`/`show --child <id>` |
| `label` | Labels: `add <label> [<issue-id>]`, `remove`, `list <issue-id>`, `list-all`; use `--ids` for batches |
| `resource` | Associated Resources: `add`, `list`, `update`, `remove` (see below) |
| `stale` | Issues not updated in N days (`--days`, default 30) |
| `stats` | Project statistics (`--detailed` for a breakdown) |

Global flags include `--json` for data-command output and `-y`/`--yes` to skip confirmation prompts.

## Usage

The IDs below (`demo-a3f8`, `demo-b2c9`) are illustrative generated IDs.
Replace them with IDs printed by `rivets create` in your repository.

### Managing Issues

```bash
rivets create --title "Fix login bug" --kind bug --priority 1
rivets list                              # All Workflow States (priority-sorted, max 50)
rivets list --status open                # Filter to open issues
rivets list --status in_progress         # Filter by status
rivets show demo-a3f8                    # View issue details
rivets update demo-a3f8 --priority 2     # Update fields
rivets claim demo-a3f8 --assignee alice  # Atomically claim Ready work
rivets release demo-a3f8 --assignee alice
rivets claim demo-a3f8 --assignee alice
rivets update demo-a3f8 --status in_progress
rivets close demo-a3f8 --reason "Fixed in commit abc123" # Closing clears Assignment
```

### Blocking Dependencies

```bash
rivets blocking-dependency add --dependent demo-a3f8 --prerequisite demo-b2c9
rivets blocking-dependency remove --dependent demo-a3f8 --prerequisite demo-b2c9
rivets blocking-dependency list --dependent demo-a3f8       # Its prerequisites
rivets blocking-dependency list --prerequisite demo-b2c9    # Issues that depend on it
rivets blocking-dependency tree --dependent demo-a3f8 --depth 3
rivets blocked
rivets ready                             # Unassigned Ready Issues
rivets ready --assignee alice            # Ready Issues claimed by alice
rivets ready --all-assignees             # Ready Issues regardless of Assignment
```

A Blocking Dependency always points from the dependent Issue to its
prerequisite. Self-dependencies and Blocking-only cycles are rejected. Closing
a prerequisite leaves the relationship recorded but stops it from blocking.
Ready requires Workflow State Open. Parentage, Related Associations, and
Discovery Origins never affect Blocked or Ready, and neither condition is
serialized on Issue records.

### Non-blocking Relationships

```bash
rivets related add --issue demo-a3f8 --related demo-b2c9
rivets related list --issue demo-b2c9
rivets related remove --issue demo-b2c9 --related demo-a3f8

rivets discovery add --discovered demo-a3f8 --source demo-b2c9
rivets discovery list --discovered demo-a3f8
rivets discovery remove --discovered demo-a3f8 --source demo-b2c9
```

Related Associations are symmetric and idempotent: endpoint order does not
change their identity. Discovery Origins are directed from the discovered Issue
to a source Issue, may record multiple sources, and reject duplicates and
Discovery-only cycles. Neither relationship affects Blocked or Ready status.

The MCP equivalents are `related_add`, `related_remove`, `related_list`,
`discovery_add`, `discovery_remove`, and `discovery_list`, with the same
endpoint names and semantics.

### Parentage

```bash
rivets parent set --child demo-a3f8 --parent demo-b2c9
rivets parent show --child demo-a3f8
rivets parent move --child demo-a3f8 --parent demo-c4d1
rivets parent clear --child demo-a3f8
```

Each child has at most one parent, and only an Epic can be the parent. Use
`move` rather than `set` to replace an existing parent. Nested Epics are
allowed, but Parentage cycles are rejected. A non-Closed child cannot attach
to a Closed Epic or reopen beneath one, and an Epic cannot close while it has
non-Closed direct children. Parentage never changes Blocked or Ready.

### Labels

```bash
rivets label add urgent demo-a3f8         # Syntax: label add <label> <issue-id>
rivets label remove urgent demo-a3f8
rivets label list demo-a3f8               # Labels on one issue
rivets label list-all                     # Every label in the repository
rivets list --label backend
```

### Associated Resources

Attach absolute HTTP/HTTPS URLs or workspace-relative file paths to an
Issue, then curate them in place: `update` changes only the fields you
provide, and `remove` deletes a single resource — in both cases every
other resource keeps its stable ID and position. The same operations are
available through the MCP server as `resource_add`, `resource_list`,
`resource_update`, and `resource_remove`.

```bash
rivets resource add demo-a3f8 \
  --url https://example.com/pull/123 \
  --role implementation \
  --label "Implementation PR"
rivets resource add demo-a3f8 --path docs/design/feature.md --role documentation
rivets resource list demo-a3f8
rivets resource update demo-a3f8 --resource r1 --role evidence --no-label
rivets resource remove demo-a3f8 --resource r2
```

Roles are `implementation`, `documentation`, `evidence`, `successor`, and
`reference`. Resources retain insertion order and a stable per-Issue ID.
Workspace paths are stored normalized (`docs/../docs/x` becomes `docs/x`),
always use `/` as the separator, cannot be absolute or escape the
workspace root, and need not exist yet — branch-local and generated files
are fine.

### JSON Output

Data commands accept `--json` for scripting (init always prints text):

```bash
rivets list --json | jq '.[] | select(.priority == 1)'
ID=$(rivets create --title "Fix login bug" --json | jq -r '.id')
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

- Rust 1.94+ (edition 2024)

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

### Publishing

<details>
<summary>Release procedure</summary>

Rivets publishes `rivets-jsonl`, `rivets`, and `rivets-mcp` at one shared
workspace version. Publish them in that order because each later crate depends
on the previous one.

#### Prerequisites

- Publish access to all three crates on crates.io. For a manual release,
  authenticate with `cargo login`.
- [`git-cliff`](https://git-cliff.org/) for changelog generation.
- A clean release branch based on `main`, with the current `main` CI run green.
- Optional: the GitHub CLI (`gh`) for creating the GitHub release.

#### 1. Prepare the release

Choose a semantic version without the leading `v`:

```bash
VERSION=0.2.0
TAG="v${VERSION}"
```

Update the root `Cargo.toml` in three places:

1. `[workspace.package] version`
2. `[workspace.dependencies] rivets-jsonl` version
3. `[workspace.dependencies] rivets` version

All crate manifests inherit the workspace version. Refresh `Cargo.lock`, then
generate the release changelog:

```bash
cargo check --workspace
git cliff --unreleased --tag "$TAG" --prepend CHANGELOG.md
```

Review `CHANGELOG.md`: leave a fresh `[Unreleased]` section above the new
release and update the comparison links at the bottom.

#### 2. Verify the release commit

Run the same essential checks enforced by CI:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features --workspace
cargo test --doc --all-features --workspace
cargo build --release --all-features --workspace
```

Commit `Cargo.toml`, `Cargo.lock`, and `CHANGELOG.md` with a message such as
`chore(release): prepare v0.2.0`, open a pull request, and merge only after CI
passes.

#### 3. Tag the merged release

Tag the merge commit on `main`:

```bash
git checkout main
git pull --ff-only
git tag -a "$TAG" -m "Rivets ${VERSION}"
git push origin "$TAG"
```

#### 4. Publish to crates.io

Dry-run and publish each crate serially. Wait for each version to appear in the
crates.io index before checking or publishing its dependent:

```bash
cargo publish --locked -p rivets-jsonl --dry-run
cargo publish --locked -p rivets-jsonl
cargo info "rivets-jsonl@${VERSION}"  # Retry until indexed

cargo publish --locked -p rivets --dry-run
cargo publish --locked -p rivets
cargo info "rivets@${VERSION}"        # Retry until indexed

cargo publish --locked -p rivets-mcp --dry-run
cargo publish --locked -p rivets-mcp
cargo info "rivets-mcp@${VERSION}"    # Retry until indexed
```

Published crate versions are immutable. If publication stops partway through,
fix the failure and continue with the remaining crate; do not reuse or overwrite
an already-published version.

#### 5. Create the GitHub release

After all three `cargo info` checks succeed, create the release from the
existing tag:

```bash
gh release create "$TAG" \
  --verify-tag \
  --title "Rivets ${VERSION}" \
  --generate-notes \
  --fail-on-no-commits
```

### Automating releases

This repository does not currently have a release workflow. The recommended
automation is a tag-triggered `.github/workflows/release.yml` using
[crates.io Trusted Publishing](https://crates.io/docs/trusted-publishing).
Trusted Publishing uses GitHub OIDC instead of a long-lived crates.io token.

A safe workflow should:

1. Run only for `v*` tags and verify the tag matches the workspace version.
2. Use a protected GitHub `release` environment with required approval.
3. Grant only `contents: write` and `id-token: write`.
4. Re-run release checks.
5. Authenticate with `rust-lang/crates-io-auth-action@v1`.
6. Publish the three crates serially, waiting for crates.io indexing between them.
7. Create the GitHub release after all publications succeed.

Configure each crate's trusted publisher on crates.io with this repository, the
exact workflow filename, and the `release` environment. A local script or
`cargo-release` can automate version/changelog/tag preparation, but CI-based
Trusted Publishing is safer for registry credentials and provides an auditable
approval gate.

</details>

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
