"""
Independent oracle for rivets-714v.

Bypasses tethys entirely: spawns rust-analyzer directly via stdio JSON-RPC and
sends initialize + didOpen + textDocument/definition against the same 2-crate
fixture the probe uses. If rust-analyzer succeeds via this client and fails
via tethys, the bug is unambiguously in tethys's URI / handshake — not in
rust-analyzer or the fixture itself.

Mechanism (different from the probe's `tethys ... --lsp` subprocess approach):
direct JSON-RPC over rust-analyzer's stdin/stdout, hand-crafted messages, no
shared code with tethys's LSP client.
"""

import json
import os
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

# Same fixture as the probe — must match exactly so the comparison is honest.
FIXTURE_FILES = {
    "Cargo.toml":
        "[workspace]\nmembers = [\"crate_caller\", \"crate_target\"]\nresolver = \"2\"\n",
    "crate_caller/Cargo.toml":
        "[package]\nname = \"crate_caller\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    "crate_caller/src/lib.rs":
        "use std::collections::HashMap;\n"
        "pub fn count(map: &HashMap<u32, String>) -> usize {\n"
        "    map.len()\n"
        "}\n",
    "crate_target/Cargo.toml":
        "[package]\nname = \"crate_target\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    "crate_target/src/lib.rs":
        "pub struct Widget;\nimpl Widget { pub fn ping(&self) {} }\n",
}


def path_to_uri_unix_style(path: Path) -> str:
    """Construct a `file://` URI without canonicalize().

    Intentionally bypasses Path.resolve()/canonicalize() which on Windows
    returns `\\\\?\\C:\\...` extended-length paths. Uses the as-given absolute
    path with backslashes → forward slashes.
    """
    p = path.absolute()
    s = str(p).replace("\\", "/")
    if sys.platform == "win32":
        return f"file:///{s}"
    return f"file://{s}"


def write_msg(stdin, obj):
    body = json.dumps(obj).encode("utf-8")
    header = f"Content-Length: {len(body)}\r\n\r\n".encode("ascii")
    stdin.write(header)
    stdin.write(body)
    stdin.flush()


def read_msg(stdout):
    headers = {}
    while True:
        line = stdout.readline()
        if not line or line == b"\r\n":
            break
        if b":" in line:
            k, _, v = line.partition(b":")
            headers[k.strip().lower()] = v.strip()
    length = int(headers.get(b"content-length", b"0"))
    if length == 0:
        return None
    body = stdout.read(length)
    return json.loads(body)


def request(stdin, stdout, rid, method, params):
    write_msg(stdin, {"jsonrpc": "2.0", "id": rid, "method": method, "params": params})
    while True:
        msg = read_msg(stdout)
        if msg is None:
            return None
        if "id" in msg and msg["id"] == rid:
            return msg


def notify(stdin, method, params):
    write_msg(stdin, {"jsonrpc": "2.0", "method": method, "params": params})


def main():
    with tempfile.TemporaryDirectory(prefix="rivets-714v-oracle-") as tmp:
        workspace = Path(tmp)
        for rel, content in FIXTURE_FILES.items():
            p = workspace / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(content, encoding="utf-8")

        workspace_uri = path_to_uri_unix_style(workspace)
        caller = workspace / "crate_caller" / "src" / "lib.rs"
        caller_uri = path_to_uri_unix_style(caller)

        print(f"Fixture: {workspace}")
        print(f"workspace URI: {workspace_uri}")
        print(f"caller URI:    {caller_uri}")

        ra = subprocess.Popen(
            ["rust-analyzer"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            bufsize=0,
        )
        # Drain stderr in background to avoid pipe-fill blocking
        def drain():
            for _ in iter(ra.stderr.readline, b""):
                pass
        threading.Thread(target=drain, daemon=True).start()

        try:
            init_resp = request(ra.stdin, ra.stdout, 1, "initialize", {
                "processId": os.getpid(),
                "rootUri": workspace_uri,
                "workspaceFolders": [{"uri": workspace_uri, "name": "fixture"}],
                "capabilities": {},
            })
            print(f"initialize OK: {'result' in init_resp}")
            notify(ra.stdin, "initialized", {})

            notify(ra.stdin, "textDocument/didOpen", {
                "textDocument": {
                    "uri": caller_uri,
                    "languageId": "rust",
                    "version": 1,
                    "text": caller.read_text(encoding="utf-8"),
                },
            })

            # Position of `.len()` call: line 2 (0-indexed), column ~8 (after "map.")
            #   use std::collections::HashMap;
            #   pub fn count(map: &HashMap<u32, String>) -> usize {
            #       map.len()    <- line 2, the 'l' of len is around column 8
            # }
            def_resp = request(ra.stdin, ra.stdout, 2, "textDocument/definition", {
                "textDocument": {"uri": caller_uri},
                "position": {"line": 2, "character": 9},
            })

            print(f"\n=== textDocument/definition response ===")
            print(json.dumps(def_resp, indent=2)[:800])

            if def_resp is None:
                verdict = "FAIL — no response"
            elif "error" in def_resp:
                verdict = f"FAIL — error: {def_resp['error']}"
            elif def_resp.get("result"):
                verdict = "SUCCESS — got a definition location"
            else:
                # rust-analyzer often takes time to load the workspace; first
                # response may be empty/null. That's still NOT 'url is not a file'.
                verdict = "PARTIAL — no error but no result either (workspace likely still loading)"

            print(f"\n=== Verdict ===")
            print(f"  {verdict}")
            print(f"\n  Key comparison vs probe:")
            print(f"  - probe (tethys --lsp): fails with 'url is not a file' -32603")
            print(f"  - oracle (direct client): {verdict}")

        finally:
            try:
                request(ra.stdin, ra.stdout, 99, "shutdown", None)
                notify(ra.stdin, "exit", None)
            except Exception:
                pass
            ra.terminate()
            ra.wait(timeout=5)


if __name__ == "__main__":
    main()
