# Code Intelligence for Rivets

This document describes how rivets could implement tree-sitter based structural indexing and semantic search to give agents better context when working on issues.

## Motivation

When an agent picks up an issue that references specific files, they currently have no visibility into:
- What symbols (functions, structs, traits) exist in those files
- What other files depend on the code being changed
- The potential "blast radius" of their changes
- Which tests are relevant to run

By adding code intelligence, rivets can provide agents with structural awareness before they start making changes.

## Inspiration: Claude OS Hybrid Indexing

Claude OS implements a two-phase indexing system:

| Phase | Method | Speed | Purpose |
|-------|--------|-------|---------|
| **Structural** | tree-sitter parsing | ~30 sec / 10K files | Symbol extraction, dependency graph |
| **Semantic** | Embeddings (optional) | ~20-30 min | Conceptual search, "how does X work?" |

For rivets, the structural index alone provides significant value without the complexity of embeddings.

## Architecture

### Data Flow

```
Source Files → tree-sitter parse → Symbols + Dependencies → JSONL storage
                                          ↓
                                   petgraph DiGraph
                                          ↓
                              Issue enrichment / queries
```

### Integration with Existing Storage

Code context data lives in the same JSONL file as issues, using the entity-per-line pattern:

```jsonl
{"entity":"issue","id":"rivets-abc","title":"Refactor auth middleware"}
{"entity":"file_symbols","path":"src/middleware/auth.rs","mtime":1735123456.78,"size":2048,"symbols":[...]}
{"entity":"code_dep","from":"src/routes/api.rs","to":"src/middleware/auth.rs","symbol":"AuthMiddleware"}
```

## Schema Design

### New Entity Types

```rust
/// Symbols extracted from a single file
#[derive(Serialize, Deserialize)]
struct FileSymbols {
    path: PathBuf,
    mtime: f64,           // For cache invalidation
    size: u64,            // For cache invalidation
    language: String,     // "rust", "typescript", etc.
    symbols: Vec<Symbol>,
}

/// A code symbol (function, struct, trait, const, etc.)
#[derive(Serialize, Deserialize)]
struct Symbol {
    name: String,
    kind: SymbolKind,
    line: u32,
    signature: String,    // e.g., "fn authenticate(token: &str) -> Result<User>"
    #[serde(skip_serializing_if = "Option::is_none")]
    visibility: Option<Visibility>,  // pub, pub(crate), private
}

#[derive(Serialize, Deserialize)]
enum SymbolKind {
    Function,
    Struct,
    Enum,
    Trait,
    Impl,
    Const,
    Static,
    Module,
    TypeAlias,
    Macro,
}

/// Dependency between files (A uses something from B)
#[derive(Serialize, Deserialize)]
struct CodeDependency {
    from_file: PathBuf,   // File that has the dependency
    to_file: PathBuf,     // File being depended on
    symbol: String,       // Symbol being used
    dep_type: CodeDepType,
}

#[derive(Serialize, Deserialize)]
enum CodeDepType {
    Import,      // use statement
    Call,        // Function call
    Inherit,     // Trait implementation
    Type,        // Type usage in signature
}
```

### Extended RivetsRecord Enum

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "entity")]
enum RivetsRecord {
    // Existing
    #[serde(rename = "issue")]
    Issue(Issue),

    #[serde(rename = "issue_dep")]
    IssueDependency(Dependency),

    // New: Code intelligence
    #[serde(rename = "file_symbols")]
    FileSymbols(FileSymbols),

    #[serde(rename = "code_dep")]
    CodeDependency(CodeDependency),
}
```

## Implementation

### Crate Dependencies

```toml
[dependencies]
tree-sitter = "0.24"
tree-sitter-rust = "0.23"
tree-sitter-typescript = "0.23"
tree-sitter-python = "0.23"
# Add more languages as needed

petgraph = "0.6"  # Already using this for issue dependencies
```

### Module Structure

```
crates/rivets/src/
├── code_intel/
│   ├── mod.rs
│   ├── parser.rs         # tree-sitter parsing
│   ├── symbols.rs        # Symbol extraction per language
│   ├── graph.rs          # Dependency graph operations
│   └── cache.rs          # mtime-based cache invalidation
```

### Core Parser

```rust
// code_intel/parser.rs

