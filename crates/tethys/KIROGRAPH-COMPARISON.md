# Tethys vs. KiroGraph — Comparison & Learnings

A side-by-side comparison of [`tethys`](./README.md) (this crate) and
[KiroGraph](https://github.com/davide-desio-eleva/kirograph), a TypeScript-based
semantic code knowledge graph for the Kiro IDE. Both projects solve the same
core problem; the comparison is useful because they have made different
trade-offs and are at different stages of feature completeness.

> Date of comparison: 2026-05-09
> KiroGraph version reviewed: 0.12.0

## TL;DR

- **Same fundamental data model**: tree-sitter parsing → SQLite → (file → symbol → reference) + a graph of edges.
- **KiroGraph** is further along the **feature-breadth** dimension (17 languages, framework awareness, semantic embeddings, MCP server, architecture analysis, interactive graph export).
- **Tethys** is further along the **engineering-rigor** dimension (Rust, newtypes, `unsafe_code = forbid`, pedantic clippy, library-first API, parallel indexing via rayon, LSP integration, ~1400 tests).
- **Highest-leverage borrow**: KiroGraph's **MCP server** pattern. Tethys already exposes the right methods on its public API and the workspace already has a `rivets-mcp` crate ready to host them.
- **Lowest-effort, highest-signal additions**: hotspots, surprising connections, dead code, snapshot/diff, path queries, coupling metrics — these are mostly **SQL queries on top of tethys's existing schema**, not new indexing logic.

## Architectural philosophy

### Tethys

Declares itself a **cache, not an analyzer**. From `lib.rs`:

> - **Cache, not analyzer** — Tethys indexes and caches; LSPs do the hard semantic work
> - **Layered accuracy** — Fast approximate results (tree-sitter), optional precision (LSP integration)
> - **Language extensible** — Start with Rust + C#, design for adding more
> - **Embeddable** — Library first, CLI second
> - **Intelligence, not policy** — Reports facts ("12 callers"), not judgments ("too risky")

The public surface is 35+ methods on a `Tethys` struct; the CLI is a thin shell over them.

### KiroGraph

**AI-assistant-shaped**. Every design choice serves "fewer tool calls for Kiro":
- The MCP server is the central integration.
- Semantic search exists because natural-language task descriptions need it.
- Caveman mode compresses agent prose because long sessions waste tokens.
- Auto-sync via Kiro hooks keeps the index fresh with zero overhead during active editing.

It is less a library, more an embeddable assistant backend.

## Schema design difference

This is the most interesting low-level divergence:

- **Tethys**: **specialized tables per edge kind** (`call_edges`, `refs`, `file_deps`, `imports`) with **integer surrogate keys** (`FileId`, `SymbolId`).
- **KiroGraph**: a single polymorphic `edges` table with a `kind` discriminator, **TEXT primary keys**, plus an `unresolved_refs` staging table.

Trade-offs:

| Dimension | Tethys (split tables) | KiroGraph (unified) |
|---|---|---|
| Type safety | Higher — different shapes for different edges | Lower — one shape fits all |
| Index locality / query plan | Tighter — narrow indexes per kind | Wider — must filter by `kind` |
| Extensibility (new edge kind) | Schema migration | Just a new string |
| Cross-kind queries (e.g. "all incoming edges of any kind") | UNION across tables | Single scan |

There is no "right" answer; tethys's choice is consistent with its Rust/typed-newtypes culture. **Do not refactor this** without a concrete pain point.

## Side-by-side capability matrix

| Capability | Tethys | KiroGraph |
|---|---|---|
| **Languages** | Rust, C# | TS/JS/TSX/JSX, Python, Go, Rust, Java, C, C++, C#, PHP, Ruby, Swift, Kotlin, Dart, Svelte, Elixir (17) |
| **Framework awareness** | None (Cargo-aware) | React, Next.js, Express, Django, Flask, FastAPI, Rails, Spring, ASP.NET, SwiftUI, Vapor, Laravel, Phoenix, etc. |
| **Symbol search** | Fuzzy by name | Exact + FTS5 + LIKE + vector fallback |
| **Callers / callees / impact** | Yes | Yes |
| **Reachability (BFS)** | Yes — forward + backward | Yes |
| **Cycle detection** | Yes | Yes (Tarjan SCC) |
| **Affected tests** | Yes | Yes |
| **Path between symbols** | Files only | Symbols + files |
| **Type hierarchy** | No | Yes — extends / implements |
| **Dead code** | No | Yes |
| **Hotspots / surprising connections** | No | Yes |
| **Snapshot + diff** | No | Yes |
| **Architecture analysis (packages, layers, coupling)** | No | Yes — Ca / Ce / instability |
| **Semantic embeddings** | No | Yes — 7 vector engines (cosine, sqlite-vec, orama, pglite, lancedb, qdrant, typesense) |
| **MCP server** | No (but `rivets-mcp` exists in workspace) | Yes — 16 tools, auto-approved |
| **Interactive HTML graph export** | No | Yes |
| **LSP integration** | Yes | No |
| **Panic-point detection** | Yes — Rust-specific `.unwrap()` / `.expect()` finder | No |
| **Cargo-aware module paths** | Yes | Partial (manifest-based packages) |
| **Auto-sync hooks** | No | Yes — dirty-marker + deferred flush |
| **Library-first API** | Strong | Weaker (CLI-shaped) |
| **Type safety** | Newtypes, `unsafe_code = forbid`, pedantic clippy | TypeScript |
| **Parallel indexing** | Yes — rayon | Sequential |
| **Test count** | ~1400 (workspace) | Smaller |

## Suggestions for tethys

Ordered by approximate **value-to-effort ratio**. Several are mostly **SQL on the existing schema**, not new indexing.

### 1. MCP server exposing tethys's API to AI assistants — *medium effort, transformational*

The single biggest UX win in KiroGraph. Kiro literally queries the graph instead of grepping files, so tool-call count and context usage drop dramatically.

- Tethys's `lib.rs` already exposes the right methods (`search_symbols`, `get_callers`, `get_impact`, `get_forward_reachable`, `get_affected_tests`, …).
- The workspace already contains a `rivets-mcp` crate that can host them.
- Recommended tools to expose first: `search`, `callers`, `callees`, `impact`, `path`, `affected-tests`, `panic-points`, `stats`.

### 2. Hotspots and surprising connections — *low effort, high signal*

Both are pure read queries on the existing call/reference graph.

- **Hotspots**: top-N symbols ranked by `in_degree + out_degree`, optionally excluding structural edges. One SQL query.
- **Surprising**: cross-file edges scored by `path_distance(source_file, target_file) × edge_kind_weight`. Surfaces unexpected coupling that a human reviewer might never spot.

### 3. Snapshot + diff — *low effort*

- Save a JSON-encoded list of `(symbol_id, edge_tuple)` triples to `.rivets/snapshots/`.
- Diff via set operations: O(n) regardless of codebase size.
- CI use case: "What did this PR add/remove from the graph?" is a strong primitive for code review.

### 4. Architecture / coupling metrics — *medium effort, well-known utility*

Tethys already discovers crates via `cargo.rs` and tracks `file_deps`. Rolling those up gives you Robert C. Martin's classic metrics:

- **Ca** (afferent coupling): how many other packages depend on this one.
- **Ce** (efferent coupling): how many packages this one depends on.
- **Instability**: `Ce / (Ca + Ce)` — 0 = maximally stable, 1 = maximally unstable.

Immediately legible to seasoned engineers; useful for refactor planning and PR review.

### 5. Path queries between symbols — *low effort*

Tethys has `get_dependency_chain` for files; the same BFS, but over the union of edge kinds (`call_edges` ∪ `refs` ∪ `imports`), gives you `path(symbolA, symbolB)`. Answers "how is `LoginController` connected to `Pool`?"

### 6. Type-hierarchy queries — *low effort, language extension*

Tree-sitter can already extract `impl X for Y` and `trait X: Y` in Rust, and `: BaseClass, IInterface` in C#. Add `extends` / `implements` edges + an upward/downward traversal.

### 7. More languages — *higher effort, but the hard part is done*

Tree-sitter grammars exist for everything. Tethys's `languages/` module is already structured around per-language extractors. Highest-leverage adds:

1. **Python** — broad ecosystem, already covered by tree-sitter-python.
2. **TypeScript / TSX** — frontend coverage; valuable for full-stack repos.
3. **Go** — well-defined module system, easy parser.

### 8. Auto-sync via dirty-marker + deferred flush — *low effort, big UX*

KiroGraph's pattern:

- File save / create → write a 1-byte `.dirty` marker (cheap; no parse).
- Agent stop / session boundary → re-sync all dirty files in one batch.

Avoids per-keystroke overhead while keeping the graph fresh. A natural fit for Claude Code hooks.

### 9. Dead-code finder — *trivial*

```sql
SELECT s.* FROM symbols s
LEFT JOIN call_edges c ON c.callee_symbol_id = s.id
WHERE c.callee_symbol_id IS NULL
  AND s.visibility = 'private';
```

One query. Filtering to non-exported symbols avoids false positives from public API.

### 10. Interactive HTML graph export — *higher effort, mostly orthogonal*

Nice but separable. Defer until the rest is in place. The PNG / path / cluster / heat-map UI is polish, not core.

### Things to *not* borrow

- **Single-table edges with `kind` discriminator** — Tethys's split-tables design is reasonable and consistent with its typing culture. Don't refactor without a concrete pain point.
- **Caveman mode** — Creative, but a tooling-level concern, not an indexer concern.
- **7 vector-engine backends** — KiroGraph's optional dependency surface is huge (`@orama`, `@lancedb`, `@electric-sql/pglite`, `qdrant-local`, `typesense`, `sqlite-vec`, `better-sqlite3`). If semantic search ever becomes a goal, pick **one** engine and integrate it well rather than offering seven.

## What KiroGraph could learn from tethys

Recorded for symmetry; not actionable for us, but useful context.

- **Strong typing with newtypes** (`FileId`, `SymbolId`) prevents whole classes of bugs that a `string` ID design invites.
- **Library-first API** — KiroGraph's logic is shaped by its CLI; tethys's CLI is shaped by its library.
- **Cargo-aware module path resolution** — tethys's `crate::db::Index` qualified names are more precise than KiroGraph's manifest-based package detection.
- **Performance ceiling** — Rust + rayon will scale to repositories where Node + sequential indexing won't.
- **LSP integration as an accuracy upgrade** — tree-sitter is fast and approximate; LSP is slow and precise. Tethys lets you opt into the latter when it matters.

## Bottom line

The two projects are complementary studies of the same problem. KiroGraph shows
what a feature-rich, AI-assistant-integrated code graph looks like; tethys
shows what a rigorously engineered, embeddable, library-first one looks like.
The most leveraged step for tethys is to **expose its existing API as an MCP
server**, then build out the **read-only query features that ride on the graph
it already populates** (hotspots, surprising, dead code, snapshot/diff, path,
coupling). More languages and an auto-sync hook pattern are bigger but
well-marked roads after that.
