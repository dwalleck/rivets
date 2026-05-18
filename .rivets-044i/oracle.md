# Oracle for rivets-044i

## Probe under test

`crates/tethys/tests/probe_044i.rs` — two `#[test]` cases that build synthetic
workspaces, run `Tethys::index()`, and query `tethys.db` for whether the
qualified ref is resolved.

## Oracle: source-text inspection (independent of resolver)

The probe asks: "does the resolver link refA → symbolB?" The oracle answers
the same question by a fully different mechanism — **reading the source files
that the test builds** and applying Rust's own rules to determine ground
truth.

### Shape #1 (sibling module under same crate)

Workspace contents (verbatim from the fixture in `probe_044i.rs`):

```
Cargo.toml          [package] name = "crate_a"
src/lib.rs          mod helper;  pub fn entry() { helper::do_thing_q(); }
src/helper.rs       pub fn do_thing_q() {}
```

Ground truth by Rust language rules:
- `lib.rs` declares submodule `helper` via `mod helper;`
- `helper::do_thing_q()` is a fully-qualified path within `crate_a`
- `do_thing_q` is `pub fn` in `helper.rs`, visible to its parent module
- Therefore the call MUST bind to the definition at `src/helper.rs::do_thing_q`

The oracle does not invoke any tethys resolver internals. It uses the same
mechanism `rustc` would: lexical mod tree + path lookup.

### Shape #2 (workspace-crate prefix from import-less integration test)

Workspace contents:

```
Cargo.toml                          [workspace] members = ["crate_a", "crate_b"]
crate_a/src/lib.rs                  pub struct Widget; impl Widget { pub fn make_widget_044i() -> Self }
crate_b/Cargo.toml                  [dependencies] crate_a = { path = "../crate_a" }
crate_b/tests/it.rs                 crate_a::Widget::make_widget_044i()
```

Ground truth: `crate_b` lists `crate_a` as a dependency. Per Rust 2018+ path
prefix rules, `crate_a::Widget::make_widget_044i()` from `crate_b` binds to
`make_widget_044i` in `crate_a/src/lib.rs`. The oracle requires no resolver
component; it is the standard Rust crate-resolution algorithm.

## Probe result (pre-fix)

```
PROBE 044i state: total_refs=1, resolved_refs=0, resolved_to_target=0, definition_exists=1
PROBE 044i shape #2 state: resolved_to_target=0, definition_exists=1, unresolved_in_test=1
```

## Agreement check

| Shape | Oracle says ref should resolve | Probe says ref resolves | Agreement |
|-------|-------------------------------|------------------------|-----------|
| #1    | YES                           | NO                     | **DISAGREE → bug** |
| #2    | YES                           | NO                     | **DISAGREE → bug** |

Probe and oracle disagree consistently on both shapes. Per
`prove-it-prototype.md`, disagreement causes 1 (broken substrate) and 3 (bad
probe) are the candidates. The probe is mechanically simple (4 SQL queries
against a known fixture); the bug *is* the substrate, which is exactly what
rivets-044i claims. So this is cause 2 — model wrong/system broken as
described.

Both shapes hold across the bug claim. Proceed to falsifiable-design.
