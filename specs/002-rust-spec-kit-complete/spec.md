# Feature Specification: AIKIT - Rust Spec Kit CLI Complete Reimplementation

**Feature Branch**: `002-rust-spec-kit-complete`  
**Created**: 2025-01-27  
**Status**: Draft  
**Author**: @aroff (GitHub)  
**Tool Name**: AIKIT  
**Binary Command**: `aikit`  
**Input**: User description: "please, create a spec based on functional aspects of @aikit/specs/spec-kit_feature-inventory.md, but based in rust. base folder of project is /home/sysuser/ws001/aikit"

## Abstract Summary

This specification defines a complete Rust reimplementation of the GitHub Spec Kit CLI tool, providing behaviorally identical functionality to the Python-based `specify` command. The implementation will be distributed as **AIKIT** (binary command: `aikit`), authored by @aroff, and will replicate all features documented in the feature inventory, including project initialization with template downloading/extraction, agent configuration management, tool checking, version reporting, cross-platform script support, and the release packaging pipeline. The core goal is 100% functional parity while leveraging Rust's performance, type safety, and the cli-framework for enhanced interactive terminal experiences. The implementation must support all 17 AI agents (claude, gemini, copilot, cursor-agent, qwen, opencode, codex, windsurf, kilocode, auggie, roo, codebuddy, qoder, amp, shai, q, bob), handle GitHub API interactions with proper rate limiting, manage complex file merging scenarios (including deep JSON merging for `.vscode/settings.json`), provide both interactive TUI and non-interactive CLI modes, and include the complete release packaging pipeline that builds template zip archives for GitHub releases. All edge cases, error handling behaviors, and output formatting must match the Python implementation exactly.

## Clarifications

### Session 2025-01-27

- Q: Should the Rust implementation include the release packaging pipeline (the scripts that build template zip files for GitHub releases), or is that out of scope? → A: Include packaging pipeline - replicate all `.github/workflows/scripts/*` functionality
- **Tool Name**: The tool will be named **AIKIT** (binary command: `aikit`)
- **Author**: @aroff (GitHub)

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Initialize New Project (Priority: P1)

**Description**: As a developer, I want to initialize a new Spec-Driven Development project in a new directory by running `aikit init <project-name>`, so that I can start using the SDD workflow immediately.

**Why this priority**: This is the primary use case and entry point for all users. Without this, the tool provides no value.

**Independent Test**: Can be fully tested by running `aikit init test-project --ai claude` and verifying that:
- A new directory `test-project` is created
- Template files are downloaded and extracted correctly
- Agent-specific command files are present in the correct locations
- Git repository is initialized (if git is available)
- Script permissions are set correctly on Unix systems
- Output messages match Python version format

**Acceptance Scenarios**:

1. **Given** a clean directory, **When** user runs `aikit init my-project --ai claude`, **Then** a new `my-project` directory is created with all template files extracted and git initialized
2. **Given** a directory name that already exists, **When** user runs `aikit init existing-dir`, **Then** an error is displayed and the command exits with code 1
3. **Given** no `--ai` flag provided, **When** user runs `aikit init my-project` in an interactive terminal, **Then** an interactive agent selection UI is shown with arrow key navigation
4. **Given** `--here` flag, **When** user runs `aikit init --here --ai copilot` in a non-empty directory, **Then** files are merged into the current directory with confirmation prompt (unless `--force` is used)
5. **Given** a non-empty directory with `--here --force`, **When** user runs `aikit init --here --force`, **Then** files are merged without confirmation prompt
6. **Given** `.vscode/settings.json` already exists, **When** template extraction occurs, **Then** the files are deep-merged (nested objects merged, arrays replaced) rather than overwritten

---

### User Story 2 - Check Installed Tools (Priority: P1)

**Description**: As a developer, I want to run `aikit check` to verify that required tools (git, AI agent CLIs) are installed, so that I know what's available before starting a project.

**Why this priority**: Users need to verify their environment before attempting to initialize projects. This prevents frustration from missing dependencies.

**Independent Test**: Can be fully tested by running `aikit check` and verifying:
- All tools in AGENT_CONFIG are checked (CLI tools if `requires_cli=True`, skipped if IDE-based)
- Git is checked
- VS Code variants (`code`, `code-insiders`) are checked
- Output format matches Python version (tracker tree format)
- Exit code is 0 regardless of tool availability

**Acceptance Scenarios**:

1. **Given** all tools installed, **When** user runs `aikit check`, **Then** all tools are marked as available in the output
2. **Given** some tools missing, **When** user runs `aikit check`, **Then** missing tools are marked as "not found" and helpful tips are displayed
3. **Given** Claude CLI migrated to local path, **When** user runs `aikit check`, **Then** Claude is detected via `~/.claude/local/claude` even if not on PATH
4. **Given** IDE-based agents (copilot, windsurf), **When** user runs `aikit check`, **Then** these are marked as "IDE-based, no CLI check" rather than checked

---

### User Story 3 - Display Version Information (Priority: P2)

**Description**: As a developer, I want to run `aikit version` to see the CLI version, template version, and system information, so that I can verify I'm using the latest version and troubleshoot issues.

**Why this priority**: Important for troubleshooting and version management, but not critical for core functionality.

**Independent Test**: Can be fully tested by running `aikit version` and verifying:
- CLI version is displayed (from Cargo.toml or package metadata)
- Template version is fetched from GitHub latest release API
- System information (OS, architecture, Rust version) is displayed
- Format matches Python version (panel with table)
- Graceful handling when GitHub API is unavailable (shows "unknown")

**Acceptance Scenarios**:

1. **Given** normal operation, **When** user runs `aikit version`, **Then** all version information is displayed in a formatted panel
2. **Given** GitHub API unavailable, **When** user runs `aikit version`, **Then** template version shows "unknown" but CLI version and system info still display
3. **Given** GitHub token provided, **When** user runs `aikit version`, **Then** API request uses authentication for higher rate limits

---

### User Story 4 - Interactive Agent Selection (Priority: P2)

**Description**: As a developer, I want to use arrow keys to interactively select an AI agent when I don't specify `--ai`, so that I can choose from available options without remembering exact agent keys.

**Why this priority**: Enhances user experience for users unfamiliar with agent keys, but core functionality works without it.

**Independent Test**: Can be fully tested by running `aikit init my-project` (without `--ai`) in an interactive terminal and verifying:
- Arrow key navigation works (↑/↓)
- Enter selects the highlighted option
- Esc or Ctrl+C cancels with exit code 1
- Default selection starts at "copilot"
- Output format matches Python version (cyan highlighting, gray descriptions)

**Acceptance Scenarios**:

1. **Given** interactive terminal, **When** user runs `aikit init my-project` without `--ai`, **Then** interactive selection UI appears
2. **Given** non-interactive terminal (piped input), **When** user runs `aikit init my-project` without `--ai`, **Then** default agent (copilot) is selected automatically
3. **Given** user presses Esc, **When** in selection UI, **Then** selection is cancelled and command exits with code 1

---

### User Story 5 - GitHub Rate Limit Handling (Priority: P2)

**Description**: As a developer in a corporate environment or CI, I want clear error messages when GitHub API rate limits are hit, so that I understand what happened and how to resolve it.

**Why this priority**: Important for reliability in constrained environments, but edge case for most users.

**Independent Test**: Can be fully tested by simulating rate limit responses and verifying:
- Error message includes rate limit information (limit, remaining, reset time)
- Troubleshooting tips are provided
- Suggestion to use `--github-token` is included
- Format matches Python version exactly

**Acceptance Scenarios**:

1. **Given** GitHub API returns 403 with rate limit headers, **When** template download is attempted, **Then** a detailed error message with rate limit info and troubleshooting tips is displayed
2. **Given** authenticated request with token, **When** rate limit is hit, **Then** error message indicates higher limits (5000/hour vs 60/hour)
3. **Given** `Retry-After` header present, **When** rate limit error occurs, **Then** retry time is included in the error message

---

### User Story 6 - Build Release Packages (Priority: P3)

**Description**: As a maintainer, I want to build template zip archives for all agent/script combinations using `aikit package <version>`, so that I can publish releases with all template variants.

**Why this priority**: Required for maintaining the spec-kit repository and publishing releases, but not needed for end users of the CLI tool.

**Independent Test**: Can be fully tested by running `aikit package v1.0.0` and verifying:
- Template zip files are created for all agent/script combinations
- Files are generated with correct naming pattern: `spec-kit-template-<agent>-<script>-vX.Y.Z.zip`
- Command files are generated with correct placeholders replaced
- Path rewrites are applied correctly (memory/ → .specify/memory/, etc.)
- Agent-specific output directories and formats are correct

**Acceptance Scenarios**:

1. **Given** a valid version string, **When** user runs `aikit package v1.0.0`, **Then** zip archives are created for all agents and script types in `.genreleases/` directory
2. **Given** `AGENTS=claude,gemini` environment variable, **When** user runs `aikit package v1.0.0`, **Then** only packages for specified agents are created
3. **Given** `SCRIPTS=sh` environment variable, **When** user runs `aikit package v1.0.0`, **Then** only bash script variants are packaged
4. **Given** invalid version format, **When** user runs `aikit package invalid`, **Then** an error is displayed and command exits with code 1

