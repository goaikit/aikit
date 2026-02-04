# AI Agent Deploy Service

A Rust library crate that provides agent catalog management and deployment capabilities for AI assistant configurations.

## Overview

This crate holds the full agent catalog with per-agent capabilities (commands, skills, subagents) and provides deploy APIs that write files to the correct locations, throwing clear errors when an agent does not support a given concept.

## Features

- **Complete Agent Catalog**: All 17 supported agents including their capabilities
- **Path Resolution**: Resolves target paths per agent and concept
- **File Deployment**: Creates directories and writes files for commands, skills, and subagents
- **Error Handling**: Clear error types when concepts are unsupported
- **Filename Conventions**: Proper filename formats per agent type

## Agents Supported

| Agent Key    | Name               | Commands | Skills | Subagents |
| ------------ | ------------------ | -------- | ------ | --------- |
| claude       | Claude Code        | ✓        | ✓      | ✓         |
| gemini       | Google Gemini      | ✓        | ✓      | ✓         |
| copilot      | GitHub Copilot     | ✓        | ✗      | ✓         |
| cursor-agent | Cursor             | ✓        | ✓      | ✓         |
| qwen         | Qwen Code          | ✓        | ✗      | ✗         |
| opencode     | opencode           | ✓        | ✗      | ✗         |
| codex        | Codex CLI          | ✓        | ✓      | ✗         |
| windsurf     | Windsurf           | ✓        | ✓      | ✗         |
| kilocode     | Kilo Code          | ✓        | ✓      | ✗         |
| auggie       | Auggie CLI         | ✓        | ✓      | ✓         |
| roo          | Roo Code           | ✓        | ✓      | ✗         |
| codebuddy    | CodeBuddy CLI      | ✓        | ✗      | ✗         |
| qoder        | Qoder CLI          | ✓        | ✗      | ✓         |
| amp          | Amp                | ✓        | ✗      | ✗         |
| shai         | SHAI               | ✓        | ✗      ✗         |
| q            | Amazon Q Developer | ✓        | ✗      | ✗         |
| bob          | IBM Bob            | ✓        | ✗      | ✗         |

## Public API

### Catalog

```rust
use aikit_sdk::*;

// Get all agents
let agents = all_agents();

// Get a specific agent
let claude = agent("claude");

// Validate an agent key
validate_agent_key("claude")?;
```

### Path Resolution

```rust
// Command directory (all agents support this)
let commands_dir = commands_dir(project_root, "claude")?;

// Skill directory (only if agent supports skills)
let skill_dir = skill_dir(project_root, "claude", "my-skill")?;

// Subagent path (only if agent supports subagents)
let subagent_path = subagent_path(project_root, "claude", "my-agent")?;
```

### Deployment

```rust
// Deploy a command
let path = deploy_command("claude", project_root, "my-command", "# Content")?;

// Deploy a skill with optional scripts
let path = deploy_skill(
    "cursor-agent",
    project_root,
    "my-skill",
    "# Skill content",
    Some(&[("script.sh", b"#!/bin/sh")])
)?;

// Deploy a subagent
let path = deploy_subagent("claude", project_root, "my-agent", "# Agent content")?;
```

## Error Types

```rust
pub enum DeployError {
    AgentNotFound(String),
    UnsupportedConcept { agent_key: String, concept: DeployConcept },
    Io(std::io::Error),
}
```

## Error Handling

The crate returns `Result` types with `aikit_sdk::DeployError` for:

- Agent key not found in catalog
- Attempting to deploy to an agent that doesn't support the concept
- Filesystem operations failing

## Dependencies

Only `std` is required. No external dependencies.

## License

Apache-2.0
