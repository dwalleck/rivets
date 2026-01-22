# Tethys: Code Intelligence Library

**Status:** Design
**Last Updated:** 2026-01-22

## Overview

Tethys is a Rust library for code intelligence - extracting symbols, building dependency graphs, and answering questions like "what calls this function?" and "what's the blast radius of changing this file?"

Tethys provides the *data*. What consumers do with it is up to them:

| Consumer | Use Case |
|----------|----------|
| **Rivets** | Enrich issues with code context, show blast radius |
| **Claude Code Hook** | Warn about callers before modifying a function |
| **MCP Server** | Let AI agents query symbol references |
| **Catalyst Agents** | Help `code-refactor-master` track dependencies |

## Design Principles

1. **Intelligence, not policy** - Tethys reports facts ("12 callers"), not judgments ("too risky")
2. **Layered accuracy** - Fast approximate results (tree-sitter), optional precision (LSP)
3. **Language extensible** - Start with Rust, design for adding C#, TypeScript, etc.
4. **Embeddable** - Library first, CLI second, MCP optional

## Architecture

### Workspace Structure

```
rivets/
├── crates/
│   ├── rivets/           # Issue tracking (depends on tethys)
│   ├── tethys/           # Code intelligence (standalone)
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── parser.rs       # tree-sitter parsing
│   │   │   ├── symbols.rs      # Symbol extraction per language
│   │   │   ├── resolver.rs     # Module/import resolution
│   │   │   ├── graph.rs        # Dependency graph (petgraph)
│   │   │   ├── lsp.rs          # Optional LSP refinement
│   │   │   └── languages/
│   │   │       ├── mod.rs
│   │   │       ├── rust.rs
│   │   │       └── csharp.rs
│   │   └── Cargo.toml
│   └── rivets-mcp/       # MCP server (uses both)
```

### Core Types

```rust
// tethys/src/lib.rs

/// A code symbol (function, struct, trait, class, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub file: PathBuf,
    pub line: u32,
    pub signature: String,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Method,
    Struct,
    Class,
    Enum,
    Trait,
    Interface,
    Const,
    Static,
    Module,
    TypeAlias,
    Macro,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Visibility {
    Public,
    Crate,      // pub(crate) in Rust, internal in C#
    Module,     // pub(super), pub(in path) in Rust
    Private,
}

/// A dependency from one file to another
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub from_file: PathBuf,
    pub to_file: PathBuf,
    pub symbol: String,
    pub kind: DependencyKind,
    pub line: u32,  // Line in from_file where dependency occurs
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum DependencyKind {
    Import,     // use statement, using directive
    Call,       // Function/method call
    Type,       // Type reference in signature or variable
    Inherit,    // Trait impl, class inheritance
    Construct,  // Struct literal, new ClassName()
}

/// Analysis results for a single file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAnalysis {
    pub path: PathBuf,
    pub language: String,
    pub mtime: f64,
    pub size: u64,
    pub symbols: Vec<Symbol>,
    pub dependencies: Vec<Dependency>,
}

/// Query results for blast radius
#[derive(Debug, Clone)]
pub struct BlastRadius {
    pub target: PathBuf,
    pub direct_dependents: Vec<Dependent>,
    pub transitive_dependents: Vec<Dependent>,
}

#[derive(Debug, Clone)]
pub struct Dependent {
    pub file: PathBuf,
    pub symbols_used: Vec<String>,
    pub line_count: usize,  // How many lines reference the target
}
```

### Public API

```rust
// tethys/src/lib.rs

pub struct Tethys {
    index: Index,
    graph: DependencyGraph,
    lsp: Option<LspRefinement>,
}

impl Tethys {
    /// Create a new Tethys instance for a workspace
    pub fn new(workspace_root: &Path) -> Result<Self>;

    /// Create with LSP refinement enabled
    pub fn with_lsp(workspace_root: &Path, lsp_command: &str) -> Result<Self>;

    // === Indexing ===

    /// Index all source files in the workspace
    pub fn index(&mut self) -> Result<IndexStats>;

    /// Incrementally update index for changed files
    pub fn update(&mut self) -> Result<IndexUpdate>;

    /// Check if index is stale
    pub fn is_stale(&self) -> bool;

    // === Symbol Queries ===

    /// Search for symbols by name (fuzzy matching)
    pub fn search_symbols(&self, query: &str) -> Vec<Symbol>;

    /// Get all symbols in a file
    pub fn symbols_in_file(&self, path: &Path) -> Vec<Symbol>;

    /// Find symbol definition by name
    pub fn find_symbol(&self, name: &str) -> Option<Symbol>;

    // === Dependency Queries ===

    /// Get all files that depend on the given file
    pub fn get_dependents(&self, path: &Path) -> Vec<PathBuf>;

    /// Get all files that the given file depends on
    pub fn get_dependencies(&self, path: &Path) -> Vec<PathBuf>;

    /// Get callers of a specific symbol
    pub fn get_callers(&self, symbol: &str) -> Vec<Dependent>;

    /// Get full blast radius (transitive dependents)
    pub fn get_blast_radius(&self, path: &Path) -> BlastRadius;

    /// Get blast radius for a specific symbol
    pub fn get_symbol_blast_radius(&self, symbol: &str) -> BlastRadius;

    // === Export ===

    /// Export index to JSONL format
    pub fn export_jsonl(&self, writer: impl Write) -> Result<()>;

    /// Import index from JSONL format
    pub fn import_jsonl(&mut self, reader: impl Read) -> Result<()>;
}
```

