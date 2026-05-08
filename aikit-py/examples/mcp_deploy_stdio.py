#!/usr/bin/env python3
"""Merge a stdio MCP server into a project-local Claude `.mcp.json` (demo)."""

from __future__ import annotations

import argparse
import json
import os
import sys


def main() -> int:
    try:
        import aikit_py
    except ImportError:
        print("Install aikit-py (e.g. maturin develop in aikit-py/)", file=sys.stderr)
        return 1

    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument(
        "--project-root",
        default=".",
        help="Project directory (default: current directory)",
    )
    p.add_argument("--server-name", default="demo-fs", help="Entry name under mcpServers")
    p.add_argument(
        "--path",
        default=None,
        help="Filesystem MCP root path (default: project root)",
    )
    p.add_argument("--overwrite", action="store_true")
    args = p.parse_args()

    root = os.path.abspath(args.project_root)
    scan = os.path.abspath(args.path) if args.path else root

    written = aikit_py.add_mcp_server(
        "claude",
        root,
        args.server_name,
        scope="project",
        command="npx",
        args=["-y", "@modelcontextprotocol/server-filesystem", scan],
        overwrite=args.overwrite,
    )
    print(written)
    with open(written, encoding="utf-8") as f:
        print(json.dumps(json.load(f), indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
