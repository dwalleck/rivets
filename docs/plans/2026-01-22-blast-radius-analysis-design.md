# Blast Radius Analysis Design

**Date**: 2026-01-22
**Status**: Draft
**Author**: Collaborative design session

## Overview

This feature enables associating tasks/issues with specific code symbols and analyzing the "blast radius" of potential changes. The goal is to provide pre-planning risk assessment and impact analysis, helping developers understand what might be affected before making changes.

## Problem Statement

When planning work, understanding *what code needs to change* and *what might be affected* is often tribal knowledge. This feature makes that explicit by:

- Linking issues to specific code symbols
- Analyzing the dependency graph around those symbols
- Computing a configurable risk score based on weighted factors

## Data Model

Issues gain a `code_refs` field - an array of code references:

```yaml
code_refs:
  - symbol: "IssueStorage::save"
    file: "src/storage/issue.rs"
    line: 142
    language: rust
    analysis:
      risk_score: 7.2
      direct_callers: 8
      transitive_dependents: 23
      test_coverage: 0.85
      last_analyzed: 2024-01-22T10:30:00Z
```

Each reference captures:

- **Symbol identity**: name, file, line (resolved at link time)
- **Analysis results**: cached blast radius data
- **Staleness tracking**: when analysis was last run

Analysis results are cached on the issue but can be refreshed on demand.

## Technical Architecture

### Two-Layer Analysis (Tree-sitter + LSP Hybrid)

**Layer 1: Tree-sitter Index (Always Available)**

- Parses source files to extract symbol definitions (functions, methods, structs, classes)
- Builds a basic call graph by matching symbol names at call sites
- Fast, works offline, no external dependencies
- Limitations: can miss renamed imports, generics, trait implementations

**Layer 2: LSP Enhancement (When Available)**

- Queries rust-analyzer or OmniSharp for precise information
- `textDocument/references` - find all callers of a symbol
- `callHierarchy/incomingCalls` - build accurate call trees
- `textDocument/definition` - resolve symbols through indirection

**How They Work Together**

```
1. User links symbol "IssueStorage::save" to issue
2. Tree-sitter index provides quick initial analysis (~100ms)
   → "Found 6 direct calls, 15 transitive"
3. If LSP available, refines the analysis (~500ms)
   → "Actually 8 direct calls (found 2 via trait impl), 23 transitive"
4. Results cached on issue with source noted (tree-sitter vs LSP)
```

### Index Management

The tree-sitter index lives in `.rivets/index/` and rebuilds incrementally when files change. It's project-local and git-ignored.

## Risk Scoring System

### Configurable Weighted Factors

The risk score (1-10 scale) is computed from weighted factors defined in `.rivets/blast-radius.toml`:

```toml
[scoring]
# Weights must sum to 1.0
[scoring.weights]
dependency_depth = 0.25      # How deep the call chain goes
caller_count = 0.20          # Direct callers
transitive_count = 0.15      # Total affected symbols
cyclomatic_complexity = 0.10 # Function complexity
public_api = 0.10            # Is it exported/public?
test_coverage = 0.10         # Inverse - less coverage = more risk
churn_rate = 0.10            # How often this code changes

[scoring.thresholds]
# Map raw values to 1-10 scores
caller_count = [2, 5, 10, 20, 50]      # <2=1, <5=2, etc.
transitive_count = [5, 15, 30, 60, 100]
cyclomatic_complexity = [5, 10, 15, 25, 40]
```

### Factor Sources

| Factor | Tree-sitter | LSP | Git |
|--------|-------------|-----|-----|
| caller_count | ✓ (approximate) | ✓ (precise) | |
| transitive_count | ✓ | ✓ | |
| cyclomatic_complexity | ✓ | | |
| public_api | ✓ | ✓ | |
| test_coverage | | ✓ (if available) | |
| churn_rate | | | ✓ |

Factors gracefully degrade - if LSP isn't available, tree-sitter values are used. If git history isn't accessible, churn defaults to neutral.

## Output Formats

### 1. Score + Summary (Default)

Quick view for issue listings and AI agents:

```
Risk: 7.2/10 (High)
Symbol: IssueStorage::save (src/storage/issue.rs:142)
Impact: 8 direct callers across 4 modules, 23 transitive dependents
Concern: Low test coverage (45%), high churn (12 changes in 30 days)
```

### 2. Detailed Report

Full breakdown for planning sessions:

```
═══ Blast Radius Analysis: IssueStorage::save ═══

Risk Score: 7.2/10

Factor Breakdown:
  dependency_depth:     6/10  (4 levels deep)
  caller_count:         7/10  (8 direct callers)
  transitive_count:     5/10  (23 dependents)
  cyclomatic_complexity: 4/10 (complexity: 8)
  public_api:           10/10 (pub trait method)
  test_coverage:        8/10  (45% coverage)
  churn_rate:           6/10  (12 changes/30d)

Direct Callers (8):
  cli/commands/update.rs:87      execute()
  cli/commands/close.rs:54       execute()
  mcp/tools/update.rs:123        handle_update()
  ...

Transitive Dependents (23): [expandable]
```

