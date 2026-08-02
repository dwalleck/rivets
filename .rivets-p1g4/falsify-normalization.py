#!/usr/bin/env python3
"""Cheapest falsifier for rivets-p1g4 claim C1/C2: the WorkspacePath
normalization rule agrees with realpath -m on a random corpus (not the
hand-picked probe cases)."""
import os
import random
import subprocess
import sys

ROOT = "/home/dwalleck/repos/rivets"
random.seed(20260802)

COMPONENTS = [
    "src", "lib.rs", "docs", "a b", "é", "文件.md", ".hidden", "x.y.z",
    "a-b_c", "deep", "..", ".", "", "dir:", "C:", "with space",
]

def normalize(raw: str) -> str | None:
    if raw == "" or raw.isspace():
        return None
    if any(ord(c) < 32 or ord(c) == 127 for c in raw):
        return None
    if raw.startswith("/"):
        return None
    stack: list[str] = []
    for comp in raw.split("/"):
        if comp in ("", "."):
            continue
        if comp == "..":
            if stack:
                stack.pop()
            else:
                return None
        else:
            stack.append(comp)
    return "/".join(stack) if stack else None

def oracle(raw: str) -> str | None:
    out = subprocess.run(
        ["realpath", "-m", os.path.join(ROOT, raw)],
        capture_output=True, text=True, check=True,
    ).stdout.strip()
    if out == ROOT or not out.startswith(ROOT + "/"):
        return None
    return out[len(ROOT) + 1:]

cases = set()
for _ in range(400):
    n = random.randint(1, 5)
    cases.add("/".join(random.choice(COMPONENTS) for _ in range(n)))
# Prefix variants that stress the absolute/escape boundary.
for _ in range(100):
    cases.add("/" + random.choice(COMPONENTS))
for _ in range(100):
    cases.add("../" + "/".join(random.choice(COMPONENTS) for _ in range(3)))

fails = 0
for case in sorted(cases):
    got, want = normalize(case), oracle(case)
    if got != want:
        fails += 1
        print(f"FAIL {case!r}: probe={got!r} oracle={want!r}")
print(f"falsifier: {len(cases) - fails}/{len(cases)} random cases agree with realpath -m")
sys.exit(1 if fails else 0)