use tree_sitter::{Parser, Language};
use std::path::Path;

pub struct CodeParser {
    parsers: HashMap<String, Parser>,
}

impl CodeParser {
    pub fn new() -> Self {
        let mut parsers = HashMap::new();

        // Initialize parsers for each language
        let mut rust_parser = Parser::new();
        rust_parser.set_language(&tree_sitter_rust::LANGUAGE.into()).unwrap();
        parsers.insert("rust".to_string(), rust_parser);

        // Add more languages...

        Self { parsers }
    }

    pub fn parse_file(&mut self, path: &Path) -> Result<FileSymbols> {
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let language = match extension {
            "rs" => "rust",
            "ts" | "tsx" => "typescript",
            "py" => "python",
            "js" | "jsx" => "javascript",
            _ => return Err(Error::UnsupportedLanguage(extension.to_string())),
        };

        let content = std::fs::read(path)?;
        let metadata = std::fs::metadata(path)?;

        let parser = self.parsers.get_mut(language)
            .ok_or_else(|| Error::UnsupportedLanguage(language.to_string()))?;

        let tree = parser.parse(&content, None)
            .ok_or(Error::ParseFailed)?;

        let symbols = self.extract_symbols(language, &tree, &content)?;

        Ok(FileSymbols {
            path: path.to_path_buf(),
            mtime: metadata.modified()?.duration_since(UNIX_EPOCH)?.as_secs_f64(),
            size: metadata.len(),
            language: language.to_string(),
            symbols,
        })
    }

    fn extract_symbols(&self, language: &str, tree: &Tree, content: &[u8]) -> Result<Vec<Symbol>> {
        match language {
            "rust" => self.extract_rust_symbols(tree, content),
            "typescript" => self.extract_ts_symbols(tree, content),
            // ...
            _ => Ok(vec![]),
        }
    }
}
```

### Rust Symbol Extraction

```rust
// code_intel/symbols.rs

impl CodeParser {
    fn extract_rust_symbols(&self, tree: &Tree, content: &[u8]) -> Result<Vec<Symbol>> {
        let mut symbols = Vec::new();
        let mut cursor = tree.walk();

        self.traverse_rust(&mut cursor, content, &mut symbols);

        Ok(symbols)
    }

    fn traverse_rust(&self, cursor: &mut TreeCursor, content: &[u8], symbols: &mut Vec<Symbol>) {
        loop {
            let node = cursor.node();

            match node.kind() {
                "function_item" => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = self.node_text(name_node, content);
                        let signature = self.extract_rust_fn_signature(&node, content);
                        symbols.push(Symbol {
                            name,
                            kind: SymbolKind::Function,
                            line: node.start_position().row as u32 + 1,
                            signature,
                            visibility: self.extract_rust_visibility(&node, content),
                        });
                    }
                }
                "struct_item" => {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        let name = self.node_text(name_node, content);
                        symbols.push(Symbol {
                            name: name.clone(),
                            kind: SymbolKind::Struct,
                            line: node.start_position().row as u32 + 1,
                            signature: format!("struct {}", name),
                            visibility: self.extract_rust_visibility(&node, content),
                        });
                    }
                }
                "impl_item" => {
                    // Extract impl blocks
                }
                "trait_item" => {
                    // Extract traits
                }
                // ... more node types
                _ => {}
            }

            // Recurse into children
            if cursor.goto_first_child() {
                self.traverse_rust(cursor, content, symbols);
                cursor.goto_parent();
            }

            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }

    fn node_text(&self, node: Node, content: &[u8]) -> String {
        std::str::from_utf8(&content[node.byte_range()]).unwrap_or("").to_string()
    }
}
```

### Dependency Graph

```rust
// code_intel/graph.rs

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::algo;
use std::collections::HashMap;

pub struct CodeGraph {
    graph: DiGraph<PathBuf, CodeDepType>,
    node_map: HashMap<PathBuf, NodeIndex>,
}

