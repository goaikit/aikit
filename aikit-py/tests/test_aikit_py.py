import pytest
import aikit_py
import tempfile
import os
import sys
import stat


# Fixture for a temporary directory to simulate project_root
@pytest.fixture
def temp_project_root():
    with tempfile.TemporaryDirectory() as tmpdir:
        yield tmpdir


def test_all_agents_count():
    agents = aikit_py.all_agents()
    assert len(agents) == 18  # As per aikit-sdk/src/lib.rs AGENTS const


def test_agent_validation():
    aikit_py.validate_agent_key("claude")  # Should not raise
    aikit_py.validate_agent_key("copilot")  # Should not raise
    with pytest.raises(aikit_py.DeployError) as excinfo:  # Changed to DeployError
        aikit_py.validate_agent_key("nonexistent")
    assert "Agent not found" in str(excinfo.value)  # Check message content


def test_agent_lookup():
    assert aikit_py.agent("claude") is not None
    assert aikit_py.agent("copilot") is not None
    assert aikit_py.agent("nonexistent") is None


def test_agent_fields():
    config = aikit_py.agent("claude")
    assert config.name == "Claude Code"
    assert config.commands_dir == ".claude/commands"
    assert config.skills_dir == ".claude/skills"
    assert config.agents_dir == ".claude/agents"
    assert config.key() == "claude code"  # Corrected assertion


def test_agent_fields_for_copilot():
    config = aikit_py.agent("copilot")
    assert config.name == "GitHub Copilot"
    assert config.commands_dir == ".github/agents"
    assert config.skills_dir is None
    assert config.agents_dir == ".github/agents"
    assert config.key() == "github copilot"  # Corrected assertion


def test_agent_fields_for_qwen():
    config = aikit_py.agent("qwen")
    assert config.name == "Qwen Code"
    assert config.commands_dir == ".qwen/commands"
    assert config.skills_dir is None
    assert config.agents_dir is None
    assert config.key() == "qwen code"  # Corrected assertion


def test_commands_dir(temp_project_root):
    path = aikit_py.commands_dir(temp_project_root, "claude")
    expected_path = os.path.join(temp_project_root, ".claude/commands")
    assert path == expected_path


def test_skill_dir_unsupported(temp_project_root):
    with pytest.raises(aikit_py.DeployError) as excinfo:  # Changed to DeployError
        aikit_py.skill_dir(temp_project_root, "qwen", "my-skill")
    assert "skill" in str(excinfo.value)  # Check for "skill" in message


def test_subagent_path_copilot(temp_project_root):
    path = aikit_py.subagent_path(temp_project_root, "copilot", "my-agent")
    expected_path = os.path.join(temp_project_root, ".github/agents/my-agent.agent.md")
    assert path == expected_path


def test_subagent_filename_convention():
    assert aikit_py.subagent_filename("claude", "test") == "test.md"
    assert aikit_py.subagent_filename("copilot", "test") == "test.agent.md"
    assert aikit_py.subagent_filename("cursor-agent", "test") == "test.md"


def test_subagent_path_unsupported(temp_project_root):
    with pytest.raises(aikit_py.DeployError) as excinfo:  # Changed to DeployError
        aikit_py.subagent_path(temp_project_root, "qwen", "my-agent")
    assert "subagent" in str(excinfo.value)  # Check for "subagent" in message


def test_deploy_command(temp_project_root):
    content = """# My Command
Hello World"""
    path = aikit_py.deploy_command("claude", temp_project_root, "test-command", content)
    expected_path = os.path.join(temp_project_root, ".claude/commands/test-command.md")
    assert path == expected_path
    with open(path, "r") as f:
        assert f.read() == content


def test_deploy_skill(temp_project_root):
    skill_md = """# Skill Name

Description here."""
    scripts = [
        ("setup.sh", b"#!/bin/sh\necho 'setup'"),
        ("cleanup.sh", b"#!/bin/sh\necho 'cleanup'"),
    ]

    path = aikit_py.deploy_skill(
        "cursor-agent",
        temp_project_root,
        "my-skill",
        skill_md,
        scripts,
    )
    expected_skill_md_path = os.path.join(
        temp_project_root, ".cursor/skills/my-skill/SKILL.md"
    )
    assert path == expected_skill_md_path
    assert os.path.exists(
        os.path.join(temp_project_root, ".cursor/skills/my-skill/scripts/setup.sh")
    )
    assert os.path.exists(
        os.path.join(temp_project_root, ".cursor/skills/my-skill/scripts/cleanup.sh")
    )

    with open(path, "r") as f:
        assert f.read() == skill_md

    with open(
        os.path.join(temp_project_root, ".cursor/skills/my-skill/scripts/setup.sh"),
        "rb",
    ) as f:
        assert f.read() == b"#!/bin/sh\necho 'setup'"

    with open(
        os.path.join(temp_project_root, ".cursor/skills/my-skill/scripts/cleanup.sh"),
        "rb",
    ) as f:
        assert f.read() == b"#!/bin/sh\necho 'cleanup'"


