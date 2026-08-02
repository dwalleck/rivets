# Review-feedback decisions — round 2

Source: two-axis review (`/code-review xhigh PR 91`), Standards + Spec sub-agents,
2026-08-02. Round 1 decisions are recorded in commit `eac61b4`'s message.

Spec axis returned clean (all four acceptance criteria verified met). All
findings below are from the Standards axis except the last two rows.

| # | Finding (one line) | Category | Verified? | Decision | Note |
|---|---|---|---|---|---|
| 1 | Serde is an unfenced third string table; a serde-attr edit would split JSON vs Display vocabularies | Bug (latent) | Yes — no test pinned serde against Display; `output/tree.rs` emits via Display, `--json` via serde | Accept | Serde↔Display fence tests added for all 18 variants of the four enums (`test_*_serde_matches_display`, `resource_role_serde_matches_display`), both directions |
| 2 | `validate_status`/`validate_dep_type` tests assert the function against its own body (tautology) | Bug (test) | Yes — `validate_status` is `parse().map_err(...)`; the assert compared it to `parse()` | Accept | Rewritten as rstest `#[case]` with explicit expected variants, mirroring `models.rs`; reject cases now pin exact `valid_values` text instead of `contains` |
| 3 | Hardcoded vocabulary lists in error messages (`tools.rs` ×3 incl. resource-role site the review missed, `execute.rs` ×1) | Style / smell | Yes — four comma-joined variant lists outside the enum declarations | Modify | Reviewer suggested embedding lists in FromStr errors; instead added `valid_values()` on `IssueStatus`/`DependencyType`/`ResourceRole` (OnceLock + clap `value_variants()`, keeps `&'static str` so the MCP `InvalidArgument` shape is untouched). Fence tests pin the derived strings to the shipped wording. `IssueKind` gets none — no consumer (negative space) |
| 4 | Public error-variant fields lack `///` docs (`IssueStatusError`, `IssueKindError`, `DependencyTypeError`) | Style | Yes | Accept | `/// The rejected input string.` on all three |
| 5a | Loop-style tests where rstest `#[case]` is the project pattern (`tools.rs`) | Style | Yes | Accept | Folded into #2's rewrite |
| 5b | `fn kind_input` byte-identical in `tools.rs` tests and `tests/integration.rs` | Smell (Duplicated Code) | Yes | Reject | Separate compilation units; sharing would require a `pub` test-only helper on the lib's API surface — worse than 5 duplicated lines |
| 6 | `// rivets-bkjj C3/C4/C5/C7` provenance markers in seven comments | Style | Yes | Accept | Issue-ID + criterion labels stripped; explanatory WHY halves kept. `domain/mod.rs` now points at the committed artifact `.rivets-bkjj/baseline-cli-contract.txt` instead of "the probe" |
| 7 | `models.rs` doc claims `McpIssueKindSchema` "holds no string table" — false | Style (doc drift) | Yes — the type is five variant names + `rename_all` | Accept | Reworded: "its variant list is a fenced duplicate of the domain vocabulary" |
| 8 | `McpIssueKindSchema` is the one literal survivor of "no string table may survive" (Spec axis) | Design | Yes | Reject | Pre-registered orphan-rule exception (design negative-space item 5), fence-tested; unchanged from round 1 |
| 9 | `probe/Cargo.lock` is 1,599 lines of the diff | Polish | Yes | Reject | Lockfile pins the point-in-time probe's dependency set per the diagnostic-dir convention; deleting it makes the probe unreproducible |
| 10 | `docs/design/rest-api.md` documents `parent_child`/`discovered_from` underscore forms | Doc drift (pre-existing) | Yes — rest-api.md ~line 293 | Reject (defer) | Pre-existing, REST API unimplemented, out of PR scope. Tracked at rivets-737v |

Gates after applying: `cargo fmt` clean, `cargo clippy --all-targets
--all-features -- -D warnings` clean, `cargo nextest run` 1072/1072
(1051 → 1072: serde fences, valid-values fences, rstest case expansion).
