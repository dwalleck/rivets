# `tethys overview` Command — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a `tethys overview` command that produces a budget-aware, layered summary of a codebase's architecture — optimized for LLM consumption during initial project orientation.

**Architecture:** Five query layers (module tree, trait/interface map, public API surface, entry points, error flow) are each backed by existing SQLite queries against the Tethys index. A budget allocator distributes a line budget across layers, truncating lower-priority layers first. Output is compact indented text (not JSON) designed to fit in a single LLM context read.

**Tech Stack:** Rust (edition 2024), rusqlite queries against existing Tethys schema, clap derive for CLI, `colored` for terminal output. No new dependencies. No schema changes.

**Prerequisite:** Task 1 (Inherit extraction) is a standalone improvement that makes Layer 2 useful. Tasks 2-7 are the overview command itself. Tasks can be developed independently on separate branches if desired.

---

## Task 1: Extract `Inherit` References from Rust and C# (Prerequisite)

**Why:** The trait/interface map (Layer 2) is the highest-value layer in the overview. Currently, neither the Rust nor C# tree-sitter extractors emit `ReferenceKind::Inherit`. The domain model supports it (`types.rs:266`), the DB stores it, but no extraction path produces it. Without this, the overview can list traits/interfaces but cannot answer "who implements them?"

**Files:**
- Modify: `crates/tethys/src/languages/common.rs:59-78` (add `Inherit` variant)
- Modify: `crates/tethys/src/languages/rust.rs:681-700` (emit Inherit in `impl_item` handling)
- Modify: `crates/tethys/src/languages/csharp.rs:563-592` (emit Inherit from `base_list`)
- Test: existing test modules in `rust.rs` and `csharp.rs`

### Step 1: Add `Inherit` variant to `ExtractedReferenceKind`

In `crates/tethys/src/languages/common.rs`, add the variant and map it:

```rust
pub enum ExtractedReferenceKind {
    Call,
    Type,
    Constructor,
    Inherit,  // NEW: trait impl (Rust) or base type/interface (C#)
}

impl ExtractedReferenceKind {
    #[must_use]
    pub fn to_db_kind(self) -> crate::types::ReferenceKind {
        match self {
            Self::Call => crate::types::ReferenceKind::Call,
            Self::Type => crate::types::ReferenceKind::Type,
            Self::Constructor => crate::types::ReferenceKind::Construct,
            Self::Inherit => crate::types::ReferenceKind::Inherit,
        }
    }
}
```

### Step 2: Emit `Inherit` from Rust `impl_item` handling

In `crates/tethys/src/languages/rust.rs`, the `IMPL_ITEM` branch in `extract_references_recursive` (around line 194) currently recurses into methods but does not emit a reference for the trait being implemented. The tree-sitter AST for `impl Foo for Bar` has a structure like:

```
(impl_item
  trait: (type_identifier) "Foo"    ← this is the trait reference
  type: (type_identifier) "Bar"     ← this is the implementing type
  body: (declaration_list ...))
```

Add trait extraction before recursing into the body. The key is checking for the `trait` field on the `impl_item` node — if present, it's a trait impl (`impl Trait for Type`); if absent, it's an inherent impl (`impl Type`).

```rust
// Inside IMPL_ITEM branch of extract_references_recursive:
IMPL_ITEM => {
    // NEW: Emit Inherit reference for trait impls
    // impl_item has a "trait" field when it's `impl Trait for Type`
    if let Some(trait_node) = node.child_by_field_name("trait") {
        if let Some(trait_name) = extract_type_name(&trait_node, content) {
            refs.push(ExtractedReference {
                name: trait_name,
                kind: ExtractedReferenceKind::Inherit,
                line: trait_node.start_position().row as u32 + 1,
                column: trait_node.start_position().column as u32 + 1,
                path: None,
                containing_symbol_span: None, // impl block level, no containing fn
            });
        }
    }

    // existing method recursion continues unchanged...
    let mut cursor = node.walk();
    // ...
}
```

**Important:** Verify the tree-sitter-rust grammar field name. Parse a test snippet `impl Clone for Foo {}` and inspect the tree to confirm the field is called `"trait"`. If the field name differs, adjust accordingly. You can verify by running:
```bash
# In a test, parse and print the tree
let tree = parse_rust("impl Clone for Foo {}");
println!("{}", tree.root_node().to_sexp());
```

Also need a helper `extract_type_name` or reuse `find_impl_type` logic. The existing `find_impl_type` function (line 831) extracts the *implementing* type, not the trait. We need similar logic but targeting the `trait` field child. Alternatively, check if `node_text` on the trait node works directly (it should for simple trait names; for generic traits like `Iterator<Item = T>`, use `GENERIC_TYPE` handling).

### Step 3: Emit `Inherit` from C# base types

In `crates/tethys/src/languages/csharp.rs`, type declarations (class, struct, record) can have a `base_list` child containing base types and interfaces:

