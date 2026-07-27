# `tethys overview` Review Fixes Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Address 17 findings from the Dinesh-vs-Gilfoyle review of the `feature/tethys-overview` branch — 4 critical data-correctness bugs, 6 important correctness/resolution gaps, and 7 minor polish items.

**Architecture:** Fixes are grouped by shared root cause. Group A removes the three "substring-matching on signature strings" bugs by plumbing a structured `return_type` column through the schema. Group B fixes the transaction scope and name-collision issues in the indexing pipeline. Group C tightens budget accounting and enum hygiene. Group D is single-line cleanup.

**Tech Stack:** Rust (edition 2024), rusqlite against existing Tethys schema, tree-sitter for parser changes. One schema change: add `symbols.return_type` column.

**Prerequisite work already done:** None — this plan applies directly on top of `feature/tethys-overview`.

**Review summary:** 4 critical, 6 important, 7 minor. Dinesh conceded all critical and important findings during Round 2 of the debate; this plan turns concessions into code.

---

## Design Decisions (read before implementing)

These are the judgment calls the review surfaced. Each has a trade-off worth thinking through *before* touching code.

### D1: Structural fix for return-type bugs (Tasks 1–4)

**Three bugs share one root cause:** `overview.rs:604` `return_type()`, `db/overview.rs:334` SQL `LIKE '%-> Result<%'`, and `db/overview.rs:370` `classify_fallibility` all substring-match on the raw `signature` string. Functions like `fn handle(cb: impl FnOnce() -> Result<T, E>) -> String` break all three.

**Option A (patch each site):** Rewrite `return_type()` with paren-depth tracking. Keep SQL LIKE as is. Keep `classify_fallibility`'s substring checks. ~30 lines across three files.

**Option B (structural — CHOSEN):** Add a dedicated `return_type TEXT` column to the `symbols` table, populate it from `FunctionSignature.return_type` (already parsed at `types.rs:425` but currently thrown away before insert). Query on `return_type`, not `signature`. ~60 lines but eliminates all three bugs at the root.

**Why Option B:** The parser already extracts structured return types — we're discarding free information. Patching `return_type()` with paren-depth still leaves the SQL LIKE filter vulnerable to false positives on parameter types. The structural fix is ~2x the code for permanent root-cause elimination. Adding `symbols.return_type` is cheap; there are no existing external users, so the database is recreated fresh and no migration runner is needed.

### D2: Cross-file parent resolution (Task 9)

**The problem:** When `impl Foo for Bar` has `Bar` declared in another file, the current name-based lookup in `indexing.rs:644` misses it and silently leaves `parent_symbol_id` NULL at `trace!` level. This is invisible data loss in the Public API layer.

**Option A (log louder, accept gap — CHOSEN for this plan):** Upgrade from `trace!` to `warn!` when the unresolved parent belongs to a public symbol. Document the limitation. File a follow-up rivets issue for structural fix.

**Option B (Pass 2 resolution):** Add a second pass after all files are indexed that resolves pending `(child_id, parent_name)` tuples against the full symbols table. Mirrors how `resolve_references` already works in Phase 2 (see `crates/tethys/CLAUDE.md` — "Phase 2: Dependency Resolution").

**Why Option A now:** Pass 2 is a rabbit hole that belongs in its own plan. This review-fix plan should not balloon into a multi-pass indexing refactor. A follow-up issue (`rivets-XXXX`) tracks the structural fix.

### D3: `name_to_id` collision fix (Task 8)

**The problem:** `build_symbol_maps_from_data` in `indexing.rs:687` keys on `sym.name` alone. `struct Foo; mod tests { struct Foo }` compiles, tree-sitter extracts both, and one silently overwrites the other. Inherit resolution then binds to whichever lost the coin flip.

**Option A (`(name, module_path)` key — CHOSEN):** Most precise. `module_path` is already populated on `OwnedSymbolData` (`parallel.rs:73`) and on `SymbolData` (`db/mod.rs:61`), so no parser changes are needed. For the nested-mod case, `struct Foo` in `crate` and `struct Foo` in `crate::tests` map to different keys and stop colliding.

**Option B (`(name, kind)` key):** Simpler but doesn't distinguish two `struct Foo` in different nested modules — only catches type-vs-function collisions. Fails the nested-duplicate test case we want to assert.

**Option C (prefer span-based, fall back to name — REJECTED):** For Rust `impl Trait for Type`, pass the implementing type's symbol span through instead of the name. This is semantically ideal but **not implementable without changing the `LanguageSupport::extract_references` API**: `extract_symbols` and `extract_references` are currently independent tree-sitter traversals (`languages/rust.rs:77-86`), and the reference extractor has no handle to the symbol list. `find_impl_type` returns only a name string, and even the impl-site identifier's span doesn't match the struct-declaration span that `span_to_id` is keyed on. Making spans work requires plumbing the extracted symbols (or a name→span map built from them) into the reference extractor — a scope increase that doesn't belong in a review-fix plan.

**Why Option A:** It fixes the exact case the bug describes (nested modules with duplicate type names), the plumbing already exists, and it's a one-line key change to `build_symbol_maps_from_data`. Spans would be more semantically pure but require an API refactor that's out of scope. A follow-up issue can pursue the span approach if Option A's keying proves insufficient in practice.

### D4: Budget accounting (Task 12)

**The problem:** `lib.rs:876` says "approximate target line count" but no test asserts a real bound. `--budget 100` produces ~109 rendered lines (5 section headers + 4 blank separators).

**Option A (subtract overhead — CHOSEN):** Define `const OVERVIEW_OVERHEAD: usize = 9`. Subtract from `total` before allocating layers. `--budget 100` actually caps output at 100.

**Option B (document and assert):** Keep the slop, document it, add a test that binds `rendered.lines().count() <= budget + OVERVIEW_OVERHEAD`.

**Why Option A:** Users passing `--budget 200` to an LLM integration with `budget = context_remaining` expect ~200 lines of output, not 209. Subtracting overhead is the honest interpretation of "budget." We still keep the test from Option B as a regression guard.

### D5: Transaction scope for parent resolution (Task 7)

**The problem:** `index_file_atomic` commits at `db/files.rs:188`, then the `set_symbol_parent` loop at `indexing.rs:640` runs N independent UPDATEs outside the transaction. A failure leaves partial state; the "atomic" name is aspirational.

**Option A (move resolution into `index_file_atomic` — CHOSEN):** Plumb `parent_name` through `SymbolData` into `index_file_atomic`. Resolve names to IDs inside the transaction immediately after the symbol INSERTs. Single commit, single atomicity boundary.

**Option B (closure hook):** Add a `FnOnce(&tx, &[SymbolId]) -> Result<()>` parameter to `index_file_atomic` that runs before commit.

**Why Option A:** The data needed for resolution (all symbol names and their new IDs) is already inside the transaction — we just need to compute the lookup before committing. Option B adds an API surface for one caller. Option A is a cleaner internalization.

---

## Task Groups

- **Group A — Critical: Structural return-type fix (Tasks 1–4)** — adds `symbols.return_type` column, fixes `return_type()`, SQL LIKE filter, and `classify_fallibility` at the root.
- **Group B — Critical: `call_edges` pollution (Tasks 5–6)** — adds `kind = 'call'` filter and rebuild helper.
- **Group C — Critical: Transaction scope + resolution (Tasks 7–9)** — moves parent resolution inside `index_file_atomic`, fixes name-collision, upgrades cross-file logging.
- **Group D — Important: Truncation & index (Tasks 10–11)** — `truncate_*` break→continue, `parent_symbol_id` index.
- **Group E — Important: Budget + enum hygiene (Tasks 12–13)** — overhead subtraction, `#[non_exhaustive]`.
- **Group F — Minor cleanup (Tasks 14–17)** — debug_assert→tracing, `.unwrap()`→`.expect()`, `#[from]` serde, docstring fix, query_trait_map implementors filter.

Each task is TDD-structured: write the failing test first, watch it fail, make it pass, commit. Use `cargo nextest run -p tethys` per the project's test convention.

---

## Task 1: Add `return_type` column to symbols schema

**Why:** Root-cause fix for three bugs. The parser already extracts `FunctionSignature.return_type` (`types.rs:425`) but throws it away at the DB boundary — `Symbol.signature_details` is marked `"Not persisted to database; populated by parsers only"` at `types.rs:564`. Adding a column to `symbols` and plumbing it through lets us query on structured return types instead of substring-matching signatures.

