# aikit-py: Python Bindings for aikit SDK

Python bindings for the `aikit-sdk` providing agent catalog and deployment functionalities from Python. This library allows you to programmatically interact with aikit agents, list available agents, and deploy commands, skills, and subagents to your project.

## Requirements

- Python 3.9+
- Rust toolchain (for building from source or development)

## Installation

You can install `aikit-py` using `pip`, `poetry`, or `uv`.

### pip

```bash
# It's recommended to use a virtual environment
python -m venv .venv
source .venv/bin/activate
pip install aikit-py
```

### poetry

```bash
# Add to an existing project
poetry add aikit-py

# Or create a new project and add it
poetry new my-aikit-project
cd my-aikit-project
poetry add aikit-py
```

### uv

```bash
# In your project directory
uv add aikit-py

# Global installation (use with caution)
uv pip install aikit-py
```

## Quick Start

Here's a quick example to get started with `aikit-py`:

```python
import aikit_py
import tempfile
import os

# List all available agents
print("Available Agents:")
for agent_config in aikit_py.all_agents():
    print(f"- {agent_config.name} (Key: {agent_config.key()})")

# Get a specific agent's configuration
claude_config = aikit_py.agent("claude")
if claude_config:
    print(f"
Claude Agent Commands Dir: {claude_config.commands_dir}")

# Validate an agent key
try:
    aikit_py.validate_agent_key("claude")
    print("
'claude' is a valid agent key.")
except aikit_py.PyDeployError as e:
    print(f"
Error: {e.message}")

# Deploy a command
with tempfile.TemporaryDirectory() as project_root:
    command_content = "# My Test Command
print('Hello from aikit_py!')"
    command_path = aikit_py.deploy_command("claude", project_root, "my-python-command", command_content)
    print(f"
Deployed command to: {command_path}")
    # Verify content
    with open(command_path, "r") as f:
        print(f"Command content:
{f.read()}")

```

## Adding a new skill

You can programmatically add new skills to agents using `deploy_skill`. This function creates the skill's directory structure and writes the `SKILL.md` content, along with any optional scripts.

```python
import aikit_py
import tempfile
import os

with tempfile.TemporaryDirectory() as project_root:
    agent_key = "cursor-agent" # An agent that supports skills
    skill_name = "my-new-python-skill"
    skill_md_content = """---
name: My New Python Skill
description: A skill deployed via aikit-py.
license: Apache-2.0
---
This is the content of my new skill, written from Python.
"""
    # Optional scripts to be included with the skill
    optional_scripts = [
        ("setup.sh", b"#!/bin/sh
echo 'Running skill setup'"),
        ("run.py", b"#!/usr/bin/env python
print('Skill executed!')")
    ]

    try:
        skill_path = aikit_py.deploy_skill(
            agent_key,
            project_root,
            skill_name,
            skill_md_content,
            optional_scripts
        )
        print(f"Skill '{skill_name}' deployed successfully to: {skill_path}")

        # Verify files were created
        expected_skill_md_path = os.path.join(project_root, ".cursor/skills/my-new-python-skill/SKILL.md")
        assert os.path.exists(expected_skill_md_path)
        print(f"SKILL.md exists at: {expected_skill_md_path}")

        expected_setup_script = os.path.join(project_root, ".cursor/skills/my-new-python-skill/scripts/setup.sh")
        assert os.path.exists(expected_setup_script)
        print(f"Setup script exists at: {expected_setup_script}")

        expected_run_script = os.path.join(project_root, ".cursor/skills/my-new-python-skill/scripts/run.py")
        assert os.path.exists(expected_run_script)
        print(f"Run script exists at: {expected_run_script}")

    except aikit_py.PyDeployError as e:
        print(f"Failed to deploy skill: {e.message} (Kind: {e.kind})")

    # Example of trying to deploy a skill for an agent that doesn't support them
    try:
        aikit_py.deploy_skill("qwen", project_root, "unsupported-skill", "# Unsupported", None)
    except aikit_py.PyDeployError as e:
        print(f"
Expected error when deploying skill to 'qwen': {e.message} (Kind: {e.kind})")
```

## Running agents

You can run a coding agent from Python. Only these agent keys are runnable: `codex`, `claude`, `gemini`, `opencode`, `agent`. Use `aikit_py.is_runnable_py(agent_key)` or `aikit_py.runnable_agents_list()` to check.