```csharp
// tree-sitter AST:
// (class_declaration
//   name: (identifier) "Foo"
//   bases: (base_list           ← walk this
//     (identifier) "BaseClass"
//     (identifier) "IInterface"))
```

Modify `extract_references_recursive` to handle base types. When visiting `CLASS_DECLARATION`, `STRUCT_DECLARATION`, or `RECORD_DECLARATION`, check for a `base_list` child and emit `Inherit` refs:

```rust
// In the CLASS_DECLARATION | STRUCT_DECLARATION | INTERFACE_DECLARATION branch,
// before recursing into DECLARATION_LIST:
let mut cursor = node.walk();
for child in node.children(&mut cursor) {
    if child.kind() == "base_list" {
        // Each child of base_list is a base type
        let mut base_cursor = child.walk();
        for base_type in child.children(&mut base_cursor) {
            if let Some(type_name) = extract_base_type_name(&base_type, content) {
                refs.push(ExtractedReference {
                    name: type_name,
                    kind: ExtractedReferenceKind::Inherit,
                    line: base_type.start_position().row as u32 + 1,
                    column: base_type.start_position().column as u32 + 1,
                    path: None,
                    containing_symbol_span: None,
                });
            }
        }
    }
    // ... existing DECLARATION_LIST handling
}
```

**Important:** Verify the C# tree-sitter grammar node kinds. The `base_list` might use different child types (`simple_base_type`, `generic_name`, `qualified_name`). Parse a test snippet and inspect:
```csharp
public class Foo : IBar, BazBase<T> { }
```

Write a helper `extract_base_type_name` that handles identifiers, qualified names, and generic names.

### Step 4: Write tests for both languages

**Rust tests** (add to existing test module in `rust.rs`):
```rust
#[test]
fn extracts_inherit_reference_for_trait_impl() {
    let code = "impl Clone for Foo {}";
    let tree = parse_rust(code);
    let refs = extract_references(&tree, code.as_bytes());
    let inherit_refs: Vec<_> = refs.iter()
        .filter(|r| r.kind == ExtractedReferenceKind::Inherit)
        .collect();
    assert_eq!(inherit_refs.len(), 1);
    assert_eq!(inherit_refs[0].name, "Clone");
}

#[test]
fn no_inherit_reference_for_inherent_impl() {
    let code = "impl Foo { fn bar() {} }";
    let tree = parse_rust(code);
    let refs = extract_references(&tree, code.as_bytes());
    let inherit_refs: Vec<_> = refs.iter()
        .filter(|r| r.kind == ExtractedReferenceKind::Inherit)
        .collect();
    assert!(inherit_refs.is_empty());
}

#[test]
fn extracts_inherit_reference_for_generic_trait() {
    let code = "impl Iterator for Foo { type Item = i32; fn next(&mut self) -> Option<Self::Item> { None } }";
    let tree = parse_rust(code);
    let refs = extract_references(&tree, code.as_bytes());
    let inherit_refs: Vec<_> = refs.iter()
        .filter(|r| r.kind == ExtractedReferenceKind::Inherit)
        .collect();
    assert_eq!(inherit_refs.len(), 1);
    assert_eq!(inherit_refs[0].name, "Iterator");
}
```

**C# tests** (add to existing test module in `csharp.rs`):
```rust
#[test]
fn extracts_inherit_reference_for_interface() {
    let code = "public class Foo : IBar { }";
    let tree = parse_csharp(code);
    let refs = extract_references(&tree, code.as_bytes());
    let inherit_refs: Vec<_> = refs.iter()
        .filter(|r| r.kind == ExtractedReferenceKind::Inherit)
        .collect();
    assert_eq!(inherit_refs.len(), 1);
    assert_eq!(inherit_refs[0].name, "IBar");
}

#[test]
fn extracts_multiple_inherit_references() {
    let code = "public class Foo : BaseClass, IFirst, ISecond { }";
    let tree = parse_csharp(code);
    let refs = extract_references(&tree, code.as_bytes());
    let inherit_refs: Vec<_> = refs.iter()
        .filter(|r| r.kind == ExtractedReferenceKind::Inherit)
        .collect();
    assert_eq!(inherit_refs.len(), 3);
}

#[test]
fn no_inherit_reference_for_class_without_base() {
    let code = "public class Foo { }";
    let tree = parse_csharp(code);
    let refs = extract_references(&tree, code.as_bytes());
    let inherit_refs: Vec<_> = refs.iter()
        .filter(|r| r.kind == ExtractedReferenceKind::Inherit)
        .collect();
    assert!(inherit_refs.is_empty());
}
```

### Step 5: Run all tests

```bash
cargo test -p tethys -- --filter "inherit"
cargo test -p tethys  # full suite to check nothing broke
```

### Step 6: Commit

```bash
git add crates/tethys/src/languages/
git commit -m "feat(tethys): extract Inherit references from Rust trait impls and C# base types"
```

