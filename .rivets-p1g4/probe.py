#!/usr/bin/env python3
"""Probe for rivets-p1g4: Workspace Path normalization rule + persistence substrate.

Three slices, each with an independent oracle:
  A. normalize_workspace_path() rule vs coreutils `realpath -m` (lexical, no FS).
  B. Real CLI resource add/list round-trip across process restarts vs raw JSONL parse.
  C. Simulated remove/update via JSONL edit: id/order stability + sequence
     continuation after reload vs hand-count of the edited file.
"""
import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

RIVETS = "/home/dwalleck/repos/rivets/target/debug/rivets"
ROOT = "/home/dwalleck/repos/rivets"  # workspace root (contains .rivets/)

# ---------------------------------------------------------------- Section A
def normalize_workspace_path(raw: str) -> str | None:
    """Proposed domain rule: lexical normalization, reject escape/absolute/empty."""
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
                return None  # escapes workspace root
        else:
            stack.append(comp)
    return "/".join(stack) if stack else None


def oracle_a(raw: str) -> str | None:
    """realpath -m: purely lexical canonicalization against the workspace root."""
    out = subprocess.run(
        ["realpath", "-m", os.path.join(ROOT, raw)],
        capture_output=True, text=True, check=True,
    ).stdout.strip()
    if out == ROOT or not out.startswith(ROOT + "/"):
        return None  # stays at root or escapes
    return out[len(ROOT) + 1:]


CASES = [
    "docs/adr/0003.md", "src/lib.rs", "a/./b", "a//b", "./x", "x/.",
    "docs/../src/lib.rs", "a/b/../../c", "../escape.md", "a/../../b",
    "/etc/passwd", "", "a/..", ".", "a/../..", "x/./../y", "é/文件.md",
    "with space/y",
]
# Policy-only rejections realpath cannot express (legal filenames, but the
# domain rejects them like WebUrl/ResourceLabel/Note reject control chars).
POLICY_CASES = ["   ", "un\tdir/x"]
fails = 0
for case in CASES:
    got, want = normalize_workspace_path(case), oracle_a(case)
    status = "OK " if got == want else "FAIL"
    if got != want:
        fails += 1
    print(f"{status} normalize({case!r}) -> probe={got!r} oracle={want!r}")
for case in POLICY_CASES:
    got = normalize_workspace_path(case)
    print(f"OK  policy-reject({case!r}) -> probe={got!r} "
          f"(oracle: realpath accepts it as a filename; rejection is domain policy)")
    if got is not None:
        fails += 1
print(f"Section A: {len(CASES) + len(POLICY_CASES) - fails}/{len(CASES) + len(POLICY_CASES)} agree")
if fails:
    sys.exit(1)

# ---------------------------------------------------------------- Section B
with tempfile.TemporaryDirectory(prefix="rivets-probe-") as td:
    ws = Path(td) / "ws"
    ws.mkdir()
    subprocess.run([RIVETS, "init", "--prefix", "probe"], cwd=ws, check=True,
                   capture_output=True)
    created = subprocess.run([RIVETS, "create", "--title", "Probe issue", "-y", "--json"],
                             cwd=ws, check=True, capture_output=True, text=True)
    issue_id = json.loads(created.stdout)["id"]
    for url, role, label in [
        ("https://example.com/a", "reference", "first"),
        ("https://example.com/b", "evidence", None),
        ("https://example.com/a/", "documentation", "second"),
    ]:
        argv = [RIVETS, "resource", "add", issue_id, "--url", url, "--role", role]
        if label:
            argv += ["--label", label]
        subprocess.run(argv, cwd=ws, check=True, capture_output=True)

    def list_json() -> list[dict]:
        out = subprocess.run([RIVETS, "resource", "list", issue_id, "--json"],
                             cwd=ws, check=True, capture_output=True, text=True)
        return json.loads(out.stdout)

    first = list_json()  # process A
    second = list_json()  # process B (fresh process, reloaded from disk)
    assert first == second, f"restart changed resources: {first} != {second}"

    # Oracle: parse the raw JSONL with the stdlib (not rivets).
    record = next(
        json.loads(line) for line in (ws / ".rivets/issues.jsonl").read_text().splitlines()
        if json.loads(line)["id"] == issue_id
    )
    file_resources = record["resources"]
    assert [r["id"] for r in file_resources] == [r["id"] for r in first], (
        "CLI order != file order")
    assert [r["target"] for r in file_resources] == [r["target"] for r in first]
    assert file_resources[0]["target"] == {"type": "web", "url": "https://example.com/a"}
    print("Section B: CLI round-trip matches raw JSONL oracle; ids/order stable:",
          [r["id"] for r in first])

    # ---------------------------------------------------------------- Section C
    # Simulate a future `remove r2` + `update r1 role/label` by editing the file.
    file_resources[1], file_resources[0]["role"] = file_resources[2], "successor"
    file_resources[0]["label"] = "updated label"
    file_resources = [file_resources[0], file_resources[1]]  # drop the removed one
    record["resources"] = file_resources
    lines = [
        json.dumps(json.loads(line), ensure_ascii=False)
        for line in (ws / ".rivets/issues.jsonl").read_text().splitlines()
        if json.loads(line)["id"] != issue_id
    ]
    lines.append(json.dumps(record))
    (ws / ".rivets/issues.jsonl").write_text("\n".join(lines) + "\n")

    after = list_json()
    got_ids = [r["id"] for r in after]
    got_roles = [r["role"] for r in after]
    got_labels = [r["label"] for r in after]
    # Oracle: hand-count from the edited file (ids r1,r3; r1 successor/updated; r3 documentation).
    assert got_ids == ["r1", "r3"], f"ids reidentified after removal: {got_ids}"
    assert got_roles == ["successor", "documentation"], got_roles
    assert got_labels == ["updated label", "second"], got_labels
    print("Section C: after simulated remove+update, remaining ids/order stable:",
          list(zip(got_ids, got_roles)))

    # Sequence continuation: next add must be r4, not r2.
    subprocess.run(
        [RIVETS, "resource", "add", issue_id, "--url", "https://example.com/new",
         "--role", "reference"],
        cwd=ws, check=True, capture_output=True)
    new_ids = [r["id"] for r in list_json()]
    assert new_ids == ["r1", "r3", "r4"], f"sequence did not continue: {new_ids}"
    print("Section C2: next add got", new_ids[-1], "(oracle: r4 = max+1, never reused)")

print("\nPROBE PASSED: all slices agree with their oracles")
