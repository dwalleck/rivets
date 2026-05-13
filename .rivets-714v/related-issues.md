# Prior-art search for rivets-714v

Date: 2026-05-13
Per gilfoyle v1.0.1 prove-it-prototype Step 0.

## Searched

`rivets list` filtered for LSP / rust-analyzer / goto_definition / multi-crate / workspace-root keywords. Also examined the full `.rivets/issues.jsonl` for any text-matching tickets.

## Related (closed) — LSP infrastructure that produces the bug

| Issue | Title | Relevance |
|---|---|---|
| rivets-nwwm | Implement LspProvider trait + RustAnalyzerProvider | Wired up the provider abstraction; the URI construction issue likely lives in or near this code |
| rivets-h1va | Implement LspClient with JSON-RPC transport | **Most likely site of the bug** — JSON-RPC transport is where URIs are serialized into the wire format |
| rivets-k3mv | Integrate LSP into `index --lsp` | The integration call site (resolve.rs::resolve_via_lsp); doesn't directly construct URIs but invokes the chain that does |
| rivets-oeu5 | Add `--lsp` flag to CLI commands | Just the flag plumbing |
| rivets-9o82 | Add gated integration tests for LSP | Source of the counter-evidence — `lsp_resolves_method_on_inferred_type` passes single-crate, which proves rust-analyzer + tethys integration *does* work in the simple case |
| rivets-6x7g | Add CSharpLsProvider | Sibling provider, not directly relevant; useful as a comparison point if the URI bug is Rust-specific |
| rivets-1dza | Integrate LSP into `callers --lsp` | Different integration point; likely affected by the same bug but not the entry path for indexing |

## Related (open) — none describing this bug

Only rivets-714v itself is open in the LSP area. The bug is not a re-discovery of prior work; it's a side finding from the rivets-3d0s investigation (per the rivets-714v issue body).

## No prior bug ticket for the same symptom

No closed/open issue describes `url is not a file` errors, Windows URI backslash issues, or multi-crate-workspace-specific LSP failures. The rivets-714v ticket is the first writeup of this bug class.

## Code surface to investigate

Per the rivets-714v issue body's two hypotheses:

1. `crates/tethys/src/lsp/transport.rs` (most likely: URI construction)
2. `crates/tethys/src/lsp/provider.rs` (workspace_root initialization)
3. `crates/tethys/src/resolve.rs::resolve_via_lsp` (Pass 3 caller; less likely to have the bug but where the error surfaces)

## Conclusion

No prior art duplicates the bug. Proceeding with probe construction. Counter-evidence (single-crate works) confirms the bug is bounded — not a fundamental LSP/rust-analyzer integration failure.