---

## Task 2: Define the `Overview` Domain Types

**Why:** The overview command returns structured data that can be rendered as text or JSON. These types define the shape of each layer and the budget allocation.

**Files:**
- Create: `crates/tethys/src/overview.rs`
- Modify: `crates/tethys/src/lib.rs` (add `mod overview; pub use overview::*;`)

### Step 1: Write the types

Create `crates/tethys/src/overview.rs`:

```rust
//! Types and logic for the `tethys overview` command.
//!
//! Produces a budget-aware, layered summary of a codebase's architecture.
//! Designed for LLM consumption during initial project orientation.

use std::path::PathBuf;

use serde::Serialize;

/// Complete overview of a codebase, produced by [`Tethys::overview()`].
///
/// Contains five layers ordered by architectural importance:
/// 1. Module tree — file/module structure with symbol counts
/// 2. Trait/interface map — the architectural contracts
/// 3. Public API surface — exported types and signatures
/// 4. Entry points — where execution starts
/// 5. Error flow — fallible function signatures
#[derive(Debug, Clone, Serialize)]
pub struct Overview {
    /// Total line budget that was requested.
    pub budget: usize,
    /// Layer 1: Module/file tree with symbol counts.
    pub modules: Vec<ModuleEntry>,
    /// Layer 2: Traits/interfaces with their methods and implementors.
    pub traits: Vec<TraitEntry>,
    /// Layer 3: Public types and function signatures by module.
    pub public_api: Vec<PublicApiModule>,
    /// Layer 4: Binary/library entry points with immediate callees.
    pub entry_points: Vec<EntryPoint>,
    /// Layer 5: Functions that return Result/Option/OneOf types.
    pub error_flow: Vec<FallibleFunction>,
}

/// A module or file in the codebase with summary counts.
#[derive(Debug, Clone, Serialize)]
pub struct ModuleEntry {
    /// File path relative to workspace root.
    pub path: PathBuf,
    /// Number of public symbols in this file.
    pub pub_count: usize,
    /// Total number of symbols in this file.
    pub total_count: usize,
}

/// A trait or interface with its methods and implementors.
#[derive(Debug, Clone, Serialize)]
pub struct TraitEntry {
    /// Qualified name of the trait/interface.
    pub name: String,
    /// File where the trait is defined.
    pub file: PathBuf,
    /// Line number of the definition.
    pub line: u32,
    /// Method signatures belonging to this trait.
    pub methods: Vec<String>,
    /// Types that implement this trait.
    pub implementors: Vec<Implementor>,
}

/// A type that implements a trait or interface.
#[derive(Debug, Clone, Serialize)]
pub struct Implementor {
    /// Qualified name of the implementing type.
    pub name: String,
    /// File where the implementation lives.
    pub file: PathBuf,
    /// Line number of the implementation.
    pub line: u32,
}

/// Public API surface for a single module.
#[derive(Debug, Clone, Serialize)]
pub struct PublicApiModule {
    /// Module path (e.g., `crate::service`).
    pub module_path: String,
    /// Public symbols in this module.
    pub symbols: Vec<PublicSymbol>,
}

/// A single public symbol with its signature.
#[derive(Debug, Clone, Serialize)]
pub struct PublicSymbol {
    /// Symbol kind (struct, enum, function, method, etc.).
    pub kind: String,
    /// Qualified name.
    pub name: String,
    /// Full signature (e.g., `pub fn add(&self, id: &str) -> Result<InstallResult>`).
    /// `None` for types that don't have signatures (structs, enums without methods).
    pub signature: Option<String>,
    /// Parent type name if this is a method (e.g., `MarketplaceService`).
    pub parent: Option<String>,
}

/// An entry point (main function or lib re-export) with its immediate callees.
#[derive(Debug, Clone, Serialize)]
pub struct EntryPoint {
    /// Qualified name of the entry point.
    pub name: String,
    /// File path.
    pub file: PathBuf,
    /// Line number.
    pub line: u32,
    /// Whether this is a binary entry point (`main`) or library root export.
    pub kind: EntryPointKind,
    /// Symbols called directly by this entry point (depth 1).
    pub callees: Vec<String>,
}

/// Whether an entry point is a binary main or library export.
#[derive(Debug, Clone, Serialize)]
pub enum EntryPointKind {
    /// `fn main()` in a binary crate.
    BinaryMain,
    /// Public re-export from `lib.rs`.
    LibraryExport,
}

/// A function that can fail (returns Result, Option, or OneOf).
#[derive(Debug, Clone, Serialize)]
pub struct FallibleFunction {
    /// Qualified name.
    pub name: String,
    /// Full signature string.
    pub signature: String,
    /// The error/fallible type category.
    pub fallibility: Fallibility,
}

/// How a function can fail.
#[derive(Debug, Clone, Serialize)]
pub enum Fallibility {
    /// Returns `Result<T, E>`.
    Result,
    /// Returns `Option<T>`.
    Option,
    /// Returns `OneOf<...>` (C# discriminated union pattern).
    OneOf,
    /// Returns `Task<T>` (C# async).
    Task,
}

/// Budget allocation across the five layers.
///
/// Each layer gets a percentage of the total budget. Lower-priority layers
/// are truncated first when the budget is tight.
#[derive(Debug, Clone)]
pub struct BudgetAllocation {
    pub modules: usize,
    pub traits: usize,
    pub public_api: usize,
    pub entry_points: usize,
    pub error_flow: usize,
}

impl BudgetAllocation {
    /// Compute budget allocation from a total line budget.
    ///
    /// Allocation percentages:
    /// - Module tree: 15% (always included, low line count)
    /// - Trait map: 30% (highest architectural value)
    /// - Public API: 35% (truncated first under pressure)
    /// - Entry points: 10%
    /// - Error flow: 10%
    #[must_use]
    pub fn from_total(total: usize) -> Self {
        Self {
            modules: total * 15 / 100,
            traits: total * 30 / 100,
            public_api: total * 35 / 100,
            entry_points: total * 10 / 100,
            error_flow: total * 10 / 100,
        }
    }
}
```

