# Findings — rivets-bkjj prove-it-prototype

Date: 2026-08-02. Probe: `.rivets-bkjj/probe/` (Rust, runtime enumeration via
clap `ValueEnum` / `parse_status` / `parse_dep_type` / serde).
Oracle: `.rivets-bkjj/oracle.py` (regex extraction from source text).

## Result

`diff <(sort probe-out) <(sort oracle-out)` — **identical, 79 lines**, across
three tables:

1. **CLI** (Arg mirrors in `cli/types.rs`): 18 accepted strings — every
   canonical name plus exactly one alias (`in-progress` → `in_progress`).
   All map 1:1 to canonical domain Display strings.
2. **MCP** (`parse_status`, `parse_dep_type`, `McpIssueKind`): 39 accepted
   strings — every canonical string, every canonical string case-folded
   (`to_lowercase()` / `eq_ignore_ascii_case`), plus three lenient aliases
   (`in-progress`, `parent_child`, `discovered_from`).
3. **Domain**: `Display` == serde output for all 18 variants (the oracle
   asserts this invariant per variant and it held).

## What I learned (not obvious before probing)

1. **The duplication is real and byte-exact for CLI↔domain.** The Arg
   mirrors' clap-visible names/aliases equal the domain `Display` strings
   plus the single `in-progress` alias. Migrating CLI to domain `ValueEnum`
   preserves the accepted set exactly — the design can claim it without
   guesswork.
2. **MCP leniency is exactly three aliases + case-folding.** Today MCP
   accepts 39 spellings; after switching to domain `FromStr` it will accept
   the 17 canonical strings (18 minus none — status has 4, kind 5, role 5,
   dep-type 4 = 18). The lenient spellings (`IN-PROGRESS`,
   `parent_child`, `BUG`, …) are pinned by tests
   (`mcp_kind_input_remains_case_insensitive`, `test_parse_status`,
   `test_parse_dep_type`) and must be deliberately rewritten, not silently
   kept.
3. **Domain serde and Display cannot drift today** (oracle invariant held),
   so the single-vocabulary migration has no latent mismatch to reconcile:
   serde renames, Display arms, and clap names all agree modulo the alias.

## Baseline for checkpointed-build

`/tmp/probe-out.txt` equivalent is the frozen CLI contract:
`[cli] <Enum> <name> -> <canonical>` + `[cli] <Enum> alias <alias> -> <canonical>`
(18 lines). After implementation, probe v2 (domain `ValueEnum` +
`FromStr`) must reproduce the `[cli]` lines exactly; MCP lines become the
canonical-only subset.

## Final integration (2026-08-02, after all four slices)

- Probe v2 (`after.rs`) vs `baseline-cli-contract.txt`: **AGREES, 19 lines**,
  plus FromStr roundtrip + lenient-rejection checks pass.
- CLI invalid-value oracle: 5 commands exit 2 with stderr byte-identical to
  `cli-invalid-baseline.txt`; `--status in-progress` alias still exit 0.
- Full gates: 1051 tests pass, clippy `--all-targets --all-features
  -D warnings` clean, `cargo fmt --check` clean, doctests 42 pass.
- Dependency edges: clap absent from `rivets-mcp/Cargo.toml` (before and
  after; only transitive via rivets) — C8 holds.
- Grep sweep: no Arg-mirror/MCP-table identifiers remain; remaining
  `"in_progress"` etc. literals are JSON output keys, error-message
  `valid_values` text, or test inputs.
- Drift check vs `origin/main`: no `.jsonl`/`.lock` divergence.
