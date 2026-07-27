# C# Integration into Tethys Indexer — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Wire existing C# extraction functions into the Tethys indexer so `.cs` files are indexed alongside Rust files.

**Architecture:** Unify the per-language extraction types (`ExtractedSymbol`, `ExtractedReference`, import types) into common types in the `LanguageSupport` trait. Update `index_file()` to dispatch through the trait instead of calling `rust::*` directly. The parser switches tree-sitter language based on file extension.

**Tech Stack:** tree-sitter, tree-sitter-c-sharp, rusqlite

---

## Current State

- `csharp.rs`: 930+ lines with `extract_symbols()`, `extract_references()`, `extract_using_directives()` — all tested (33 tests)
- `rust.rs`: Identical extraction API shape — same function signatures, same output types (but separate type definitions)
- `lib.rs`: Hardcoded to Rust — skips non-Rust files, calls `rust::*` directly
- `LanguageSupport` trait: Only has `extensions()`, `tree_sitter_language()`, `lsp_command()`

## Key Design Decision

Both `rust.rs` and `csharp.rs` define identical types: `ExtractedSymbol`, `ExtractedReference`, `ExtractedReferenceKind`. Rather than creating a *third* copy in the trait, we'll **move these types to a common location** (`languages/mod.rs`) and have both language modules re-use them. The `UseStatement` (Rust) and `UsingDirective` (C#) differ in structure, so we'll introduce a common `ImportStatement` type.

---

### Task 1: Unify Extracted Types into `languages/mod.rs`

**Files:**
- Create: `crates/tethys/src/languages/common.rs`
- Modify: `crates/tethys/src/languages/mod.rs`
- Modify: `crates/tethys/src/languages/rust.rs`
- Modify: `crates/tethys/src/languages/csharp.rs`
- Modify: `crates/tethys/src/lib.rs` (update imports)

**What:** Move `ExtractedSymbol`, `ExtractedReference`, `ExtractedReferenceKind` into `common.rs`. Both `rust.rs` and `csharp.rs` will import from there. Add a unified `ImportStatement` type that can represent both Rust `use` and C# `using`.

**Step 1: Create `common.rs` with shared extraction types**

Create `crates/tethys/src/languages/common.rs` containing:

```rust
//! Common extraction types shared across language implementations.
//!
//! These types represent the output of tree-sitter extraction before
//! being stored in the database. Each language module produces these
//! types from its language-specific AST traversal.

use crate::types::{FunctionSignature, ReferenceKind, Span, SymbolKind, Visibility};

/// An extracted symbol from source code (language-agnostic).
#[derive(Debug, Clone)]
pub struct ExtractedSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub line: u32,
    pub column: u32,
    pub span: Option<Span>,
    pub signature: Option<String>,
    pub signature_details: Option<FunctionSignature>,
    pub visibility: Visibility,
    pub parent_name: Option<String>,
}

/// An extracted reference (usage of a symbol) from source code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedReference {
    /// Name of the referenced symbol
    pub name: String,
    /// Kind of reference
    pub kind: ExtractedReferenceKind,
    /// Line number (1-indexed)
    pub line: u32,
    /// Column number (1-indexed)
    pub column: u32,
    /// The scoped path if this is a qualified reference
    pub path: Option<Vec<String>>,
    /// Span of the containing symbol for "who calls X?" queries
    pub containing_symbol_span: Option<Span>,
}

/// Kind of reference extracted from source code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractedReferenceKind {
    /// Function or method call
    Call,
    /// Type annotation
    Type,
    /// Constructor (struct literal, `new` expression)
    Constructor,
}

impl ExtractedReferenceKind {
    /// Convert to database reference kind.
    #[must_use]
    pub fn to_db_kind(self) -> ReferenceKind {
        match self {
            Self::Call => ReferenceKind::Call,
            Self::Type => ReferenceKind::Type,
            Self::Constructor => ReferenceKind::Construct,
        }
    }
}

/// A unified import statement (covers Rust `use` and C# `using`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportStatement {
    /// Path segments (e.g., `["crate", "auth"]` or `["System", "Collections"]`)
    pub path: Vec<String>,
    /// Names being imported (e.g., `["HashMap"]`). Empty for namespace imports.
    pub imported_names: Vec<String>,
    /// Whether this is a glob/wildcard import
    pub is_glob: bool,
    /// Alias if present
    pub alias: Option<String>,
    /// Line number (1-indexed)
    pub line: u32,
}
```

