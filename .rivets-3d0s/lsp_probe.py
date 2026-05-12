"""
LSP probe for rivets-3d0s.

Question: does tethys's --lsp flag (Pass 3) eliminate phantom edges that
Pass 1/2 wrongly created, OR does it only fill NULL gaps?

Builds a minimal fixture exhibiting the rivets-3d0s pattern:
  - caller crate calls .len() on a stdlib HashMap
  - target crate defines a method named `len` on its own type
  Pre-fix: tethys's resolver picks target's `len` (phantom cross-crate edge)

Then indexes the fixture twice: once without --lsp, once with --lsp.
Compares file_deps.

If the phantom edge persists under --lsp, confirms my reading of resolve.rs:
Pass 3 only fills NULL gaps via `WHERE r.symbol_id IS NULL`, never audits
existing resolutions. The rivets-3d0s fix shape needs a kind-compatibility
audit step BEFORE Pass 3.

If the phantom edge disappears under --lsp, my reading was wrong and we
need to re-investigate.
"""
import shutil
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
TETHYS = REPO / "target" / "release" / "tethys.exe"
if not TETHYS.exists():
    sys.exit(f"missing release binary: {TETHYS}")

FIXTURE_FILES = {
    "Cargo.toml": """\
[workspace]
members = ["crate_caller", "crate_target"]
resolver = "2"
""",
    "crate_caller/Cargo.toml": """\
[package]
name = "crate_caller"
version = "0.1.0"
edition = "2021"
""",
    "crate_caller/src/lib.rs": """\
use std::collections::HashMap;

pub fn count_items(map: &HashMap<u32, String>) -> usize {
    map.len()
}
""",
    "crate_target/Cargo.toml": """\
[package]
name = "crate_target"
version = "0.1.0"
edition = "2021"
""",
    "crate_target/src/lib.rs": """\
pub struct WarningCollector {
    warnings: Vec<String>,
}

impl WarningCollector {
    pub fn len(&self) -> usize {
        self.warnings.len()
    }
}
""",
}

def write_fixture(root: Path):
    for rel, content in FIXTURE_FILES.items():
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)

def query_phantom_edges(db: Path):
    conn = sqlite3.connect(db)
    try:
        return conn.execute("""
            SELECT f1.path, f2.path, d.ref_count
            FROM file_deps d
            JOIN files f1 ON f1.id = d.from_file_id
            JOIN files f2 ON f2.id = d.to_file_id
            WHERE f1.path LIKE 'crate_caller/%'
              AND f2.path LIKE 'crate_target/%'
            ORDER BY f1.path, f2.path
        """).fetchall()
    finally:
        conn.close()

def query_caller_refs(db: Path):
    """Return all refs from crate_caller/src/lib.rs with their resolution status."""
    conn = sqlite3.connect(db)
    try:
        return conn.execute("""
            SELECT r.reference_name, r.kind, r.symbol_id,
                   s.name, s.kind, f_target.path
            FROM refs r
            JOIN files f_caller ON f_caller.id = r.file_id
            LEFT JOIN symbols s ON s.id = r.symbol_id
            LEFT JOIN files f_target ON f_target.id = s.file_id
            WHERE f_caller.path LIKE '%crate_caller%lib.rs'
            ORDER BY r.line, r.column
        """).fetchall()
    finally:
        conn.close()

def index(workspace: Path, lsp: bool):
    cmd = [str(TETHYS), "index", "-w", str(workspace), "--rebuild"]
    if lsp:
        cmd.append("--lsp")
    print(f"  $ {' '.join(cmd)}")
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=180)
    if result.returncode != 0:
        print(f"  STDOUT:\n{result.stdout}")
        print(f"  STDERR:\n{result.stderr}")
        raise RuntimeError(f"tethys index failed (exit {result.returncode})")
    # Surface the last 5 lines of stdout so we see indexing stats
    for line in result.stdout.strip().splitlines()[-5:]:
        print(f"  {line}")

def run_pass(workspace: Path, lsp: bool):
    label = "WITH --lsp" if lsp else "WITHOUT --lsp"
    print(f"\n=== Pass: {label} ===")
    index(workspace, lsp=lsp)
    db = workspace / ".rivets" / "index" / "tethys.db"
    edges = query_phantom_edges(db)
    refs = query_caller_refs(db)
    print(f"  phantom file_deps (crate_caller -> crate_target): {edges}")
    print(f"  ALL refs in crate_caller/src/lib.rs:")
    for ref_name, ref_kind, sym_id, sym_name, sym_kind, target_path in refs:
        status = "UNRESOLVED" if sym_id is None else f"-> {sym_name!r} ({sym_kind}) in {target_path}"
        print(f"    {ref_name!r:<15} kind={ref_kind:<8} {status}")
    return edges, refs

with tempfile.TemporaryDirectory(prefix="rivets-3d0s-lsp-probe-") as tmp:
    workspace = Path(tmp)
    print(f"Fixture workspace: {workspace}")
    write_fixture(workspace)

    edges_no_lsp, refs_no_lsp = run_pass(workspace, lsp=False)
    edges_lsp, refs_lsp = run_pass(workspace, lsp=True)

    print("\n=== Verdict ===")
    print(f"  WITHOUT --lsp phantom edges: {len(edges_no_lsp)}")
    print(f"  WITH --lsp    phantom edges: {len(edges_lsp)}")
    if not edges_no_lsp:
        print("  WARNING: fixture didn't reproduce the phantom even without --lsp.")
        print("  Probe needs adjustment.")
    elif edges_lsp == edges_no_lsp:
        print("  CONFIRMED: --lsp does NOT audit existing resolutions.")
        print("  The fix-shape hypothesis stands: need kind-compatibility")
        print("  audit step BEFORE Pass 3.")
    elif not edges_lsp:
        print("  SURPRISE: --lsp eliminated the phantom. My reading of")
        print("  resolve.rs Pass 3 was wrong; need to re-investigate.")
    else:
        print(f"  PARTIAL: --lsp reduced phantom edges from {len(edges_no_lsp)}")
        print(f"  to {len(edges_lsp)}. Investigate the difference.")
