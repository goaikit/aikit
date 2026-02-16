# AI Agent Deploy Service

A Rust library crate that provides agent catalog management and deployment capabilities for AI assistant configurations.

## Overview

This crate holds the full agent catalog with per-agent capabilities (commands, skills, subagents) and provides deploy APIs that write files to the correct locations, throwing clear errors when an agent does not support a given concept.

## Features

- **Complete Agent Catalog**: All 18 supported agents including their capabilities
- **Path Resolution**: Resolves target paths per agent and concept
- **Instruction File Resolution**: Resolve project-level instruction files (CLAUDE.md, GEMINI.md, AGENTS.md) with deterministic precedence
- **File Deployment**: Creates directories and writes files for commands, skills, and subagents
- **Error Handling**: Clear error types when concepts are unsupported
- **Filename Conventions**: Proper filename formats per agent type
- **Agent Detection**: Identify which AI coding agents are installed and available
- **CLI Execution**: Run agents with full parameter support matching coder.sh
- **Output Capture**: Capture stdout/stderr for programmatic forwarding

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
| shai         | SHAI               | ✓        | ✗      | ✗         |
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

### Instruction File Resolution

Instruction files are project-root files used for agent guidance (e.g., `CLAUDE.md`, `GEMINI.md`, `AGENTS.md`).

```rust
// Direct lookup: Get the primary instruction file for an agent
let claude_md = instruction_file(project_root, "claude")?;
// Returns Some(project_root/CLAUDE.md) for claude

// Check if an agent supports instruction files
if agent_has_instruction_file("claude") {
    println!("Claude supports instruction files");
}

// Get all agents that support instruction files
let agents = instruction_file_agents();

// Auto-resolve: Find the best instruction file with fallback logic
let resolved = resolve_instruction_file(project_root, None)?;
// Scans for AGENTS.md, CLAUDE.md, GEMINI.md in order

// With specific agent
let resolved = resolve_instruction_file(project_root, Some("claude"))?;
// Returns CLAUDE.md if exists, else AGENTS.md, else CLAUDE.md path for creation

// With override path (takes precedence)
let override_path = Path::new("custom/instructions.md");
let resolved = instruction_file_with_override(project_root, Some("claude"), Some(override_path))?;
```

**Instruction File Support Table:**

| Agent Key | Name | Primary Instruction File | Fallback |
|-----------|------|-------------------------|----------|
| claude | Claude Code | CLAUDE.md | AGENTS.md |
| gemini | Google Gemini | GEMINI.md | AGENTS.md |
| cursor-agent | Cursor | AGENTS.md | - |
| codex | Codex CLI | AGENTS.md | - |
| newton | Newton | AGENTS.md | - |
| qwen | Qwen Code | AGENTS.md | - |
| opencode | opencode | AGENTS.md | - |
| windsurf | Windsurf | AGENTS.md | - |
| kilocode | Kilo Code | AGENTS.md | - |
| auggie | Auggie CLI | AGENTS.md | - |
| roo | Roo Code | AGENTS.md | - |
| codebuddy | CodeBuddy CLI | AGENTS.md | - |
| qoder | Qoder CLI | AGENTS.md | - |
| amp | Amp | AGENTS.md | - |
| shai | SHAI | AGENTS.md | - |
| q | Amazon Q Developer | AGENTS.md | - |
| bob | IBM Bob | AGENTS.md | - |
| copilot | GitHub Copilot | Not supported | - |


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

### Running agents

Only some agents have a runnable CLI; use `runnable_agents()` or `is_runnable(key)` before calling `run_agent`.

Runnable agent keys: `codex`, `claude`, `gemini`, `opencode`, `agent`.

```rust
use aikit_sdk::{run_agent, RunOptions, RunResult};

let options = RunOptions::default()
    .with_model("claude-3-opus")
    .with_yolo(true)
    .with_stream(false);

let result: Result<RunResult, _> = run_agent("claude", "Refactor this function", options);
match result {
    Ok(r) => {
        println!("stdout: {}", String::from_utf8_lossy(&r.stdout));
        println!("stderr: {}", String::from_utf8_lossy(&r.stderr));
        std::process::exit(r.exit_code().unwrap_or(1));
    }
    Err(e) => eprintln!("{}", e),
}
```

- `RunOptions`: optional `model`, `yolo`, `stream`.
- `RunResult`: `status`, `stdout`, `stderr`; `.exit_code()`, `.success()`.
- `RunError`: `AgentNotRunnable(key)`, `SpawnFailed`, `StdinFailed`, `OutputFailed`.

### Agent Detection

Check if agent CLI tools are installed and available on the system.

```rust
use aikit_sdk::*;

// Check if a specific agent is available
if is_agent_available("claude") {
    println!("Claude is installed and ready to run");
} else {
    println!("Claude is not available");
}

// Get all installed runnable agents (sorted alphabetically)
let installed = get_installed_agents();
println!("Installed agents: {:?}", installed);
// Output: ["agent", "claude", "codex", "gemini", "opencode"]

// Get detailed status for all runnable agents
let status = get_agent_status();
for (agent_key, agent_status) in status {
    println!("{}: {}",
        agent_key,
        if agent_status.available { "Available" } else { "Not available" }
    );
    if let Some(reason) = agent_status.reason {
        println!("  Reason: {}", reason);
    }
}
```

**Functions**:

- `is_agent_available(agent_key: &str) -> bool` - Check if agent binary is in PATH and responds to `--version`
- `get_installed_agents() -> Vec<String>` - List all runnable agents that are installed (sorted alphabetically)
- `get_agent_status() -> BTreeMap<String, AgentStatus>` - Get detailed availability info for all runnable agents (deterministic ordering)

**AgentAvailabilityReason**:

```rust
pub enum AgentAvailabilityReason {
    NotRunnable,        // Agent is not in runnable_agents list
    BinaryNotFound,     // Binary not found in PATH
    VersionCheckFailed, // Binary found but --version failed
    TimedOut,          // Probe timed out
}
```

**AgentStatus**:

```rust
pub struct AgentStatus {
    pub available: bool,
    pub reason: Option<AgentAvailabilityReason>,
}
```

**Detection behavior**:

- Detection is bounded by a 1500ms timeout per binary probe
- The `opencode` agent checks multiple binary candidates: `opencode` and `opencode-desktop`
- `get_agent_status()` returns a BTreeMap for deterministic ordering
- `get_installed_agents()` returns a sorted list of available agents

## Error Types

```rust
pub enum DeployError {
    AgentNotFound(String),
    UnsupportedConcept { agent_key: String, concept: DeployConcept },
    Io(std::io::Error),
}

pub enum DeployConcept {
    Command,
    Skill,
    Subagent,
    InstructionFile,
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
