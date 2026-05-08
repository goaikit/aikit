import json
import os
import tempfile

import pytest

import aikit_py


@pytest.fixture
def temp_project_root():
    with tempfile.TemporaryDirectory() as tmpdir:
        yield tmpdir


def test_normalize_mcp_agent_key():
    assert aikit_py.normalize_mcp_agent_key("cursor") == "cursor-agent"
    assert aikit_py.normalize_mcp_agent_key("vscode") == "copilot"
    assert aikit_py.normalize_mcp_agent_key("claude") == "claude"


def test_mcp_supported_agent_keys():
    keys = aikit_py.mcp_supported_agent_keys()
    assert "claude" in keys
    assert "cursor-agent" in keys
    assert len(keys) == 6


def test_mcp_supported_agents_shape():
    rows = aikit_py.mcp_supported_agents()
    assert len(rows) == 6
    for r in rows:
        assert set(r.keys()) == {
            "agent_key",
            "display_name",
            "project_config_path",
            "global_config_path",
        }


def test_mcp_config_path_claude_project(temp_project_root):
    p = aikit_py.mcp_config_path("claude", "project", temp_project_root)
    assert p == os.path.join(temp_project_root, ".mcp.json")


def test_mcp_config_path_cursor_alias(temp_project_root):
    p = aikit_py.mcp_config_path("cursor", "project", temp_project_root)
    assert p == os.path.join(temp_project_root, ".cursor/mcp.json")


def test_mcp_parse_env_pairs():
    m = aikit_py.mcp_parse_env_pairs(["A=b", "C=d=e"])
    assert m == {"A": "b", "C": "d=e"}


def test_mcp_parse_env_pairs_invalid():
    with pytest.raises(aikit_py.McpDeployError):
        aikit_py.mcp_parse_env_pairs(["NO_EQUALS"])


def test_add_mcp_server_claude_stdio(temp_project_root):
    path = aikit_py.add_mcp_server(
        "claude",
        temp_project_root,
        "demo",
        scope="project",
        command="npx",
        args=["-y", "@modelcontextprotocol/server-filesystem", temp_project_root],
        overwrite=False,
    )
    assert path == os.path.join(temp_project_root, ".mcp.json")
    with open(path, encoding="utf-8") as f:
        cfg = json.load(f)
    demo = cfg["mcpServers"]["demo"]
    assert demo["command"] == "npx"
    assert demo["args"][-1] == temp_project_root


def test_add_mcp_server_http(temp_project_root):
    aikit_py.add_mcp_server(
        "gemini",
        temp_project_root,
        "remote",
        scope="project",
        url="http://127.0.0.1:9/mcp",
        headers={"X-Test": "1"},
        overwrite=False,
    )
    p = os.path.join(temp_project_root, ".gemini", "settings.json")
    with open(p, encoding="utf-8") as f:
        cfg = json.load(f)
    r = cfg["mcpServers"]["remote"]
    assert r["url"] == "http://127.0.0.1:9/mcp"
    assert r["headers"]["X-Test"] == "1"


def test_add_mcp_server_already_exists(temp_project_root):
    aikit_py.add_mcp_server(
        "claude",
        temp_project_root,
        "x",
        command="true",
        args=[],
        overwrite=False,
    )
    with pytest.raises(aikit_py.McpDeployError):
        aikit_py.add_mcp_server(
            "claude",
            temp_project_root,
            "x",
            command="false",
            args=[],
            overwrite=False,
        )


def test_add_mcp_server_overwrite(temp_project_root):
    aikit_py.add_mcp_server(
        "claude",
        temp_project_root,
        "x",
        command="true",
        args=[],
        overwrite=False,
    )
    aikit_py.add_mcp_server(
        "claude",
        temp_project_root,
        "x",
        command="false",
        args=[],
        overwrite=True,
    )
    with open(os.path.join(temp_project_root, ".mcp.json"), encoding="utf-8") as f:
        cfg = json.load(f)
    assert cfg["mcpServers"]["x"]["command"] == "false"


def test_add_mcp_server_codex_toml(temp_project_root):
    os.makedirs(os.path.join(temp_project_root, ".codex"), exist_ok=True)
    aikit_py.add_mcp_server(
        "codex",
        temp_project_root,
        "demo",
        scope="project",
        command="npx",
        args=["-y", "pkg"],
        env={"FOO": "bar"},
        overwrite=False,
    )
    p = os.path.join(temp_project_root, ".codex", "config.toml")
    text = open(p, encoding="utf-8").read()
    assert "mcp_servers" in text
    assert "demo" in text


def test_mcp_scope_invalid(temp_project_root):
    with pytest.raises(ValueError):
        aikit_py.mcp_config_path("claude", "other", temp_project_root)


def test_add_mcp_server_transport_invalid(temp_project_root):
    with pytest.raises(ValueError):
        aikit_py.add_mcp_server("claude", temp_project_root, "n", command="a", url="b")


def test_mcp_config_unknown_agent(temp_project_root):
    with pytest.raises(aikit_py.McpDeployError):
        aikit_py.mcp_config_path("qwen", "project", temp_project_root)