def test_deploy_skill_unsupported_agent(temp_project_root):
    with pytest.raises(aikit_py.DeployError) as excinfo:  # Changed to DeployError
        aikit_py.deploy_skill("qwen", temp_project_root, "my-skill", "# skill", None)
    assert "skill" in str(excinfo.value)  # Check for "skill" in message


def test_deploy_subagent(temp_project_root):
    content = """# My Subagent
Config here."""
    path = aikit_py.deploy_subagent("claude", temp_project_root, "my-agent", content)
    expected_path = os.path.join(temp_project_root, ".claude/agents/my-agent.md")
    assert path == expected_path
    with open(path, "r") as f:
        assert f.read() == content


def test_command_filename_convention():
    assert aikit_py.command_filename("claude", "test") == "test.md"
    assert aikit_py.command_filename("codex", "test") == "test.prompt"
    assert aikit_py.command_filename("qwen", "test") == "test.cmd"
    assert aikit_py.command_filename("roo", "test") == "test.command"


def test_runnable_agents_list():
    agents = aikit_py.runnable_agents_list()
    assert "codex" in agents
    assert "claude" in agents
    assert "gemini" in agents
    assert "opencode" in agents
    assert "agent" in agents
    assert len(agents) == 5


def test_is_runnable_py():
    assert aikit_py.is_runnable_py("codex") == True
    assert aikit_py.is_runnable_py("claude") == True
    assert aikit_py.is_runnable_py("gemini") == True
    assert aikit_py.is_runnable_py("opencode") == True
    assert aikit_py.is_runnable_py("agent") == True
    assert aikit_py.is_runnable_py("copilot") == False
    assert aikit_py.is_runnable_py("unknown") == False


def test_run_agent_not_runnable():
    with pytest.raises(Exception) as excinfo:
        aikit_py.run_agent("unknown_agent", "test", None, False, False)
    assert "not runnable" in str(excinfo.value)


def test_run_options_defaults():
    options = aikit_py.PyRunOptions()
    assert options.model is None
    assert options.yolo == False
    assert options.stream == False


def test_run_options_with_values():
    options = aikit_py.PyRunOptions("test-model", True, True)
    assert options.model == "test-model"
    assert options.yolo == True
    assert options.stream == True


def test_is_agent_available_returns_bool():
    assert isinstance(aikit_py.is_agent_available("claude"), bool)
    assert isinstance(aikit_py.is_agent_available("codex"), bool)
    assert isinstance(aikit_py.is_agent_available("unknown"), bool)


def test_is_agent_available_false_for_non_runnable():
    assert aikit_py.is_agent_available("copilot") == False
    assert aikit_py.is_agent_available("cursor-agent") == False
    assert aikit_py.is_agent_available("unknown") == False


def test_is_agent_available_py_matches_canonical():
    assert aikit_py.is_agent_available("claude") == aikit_py.is_agent_available_py(
        "claude"
    )
    assert aikit_py.is_agent_available("codex") == aikit_py.is_agent_available_py(
        "codex"
    )
    assert aikit_py.is_agent_available("unknown") == aikit_py.is_agent_available_py(
        "unknown"
    )


def test_get_installed_agents_returns_list():
    installed = aikit_py.get_installed_agents()
    assert isinstance(installed, list)
    assert all(isinstance(item, str) for item in installed)


def test_get_installed_agents_is_subset_of_runnable():
    installed = set(aikit_py.get_installed_agents())
    runnable = set(aikit_py.runnable_agents_list())
    assert installed.issubset(runnable)


def test_get_installed_agents_sorted():
    installed = aikit_py.get_installed_agents()
    assert installed == sorted(installed)


def test_get_installed_agents_py_matches_canonical():
    assert aikit_py.get_installed_agents() == aikit_py.get_installed_agents_py()


def test_get_agent_status_returns_dict():
    status = aikit_py.get_agent_status()
    assert isinstance(status, dict)


def test_get_agent_status_keys_are_strings():
    status = aikit_py.get_agent_status()
    assert all(isinstance(key, str) for key in status.keys())


def test_get_agent_status_values_have_available_and_reason():
    status = aikit_py.get_agent_status()
    for agent_key, agent_status in status.items():
        assert isinstance(agent_status, dict)
        assert "available" in agent_status
        assert "reason" in agent_status
        assert isinstance(agent_status["available"], bool)
        assert agent_status["reason"] is None or isinstance(agent_status["reason"], str)


