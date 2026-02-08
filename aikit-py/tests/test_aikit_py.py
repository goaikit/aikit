import pytest
import aikit_py
import tempfile
import os


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
    options = aikit_py.PyRunOptions(None, None, None)
    assert options.model is None
    assert options.yolo == False
    assert options.stream == False


def test_run_options_with_values():
    options = aikit_py.PyRunOptions("test-model", True, True)
    assert options.model == "test-model"
    assert options.yolo == True
    assert options.stream == True