**Important:** There are **two symbol INSERT sites** in this codebase. The production path is `index_file_atomic` in `db/files.rs`; the `InsertSymbolParams` / `insert_symbol` pair in `db/symbols.rs` is `#[cfg(test)]`-only (test helper). Both need updating, and tests must cover the production path, not just the test helper.

**Files:**
- Modify: `crates/tethys/src/db/schema.rs` (add `return_type TEXT` column to `CREATE TABLE symbols`)
- Modify: `crates/tethys/src/db/files.rs:166` — **production INSERT path** inside `index_file_atomic`
- Modify: `crates/tethys/src/db/symbols.rs:35` — test-only INSERT helper
- Modify: `crates/tethys/src/db/helpers.rs:22` — `SYMBOLS_COLUMNS` constant needs `return_type` appended
- Modify: `crates/tethys/src/db/helpers.rs:147` — `row_to_symbol` reads columns positionally; add the new column read
- Modify: `crates/tethys/src/types.rs:542` — `Symbol` struct: add `pub return_type: Option<String>`
- Modify: `crates/tethys/src/db/mod.rs:59` — `SymbolData` struct: add `pub return_type: Option<&'a str>`
- Modify: `crates/tethys/src/parallel.rs:71` — `OwnedSymbolData` struct: add `pub return_type: Option<String>` and thread through `as_symbol_data()`
- Modify: `crates/tethys/src/indexing.rs` — populate `return_type` from `ExtractedSymbol.signature_details.and_then(|fs| fs.return_type)` where `OwnedSymbolData` is constructed
- Modify: `crates/tethys/src/batch_writer.rs` — verify `OwnedSymbolData` construction sites include the new field
- Test: `crates/tethys/src/db/files.rs` — new test that goes through `index_file_atomic` and asserts the return_type round-trips via `list_symbols_in_file`
- Test: `crates/tethys/src/db/symbols.rs` — extend existing tests for the `insert_symbol` helper path

### Step 1: Write the failing schema/roundtrip test

Add to `crates/tethys/src/db/symbols.rs` tests module:

```rust
#[test]
fn insert_and_read_symbol_with_return_type() {
    let index = Index::in_memory().expect("open in-memory index");
    let file_id = index
        .insert_file(&InsertFileParams {
            path: "src/lib.rs",
            language: Language::Rust,
            mtime_ns: 0,
            size_bytes: 0,
            content_hash: None,
        })
        .expect("insert file");

    let sym_id = index
        .insert_symbol(&InsertSymbolParams {
            file_id,
            name: "do_thing",
            qualified_name: "crate::do_thing",
            module_path: "crate",
            kind: "function",
            line: 1,
            column: 1,
            signature: Some("fn do_thing() -> Result<i32, Error>"),
            return_type: Some("Result<i32, Error>"),
            visibility: "public",
            parent_symbol_id: None,
            is_test: false,
        })
        .expect("insert symbol");

    let sym = index.get_symbol(sym_id).expect("get symbol").expect("exists");
    assert_eq!(
        sym.return_type.as_deref(),
        Some("Result<i32, Error>"),
        "return_type should roundtrip through insert/read"
    );
}
```

### Step 2: Run the test and confirm it fails

```bash
cargo nextest run -p tethys insert_and_read_symbol_with_return_type
```

Expected: compile error — `return_type` is not a field on `InsertSymbolParams` or `Symbol`.

### Step 3: Add the column to the schema

In `crates/tethys/src/db/schema.rs`:

1. Add `return_type TEXT,` to the `CREATE TABLE symbols` statement, immediately after `signature TEXT,`.

There is **no schema versioning or migration system** in Tethys (verified: no `SCHEMA_VERSION` constant, no `PRAGMA user_version`, no migrations module). Since there are no external users of this code, the upgrade story is "delete `.rivets/index/tethys.db` and re-index". The `CREATE TABLE IF NOT EXISTS` in `SCHEMA` will pick up the new column on a fresh DB; existing databases in-tree should be deleted before running the updated indexer.

### Step 4: Add `return_type` to the symbol types and both INSERT paths

**Production path (`index_file_atomic`, `db/files.rs:166`):**

- Add `pub return_type: Option<&'a str>` to `SymbolData` in `db/mod.rs:59`, right after `signature`.
- Update the INSERT statement in `index_file_atomic` to include `return_type` in both the column list and the `params![...]` values.

**Shared read path (`db/helpers.rs`):**

- Append `return_type` to the `SYMBOLS_COLUMNS` constant at `db/helpers.rs:22`.
- Update `row_to_symbol` at `db/helpers.rs:147` to read the new column. **Verify the column index** — adding a column to the end of `SYMBOLS_COLUMNS` makes the next available index `14`, and the existing reads use `0..=13`.

**Public type (`types.rs`):**

- Add `pub return_type: Option<String>` to the `Symbol` struct at `types.rs:542`, next to `signature`.

**Test helper path (`db/symbols.rs`):**

- Add `pub return_type: Option<&'a str>` to `InsertSymbolParams` and thread it through the test-only `insert_symbol` INSERT.

### Step 5: Propagate through `OwnedSymbolData` and indexing

In `crates/tethys/src/parallel.rs:71`:

1. Add `pub return_type: Option<String>` to `OwnedSymbolData`, after `signature`.
2. Update `OwnedSymbolData::as_symbol_data()` at `parallel.rs:131` to pass `return_type: self.return_type.as_deref()` into the returned `SymbolData`.

Where `OwnedSymbolData` is constructed from `ExtractedSymbol` (grep for `OwnedSymbolData {` in `indexing.rs` and any callers), populate the new field from the parser's already-extracted function signature:

```rust
return_type: sym
    .signature_details
    .as_ref()
    .and_then(|fs| fs.return_type.clone()),
```

(`ExtractedSymbol.signature_details: Option<FunctionSignature>` exists at `languages/common.rs:24`, and `FunctionSignature.return_type: Option<String>` exists at `types.rs:425`.)

In `batch_writer.rs`, any test fixtures or helpers that construct `OwnedSymbolData` literals will fail to compile until `return_type` is added. `return_type: None` is the safe default for non-function test data.

### Step 6: Run the test and confirm it passes

```bash
cargo nextest run -p tethys insert_and_read_symbol_with_return_type
```

Expected: PASS.

### Step 7: Run the full tethys test suite

```bash
cargo nextest run -p tethys
```

Expected: all green. Any failures are almost certainly places that construct `InsertSymbolParams` or `OwnedSymbolData` without the new field — add `return_type: None` at each call site.

### Step 8: Commit

```bash
git add \
    crates/tethys/src/db/schema.rs \
    crates/tethys/src/db/files.rs \
    crates/tethys/src/db/symbols.rs \
    crates/tethys/src/db/helpers.rs \
    crates/tethys/src/db/mod.rs \
    crates/tethys/src/types.rs \
    crates/tethys/src/parallel.rs \
    crates/tethys/src/indexing.rs \
    crates/tethys/src/batch_writer.rs
git commit -m "feat(tethys): persist structured return_type on symbols table"
```

---

## Task 2: Rewrite `query_error_flow` to filter on `return_type`

**Why:** `db/overview.rs:334` currently uses `signature LIKE '%-> Result<%'` which matches parameter types in closures (e.g. `fn handle(cb: impl FnOnce() -> Result<T, E>) -> String`). With the new `return_type` column, we can filter precisely.

**Files:**
- Modify: `crates/tethys/src/db/overview.rs:324-361` (`query_error_flow`)
- Test: same file's tests module

### Step 1: Write a failing test for the closure-param false positive

Add to `crates/tethys/src/db/overview.rs` tests:

```rust
#[test]
fn query_error_flow_excludes_functions_with_result_closure_params() {
    let index = Index::in_memory().expect("open in-memory index");
    let file_id = index
        .insert_file(&InsertFileParams { /* ... */ })
        .expect("insert file");

    // Function that takes a Result-returning closure but itself returns String.
    index.insert_symbol(&InsertSymbolParams {
        file_id,
        name: "handle",
        qualified_name: "crate::handle",
        module_path: "crate",
        kind: "function",
        line: 1, column: 1,
        signature: Some("fn handle(cb: impl FnOnce() -> Result<i32, Error>) -> String"),
        return_type: Some("String"),
        visibility: "public",
        parent_symbol_id: None,
        is_test: false,
    }).expect("insert symbol");

    let functions = index.query_error_flow().expect("query");
    assert!(
        !functions.iter().any(|f| f.name.ends_with("handle")),
        "handle() returns String, must not appear in error flow: {:?}",
        functions.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
}
```

### Step 2: Run the test and confirm it fails