**Step 2: Update `rust.rs` to use common types**

Replace the `ExtractedSymbol`, `ExtractedReference`, `ExtractedReferenceKind` definitions in `rust.rs` with imports from `common`:

```rust
// At top of rust.rs, replace local type definitions with:
use super::common::{ExtractedReference, ExtractedReferenceKind, ExtractedSymbol, ImportStatement};
```

Remove the local `ExtractedSymbol`, `ExtractedReference`, `ExtractedReferenceKind` structs/enums and the `to_db_kind` impl.

Keep `UseStatement` for now but add a conversion method:

```rust
impl UseStatement {
    /// Convert to the common ImportStatement type.
    pub fn to_import_statement(&self) -> ImportStatement {
        ImportStatement {
            path: self.path.clone(),
            imported_names: self.imported_names.clone(),
            is_glob: self.is_glob,
            alias: self.alias.clone(),
            line: self.line,
        }
    }
}
```

**Step 3: Update `csharp.rs` to use common types**

Same pattern — replace local `ExtractedSymbol`, `ExtractedReference`, `ExtractedReferenceKind` with imports from `common`. Remove local definitions.

Add conversion for `UsingDirective`:

```rust
impl UsingDirective {
    /// Convert to the common ImportStatement type.
    pub fn to_import_statement(&self) -> ImportStatement {
        ImportStatement {
            path: self.namespace.clone(),
            imported_names: vec![], // C# using imports entire namespaces
            is_glob: false,
            alias: self.alias.clone(),
            line: self.line,
        }
    }
}
```

**Step 4: Update `languages/mod.rs` to export common types**

```rust
pub mod common;
pub mod csharp;
pub mod rust;
mod tree_sitter_utils;
```

**Step 5: Update `lib.rs` imports**

Replace `rust::ExtractedReference` references with `languages::common::ExtractedReference`.

**Step 6: Run tests**

```bash
cd crates/tethys && cargo test
```

Expected: All 273+ tests pass. No behavior change.

**Step 7: Commit**

```bash
git add crates/tethys/src/languages/
git add crates/tethys/src/lib.rs
git commit -m "refactor(tethys): unify extraction types into languages/common.rs"
```

---

### Task 2: Add Extraction Methods to `LanguageSupport` Trait

**Files:**
- Modify: `crates/tethys/src/languages/mod.rs`
- Modify: `crates/tethys/src/languages/rust.rs`
- Modify: `crates/tethys/src/languages/csharp.rs`

**What:** Add `extract_symbols()`, `extract_references()`, and `extract_imports()` methods to the `LanguageSupport` trait, then implement them for both languages.

**Step 1: Update the trait**

In `languages/mod.rs`, add to the `LanguageSupport` trait:

```rust
use common::{ExtractedReference, ExtractedSymbol, ImportStatement};

pub trait LanguageSupport: Send + Sync {
    /// File extensions this language handles.
    fn extensions(&self) -> &[&str];

    /// Get the tree-sitter language for parsing.
    fn tree_sitter_language(&self) -> tree_sitter::Language;

    /// LSP server command, if available.
    fn lsp_command(&self) -> Option<&str>;

    /// Extract symbols from a parsed syntax tree.
    fn extract_symbols(&self, tree: &tree_sitter::Tree, content: &[u8]) -> Vec<ExtractedSymbol>;

    /// Extract references (usages) from a parsed syntax tree.
    fn extract_references(&self, tree: &tree_sitter::Tree, content: &[u8]) -> Vec<ExtractedReference>;

    /// Extract import statements from a parsed syntax tree.
    fn extract_imports(&self, tree: &tree_sitter::Tree, content: &[u8]) -> Vec<ImportStatement>;
}
```

