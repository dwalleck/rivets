# Tethys Repository Split Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use `superpowers:executing-plans` to implement this plan task-by-task. Tasks 9, 10, 12, and 20 are irreversible or affect shared state — STOP and confirm with the user before executing each. Tasks 18 and 19 (issue migration) create externally-visible changes in the new tethys repo and the rivets JSONL — confirm decisions 2, 6, 7 are recorded before running them. Tasks marked **(decision gate)** must not be run until the user has answered the corresponding question in the "Pre-flight decisions" section.

**Goal:** Extract `crates/tethys/` from the rivets workspace into a standalone GitHub repository with preserved git history, leaving rivets as a three-crate workspace (`rivets-jsonl`, `rivets`, `rivets-mcp`).

**Architecture:** Five phases. **A** makes tethys standalone-buildable in-place via a normal PR (reversible). **B** is a decisions checkpoint. **C** uses `git filter-repo` on a mirror clone to produce a single-crate repo with hoisted paths and preserved history, then pushes to a fresh GitHub remote. **D** stands up CI and conventions in the new repo. **E** removes tethys from the rivets workspace as a final cleanup PR.

**Tech Stack:** Rust 2024 edition, Cargo workspace inheritance, `git filter-repo`, GitHub Actions, `cargo machete`.

**Why this shape:** Phase A is a normal merge to `main` that can sit for days or be reverted cheaply. The irreversible work (history rewrite + new remote) is concentrated in phase C so cancellation is binary — either you've pushed to the new remote or you haven't. Phase E only runs after the new repo is healthy.

---

## Pre-flight decisions

These must be answered before starting phase C. Phase A is safe to run regardless.

1. **History strategy.** Full preserved history via `git filter-repo` (default in this plan) vs. clean single-commit cut. **Recommended:** full history — 123 of 385 commits touch tethys, and blame is valuable on an active codebase.
2. **Issue tracking in new repo.** Options: (a) GitHub Issues, (b) own `.rivets/issues.jsonl` with rivets as an external dev tool, (c) something else. Affects Tasks 15, 16–20.
3. **New-repo workspace shape.** (a) Single crate at root, or (b) Cargo workspace with room for future crates (e.g. `tethys-core`, `tethys-cli`, language plugins). **Recommended:** start single-crate; promote to workspace later if needed. Affects Task 10's `--path-rename` and Task 13's CI.
4. **crates.io publishing timing.** (a) Split first, then publish, or (b) publish a `0.1.0` from this repo to claim the name, then split. **Recommended:** (a) — simpler if the `tethys` crate name is available.
5. **Tag handling.** (a) Let `git filter-repo` prune workspace-wide tags to surviving commits (yields orphan-looking tags), or (b) drop all tags during filter and cut a fresh `v0.1.0` post-split. **Recommended:** (b) — cleaner history.
6. **Issue ID strategy.** Existing rivets issues all use the `rivets-*` prefix (configured in `.rivets/config.yaml`). After migration:
   - (a) **Keep `rivets-*` IDs in tethys repo.** Preserves direct links from git commit messages that reference these IDs. Awkward — a "tethys" repo whose issues are prefixed `rivets-`.
   - (b) **Rename to `tethys-*` IDs.** Clean per-repo identity, but every `rivets-XXX` in past commit messages becomes dead-link (or worse, ambiguous if rivets later reuses an ID).
   - (c) **Rename + `external_ref`.** New tethys IDs, with the original `rivets-XXX` stored in the `external_ref` field for forensic traceability. **Recommended.**
   Affects Tasks 18 and 19. Only relevant if decision 2 = (b) JSONL; GitHub Issues uses its own numbering.
7. **Cross-repo dependency resolution.** Tethys-labeled issues currently have `blocks` and `parent-child` dependencies on non-tethys IDs (e.g. `rivets-4tev`, `rivets-zk2q`). Some of these targets will move with tethys; some won't. For deps that cross the boundary post-split:
   - (a) **Drop the dep.** Simplest; loses context.
   - (b) **Convert to `external_ref` URL** pointing back to the remaining-side's issue. Preserves traceability for `blocks` and `parent-child`.
   - (c) **Document in body text.** Append a note like "Originally a `blocks` dep on rivets-XXXX in monorepo era."
   **Recommended:** (b) for `blocks`/`parent-child` (load-bearing), (c) for `related` (looser). Affects Task 17.
8. **`docs/` migration.** The `docs/` directory contains 16 tethys-only files (`design/tethys-*.md`, `spikes/2026-01-22-tethys-sqlite-petgraph.md`, and plans for blast-radius, cross-file-resolution, cargo-toml-parsing, module-path-*, phase3-graph-operations, tethys-quality-alignment, lsp-session-result, and tethys-architecture-analysis), 4 rivets-only files, and 7 cross-cutting files. Options for tethys-only docs:
   - (a) **Don't move.** Leave them in rivets as historical artifacts; the new tethys repo starts with no design history.
   - (b) **Move without history.** `cp` to the new repo in phase E, `git rm` from rivets. New repo gets the files but `git log <file>` is empty.
   - (c) **Move with history via `git filter-repo`.** Include the tethys-only doc paths in Task 10's filter alongside `crates/tethys`. New repo gets full per-file `git log`/`git blame`.
   **Recommended:** (c) — the design rationale (e.g. why SQLite + petgraph, why this LSP shape) is load-bearing context for future tethys contributors. Affects Tasks 8, 10, and 23. Cross-cutting docs stay in rivets with tethys sections trimmed regardless of choice.

---

## Phase A — In-repo prep (reversible PR)

### Task 1: Inline workspace inheritance in `crates/tethys/Cargo.toml`

**Files:**
- Modify: `crates/tethys/Cargo.toml`

**Step 1: Replace `*.workspace = true` fields with explicit values**

In the `[package]` section, replace the four inherited fields:

```toml
version.workspace = true
edition.workspace = true
license.workspace = true
authors.workspace = true
```