### Step 2: Register the module

In `crates/tethys/src/lib.rs`, add:
```rust
mod overview;
```

And add the public types to the existing `pub use types::{ ... };` block, or add a separate re-export:
```rust
pub use overview::{
    BudgetAllocation, EntryPoint, EntryPointKind, Fallibility, FallibleFunction,
    Implementor, ModuleEntry, Overview, PublicApiModule, PublicSymbol, TraitEntry,
};
```

### Step 3: Run `cargo check -p tethys`

Verify the types compile. No tests yet — these are data types.

### Step 4: Commit

```bash
git add crates/tethys/src/overview.rs crates/tethys/src/lib.rs
git commit -m "feat(tethys): add Overview domain types for budget-aware codebase summary"
```

---

## Task 3: Implement the Overview Query Methods on `Tethys`

**Why:** Each layer needs a method on `Tethys` that queries the SQLite index and returns the corresponding overview types. These are independent query methods that the top-level `overview()` method will compose.

**Files:**
- Modify: `crates/tethys/src/overview.rs` (add query logic)
- Modify: `crates/tethys/src/lib.rs` (add `pub fn overview()` and layer query methods)

### Step 1: Add layer query methods to `Tethys`

In `crates/tethys/src/lib.rs`, add these methods to the `impl Tethys` block. Each one is a standalone query that returns one layer of the overview.

**Layer 1 — Module tree:**
```rust
/// Query Layer 1: module/file tree with symbol counts.
fn query_module_tree(&self) -> Result<Vec<overview::ModuleEntry>> {
    let conn = self.db.connection()?;

    let mut stmt = conn.prepare(
        "SELECT f.path,
                COUNT(*) FILTER (WHERE s.visibility = 'public') as pub_count,
                COUNT(s.id) as total_count
         FROM files f
         LEFT JOIN symbols s ON s.file_id = f.id
         GROUP BY f.id
         ORDER BY f.path"
    )?;

    let entries = stmt
        .query_map([], |row| {
            Ok(overview::ModuleEntry {
                path: PathBuf::from(row.get::<_, String>(0)?),
                pub_count: row.get::<_, i64>(1)? as usize,
                total_count: row.get::<_, i64>(2)? as usize,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(entries)
}
```

**Important consideration:** The `FILTER (WHERE ...)` syntax is SQLite 3.30+. Verify the bundled rusqlite version supports it. If not, use a CASE expression: `SUM(CASE WHEN s.visibility = 'public' THEN 1 ELSE 0 END)`.

**Layer 2 — Trait/interface map:**
```rust
/// Query Layer 2: traits/interfaces with methods and implementors.
fn query_trait_map(&self) -> Result<Vec<overview::TraitEntry>> {
    let conn = self.db.connection()?;

    // Get all traits/interfaces
    let mut trait_stmt = conn.prepare(
        "SELECT s.id, s.qualified_name, f.path, s.line
         FROM symbols s
         JOIN files f ON f.id = s.file_id
         WHERE s.kind IN ('trait', 'interface')
         ORDER BY s.qualified_name"
    )?;

    let traits: Vec<(i64, String, PathBuf, u32)> = trait_stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                PathBuf::from(row.get::<_, String>(2)?),
                row.get::<_, u32>(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut entries = Vec::with_capacity(traits.len());

    for (trait_id, trait_name, trait_file, trait_line) in traits {
        // Get methods of this trait
        let mut method_stmt = conn.prepare_cached(
            "SELECT s.signature FROM symbols s
             WHERE s.parent_symbol_id = ?1 AND s.kind = 'method'
             ORDER BY s.line"
        )?;
        let methods: Vec<String> = method_stmt
            .query_map([trait_id], |row| {
                row.get::<_, Option<String>>(0)
                    .map(|s| s.unwrap_or_default())
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // Get implementors via Inherit references
        let mut impl_stmt = conn.prepare_cached(
            "SELECT DISTINCT impl_sym.qualified_name, impl_file.path, r.line
             FROM refs r
             JOIN symbols impl_sym ON impl_sym.id = r.in_symbol_id
             JOIN files impl_file ON impl_file.id = impl_sym.file_id
             WHERE r.symbol_id = ?1 AND r.kind = 'inherit'
             ORDER BY impl_sym.qualified_name"
        )?;
        let implementors: Vec<overview::Implementor> = impl_stmt
            .query_map([trait_id], |row| {
                Ok(overview::Implementor {
                    name: row.get(0)?,
                    file: PathBuf::from(row.get::<_, String>(1)?),
                    line: row.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        entries.push(overview::TraitEntry {
            name: trait_name,
            file: trait_file,
            line: trait_line,
            methods,
            implementors,
        });
    }

    Ok(entries)
}
```

