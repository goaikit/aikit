# Strategy Specification: Rust Spec Kit CLI Reimplementation

**Strategy Branch**: `001-rust-spec-kit-cli`  
**Created**: 2025-01-27  
**Status**: Draft  
**Input**: User description: "please, create a spec based on functional aspects of @aikit/specs/spec-kit_feature-inventory.md, but based in rust and leveraing cli-framework . base folder of project is /home/sysuser/ws001/aikit @README.md"

## Abstract Summary

This specification defines a complete Rust reimplementation of the GitHub Spec Kit CLI tool, leveraging the cli-framework (TUI framework) for interactive terminal interfaces. The implementation will replicate all functional behaviors of the Python-based `specify` CLI, but will be distributed as the `aikit` binary. It includes project initialization, template downloading/extraction, agent configuration management, and tool checking capabilities. The core goal is to provide behaviorally identical functionality while taking advantage of Rust's performance, type safety, and the cli-framework's TUI capabilities for enhanced user experience. The implementation will support all 17+ AI agents, handle cross-platform script generation (bash/PowerShell), manage GitHub API interactions with rate limiting, and provide both interactive TUI and non-interactive CLI modes. Key technical decisions include using async/await for network operations, structured error handling, and maintaining exact compatibility with the Python version's file structures, merge behaviors, and edge case handling.

## 0. Reference Implementation Metadata *(mandatory for replication)*

- **Title**: GitHub Spec Kit - Spec-Driven Development Toolkit
- **Authors**: GitHub (Den Delimarsky, John Lam)
- **Year**: 2024-2025
- **Repository**: https://github.com/github/spec-kit
- **Primary Source**: Python CLI implementation in `src/specify_cli/__init__.py`
- **Feature Inventory**: `/home/sysuser/ws001/aikit/specs/spec-kit_feature-inventory.md`
- **Key components to replicate**:
  - `aikit init` command (download, extract, merge, git init) - replicates `specify init`
  - `aikit check` command (tool detection) - replicates `specify check`
  - `aikit version` command (version reporting) - replicates `specify version`
  - Agent configuration system (17+ agents)
  - Template packaging pipeline
  - Cross-platform script support (bash/PowerShell)

## 1. Replication Goal *(mandatory)*

- **Primary replication target**: Behaviorally identical CLI with 100% feature parity
- **Acceptance thresholds**: 
  - All CLI commands produce identical output to Python version
  - Template extraction and merging produces identical file structures
  - GitHub API interactions handle rate limiting identically
  - Cross-platform behaviors match (Windows vs Unix script defaults)
  - Edge cases handled identically (non-empty dir merge, .vscode/settings.json deep-merge, etc.)
- **Known deviations allowed**: 
  - Implementation language: Rust instead of Python
  - CLI binary name: `aikit` instead of `specify` (different command name)
  - UI framework: cli-framework (TUI) for interactive modes instead of Rich library
  - Enhanced TUI experience for interactive selection (arrow keys, live updates)
  - Performance improvements acceptable (faster startup, faster downloads)
  - Error messages may be more structured but must convey same information

## 2. Project Scope *(mandatory)*

- **Target platform**: Cross-platform (Linux, macOS, Windows)
- **Implementation language**: Rust 1.70+ (2021 edition)
- **Primary framework**: cli-framework (TUI framework) for interactive terminal interfaces
- **Sample period/scope**: Complete reimplementation of all features documented in feature inventory
- **Inclusion criteria**:
  - All three CLI commands (`init`, `check`, `version`)
  - All 17+ AI agent configurations
  - Template download and extraction logic
  - GitHub API integration with rate limit handling
  - Cross-platform script variant support (bash/PowerShell)
  - Git repository initialization and management
  - File merging behaviors (especially .vscode/settings.json)
  - Script permission normalization (Unix only)
- **Exclusion criteria**:
  - Python-specific packaging (pyproject.toml, uv tool install)
  - Release packaging pipeline (separate concern, may be implemented later)
  - Devcontainer configuration (not part of CLI tool)