**Step 2: Implement for `RustLanguage`**

In `rust.rs`, add to the `impl LanguageSupport for RustLanguage` block:

```rust
fn extract_symbols(&self, tree: &tree_sitter::Tree, content: &[u8]) -> Vec<ExtractedSymbol> {
    extract_symbols(tree, content)
}

fn extract_references(&self, tree: &tree_sitter::Tree, content: &[u8]) -> Vec<ExtractedReference> {
    extract_references(tree, content)
}

fn extract_imports(&self, tree: &tree_sitter::Tree, content: &[u8]) -> Vec<ImportStatement> {
    extract_use_statements(tree, content)
        .into_iter()
        .map(|u| u.to_import_statement())
        .collect()
}
```

**Step 3: Implement for `CSharpLanguage`**

In `csharp.rs`, add to the `impl LanguageSupport for CSharpLanguage` block:

```rust
fn extract_symbols(&self, tree: &tree_sitter::Tree, content: &[u8]) -> Vec<ExtractedSymbol> {
    extract_symbols(tree, content)
}

fn extract_references(&self, tree: &tree_sitter::Tree, content: &[u8]) -> Vec<ExtractedReference> {
    extract_references(tree, content)
}

fn extract_imports(&self, tree: &tree_sitter::Tree, content: &[u8]) -> Vec<ImportStatement> {
    extract_using_directives(tree, content)
        .into_iter()
        .map(|u| u.to_import_statement())
        .collect()
}
```

**Step 4: Run tests**

```bash
cd crates/tethys && cargo test
```

Expected: All tests pass.

**Step 5: Commit**

```bash
git add crates/tethys/src/languages/
git commit -m "feat(tethys): add extraction methods to LanguageSupport trait"
```

---

### Task 3: Make the Parser Language-Aware

**Files:**
- Modify: `crates/tethys/src/lib.rs`

**What:** Currently `Tethys::new()` initializes the parser with only `tree_sitter_rust::LANGUAGE`. We need to switch the parser's language before parsing each file based on its `Language`.

**Step 1: Remove Rust-only parser initialization**

In `Tethys::new()`, change the parser setup — don't set a language at construction time. Instead, we'll set it per-file in `index_file()`.

Replace lines 111-114:
```rust
let mut parser = tree_sitter::Parser::new();
parser
    .set_language(&tree_sitter_rust::LANGUAGE.into())
    .map_err(|e| Error::Parser(e.to_string()))?;
```

With just:
```rust
let parser = tree_sitter::Parser::new();
```

**Step 2: Set parser language per-file in `index_file()`**

At the beginning of `index_file()`, before parsing, set the language:

```rust
fn index_file(
    &mut self,
    path: &Path,
    language: Language,
    pending: &mut Vec<PendingDependency>,
) -> Result<(usize, usize)> {
    let content = std::fs::read(path)?;
    let content_str = std::str::from_utf8(&content)
        .map_err(|_| Error::Parser("file is not valid UTF-8".to_string()))?;

    // Get language support for extraction
    let lang_support = languages::get_language_support(language)
        .ok_or_else(|| Error::Parser(format!("no support for language: {language:?}")))?;

    // Set parser to the correct tree-sitter language
    self.parser
        .set_language(&lang_support.tree_sitter_language())
        .map_err(|e| Error::Parser(e.to_string()))?;

    // ... rest of method
```

**Step 3: Run tests**

```bash
cd crates/tethys && cargo test
```

Expected: All tests pass.

**Step 4: Commit**

```bash
git add crates/tethys/src/lib.rs
git commit -m "refactor(tethys): make parser language-aware per file"
```

---

### Task 4: Dispatch Through `LanguageSupport` Trait in `index_file()`