```bash
cargo nextest run -p tethys query_error_flow_excludes_functions_with_result_closure_params
```

Expected: FAIL — the old SQL matches the signature substring.

### Step 3: Rewrite the SQL to filter on `return_type`

In `crates/tethys/src/db/overview.rs:327`:

```rust
let mut stmt = conn.prepare(
    "SELECT s.qualified_name, s.signature, s.return_type
     FROM symbols s
     WHERE s.visibility = 'public'
       AND s.kind IN ('function', 'method')
       AND s.return_type IS NOT NULL
       AND (
           s.return_type = 'Result'
           OR s.return_type LIKE 'Result<%'
           OR s.return_type LIKE 'Result %'
           OR s.return_type LIKE 'Result::%'
           OR s.return_type = 'Option'
           OR s.return_type LIKE 'Option<%'
           OR s.return_type LIKE 'Option::%'
           OR s.return_type LIKE 'OneOf<%'
           OR s.return_type LIKE 'Task<%'
       )
     ORDER BY s.module_path, s.qualified_name",
)?;
```

The bare `= 'Result'` and `LIKE 'Result::%'` patterns catch Rust type aliases like `type Result<T> = core::result::Result<T, crate::Error>;` where the alias is used without generics (`fn foo() -> Result`), as well as fully-qualified paths (`fn foo() -> core::result::Result<T, E>`). Same for `Option`.