## Dependency Detection Strategy

Tree-sitter provides syntax, not semantics. Detecting dependencies requires multiple strategies with increasing accuracy.

### Level 1: Use Statement Parsing (~70% accuracy)

Parse import/use statements and resolve them to file paths.

```rust
/// A parsed use statement
#[derive(Debug)]
struct UseStatement {
    path: Vec<String>,           // ["crate", "auth", "middleware"]
    imported_names: Vec<String>, // ["AuthMiddleware", "authenticate"]
    is_glob: bool,               // use foo::*
}

fn extract_use_statements(tree: &Tree, content: &[u8]) -> Vec<UseStatement>;

/// Resolve module path to file path
/// Handles: crate::, self::, super::, and Rust's module resolution rules
fn resolve_module_path(
    module_path: &[String],
    current_file: &Path,
    crate_root: &Path,
) -> Option<PathBuf> {
    let mut path = match module_path.first().map(|s| s.as_str()) {
        Some("crate") => crate_root.to_path_buf(),
        Some("self") => current_file.parent()?.to_path_buf(),
        Some("super") => current_file.parent()?.parent()?.to_path_buf(),
        Some(_extern) => return None, // External crate - can't analyze
        None => return None,
    };

    for segment in module_path.iter().skip(1) {
        // Try Rust's resolution rules: foo.rs, foo/mod.rs, foo/
        let candidates = [
            path.join(format!("{}.rs", segment)),
            path.join(segment).join("mod.rs"),
            path.join(segment),
        ];

        path = candidates.into_iter()
            .find(|p| p.exists() || p.with_extension("rs").exists())?;
    }

    normalize_to_rs_file(path)
}
```

**Limitations:**
- Misses type annotations without explicit `use`
- Can't resolve glob imports (`use foo::*`)
- Misses method calls on imported types

### Level 2: Use + Call Site Cross-Reference (~85% accuracy)

Cross-reference imports with actual usage in code. Only record a dependency if the symbol is actually used, not just imported.

```rust
/// Maps symbol names to their source files
struct SymbolOriginMap {
    origins: HashMap<String, (PathBuf, Vec<String>)>,
}

impl SymbolOriginMap {
    fn from_uses(uses: &[UseStatement], current_file: &Path, crate_root: &Path) -> Self;
    fn lookup(&self, symbol: &str) -> Option<&(PathBuf, Vec<String>)>;
}

/// A reference to a symbol in code
#[derive(Debug)]
struct SymbolRef {
    name: String,
    kind: RefKind,
    line: u32,
}

#[derive(Debug)]
enum RefKind {
    Call,        // foo(), Bar::new()
    Type,        // x: Foo, Vec<Bar>
    Constructor, // Foo { }, new Bar()
    Scoped,      // crate::foo::bar
}

/// Extract all symbol references from code
fn extract_symbol_references(tree: &Tree, content: &[u8]) -> Vec<SymbolRef> {
    let mut refs = Vec::new();

    visit_nodes(tree, |node| {
        match node.kind() {
            "call_expression" => { /* extract function name */ }
            "type_identifier" => { /* extract type name */ }
            "scoped_identifier" => { /* extract qualified path */ }
            "struct_expression" => { /* extract constructor */ }
            _ => {}
        }
    });

    refs
}

/// Build dependencies by cross-referencing imports with usage
fn build_dependencies_l2(
    file: &Path,
    tree: &Tree,
    content: &[u8],
    crate_root: &Path,
) -> Vec<Dependency> {
    let uses = extract_use_statements(tree, content);
    let origin_map = SymbolOriginMap::from_uses(&uses, file, crate_root);
    let refs = extract_symbol_references(tree, content);

    let mut deps = Vec::new();

    for sym_ref in refs {
        if let Some((source_file, _)) = origin_map.lookup(&sym_ref.name) {
            deps.push(Dependency {
                from_file: file.to_path_buf(),
                to_file: source_file.clone(),
                symbol: sym_ref.name,
                kind: ref_kind_to_dep_kind(sym_ref.kind),
                line: sym_ref.line,
            });
        }
    }

    dedup_dependencies(deps)
}
```

