"""
prove-it-prototype probe for rivets-714v.

Smallest factual question:
  Does tethys's `--lsp` flag successfully complete `goto_definition` against
  a 2-crate Cargo workspace?

Mechanism: build a minimal 2-crate fixture in a tempdir, run
`tethys index --rebuild --lsp -w <fixture>`, capture stderr, check for the
characteristic error pattern `url is not a file` (LSP error -32603).

Oracle (.rivets-714v/oracle.py) sends the same `initialize` + `didOpen` +
`textDocument/definition` sequence directly to rust-analyzer via a stdio
JSON-RPC client, bypassing tethys entirely. If rust-analyzer succeeds via
the direct client and fails via tethys, the bug is unambiguously in
tethys's URI construction or workspace-root handshake.
"""

import re
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
TETHYS = REPO / "target" / "release" / "tethys.exe"
if not TETHYS.exists():
    sys.exit(f"missing release binary: {TETHYS} (run `cargo build --release -p tethys`)")

FIXTURE_FILES = {
    "Cargo.toml":
        "[workspace]\nmembers = [\"crate_caller\", \"crate_target\"]\nresolver = \"2\"\n",
    "crate_caller/Cargo.toml":
        "[package]\nname = \"crate_caller\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    "crate_caller/src/lib.rs":
        # Use std::collections::HashMap and call .len() — triggers a method ref
        # that tethys's Pass 1/2 will try to resolve and Pass 3 (LSP) would
        # refine. This is the same shape as the rivets-3d0s lsp_probe fixture.
        "use std::collections::HashMap;\n"
        "pub fn count(map: &HashMap<u32, String>) -> usize {\n"
        "    map.len()\n"
        "}\n",
    "crate_target/Cargo.toml":
        "[package]\nname = \"crate_target\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    "crate_target/src/lib.rs":
        "pub struct Widget;\nimpl Widget { pub fn ping(&self) {} }\n",
}

ERROR_PATTERN = re.compile(r"url is not a file|LSP error -32603")

with tempfile.TemporaryDirectory(prefix="rivets-714v-probe-") as tmp:
    workspace = Path(tmp)
    for rel, content in FIXTURE_FILES.items():
        path = workspace / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")

    print(f"Fixture: {workspace}")
    cmd = [str(TETHYS), "index", "-w", str(workspace), "--rebuild", "--lsp"]
    print(f"$ {' '.join(cmd)}")
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=180)
    combined = (result.stdout or "") + "\n" + (result.stderr or "")
    matches = [line for line in combined.splitlines() if ERROR_PATTERN.search(line)]

    print(f"\nexit code: {result.returncode}")
    print(f"matched error lines: {len(matches)}")
    for line in matches[:5]:
        print(f"  {line}")

    print("\n=== Verdict ===")
    if matches:
        print(f"  CONFIRMED: tethys --lsp emits 'url is not a file' errors on the 2-crate fixture.")
        print(f"  Bug reproduces. Proceed to oracle to verify rust-analyzer itself works on this fixture.")
    else:
        print(f"  SURPRISE: no matching error lines. Bug may have been fixed already, or the")
        print(f"  fixture/build version doesn't trigger it. Investigate before proceeding.")
        print(f"  Full stderr tail:")
        for line in (result.stderr or "").splitlines()[-20:]:
            print(f"    {line}")