**Note on the implementor query:** The `refs.in_symbol_id` for Inherit references needs to resolve to the *implementing type*, not a method. This depends on how Task 1 sets `containing_symbol_span` — if it's `None` (impl block level), the reference resolution pass will assign `in_symbol_id` based on the closest enclosing symbol. Verify this produces the expected implementor symbol after Task 1 is complete. If `in_symbol_id` is NULL for Inherit refs, the query needs to join differently (e.g., via `refs.file_id` → find the struct/class in that file).

**Layer 3 — Public API surface:**
```rust
/// Query Layer 3: public symbols grouped by module.
fn query_public_api(&self) -> Result<Vec<overview::PublicApiModule>> {
    let conn = self.db.connection()?;

    let mut stmt = conn.prepare(
        "SELECT s.module_path, s.kind, s.qualified_name, s.signature,
                parent.qualified_name as parent_name
         FROM symbols s
         LEFT JOIN symbols parent ON parent.id = s.parent_symbol_id
         WHERE s.visibility = 'public'
         ORDER BY s.module_path,
                  CASE s.kind
                    WHEN 'struct' THEN 1 WHEN 'class' THEN 1
                    WHEN 'enum' THEN 2
                    WHEN 'trait' THEN 3 WHEN 'interface' THEN 3
                    WHEN 'function' THEN 4
                    WHEN 'method' THEN 5
                    ELSE 6
                  END,
                  s.line"
    )?;

    let mut modules: Vec<overview::PublicApiModule> = Vec::new();
    let mut current_module: Option<String> = None;
    let mut current_symbols: Vec<overview::PublicSymbol> = Vec::new();

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,        // module_path
            row.get::<_, String>(1)?,        // kind
            row.get::<_, String>(2)?,        // qualified_name
            row.get::<_, Option<String>>(3)?, // signature
            row.get::<_, Option<String>>(4)?, // parent_name
        ))
    })?;

    for row in rows {
        let (module_path, kind, name, signature, parent) = row?;

        if current_module.as_ref() != Some(&module_path) {
            if let Some(prev_module) = current_module.take() {
                modules.push(overview::PublicApiModule {
                    module_path: prev_module,
                    symbols: std::mem::take(&mut current_symbols),
                });
            }
            current_module = Some(module_path);
        }

        current_symbols.push(overview::PublicSymbol {
            kind,
            name,
            signature,
            parent,
        });
    }

    // Don't forget the last module
    if let Some(last_module) = current_module {
        modules.push(overview::PublicApiModule {
            module_path: last_module,
            symbols: current_symbols,
        });
    }

    Ok(modules)
}
```

**Layer 4 — Entry points:**
```rust
/// Query Layer 4: binary main functions and their depth-1 callees.
fn query_entry_points(&self) -> Result<Vec<overview::EntryPoint>> {
    let conn = self.db.connection()?;

    // Find main functions (binary entry points)
    let mut main_stmt = conn.prepare(
        "SELECT s.id, s.qualified_name, f.path, s.line
         FROM symbols s
         JOIN files f ON f.id = s.file_id
         WHERE s.name = 'main' AND s.kind = 'function'
         ORDER BY f.path"
    )?;

    let mains: Vec<(i64, String, PathBuf, u32)> = main_stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                PathBuf::from(row.get::<_, String>(2)?),
                row.get::<_, u32>(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut entries = Vec::new();

    for (sym_id, name, file, line) in mains {
        // Get depth-1 callees
        let mut callee_stmt = conn.prepare_cached(
            "SELECT callee.qualified_name
             FROM call_edges ce
             JOIN symbols callee ON callee.id = ce.callee_symbol_id
             WHERE ce.caller_symbol_id = ?1
             ORDER BY callee.qualified_name"
        )?;
        let callees: Vec<String> = callee_stmt
            .query_map([sym_id], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        entries.push(overview::EntryPoint {
            name,
            file,
            line,
            kind: overview::EntryPointKind::BinaryMain,
            callees,
        });
    }

    Ok(entries)
}
```

