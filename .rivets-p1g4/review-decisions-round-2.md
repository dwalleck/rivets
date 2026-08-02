# Review decisions — round 2 (post-PR two-axis review, max effort)

Round 1 was the pre-PR review triaged in commit 982cedb. This round ran the
two-axis review (`/code-review max --fix PR 90`) against `origin/main...HEAD`
with parallel Standards and Spec sub-agents; every finding below was verified
against the working tree before a decision was made.

## Accepted

1. **[BUG, both axes, high] `WorkspacePath` accepted Windows-form paths.**
   `new` split on `/` only and rejected only a leading `/`, so
   `..\..\secrets.txt`, `C:\Windows`, `C:/Windows`, `C:relative.txt`,
   `\\server\share`, and `\etc\passwd` were all stored as "workspace-relative"
   — contradicting the type's own escape guarantee, and CI ships a
   `rivets-windows` binary, so the design.md Linux-only waiver did not hold.
   Fix: reject any `\` with the new `ResourceError::WorkspacePathBackslash`
   (char position, matching `find_control_char` semantics) and treat an
   ASCII `X:` drive prefix as `AbsoluteWorkspacePath`. Rejection was chosen
   over `\`→`/` normalization because `\` is a legal POSIX filename character:
   normalizing would silently alias the distinct files `a\b` and `a/b`.
   Interior `C:` components stay legal (oracle-verified, Windows-illegal
   colon aside — out of scope, see rejected #5).
   The 19 oracle-accepted `C:`-prefixed corpus cases migrated from `CORPUS`
   to `POLICY_REJECT` (legal POSIX filenames now rejected by policy);
   `("C:/..", None)` stays in `CORPUS` since both oracle and domain reject
   it. Corpus is now 413 oracle-agreement cases + 27 policy rejects
   (439 recorded inputs total). New fences: 7 rstest reject cases,
   char-position variant test, drive-prefix variant test, CLI backslash
   probe. Both exhaustive migration classification lists extended (the
   yx1h guardrail working as designed).

2. **[Standards, hard] Bare `.unwrap()` in new tests.** All 27 diff-added
   sites across `in_memory_storage.rs`, `cli_tests.rs`,
   `in_memory_resilient_loading.rs`, and MCP `integration.rs` converted to
   `.expect("…")` per CLAUDE.md "Descriptive Assertions in Tests".
   Pre-existing unwraps untouched (out of scope).

3. **[Standards, judgement] Duplicated `(url, path)` four-arm cascade (4×).**
   Collapsed to one canonical helper per crate: CLI `parse_target_flags`
   (Add layers "exactly one" on the `None` case), MCP
   `parse_optional_resource_target` with `parse_resource_target` delegating.
   The round-1 four-arm exhaustive convention is preserved — it now lives in
   exactly one place per crate. The both-provided MCP message for `add`
   changed from "exactly one of url or path" to "at most one of url or path"
   (no test asserted the old text).

4. **[Standards, judgement] `resource update` accepted zero fields at parse
   time.** Added a required-any clap `ArgGroup` over
   `url/path/role/label/no_label`, so the CLI fails at parse without loading
   the workspace. Domain `EmptyUpdate` remains the seam guard for MCP and
   stays fenced by domain tests. New CLI fence:
   `resource_update_requires_at_least_one_field`.

5. **[Spec, medium] README documented only the pre-PR surface.** Rewrote the
   Associated Resources section: `--path`, `resource update`,
   `resource remove`, all four MCP tools, and the path rules.

6. **[Spec, low] Test gaps.** Added: last-position update stability
   (`update_resource_last_position_is_stable`), CLI `--label ""` typed-error
   probe, explicit nonexistent-target fence
   (`workspace_path_accepts_nonexistent_targets`), and the backslash corpus
   coverage from #1.

7. **[Standards, judgement] Corpus test `checked` counter restated
   `CORPUS.len()`.** Replaced with a direct length assertion. The
   one-element `POLICY_REJECT` "speculative generality" concern is moot —
   it now holds 27 entries.

## Rejected

1. **`Option<Option<ResourceLabel>>` → `LabelUpdate` enum.** The double-Option
   is the documented repo precedent (`IssueUpdate::assignee`, cited in the
   `ResourceUpdate` doc comment). Changing only the label field would break
   convention symmetry; changing both is out of scope. At the seams the
   invalid state is unrepresentable at parse time (CLI `conflicts_with`) or
   validated with a typed error (MCP JSON, which cannot express sum types
   ergonomically).

2. **Twin exhaustive `ResourceError` classification lists.** Re-litigated and
   re-rejected: round-1 decision stands — it is a deliberate fail-loudly
   guardrail from rivets-yx1h, documented in-code, and this round proved its
   value (the new variant forced an explicit benign-vs-fail decision at both
   sites).

3. **Drop "unreachable" CLI match arms excluded by clap.** Round-1 decision
   stands: the four-arm exhaustive convention is a fail-loudly guard against
   clap-config drift. Subsumed by accepted #3 — the arms now exist once per
   crate.

4. **Split `resource_path_add_update_remove_and_error_cases`.** The test is a
   sequential CLI lifecycle whose later stages consume earlier state;
   splitting would multiply process spawns to rebuild state, and each
   assertion already carries a message naming its behaviour.

5. **Trim/reject leading-trailing whitespace in paths (`" src "`).**
   Store-verbatim matches the sibling convention (`ResourceLabel`,
   `ResourceId` validate but never rewrite), ` src ` is a legal, distinct
   POSIX filename, and the realpath oracle agrees. Trimming would silently
   alias two distinct legal targets.

## Deferred (filed)

- **MCP `Tools::resource_add` takes 6 positional args (adjacent
  `Option<String>` url/path risk transposition).** Passing
  `ResourceAddParams` through, as `resource_update` already does, touches
  ~19 green call sites — mechanical churn disproportionate to a remediation
  commit. Filed as a follow-up issue (see `.rivets/issues.jsonl`).

## PR-description drift to fix when updating the PR

- "432-case corpus (100% agreement)" → 413 oracle-agreement cases +
  27 policy rejects (439 recorded inputs).
- "1036/1036 tests" → 1048/1048.
- MCP tool `resource_add` both-provided error text now says "at most one of
  url or path".
