"""
Find which file-pairs disappeared from rivets-mcp -> rivets between baseline and post-slice-2.
Pre-fix had 15 edges; post-slice-2 has 11. Surface the 4 that vanished.

If those 4 file-pairs correspond to references where rivets-mcp has a same-named
symbol that took precedence, that explains the drop (and likely was correct).
If they correspond to genuine `use rivets::Foo` cases that should still produce
file_deps, the fix broke real edges and needs revising.
"""
import re
from collections import Counter

def parse_section1(path):
    cross = {}
    with open(path) as f:
        lines = f.readlines()
    in_section = False
    for line in lines:
        if line.startswith("=== Section 1:"):
            in_section = True
            continue
        if line.startswith("=== Section 2:"):
            in_section = False
            continue
        if not in_section:
            continue
        m = re.match(r"(\S+)\s+(\S+)\s+(\d+)\s*$", line.rstrip())
        if m:
            cross[(m.group(1), m.group(2))] = int(m.group(3))
    return cross

baseline = parse_section1(".rivets-0gom/baseline-pre-fix.txt")
after = parse_section1(".rivets-0gom/after-slice2-fixed.txt")

print(f"BASELINE rivets-mcp -> rivets: {baseline.get(('rivets-mcp', 'rivets'))}")
print(f"AFTER   rivets-mcp -> rivets: {after.get(('rivets-mcp', 'rivets'))}")
print()

import sqlite3
conn = sqlite3.connect(".rivets/index/tethys.db")

# Find rivets-mcp -> rivets file_deps NOW
post_pairs = set(conn.execute("""
    SELECT f1.path, f2.path FROM file_deps d
    JOIN files f1 ON f1.id = d.from_file_id
    JOIN files f2 ON f2.id = d.to_file_id
    WHERE f1.path LIKE 'crates/rivets-mcp/%' AND f2.path LIKE 'crates/rivets/%'
""").fetchall())
print(f"current rivets-mcp -> rivets file pairs: {len(post_pairs)}")

# We don't have a snapshot of pre-fix pairs, but we can look at refs to figure
# out what symbol names are involved.
# For each remaining (mcp_file, rivets_file) pair, list the symbols in rivets_file
# that mcp_file's refs point at.
print()
print("Current rivets-mcp -> rivets (target symbol distribution):")
sym_targets = conn.execute("""
    SELECT s.name, COUNT(*) FROM refs r
    JOIN files f_caller ON f_caller.id = r.file_id
    JOIN symbols s ON s.id = r.symbol_id
    JOIN files f_target ON f_target.id = s.file_id
    WHERE r.symbol_id IS NOT NULL
      AND f_caller.path LIKE 'crates/rivets-mcp/%'
      AND f_target.path LIKE 'crates/rivets/%'
    GROUP BY s.name ORDER BY COUNT(*) DESC LIMIT 20
""").fetchall()
for name, n in sym_targets:
    print(f"  {n:>4}  {name}")

# Symbols where rivets-mcp ALSO defines something of the same name
# (these are the cases slice 2's same-crate-first would now resolve internally)
print()
print("Symbol names that exist in BOTH rivets-mcp AND rivets (same-crate shadowing):")
shadowed = conn.execute("""
    SELECT s_rmcp.name FROM symbols s_rmcp
    JOIN files f_rmcp ON f_rmcp.id = s_rmcp.file_id
    WHERE f_rmcp.path LIKE 'crates/rivets-mcp/%'
      AND s_rmcp.name IN (
        SELECT s_r.name FROM symbols s_r
        JOIN files f_r ON f_r.id = s_r.file_id
        WHERE f_r.path LIKE 'crates/rivets/%'
      )
    GROUP BY s_rmcp.name LIMIT 30
""").fetchall()
for (name,) in shadowed[:30]:
    print(f"  {name}")