- **Edge case handling requirements**:
  - Non-empty directory merging with confirmation
  - ZIP flattening when single top-level directory exists
  - Deep JSON merging for .vscode/settings.json
  - Claude CLI special path handling (~/.claude/local/claude)
  - GitHub branch name 244-byte limit enforcement
  - Non-git repository support (SPECIFY_FEATURE env var)
- **Missing data policy**:
  - GitHub API failures: graceful degradation (show "unknown" for version)
  - Missing templates: create empty files with warnings
  - Missing git: skip git operations with warnings

## 3. Data Map *(mandatory)*

| Python Implementation | Rust Equivalent | Data Source | Field/API | Frequency | Timing | Notes |
|---|---|---|---|---|---|---|
| `AGENT_CONFIG` dict | `AgentConfig` struct/enum | Hardcoded in code | Agent metadata | Static | Compile-time | Must match exact agent keys, folders, install URLs |
| GitHub Releases API | `reqwest` (async) | `api.github.com/repos/github/spec-kit/releases/latest` | JSON response | On-demand | During `init` | Handle rate limiting, auth headers, async streaming |
| Template ZIP assets | Downloaded files | GitHub release assets | Binary ZIP | On-demand | During `init` | Stream download with progress |
| Project directory structure | `std::fs` operations | Local filesystem | Directories/files | On-demand | During `init` | Cross-platform path handling |
| Git repository state | `std::process::Command` | Local git repo | Git commands | On-demand | During `init`/`check` | Matches Python version's subprocess approach |
| Tool detection | `which` crate or `std::process` | System PATH | Executable lookup | On-demand | During `check`/`init` | Special handling for Claude local path |
| Environment variables | `std::env` | Process environment | GH_TOKEN, GITHUB_TOKEN, SPECIFY_FEATURE | On-demand | Throughout | Precedence: CLI arg > GH_TOKEN > GITHUB_TOKEN |

## 4. Core Features/Components *(mandatory)*

### Feature: CLI Application Structure

- **Type**: Application entry point
- **Framework integration**: cli-framework AppBuilder pattern
- **Commands to implement**:
  - `init` - Project initialization with template download
  - `check` - Tool availability checking
  - `version` - Version and system information display
- **Interactive mode detection**: Use cli-framework's `cli_mode::is_interactive()` or TTY detection
- **Default behavior**: CLI mode (text output) as default, matching Python version behavior
- **TUI mode**: Enabled via `--tui` flag for enhanced interactive experience
- **Non-interactive fallback**: CLI output utilities for JSON/text tables when not TTY (piped/scripted)
- **Error handling**: Structured errors with context, display via cli-framework modals or CLI output
- **Configuration**: Agent config as static data structure, CLI args via `clap` or similar

### Feature: Template Download & Extraction

- **Type**: Network + file system operation
- **Implementation approach**: 
  - Async HTTP client (`reqwest` with streaming) for network operations
  - Progress tracking via cli-framework progress widgets or CLI output
  - Sync ZIP extraction using `zip` crate (file system operations)
  - Sync file merging logic for `--here` mode (file system operations)
- **Special handling**:
  - `.vscode/settings.json` deep merge (recursive JSON merge)
  - ZIP flattening when single top-level directory
  - Directory merging (recursive file copy)
  - File overwriting (non-directory files)
- **Error recovery**: Cleanup on failure (remove created directories if not `--here`)
- **Cross-platform**: Handle path separators, line endings appropriately

### Feature: Agent Configuration System

- **Type**: Static configuration + dynamic selection
- **Data structure**: Enum or struct with agent metadata
- **Agent properties**:
  - Key (must match CLI executable name)
  - Display name
  - Folder path (e.g., `.claude/`, `.cursor/`)
  - Install URL (optional for IDE-based)
  - Requires CLI flag
- **Selection logic**:
  - Interactive: cli-framework arrow-key selector (GridView or custom widget)
  - Non-interactive: CLI argument parsing
  - Default: `copilot` for interactive selection
- **Validation**: Check agent key exists in config, validate CLI tool if required

### Feature: GitHub API Integration

- **Type**: HTTP client with rate limit handling
- **Implementation**: 
  - `reqwest` (async HTTP client) with custom headers
  - Token detection (CLI arg > GH_TOKEN > GITHUB_TOKEN env vars)
  - Rate limit header parsing (X-RateLimit-*)
  - Error formatting with rate limit information
  - Streaming support for large file downloads