---

### Edge Cases

- What happens when the downloaded zip file is corrupted or incomplete?
- How does the system handle network timeouts during template download?
- What happens when `.vscode/settings.json` exists but contains invalid JSON?
- How does the system handle very long branch names that exceed GitHub's 244-byte limit?
- What happens when git init fails but template extraction succeeds?
- How does the system handle missing template files in the zip archive?
- What happens when multiple top-level directories exist in the zip (should not happen, but edge case)?
- How does the system handle Windows vs Unix path separators in template paths?
- What happens when script permission setting fails on some files but succeeds on others?
- How does the system handle GitHub API returning HTML error pages instead of JSON?
- What happens when package build fails partway through (some agents succeed, others fail)?
- How does the system handle invalid YAML frontmatter in command templates during packaging?
- What happens when template files are missing during package generation?
- How does the system handle version conflicts when a release already exists?
- What happens when GitHub CLI (`gh`) is not available for release creation?

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST support all 17 AI agents documented in AGENT_CONFIG with correct directory structures and file formats (claude, gemini, copilot, cursor-agent, qwen, opencode, codex, windsurf, kilocode, auggie, roo, codebuddy, qoder, amp, shai, q, bob)
- **FR-002**: System MUST download templates from GitHub releases API with proper authentication and rate limit handling
- **FR-003**: System MUST extract zip archives and handle nested directory flattening when exactly one top-level directory exists
- **FR-004**: System MUST merge files when using `--here` flag, preserving existing content where appropriate
- **FR-005**: System MUST deep-merge `.vscode/settings.json` files (nested objects merged recursively, arrays replaced, scalars overwritten)
- **FR-006**: System MUST set execute permissions on `.specify/scripts/**/*.sh` files on Unix systems (non-Windows)
- **FR-007**: System MUST initialize git repositories when `--no-git` is not set and git is available
- **FR-008**: System MUST detect existing git repositories and skip initialization with appropriate message
- **FR-009**: System MUST validate agent selection against AGENT_CONFIG and enforce CLI tool checks when `requires_cli=True` (unless `--ignore-agent-tools`)
- **FR-010**: System MUST provide interactive selection UI when `--ai` or `--script` not provided and stdin is a TTY
- **FR-011**: System MUST default script type to `ps` on Windows and `sh` on non-Windows when not specified
- **FR-012**: System MUST handle GitHub token from CLI arg, `GH_TOKEN`, or `GITHUB_TOKEN` environment variables (in that precedence order)
- **FR-013**: System MUST format rate limit errors with detailed information including limit, remaining, reset time, and troubleshooting tips
- **FR-014**: System MUST support `--skip-tls` flag for troubleshooting (not recommended but available)
- **FR-015**: System MUST support `--debug` flag for verbose diagnostic output on failures
- **FR-016**: System MUST detect Claude CLI at `~/.claude/local/claude` even if not on PATH (special case for migrated installers)
- **FR-017**: System MUST validate project name and prevent initialization into existing directories (unless `--here`)
- **FR-018**: System MUST handle `project_name` argument of `.` as equivalent to `--here` flag
- **FR-019**: System MUST provide Codex-specific setup instructions (CODEX_HOME environment variable) when codex agent is selected
- **FR-020**: System MUST display agent folder security notice after successful initialization
- **FR-021**: System MUST check all tools in AGENT_CONFIG for `check` command, marking IDE-based agents as skipped
- **FR-022**: System MUST fetch template version from GitHub latest release API for `version` command
- **FR-023**: System MUST gracefully handle GitHub API failures in `version` command (show "unknown" but continue)
- **FR-024**: System MUST output all messages in format matching Python version (Rich panels, tables, trees)
- **FR-025**: System MUST support both interactive TUI mode (using cli-framework) and non-interactive CLI mode
- **FR-026**: System MUST preserve exact file structures and content from Python version templates
- **FR-027**: System MUST handle cross-platform path differences (Windows vs Unix) correctly
- **FR-028**: System MUST validate branch names against GitHub's 244-byte limit and truncate with warning if needed
- **FR-029**: System MUST support `--force` flag to skip confirmation when merging into non-empty directory
- **FR-030**: System MUST provide helpful error messages with actionable suggestions when operations fail
- **FR-031**: System MUST provide `package` command to build template zip archives for GitHub releases
- **FR-032**: System MUST generate agent-specific command files from templates with correct placeholder replacements (`{SCRIPT}`, `{AGENT_SCRIPT}`, `{ARGS}`, `__AGENT__`)
- **FR-033**: System MUST rewrite path references in templates (memory/ → .specify/memory/, scripts/ → .specify/scripts/, templates/ → .specify/templates/)
- **FR-034**: System MUST support filtering package builds by agent list via `AGENTS` environment variable (comma or space separated)
- **FR-035**: System MUST support filtering package builds by script type via `SCRIPTS` environment variable (comma or space separated)
- **FR-036**: System MUST generate correct output formats per agent (Markdown for most, TOML for gemini/qwen, agent.md for copilot)
- **FR-037**: System MUST generate correct output directories per agent (e.g., `.claude/commands`, `.gemini/commands`, `.github/agents`, etc.)
- **FR-038**: System MUST generate Copilot prompt files (`.github/prompts/*.prompt.md`) when packaging copilot agent
- **FR-039**: System MUST copy base directories (memory, scripts, templates) into package structure under `.specify/` with correct filtering
- **FR-040**: System MUST exclude `templates/commands/*` and `templates/vscode-settings.json` from base template copy (these are generated per-agent)
- **FR-041**: System MUST remove `scripts:` and `agent_scripts:` sections from YAML frontmatter in generated command files
- **FR-042**: System MUST validate version format (must match `vX.Y.Z` pattern) before packaging
- **FR-043**: System MUST create zip archives with correct naming: `spec-kit-template-<agent>-<script>-<version>.zip`
- **FR-044**: System MUST provide `release` command to create GitHub releases with all package files attached
- **FR-045**: System MUST provide version management utilities (get-next-version, update-version, generate-release-notes, check-release-exists). **Note**: These utilities are primarily for CI/CD workflows and may be implemented as separate commands or scripts. For MVP, focus on core CLI commands (init, check, version, package, release).