**Files:**
- Modify: `crates/tethys/src/lib.rs`

**What:** Replace direct `rust::extract_*` calls with trait dispatch, and remove the `Language::Rust` skip guard.

**Step 1: Remove the Rust-only skip guard**

In `index()`, remove lines 171-175:
```rust
// Only Rust is implemented for now
if language != Language::Rust {
    files_skipped += 1;
    continue;
}
```

**Step 2: Replace `rust::*` calls with trait dispatch**

In `index_file()`, replace:
```rust
let extracted = rust::extract_symbols(&tree, content_str.as_bytes());
let uses = rust::extract_use_statements(&tree, content_str.as_bytes());
let refs = rust::extract_references(&tree, content_str.as_bytes());
```

With:
```rust
let extracted = lang_support.extract_symbols(&tree, content_str.as_bytes());
let imports = lang_support.extract_imports(&tree, content_str.as_bytes());
let refs = lang_support.extract_references(&tree, content_str.as_bytes());
```

**Step 3: Update `store_references` to use common type**

Change the signature from `&[rust::ExtractedReference]` to `&[languages::common::ExtractedReference]`:

```rust
fn store_references(
    &self,
    file_id: i64,
    refs: &[languages::common::ExtractedReference],
    name_to_id: &HashMap<String, i64>,
    span_to_id: &HashMap<Span, i64>,
) -> Result<usize> {
```

**Step 4: Update `compute_dependencies` to use `ImportStatement`**

Change the signature from `&[rust::UseStatement]` to `&[languages::common::ImportStatement]`. Update internal field access — `UseStatement` fields map directly to `ImportStatement` fields (same names).

```rust
fn compute_dependencies(
    &self,
    current_file: &Path,
    file_id: i64,
    imports: &[languages::common::ImportStatement],
    refs: &[languages::common::ExtractedReference],
    pending: &mut Vec<PendingDependency>,
) -> Result<()> {
```

Update the `uses` variable references to `imports`, and `use_stmt` to `import_stmt` throughout the method body.

**Step 5: Update the call in `index_file()`**

```rust
self.compute_dependencies(path, file_id, &imports, &refs, pending)?;
```

**Step 6: Remove the direct `use languages::rust` import from lib.rs**

Replace:
```rust
use languages::rust;
```
With:
```rust
use languages::common::ExtractedReference;
```

(Or just use the full path `languages::common::ExtractedReference` where needed.)

**Step 7: Run tests**

```bash
cd crates/tethys && cargo test
```

Expected: All existing tests pass. C# files are no longer skipped during indexing.

**Step 8: Commit**

```bash
git add crates/tethys/src/lib.rs
git commit -m "feat(tethys): dispatch indexing through LanguageSupport trait

C# files are now indexed alongside Rust files. The indexer uses
trait-based dispatch instead of hardcoded rust::* calls."
```

---

### Task 5: Add `resolve_import` to `LanguageSupport` Trait

**Files:**
- Modify: `crates/tethys/src/languages/mod.rs`
- Modify: `crates/tethys/src/languages/rust.rs`
- Modify: `crates/tethys/src/languages/csharp.rs`

**What:** Add an `resolve_import` method to the trait that maps import paths to file paths. Rust delegates to the existing `resolver::resolve_module_path()`. C# will use a new namespace-to-file resolution strategy (Task 5b).

**Step 1: Add the method to the trait**

In `languages/mod.rs`:

```rust
pub trait LanguageSupport: Send + Sync {
    // ... existing methods ...

    /// Resolve an import path to a file path within the workspace.
    ///
    /// Returns `None` for external/unresolvable imports.
    fn resolve_import(
        &self,
        import_path: &[String],
        current_file: &Path,
        workspace_root: &Path,
    ) -> Option<PathBuf>;
}
```

**Step 2: Implement for `RustLanguage`**

In `rust.rs`:

