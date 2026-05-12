"""
Probe step 2 (refinement): for one phantom (from_crate, to_crate) pair,
dump every (from_file, to_file) pair that contributes. Look at the
basenames — the rivets-0gom hypothesis is that shared filenames trip the
resolver. Confirm or refute.
"""
import sqlite3
import sys
from collections import Counter
from pathlib import Path

DB = Path(".rivets/index/tethys.db")
CRATES = {
    "rivets":       "crates/rivets/",
    "rivets-jsonl": "crates/rivets-jsonl/",
    "rivets-mcp":   "crates/rivets-mcp/",
    "tethys":       "crates/tethys/",
}
TARGET = (sys.argv[1] if len(sys.argv) > 1 else "tethys",
          sys.argv[2] if len(sys.argv) > 2 else "rivets")

def crate_of(path):
    for n, r in CRATES.items():
        if path.startswith(r):
            return n
    return None

def basename(path):
    return path.rsplit("/", 1)[-1]

conn = sqlite3.connect(DB)
rows = conn.execute("""
    SELECT f1.path, f2.path
    FROM file_deps d
    JOIN files f1 ON f1.id = d.from_file_id
    JOIN files f2 ON f2.id = d.to_file_id
""").fetchall()

pairs = [(s, t) for s, t in rows
         if crate_of(s) == TARGET[0] and crate_of(t) == TARGET[1]]

print(f"({TARGET[0]} -> {TARGET[1]}): {len(pairs)} file pairs")
print()
shared_basename = sum(1 for s, t in pairs if basename(s) == basename(t))
print(f"pairs where basename(src) == basename(tgt): {shared_basename} / {len(pairs)}")
print()
# Distribution of target basenames
tgt_basenames = Counter(basename(t) for _, t in pairs)
print("most-referenced target basenames:")
for name, n in tgt_basenames.most_common(10):
    print(f"  {n:>4}  {name}")
print()
# Sample 8 pairs
print("sample pairs:")
for s, t in pairs[:8]:
    flag = " <-- BASENAME MATCH" if basename(s) == basename(t) else ""
    print(f"  {s}")
    print(f"     -> {t}{flag}")
