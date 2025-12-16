# Implementation Plan: AIKIT - Rust Spec Kit CLI Complete Reimplementation

**Branch**: `002-rust-spec-kit-complete` | **Date**: 2025-01-27 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/002-rust-spec-kit-complete/spec.md`

**Note**: This template is filled in by the `/speckit.plan` command. See `.specify/templates/commands/plan.md` for the execution workflow.

## Summary

Complete Rust reimplementation of the GitHub Spec Kit CLI tool (`specify`), distributed as **AIKIT** (binary: `aikit`), authored by @aroff. The implementation provides 100% functional parity with the Python version, including project initialization with template downloading/extraction, agent configuration management, tool checking, version reporting, cross-platform script support, and the complete release packaging pipeline. The core goal is to leverage Rust's performance, type safety, and the cli-framework (TUI framework) for enhanced interactive terminal experiences while maintaining exact behavioral compatibility with the Python implementation.

**Technical Approach**: Rust-based CLI application using `cli-framework` for interactive TUI components, `clap` for argument parsing, `reqwest` for GitHub API interactions, `zip` for archive handling, `serde_json` for JSON processing, and `git2` for Git operations. The application supports both interactive TUI mode (using cli-framework) and non-interactive CLI mode, with identical output formatting to the Python version.

## Technical Context

**Language/Version**: Rust (latest stable, minimum 1.75+)  
**Primary Dependencies**: 
- `cli-framework` (TUI framework for interactive terminal UI)
- `clap` (command-line argument parsing)
- `reqwest` (HTTP client for GitHub API interactions)
- `zip` (ZIP archive extraction)
- `serde` + `serde_json` (JSON serialization/deserialization)
- `git2` (Git operations)
- `anyhow` (error handling)
- `tokio` (async runtime, required by cli-framework)
- `ratatui` + `crossterm` (TUI rendering, via cli-framework)
- `walkdir` (directory traversal)
- `toml` (TOML parsing for agent configs)

**Storage**: N/A (file system operations only; no database)

**Testing**: `cargo test` with unit tests, integration tests, and contract tests. Testing strategy includes:
- Unit tests for core logic (template extraction, file merging, agent config parsing)
- Integration tests for CLI commands (init, check, version, package)
- Mock HTTP responses for GitHub API interactions
- Cross-platform testing (Linux, macOS, Windows)

**Target Platform**: Cross-platform (Linux, macOS, Windows) with binary distribution

**Project Type**: Single CLI application (binary: `aikit`)

**Performance Goals**: 
- Startup time: <100ms (faster than Python version)
- Template download/extraction: Comparable or better than Python version
- Interactive UI responsiveness: <50ms latency for keypress handling

**Constraints**: 
- Must maintain 100% behavioral compatibility with Python version
- Output formatting must match Python version exactly (Rich panels, tables, trees)
- Must handle all edge cases documented in feature inventory
- Binary must work without requiring Rust toolchain installation

**Scale/Scope**: 
- Support for 17 AI agents with agent-specific configurations (claude, gemini, copilot, cursor-agent, qwen, opencode, codex, windsurf, kilocode, auggie, roo, codebuddy, qoder, amp, shai, q, bob)
- Template packaging for all agent/script combinations: 17 agents × 2 script types (sh, ps) = 34 packages per release
- Cross-platform script support (bash and PowerShell)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

**Note**: Constitution file not found in `.specify/memory/constitution.md`. Assuming standard Rust project practices:
- ✅ Single binary project (not multiple projects)
- ✅ Test-first development approach
- ✅ Clear separation of concerns (CLI, core logic, TUI components)
- ✅ Error handling with actionable messages
- ✅ Cross-platform compatibility

**Gates**: All gates pass. No violations detected.

## Project Structure

### Documentation (this feature)

```text
specs/002-rust-spec-kit-complete/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output (/speckit.plan command)
├── data-model.md        # Phase 1 output (/speckit.plan command)
├── quickstart.md        # Phase 1 output (/speckit.plan command)
├── contracts/           # Phase 1 output (/speckit.plan command)
└── tasks.md             # Phase 2 output (/speckit.tasks command - NOT created by /speckit.plan)
```

### Source Code (repository root)

```text
src/
├── main.rs              # CLI entry point, argument parsing, command dispatch
├── cli/
│   ├── mod.rs           # CLI command definitions
│   ├── init.rs          # `aikit init` command implementation
│   ├── check.rs         # `aikit check` command implementation
│   ├── version.rs       # `aikit version` command implementation
│   ├── package.rs       # `aikit package` command implementation
│   └── release.rs       # `aikit release` command implementation
├── core/
│   ├── mod.rs           # Core business logic
│   ├── agent.rs         # Agent configuration and validation
│   ├── template.rs      # Template download, extraction, merging
│   ├── git.rs           # Git repository operations
│   ├── tools.rs         # Tool detection and checking
│   └── package.rs       # Package generation logic
├── tui/
│   ├── mod.rs           # TUI components (using cli-framework)
│   ├── agent_select.rs  # Interactive agent selection UI
│   └── output.rs        # Formatted output (panels, tables, trees)
├── github/
│   ├── mod.rs           # GitHub API client
│   ├── api.rs           # API request/response handling
│   └── rate_limit.rs    # Rate limit detection and error formatting
├── fs/
│   ├── mod.rs           # File system operations
│   ├── merge.rs         # File merging logic (including deep JSON merge)
│   └── permissions.rs   # File permission handling (Unix)
└── config/
    ├── mod.rs           # Configuration management
    └── agent_config.rs  # Agent configuration parsing

tests/
├── unit/                # Unit tests for core modules
├── integration/         # Integration tests for CLI commands
└── fixtures/            # Test fixtures (sample templates, configs)

Cargo.toml               # Project dependencies and metadata
```

**Structure Decision**: Single Rust project with modular organization. The structure separates concerns:
- `cli/`: Command implementations (thin wrappers around core logic)
- `core/`: Business logic (agent management, template handling, Git operations)
- `tui/`: Interactive UI components (using cli-framework)
- `github/`: GitHub API integration
- `fs/`: File system operations (cross-platform)
- `config/`: Configuration parsing and management

This structure allows for clear separation between CLI interface, business logic, and external integrations, making testing and maintenance easier.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

No violations detected. All gates pass.