```rust
fn resolve_import(
    &self,
    import_path: &[String],
    current_file: &Path,
    workspace_root: &Path,
) -> Option<PathBuf> {
    let crate_root = workspace_root.join("src");
    crate::resolver::resolve_module_path(import_path, current_file, &crate_root)
}
```

**Step 3: Implement stub for `CSharpLanguage`**

In `csharp.rs` (placeholder — Task 5b fills this in):

```rust
fn resolve_import(
    &self,
    _import_path: &[String],
    _current_file: &Path,
    _workspace_root: &Path,
) -> Option<PathBuf> {
    // C# namespace resolution implemented in Task 5b
    None
}
```

**Step 4: Update `compute_dependencies` to use the trait**

In `lib.rs`, change `compute_dependencies` to accept a `&dyn LanguageSupport` and call `resolve_import` instead of `resolve_module_path`:

```rust
fn compute_dependencies(
    &self,
    current_file: &Path,
    file_id: i64,
    lang_support: &dyn languages::LanguageSupport,
    imports: &[languages::common::ImportStatement],
    refs: &[languages::common::ExtractedReference],
    pending: &mut Vec<PendingDependency>,
) -> Result<()> {
    // ... existing logic, but replace:
    //   resolve_module_path(&import_stmt.path, current_file, &crate_root)
    // with:
    //   lang_support.resolve_import(&import_stmt.path, current_file, &self.workspace_root)
```

**Step 5: Run tests**

```bash
cd crates/tethys && cargo test
```

Expected: All tests pass. C# files index symbols/references but return no file-level deps yet.

**Step 6: Commit**

```bash
git add crates/tethys/src/languages/ crates/tethys/src/lib.rs
git commit -m "refactor(tethys): add resolve_import to LanguageSupport trait"
```

---

### Task 5b: Implement C# Namespace Resolution

**Files:**
- Modify: `crates/tethys/src/languages/csharp.rs`
- Modify: `crates/tethys/src/lib.rs` (build namespace map)
- Test: `crates/tethys/tests/indexing.rs`

**What:** C# namespace resolution works differently from Rust module resolution:

- **Rust**: `use crate::auth::middleware` → maps to `src/auth/middleware.rs` via filesystem conventions
- **C#**: `using MyApp.Services` → maps to *whichever files declare `namespace MyApp.Services`*

This requires a two-pass approach:
1. **First pass (during indexing)**: Build a `namespace → [file_ids]` map from `namespace` declarations in all `.cs` files
2. **Resolution**: When a `using MyApp.Services` import is encountered, look up which files declare that namespace

Since we already do multi-pass indexing for Rust (to handle circular deps), we extend this: after the first pass indexes all files, C# namespace resolution runs as a second pass using the namespace map built from extracted symbols.

**Step 1: Write the failing test**

Add to `crates/tethys/tests/indexing.rs`:

```rust
#[test]
fn csharp_namespace_dependency_resolution() {
    let service_code = r"
namespace MyApp.Services;

public class UserService {
    public void Save() { }
}
";
    let controller_code = r"
using MyApp.Services;

namespace MyApp.Controllers;

public class UserController {
    public void Create() {
        var svc = new UserService();
        svc.Save();
    }
}
";
    let (_dir, mut tethys) = workspace_with_files(&[
        ("Services/UserService.cs", service_code),
        ("Controllers/UserController.cs", controller_code),
    ]);
    let stats = tethys.index().expect("index failed");
    assert_eq!(stats.files_indexed, 2);

    // UserController.cs depends on UserService.cs via `using MyApp.Services`
    let deps = tethys
        .get_dependencies(
            &_dir.path().join("Controllers/UserController.cs"),
        )
        .expect("get_dependencies failed");

    assert_eq!(deps.len(), 1, "should have 1 dependency");
    assert!(
        deps[0].ends_with("Services/UserService.cs"),
        "should depend on UserService.cs, got: {:?}",
        deps[0]
    );
}

#[test]
fn csharp_namespace_shared_by_multiple_files() {
    let model_a = r"
namespace MyApp.Models;
public class User { }
";
    let model_b = r"
namespace MyApp.Models;
public class Order { }
";
    let consumer = r"
using MyApp.Models;
namespace MyApp.Services;
public class Service {
    public void Run() {
        var u = new User();
        var o = new Order();
    }
}
";
    let (_dir, mut tethys) = workspace_with_files(&[
        ("Models/User.cs", model_a),
        ("Models/Order.cs", model_b),
        ("Services/Service.cs", consumer),
    ]);
    tethys.index().expect("index failed");

    let deps = tethys
        .get_dependencies(&_dir.path().join("Services/Service.cs"))
        .expect("get_dependencies failed");

    // Should depend on both files that declare the MyApp.Models namespace
    assert_eq!(deps.len(), 2, "should depend on both model files");
}
```