### 3. Visual Graph

ASCII tree for terminal, with optional DOT export for Graphviz:

```
IssueStorage::save
├── cli/commands/update::execute
│   └── cli/main::run_command
├── cli/commands/close::execute
│   └── cli/main::run_command
├── mcp/tools/update::handle_update
│   └── mcp/server::dispatch
...

[Export: rivets analyze ISSUE-1 --format=dot > graph.dot]
```

## CLI Interface

```bash
# Link a symbol to an issue (interactive symbol search)
rivets link ISSUE-1
> Search symbol: save
> Found 3 matches:
>   1. IssueStorage::save (src/storage/issue.rs:142)
>   2. Config::save (src/config.rs:87)
>   3. save_to_disk (src/utils/io.rs:23)
> Select [1-3]: 1
✓ Linked IssueStorage::save to ISSUE-1

# Run analysis on an issue's linked symbols
rivets analyze ISSUE-1
# Outputs score + summary by default

rivets analyze ISSUE-1 --detailed    # Full report
rivets analyze ISSUE-1 --graph       # ASCII dependency tree
rivets analyze ISSUE-1 --format=dot  # Graphviz export
rivets analyze ISSUE-1 --format=json # Machine-readable

# Re-analyze with fresh LSP data
rivets analyze ISSUE-1 --refresh

# Search symbols without linking (exploration)
rivets symbols search "save"
rivets symbols callers IssueStorage::save
```

## MCP Tools

```
symbol_search(query, language?) → [Symbol]
link_symbol(issue_id, symbol_ref) → Issue
analyze_issue(issue_id, refresh?) → AnalysisResult
get_blast_radius(symbol_ref) → BlastRadius
```

AI agents can search for symbols, link them to issues, and query blast radius - all without needing the visual output. They receive structured JSON they can reason about.

## Language Support Architecture

### Plugin-Based Language Handlers

Each language is handled by a dedicated module implementing a common trait:

```rust
trait LanguageAnalyzer {
    fn parse_symbols(&self, source: &str) -> Vec<Symbol>;
    fn find_references(&self, symbol: &Symbol) -> Vec<Reference>;
    fn compute_complexity(&self, symbol: &Symbol) -> u32;
    fn tree_sitter_language(&self) -> tree_sitter::Language;
    fn lsp_server_command(&self) -> Option<(&str, &[&str])>;
}
```

### Supported Languages

**Rust**
- Tree-sitter: `tree-sitter-rust`
- LSP: `rust-analyzer` (auto-detected if in PATH)
- Special handling: trait impls, macro expansions, generics

**C#**
- Tree-sitter: `tree-sitter-c-sharp`
- LSP: `OmniSharp` or `csharp-ls`
- Special handling: partial classes, extension methods, LINQ expressions

### Language Detection

Language is determined by file extension, with `.rivets/blast-radius.toml` allowing overrides:

```toml
[languages]
rust = { extensions = ["rs"], lsp = "rust-analyzer" }
csharp = { extensions = ["cs"], lsp = "omnisharp" }
```

Adding a new language means implementing the `LanguageAnalyzer` trait and adding a tree-sitter grammar.

## Index Storage & Lifecycle

### Index Location

```
.rivets/
├── blast-radius.toml      # Configuration (versioned)
├── index/                  # Analysis cache (git-ignored)
│   ├── rust/
│   │   ├── symbols.db     # Symbol definitions
│   │   └── refs.db        # Reference graph
│   └── csharp/
│       ├── symbols.db
│       └── refs.db
```

### Index Updates

- **On-demand**: `rivets index rebuild` for full rebuild
- **Incremental**: When `analyze` runs, stale files (modified since last index) are re-parsed
- **Background** (future): Optional file watcher for real-time updates

### Staleness Detection

Each indexed file stores its last-modified timestamp. When analyzing:

1. Check if any file in the dependency chain has changed
2. If yes, re-index affected files before analysis
3. Flag results with confidence: "fresh" vs "possibly stale"

### Cache Invalidation

Analysis results cached on issues include the commit SHA when analyzed. If HEAD has moved significantly (configurable threshold), the CLI suggests re-analysis:

```
⚠ Analysis from 3 days ago (15 commits behind). Run with --refresh?
```

## Future Considerations

- **Background indexing daemon**: File watcher for real-time index updates
- **Additional languages**: TypeScript, Python, Go based on user demand
- **IDE integration**: VS Code extension showing blast radius inline
- **Historical analysis**: Track how blast radius changes over time for a symbol
