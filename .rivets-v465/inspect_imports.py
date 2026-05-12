"""One-shot: show what tethys extracted as imports for the caller files
that the probe classified as having no target import."""
import sqlite3
from pathlib import Path

conn = sqlite3.connect(Path(".rivets/index/tethys.db"))
for path in [
    "crates/rivets-mcp/src/server.rs",
    "crates/rivets-mcp/tests/integration.rs",
    "crates/rivets-mcp/src/context.rs",
    "crates/rivets-mcp/src/tools.rs",
]:
    fid = conn.execute("SELECT id FROM files WHERE path = ?", (path,)).fetchone()
    if not fid:
        print(f"\n{path}: NOT IN DB"); continue
    print(f"\n=== {path} (file_id={fid[0]}) ===")
    imports = conn.execute(
        "SELECT symbol_name, source_module, alias FROM imports WHERE file_id = ? ORDER BY source_module",
        (fid[0],),
    ).fetchall()
    if not imports:
        print("  (no imports recorded)")
    for sym, src, alias in imports:
        alias_part = f" as {alias}" if alias else ""
        print(f"  use {src}::{sym}{alias_part}")
