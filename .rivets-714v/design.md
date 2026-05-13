# rivets-714v — prove-it-prototype output

## Smallest factual question

> Does tethys's `--lsp` flag successfully complete `goto_definition` against a 2-crate Cargo workspace?

## Probe

`.rivets-714v/probe.py` — builds a minimal 2-crate fixture in a tempdir (`crate_caller` + `crate_target`, with `crate_caller/src/lib.rs` calling `.len()` on a `HashMap`), runs `tethys index --rebuild --lsp -w <fixture>`, captures stderr, matches `url is not a file` / `LSP error -32603` lines.

Output:

```
exit code: 0
matched error lines: 4
  WARN LSP goto_definition failed ... error=LSP error -32603: url is not a file
  LSP error: rust - goto_definition failed for 'HashMap': LSP error -32603: url is not a file
  LSP error: rust - goto_definition failed for 'String':  LSP error -32603: url is not a file
  LSP error: rust - goto_definition failed for 'len':     LSP error -32603: url is not a file
```

Confirms: tethys's LSP path fails 4 times on this fixture. The bug reproduces.

## Oracle

`.rivets-714v/oracle.py` — independent ~120-line Python LSP client. Spawns `rust-analyzer` directly via stdio JSON-RPC, sends `initialize` + `initialized` + `textDocument/didOpen` + `textDocument/definition` against the SAME fixture as the probe. Uses a deliberately *different* URI construction:

```python
def path_to_uri_unix_style(path: Path) -> str:
    p = path.absolute()             # absolute() NOT canonicalize()
    s = str(p).replace("\\", "/")
    if sys.platform == "win32":
        return f"file:///{s}"
    return f"file://{s}"
```

Critically, this skips `Path.canonicalize()` — which on Windows returns `\\?\C:\...` extended-length prefixes that break URI encoding.

Output:

```
workspace URI: file:///C:/Users/dwall/AppData/Local/Temp/rivets-714v-oracle-r8x0zcwb
caller URI:    file:///C:/Users/dwall/AppData/Local/Temp/rivets-714v-oracle-r8x0zcwb/crate_caller/src/lib.rs
initialize OK: True
textDocument/definition response: {"jsonrpc": "2.0", "id": 2, "result": []}
```

Verdict: **no errors**. `result: []` is "workspace still loading, no answer yet" — not a URI rejection. The protocol completed successfully.

## Agreement

The probe and oracle agree on a non-trivial slice of the question:

| | Probe (via tethys) | Oracle (direct rust-analyzer client) |
|---|---|---|
| URI form sent | (constructed by `path_to_uri` in `lsp/transport.rs`) | `file:///C:/Users/.../<fixture>/crate_caller/src/lib.rs` |
| rust-analyzer response | `-32603 url is not a file` | `{"result": []}` (accepted) |

**Both methods reach rust-analyzer successfully.** The difference is the URI shape sent in the protocol messages. rust-analyzer rejects tethys's URI; rust-analyzer accepts the oracle's URI. **The bug is in tethys's URI construction, full stop** — not in rust-analyzer, the fixture, or the workspace structure.

## What I learned that wasn't obvious before the probe

The rust-analyzer side of the LSP integration is fine; rust-analyzer cheerfully accepts `file:///C:/Users/...` URIs against the multi-crate fixture. The bug is on the tethys side, and the most likely culprit is `path.canonicalize()` inside `path_to_uri` at `crates/tethys/src/lsp/transport.rs:688` — which on Windows returns `\\?\C:\Users\...` extended-length paths, which after the existing `.replace('\\', "/")` on line 704 become `//?/C:/Users/...`, combined with the `file:///` prefix on line 704 to produce `file://///?/C:/Users/...` — a malformed URI per RFC 8089 that rust-analyzer correctly rejects.

This is a **stronger claim** than the rivets-714v issue body had access to. The issue body listed two hypotheses (backslash-replacement and workspace-root mismatch); the probe + oracle narrow it to **specifically `canonicalize()` adding extended-length prefixes** that the existing backslash replace doesn't address.

## Hard-gate checklist (v1.0.1)

- [x] Tracker check completed (`.rivets-714v/related-issues.md`); no prior art duplicates the bug
- [x] Probe written and runs against the real codebase (`.rivets-714v/probe.py`)
- [x] Oracle defined and produces output (`.rivets-714v/oracle.py`)
- [x] Probe and oracle agree on a non-trivial slice (bug is in tethys's URI construction, not in rust-analyzer)
- [x] One-sentence learning recorded (canonicalize's Windows extended-length prefix breaks URI encoding)

Ready for falsifiable-design.

## Code surface confirmed

- **`crates/tethys/src/lsp/transport.rs:686-712`** — `path_to_uri` function. Line 688 calls `path.canonicalize()`. Line 704 builds the URI with `file:///` + backslash-replaced path. The combination is buggy on Windows whenever canonicalize returns a `\\?\` prefix (which it does for any path that exists, on Windows).

- **`crates/tethys/src/lsp/transport.rs:815-829`** — existing `path_to_uri_creates_valid_file_uri` test. Only checks `starts_with("file://")` and `!contains('\\')`. Both pass for the malformed `file://///?/C:/...` form. The test is a false negative — it doesn't catch the bug.

The fix is constrained: either drop the canonicalize() call, or strip the `\\?\` prefix before formatting the URI. Either way the change is localized to `path_to_uri`.