### Key Entities *(include if feature involves data)*

- **AgentConfig**: Represents an AI agent configuration with fields: `key` (executable name), `name` (display name), `folder` (project directory), `install_url` (optional), `requires_cli` (boolean)
- **ProjectPath**: Represents a target project location with validation rules (must not exist unless `--here`, must be valid path)
- **TemplateAsset**: Represents a downloadable template zip file with metadata: `filename`, `size`, `release_tag`, `download_url`
- **GitHubRateLimitInfo**: Represents rate limit information parsed from API headers: `limit`, `remaining`, `reset_epoch`, `reset_time`, `retry_after_seconds`
- **FeatureBranch**: Represents a feature branch/spec directory with format `NNN-short-name` where NNN is zero-padded 3-digit number
- **ScriptVariant**: Enum representing script type: `Sh` (bash) or `Ps` (PowerShell)
- **PackageConfig**: Represents packaging configuration with fields: `version` (with v prefix), `agents` (optional filter list), `scripts` (optional filter list), `output_dir` (default `.genreleases/`)
- **CommandTemplate**: Represents a command template file with metadata: `name`, `description`, `script_commands` (per variant), `agent_script_commands` (optional, per variant), `body` (template content)
- **ReleaseMetadata**: Represents GitHub release information with fields: `tag_name`, `published_at`, `assets` (list of downloadable files)

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All three CLI commands (`init`, `check`, `version`) produce output that is functionally identical to Python version when given same inputs
- **SC-002**: Template extraction produces identical file structures and content as Python version for all 17 agent types
- **SC-003**: File merging behavior (especially `.vscode/settings.json` deep-merge) produces identical results to Python version
- **SC-004**: GitHub API interactions handle rate limiting identically to Python version (same error messages, same retry logic)
- **SC-005**: Cross-platform behavior matches Python version (Windows defaults to PowerShell, Unix defaults to bash)
- **SC-006**: Interactive selection UI provides equivalent user experience to Python version (same keybindings, same visual format)
- **SC-007**: All edge cases documented in feature inventory are handled with identical behavior to Python version
- **SC-008**: Error messages convey same information as Python version (may be more structured but semantically equivalent)
- **SC-009**: Performance is equal or better than Python version (startup time, download speed, extraction speed)
- **SC-010**: Binary distribution works on all target platforms (Linux, macOS, Windows) without requiring Rust toolchain
- **SC-011**: Package command produces identical zip archive contents as Python version for all agent/script combinations
- **SC-012**: Command file generation produces identical output (placeholder replacements, path rewrites, frontmatter removal) as Python version
- **SC-013**: Release creation command produces identical GitHub releases with same assets and metadata as Python version

