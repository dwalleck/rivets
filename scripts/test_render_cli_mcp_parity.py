#!/usr/bin/env python3
"""Regression tests for the CLI/MCP parity Markdown renderer."""

from __future__ import annotations

import copy
import importlib.util
import json
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = ROOT / "scripts" / "render-cli-mcp-parity.py"
REGISTRY_PATH = ROOT / "docs" / "cli-mcp-parity.json"

sys.dont_write_bytecode = True

SPEC = importlib.util.spec_from_file_location("render_cli_mcp_parity", SCRIPT_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"Cannot load renderer from {SCRIPT_PATH}")
RENDERER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RENDERER)


class RenderCliMcpParityTests(unittest.TestCase):
    """Exercise renderer behavior not represented by today's registry values."""

    def test_table_control_characters_are_escaped(self) -> None:
        registry = json.loads(REGISTRY_PATH.read_text(encoding="utf-8"))
        probe = copy.deepcopy(registry)
        probe["operations"][0]["cli"]["surfaces"][0] = "bad|surface\nline"
        probe["delivery_groups"][0]["intents"][0] = "bad|intent\nline"
        probe["delivery_groups"][0]["blocked_by"] = ["bad|issue\nline"]

        rendered = RENDERER.render(probe)

        self.assertIn("`bad\\|surface<br>line`", rendered)
        self.assertIn("`bad\\|intent<br>line`", rendered)
        self.assertIn("`bad\\|issue<br>line`", rendered)
        self.assertNotIn("`bad|surface\nline`", rendered)


if __name__ == "__main__":
    unittest.main()
