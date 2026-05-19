# What I learned (one sentence)

The dn35 fix is necessary but not sufficient: `try_resolve_reference`'s
fallback `get_symbol_by_qualified_name` does a literal string-equality match
against `symbols.qualified_name`, but the database stores qualified names
**module-stripped** (`indexing.rs:627-630`: free fns store `qualified_name =
name`; methods store `parent_name::name` only) — so any ref whose source text
carries a module or workspace-crate prefix systematically misses the fallback
*regardless* of whether the target symbol exists in the same workspace.

## Why this matters for the design

The fix is not "loosen the lookup" — that would re-introduce ambiguity drift
(rivets-0gom). It has to be "translate the ref's path to a target file via
`resolver::resolve_module_path`, then look up the *unqualified* tail in that
specific file." Option (a) in the issue description is the right shape,
because the workspace-crate-prefix arm of `resolve_module_path` already
exists; we just don't currently dispatch to it from `try_resolve_reference`'s
qualified branch unless the first segment matches an explicit import.

## Bonus learning (shape #2)

`make_widget_044i` is an `impl` method, so its stored `qualified_name` is
`Widget::make_widget_044i` (parent_name = `Widget`). The lookup key is
`crate_a::Widget::make_widget_044i`. The mismatch is the crate-prefix
segment, not the method-prefix segment. So even methods (which DO get a
prefix) miss this fallback when called with a crate prefix.
