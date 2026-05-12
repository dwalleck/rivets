"""Look at where tethys recorded the suspicious 'fabricated' symbols."""
import sqlite3
from pathlib import Path

conn = sqlite3.connect(Path(".rivets/index/tethys.db"))
for name in ("Serialize", "Deserialize", "Tree", "Parser"):
    print(f"=== Symbols named {name!r} in tethys's DB ===")
    rows = conn.execute("""
        SELECT s.name, s.kind, s.qualified_name, f.path, s.line, s.signature
        FROM symbols s JOIN files f ON f.id = s.file_id
        WHERE s.name = ?
    """, (name,)).fetchall()
    for nm, kd, qual, path, line, sig in rows:
        print(f"  {path}:{line}  kind={kd:<12}  qual={qual!r}")
        if sig:
            sig_short = sig.replace("\n", " ")[:120]
            print(f"     signature: {sig_short}")
    if not rows:
        print("  (no rows)")
    print()
