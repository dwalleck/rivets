#!/usr/bin/env python3
"""Oracle for the enum-vocab probe (rivets-bkjj).

Independent mechanism: parse source text with regexes — no clap, serde, or
runtime enumeration. Extracts the same three tables the probe prints at
runtime:

  1. CLI: Arg mirror enums in crates/rivets/src/cli/types.rs — variant
     names, `#[value(...)]` name/alias attrs; canonical string from the
     domain enum's Display arm for the same variant.
  2. MCP: parse_status / parse_dep_type match arms and the McpIssueKind
     literal list in crates/rivets-mcp/src/models.rs — accepted strings,
     including upper-case and alternate-case spellings implied by the
     `to_lowercase()` / `eq_ignore_ascii_case()` calls in source.
  3. Domain: Display arms + serde rename_all / explicit renames in
     crates/rivets/src/domain/{mod,resource}.rs.

Output format matches the probe line for line. Diff the two sorted outputs.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

TYPES_RS = ROOT / "crates/rivets/src/cli/types.rs"
MOD_RS = ROOT / "crates/rivets/src/domain/mod.rs"
RESOURCE_RS = ROOT / "crates/rivets/src/domain/resource.rs"
MODELS_RS = ROOT / "crates/rivets-mcp/src/models.rs"


def kebab(name: str) -> str:
    return re.sub(r"(?<!^)(?=[A-Z])", "-", name).lower()


def snake(name: str) -> str:
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


def lower(name: str) -> str:
    return name.lower()


def mixed_case(s: str) -> str:
    return "".join(c.upper() if i % 2 == 0 else c for i, c in enumerate(s))


def title_case(s: str) -> str:
    return s[:1].upper() + s[1:]


# ---------------------------------------------------------------------------
# Domain enums: Display arms + serde strings
# ---------------------------------------------------------------------------

def extract_domain_enums() -> dict[str, dict[str, str]]:
    """name -> {variant -> display_string} for the four domain enums."""
    enums: dict[str, dict[str, str]] = {}
    for path in (MOD_RS, RESOURCE_RS):
        text = path.read_text()
        for m in re.finditer(
            r"impl fmt::Display for (\w+) \{(.*?)\n\}", text, re.DOTALL
        ):
            name, body = m.group(1), m.group(2)
            if name not in ("IssueStatus", "IssueKind", "DependencyType", "ResourceRole"):
                continue
            display_arms = dict(
                re.findall(r'Self::(\w+) => write!\(f, "([^"]+)"\)', body)
            )
            assert display_arms, f"no Display arms found for {name}"
            enums[name] = display_arms
    return enums


def domain_serde_lines(enums: dict[str, dict[str, str]]) -> list[str]:
    lines: list[str] = []
    for path in (MOD_RS, RESOURCE_RS):
        text = path.read_text()
        for m in re.finditer(
            r"pub enum (\w+) \{(.*?)\n\}", text, re.DOTALL
        ):
            name, body = m.group(1), m.group(2)
            if name not in enums:
                continue
            # rename_all sits on the attribute lines ABOVE the enum declaration.
            prefix = text[: m.start()]
            rename_all = re.search(
                r'#\[serde\(rename_all = "([a-z_-]+)"\)\]\s*pub enum$',
                prefix + "pub enum",
            )
            rule = {"snake_case": snake, "kebab-case": kebab, "lowercase": lower}[
                rename_all.group(1)
            ] if rename_all else lower
            explicit = dict(re.findall(r'#\[serde\(rename = "([^"]+)"\)\]\s+(\w+),', body))
            for variant in enums[name]:
                serde_str = explicit.get(variant, rule(variant))
                display = enums[name][variant]
                assert serde_str == display, (
                    f"{name}::{variant}: serde {serde_str!r} != display {display!r}"
                )
                lines.append(f'[domain] {name} {display} serde "{serde_str}"')
    return lines


# ---------------------------------------------------------------------------
# CLI Arg mirrors
# ---------------------------------------------------------------------------

def cli_table_lines(enums: dict[str, dict[str, str]]) -> list[str]:
    lines: list[str] = []
    text = TYPES_RS.read_text()
    for m in re.finditer(r"pub enum (\w+) \{(.*?)\n\}", text, re.DOTALL):
        arg_name, body = m.group(1), m.group(2)
        domain_name = arg_name[:-3]  # strip "Arg"
        if domain_name not in enums:
            continue
        # variant -> (value name, [aliases])
        table: dict[str, tuple[str, list[str]]] = {}
        current_attrs: list[str] = []
        for line in body.splitlines():
            attr = re.search(r"#\[value\((.*?)\)\]", line)
            if attr:
                current_attrs = [
                    a.strip() for a in attr.group(1).split(",")
                ]
                continue
            v = re.search(r"^\s{4}(\w+),?\s*$", line)
            if v:
                variant = v.group(1)
                name_attr = next((a for a in current_attrs if a.startswith("name =")), None)
                name = name_attr.split("=")[1].strip().strip('"') if name_attr else kebab(variant)
                aliases = [
                    a.split("=")[1].strip().strip('"')
                    for a in current_attrs
                    if a.startswith("alias =")
                ]
                table[variant] = (name, aliases)
                current_attrs = []
        for variant, (name, aliases) in table.items():
            canon = enums[domain_name][variant]
            lines.append(f"[cli] {domain_name} {name} -> {canon}")
            for alias in aliases:
                lines.append(f"[cli] {domain_name} alias {alias} -> {canon}")
    return lines


# ---------------------------------------------------------------------------
# MCP parse tables
# ---------------------------------------------------------------------------

def mcp_table_lines(enums: dict[str, dict[str, str]]) -> list[str]:
    lines: list[str] = []
    text = MODELS_RS.read_text()

    # parse_status / parse_dep_type: literal -> variant, plus upper-case
    # spelling implied by `s.to_lowercase()` in source.
    for fn_name, domain_name, label in (
        ("parse_status", "IssueStatus", "status"),
        ("parse_dep_type", "DependencyType", "dep_type"),
    ):
        m = re.search(rf"pub fn {fn_name}\(s: &str\) -> Option<\w+> \{{(.*?)\n\}}", text, re.DOTALL)
        assert m, f"{fn_name} not found in models.rs"
        body = m.group(1)
        assert ".to_lowercase()" in body, f"{fn_name} lost its case fold"
        for arm, _, variant in re.findall(
            r'((?:"[^"]+"(?:\s*\|\s*)?)+)\s*=>\s*Some\((\w+)::(\w+)\)', body
        ):
            for lit in re.findall(r'"([^"]+)"', arm):
                canon = enums[domain_name][variant]
                lines.append(f"[mcp] {label} {lit} -> {canon}")
                lines.append(f"[mcp] {label} {lit.upper()} -> {canon}")

    # McpIssueKind: literals from the macro invocation, alternate-case
    # spellings implied by `eq_ignore_ascii_case` in source.
    m = re.search(
        r"mcp_issue_kinds!\[(.*?)\];", text, re.DOTALL
    )
    assert m, "mcp_issue_kinds! invocation not found"
    literals = re.findall(r'"([^"]+)", (\w+)', m.group(1))
    assert "eq_ignore_ascii_case" in text, "McpIssueKind lost its case fold"
    for lit, variant in literals:
        canon = enums["IssueKind"][variant]
        for candidate in (lit, lit.upper(), title_case(lit), mixed_case(lit)):
            lines.append(f"[mcp] kind {candidate} -> {canon}")
    return lines


def main() -> None:
    enums = extract_domain_enums()
    lines = cli_table_lines(enums) + mcp_table_lines(enums) + domain_serde_lines(enums)
    for line in sorted(lines):
        print(line)


if __name__ == "__main__":
    sys.exit(main())