**Layer 5 — Error flow:**
```rust
/// Query Layer 5: public functions that return Result, Option, OneOf, or Task.
fn query_error_flow(&self) -> Result<Vec<overview::FallibleFunction>> {
    let conn = self.db.connection()?;

    let mut stmt = conn.prepare(
        "SELECT s.qualified_name, s.signature
         FROM symbols s
         WHERE s.visibility = 'public'
           AND s.kind IN ('function', 'method')
           AND s.signature IS NOT NULL
           AND (
               s.signature LIKE '%-> Result<%'
               OR s.signature LIKE '%-> Result %'
               OR s.signature LIKE '%-> Option<%'
               OR s.signature LIKE '%OneOf<%'
               OR s.signature LIKE '%Task<%'
           )
         ORDER BY s.module_path, s.qualified_name"
    )?;

    let functions = stmt
        .query_map([], |row| {
            let name: String = row.get(0)?;
            let signature: String = row.get(1)?;

            let fallibility = if signature.contains("OneOf<") {
                overview::Fallibility::OneOf
            } else if signature.contains("Result<") || signature.contains("-> Result ") {
                overview::Fallibility::Result
            } else if signature.contains("Option<") {
                overview::Fallibility::Option
            } else {
                overview::Fallibility::Task
            };

            Ok(overview::FallibleFunction {
                name,
                signature,
                fallibility,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(functions)
}
```

### Step 2: Compose the top-level `overview()` method

```rust
/// Generate a budget-aware overview of the codebase.
///
/// The `budget` parameter controls the approximate number of output lines.
/// Layers are populated in priority order, with lower-priority layers
/// truncated when the budget is tight.
pub fn overview(&self, budget: usize) -> Result<overview::Overview> {
    let allocation = overview::BudgetAllocation::from_total(budget);

    let mut modules = self.query_module_tree()?;
    let mut traits = self.query_trait_map()?;
    let mut public_api = self.query_public_api()?;
    let mut entry_points = self.query_entry_points()?;
    let mut error_flow = self.query_error_flow()?;

    // Truncate layers to fit within budget allocation.
    // Each entry is approximately 1 line (modules, error_flow)
    // or variable lines (traits, public_api, entry_points).
    modules.truncate(allocation.modules);
    truncate_traits(&mut traits, allocation.traits);
    truncate_public_api(&mut public_api, allocation.public_api);
    entry_points.truncate(allocation.entry_points);
    error_flow.truncate(allocation.error_flow);

    Ok(overview::Overview {
        budget,
        modules,
        traits,
        public_api,
        entry_points,
        error_flow,
    })
}
```

### Step 3: Implement truncation helpers

Add these to `overview.rs`:

```rust
/// Estimate the line count for a trait entry.
fn trait_line_count(entry: &TraitEntry) -> usize {
    1 + entry.methods.len() + entry.implementors.len()
}

/// Truncate trait entries to fit within a line budget.
pub(crate) fn truncate_traits(traits: &mut Vec<TraitEntry>, budget: usize) {
    let mut total = 0;
    let mut keep = traits.len();
    for (i, entry) in traits.iter().enumerate() {
        total += trait_line_count(entry);
        if total > budget {
            keep = i;
            break;
        }
    }
    traits.truncate(keep);
}

/// Truncate public API modules to fit within a line budget.
pub(crate) fn truncate_public_api(modules: &mut Vec<PublicApiModule>, budget: usize) {
    let mut total = 0;
    let mut keep = modules.len();
    for (i, module) in modules.iter().enumerate() {
        total += 1 + module.symbols.len(); // 1 header + symbols
        if total > budget {
            keep = i;
            break;
        }
    }
    modules.truncate(keep);
}
```

### Step 4: Handle DB access