With:

```toml
version = "0.1.0"
edition = "2024"
rust-version = "1.94.0"
license = "MIT OR Apache-2.0"
authors = ["Rivets Contributors"]
```

(Values copied from root `Cargo.toml:11-15`. Adding `rust-version` because workspaces don't auto-propagate MSRV to package manifests.)

**Step 2: Inline workspace dependencies**

Replace each `*.workspace = true` line in `[dependencies]` and `[dev-dependencies]` with the explicit version from root `Cargo.toml:17-69`:

| Dep | Replace with |
|---|---|
| `petgraph.workspace = true` | `petgraph = "0.6"` |
| `serde.workspace = true` | `serde = { version = "1.0", features = ["derive"] }` |
| `serde_json.workspace = true` | `serde_json = "1.0"` |
| `thiserror.workspace = true` | `thiserror = "2.0"` |
| `tracing.workspace = true` | `tracing = "0.1"` |
| `clap.workspace = true` | `clap = { version = "4.5", features = ["derive", "cargo"] }` |
| `tracing-subscriber.workspace = true` | `tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt", "ansi"] }` |
| `colored.workspace = true` | `colored = "3"` |
| `tempfile.workspace = true` | `tempfile = "3.18"` |
| `rstest.workspace = true` | `rstest = "0.26"` |
| `proptest.workspace = true` | `proptest = "1.6"` |

**Step 3: Verify build still works**

Run: `cargo build -p tethys`
Expected: builds cleanly with no warnings about workspace inheritance.

Run: `cargo nextest run -p tethys`
Expected: all tethys tests pass.

**Step 4: Commit**

```bash
git add crates/tethys/Cargo.toml
git commit -m "chore(tethys): inline workspace inheritance to prep for repo split"
```

---

### Task 2: Copy workspace lints into tethys

**Files:**
- Modify: `crates/tethys/Cargo.toml`

**Step 1: Replace `[lints]` block**

Replace:

```toml
[lints]
workspace = true
```

With:

```toml
[lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"

[lints.clippy]
all = "warn"
pedantic = "warn"
```

(Copied verbatim from root `Cargo.toml:70-76`.)

**Step 2: Verify lint behavior unchanged**

Run: `cargo clippy -p tethys --all-targets --all-features -- -D warnings`
Expected: identical output to before the change (zero warnings if currently clean).

**Step 3: Commit**

```bash
git add crates/tethys/Cargo.toml
git commit -m "chore(tethys): inline lint config from workspace"
```

---

### Task 3: Copy profile blocks into tethys

**Files:**
- Modify: `crates/tethys/Cargo.toml`

**Step 1: Append profile blocks to the end of the file**

```toml
[profile.test]
opt-level = 1

[profile.release]
lto = "fat"
codegen-units = 1
strip = "symbols"

[profile.release-with-debug]
inherits = "release"
debug = 2
strip = "none"
```

(Copied from root `Cargo.toml:78-89`.)

**Note:** In a Cargo workspace, only the root manifest's `[profile.*]` blocks are active — package-level profile blocks are ignored with a warning while tethys still lives in the workspace. That's fine; this is forward-prep for the standalone repo. Cargo will warn during build; that's expected.

**Step 2: Verify build still works (expect profile warnings)**

Run: `cargo build -p tethys`
Expected: build succeeds. Cargo emits a warning about profiles in non-root manifest — this is intentional and goes away after extraction.

**Step 3: Commit**

```bash
git add crates/tethys/Cargo.toml
git commit -m "chore(tethys): copy release profiles in prep for standalone repo

Cargo will warn that package-level profiles are ignored while tethys is
still a workspace member. The warning resolves after the repo split."
```

---

### Task 4: Add per-crate license files

**Files:**
- Create: `crates/tethys/LICENSE-MIT`
- Create: `crates/tethys/LICENSE-APACHE`

**Step 1: Copy workspace-root license files into the tethys crate**

```bash
cp LICENSE-MIT crates/tethys/LICENSE-MIT
cp LICENSE-APACHE crates/tethys/LICENSE-APACHE
```

**Step 2: Verify they match**

Run: `diff LICENSE-MIT crates/tethys/LICENSE-MIT`
Expected: no output (identical).

Run: `diff LICENSE-APACHE crates/tethys/LICENSE-APACHE`
Expected: no output.

**Step 3: Commit**

```bash
git add crates/tethys/LICENSE-MIT crates/tethys/LICENSE-APACHE
git commit -m "chore(tethys): add per-crate license files for standalone publishing"
```

---

### Task 5: Verify tethys builds in isolation

**Goal:** Prove tethys can build with no workspace context. We do this *without* breaking the working tree by using a scratch clone.

**Step 1: Create a scratch clone**

```bash
git clone --no-local . ../tethys-isolation-check
cd ../tethys-isolation-check
```

**Step 2: Strip the workspace down to just tethys**

Edit `Cargo.toml` in the scratch clone:

```toml
[workspace]
resolver = "3"
members = ["crates/tethys"]
```

(Removes `rivets-jsonl`, `rivets`, `rivets-mcp` from members. Also remove the `[workspace.dependencies]` section entirely — tethys no longer needs it since Task 1 inlined everything.)

**Step 3: Verify build and tests**

Run: `cargo build -p tethys`
Expected: builds cleanly.

Run: `cargo nextest run -p tethys`
Expected: all tests pass.

Run: `cargo clippy -p tethys --all-targets -- -D warnings`
Expected: zero warnings.

**Step 4: Clean up scratch clone**

```bash
cd ../rivets
rm -rf ../tethys-isolation-check
```

**Step 5: No commit needed** — this task only verifies. If anything failed, return to Task 1-3 and fix.

---

### Task 6: Open phase-A PR

**Step 1: Push branch and open PR**

```bash
git push -u origin <branch-name>
gh pr create --title "chore(tethys): prep crate for repo extraction" --body "$(cat <<'EOF'
## Summary
- Inlines workspace inheritance in `crates/tethys/Cargo.toml` (deps, package fields, lints, profiles)
- Adds per-crate `LICENSE-MIT` and `LICENSE-APACHE`
- Tethys now builds standalone (verified via isolated scratch clone)

This is phase A of the tethys repo split. See `docs/plans/2026-05-11-tethys-repo-split.md`.

## Test plan
- [x] `cargo build -p tethys` — clean
- [x] `cargo nextest run -p tethys` — all pass
- [x] `cargo clippy -p tethys -- -D warnings` — zero warnings
- [x] Isolated build (workspace stripped to tethys only) — clean
EOF
)"
```

**Step 2: Wait for review and merge.** This is a normal PR with no special handling.

---

## Phase B — Decisions checkpoint

### Task 7: Resolve pre-flight decisions

**STOP.** Before proceeding to phase C, confirm with the user:

- Decision 1 (history strategy): **default = full history**
- Decision 2 (issue tracking): _user answer required_ — affects Tasks 15, 16–20
- Decision 3 (workspace shape): _user answer required_ — affects Task 10 path-rename and Task 13 CI
- Decision 4 (crates.io timing): _user answer required_
- Decision 5 (tag handling): _user answer required_
- Decision 6 (issue ID strategy): _user answer required if decision 2 = (b) JSONL_ — affects Tasks 18, 19
- Decision 7 (cross-repo dep resolution): _user answer required_ — affects Task 17
- Decision 8 (docs migration strategy): _user answer required_ — affects Tasks 8, 10, 23

Record the answers in this section, then proceed.

---

## Phase C — Extraction (irreversible)

> **Docs-migration prep (Task 8):** This task is reversible — it builds an artifact (`tethys-paths.txt`) that drives Task 10's filter. Run it before Task 10 so the filter can include docs alongside `crates/tethys`. If decision 8 = (a) "don't move docs," this task is mostly a no-op (still useful for documenting what *would have* moved).

### Task 8: Audit which docs belong in which repo (decision gate)

**Goal:** Mirror the structure of Task 16 (issue audit) for the `docs/` directory: produce a per-file manifest classifying each doc as tethys-only, rivets-only, or cross-cutting, plus the concrete `tethys-paths.txt` artifact that Task 10's filter consumes.

**Files:**
- Create (working artifact, committed alongside Task 23's cleanup PR): `docs/plans/tethys-split-docs-manifest.md` *or* a scratch file — your call
- Create (scratch input for Task 10): `../tethys-paths.txt` in the parent directory of where the scratch clone will live

**Step 1: Extract the candidate tethys-only doc list**

The known tethys-only docs as of this plan-write are 16 files. Verify nothing new has been added since:

```bash
# Files with "tethys" in path or content
grep -l -i tethys docs/**/*.md
# Plan files matching tethys-related feature names
ls docs/plans/ | grep -iE '(tethys|lsp|cross-file|module-path|cargo-toml|blast-radius|phase3-graph)'
# Spikes
ls docs/spikes/
```

Expected result (16 files):

```
docs/design/tethys-code-intelligence.md
docs/design/tethys-architecture-analysis.md
docs/spikes/2026-01-22-tethys-sqlite-petgraph.md
docs/plans/2026-01-22-blast-radius-analysis-design.md
docs/plans/2026-01-25-phase3-graph-operations-design.md
docs/plans/2026-01-25-phase3-graph-operations-impl.md
docs/plans/2026-01-28-cross-file-resolution.md
docs/plans/2026-02-01-cargo-toml-parsing-design.md
docs/plans/2026-02-01-cargo-toml-parsing.md
docs/plans/2026-02-01-module-path-computation.md
docs/plans/2026-02-01-module-path-implementation.md
docs/plans/2026-03-18-tethys-quality-alignment-design.md
docs/plans/2026-03-18-tethys-quality-alignment.md
docs/plans/2026-03-19-lsp-session-result-design.md
docs/plans/2026-03-19-lsp-session-result.md
docs/plans/2026-05-10-tethys-architecture-analysis.md
```

**Exclusion:** `docs/plans/2026-05-11-tethys-repo-split.md` (this plan) is *not* in the list — it stays in rivets as the meta-doc describing the split, and is deleted post-merge per the post-merge checklist.

**Step 2: Classify the cross-cutting docs**

The following 7 docs mention tethys but cover broader workspace concerns. Open each, confirm whether it should stay (with tethys sections trimmed) or be deleted as superseded:

| File | Action | Note |
|---|---|---|
| `docs/README.md` | Stay | Currently rivets-only despite being the docs index |
| `docs/architecture.md` | Trim | Remove tethys from per-crate breakdowns |
| `docs/data-flow.md` | Trim | Check for tethys references |
| `docs/design/code-intelligence.md` | **Delete (recommended)** | Superseded by `tethys-code-intelligence.md` which is moving |
| `docs/module-structure.md` | Trim | Remove tethys from workspace structure |
| `docs/task-dependency-graph.md` | Trim | Remove tethys task references |
| `docs/terminology.md` | Trim | Remove tethys-specific terms |

The 4 rivets-only docs (`docs/design/automerge-storage.md`, `docs/design/rivets-roadmap.md`, `docs/storage-architecture.md`, `docs/rivets-jsonl-research.md`) require no action.

**Step 3: Build the migration manifest**

Write a working markdown file with one row per doc:

```markdown
| Path | Destination | Action | Note |
|---|---|---|---|
| docs/design/tethys-code-intelligence.md | tethys | filter | tethys-only |
| docs/design/code-intelligence.md | rivets | delete | superseded |
| docs/architecture.md | rivets | trim | cross-cutting |
| docs/design/automerge-storage.md | rivets | keep | rivets-only |
| ... | | | |
```

This artifact drives both Task 10 (filter) and Task 23 (rivets-side cleanup).

**Step 4: Build `tethys-paths.txt` per decision 8**

If **decision 8 = (c)** (recommended — move with history), create `tethys-paths.txt` in the parent of the future scratch clone with the 16 doc paths from Step 1, plus `crates/tethys`:

```
crates/tethys
docs/design/tethys-code-intelligence.md
docs/design/tethys-architecture-analysis.md
docs/spikes/2026-01-22-tethys-sqlite-petgraph.md
docs/plans/2026-01-22-blast-radius-analysis-design.md
docs/plans/2026-01-25-phase3-graph-operations-design.md
docs/plans/2026-01-25-phase3-graph-operations-impl.md
docs/plans/2026-01-28-cross-file-resolution.md
docs/plans/2026-02-01-cargo-toml-parsing-design.md
docs/plans/2026-02-01-cargo-toml-parsing.md
docs/plans/2026-02-01-module-path-computation.md
docs/plans/2026-02-01-module-path-implementation.md
docs/plans/2026-03-18-tethys-quality-alignment-design.md
docs/plans/2026-03-18-tethys-quality-alignment.md
docs/plans/2026-03-19-lsp-session-result-design.md
docs/plans/2026-03-19-lsp-session-result.md
docs/plans/2026-05-10-tethys-architecture-analysis.md
```

If **decision 8 = (a)** (don't move), `tethys-paths.txt` contains only `crates/tethys`.

If **decision 8 = (b)** (move without history), `tethys-paths.txt` contains only `crates/tethys` (the docs will be `cp`'d into the new repo after the filter in Task 10 step 4).

**Step 5: No commit needed in source rivets repo** — the manifest is a working doc, and `tethys-paths.txt` lives outside the repo. The manifest itself can be committed as part of Task 23's cleanup PR if you want it preserved as a record of the split (recommended).

---

### Task 9: Mirror-clone rivets to a scratch location ⚠️

**STOP.** This task operates outside the working repo. Confirm with the user that phase A has merged to `main` before proceeding.

**Step 1: Install `git filter-repo` if not present**

```bash
git filter-repo --version
```

If missing: `pip install git-filter-repo` (or your platform's equivalent).

**Step 2: Mirror clone**

```bash
cd ~/scratch  # or wherever you keep scratch repos
git clone --no-local https://github.com/dwalleck/rivets.git tethys
cd tethys
git remote remove origin  # prevent accidental push back to rivets
```

**Why `--no-local`:** Forces a real clone over the wire-format protocol, ensuring an independent object database. Local clones hard-link objects, which makes `git filter-repo` refuse to run for safety.

**Step 3: Verify state**

Run: `git log --oneline | head -5`
Expected: same recent commits as rivets `main`.

Run: `git remote -v`
Expected: no remotes (we removed origin).

---

### Task 10: Run `git filter-repo` to extract tethys ⚠️

**STOP.** This rewrites all of git history in the scratch clone. The scratch clone is disposable, but confirm with the user before running. Verify that Task 8's `tethys-paths.txt` exists in the parent directory before proceeding:

```bash
ls -l ../tethys-paths.txt && wc -l ../tethys-paths.txt
```

Expected: 17 lines for decision 8 = (c), 1 line for decision 8 = (a) or (b).

**Step 1: Run the filter**

For decision-3 = single-crate (recommended) + decision 8 = (c):

```bash
git filter-repo \
  --paths-from-file ../tethys-paths.txt \
  --path-rename crates/tethys/:
```

For decision-3 = single-crate + decision 8 = (a) or (b) (no docs in filter):

```bash
git filter-repo \
  --path crates/tethys \
  --path-rename crates/tethys/:
```

For decision-3 = workspace (less common):

```bash
# Replace --path-rename with the workspace-preserving form
git filter-repo \
  --paths-from-file ../tethys-paths.txt \
  --path-rename crates/tethys/:crates/tethys/
```

**Path-rename mechanics:**
- `crates/tethys/:` — trailing colon means "rename prefix to empty" — hoists `crates/tethys/*` files to repo root. Docs paths under `docs/` are not affected (they aren't matched by the rename prefix), so they keep their `docs/design/...`, `docs/plans/...`, `docs/spikes/...` paths in the new repo.
- `crates/tethys/:crates/tethys/` — keeps the nested path inside a workspace structure.
- `--paths-from-file` reads one path per line; each entry behaves like `--path <line>` (substring/prefix match against the full path).

**Step 2: If decision-5 = fresh tags, drop all rewritten tags**

```bash
git tag -l | xargs -r git tag -d
```

(Tags pointing to filtered-out commits become orphan refs; easier to retag post-split.)

**Step 3: Audit result**

Run: `git log --oneline | wc -l`
Expected: ~123+ commits (tethys-touching subset; slightly higher if decision 8 = (c) because docs-only commits now survive too).

Run: `git ls-files | head -20`
Expected: `Cargo.toml`, `src/lib.rs`, `tests/`, `benches/`, `LICENSE-MIT`, `LICENSE-APACHE`, `README.md` at the repo root; plus a populated `docs/design/`, `docs/plans/`, `docs/spikes/` tree if decision 8 = (c).

Run: `git ls-files | grep -E '^(crates|src|tests)/' | head`
Expected: nothing under `crates/` for single-crate option (a). Single-crate root has `src/` and `tests/` directly.

Run: `git log --oneline -- docs/`
Expected (decision 8 = c): commits about the migrated tethys design docs over time. Should be empty if decision 8 = (a) or (b).

Run: `git log --oneline -- .`
Expected: each commit's message mentions tethys-related changes (code or doc).

**Step 4: If decision 8 = (b) — add docs without history after filter**

Skip if you chose (a) or (c). For (b), now `cp` the tethys-only docs from the original rivets working tree into the scratch clone's `docs/` and commit them in a single "import docs" commit. This produces a clean per-file `docs/` tree but with no per-file git log.

---

### Task 11: Verify the filtered repo builds

**Step 1: Build and test**

Run: `cargo build`
Expected: clean build (because phase A made tethys standalone-buildable).

Run: `cargo nextest run`
Expected: all tests pass.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: zero warnings.

**Step 2: Confirm `Cargo.toml` self-contained**

Run: `grep -E '\.workspace = true' Cargo.toml`
Expected: no matches (phase A inlined all workspace inheritance).

Run: `grep -A1 '^\[workspace\]' Cargo.toml`
Expected: no workspace block, or workspace block with only this crate as member (if decision-3 = workspace).

**Step 3: No commit needed** — this is verification only.

---

### Task 12: Create the new GitHub repo and push ⚠️

**STOP.** This creates externally-visible state. Confirm with the user before running. **Do not push if Task 11 had any failures.**

**Step 1: Create the new GitHub repository**

```bash
gh repo create dwalleck/tethys \
  --description "Code intelligence cache and query interface" \
  --public \
  --license "MIT" \
  --homepage "https://github.com/dwalleck/tethys"
```

(`--license MIT` creates an initial commit with a LICENSE file — we'll force-push past it. If you want a fully clean history, omit `--license` and create the repo empty, then push.)

**Step 2: Configure and push**

```bash
git remote add origin https://github.com/dwalleck/tethys.git
git branch -M main
git push -u origin main --force-with-lease
```

`--force-with-lease`: Safe-force-push variant — fails if the remote was modified by anyone else since we last saw it. Use plain `--force` only if you created the repo fully empty.

**Step 3: Verify push**

Run: `gh repo view dwalleck/tethys --web`
Expected: opens the new repo; shows ~123 commits, files at root.

**Step 4: Tag a fresh release (if decision-5 = b)**

```bash
git tag v0.1.0 -m "Initial release after extraction from rivets"
git push origin v0.1.0
```

---

## Phase D — New-repo bring-up

### Task 13: Adapt CI workflow for single-crate repo (decision gate)

**Files (in new tethys repo):**
- Modify: `.github/workflows/ci.yml` (still has rivets-wide config)

**Step 1: Strip workspace-wide flags**

In the new repo, edit `.github/workflows/ci.yml`:

- Replace `cargo nextest run --all-features --workspace` with `cargo nextest run --all-features` (line ~128).
- Replace `cargo test --doc --all-features --workspace` with `cargo test --doc --all-features` (line ~131).
- Replace `cargo build --release --all-features --workspace` with `cargo build --release --all-features` (line ~155).

**Step 2: Update binary upload steps**

Lines 157-176 currently upload `target/release/rivets` (and `.exe`). Replace with `target/release/tethys`:

```yaml
- name: Upload binary (Ubuntu)
  if: matrix.os == 'ubuntu-latest'
  uses: actions/upload-artifact@v4
  with:
    name: tethys-linux
    path: target/release/tethys

# ... same pattern for macOS and Windows
```

**Step 3: Verify scopes in commitlint**

Lines 38 of `ci.yml` has a regex allowing scopes like `cli`, `storage`, `mcp`, `jsonl`. Update the error-message hint at line 57 to drop rivets-specific scopes. The regex itself is generic and needs no change.

**Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: adapt workflow for single-crate repo"
git push
```

**Step 5: Verify CI passes**

Run: `gh run watch`
Expected: all jobs green on first push.

---

### Task 14: Update `repository` URL in tethys Cargo.toml

**Files (in new tethys repo):**
- Modify: `Cargo.toml`

**Step 1: Update repository field**

Change:

```toml
repository = "https://github.com/dwalleck/rivets"
```

To:

```toml
repository = "https://github.com/dwalleck/tethys"
```

**Step 2: Commit and push**

```bash
git add Cargo.toml
git commit -m "chore: update repository URL post-extraction"
git push
```

---

### Task 15: Decide CONTRIBUTING / issue templates / branch protection (decision gate)

Based on decision 2 (issue tracking):

- If GitHub Issues: add `.github/ISSUE_TEMPLATE/bug.md` and `feature.md`. Enable Issues in repo settings.
- If `.rivets/issues.jsonl`: disable Issues in repo settings (`gh repo edit --enable-issues=false`). Create `.rivets/` and seed with any tethys-specific issues currently in the rivets workspace.

Set branch protection:

```bash
gh api -X PUT repos/dwalleck/tethys/branches/main/protection \
  --input <(cat <<'EOF'
{
  "required_status_checks": {"strict": true, "contexts": ["CI Success"]},
  "enforce_admins": false,
  "required_pull_request_reviews": null,
  "restrictions": null
}
EOF
)
```

(Tweak as needed for your workflow.)

---

## Phase E — Cleanup in rivets

> **Issue-migration sub-phase (Tasks 15–19):** Tethys issues currently live in `.rivets/issues.jsonl`. As of plan-write time: 38 issues labeled `tethys` (25 closed, 12 open, 1 in_progress) plus ~13 issues that *mention* tethys without the label. The parent epic is `rivets-j9bu` ("Tethys: Codebase Intelligence Engine"). The `rivets` CLI has no export/import command, so migration is mechanical (jq / scripted JSONL rewrite) or done via `gh issue create` if going to GitHub Issues. `.rivets/index/tethys.db` is *not* checked in — it's a local-only secondary index regenerated from the JSONL on first command and can be ignored throughout the migration.

### Task 16: Audit which issues belong in which repo

**Files:**
- Create (working doc, not committed): `tethys-split-issue-manifest.md` in a scratch location

**Step 1: Extract tethys-labeled issues**

```bash
grep '"labels":\[[^]]*"tethys"' .rivets/issues.jsonl > /tmp/tethys-labeled.jsonl
wc -l /tmp/tethys-labeled.jsonl
```
Expected: ~38 lines.

**Step 2: Find issues that mention tethys without the label**

```bash
grep -i tethys .rivets/issues.jsonl \
  | grep -v '"labels":\[[^]]*"tethys"' \
  > /tmp/tethys-mentions.jsonl
wc -l /tmp/tethys-mentions.jsonl
```
Expected: ~13 lines.

**Step 3: Triage the unlabeled mentions**

For each issue in `/tmp/tethys-mentions.jsonl`, decide:

- **Belongs to tethys** — tethys-specific feature/bug that happened to miss the label
- **Belongs to rivets** — a rivets issue that references tethys as context only
- **Cross-cutting** — genuinely about both (e.g., MCP server integration that spans both). Default: keeps in rivets, add `external_ref` to tethys.

Use `cargo run -- show rivets-XXX` for full detail on each.

**Step 4: Identify the parent epic and descendants**

`rivets-j9bu` is the top-level "Tethys: Codebase Intelligence Engine" epic. Find its children:

```bash
grep '"depends_on_id":"rivets-j9bu","dep_type":"parent-child"' .rivets/issues.jsonl \
  | grep -o '"id":"rivets-[^"]*"'
```

All of these go to tethys regardless of label state.

**Step 5: Write the migration manifest**

Create a working markdown file with one row per issue:

```markdown
| ID | Title | Status | Destination | Notes |
|---|---|---|---|---|
| rivets-rndz | ... | open | tethys | labeled |
| rivets-j9bu | Tethys: Codebase ... | open | tethys | parent epic |
| rivets-xxxx | ... | closed | rivets | mentioned only |
```

**Step 6: No commit needed** — manifest is a working doc.

---

### Task 17: Resolve cross-repo dependency strategy (decision gate)

**Goal:** For every dependency edge in the manifest, determine whether it stays intra-repo or crosses the boundary, and apply decision 7's resolution.

**Step 1: Build the cross-boundary edge list**

For each issue in the manifest's `Destination=tethys` set, examine its `dependencies` array (and any incoming deps where it's the target). An edge is "cross-boundary" if exactly one of its endpoints is destined for tethys.

A scratch jq query (Windows users: install via `winget install jqlang.jq` or use WSL):

```bash
jq -c 'select(.labels | contains(["tethys"])) | {id, deps: .dependencies}' \
  .rivets/issues.jsonl > /tmp/tethys-deps.json
```

Then for each dep, check whether `depends_on_id` is in the manifest's tethys set.

**Step 2: Apply decision 7's resolution per edge**

For each cross-boundary edge:

- If decision 7 = (b) **`external_ref`**: in the migrated issue, replace the cross-boundary `dependencies` entry with an `external_ref` URL pointing to the issue's location in the *other* repo (e.g. `"external_ref":"https://github.com/dwalleck/rivets/issues/rivets-4tev"`).
- If decision 7 = (c) **body note**: keep the dependency removed, append a sentence to the issue's `description` recording the original relationship.
- If decision 7 = (a) **drop**: silently remove the cross-boundary dep entry.

For `parent-child` edges where the *parent* is on the other side, this matters a lot — orphaned children lose context.

**Step 3: Update the manifest**

Add a "Cross-boundary deps" column listing the resolutions per issue.

**Step 4: No commit needed** — still working on the manifest.

---

### Task 18: Migrate issues into the new tethys repo

**Branches based on decision 2.**

#### Path A: decision 2 = (a) GitHub Issues

**Step 1: Enable GitHub Issues on the new repo**

```bash
gh repo edit dwalleck/tethys --enable-issues=true
```

**Step 2: For each issue in the manifest (destination=tethys), create on GitHub**

A loop using `gh issue create`:

```bash
# Pseudo-script — run from a scratch dir with the manifest available
while IFS= read -r line; do
  id=$(echo "$line" | jq -r .id)
  title=$(echo "$line" | jq -r .title)
  body=$(echo "$line" | jq -r .description)
  labels=$(echo "$line" | jq -r '.labels | join(",")')
  status=$(echo "$line" | jq -r .status)

  # Append origin reference to body
  body="$body

---
Migrated from rivets monorepo. Original ID: $id"

  url=$(gh issue create --repo dwalleck/tethys \
    --title "$title" \
    --body "$body" \
    --label "$labels")

  # Save mapping for Task 19 (closing migrated issues in rivets)
  echo "$id $url" >> /tmp/migration-mapping.tsv

  # Close immediately if already closed in rivets
  if [ "$status" = "closed" ]; then
    issue_num=$(basename "$url")
    gh issue close --repo dwalleck/tethys "$issue_num" \
      --comment "Originally closed in rivets monorepo."
  fi
done < /tmp/tethys-labeled.jsonl
```

**Step 3: Re-create dependencies as GitHub task-list references**

GitHub Issues has no first-class dep graph; the common convention is task-list checkboxes in the parent that link to children: `- [ ] dwalleck/tethys#42`. For each `parent-child` rel in the manifest, edit the parent issue to include such a list.

**Step 4: Verify**

```bash
gh issue list --repo dwalleck/tethys --state all --limit 100 | wc -l
```
Expected: ~38 issues.

#### Path B: decision 2 = (b) `.rivets/issues.jsonl`

**Step 1: Initialize the new repo's `.rivets/`**

In the new tethys repo's working tree:

```bash
rivets init --prefix tethys  # generates .rivets/config.yaml with issue-prefix: tethys
```

**Step 2: Build the migration JSONL**

Write a one-off script (Python or jq) that, for each line in `/tmp/tethys-labeled.jsonl` (and triaged additions from Task 16 step 3):

1. **ID rewriting per decision 6:**
   - (a) Keep: leave `id` unchanged.
   - (b) Rename: generate a new `tethys-XXXX` ID (use the existing rivets ID-generator logic, or hand-assign sequentially).
   - (c) Rename + `external_ref`: generate `tethys-XXXX`, set `external_ref` to the original `rivets-XXXX`.

2. **Dependency rewriting:**
   - Intra-tethys deps: update `depends_on_id` to the new tethys ID (if renaming).
   - Cross-boundary deps: apply Task 17's resolution.

3. **Append to new repo's `.rivets/issues.jsonl`:**
   ```bash
   cat /tmp/migrated-tethys-issues.jsonl >> .rivets/issues.jsonl
   ```

**Step 3: Verify in new repo**

```bash
rivets stats        # confirm issue count
rivets list         # spot-check open issues
rivets blocked      # verify dep graph is sensible (no dangling refs)
rivets show <id>    # verify external_ref / body notes on a few migrated issues
```

#### Both paths: Step 4: Commit in new repo

```bash
git add .rivets/
git commit -m "chore: migrate tethys issues from rivets monorepo

Migrated 38 tethys-labeled issues (25 closed, 12 open, 1 in_progress)
plus N additional issues identified during audit. See migration manifest
for ID mappings and cross-repo dependency resolutions."
git push
```

---

### Task 19: Update rivets-side issues to point at new repo

**Goal:** In the rivets repo, every migrated issue should be closed (if still open) and carry an `external_ref` to its new location. This preserves discoverability — `rivets show rivets-XXX` still returns context, just with a "moved to" pointer.

**Step 1: Create the rivets cleanup branch**

```bash
cd /path/to/rivets
git checkout main
git pull
git checkout -b chore/remove-tethys-from-workspace
```

**Step 2: For each issue in the manifest with destination=tethys**

If the issue is **already closed**, update only `external_ref`:

```bash
rivets update rivets-XXX --external-ref "<new-url-or-id-from-mapping>"
```

(If the CLI lacks an `--external-ref` flag, edit the JSONL directly: each issue is a single line; rewrite the `"external_ref":null` field to the new URL.)

If the issue is **open or in_progress**, close it with a migration reason:

```bash
rivets close rivets-XXX --reason "Migrated to tethys repo: <new-url-or-id>"
rivets update rivets-XXX --external-ref "<new-url-or-id>"
```

**Step 3: Audit**

```bash
# Should be zero — every tethys issue should now be closed
cargo run -- list --label tethys --status open
cargo run -- list --label tethys --status in_progress
```

**Step 4: Commit on cleanup branch**

```bash
git add .rivets/issues.jsonl
git commit -m "chore: close tethys issues migrated to standalone repo

External refs point to the new tethys repo for traceability.
38 tethys-labeled issues + N cross-cutting issues processed."
```

---

### Task 20: Remove tethys from workspace ⚠️

> The cleanup branch `chore/remove-tethys-from-workspace` already exists from Task 19 (it now also carries the migrated-issue closures). Continue on the same branch.

**STOP.** This removes a substantial code directory. The history is preserved in the new repo, but confirm with the user that the new repo is healthy (Task 13 CI green) and Task 19's issue closures are committed before deleting here.

**Files:**
- Modify: `Cargo.toml` (root)
- Delete: `crates/tethys/` (entire directory)

**Step 1: Remove from workspace members**

In root `Cargo.toml:3-8`, change:

```toml
members = [
    "crates/rivets-jsonl",
    "crates/rivets",
    "crates/rivets-mcp",
    "crates/tethys",
]
```

To:

```toml
members = [
    "crates/rivets-jsonl",
    "crates/rivets",
    "crates/rivets-mcp",
]
```

**Step 2: Delete the crate directory**

```bash
git rm -r crates/tethys/
```

**Step 3: Verify rivets workspace still builds**

Run: `cargo build --workspace`
Expected: clean build of the three remaining crates.

Run: `cargo nextest run --workspace`
Expected: all rivets tests pass.

**Step 4: Commit**

```bash
git add Cargo.toml
git commit -m "chore: remove tethys from workspace

Tethys has been extracted to https://github.com/dwalleck/tethys.
History is preserved in the new repo via git filter-repo."
```

---

### Task 21: Remove orphan workspace dependencies

**Files:**
- Modify: `Cargo.toml` (root)

**Step 1: Run `cargo machete` to find unused deps**

Run: `cargo machete`
Expected output: a list of workspace deps no longer referenced by any remaining crate. Likely candidates:

- `petgraph` (used by tethys for graph ops)
- The rest of `[workspace.dependencies]` is likely still used by rivets/rivets-jsonl/rivets-mcp — verify before removing.

**Step 2: Verify each flagged dep manually**

For each dep machete flags, confirm:

```bash
grep -r "<dep_name>" crates/*/Cargo.toml crates/*/src/
```

If no matches outside `target/`, safe to remove.

**Step 3: Remove confirmed-orphan workspace deps**

Edit `Cargo.toml` `[workspace.dependencies]` section — delete each confirmed orphan line.

**Step 4: Verify build**

Run: `cargo build --workspace`
Expected: clean.

**Step 5: Commit**

```bash
git add Cargo.toml
git commit -m "chore: remove workspace dependencies orphaned by tethys removal"
```

---

### Task 22: Update `CLAUDE.md`

**Files:**
- Modify: `CLAUDE.md`

**Step 1: Remove tethys references**

Edit `CLAUDE.md`:

- In the "Architecture" crate table: delete the `tethys` row.
- In "Crate-specific" testing commands: delete the `cargo nextest run -p tethys` line.
- In "Suggested Scopes (Optional)": delete `tethys: Code intelligence engine`.
- Search for any other "tethys" mentions in the file and remove or update.

**Step 2: Verify**

Run: `grep -i tethys CLAUDE.md`
Expected: no matches.

**Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: drop tethys references from CLAUDE.md post-extraction"
```

---

### Task 23: Update root `README.md` and `docs/`

**Files:**
- Modify: `README.md`
- Modify: `docs/README.md`, `docs/architecture.md`, `docs/data-flow.md`, `docs/design/code-intelligence.md`, `docs/module-structure.md`, `docs/task-dependency-graph.md`, `docs/terminology.md` (trim tethys sections from cross-cutting docs)
- Delete (if decision 8 = (b) or (c)): the 16 tethys-only docs listed below

**Step 1: Strip tethys mentions from rivets `README.md`**

Remove any sections describing tethys as part of rivets. Add a brief note pointing to the new repo:

```markdown
> **Tethys** (code intelligence engine) has moved to [its own repository](https://github.com/dwalleck/tethys).
```

**Step 2: Remove tethys-only docs from rivets (if decision 8 = (b) or (c))**

If decision 8 = (a) (don't move docs), skip this step — the tethys-only docs stay in rivets as historical artifacts and are *not* in the new repo.

Otherwise, `git rm` the 16 files that moved to the new repo. These match the `tethys-paths.txt` entries from Task 8 except for the `crates/tethys` entry (which is removed by Task 20):

```bash
git rm docs/design/tethys-code-intelligence.md
git rm docs/design/tethys-architecture-analysis.md
git rm docs/spikes/2026-01-22-tethys-sqlite-petgraph.md
git rm docs/plans/2026-01-22-blast-radius-analysis-design.md
git rm docs/plans/2026-01-25-phase3-graph-operations-design.md
git rm docs/plans/2026-01-25-phase3-graph-operations-impl.md
git rm docs/plans/2026-01-28-cross-file-resolution.md
git rm docs/plans/2026-02-01-cargo-toml-parsing-design.md
git rm docs/plans/2026-02-01-cargo-toml-parsing.md
git rm docs/plans/2026-02-01-module-path-computation.md
git rm docs/plans/2026-02-01-module-path-implementation.md
git rm docs/plans/2026-03-18-tethys-quality-alignment-design.md
git rm docs/plans/2026-03-18-tethys-quality-alignment.md
git rm docs/plans/2026-03-19-lsp-session-result-design.md
git rm docs/plans/2026-03-19-lsp-session-result.md
git rm docs/plans/2026-05-10-tethys-architecture-analysis.md
```

`docs/plans/2026-05-11-tethys-repo-split.md` (this plan) stays in rivets — it's the meta-doc describing the split.

After deletion, the `docs/spikes/` directory will be empty (its only file was the SQLite+petgraph spike). Decide whether to leave the empty dir or remove it.

**Step 3: Trim tethys sections from cross-cutting docs**

The following docs mention tethys but cover broader workspace concerns. Open each and trim tethys-specific sections while preserving rivets-relevant content:

- `docs/README.md` — currently rivets-focused with no tethys content (verified empty); no changes needed.
- `docs/architecture.md` — search for "tethys" and remove the crate from any per-crate breakdowns; keep workspace-level architecture.
- `docs/design/code-intelligence.md` — this is the precursor doc that became `tethys-code-intelligence.md`. Either delete it (`git rm`) since the successor moved to the tethys repo, or keep as historical and add a pointer to the new repo. **Recommended:** delete, since it's superseded.
- `docs/data-flow.md`, `docs/module-structure.md`, `docs/task-dependency-graph.md`, `docs/terminology.md` — grep each for "tethys" and trim any references to the crate as a current workspace member. Add a "see also" pointer to the new repo if appropriate.

```bash
# Audit pass after edits — should show only acceptable references (e.g. "see https://github.com/dwalleck/tethys")
grep -ri tethys docs/
```

**Step 4: Commit**

```bash
git add README.md docs/
git commit -m "docs: remove tethys content and update cross-cutting docs

The 16 tethys-only design docs and plans have moved to the new tethys
repository (with full git history, via git filter-repo). Cross-cutting
docs are trimmed to remove tethys-specific sections."
```

---

### Task 24: Final verification and PR

**Step 1: Full workspace verification**

Run: `cargo build --workspace --all-features`
Expected: clean.

Run: `cargo nextest run --workspace --all-features`
Expected: all remaining tests pass.

Run: `cargo clippy --workspace --all-features --all-targets -- -D warnings`
Expected: zero warnings.

Run: `cargo fmt --all -- --check`
Expected: clean.

Run: `cargo machete`
Expected: no remaining unused workspace deps.

**Step 2: Open cleanup PR**

```bash
git push -u origin chore/remove-tethys-from-workspace
gh pr create --title "chore: remove tethys from workspace (extracted to standalone repo)" --body "$(cat <<'EOF'
## Summary
- Removes `crates/tethys/` from the rivets workspace
- Tethys is now maintained at https://github.com/dwalleck/tethys
- Drops orphaned workspace dependencies
- Updates `CLAUDE.md` and `README.md`

History for `crates/tethys/` is preserved in the new repo via `git filter-repo`.

## Test plan
- [x] `cargo build --workspace` — clean
- [x] `cargo nextest run --workspace` — all pass
- [x] `cargo clippy --workspace -- -D warnings` — zero warnings
- [x] `cargo machete` — no unused deps
- [x] New tethys repo CI is green (verified at Task 13)
EOF
)"
```

**Step 3: Merge after review.**

---

## Post-merge checklist

After the cleanup PR merges to `main`:

- [ ] Tag a fresh rivets release if appropriate (the removal is a noteworthy event).
- [ ] Update any external docs/blogs/links that reference `crates/tethys` paths in the rivets repo.
- [ ] If `tethys` is published to crates.io, add a `[dev-dependencies]` or example showing how rivets consumes it (if applicable — currently no consumption).
- [ ] Delete this plan doc, or move it to `docs/plans/archive/` if the project keeps completed plans.

---

## Rollback notes

- **Phase A is reversible.** Revert the prep PR; tethys returns to workspace inheritance.
- **Phase C is reversible until Task 12 push.** Delete the scratch clone, no harm done.
- **Phase C is irreversible after Task 12.** The new repo exists; deleting it loses any work that happens there. Only delete via `gh repo delete dwalleck/tethys` if it's the same day and no commits have been added.
- **Phase E is reversible.** The cleanup PR can be reverted; tethys can be re-added to the workspace by copying files back from the new repo (history will diverge, but functionally equivalent).
