#!/usr/bin/env python3
"""Observe real CLI/MCP Issue JSON and compare it with an independent JSONL oracle."""
import json
import pathlib
import subprocess
ROOT = pathlib.Path(__file__).resolve().parents[1]
def run_cli() -> list[dict]:
    command = ["cargo", "run", "-q", "-p", "rivets", "--", "list", "--json", "-n", "50", "--sort", "oldest"]
    return json.loads(subprocess.run(command, cwd=ROOT, check=True, capture_output=True, text=True).stdout)
def persisted_records() -> dict[str, dict]:
    path = ROOT / ".rivets" / "issues.jsonl"
    return {record["id"]: record for line in path.read_text().splitlines() if line.strip() for record in [json.loads(line)]}
def select_fixture(issues: list[dict]) -> dict:
    required = ("design", "acceptance_criteria", "notes", "dependencies")
    return next(issue for issue in issues if all(issue.get(field) for field in required))
def run_mcp(issue_id: str) -> dict:
    process = subprocess.Popen(["cargo", "run", "-q", "-p", "rivets-mcp"], cwd=ROOT,
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)

    def send(message: dict) -> dict:
        process.stdin.write(json.dumps(message) + "\n")
        process.stdin.flush()
        return json.loads(process.stdout.readline())

    send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "probe", "version": "0"},
    }})
    process.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}\n')
    process.stdin.flush()
    response = send({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                     "params": {"name": "show", "arguments": {"issue_id": issue_id, "workspace_root": str(ROOT)}}})
    process.stdin.close()
    process.wait(timeout=10)
    return json.loads(response["result"]["content"][0]["text"])
shown = select_fixture(run_cli())
records = persisted_records()
oracle = records[shown["id"]]
assert shown == oracle, f"canonical CLI probe disagrees with JSONL oracle: {shown['id']}"
assert "next_resource_id" not in shown
mcp = run_mcp(shown["id"])
assert "+00:00" not in json.dumps(mcp), f"MCP retained non-canonical UTC offset: {shown['id']}"
normalized_mcp = json.loads(json.dumps(mcp).replace("+00:00", "Z"))
assert normalized_mcp == oracle, f"MCP differs beyond timestamp offset: {shown['id']}"
print(json.dumps({"id": shown["id"], "keys": sorted(shown), "notes": len(shown["notes"]), "dependencies": len(shown["dependencies"])}))
print("oracle=JSONL record; canonical_cli_matches_oracle=true; mcp_matches_after_utc_normalization=true")