```python
import aikit_py

# Optional: check before calling
if aikit_py.is_runnable_py("claude"):
    result = aikit_py.run_agent("claude", "Suggest a refactor", model=None, yolo=False, stream=False)
    # result is a dict: status_code (int | None), stdout (bytes), stderr (bytes)
    print(result["stdout"].decode())
    exit(result["status_code"] or 1)
```

Raises an exception if the agent is not runnable or the process fails to start.

## Agent Detection

### Check if Agent is Installed

```python
import aikit_py

# Check if a specific agent is available
if aikit_py.is_agent_available("claude"):
    print("Claude is installed and ready to run")
else:
    print("Claude is not available")

# Compatibility alias (same behavior)
if aikit_py.is_agent_available_py("claude"):
    print("Claude is installed and ready to run")
```

### List Installed Agents

```python
import aikit_py

# Get all installed runnable agents (sorted alphabetically)
installed = aikit_py.get_installed_agents()
print(f"Installed agents: {installed}")
# Output: ['agent', 'claude', 'codex', 'gemini', 'opencode']

# Compatibility alias (same behavior)
installed_py = aikit_py.get_installed_agents_py()
```

### Get Detailed Agent Status

```python
import aikit_py

# Get status for all runnable agents
status = aikit_py.get_agent_status()
for agent_key, agent_status in status.items():
    print(f"{agent_key}: {'Available' if agent_status['available'] else 'Not available'}")
    if agent_status['reason']:
        print(f"  Reason: {agent_status['reason']}")

# Compatibility alias (same behavior)
status_py = aikit_py.get_agent_status_py()
```

### Agent Status Structure

Agent status is returned as a dictionary with the following structure:

```python
{
    "agent_key": {
        "available": bool,       # Whether the agent is installed and runnable
        "reason": Optional[str]  # Optional explanation if not available
    }
}
```

The `reason` field contains one of the following values when unavailable:
- `"not_runnable"` - Agent is not in runnable_agents list
- `"binary_not_found"` - Binary not found in PATH
- `"version_check_failed"` - Binary found but --version failed
- `"timed_out"` - Probe timed out (1500ms timeout)

**Detection behavior**:
- Detection is bounded by a 1500ms timeout per binary probe
- The `opencode` agent checks multiple binary candidates: `opencode` and `opencode-desktop`
- Status is returned in deterministic order (sorted by agent key)
- Only runnable agents are included in the status map

## API Overview

The `aikit_py` module exposes functions and classes that mirror the `aikit-sdk` Rust library:

-   `all_agents()`: Returns a list of `AgentConfig` objects for all known agents.
-   `agent(key: str)`: Returns an `AgentConfig` object for the specified agent key, or `None` if not found.
-   `validate_agent_key(key: str)`: Validates if an agent key exists, raises `PyDeployError` if not.
-   `commands_dir(project_root: str, agent_key: str)`: Returns the path to an agent's commands directory.
-   `skill_dir(project_root: str, agent_key: str, skill_name: str)`: Returns the path to a specific skill directory.
-   `subagent_path(project_root: str, agent_key: str, name: str)`: Returns the path to a subagent file.
-   `deploy_command(agent_key: str, project_root: str, name: str, content: str)`: Deploys a command file.
-   `deploy_skill(agent_key: str, project_root: str, skill_name: str, skill_md_content: str, optional_scripts: Optional[List[Tuple[str, bytes]]])`: Deploys a skill, including `SKILL.md` and optional script files.
-   `deploy_subagent(agent_key: str, project_root: str, name: str, content: str)`: Deploys a subagent file.
-   `command_filename(agent_key: str, name: str)`: Returns the conventional filename for a command.
-   `subagent_filename(agent_key: str, name: str)`: Returns the conventional filename for a subagent.
-   `run_agent(agent_key, prompt, model=None, yolo=False, stream=False)`: Runs the agent CLI; returns a dict with `status_code`, `stdout`, `stderr`. Raises on invalid agent or spawn failure.
-   `runnable_agents_list()`: Returns list of runnable agent keys (`codex`, `claude`, `gemini`, `opencode`, `agent`).
-   `is_runnable_py(agent_key: str)`: Returns whether the agent can be run via `run_agent`.
-   `PyRunOptions`: Optional builder for run options (model, yolo, stream); used internally by `run_agent`.
-   `AgentConfig`: A Python class representing an agent's configuration, with properties like `name`, `commands_dir`, `skills_dir`, `agents_dir`, and `key()`.
-   `PyDeployError`: A custom Python exception class for errors originating from the `aikit-sdk`.
-   `PyDeployConcept`: A Python enum mirroring `DeployConcept` (Command, Skill, Subagent).

## License

This project is licensed under the Apache-2.0 License.
