# Review-feedback decisions — round 1

Source: `/pr-review-toolkit:review-pr 74` (all 6 agents) — code-reviewer, comment-analyzer,
test-analyzer, silent-failure-hunter, type-design-analyzer, code-simplifier.

Verified against current branch `feat/rivets-044i-qualified-paths` (HEAD `e559eee`).

## Important Issues

| # | Finding (one line) | Reviewer | Category | Verified? | Decision | Note |
|---|---|---|---|---|---|---|
| I1 | Test query filters `reference_name LIKE 'std::%'` AND `symbol_id IS NOT NULL` — `resolve_reference` clears `reference_name` to NULL atomically, so the two predicates can never both hold. Negative assertion is vacuous. | code-reviewer | Bug | YES — `db/references.rs:156-159` UPDATE sets `symbol_id = ?2, reference_name = NULL` in one statement. The test at `pass2_qualified_paths.rs:399-408` would pass even if `std::*` got phantom-resolved. | **Accept** | Tighten the query to use `s.name IN ('HashMap','new')` + a `LEFT JOIN symbols` and assert no `std::`-shaped lookup phantom-resolved. The local-resolves sanity at L416-433 stays as-is. |
| I2 | Longest-prefix iteration (`(1..segments.len()).rev()`) is untested. No fixture has both `a::b::c` AND `a::b` resolvable; direction inversion would silently pass. | test-analyzer (rating 8) | Bug | YES — read all 5 fixtures: each has exactly one resolvable prefix length. Test 2 (`crate_a::Widget::make_widget_044i`) has 3 segments but only `crate_a` is a module (Widget is a struct). Test 5 (`crate::sub_044i::ThingFour`) has 3 segments but only `crate::sub_044i` resolves. Flipping the loop to `1..segments.len()` (shortest-first) does not fail any current test. | **Accept** | New test `longest_prefix_wins_over_shorter` — 3-segment ref where both `a::b` and `a::b::c` resolve to *different* files, assert resolution lands in the deeper file. |
| I3 | "Prefix resolves but tail doesn't exist" branch (line 329-335) is untested. | test-analyzer (rating 7) | Bug | YES — no fixture has `helper::nonexistent_fn()` or equivalent. | **Accept** | New small test `prefix_resolves_but_tail_missing_stays_unresolved` — `mod helper;` exists, helper.rs has a different fn, ref is `helper::missing()`. Assert ref `symbol_id IS NULL`. |
| I4 | `qualified_crate_prefix_resolves` (test 5) asserts only `resolved_to_target >= 1` — even if the `prefix[0]=="crate"` gate were broken, Interpretation B (as-written) would still resolve `crate::sub_044i::ThingFour`. The test cannot distinguish "gate working" from "gate broken but B saves us." | test-analyzer + code-reviewer | Bug | YES — traced the algorithm: with a broken gate, Interpretation A tries `crate::crate::sub_044i` which `resolve_module_path` returns None for; falls through to Interpretation B which succeeds. Existing assertion can't fail. | **Modify** | Reviewer suggested "negative assertion against double-`crate` artifact path." Better fix: assert `resolved_to_target == 1` (exact) and add a count check on resolution path — but the framing is tricky because the path provenance isn't stored. Simpler modification: add a sibling test `crate_prefix_skips_implicit_crate` that *would* break under a malformed gate by setting up a fixture where the doubled-crate prepend would phantom-resolve to a same-named symbol (e.g., a `crate/` directory with a colliding symbol). If unfeasible without contortion, ship with `== 1` exact-count and note the limitation in the docstring. |
| I5 | Phantom-resolution risk on Interpretation A: in a workspace with `helper.rs` at crate root AND `helper.rs` as a sibling module, Interpretation A always wins via `resolve_module_path`'s first match. Risk bounded to exact-name tail collision. Reviewer asked to document as "known acceptable" mirroring `fallback_symbol_search`. | silent-failure-hunter | Design (doc) | YES the risk exists. NO precedent in `fallback_symbol_search` — its rustdoc (resolve.rs:452-458) does NOT document its prefix collision risk either. The "mirroring precedent" framing is incorrect. | **Modify** | Document the bounded risk in `qualified_module_fallback`'s rustdoc as a known acceptable ambiguity (it's real), but drop the "matching fallback_symbol_search precedent" framing because no such precedent exists. The acceptable-ambiguity language is justified by design-v3 C5/C6 which the audit dir documents. |

## Suggestions

| # | Finding (one line) | Reviewer | Category | Verified? | Decision | Note |
|---|---|---|---|---|---|---|
| S1 | No `trace!` on `qualified_module_fallback`'s final `Ok(None)` — every other resolution arm logs on miss. | silent-failure-hunter | Polish | YES — confirmed at `resolve.rs:338`. Other miss paths at `resolve.rs:245-249` and `resolve.rs:441-446` do log. | **Accept** | Add `trace!(ref_name, segments = segments.len(), "qualified_module_fallback: no prefix split resolved")` before the final `Ok(None)`. Forensic debugging value, near-zero release cost. |
| S2 | `relative_path` emits a `warn!` when `resolved` is outside workspace; can fire 2N times per qualified ref. | silent-failure-hunter | Polish | PARTIAL — `lib.rs:220-224` does emit the `warn!`, but `resolve_module_path` for in-workspace prefixes returns paths inside the workspace by construction. Outside-workspace `resolved` is essentially impossible on this code path (external prefixes hit the `None` branch in `resolve_module_path` first). | **Reject (not deferred)** | Not observed in practice. The `warn!` exists for genuinely-misconfigured workspace roots, which is a wholly different operational scenario. Adding a gate would add noise without observable benefit. Rejection rationale stands; no tracker entry needed because the concern isn't real. |
| S3 | Untested defense-in-depth branches: `current_file_path = None` (L285), `segments.len() < 2` (L294), `self::*`/`super::*` paths. | test-analyzer | Bug (coverage) | YES — none of the three branches has direct test coverage. But: the first two are unreachable from the only call site (the `is_qualified` gate at L228 already enforces `contains("::")`, and `ctx.current_file_path` is always `Some` at the call site for normal Pass-2 input). The `self::*`/`super::*` path is the only one with realistic reachability. | **Modify (partial)** | Accept the `self::*`/`super::*` coverage (add to the prefix-resolves-tail-missing test or new mini-test). Reject the other two: testing unreachable defense-in-depth branches inverts the cost/value ratio and amounts to mocking the call site. The doc comments already explain why those guards exist. |
| S4 | Probe-era `eprintln!("PROBE 044i state: ...")` and "Pre-fix this will hold" comments in tests 1 and 2. | comment-analyzer + simplifier | Style | YES — `pass2_qualified_paths.rs:95-98, 106-108, 113, 215-218`. | **Accept** | Cosmetic but real noise on green runs. Remove `eprintln!`s; rewrite the "pre-fix this will hold" framing as steady-state regression-fence wording. |
| S5 | Redundant `// Interpretation A:` / `// Interpretation B:` inline labels restate the rustdoc bullets. | comment-analyzer | Style | YES — `resolve.rs:305, 318` exactly match rustdoc enumeration at L267, L270. | **Accept** | Drop both inline labels. Rustdoc already names them. |
| S6 | Unidiomatic `(*s).to_string()` at L309, L320; may trip `clippy::explicit_auto_deref`. | simplifier | Style | NO — verified `cargo clippy -p tethys --tests --all-features -- -D warnings -W clippy::explicit_auto_deref` produces no warnings. | **Reject** | Clippy doesn't flag it. The simplifier's qualifier ("may trip") was honest; it doesn't, so this is a stylistic preference without machine backing. Skip. |
| S7 | Two `Vec<String>` allocation blocks could share a closure/helper. | simplifier | Design | YES — duplication is real. | **Reject** | Simplifier itself flagged this as "Borderline." Per CLAUDE.md: three similar lines beats premature abstraction. The two blocks differ in their composition (chain-once vs straight-collect) and live within one short loop — extracting would obscure, not clarify. |
| S8 | Hoist `Option<&Path>` check to the call site so the helper signature can be `&Path`. | type-design | Design | YES — the early-exit is a propagated `Option`. | **Reject** | The helper's `Option<&Path>` mirrors `ResolveContext::current_file_path` honestly. Hoisting moves a 3-line early-exit and a load-bearing rustdoc note to the call site without simplifying either site materially. Net wash. |
| S9 | Two `// Load-bearing for correctness:` comments lean toward restating WHAT. | comment-analyzer | Style | PARTIAL — comment at L283-284 IS mostly WHAT. Comment at L290-293 has both WHAT (`would loop zero times and return None`) and WHY (`defended in depth ... preserves that property on hand-rolled paths`). | **Modify** | Tighten both — first comment to name the synthetic-ref case explicitly; second comment to drop the "would loop zero times" sentence and keep only the defense-in-depth rationale. |
| S10 | Module-doc anchor `indexing.rs:627-630` is rot-prone. | comment-analyzer | Style | YES — `pass2_qualified_paths.rs:4` cites the line range. | **Accept** | Rephrase to symbol-relative: `indexing.rs::store_references` free-fn arm. Line-range citation rots on the next reformat. |

## Summary of decisions

- **Accept:** I1, I2, I3, S1, S4, S5, S10 (7 findings)
- **Modify:** I4, I5, S3 (partial), S9 (4 findings)
- **Reject:** S2, S6, S7, S8 (4 findings — none requires a tracker entry; rationale recorded above)

15 findings reviewed. Distribution 7/4/4 (accept/modify/reject) sits in the healthy range — not "accept everything" theater.

## Tracker discipline check

Per `feedback_tracker_entries_for_deferrals` and the skill rule: every Reject(defer) or Modify(deferred work) names a tracker ID. **No findings are deferred to follow-up work** — every accept/modify above is in-scope for this branch's next commit. Rejections are rejections (concern not real or fix worse than the disease), not deferrals. Therefore no new rivets issues need to be filed.

## Application plan

1. Add `trace!` on final `Ok(None)` (S1) — `resolve.rs:338`.
2. Drop `// Interpretation A/B:` inline labels (S5) — `resolve.rs:305, 318`.
3. Tighten two `// Load-bearing for correctness:` comments (S9) — `resolve.rs:283-284, 290-293`.
4. Add bounded-ambiguity paragraph to `qualified_module_fallback` rustdoc (I5) — `resolve.rs:253-276`.
5. Remove `eprintln!` and rephrase pre-fix comments in tests 1, 2 (S4) — `pass2_qualified_paths.rs:95-98, 106-108, 113, 215-218`.
6. Rephrase module-doc anchor (S10) — `pass2_qualified_paths.rs:4`.
7. Rewrite test 4's negative assertion (I1) — `pass2_qualified_paths.rs:399-414`.
8. Strengthen test 5 (I4) — `pass2_qualified_paths.rs:488-492`.
9. New test `longest_prefix_wins_over_shorter` (I2).
10. New test `prefix_resolves_but_tail_missing_stays_unresolved`, with `self::*` / `super::*` coverage absorbed (I3 + S3 partial).

Steps 1-6 are pure doc/comment/log edits and can land in one commit. Steps 7-10 are behavioral test changes and land in a second commit (TDD discipline: tests must fail in a meaningful way before being added — verify by temporarily inverting the relevant invariant in `qualified_module_fallback`).