**What L2 catches that L1 misses:**

| Scenario | L1 | L2 |
|----------|----|----|
| Import but never use | ❌ False positive | ✅ Correctly ignored |
| Glob import `use foo::*` | ❌ Can't track | ✅ Tracks actual usage |
| Type in function signature | ❌ Misses | ✅ Catches |
| Struct literal `Foo { }` | ❌ Misses | ✅ Catches |

**Remaining gaps:**
- Method calls on variables: `x.authenticate()` - don't know type of `x`
- Trait methods from imported traits
- Macro expansions

### Level 3: LSP Refinement (~98% accuracy)

For ambiguous cases that tree-sitter can't resolve, optionally query an LSP server.

```rust
/// Cases where tree-sitter can't determine the dependency
#[derive(Debug)]
enum AmbiguousRef {
    /// Method call on unknown type: `x.foo()`
    MethodCall { receiver: String, method: String, line: u32 },

    /// Trait method could come from multiple traits
    TraitMethod { method: String, line: u32 },

    /// Fully qualified path without use statement
    QualifiedPath { path: String, line: u32 },
}

/// Identify references that need LSP to resolve
fn find_ambiguous_refs(
    file: &Path,
    tree: &Tree,
    content: &[u8],
    origin_map: &SymbolOriginMap,
) -> Vec<AmbiguousRef>;

/// Minimal LSP client for dependency resolution
pub struct LspClient {
    process: Child,
    request_id: u64,
}

impl LspClient {
    pub async fn start(command: &str, workspace: &Path) -> Result<Self>;

    /// Ask LSP: "What is the definition of the symbol at this position?"
    pub async fn goto_definition(&mut self, file: &Path, line: u32, col: u32)
        -> Result<Option<Location>>;

    /// Ask LSP: "What references this symbol?"
    pub async fn find_references(&mut self, file: &Path, line: u32, col: u32)
        -> Result<Vec<Location>>;
}

/// Resolve ambiguous references using LSP
async fn refine_with_lsp(
    lsp: &mut LspClient,
    ambiguous: &[AmbiguousRef],
    file: &Path,
) -> Vec<Dependency> {
    let mut refined = Vec::new();

    for amb in ambiguous {
        let (line, col) = amb.position();
        if let Ok(Some(location)) = lsp.goto_definition(file, line, col).await {
            refined.push(Dependency {
                from_file: file.to_path_buf(),
                to_file: uri_to_path(&location.uri),
                symbol: extract_symbol_at(&location),
                kind: DependencyKind::Call,
                line,
            });
        }
    }

    refined
}
```

**When to use LSP:**

| Scenario | Tree-sitter | LSP |
|----------|-------------|-----|
| `use foo::Bar; Bar::new()` | ✅ | |
| `let x: Bar = ...` | ✅ | |
| `x.authenticate()` | ❌ | ✅ |
| `impl Trait for Foo` | Partial | ✅ |
| Method chaining `a.b().c()` | ❌ | ✅ |

**Performance trade-off:**

```
L1+L2 only:  ~30 seconds / 10K files
L1+L2+L3:    ~2-5 minutes / 10K files (LSP startup + queries)
```

### Recommended Implementation

```rust
impl Tethys {
    pub fn analyze_file(&mut self, file: &Path) -> Result<FileAnalysis> {
        let content = std::fs::read(file)?;
        let tree = self.parser.parse(file, &content)?;

        // L1 + L2: Fast tree-sitter analysis
        let symbols = extract_symbols(&tree, &content, self.language(file)?);
        let deps = build_dependencies_l2(file, &tree, &content, &self.crate_root);

        // L3: Optional LSP refinement
        let deps = if let Some(lsp) = &mut self.lsp {
            let ambiguous = find_ambiguous_refs(file, &tree, &content, &origin_map);
            if !ambiguous.is_empty() {
                let refined = refine_with_lsp(lsp, &ambiguous, file).await;
                merge_dependencies(deps, refined)
            } else {
                deps
            }
        } else {
            deps
        };

        Ok(FileAnalysis { path: file.to_path_buf(), symbols, dependencies: deps, ... })
    }
}
```