The layer query methods need raw `rusqlite::Connection` access. The current `db: Index` field wraps the connection in a `Mutex`. Either:
- Add a `pub(crate) fn connection(&self) -> Result<MutexGuard<Connection>>` to `Index` (if it doesn't already exist), or
- Add the overview queries as methods on `Index` itself (matches the existing pattern where all SQL lives in `db/`).

**Recommended approach:** Add a new file `crates/tethys/src/db/overview.rs` with the SQL queries as methods on `Index`, then call them from `Tethys::overview()`. This follows the existing pattern where `db/graph.rs`, `db/symbols.rs`, etc. each own their SQL domain.

### Step 5: Run `cargo check -p tethys`

### Step 6: Write tests

Test the truncation helpers (pure logic, no DB needed):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_allocation_sums_to_total() {
        let alloc = BudgetAllocation::from_total(200);
        let sum = alloc.modules + alloc.traits + alloc.public_api
            + alloc.entry_points + alloc.error_flow;
        // May be slightly less than 200 due to integer division
        assert!(sum <= 200);
        assert!(sum >= 190); // within 5%
    }

    #[test]
    fn truncate_traits_respects_budget() {
        let traits = vec![
            TraitEntry {
                name: "Foo".into(), file: "a.rs".into(), line: 1,
                methods: vec!["fn a()".into(), "fn b()".into()],
                implementors: vec![],
            },
            TraitEntry {
                name: "Bar".into(), file: "b.rs".into(), line: 1,
                methods: vec!["fn c()".into()],
                implementors: vec![Implementor {
                    name: "Baz".into(), file: "c.rs".into(), line: 10,
                }],
            },
        ];
        let mut t = traits.clone();
        truncate_traits(&mut t, 3); // Foo = 3 lines (1 + 2 methods), fits
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn truncate_traits_keeps_all_when_budget_sufficient() {
        let traits = vec![
            TraitEntry {
                name: "Foo".into(), file: "a.rs".into(), line: 1,
                methods: vec!["fn a()".into()],
                implementors: vec![],
            },
        ];
        let mut t = traits.clone();
        truncate_traits(&mut t, 100);
        assert_eq!(t.len(), 1);
    }
}
```

### Step 7: Commit

```bash
git add crates/tethys/src/overview.rs crates/tethys/src/lib.rs crates/tethys/src/db/
git commit -m "feat(tethys): implement overview query methods for all 5 layers"
```

---

## Task 4: Implement the Text Formatter

**Why:** The overview needs a compact, indented text output optimized for LLM consumption. This is the primary output format — JSON is secondary.

**Files:**
- Modify: `crates/tethys/src/overview.rs` (add `Display` impl or format function)

### Step 1: Implement `Display` for `Overview`

The text format should look like this:

```
── Module Tree ──────────────────────────────────
  src/service.rs                    6 pub / 12 total
  src/git.rs                        3 pub / 8 total
  src/platform.rs                   3 pub / 3 total
  src/cache.rs                      4 pub / 6 total

── Traits & Interfaces ──────────────────────────
  trait GitBackend                   src/git.rs:15
    fn clone_repo(&self, url: &str, dest: &Path) -> Result<()>
    fn pull(&self, repo: &Path) -> Result<()>
    impl GixCliBackend               src/git/gix_cli.rs:42
    impl MockGitBackend              tests/mocks.rs:10

── Public API ───────────────────────────────────
  mod service
    pub struct MarketplaceService
      pub fn add(&self, id: &str) -> Result<InstallResult>
      pub fn remove(&self, id: &str) -> Result<()>

── Entry Points ─────────────────────────────────
  fn main()                          src/main.rs:142
    -> cli::index::run
    -> cli::search::run

── Error Flow ───────────────────────────────────
  MarketplaceService::add            -> Result<InstallResult>
  MarketplaceService::remove         -> Result<()>
  GitBackend::clone_repo             -> Result<()>
```

Implement as `impl std::fmt::Display for Overview`. Use fixed-width section headers. Right-align file paths and counts. Use `->` prefix for callees to distinguish them from declarations.

### Step 2: Write format tests

Test that the Display output contains expected section headers, doesn't exceed budget (approximately), and degrades gracefully with empty layers.

### Step 3: Commit

```bash
git add crates/tethys/src/overview.rs
git commit -m "feat(tethys): add compact text formatter for overview output"
```

---

## Task 5: Add the CLI Subcommand

**Why:** Wire the overview into the `tethys` CLI as `tethys overview`.

**Files:**
- Create: `crates/tethys/src/cli/overview.rs`
- Modify: `crates/tethys/src/cli/mod.rs` (add `pub mod overview;`)
- Modify: `crates/tethys/src/main.rs` (add `Overview` variant to `Commands`)

### Step 1: Add the CLI command

In `crates/tethys/src/main.rs`, add to the `Commands` enum:

```rust
/// Show a budget-aware architectural overview of the codebase
Overview {
    /// Maximum lines of output (default: 200)
    #[arg(short, long, default_value = "200")]
    budget: usize,

    /// Output as JSON instead of formatted text
    #[arg(long)]
    json: bool,
},
```

Add the match arm:
```rust
Commands::Overview { budget, json } => cli::overview::run(&workspace, budget, json),
```

### Step 2: Implement the CLI handler

Create `crates/tethys/src/cli/overview.rs`:

```rust
//! CLI handler for the `tethys overview` command.

use std::path::Path;

use colored::Colorize;

pub fn run(workspace: &Path, budget: usize, json: bool) -> anyhow::Result<()> {
    let tethys = tethys::Tethys::new(workspace)?;
    let overview = tethys.overview(budget)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&overview)?);
    } else {
        print!("{overview}");
    }

    Ok(())
}
```

**Note:** Check the return type convention in existing CLI handlers — they may return `anyhow::Result<()>` or `Result<(), tethys::Error>`. Match the existing pattern (looks like bare `anyhow::Result<()>` based on `main.rs` error handling).

### Step 3: Register in `cli/mod.rs`

```rust
pub mod overview;
```

### Step 4: Test manually

```bash
cargo run -p tethys -- overview --budget 100
cargo run -p tethys -- overview --budget 200 --json
```

### Step 5: Commit

```bash
git add crates/tethys/src/cli/overview.rs crates/tethys/src/cli/mod.rs crates/tethys/src/main.rs
git commit -m "feat(tethys): add 'tethys overview' CLI subcommand with budget and JSON flags"
```

---

## Task 6: Integration Test

**Why:** Verify the full pipeline — index a workspace, then generate an overview — produces correct output.

**Files:**
- Test: add integration test (either in `crates/tethys/tests/` or as `#[cfg(test)]` in `lib.rs`)