Update the row reader to pass the bare `return_type` to `classify_fallibility` (we'll rewrite that function in Task 3).

### Step 4: Run the test and confirm it passes

```bash
cargo nextest run -p tethys query_error_flow_excludes_functions_with_result_closure_params
```

### Step 5: Commit

```bash
git add crates/tethys/src/db/overview.rs
git commit -m "fix(tethys): filter error flow on structured return_type column"
```

---

## Task 3: Rewrite `classify_fallibility` as an outermost-generic state machine

**Why:** The current implementation substring-matches on a signature string and orders by `contains` checks. It gets `Option<Result<T, E>>` wrong (classifies as `Result`). A small state machine reading the first type constructor after `->` gets both orderings right without ordering heuristics.

**Files:**
- Modify: `crates/tethys/src/db/overview.rs:370-387` (`classify_fallibility`)
- Test: same file

### Step 1: Write failing tests for the outermost-generic semantics

Add to the `tests` module in `db/overview.rs`:

```rust
#[test]
fn classify_fallibility_nested_option_of_result_is_option() {
    // The function's outermost return type is Option, so it's Option-fallible.
    assert_eq!(
        classify_fallibility("Option<Result<i32, Error>>"),
        Fallibility::Option,
    );
}

#[test]
fn classify_fallibility_nested_result_of_option_is_result() {
    assert_eq!(
        classify_fallibility("Result<Option<i32>, Error>"),
        Fallibility::Result,
    );
}

#[test]
fn classify_fallibility_task_of_oneof_is_oneof() {
    // C# convention: Task<OneOf<...>>. The inner OneOf conveys fallibility.
    // Open question: should this be OneOf or Task?
    // Decision: the *fallible* shape is OneOf; Task wraps it for async.
    assert_eq!(
        classify_fallibility("Task<OneOf<Success, Error>>"),
        Fallibility::OneOf,
    );
}

#[test]
fn classify_fallibility_plain_task_is_task() {
    assert_eq!(classify_fallibility("Task<int>"), Fallibility::Task);
}
```

Also update signatures passed into this function throughout the caller (`query_error_flow`) — input is now the bare return type, not the full signature.

### Step 2: Run the tests and watch them fail

```bash
cargo nextest run -p tethys classify_fallibility
```

Expected: `classify_fallibility_nested_option_of_result_is_option` fails with `Result != Option`.

### Step 3: Implement the outermost-generic classifier

Replace the function body with a split design: a public `classify_fallibility` that logs on a top-level miss, and a private `classify_inner` that returns `Option<Fallibility>` for recursion. This prevents recursive calls like `classify_fallibility("int")` (from unwrapping `Task<int>`) from spamming the error log on every plain async return type.

```rust
/// Classify a return type string by its outermost generic constructor.
///
/// Input is a bare return type like `Result<i32, Error>` or `Task<OneOf<A, B>>`
/// (no leading `-> `, no surrounding function signature). Walks the string
/// character-by-character until the first `<`, then compares the prefix.
///
/// For C# `Task<OneOf<...>>`, the Task wrapper is stripped and classification
/// recurses into the inner type — `Task` is an async marker, OneOf is the
/// fallibility shape. A plain `Task<int>` stays `Task`.
///
/// Logs an `error!` if the outermost type isn't recognized, since the SQL
/// filter in `query_error_flow` should only admit known shapes.
fn classify_fallibility(return_type: &str) -> Fallibility {
    classify_inner(return_type).unwrap_or_else(|| {
        tracing::error!(
            return_type = %return_type,
            "classify_fallibility received return type that passed SQL filter \
             but matched no known fallibility shape"
        );
        Fallibility::Task
    })
}

/// Recursive helper that returns `None` for unknown outer types, so the
/// recursive `Task<X>` path can silently treat plain `X` as "not fallible"
/// without logging a false-positive anomaly for every `Task<int>` etc.
fn classify_inner(return_type: &str) -> Option<Fallibility> {
    let trimmed = return_type.trim();
    let outer_end = trimmed.find('<').unwrap_or(trimmed.len());
    let outer = trimmed[..outer_end].trim();

    match outer {
        "Result" => Some(Fallibility::Result),
        "Option" => Some(Fallibility::Option),
        "OneOf" => Some(Fallibility::OneOf),
        "Task" => {
            // If Task wraps a fallible shape, classify by the inner type.
            // For a plain `Task<int>` (or Task with no generic), stay Task.
            let inner_fallibility = extract_inner_generic(trimmed)
                .and_then(classify_inner)
                .unwrap_or(Fallibility::Task);
            Some(inner_fallibility)
        }
        _ => None,
    }
}

/// Given `Foo<A, B>`, returns `A, B` (the generic argument list).
/// Returns None if no `<` is present or brackets are unbalanced.
fn extract_inner_generic(s: &str) -> Option<&str> {
    let start = s.find('<')?;
    // Find matching `>` respecting nesting.
    let bytes = s.as_bytes();
    let mut depth: i32 = 0;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        match b {
            b'<' => depth += 1,
            b'>' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start + 1..i]);
                }
            }
            _ => {}
        }
    }
    None
}
```

Delete the old `debug_assert!(false, ...)` fallback — the top-level `tracing::error!` in the public wrapper replaces it and logs in release builds too.

**Why the split:** When `Task<int>` is classified, the original single-function design recurses into `classify_fallibility("int")`, which falls into the `_ => tracing::error!` branch and logs an error. But `int` is a perfectly valid inner type for `Task` — we just want to fall back to `Fallibility::Task`. The split design makes recursion silent and reserves the error log for top-level inputs that actually slipped through the SQL filter unexpectedly.

### Step 4: Run the tests

```bash
cargo nextest run -p tethys classify_fallibility
```

Expected: all four new tests pass. The existing `classify_fallibility_prefers_result_over_option` test at `db/overview.rs:386-392` **does not need its assertion changed** — it asserts `Result<Option<u32>, Error>` → `Fallibility::Result`, which is still correct under the new outermost-generic semantics (the outer constructor is `Result`). What **does** need to change is the test's **input**: the function now takes a bare return type, not a full signature. Update the input from `"fn f() -> Result<Option<u32>, Error>"` to `"Result<Option<u32>, Error>"`. Rename to `classify_fallibility_nested_result_of_option_is_result` for clarity.

The old `classify_fallibility_detects_result`, `_detects_option`, `_detects_one_of`, `_detects_task`, and `_recognizes_task_type` tests at `db/overview.rs:348-383` also need their inputs updated from full signatures to bare return types. Their assertions stay the same.

### Step 5: Commit

```bash
git add crates/tethys/src/db/overview.rs
git commit -m "fix(tethys): classify fallibility by outermost return-type generic"
```

---

## Task 4: Rewrite `return_type()` helper in `overview.rs`

**Why:** `overview.rs:604` `return_type()` does `signature.find("-> ")` which returns the *first* arrow in the signature. Closure parameters with their own arrows break this. The Display formatter then right-aligns the garbage into a column.

**Two options here:**
- **Option 4a (simpler):** Just use the new `FallibleFunction.signature` field's companion `return_type` — we can plumb the structured return type into `FallibleFunction` now that the column exists.
- **Option 4b (keep string parsing):** Rewrite `return_type()` with paren-depth tracking for cases where we only have the signature string.

**CHOSEN: Option 4a.** Since Task 1 added the column and Task 2 pulls it through the query, we can add `return_type: String` to `FallibleFunction` and delete the `return_type()` helper entirely.

**Files:**
- Modify: `crates/tethys/src/overview.rs:576-597` (`FallibleFunction` struct)
- Modify: `crates/tethys/src/overview.rs:599-606` (delete `return_type()` helper)
- Modify: `crates/tethys/src/overview.rs:749-751` (Display impl uses the struct field)
- Modify: `crates/tethys/src/db/overview.rs:343-361` (populate the field from SQL)

### Step 1: Write a failing Display test

```rust
#[test]
fn display_renders_return_type_for_function_with_closure_param() {
    let overview = Overview {
        budget: 100,
        modules: vec![],
        traits: vec![],
        public_api: vec![],
        entry_points: vec![],
        error_flow: vec![FallibleFunction {
            name: "handle".into(),
            signature: "fn handle(cb: impl FnOnce() -> i32) -> Result<(), Error>".into(),
            return_type: "Result<(), Error>".into(),
            fallibility: Fallibility::Result,
        }],
    };
    let text = format!("{overview}");
    assert!(
        text.contains("Result<(), Error>"),
        "Display should render the outer return type: {text}"
    );
    assert!(
        !text.contains("-> i32) -> Result"),
        "Display must not splice the closure param arrow into the return column"
    );
}
```

### Step 2: Run the test and confirm it fails

```bash
cargo nextest run -p tethys display_renders_return_type_for_function_with_closure_param
```

Expected: compile error — `return_type` is not a field on `FallibleFunction`.

### Step 3: Add `return_type` to `FallibleFunction`, populate from SQL, use in Display

```rust
pub struct FallibleFunction {
    pub name: String,
    pub signature: String,
    /// Bare return type (just the type after `->`, without any wrapping).
    pub return_type: String,
    pub fallibility: Fallibility,
}
```

In `db/overview.rs:query_error_flow`, read `return_type` from the SELECT and pass it in.

In `overview.rs` Display impl, replace `let ret = return_type(&func.signature);` with `let ret = &func.return_type;`.

Delete the free-function `return_type()` and its tests (`return_type_returns_full_signature_when_no_arrow`, `return_type_extracts_arrow_substring`) — they no longer have a caller.

### Step 4: Run the tests

```bash
cargo nextest run -p tethys
```

Expected: PASS. Fix any `FallibleFunction { ... }` literal in tests that's missing the new field.

### Step 5: Commit

```bash
git add crates/tethys/src/overview.rs crates/tethys/src/db/overview.rs
git commit -m "fix(tethys): render error-flow return type from structured field"
```

---

## Task 5: Add `kind = 'call'` filter to `populate_call_edges`

**Why:** **Critical.** `db/call_edges.rs:34` currently inserts every resolved `refs` row into `call_edges`, regardless of reference kind. With `ReferenceKind::Inherit` added in this branch, `impl Clone for Foo` now appears as a phantom `Foo → Clone` call edge, poisoning `get_callers`, `get_callees`, `find_cycles`, `shortest_path`, and the new `query_entry_points.callees`. This was a latent bug (Type/Import refs already polluted call_edges); the Inherit addition made it acute.

**Files:**
- Modify: `crates/tethys/src/db/call_edges.rs:34-43`
- Test: new test in the same file (or `tests/integration/` if the project has graph integration tests)

### Step 1: Write a failing test

```rust
#[test]
fn populate_call_edges_excludes_inherit_references() {
    let index = Index::in_memory().expect("open");
    // Insert two symbols: a struct Foo and a trait Clone.
    let file_id = index.insert_file(/* ... */).expect("file");
    let foo_id = index.insert_symbol(/* name: "Foo", kind: "struct" */).expect("foo");
    let clone_id = index.insert_symbol(/* name: "Clone", kind: "trait" */).expect("clone");

    // Insert an Inherit reference: Foo → Clone (via `impl Clone for Foo`).
    index.insert_reference(&InsertReferenceParams {
        file_id,
        symbol_id: Some(clone_id),
        in_symbol_id: Some(foo_id),
        name: "Clone",
        kind: "inherit",
        line: 1,
        column: 1,
    }).expect("insert ref");

    index.populate_call_edges().expect("populate");

    let callers_of_clone = index.get_callers(clone_id).expect("callers");
    assert!(
        callers_of_clone.is_empty(),
        "Foo does not CALL Clone — it IMPLEMENTS it. \
         Inherit refs must not appear as call edges. Got: {callers_of_clone:?}"
    );
}
```

### Step 2: Run the test and confirm it fails

```bash
cargo nextest run -p tethys populate_call_edges_excludes_inherit_references
```

Expected: FAIL — Foo shows up as a caller of Clone.

### Step 3: Add the filter

In `crates/tethys/src/db/call_edges.rs`, the `populate_call_edges` SELECT becomes:

```sql
INSERT INTO call_edges (caller_symbol_id, callee_symbol_id, call_count)
SELECT in_symbol_id, symbol_id, COUNT(*) as call_count
FROM refs
WHERE in_symbol_id IS NOT NULL
  AND symbol_id IS NOT NULL
  AND kind = 'call'
GROUP BY in_symbol_id, symbol_id
ON CONFLICT(caller_symbol_id, callee_symbol_id) DO UPDATE SET
    call_count = call_edges.call_count + excluded.call_count
```

Add a doc-comment note explaining *why* the filter is here: "Only `Call` kind references represent runtime invocations. Type/Import/Construct/Inherit references are structural, not dynamic, and would pollute the call graph."

### Step 4: Run the test

```bash
cargo nextest run -p tethys populate_call_edges_excludes_inherit_references
```

Expected: PASS.

### Step 5: Commit

```bash
git add crates/tethys/src/db/call_edges.rs
git commit -m "fix(tethys): exclude non-call refs from populate_call_edges"
```

---

## Task 6: Add a `clear_and_rebuild_call_edges` helper (optional)

**Why:** This task was originally motivated by existing DBs having polluted `call_edges` rows from before Task 5's filter. With no external users and a "delete `.rivets/index/tethys.db` and re-index" dev workflow, this helper is **not strictly necessary for correctness** — a fresh re-index populates `call_edges` from scratch with the new filter in place.

**Keep or skip:** The helper is ~5 lines and serves as a defensive API for future cases where someone pollutes the table (e.g. an experimental populator that writes buggy edges). Keeping it is cheap insurance; skipping is fine if the plan is being trimmed. Default is "keep".

**Files:**
- Modify: `crates/tethys/src/db/call_edges.rs` (add method)
- Modify: `crates/tethys/src/lib.rs` (expose on `Tethys`)
- Doc: add to release notes / CHANGELOG if the project maintains one

### Step 1: Write a test

```rust
#[test]
fn clear_and_rebuild_call_edges_removes_polluted_rows() {
    let index = Index::in_memory().expect("open");
    // Simulate a polluted state by inserting a call_edges row that doesn't
    // correspond to a Call ref (as would exist in a pre-fix DB).
    index.clear_all_call_edges().expect("clear");
    // ... set up refs with mixed kinds ...
    // Manually INSERT a polluted edge:
    index.connection().unwrap().execute(
        "INSERT INTO call_edges (caller_symbol_id, callee_symbol_id, call_count) VALUES (1, 2, 1)",
        [],
    ).expect("pollute");

    index.clear_and_rebuild_call_edges().expect("rebuild");

    let all_edges: usize = index.connection().unwrap()
        .query_row("SELECT COUNT(*) FROM call_edges", [], |r| r.get(0))
        .expect("count");
    // With no Call-kind refs, the rebuild produces zero edges.
    assert_eq!(all_edges, 0, "rebuild should clear polluted rows");
}
```

### Step 2: Run and confirm it fails (method doesn't exist)

### Step 3: Add the method

```rust
/// Clear `call_edges` and rebuild from the current `refs` table.
///
/// Use this after upgrading from a Tethys version that had the call-edges
/// pollution bug (Call/Type/Import/Construct/Inherit refs all inserted).
/// New indexes don't need this — the bug was fixed in `populate_call_edges`.
pub fn clear_and_rebuild_call_edges(&self) -> Result<usize> {
    self.clear_all_call_edges()?;
    self.populate_call_edges()
}
```

### Step 4: Run the test, commit

```bash
git add crates/tethys/src/db/call_edges.rs
git commit -m "feat(tethys): add clear_and_rebuild_call_edges recovery helper"
```

---

## Task 7: Move `set_symbol_parent` loop inside `index_file_atomic`

**Why:** **Critical.** The current post-insert loop at `indexing.rs:640-654` runs outside the transaction committed at `db/files.rs:188`. A failure mid-loop leaves files with partially-wired parent pointers and no references stored. `index_file_atomic`'s name is a lie. Per D5, we fix this by computing the parent-name lookup inside the transaction and threading `parent_symbol_id` through at INSERT time — OR by extending `index_file_atomic` to resolve after INSERTs but before commit.

**Files:**
- Modify: `crates/tethys/src/db/files.rs:91-190` (`index_file_atomic`)
- Modify: `crates/tethys/src/indexing.rs:627-654` (remove post-commit loop)

### Step 1: Write a failing test asserting atomicity

```rust
#[test]
fn index_file_atomic_rolls_back_all_symbols_if_parent_resolution_fails() {
    // Construct a SymbolData batch where one symbol has an unresolvable
    // parent_name AND the SQL is rigged to fail on that row specifically
    // (e.g. by using an invalid kind that violates a CHECK constraint).
    // Assert: on failure, zero symbols from the batch were committed.
    //
    // This test is load-bearing proof that the parent-resolution step
    // participates in the same transaction as the symbol INSERTs.
    //
    // Alternative: use a smaller assertion — after a forced failure,
    // query `SELECT COUNT(*) FROM symbols WHERE file_id = ?` and assert 0.
    // ... implementation left as exercise for the executor ...
}
```

(The exact setup depends on how easily the project's test helpers can inject a failure into an `index_file_atomic` call. If it's awkward, a simpler test is: index a file with a valid trait + methods, query `symbols` immediately after `index_file_atomic` returns, confirm all `parent_symbol_id` fields are populated in one read — proving resolution happened inside the transaction.)

### Step 2: Confirm the test fails or is blocked by the current architecture

### Step 3: Move parent resolution inside `index_file_atomic`

**Add `parent_name` to `SymbolData`.** The production type for `index_file_atomic` is `SymbolData` at `db/mod.rs:59` (not `InsertSymbolParams`, which is the `#[cfg(test)]`-gated test helper). `SymbolData` currently has `parent_symbol_id: Option<SymbolId>` but no `parent_name` — add a new field:

```rust
// db/mod.rs:59
pub struct SymbolData<'a> {
    // ... existing fields ...
    pub parent_symbol_id: Option<crate::types::SymbolId>,
    /// Name of the enclosing parent (trait/impl/class). Resolved to a SymbolId
    /// inside `index_file_atomic` after all symbols in the file are inserted.
    pub parent_name: Option<&'a str>,
    pub is_test: bool,
}
```

Update `OwnedSymbolData::as_symbol_data()` at `parallel.rs:131` to populate it:

```rust
parent_name: self.parent_name.as_deref(),
```

**Move resolution into the transaction.** In `db/files.rs:index_file_atomic`, after the symbol INSERT loop and before `tx.commit()?`:

```rust
// Build a (name, module_path) → SymbolId map from the just-inserted
// symbols for same-file parent resolution. See D3 for why module_path
// is part of the key.
let mut name_to_id: HashMap<(&str, &str), SymbolId> =
    HashMap::with_capacity(symbols.len());
for (sym, &id) in symbols.iter().zip(&symbol_ids) {
    name_to_id.insert((sym.name, sym.module_path), id);
}

// Resolve parent_name → parent_symbol_id for each symbol that has one.
// Cross-file parents are not resolved here — see Task 9.
for (sym, &child_id) in symbols.iter().zip(&symbol_ids) {
    let Some(parent_name) = sym.parent_name else { continue };
    if let Some(&parent_id) = name_to_id.get(&(parent_name, sym.module_path)) {
        tx.execute(
            "UPDATE symbols SET parent_symbol_id = ?1 WHERE id = ?2",
            params![parent_id.as_i64(), child_id.as_i64()],
        )?;
    }
}

tx.commit()?;
```

Delete the post-commit loop in `indexing.rs:640-654` and drop the now-unused `Self::build_symbol_maps_from_data` call's `name_to_id` return — `store_references` still needs it, so the function continues to return it, but for a *different* purpose (reference resolution, not parent resolution).

**Important:** `store_references` in `indexing.rs:711` still uses `name_to_id` for cross-symbol reference lookup and runs *after* `index_file_atomic` returns. That path is unaffected by this task — we're only moving parent resolution, not reference resolution.

### Step 4: Run tests

```bash
cargo nextest run -p tethys
```

### Step 5: Commit

```bash
git add crates/tethys/src/db/files.rs crates/tethys/src/indexing.rs
git commit -m "fix(tethys): resolve parent_symbol_id inside index_file_atomic transaction"
```

---

## Task 8: Fix `name_to_id` duplicate-name silent overwrite — DEFERRED

> **DEFERRED — see [`rivets-w02j`](../../.rivets/issues.jsonl) and the errata section in [`2026-04-07-tethys-overview-tasks-1-3-review.md`](./2026-04-07-tethys-overview-tasks-1-3-review.md).**
>
> During execution of Task 7, the design rationale below was found to be wrong: `compute_module_path_for_file` in `crates/tethys/src/lib.rs:133` returns *one* `module_path` per file, and `write_parsed_file` at `crates/tethys/src/indexing.rs:608-617` assigns that single value to every symbol in the file. So two `struct Foo` in nested mods within one `lib.rs` get *identical* `(name, module_path)` keys — the proposed fix is a no-op.
>
> The real fix requires the parser to track nested mod/namespace paths per symbol, which is a multi-day refactor of `languages/rust.rs` and `languages/csharp.rs`. That work is tracked as **rivets-w02j**. Task 8 itself is deferred until rivets-w02j lands; at that point the `(name, module_path)` key becomes meaningful and this task can be implemented as a small follow-up.
>
> **Skip the steps below.** They are preserved as historical context for the original (flawed) design.

**Why:** `build_symbol_maps_from_data` in `indexing.rs:687` keys on `sym.name` alone. Two `struct Foo` in the same file (nested in different `mod` blocks) compile in Rust, but one overwrites the other in the map. Inherit reference resolution silently binds to the wrong type.

Per D3 (Option A chosen), we change the key to `(name, module_path)`. This handles the nested-mod duplicate case because `struct Foo` in `crate::outer` and `struct Foo` in `crate::outer::tests` have different `module_path` values, and `module_path` is already populated on `OwnedSymbolData` (`parallel.rs:73`) and `SymbolData` (`db/mod.rs:61`) — no parser changes required. **(Wrong — see deferral note above.)**

**Files:**
- Modify: `crates/tethys/src/indexing.rs:679-702` (`build_symbol_maps_from_data` — change key type)
- Modify: `crates/tethys/src/indexing.rs:711-768` (`store_references` — update lookups to include `module_path`)

### Step 1: Write a failing test

This test goes through the full indexer to verify Inherit resolution picks the right `Foo` when there are duplicates in nested modules. Use a tempdir + `Tethys::new()` + `Tethys::index()` + `query_trait_map()` rather than in-memory `Index` helpers, because Inherit resolution runs during the full indexing pipeline, not in isolation.

```rust
#[test]
fn inherit_resolution_survives_duplicate_struct_names_in_nested_mod() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let src = workspace.path().join("src");
    std::fs::create_dir_all(&src).expect("mkdir src");
    std::fs::write(
        workspace.path().join("Cargo.toml"),
        "[package]\nname = \"t\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n",
    )
    .expect("write cargo toml");
    std::fs::write(
        src.join("lib.rs"),
        r#"
pub trait OuterMarker {}
pub trait InnerMarker {}

pub struct Foo;
impl OuterMarker for Foo {}

pub mod nested {
    pub struct Foo;
    impl super::InnerMarker for Foo {}
}
"#,
    )
    .expect("write lib.rs");

    let mut tethys = Tethys::new(workspace.path()).expect("new");
    tethys.index().expect("index");

    let overview = tethys.overview(500).expect("overview");

    let outer = overview
        .traits
        .iter()
        .find(|t| t.name.ends_with("OuterMarker"))
        .expect("OuterMarker trait should be in overview");
    let inner = overview
        .traits
        .iter()
        .find(|t| t.name.ends_with("InnerMarker"))
        .expect("InnerMarker trait should be in overview");

    // Each trait should have exactly one implementor, and they should be
    // different Foos (distinguishable by module_path in qualified_name).
    assert_eq!(outer.implementors.len(), 1, "OuterMarker implementors");
    assert_eq!(inner.implementors.len(), 1, "InnerMarker implementors");
    assert_ne!(
        outer.implementors[0].name, inner.implementors[0].name,
        "the two Foos should not both resolve to the same symbol"
    );
    assert!(
        !outer.implementors[0].name.contains("nested"),
        "OuterMarker should implement the top-level Foo, got {}",
        outer.implementors[0].name
    );
    assert!(
        inner.implementors[0].name.contains("nested"),
        "InnerMarker should implement nested::Foo, got {}",
        inner.implementors[0].name
    );
}
```

### Step 2: Run and confirm failure

```bash
cargo nextest run -p tethys inherit_resolution_survives_duplicate_struct_names_in_nested_mod
```

Expected: FAIL — both Inherit refs resolve to whichever Foo got inserted second (the nested one), so OuterMarker ends up with zero implementors and InnerMarker ends up with one.

### Step 3: Change the map key to `(name, module_path)`

In `indexing.rs:679-702`:

```rust
fn build_symbol_maps_from_data(
    symbols: &[SymbolData<'_>],
    symbol_ids: &[SymbolId],
) -> (
    HashMap<(String, String), SymbolId>,
    HashMap<Span, SymbolId>,
) {
    let mut name_to_id: HashMap<(String, String), SymbolId> = HashMap::new();
    let mut span_to_id: HashMap<Span, SymbolId> = HashMap::new();

    for (sym, &id) in symbols.iter().zip(symbol_ids) {
        let key = (sym.name.to_string(), sym.module_path.to_string());
        if let Some(prev_id) = name_to_id.insert(key, id) {
            tracing::warn!(
                name = %sym.name,
                module_path = %sym.module_path,
                new_id = %id,
                prev_id = %prev_id,
                "duplicate (name, module_path) in file — Inherit resolution may be ambiguous"
            );
        }

        if let Some(span) = sym.span {
            span_to_id.insert(span, id);
        }
    }

    (name_to_id, span_to_id)
}
```

### Step 4: Update `store_references` lookups

`store_references` in `indexing.rs:711-768` currently does `name_to_id.get(&r.name)` and `name_to_id.get(&qualified_name)`. These need the module path as the second key component. Two options:

1. **Same-module assumption**: most references within a file are to symbols in the same module. Pass the *reference's* module path (reachable via the containing symbol's module, or the file's top-level module) as the second key.
2. **Multi-lookup**: iterate candidate module paths (reference's own module, parents, global) and try each.

**Recommended (simpler):** Use option 1 with a fallback scan. For each reference, look up `(name, ref_module_path)`; if not found, fall back to iterating `name_to_id.values()` and matching by name alone with a `warn!` on ambiguity. This preserves the current behavior for the common case and logs when the nested-mod edge case fires.

Concrete change:

```rust
// Try exact (name, module_path) match first.
let symbol_id = name_to_id
    .get(&(r.name.clone(), ref_module_path.to_string()))
    .or_else(|| name_to_id.get(&(qualified_name.clone(), ref_module_path.to_string())))
    .copied()
    .or_else(|| {
        // Fallback: name-only match with ambiguity warning.
        let matches: Vec<_> = name_to_id
            .iter()
            .filter(|((n, _), _)| n == &r.name || n == &qualified_name)
            .collect();
        if matches.len() > 1 {
            tracing::warn!(
                reference_name = %r.name,
                candidate_count = matches.len(),
                "ambiguous same-file reference; picking first match"
            );
        }
        matches.first().map(|(_, &id)| id)
    });
```

`ref_module_path` comes from the containing symbol: if `r.containing_symbol_span` resolves to a symbol, use that symbol's `module_path`; otherwise use the file's top-level module (track it in `ParsedFileData` or compute from `relative_path`).

### Step 5: Run tests

```bash
cargo nextest run -p tethys
```

Expected: the new test passes. Existing tests that indirectly exercised name-based lookup should continue to pass — `(name, module_path)` is strictly more specific than name alone for the common case (single module per file), and the fallback branch preserves behavior for references that don't match a module.

### Step 6: Commit

```bash
git add crates/tethys/src/indexing.rs
git commit -m "fix(tethys): key name_to_id on (name, module_path) to disambiguate nested duplicates"
```

---

## Task 9: Upgrade cross-file parent-miss log to `warn!`

**Why:** `indexing.rs:646-652` logs at `trace!` when a parent name isn't in the current file's `name_to_id`. Nobody reads `trace!`. This is invisible data loss for `impl Display for MyError` patterns where `MyError` is imported. Per D2, we upgrade the log and file a follow-up for Pass 2 resolution.

**Files:**
- Modify: `crates/tethys/src/db/files.rs` (wherever Task 7 put the resolution loop)
- Add a rivets issue for structural fix (manual step, not code)

### Step 1: Change `trace!` to `warn!` in the miss branch

```rust
} else {
    // No matching parent in the current file. Cross-file parent resolution
    // is not implemented yet (see rivets-XXXX). For public symbols, the
    // missing parent is a visible gap in the Public API layer grouping.
    if sym.visibility == Visibility::Public {
        tracing::warn!(
            child = %sym.name,
            parent_name = %parent_name,
            file = %relative_path.display(),
            "public symbol parent not resolvable within file; parent_symbol_id left NULL \
             (cross-file parent resolution is tracked in rivets-XXXX)"
        );
    } else {
        tracing::debug!(
            child = %sym.name,
            parent_name = %parent_name,
            file = %relative_path.display(),
            "non-public symbol parent not found in file; parent_symbol_id left NULL"
        );
    }
}
```

### Step 2: Create a rivets issue for the structural fix

Run (manually, do not automate in this plan):

```bash
rivets create \
  --title "Cross-file parent_symbol_id resolution (Pass 2)" \
  --type feature \
  --priority 3 \
  --description "Currently set_symbol_parent only resolves within a single file. Add a post-indexing Pass 2 that scans unresolved parent_name values and looks them up against the full symbols table, mirroring how reference resolution already works." \
  --design "In indexing::build_index after all files are indexed but before populate_call_edges, SELECT id, name, parent_name FROM symbols WHERE parent_symbol_id IS NULL AND parent_name IS NOT NULL; for each row, look up by (parent_name, module_path) or qualified name and UPDATE."
```

Replace `rivets-XXXX` in the code comment with the returned ID.

### Step 3: Run tests, commit

```bash
git add crates/tethys/src/db/files.rs
git commit -m "fix(tethys): warn on unresolved public-symbol parents (rivets-XXXX follow-up)"
```

---

## Task 10: `truncate_traits` and `truncate_public_api` — `break` → `continue`

**Why:** `overview.rs:804-815` and `overview.rs:818-829` both use `break` on budget overflow, meaning a single oversized entry drops every subsequent smaller entry. Dinesh defended this as "half a trait is useless" in Round 1 but had to concede in Round 2 once Gilfoyle pointed at the `break` statement. The fix is `continue` — skip the oversized entry, keep accepting smaller ones.

**Files:**
- Modify: `crates/tethys/src/overview.rs:804-815` (`truncate_traits`)
- Modify: `crates/tethys/src/overview.rs:818-829` (`truncate_public_api`)
- Test: same file

### Step 1: Write failing tests

```rust
#[test]
fn truncate_traits_skips_oversized_entry_and_keeps_smaller_ones() {
    let mut traits = vec![
        // This one is too large for the budget.
        TraitEntry {
            name: "Huge".into(),
            file: "a.rs".into(),
            line: 1,
            methods: (0..40).map(|i| format!("fn m{i}()")).collect(),
            implementors: vec![],
        },
        // This one fits.
        TraitEntry {
            name: "Small".into(),
            file: "b.rs".into(),
            line: 1,
            methods: vec!["fn go()".into()],
            implementors: vec![],
        },
    ];
    // Budget=5 can't fit Huge (41 lines) but can fit Small (2 lines).
    truncate_traits(&mut traits, 5);
    assert_eq!(traits.len(), 1, "Small should survive after skipping Huge");
    assert_eq!(traits[0].name, "Small");
}

#[test]
fn truncate_public_api_skips_oversized_module_and_keeps_smaller_ones() {
    let mut modules = vec![
        PublicApiModule {
            module_path: "big".into(),
            symbols: (0..30).map(|i| PublicSymbol {
                kind: "function".into(),
                name: format!("f{i}"),
                signature: None,
                parent: None,
            }).collect(),
        },
        PublicApiModule {
            module_path: "small".into(),
            symbols: vec![PublicSymbol {
                kind: "struct".into(),
                name: "S".into(),
                signature: None,
                parent: None,
            }],
        },
    ];
    truncate_public_api(&mut modules, 5);
    assert_eq!(modules.len(), 1);
    assert_eq!(modules[0].module_path, "small");
}
```

### Step 2: Run and confirm failure

### Step 3: Change `break` to `continue` in both functions

```rust
pub(crate) fn truncate_traits(traits: &mut Vec<TraitEntry>, budget: usize) {
    let mut total = 0;
    traits.retain(|entry| {
        let entry_lines = trait_line_count(entry);
        if total + entry_lines > budget {
            false  // skip this entry, keep trying smaller ones
        } else {
            total += entry_lines;
            true
        }
    });
}

pub(crate) fn truncate_public_api(modules: &mut Vec<PublicApiModule>, budget: usize) {
    let mut total = 0;
    modules.retain(|module| {
        let module_lines = 1 + module.symbols.len();
        if total + module_lines > budget {
            false
        } else {
            total += module_lines;
            true
        }
    });
}
```

`Vec::retain` with closure-captured mutable state is the idiomatic Rust way. Note that existing tests for these functions expect `keep = i; break;` semantics — verify both old tests still pass under the new behavior (the single-entry-truncation test should still work; the "skip and continue" semantics is strictly more permissive).

### Step 4: Run tests

```bash
cargo nextest run -p tethys truncate_
```

### Step 5: Commit

```bash
git add crates/tethys/src/overview.rs
git commit -m "fix(tethys): truncate_traits/public_api skip oversized entries instead of breaking"
```

---

## Task 11: Add index on `symbols.parent_symbol_id`

**Why:** `db/overview.rs:query_trait_map` runs a subquery per trait filtering `WHERE parent_symbol_id = ?1 AND kind = 'method'`. There is no index on `parent_symbol_id`; the query falls back to `idx_symbols_kind` (or a full scan). For a codebase with thousands of traits, it's `num_traits × O(kind-matched symbols)`.

**Files:**
- Modify: `crates/tethys/src/db/schema.rs` (add partial index to `SCHEMA`)

### Step 1: Add a benchmark or timing test (optional)

If the project has a criterion bench suite, a before/after benchmark is the proof. Otherwise skip to Step 2.

### Step 2: Add the index

In `db/schema.rs` near the other symbols indexes:

```sql
CREATE INDEX IF NOT EXISTS idx_symbols_parent
  ON symbols(parent_symbol_id)
  WHERE parent_symbol_id IS NOT NULL;
```

Partial index keeps it small — most symbols have NULL parents.

As noted in Task 1, there is no schema versioning system; `CREATE INDEX IF NOT EXISTS` is enough to bring new databases into shape, and the dev workflow for this branch is "delete `.rivets/index/tethys.db` and re-index".

### Step 3: Run tests

```bash
cargo nextest run -p tethys
```

### Step 4: Commit

```bash
git add crates/tethys/src/db/schema.rs
git commit -m "perf(tethys): add partial index on symbols.parent_symbol_id"
```

---

## Task 12: Budget accounting — subtract overhead from layer allocations

**Why:** `lib.rs:786` `Tethys::overview()` allocates budget per layer but doesn't account for the fixed section-header + blank-line overhead (~9 lines for the 5-layer design). `--budget 100` produces ~109 rendered lines. Per D4, we subtract the overhead so `budget` is an honest upper bound.

**Files:**
- Modify: `crates/tethys/src/overview.rs` (add `OVERVIEW_OVERHEAD` const near other constants)
- Modify: `crates/tethys/src/lib.rs:786-813` (`Tethys::overview`)
- Test: `crates/tethys/src/lib.rs` (add budget assertion test)

### Step 1: Write failing budget-bound tests

Two tests: one for a content-heavy overview that exercises all five layers, and one for a sparse overview (modules only) that verifies the `saturating_sub` doesn't misbehave when the actual overhead is much less than `OVERVIEW_OVERHEAD`.

```rust
#[test]
fn overview_rendered_output_respects_budget_ceiling() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let src = workspace.path().join("src");
    std::fs::create_dir_all(&src).expect("mkdir");
    // Large file with many public functions to saturate modules + public_api + error_flow.
    let mut content = String::new();
    for i in 0..50 {
        std::fmt::Write::write_fmt(
            &mut content,
            format_args!("pub fn f{i}() -> Result<(), String> {{ Ok(()) }}\n"),
        ).unwrap();
    }
    std::fs::write(src.join("lib.rs"), content).expect("write");
    std::fs::write(
        workspace.path().join("Cargo.toml"),
        "[package]\nname = \"t\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n",
    ).expect("cargo toml");

    let mut tethys = Tethys::new(workspace.path()).expect("new");
    tethys.index().expect("index");

    let budget = 50;
    let overview = tethys.overview(budget).expect("overview");
    let rendered = format!("{overview}");
    let line_count = rendered.lines().count();
    assert!(
        line_count <= budget,
        "rendered output ({line_count} lines) must not exceed budget ({budget})"
    );
}

#[test]
fn overview_respects_budget_ceiling_with_sparse_layers() {
    // A workspace with just modules (no traits, no public API, no main, no Result-returning
    // functions). Verifies the overhead subtraction doesn't underflow and the sparse output
    // still fits under the budget.
    let workspace = tempfile::tempdir().expect("tempdir");
    let src = workspace.path().join("src");
    std::fs::create_dir_all(&src).expect("mkdir");
    std::fs::write(src.join("lib.rs"), "fn private_helper() {}\n").expect("write");
    std::fs::write(
        workspace.path().join("Cargo.toml"),
        "[package]\nname = \"t\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n",
    )
    .expect("cargo toml");

    let mut tethys = Tethys::new(workspace.path()).expect("new");
    tethys.index().expect("index");

    for &budget in &[5usize, 12, 100] {
        let overview = tethys.overview(budget).expect("overview");
        let rendered = format!("{overview}");
        let line_count = rendered.lines().count();
        assert!(
            line_count <= budget,
            "sparse rendered output ({line_count} lines) must not exceed budget ({budget})"
        );
    }
}
```

The sparse test documents that `OVERVIEW_OVERHEAD = 9` is a conservative worst-case estimate. For budgets smaller than 9, `saturating_sub` clamps `data_budget` to 0 and the overview degrades gracefully (headers only, no content rows).

### Step 2: Run and confirm it fails

Expected: `line_count > budget` because of header/separator overhead.

### Step 3: Subtract overhead in `Tethys::overview`

In `overview.rs`:

```rust
/// Fixed rendering overhead for a fully populated overview:
/// 5 section headers + 4 inter-section blank lines.
/// Subtract this from the user's budget before allocating to layers.
pub const OVERVIEW_OVERHEAD: usize = 9;
```

In `lib.rs:Tethys::overview`:

```rust
pub fn overview(&self, budget: usize) -> Result<overview::Overview> {
    let data_budget = budget.saturating_sub(overview::OVERVIEW_OVERHEAD);
    let allocation = overview::BudgetAllocation::from_total(data_budget);
    // ... rest unchanged ...
}
```

### Step 4: Run the test

Expected: PASS.

### Step 5: Commit

```bash
git add crates/tethys/src/lib.rs crates/tethys/src/overview.rs
git commit -m "fix(tethys): subtract fixed rendering overhead from overview budget"
```

---

## Task 13: Add `#[non_exhaustive]` to `EntryPointKind` and `Fallibility`

**Why:** Both enums are public, `Serialize`-derived, and currently missing `#[non_exhaustive]`. `EntryPointKind::LibraryExport` is shipped but never produced — future emission is a SemVer-major for downstream Rust consumers. Same risk for future `Fallibility` variants (e.g. `ValueTask` for C#).

**Files:**
- Modify: `crates/tethys/src/overview.rs:567-574` (`EntryPointKind`)
- Modify: `crates/tethys/src/overview.rs:587-597` (`Fallibility`)

### Step 1: Add attributes

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[non_exhaustive]
pub enum EntryPointKind {
    BinaryMain,
    LibraryExport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[non_exhaustive]
pub enum Fallibility {
    Result,
    Option,
    OneOf,
    Task,
}
```

### Step 2: Run tests

```bash
cargo nextest run -p tethys
```

Expected: PASS. Any downstream consumer in the workspace that does `match kind { BinaryMain => ..., LibraryExport => ... }` without a wildcard arm will now get a warning — add `_ => unreachable!()` or a wildcard to satisfy `#[non_exhaustive]`.

### Step 3: Commit

```bash
git add crates/tethys/src/overview.rs
git commit -m "chore(tethys): mark EntryPointKind and Fallibility as non_exhaustive"
```

---

## Task 14: `.unwrap()` → `.expect()` in overview tests

**Why:** `lib.rs:1297,1305` (approximately) use `.unwrap()` on `result_fn` and `option_fn` in the new overview integration tests. `CLAUDE.md` explicitly mandates `.expect("descriptive message")` in tests for better failure output.

**Files:**
- Modify: `crates/tethys/src/lib.rs` (the two `.unwrap()` calls in the error-flow test)

### Step 1: Replace

```rust
let result_fn = overview
    .error_flow
    .iter()
    .find(|f| f.name.contains("always_ok"))
    .expect("always_ok should be in error_flow");
// ... and same for option_fn ...
```

### Step 2: Run, commit

```bash
cargo nextest run -p tethys overview_extracts_fallible_functions_returning_result
git add crates/tethys/src/lib.rs
git commit -m "test(tethys): use .expect() instead of .unwrap() in overview tests"
```

---

## Task 15: `#[from] serde_json::Error` variant on `tethys::Error`

**Why:** `cli/overview.rs:31-33` flattens `serde_json::Error` into a `format!`'d string via `Error::Internal`, losing the error source chain. Adding a `#[from]` variant preserves structure.

**Files:**
- Modify: `crates/tethys/src/error.rs` (add variant)
- Modify: `crates/tethys/src/cli/overview.rs:31-33` (use `?`)

### Step 1: Add the variant

In `error.rs`:

```rust
#[derive(thiserror::Error, Debug)]
pub enum Error {
    // ... existing variants ...

    #[error("JSON serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}
```

### Step 2: Use `?` in cli/overview.rs

```rust
if json {
    let rendered = serde_json::to_string_pretty(&overview)?;
    println!("{rendered}");
}
```

### Step 3: Run, commit

```bash
cargo nextest run -p tethys
git add crates/tethys/src/error.rs crates/tethys/src/cli/overview.rs
git commit -m "refactor(tethys): add From<serde_json::Error> on tethys::Error"
```

---

## Task 16: Fix `classify_fallibility` docstring about ordering reason

**Why:** `db/overview.rs:365-369` docstring says OneOf is checked first because OneOf signatures "happen to also contain `<` and `>`" — but every generic type contains `<` and `>`. The real reason (before the Task 3 rewrite) was C# `Task<OneOf<...>>` requiring OneOf to win over Task.

After Task 3 this function is a state machine, not a substring-check, so the docstring should describe the *current* recursive Task-unwrapping behavior instead of the old ordering.

**Files:**
- Modify: `crates/tethys/src/db/overview.rs:365-369`

### Step 1: Rewrite the docstring

```rust
/// Classify a return type into a [`Fallibility`] category by its outermost
/// generic constructor.
///
/// For the C# convention of wrapping fallible results in async — e.g.
/// `Task<OneOf<Success, Error>>` — the outer `Task` is stripped and we
/// recurse into the inner type. A plain `Task<int>` stays `Task`.
///
/// This function receives the bare return type (no leading `-> `,
/// no function signature), which is stored on `symbols.return_type`
/// by the indexer.
fn classify_fallibility(return_type: &str) -> Fallibility {
    // ...
}
```

### Step 2: Commit

```bash
git add crates/tethys/src/db/overview.rs
git commit -m "docs(tethys): accurate docstring for classify_fallibility state machine"
```

---

## Task 17: `query_trait_map` — distinguish concrete implementors from interface-extends-interface

**Why:** For C# `interface IReadableStream : IStream`, the current `query_trait_map` treats `IReadableStream` as an implementor of `IStream`. It's actually a sub-interface. For the overview's purpose ("who concretely implements this?"), mixing sub-interfaces into the implementors list confuses the picture.

Two options:
- **Option A:** Filter implementors by `impl_sym.kind NOT IN ('trait', 'interface')`.
- **Option B:** Add a distinct `ReferenceKind::Extends` for interface-to-interface inheritance. More work but more semantically accurate.

**CHOSEN: Option A.** Matches the existing extraction scheme without adding a new ReferenceKind.

**Files:**
- Modify: `crates/tethys/src/db/overview.rs:149-177` (implementor SELECT)
- Test: new test for interface-extends-interface

### Step 1: Write failing tests (C# sub-interface + Rust supertrait)

We need two tests because the filter affects both C# inheritance (interface extends interface) and Rust supertraits (trait bounds like `trait Sub: Super`). Both are emitted as `Inherit` references by the parsers, and we want both excluded from the "concrete implementors" list. The tests pin that behavior so a future refactor can't silently reintroduce trait-on-trait entries.

```rust
#[test]
fn query_trait_map_excludes_subinterfaces_from_implementors() {
    // Set up C# fixture:
    //   interface IStream { }
    //   interface IReadableStream : IStream { }
    //   class FileStream : IReadableStream { }
    //
    // Assert: IStream.implementors should contain only FileStream after the
    // fix (concrete types only). IReadableStream is itself an interface and
    // must not appear in IStream's implementor list.
}

#[test]
fn query_trait_map_excludes_rust_supertraits_from_implementors() {
    // Set up Rust fixture:
    //   pub trait Base {}
    //   pub trait Extended: Base {}
    //   pub struct Concrete;
    //   impl Base for Concrete {}
    //   impl Extended for Concrete {}
    //
    // Assert: Base.implementors should contain only Concrete, NOT Extended.
    // Extended is a supertrait-bound, not a concrete implementer, and its
    // relationship belongs in a separate "trait hierarchy" layer (out of
    // scope for the overview today).
}
```

**Design decision (locked in by this test):** The overview's "who implements this trait?" answer should be the set of **concrete types** that carry implementations, not other traits that transitively extend it. If a future plan wants to show trait hierarchies, it should be a new layer (or a new field on `TraitEntry`), not a mix-in of the implementor list.

### Step 2: Run and fail

### Step 3: Add the kind filter to the implementor SELECT

```sql
SELECT DISTINCT impl_sym.qualified_name, impl_file.path, r.line
FROM refs r
JOIN symbols impl_sym ON impl_sym.id = r.in_symbol_id
JOIN files impl_file ON impl_file.id = impl_sym.file_id
WHERE r.symbol_id = ?1
  AND r.kind = 'inherit'
  AND impl_sym.kind NOT IN ('trait', 'interface')
ORDER BY impl_sym.qualified_name
```

### Step 4: Run, commit

```bash
cargo nextest run -p tethys query_trait_map
git add crates/tethys/src/db/overview.rs
git commit -m "fix(tethys): exclude sub-interfaces from trait-map implementor list"
```

---

## Verification Checklist

After all tasks are complete, run the full suite and manual sanity checks:

```bash
# All tests pass
cargo nextest run -p tethys

# No clippy warnings
cargo clippy -p tethys --all-targets -- -D warnings

# Format clean
cargo fmt --check

# End-to-end: run the fixed overview on rivets itself
cargo run -p tethys -- overview --budget 100

# Verify Inherit refs no longer appear in call graph (smoke test)
cargo run -p tethys -- callers <some_trait>   # should not list types that merely impl it
```

**Success criteria:**
- All 17 findings resolved
- `cargo nextest run -p tethys` green
- `cargo run -p tethys -- overview --budget 100` prints ≤100 lines
- Manual spot-check: a function like `fn handle(cb: impl FnOnce() -> i32) -> Result<(), Error>` appears in Error Flow with `Result<(), Error>` as its return type, not the closure's `-> i32) -> Result<...>`.

---

## Notes for the Executor

- **Task 1 is load-bearing.** It unblocks Tasks 2, 3, 4, and 16. Do it first.
- **Tasks 5 and 6 can run in parallel** with Tasks 1–4 — they touch a different file (`db/call_edges.rs`).
- **Task 7 depends on Task 1** because the `SymbolData` type changes in both (Task 1 adds `return_type`, Task 7 adds `parent_name`). Do Task 1 first to avoid rebasing the `SymbolData` field list twice.
- **Task 8 is deferred** — the original design was based on a faulty assumption about per-symbol module paths. Tracked as `rivets-w02j`. See the deferral note at the top of Task 8 below.
- **Tasks 10–17 are independent** and can be done in any order.
- **Commit after every task.** Per project convention (CLAUDE.md → Conventional Commits), use appropriate types: `fix`, `feat`, `perf`, `chore`, `docs`, `test`, `refactor`.
- **Use `.expect("descriptive message")`** in all new tests. Use structured `tracing::*` fields for all new log calls.
- **Do not add code comments** beyond what's already shown in this plan — the project style favors self-explanatory code and only comments on non-obvious *why*.