## Dependency Graph

Uses petgraph for graph operations:

```rust
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;

pub struct DependencyGraph {
    graph: DiGraph<PathBuf, DependencyKind>,
    node_map: HashMap<PathBuf, NodeIndex>,
}

impl DependencyGraph {
    pub fn new() -> Self;

    pub fn add_file(&mut self, path: PathBuf) -> NodeIndex;

    pub fn add_dependency(&mut self, from: &Path, to: &Path, kind: DependencyKind);

    /// Files that depend on this file (reverse dependencies)
    pub fn get_dependents(&self, path: &Path) -> Vec<PathBuf> {
        let Some(&node) = self.node_map.get(path) else { return vec![] };

        self.graph
            .neighbors_directed(node, Direction::Incoming)
            .map(|n| self.graph[n].clone())
            .collect()
    }

    /// Files this file depends on
    pub fn get_dependencies(&self, path: &Path) -> Vec<PathBuf> {
        let Some(&node) = self.node_map.get(path) else { return vec![] };

        self.graph
            .neighbors_directed(node, Direction::Outgoing)
            .map(|n| self.graph[n].clone())
            .collect()
    }

    /// Transitive dependents (full blast radius)
    pub fn get_blast_radius(&self, path: &Path) -> Vec<PathBuf> {
        let Some(&start) = self.node_map.get(path) else { return vec![] };

        let mut visited = HashSet::new();
        let mut queue = VecDeque::from([start]);

        while let Some(node) = queue.pop_front() {
            for neighbor in self.graph.neighbors_directed(node, Direction::Incoming) {
                if visited.insert(neighbor) {
                    queue.push_back(neighbor);
                }
            }
        }

        visited.iter()
            .map(|&n| self.graph[n].clone())
            .collect()
    }
}
```

## Language Support

Tethys uses a trait-based design for language support:

```rust
pub trait LanguageSupport: Send + Sync {
    /// File extensions this language handles
    fn extensions(&self) -> &[&str];

    /// tree-sitter language
    fn tree_sitter_language(&self) -> tree_sitter::Language;

    /// Extract symbols from a parsed tree
    fn extract_symbols(&self, tree: &Tree, content: &[u8]) -> Vec<Symbol>;

    /// Extract use/import statements
    fn extract_imports(&self, tree: &Tree, content: &[u8]) -> Vec<UseStatement>;

    /// Resolve import path to file path
    fn resolve_import(&self, import_path: &[String], current_file: &Path, root: &Path)
        -> Option<PathBuf>;

    /// Extract symbol references (calls, type usage, etc.)
    fn extract_references(&self, tree: &Tree, content: &[u8]) -> Vec<SymbolRef>;

    /// LSP server command (if available)
    fn lsp_command(&self) -> Option<&str>;
}

// Implementations
pub struct RustLanguage;
pub struct CSharpLanguage;

impl LanguageSupport for RustLanguage {
    fn extensions(&self) -> &[&str] { &["rs"] }
    fn tree_sitter_language(&self) -> tree_sitter::Language { tree_sitter_rust::LANGUAGE.into() }
    fn lsp_command(&self) -> Option<&str> { Some("rust-analyzer") }
    // ...
}

impl LanguageSupport for CSharpLanguage {
    fn extensions(&self) -> &[&str] { &["cs"] }
    fn tree_sitter_language(&self) -> tree_sitter::Language { tree_sitter_c_sharp::LANGUAGE.into() }
    fn lsp_command(&self) -> Option<&str> { Some("csharp-ls") }
    // ...
}
```

## Storage

Tethys can export/import its index as JSONL for persistence:

```jsonl
{"entity":"file","path":"src/auth.rs","language":"rust","mtime":1735123456.78,"size":2048}
{"entity":"symbol","file":"src/auth.rs","name":"authenticate","kind":"Function","line":42,"signature":"fn authenticate(token: &str) -> Result<User>","visibility":"Public"}
{"entity":"symbol","file":"src/auth.rs","name":"AuthMiddleware","kind":"Struct","line":15,"signature":"struct AuthMiddleware","visibility":"Public"}
{"entity":"dep","from":"src/routes/api.rs","to":"src/auth.rs","symbol":"AuthMiddleware","kind":"Import","line":3}
{"entity":"dep","from":"src/handlers/login.rs","to":"src/auth.rs","symbol":"authenticate","kind":"Call","line":27}
```

This format is:
- Human readable
- Git-friendly (line-based diffs)
- Compatible with rivets' existing JSONL approach
- Streamable (can process large indexes without loading entirely into memory)

## CLI (Optional)