impl CodeGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            node_map: HashMap::new(),
        }
    }

    pub fn add_file(&mut self, path: PathBuf) -> NodeIndex {
        *self.node_map.entry(path.clone())
            .or_insert_with(|| self.graph.add_node(path))
    }

    pub fn add_dependency(&mut self, from: &Path, to: &Path, dep_type: CodeDepType) {
        let from_idx = self.add_file(from.to_path_buf());
        let to_idx = self.add_file(to.to_path_buf());
        self.graph.add_edge(from_idx, to_idx, dep_type);
    }

    /// Get all files that depend on the given file (reverse dependencies)
    pub fn get_dependents(&self, path: &Path) -> Vec<PathBuf> {
        let Some(&node) = self.node_map.get(path) else {
            return vec![];
        };

        self.graph
            .neighbors_directed(node, petgraph::Direction::Incoming)
            .map(|n| self.graph[n].clone())
            .collect()
    }

    /// Get all files that this file depends on
    pub fn get_dependencies(&self, path: &Path) -> Vec<PathBuf> {
        let Some(&node) = self.node_map.get(path) else {
            return vec![];
        };

        self.graph
            .neighbors_directed(node, petgraph::Direction::Outgoing)
            .map(|n| self.graph[n].clone())
            .collect()
    }

    /// Get transitive dependents (full blast radius)
    pub fn get_blast_radius(&self, path: &Path) -> Vec<PathBuf> {
        let Some(&start) = self.node_map.get(path) else {
            return vec![];
        };

        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(start);

        while let Some(node) = queue.pop_front() {
            for neighbor in self.graph.neighbors_directed(node, petgraph::Direction::Incoming) {
                if visited.insert(neighbor) {
                    queue.push_back(neighbor);
                }
            }
        }

        visited.iter()
            .map(|&n| self.graph[n].clone())
            .collect()
    }

    /// Calculate PageRank importance scores
    pub fn calculate_importance(&self) -> HashMap<PathBuf, f64> {
        let scores = petgraph::algo::page_rank(&self.graph, 0.85, 100);

        self.graph.node_indices()
            .map(|n| (self.graph[n].clone(), scores[n.index()]))
            .collect()
    }
}
```

### Cache Invalidation

```rust
// code_intel/cache.rs

impl CodeIntelligence {
    /// Check if cached symbols are still valid
    pub fn needs_reindex(&self, cached: &FileSymbols) -> Result<bool> {
        let metadata = std::fs::metadata(&cached.path)?;
        let current_mtime = metadata.modified()?.duration_since(UNIX_EPOCH)?.as_secs_f64();
        let current_size = metadata.len();

        Ok(cached.mtime != current_mtime || cached.size != current_size)
    }

    /// Incrementally update index for changed files
    pub fn update_index(&mut self, workspace_path: &Path) -> Result<IndexUpdate> {
        let mut updated = Vec::new();
        let mut added = Vec::new();
        let mut removed = Vec::new();

        // Find all source files
        let current_files: HashSet<PathBuf> = self.find_source_files(workspace_path)?;

        // Check existing cached files
        for (path, cached) in &self.file_symbols {
            if !current_files.contains(path) {
                removed.push(path.clone());
            } else if self.needs_reindex(cached)? {
                updated.push(path.clone());
            }
        }

        // Find new files
        for path in &current_files {
            if !self.file_symbols.contains_key(path) {
                added.push(path.clone());
            }
        }

        // Re-parse changed files
        for path in updated.iter().chain(added.iter()) {
            let symbols = self.parser.parse_file(path)?;
            self.file_symbols.insert(path.clone(), symbols);
        }

        // Remove deleted files
        for path in &removed {
            self.file_symbols.remove(path);
        }

        // Rebuild dependency graph
        self.rebuild_graph()?;

        Ok(IndexUpdate { updated, added, removed })
    }
}
```

## CLI Integration

### New Commands

```bash
# Index the current workspace
rivets index

# Show code context for an issue
rivets show rivets-abc --code-context

# Query dependencies
rivets deps src/middleware/auth.rs

# Show blast radius for a file
rivets blast-radius src/middleware/auth.rs
```

### Issue Context Display

```
$ rivets show rivets-abc --code-context

rivets-abc: Refactor authentication middleware
Status: open | Priority: 1 | Created: 2 days ago

Target Files:
  src/middleware/auth.rs
    ├─ struct AuthMiddleware (line 15, pub)
    ├─ fn authenticate(token: &str) -> Result<User> (line 42, pub)
    └─ fn require_admin(user: &User) -> bool (line 78, pub(crate))

  src/handlers/login.rs
    ├─ async fn handle_login(req: Request) -> Response (line 12, pub)
    └─ fn validate_credentials(email: &str, pass: &str) (line 45)