**Step 2: Run test to verify it fails**

```bash
cd crates/tethys && cargo test csharp_namespace -- --nocapture
```

Expected: FAIL — deps are empty because `resolve_import` returns `None` for C#.

**Step 3: Build namespace map during indexing**

In `lib.rs`, after the first indexing pass, build a map of `namespace_name → Vec<file_id>` from the indexed C# symbols:

```rust
/// Build a namespace-to-file map from indexed C# Module symbols.
///
/// This enables C# `using` directive resolution: `using MyApp.Services`
/// resolves to whichever files declare `namespace MyApp.Services`.
fn build_namespace_map(&self) -> Result<HashMap<String, Vec<i64>>> {
    let mut map: HashMap<String, Vec<i64>> = HashMap::new();

    // Query all Module-kind symbols (namespaces) from C# files
    let symbols = self.db.search_symbols_by_kind("module", 10000)?;

    for sym in symbols {
        // Only include C# files (Rust modules use different resolution)
        if let Some(file) = self.db.get_file_by_id(sym.file_id)? {
            if file.language == Language::CSharp {
                map.entry(sym.name.clone())
                    .or_default()
                    .push(sym.file_id);
            }
        }
    }

    Ok(map)
}
```

Note: This requires adding a `search_symbols_by_kind` method to `db.rs`, or reusing/extending an existing query. Alternatively, we can query all symbols with `kind = 'module'` and `language = 'csharp'` via a new DB method.

**Step 4: Add `search_symbols_by_kind` to `db.rs`**

```rust
/// Search symbols by kind.
pub fn search_symbols_by_kind(&self, kind: &str, limit: usize) -> Result<Vec<Symbol>> {
    let mut stmt = self.conn.prepare_cached(
        "SELECT s.id, s.file_id, s.name, s.module_path, s.qualified_name,
                s.kind, s.line, s.column, s.end_line, s.end_column,
                s.signature, s.visibility, s.parent_symbol_id
         FROM symbols s
         WHERE s.kind = ?1
         LIMIT ?2"
    )?;

    let symbols = stmt.query_map(params![kind, limit as i64], |row| {
        Self::row_to_symbol(row)
    })?
    .filter_map(|r| r.ok())
    .collect();

    Ok(symbols)
}
```

**Step 5: Implement `resolve_import` for `CSharpLanguage`**

The C# resolver needs access to the namespace map, which is built *after* indexing. Rather than passing the map through the trait, we use a different approach: after the first indexing pass, run a C# namespace resolution pass that uses the namespace map.

Add to `lib.rs` in the `index()` method, after the first pass and before the Rust dependency resolution passes:

```rust
// C# namespace resolution pass: resolve using directives via namespace map
let namespace_map = self.build_namespace_map()?;
if !namespace_map.is_empty() {
    self.resolve_csharp_dependencies(&namespace_map)?;
}
```

Then implement:

```rust
/// Resolve C# file dependencies using namespace-to-file mapping.
///
/// For each C# file, look at its `using` directives and find which files
/// declare those namespaces. Record file-level dependencies.
fn resolve_csharp_dependencies(
    &self,
    namespace_map: &HashMap<String, Vec<i64>>,
) -> Result<()> {
    // Get all C# files
    let csharp_files = self.db.get_files_by_language(Language::CSharp)?;

    for file in &csharp_files {
        // Get the using directives for this file by re-parsing
        // (or we could store them — but re-parsing is simpler for now)
        let full_path = self.workspace_root.join(&file.path);
        let content = match std::fs::read(&full_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let content_str = match std::str::from_utf8(&content) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let lang_support = languages::get_language_support(Language::CSharp).unwrap();
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&lang_support.tree_sitter_language())
            .ok();
        let Some(tree) = parser.parse(content_str, None) else {
            continue;
        };

        let imports = lang_support.extract_imports(&tree, content_str.as_bytes());

        for import in &imports {
            // Join path segments to form namespace name: ["MyApp", "Services"] → "MyApp.Services"
            let namespace = import.path.join(".");

            if let Some(file_ids) = namespace_map.get(&namespace) {
                for &dep_file_id in file_ids {
                    // Don't add self-dependency
                    if dep_file_id != file.id {
                        self.db.insert_file_dependency(file.id, dep_file_id)?;
                    }
                }
            }
        }
    }

    Ok(())
}
```

**Step 6: Add `get_files_by_language` to `db.rs`**

```rust
/// Get all files of a specific language.
pub fn get_files_by_language(&self, language: Language) -> Result<Vec<IndexedFile>> {
    let lang_str = language.as_str();
    let mut stmt = self.conn.prepare_cached(
        "SELECT id, path, language, mtime_ns, size_bytes, content_hash, indexed_at
         FROM files WHERE language = ?1"
    )?;

    let files = stmt.query_map(params![lang_str], |row| {
        Self::row_to_file(row)
    })?
    .filter_map(|r| r.ok())
    .collect();

    Ok(files)
}
```

**Step 7: Run tests**

```bash
cd crates/tethys && cargo test csharp_namespace -- --nocapture
```

Expected: Both namespace resolution tests pass.

**Step 8: Run full test suite**

```bash
cd crates/tethys && cargo test
```

Expected: All tests pass.

**Step 9: Commit**

```bash
git add crates/tethys/src/
git add crates/tethys/tests/
git commit -m "feat(tethys): implement C# namespace-to-file dependency resolution

C# using directives (e.g., using MyApp.Services) now resolve to file
dependencies by mapping namespace declarations to the files that contain
them. Uses a two-pass approach: first pass indexes all files, second pass
builds namespace map and resolves dependencies."
```

---

### Task 6: Add Integration Tests for C# Indexing

**Files:**
- Modify: `crates/tethys/tests/indexing.rs`

**What:** Add integration tests that verify C# files are indexed, symbols are extracted, and references are stored.

**Step 1: Write tests**

Add to `crates/tethys/tests/indexing.rs`:

```rust
// ============================================================================
// C# Indexing Tests
// ============================================================================

#[test]
fn indexes_csharp_class() {
    let code = r"
public class UserService {
    public void Save(User user) { }
}
";
    let (_dir, mut tethys) = workspace_with_files(&[("UserService.cs", code)]);
    let stats = tethys.index().expect("index failed");

    assert_eq!(stats.files_indexed, 1, "should index 1 C# file");
    assert!(stats.symbols_found >= 2, "should find class + method");
}

#[test]
fn indexes_csharp_symbols() {
    let code = r"
namespace MyApp.Services;

public class Calculator {
    public int Add(int a, int b) { return a + b; }
    public static int Multiply(int a, int b) { return a * b; }
}

public interface ICalculator {
    int Add(int a, int b);
}
";
    let (_dir, mut tethys) = workspace_with_files(&[("Calculator.cs", code)]);
    tethys.index().expect("index failed");

    let symbols = tethys
        .search_symbols("Calculator")
        .expect("search failed");
    assert!(!symbols.is_empty(), "should find Calculator symbol");

    let symbols = tethys
        .search_symbols("ICalculator")
        .expect("search failed");
    assert!(!symbols.is_empty(), "should find ICalculator interface");
}

#[test]
fn indexes_mixed_rust_and_csharp() {
    let rust_code = r"
pub fn hello() {}
";
    let csharp_code = r"
public class Greeter {
    public void Hello() { }
}
";
    let (_dir, mut tethys) = workspace_with_files(&[
        ("src/lib.rs", rust_code),
        ("Greeter.cs", csharp_code),
    ]);
    let stats = tethys.index().expect("index failed");

    assert_eq!(stats.files_indexed, 2, "should index both Rust and C# files");
}

#[test]
fn csharp_stats_include_language() {
    let code = "public class Foo { }";
    let (_dir, mut tethys) = workspace_with_files(&[("Foo.cs", code)]);
    tethys.index().expect("index failed");

    let stats = tethys.get_stats().expect("get_stats failed");
    let csharp_count = stats.files_by_language.get("csharp").copied().unwrap_or(0);
    assert_eq!(csharp_count, 1, "should count 1 C# file in stats");
}

#[test]
fn csharp_references_are_stored() {
    let code = r"
public class Test {
    public void Run() {
        var user = new User();
        user.Save();
    }
}
";
    let (_dir, mut tethys) = workspace_with_files(&[("Test.cs", code)]);
    let stats = tethys.index().expect("index failed");

    assert!(stats.references_found > 0, "should find references in C# code");
}
```

**Step 2: Run tests**

```bash
cd crates/tethys && cargo test -- csharp
```

Expected: All new C# tests pass.

**Step 3: Run full test suite**

```bash
cd crates/tethys && cargo test
```

Expected: All tests pass (existing + new).

**Step 4: Commit**

```bash
git add crates/tethys/tests/indexing.rs
git commit -m "test(tethys): add integration tests for C# indexing"
```

---

### Task 7: Run Clippy and Final Cleanup

**Files:**
- Any files flagged by clippy

**Step 1: Run clippy**

```bash
cd crates/tethys && cargo clippy -- -D warnings
```

Fix any warnings (likely dead_code or unused import cleanup since the C# extraction functions are now called through the trait).

**Step 2: Remove `#[allow(dead_code)]` annotations**

In `csharp.rs`, many functions and types have `#[allow(dead_code)]` with comments like "Public API, used by tests and future indexer integration". Now that they're wired in through the trait, remove these annotations.

**Step 3: Run full test suite**

```bash
cargo test --workspace
```

Expected: All 1000+ workspace tests pass.

**Step 4: Commit**

```bash
git add -A
git commit -m "chore(tethys): remove dead_code annotations for now-integrated C# support"
```

---

## Summary of Changes

| Task | What Changes | Risk |
|------|-------------|------|
| 1. Unify types | New `common.rs`, refactor imports | Low — same types, just moved |
| 2. Trait methods | Add 3 methods to `LanguageSupport` | Low — wraps existing functions |
| 3. Parser language | Set language per-file | Low — tree-sitter supports this |
| 4. Dispatch | Remove skip guard, use trait | Medium — core indexing change |
| 5. Trait resolve_import | Add resolve_import to trait | Low — Rust delegates to existing resolver |
| 5b. C# namespace resolution | Namespace → file map, two-pass resolve | Medium — new DB queries + resolution logic |
| 6. Integration tests | New tests for C# indexing + namespace deps | Low — additive |
| 7. Cleanup | Remove dead_code, clippy | Low — cosmetic |

## What This Does NOT Include

- **C# property/field symbols**: The current extraction covers classes, structs, interfaces, enums, methods, constructors, and namespaces. Properties and fields are declared in `node_kinds` but not extracted as symbols yet.
- **Cross-file reference resolution**: Neither Rust nor C# resolves references across files yet — this is a separate effort.
- **External namespace resolution**: `using System.Collections.Generic` and other framework/NuGet namespaces can't be resolved to local files. Only project-internal namespaces are resolved.