### Step 1: Write the integration test

```rust
#[test]
fn overview_produces_all_layers_for_indexed_workspace() {
    let workspace = tempfile::tempdir().unwrap();
    let src = workspace.path().join("src");
    std::fs::create_dir_all(&src).unwrap();

    // Write a minimal Rust file with a trait, impl, and pub functions
    std::fs::write(src.join("lib.rs"), r#"
        pub trait Greeter {
            fn greet(&self) -> String;
        }

        pub struct English;

        impl Greeter for English {
            fn greet(&self) -> String {
                "Hello".to_string()
            }
        }

        pub fn fallible() -> Result<(), String> {
            Ok(())
        }
    "#).unwrap();

    let mut tethys = Tethys::new(workspace.path()).unwrap();
    tethys.index().unwrap();

    let overview = tethys.overview(200).unwrap();

    // Layer 1: module tree should have lib.rs
    assert!(!overview.modules.is_empty());
    assert!(overview.modules.iter().any(|m| m.path.ends_with("lib.rs")));

    // Layer 2: should find the Greeter trait
    assert!(overview.traits.iter().any(|t| t.name.contains("Greeter")));

    // Layer 3: should have public symbols
    assert!(!overview.public_api.is_empty());

    // Layer 5: should find the fallible function
    assert!(overview.error_flow.iter().any(|f| f.name.contains("fallible")));
}

#[test]
fn overview_text_format_contains_section_headers() {
    let workspace = tempfile::tempdir().unwrap();
    let src = workspace.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"), "pub fn hello() {}").unwrap();

    let mut tethys = Tethys::new(workspace.path()).unwrap();
    tethys.index().unwrap();

    let overview = tethys.overview(200).unwrap();
    let text = format!("{overview}");

    assert!(text.contains("Module Tree"));
    assert!(text.contains("Public API"));
}

#[test]
fn overview_respects_small_budget() {
    let workspace = tempfile::tempdir().unwrap();
    let src = workspace.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"), "pub fn hello() {}\npub fn world() {}").unwrap();

    let mut tethys = Tethys::new(workspace.path()).unwrap();
    tethys.index().unwrap();

    let overview = tethys.overview(5).unwrap();
    let text = format!("{overview}");
    let line_count = text.lines().count();

    // Should be roughly within budget (headers add some overhead)
    assert!(line_count <= 15, "Output {line_count} lines exceeds budget 5 + overhead");
}
```

### Step 2: Run tests

```bash
cargo test -p tethys -- overview
```

### Step 3: Commit

```bash
git add crates/tethys/
git commit -m "test(tethys): add integration tests for overview command"
```

---

## Task 7: Clippy and Final Polish

**Files:**
- All modified files

### Step 1: Run clippy

```bash
cargo clippy -p tethys -- -D warnings
```

Fix any warnings (the crate uses `clippy::pedantic`).

### Step 2: Run full test suite

```bash
cargo test -p tethys
```

### Step 3: Final commit

```bash
git add -A
git commit -m "chore(tethys): clippy fixes for overview command"
```

---

## Summary of Dependencies

```
Task 1 (Inherit extraction) ─── independent, can ship alone
    │
    ▼
Task 2 (types) ──► Task 3 (queries) ──► Task 4 (formatter) ──► Task 5 (CLI)
                                                                     │
                                                                     ▼
                                                              Task 6 (tests)
                                                                     │
                                                                     ▼
                                                              Task 7 (polish)
```

## Key Design Decisions

1. **Text-first, JSON-second:** The primary consumer is an LLM reading text. JSON is available via `--json` for programmatic use but the text formatter is the star.

2. **Budget is lines, not bytes:** Lines are a better proxy for LLM context cost than byte count. One line ≈ one symbol or one relationship.

3. **No new DB tables or columns:** The entire feature is a read-only projection of existing indexed data. Zero migration burden.

4. **Truncation is per-layer, not global:** Each layer gets its allocation and manages its own truncation. This prevents one verbose layer (e.g., a crate with 200 public functions) from starving the others.

5. **Trait map gets 30% — the largest share:** Based on the analysis that trait/interface definitions carry the highest bits-per-line for architectural understanding.