Blast Radius: 12 files
  Direct (4):
    ├─ src/handlers/admin.rs (uses require_admin)
    ├─ src/routes/api.rs (uses AuthMiddleware)
    ├─ src/routes/protected.rs (uses authenticate)
    └─ tests/auth_test.rs (tests authenticate)

  Transitive (8):
    └─ ... (run with --full to see all)

Suggested Test Files:
  tests/auth_test.rs
  tests/integration/login_test.rs
```

## JSONL Storage Example

```jsonl
{"entity":"issue","id":"rivets-abc","title":"Refactor authentication middleware","status":"open","priority":1}
{"entity":"file_symbols","path":"src/middleware/auth.rs","mtime":1735123456.78,"size":2048,"language":"rust","symbols":[{"name":"AuthMiddleware","kind":"Struct","line":15,"signature":"struct AuthMiddleware","visibility":"Pub"},{"name":"authenticate","kind":"Function","line":42,"signature":"fn authenticate(token: &str) -> Result<User>","visibility":"Pub"}]}
{"entity":"file_symbols","path":"src/handlers/login.rs","mtime":1735123400.00,"size":1024,"language":"rust","symbols":[{"name":"handle_login","kind":"Function","line":12,"signature":"async fn handle_login(req: Request) -> Response","visibility":"Pub"}]}
{"entity":"code_dep","from_file":"src/routes/api.rs","to_file":"src/middleware/auth.rs","symbol":"AuthMiddleware","dep_type":"Import"}
{"entity":"code_dep","from_file":"src/handlers/admin.rs","to_file":"src/middleware/auth.rs","symbol":"require_admin","dep_type":"Call"}
```

## Future: Semantic Search (Optional)

For "how does X work?" queries, embeddings provide conceptual search:

### Embedding Options for Rust

| Approach | Pros | Cons |
|----------|------|------|
| **Local model (candle + ONNX)** | No external deps, fast | Larger binary, model management |
| **External API (OpenAI, etc.)** | High quality | Requires API key, network |
| **SQLite + sqlite-vec** | Simple storage | Additional dependency |

### Minimal Embedding Schema

```rust
#[derive(Serialize, Deserialize)]
struct DocumentEmbedding {
    doc_id: String,       // e.g., "src/auth.rs:authenticate"
    content: String,      // Source text or docstring
    embedding: Vec<f32>,  // 384 or 768 dimensions
}
```

### Storage in JSONL

```jsonl
{"entity":"embedding","doc_id":"src/auth.rs:authenticate","content":"Validates JWT token and returns user...","embedding":[0.123,-0.456,...]}
```

For semantic search, load embeddings into memory and use cosine similarity, or use a dedicated vector store.

## Implementation Phases

### Phase 1: Structural Index (MVP)
- [ ] tree-sitter parsing for Rust
- [ ] Symbol extraction
- [ ] JSONL storage of FileSymbols
- [ ] Basic `rivets index` command
- [ ] Cache invalidation by mtime/size

### Phase 2: Dependency Graph
- [ ] Import analysis
- [ ] CodeDependency records in JSONL
- [ ] petgraph integration
- [ ] `rivets deps` and `rivets blast-radius` commands

### Phase 3: Issue Integration
- [ ] `--code-context` flag for `rivets show`
- [ ] Link issues to files
- [ ] Automatic context enrichment

### Phase 4: Multi-Language Support
- [ ] TypeScript/JavaScript
- [ ] Python
- [ ] Go

### Phase 5: Semantic Search (Optional)
- [ ] Embedding generation
- [ ] Vector similarity search
- [ ] "How does X work?" queries

## References

- [tree-sitter](https://tree-sitter.github.io/) - Incremental parsing library
- [tree-sitter-rust](https://github.com/tree-sitter/tree-sitter-rust) - Rust grammar
- [petgraph](https://docs.rs/petgraph/) - Graph data structure library
- [Claude OS Hybrid Indexing](../../../claude-os/docs/HYBRID_INDEXING_DESIGN.md) - Inspiration for this design
