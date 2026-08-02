# Domain Docs

How the engineering skills should consume this repo's domain documentation when exploring the codebase.

## Before exploring, read these

- `CONTEXT.md` at the repository root.
- Relevant ADRs under `docs/adr/`.
- `docs/module-structure.md` — the module map: which crate/module owns what. Consult it before deciding where new code belongs; if it contradicts the code, flag the drift rather than silently following either side.

If a location does not exist, proceed silently. Domain-modeling workflows create these documents lazily when terms or decisions are resolved.

## Configured layout

This is a single-context repository:

```text
/
├── CONTEXT.md
├── docs/adr/
├── docs/module-structure.md
└── crates/
```

## Use the glossary's vocabulary

When output names a domain concept—in an issue title, refactor proposal, hypothesis, or test—use the term defined in `CONTEXT.md`. Do not drift to synonyms the glossary explicitly avoids.

If a needed concept is absent, reconsider whether the language fits the project or record the gap for domain modeling.

## Flag ADR conflicts

Surface any contradiction with an existing ADR explicitly rather than silently overriding it.