- **Endpoints used**:
  - `GET /repos/github/spec-kit/releases/latest` (for template version and asset list)
  - Asset download URLs (streaming)
- **Template source**: Uses the same GitHub releases as the Python version, ensuring shared template source and compatibility
- **Error handling**:
  - Rate limit errors: Format with reset time, remaining requests, troubleshooting tips
  - Network errors: Retry logic (optional, not in Python version)
  - Authentication: Bearer token in Authorization header if provided
- **TLS options**: Support `--skip-tls` flag (not recommended, for troubleshooting)

### Feature: Git Integration

- **Type**: External process execution
- **Implementation**: `std::process::Command` (matches Python version's `subprocess` approach)
- **Operations**:
  - Repository detection (`git rev-parse --is-inside-work-tree`)
  - Repository initialization (`git init`)
  - Initial commit (`git add .` + `git commit`)
- **Error handling**: 
  - Parse command output and exit codes for error detection
  - Git not found: Warning, skip operations
  - Git init fails: Warning panel with manual instructions
  - Already a repo: Skip initialization, report "existing repo detected"
- **Flags**: `--no-git` to skip all git operations

### Feature: Tool Detection

- **Type**: System PATH lookup
- **Implementation**: `which` crate or manual PATH search
- **Special cases**:
  - Claude: Check `~/.claude/local/claude` path in addition to PATH
  - All tools: Use `shutil.which` equivalent (PATH search)
- **Integration**: 
  - Used in `check` command (all agents + git + code/code-insiders)
  - Used in `init` command (agent tool validation unless `--ignore-agent-tools`)
- **Output**: Track status (available/not found/skipped) for display

### Feature: Interactive Selection UI

- **Type**: TUI component
- **Framework**: cli-framework GridView or custom selection widget
- **Behavior**:
  - Arrow keys (↑/↓) or Ctrl+P/Ctrl+N for navigation
  - Enter to select
  - Esc to cancel (exit code 1)
  - Live rendering with current selection highlighted
- **Use cases**:
  - AI agent selection (default: `copilot`)
  - Script type selection (default: `ps` on Windows, `sh` otherwise)
- **TTY detection**: Only show interactive UI if stdin is TTY
- **Fallback**: Auto-select defaults if not TTY

### Feature: Script Permission Normalization

- **Type**: File system operation (Unix only)
- **Implementation**: `std::fs::Permissions` manipulation
- **Scope**: Recursively find `.specify/scripts/**/*.sh` files with `#!` shebang
- **Logic**:
  - Check if file has execute bits
  - Mirror read permissions to execute permissions (owner/group/other)
  - Always ensure owner execute bit
  - Skip symlinks
- **Platform**: Windows: no-op (silent skip)
- **Reporting**: Count updated files, report failures

### Feature: Version Reporting

- **Type**: Metadata retrieval + API call
- **CLI version**: From `Cargo.toml` or compiled-in version
- **Template version**: From GitHub API (`/repos/github/spec-kit/releases/latest`)
- **System info**: Platform, architecture, OS version (via `std::env` and system APIs)
- **Error handling**: GitHub API failure keeps version as "unknown", doesn't fail command
- **Display**: Formatted table via cli-framework or CLI output utilities

## 5. Architecture & Design *(mandatory)*

- **Approach**: Modular Rust application with clear separation of concerns
- **Project structure**:
  ```
  aikit/
  ├── Cargo.toml
  ├── src/
  │   ├── main.rs              # Entry point, CLI parsing
  │   ├── commands/             # Command implementations
  │   │   ├── init.rs
  │   │   ├── check.rs
  │   │   └── version.rs
  │   ├── config/               # Configuration structures
  │   │   └── agents.rs         # AGENT_CONFIG equivalent
  │   ├── github/               # GitHub API client
  │   │   ├── client.rs
  │   │   └── rate_limit.rs
  │   ├── template/             # Template download/extraction
  │   │   ├── download.rs
  │   │   ├── extract.rs
  │   │   └── merge.rs
  │   ├── git/                  # Git operations
  │   │   └── operations.rs
  │   ├── ui/                   # TUI components (cli-framework integration)
  │   │   ├── selector.rs       # Interactive selection widget
  │   │   └── progress.rs       # Progress indicators
  │   └── utils/                # Utilities
  │       ├── paths.rs
  │       ├── tools.rs
  │       └── errors.rs
  └── tests/                    # Integration tests
  ```
- **Dependencies**:
  - `cli-framework` (TUI framework) - for interactive UI
  - `clap` or `argh` - for CLI argument parsing
  - `reqwest` (async) - for HTTP client (network operations)
  - `zip` - for ZIP extraction
  - `serde` + `serde_json` - for JSON parsing
  - `std::process` - for git operations (matches Python version's subprocess approach)
  - `which` - for tool detection
  - `tokio` - for async runtime (if using async HTTP)
  - `anyhow` - for error handling
- **Error handling strategy**: 
  - Structured errors with context (`anyhow::Error` or custom error types)
  - User-friendly messages via cli-framework modals or CLI output
  - Error recovery where possible (e.g., git init failure doesn't abort project creation)
- **Async vs Sync**: 
  - Async for network operations only (downloads, API calls via `reqwest`)
  - Sync for file system operations (matches Python version's synchronous approach)
  - Sync for git operations (via `std::process::Command`)
  - Sync for tool detection (via `which` crate or `std::process`)
  - cli-framework can handle mixed sync/async code patterns

## 6. Implementation Phases & Execution *(mandatory)*

- **Phase 1: Core CLI Structure**
  - Set up Cargo project with dependencies
  - Implement CLI argument parsing (`clap`)
  - Create command trait/structure for `init`, `check`, `version`
  - Implement basic error handling
- **Phase 2: Agent Configuration**
  - Define agent config data structures
  - Implement agent selection logic (interactive + non-interactive)
  - Implement tool detection
- **Phase 3: GitHub API Integration**
  - Implement HTTP client with token support
  - Implement rate limit parsing and error formatting
  - Implement release/asset fetching
- **Phase 4: Template Download & Extraction**
  - Implement streaming download with progress
  - Implement ZIP extraction
  - Implement file merging logic (especially .vscode/settings.json)
  - Implement ZIP flattening
- **Phase 5: Git Integration**
  - Implement repository detection
  - Implement git init and initial commit
  - Handle errors gracefully
- **Phase 6: TUI Integration**
  - Integrate cli-framework for interactive modes
  - Implement selection widgets
  - Implement progress indicators
  - Implement modal dialogs for errors/warnings
- **Phase 7: Cross-Platform Polish**
  - Test on Windows, Linux, macOS
  - Handle path separators correctly
  - Implement script permission normalization (Unix only)
  - Test edge cases (non-empty dirs, missing tools, etc.)
- **Phase 8: Testing & Validation**
  - Integration tests comparing output to Python version
  - Test all edge cases from feature inventory
  - Validate file structures match exactly
  - Performance testing

## 7. Constraints & Requirements *(mandatory)*

- **Functional constraints**: 
  - Must produce identical file structures to Python version
  - Must handle all edge cases identically
  - Must support all 17+ agents
  - Must work cross-platform (Windows, Linux, macOS)
- **Performance constraints**: 
  - Startup time: < 100ms (Rust advantage)
  - Download speed: Comparable or better (streaming)
  - Memory usage: Reasonable (avoid loading entire ZIP in memory)
- **Compatibility constraints**:
  - Must work with existing templates (no template format changes)
  - Must produce compatible project structures
  - Must handle same GitHub API responses
- **User experience constraints**:
  - Default mode: CLI mode (text output) matching Python version behavior
  - TUI mode: Available via `--tui` flag for enhanced interactive experience via cli-framework
  - Non-interactive mode: Identical output format to Python version
  - Error messages: Must convey same information (can be more structured)
- **Technical constraints**:
  - Rust 1.70+ (2021 edition)
  - cli-framework for TUI (required dependency)
  - Async runtime (Tokio) for network operations
  - Cross-platform file system operations

## 8. Evaluation & Validation *(mandatory)*

- **Functional validation**:
  - Run `aikit init` with same args, compare output directories to `specify init` results
  - Run `aikit check`, compare tool detection results to `specify check` results
  - Run `aikit version`, compare version information to `specify version` results
  - Test all edge cases from feature inventory
- **Compatibility tests**:
  - Initialize project with Rust version (`aikit`), verify Python version (`specify`) can read it
  - Initialize project with Python version (`specify`), verify Rust version (`aikit`) can read it
  - Test all agent configurations produce correct directory structures
- **Performance benchmarks**:
  - Startup time comparison
  - Download speed comparison
  - Memory usage comparison
- **Cross-platform tests**:
  - Test on Windows (script defaults, path handling)
  - Test on Linux (script permissions, path handling)
  - Test on macOS (same as Linux)
- **Error handling tests**:
  - Network failures (offline, rate limits)
  - File system failures (permissions, disk full)
  - Git failures (git not installed, repo conflicts)
  - Invalid inputs (bad agent names, invalid paths)

## 9. Open Questions / Ambiguities *(mandatory)*

- [x] CLI binary name: `aikit` (resolved - see Clarifications section)
- [x] TUI mode default: CLI mode as default, `--tui` flag for TUI (resolved - see Clarifications section)
- [ ] [NEEDS CLARIFICATION: For the release packaging pipeline, should this be included in the Rust implementation or kept as a separate Python script?]
- [ ] [NEEDS CLARIFICATION: Should the Rust version support the same installation methods (uv tool install) or use Cargo/cargo install?]
- [ ] [NEEDS CLARIFICATION: How should version information be embedded? Via Cargo.toml version or compile-time constants?]
- [ ] [NEEDS CLARIFICATION: Should the Rust version maintain backward compatibility with Python version's project structures, or can it evolve?]
- [x] Git operations approach: Use `std::process::Command` (matches Python approach) (resolved - see Clarifications section)
- [x] Async usage scope: Use async only for network operations (resolved - see Clarifications section)
- [x] Template ZIP handling: Use same GitHub releases (shared templates) (resolved - see Clarifications section)
- [ ] [NEEDS CLARIFICATION: Should the Rust version include the same devcontainer support, or is that out of scope?]

## 10. Assumptions *(optional)*

- **Assumption 1**: cli-framework provides sufficient TUI capabilities for interactive selection and progress display. If not, we'll extend or use ratatui directly.
- **Assumption 2**: Users will install via `cargo install` or similar Rust package management, not via Python's `uv tool install`.
- **Assumption 3**: The Rust version uses the same GitHub releases and template ZIP files as the Python version (no format changes needed). This ensures maximum compatibility and maintains a single source of truth for templates.
- **Assumption 4**: Cross-platform path handling via `std::path::Path` and `std::path::PathBuf` will be sufficient (no need for additional path manipulation crates).
- **Assumption 5**: The `zip` crate provides sufficient functionality for extraction and doesn't require additional ZIP manipulation libraries.
- **Assumption 6**: Error messages can be more structured (using structured error types) while still conveying the same information to users.
- **Assumption 7**: Performance improvements (faster startup, faster downloads) are acceptable and don't need to match Python version's timing exactly.

## Clarifications

### Session 2025-01-27

- Q: Should the Rust version maintain the same CLI binary name `specify` or use a different name like `specify-rs`? → A: The new tool should be called `aikit`
- Q: Should the TUI mode be the default for interactive commands, or should it require a flag like `--tui`? → A: CLI mode as default, `--tui` flag for TUI
- Q: How should the Rust version handle the same template ZIP files? Should it use the same GitHub releases or have separate releases? → A: Use same GitHub releases (shared templates)
- Q: For git operations, should we prefer git2 crate (better error handling) or std::process (simpler, matches Python approach)? → A: Use `std::process::Command` (matches Python approach)
- Q: Should async be used throughout (matching cli-framework's async nature) or only for network operations? → A: Use async only for network operations
- Q: Which HTTP client library should be used: `reqwest` (async) or `ureq` (sync)? → A: Use `reqwest` (async)
- Q: Which CLI argument parsing library should be used: `clap` or `argh`? → A: Use `clap`