Tethys can include a CLI for standalone use:

```bash
# Index a workspace
tethys index

# Search for symbols
tethys search "authenticate"

# Get callers of a symbol
tethys callers "AuthMiddleware::new"

# Get blast radius for a file
tethys blast-radius src/auth.rs

# Get blast radius for a symbol
tethys blast-radius --symbol "authenticate"

# Export index
tethys export > index.jsonl

# Use LSP for precision
tethys index --lsp
tethys callers "validate" --lsp
```

## Consumer Examples

### Rivets Integration

```rust
// In rivets, when showing an issue
fn show_issue_with_context(issue: &Issue, tethys: &Tethys) {
    println!("{}: {}", issue.id, issue.title);

    if let Some(files) = &issue.related_files {
        for file in files {
            let blast = tethys.get_blast_radius(file);
            println!("\nBlast radius for {}:", file.display());
            println!("  Direct: {} files", blast.direct_dependents.len());
            println!("  Transitive: {} files", blast.transitive_dependents.len());
        }
    }
}
```

### Claude Code Hook

```rust
// Hook that warns about callers before editing
fn pre_edit_hook(file: &Path, function_name: &str, tethys: &Tethys) -> HookResult {
    let callers = tethys.get_callers(function_name);

    if !callers.is_empty() {
        let warning = format!(
            "⚠️  `{}` has {} callers across {} files:\n{}",
            function_name,
            callers.iter().map(|c| c.line_count).sum::<usize>(),
            callers.len(),
            callers.iter()
                .take(5)
                .map(|c| format!("  • {}", c.file.display()))
                .collect::<Vec<_>>()
                .join("\n")
        );

        HookResult::Continue { message: Some(warning) }
    } else {
        HookResult::Continue { message: None }
    }
}
```

### MCP Server

```rust
// MCP tool: get_callers
async fn handle_get_callers(params: GetCallersParams, tethys: &Tethys) -> McpResult {
    let callers = tethys.get_callers(&params.symbol);

    McpResult::json(json!({
        "symbol": params.symbol,
        "caller_count": callers.len(),
        "callers": callers.iter().map(|c| json!({
            "file": c.file,
            "symbols_used": c.symbols_used,
            "line_count": c.line_count,
        })).collect::<Vec<_>>()
    }))
}
```

## Implementation Phases

### Phase 1: Core Infrastructure
- [ ] Workspace crate structure
- [ ] `Symbol`, `Dependency`, `FileAnalysis` types
- [ ] tree-sitter integration
- [ ] Rust symbol extraction
- [ ] Basic `Tethys` struct with `index()` and `symbols_in_file()`

### Phase 2: Dependency Detection (L1 + L2)
- [ ] Use statement parsing for Rust
- [ ] Module path resolution
- [ ] Symbol reference extraction
- [ ] Cross-reference to build dependencies
- [ ] `get_dependents()`, `get_dependencies()`

### Phase 3: Dependency Graph
- [ ] petgraph integration
- [ ] `get_blast_radius()` with BFS
- [ ] `get_callers()` for symbol-level queries
- [ ] JSONL export/import

### Phase 4: CLI
- [ ] `tethys index`
- [ ] `tethys search`
- [ ] `tethys callers`
- [ ] `tethys blast-radius`

### Phase 5: C# Support
- [ ] tree-sitter-c-sharp integration
- [ ] C# symbol extraction
- [ ] C# using directive parsing
- [ ] Namespace resolution

### Phase 6: LSP Refinement (L3)
- [ ] Minimal LSP client
- [ ] Ambiguous reference detection
- [ ] `--lsp` flag for precision mode
- [ ] rust-analyzer integration
- [ ] OmniSharp/csharp-ls integration

### Phase 7: Rivets Integration
- [ ] Rivets depends on tethys
- [ ] `rivets show --code-context`
- [ ] Issue ↔ file linking

## Dependencies

```toml
[dependencies]
tree-sitter = "0.24"
tree-sitter-rust = "0.23"
tree-sitter-c-sharp = "0.23"
petgraph = "0.6"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "2.0"
tracing = "0.1"

[dev-dependencies]
tempfile = "3"
rstest = "0.23"
```

## References

- [tree-sitter](https://tree-sitter.github.io/) - Incremental parsing library
- [petgraph](https://docs.rs/petgraph/) - Graph data structure library
- [LSP Specification](https://microsoft.github.io/language-server-protocol/) - Language Server Protocol
- [rust-analyzer](https://rust-analyzer.github.io/) - Rust LSP server
- [ra_ap_ide](https://docs.rs/ra_ap_ide/) - rust-analyzer as a library