def test_get_agent_status_unavailable_has_reason():
    status = aikit_py.get_agent_status()
    for agent_key, agent_status in status.items():
        if not agent_status["available"]:
            assert agent_status["reason"] is not None
            assert isinstance(agent_status["reason"], str)


def test_get_agent_status_py_matches_canonical():
    assert aikit_py.get_agent_status() == aikit_py.get_agent_status_py()


# ---------------------------------------------------------------------------
# Streaming events tests
# ---------------------------------------------------------------------------

def write_stub(directory, name, body):
    """Write an executable shell script stub."""
    path = os.path.join(directory, name)
    with open(path, "w") as f:
        f.write("#!/bin/sh\n" + body + "\n")
    os.chmod(path, stat.S_IRWXU | stat.S_IRGRP | stat.S_IXGRP | stat.S_IROTH | stat.S_IXOTH)
    return path


def with_stub_path(directory, fn):
    """Run fn with directory prepended to PATH, restore afterwards."""
    orig = os.environ.get("PATH", "")
    os.environ["PATH"] = directory + ":" + orig
    try:
        return fn()
    finally:
        os.environ["PATH"] = orig


@pytest.mark.skipif(sys.platform == "win32", reason="Shell script stubs not supported on Windows")
def test_run_agent_events_not_runnable():
    with pytest.raises(Exception) as excinfo:
        aikit_py.run_agent_events_py("unknown_agent", "test", lambda e: None)
    assert "not runnable" in str(excinfo.value)


@pytest.mark.skipif(sys.platform == "win32", reason="Shell script stubs not supported on Windows")
@pytest.mark.parametrize("agent_key,body,expected_count", [
    (
        "codex",
        r"""printf '{"type":"message","role":"system","content":"Codex session started"}\n'
printf '{"type":"message","role":"assistant","content":"Processing..."}\n'
printf '{"type":"message","role":"assistant","content":"Done."}\n'""",
        3,
    ),
    (
        "claude",
        r"""printf '{"type":"system","subtype":"init","session_id":"stub001"}\n'
printf '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Stub response."}]}}\n'
printf '{"type":"result","subtype":"success","result":"OK"}\n'""",
        3,
    ),
    (
        "gemini",
        r"""printf '{"candidates":[{"content":{"parts":[{"text":"Stub response."}],"role":"model"}}]}\n'
printf '{"candidates":[{"content":{"parts":[{"text":"Done."}],"role":"model"}}]}\n'""",
        2,
    ),
    (
        "opencode",
        r"""printf '{"type":"start","agent":"opencode"}\n'
printf '{"type":"message","role":"assistant","content":"Stub response."}\n'
printf '{"type":"end","exit_code":0}\n'""",
        3,
    ),
    (
        "agent",
        r"""printf '{"event":"start","agent":"agent"}\n'
printf '{"event":"message","role":"assistant","text":"Stub response."}\n'
printf '{"event":"end","status":"success"}\n'""",
        3,
    ),
])
def test_run_agent_events_all_agents(agent_key, body, expected_count):
    with tempfile.TemporaryDirectory() as tmpdir:
        write_stub(tmpdir, agent_key, body)
        events = []

        def on_event(event):
            events.append(event)

        result = with_stub_path(tmpdir, lambda: aikit_py.run_agent_events_py(
            agent_key, "test prompt", on_event
        ))

        assert result is not None
        assert "status_code" in result
        assert "stdout" in result
        assert "stderr" in result
        assert len(events) == expected_count

        for i, event in enumerate(events):
            assert event["agent_key"] == agent_key
            assert event["seq"] == i
            assert event["stream"] in ("stdout", "stderr")
            assert "payload" in event


@pytest.mark.skipif(sys.platform == "win32", reason="Shell script stubs not supported on Windows")
def test_run_agent_events_callback_raises():
    with tempfile.TemporaryDirectory() as tmpdir:
        write_stub(
            tmpdir,
            "codex",
            r"""printf '{"type":"message","content":"line1"}\n'
printf '{"type":"message","content":"line2"}\n'
printf '{"type":"message","content":"line3"}\n'""",
        )

        invocation_count = [0]

        def on_event(event):
            invocation_count[0] += 1
            raise ValueError("callback error")

        with pytest.raises(ValueError, match="callback error"):
            with_stub_path(tmpdir, lambda: aikit_py.run_agent_events_py(
                "codex", "test prompt", on_event
            ))

        # Callback should have been invoked exactly once (stops after first exception)
        assert invocation_count[0] == 1
