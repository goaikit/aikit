# Feature Specification: AIkit Documentation Audit and Update

**Feature Branch**: `004-docs-create`
**Created**: 2025-01-06
**Status**: Draft
**Input**: User description: "Create a spec for documentation of AIkit cli project"

## Clarifications

### Session 2025-01-06
- Q: Should this feature create entirely new documentation files, or update/enhance the existing minimal docs/ directory? Are there specific documentation topics or sections that should be explicitly excluded? → A: Create comprehensive new documentation files covering all CLI commands, configuration, and examples (7-8 new files)
- Q: What are the target metrics for documentation loading/access performance and maximum documentation size? → A: Focus on usability over performance - no specific time/size limits
- Q: How should GitHub token security and privacy be documented? Should users be warned about token permissions and storage? → A: Include security warnings about token permissions, recommend minimal required scopes, and document secure storage options
- Q: How should error conditions be documented? Should every command document its potential error states and solutions? → A: Document only the most common errors (GitHub token issues, network problems) with general troubleshooting guidance
- Q: Should the documentation address GitHub API rate limits and how to handle them? → A: No documentation of rate limiting - assume users will discover this through errors

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Create Comprehensive CLI Command Documentation (Priority: P1)

As an AIKit user exploring the tool's capabilities, I want complete and accurate documentation for all CLI commands, so I can effectively use AIKit to manage AI agent packages and templates.

**Why this priority**: CLI commands are the primary interface for AIKit. Users need accurate command documentation to accomplish any task with the tool.

**Independent Test**: Can be fully tested by verifying all actual CLI commands have corresponding documentation with runnable examples.

**Acceptance Scenarios**:

1. **Given** `aikit init` command exists, **When** user looks for documentation, **Then** they find comprehensive coverage including all options (`--ai`, `--here`, `--force`, `--no-git`)
2. **Given** `aikit package` subcommands exist, **When** user searches docs, **Then** they find dedicated sections for `init`, `build`, and `publish` with complete examples
3. **Given** `aikit install` supports local directories, **When** user reads docs, **Then** they find working examples like `aikit install .` with proper prerequisites

---

### User Story 2 - Document Configuration System Accurately (Priority: P1)

As an AIKit user configuring the tool, I want clear and consistent documentation about configuration files and options, so I can properly set up AIKit for my environment.

**Why this priority**: Configuration is essential for AIKit to work with different AI agents. Poor configuration documentation leads to setup failures.

**Independent Test**: Can be fully tested by following configuration instructions and verifying AIKit loads the expected settings.

**Acceptance Scenarios**:

1. **Given** AIKit uses `.aikit/config.toml` as primary config, **When** user creates this file, **Then** AIKit loads it successfully and documentation shows the correct path
2. **Given** global config at `~/.aikit/config.toml` exists, **When** local config is missing, **Then** AIKit falls back correctly and docs explain this hierarchy
3. **Given** agent configurations are documented, **When** user reads about supported agents, **Then** they find accurate folder paths and CLI requirements

---

### User Story 3 - Provide Working Examples for All Features (Priority: P2)

As an AIKit user learning the tool, I want all documentation examples to be runnable and accurate, so I can successfully follow tutorials and understand capabilities.

**Why this priority**: Examples are crucial for learning. Broken examples waste user time and create frustration with the tool.

**Independent Test**: Can be fully tested by running documented examples and verifying they work as described.

**Acceptance Scenarios**:

1. **Given** documentation shows package creation, **When** user runs `aikit package init my-package --description "Tools"`, **Then** it creates the expected structure
2. **Given** documentation shows GitHub installation, **When** user runs `aikit install owner/repo`, **Then** it works with proper GitHub token setup
3. **Given** documentation shows search, **When** user runs `aikit search "testing"`, **Then** they get relevant results or helpful no-results message

---

### User Story 4 - Document Package Ecosystem and Workflows (Priority: P2)

As an AIKit user discovering and using community packages, I want clear documentation about the package ecosystem, so I can find, install, and create useful AI agent extensions.

**Why this priority**: The package ecosystem is a core value proposition of AIKit. Users need to understand how to participate in the community.

**Independent Test**: Can be fully tested by following package creation and publishing workflows documented in the examples.

**Acceptance Scenarios**:

1. **Given** user wants to create a package, **When** they follow docs, **Then** `aikit package init`, edit `aikit.toml`, and `aikit package build` works end-to-end
2. **Given** user wants to publish a package, **When** they use `aikit package publish owner/repo`, **Then** docs explain GitHub token requirements and release creation
3. **Given** user wants to find packages, **When** they use `aikit search "keyword"`, **Then** docs explain the search heuristics and installation process

---

### User Story 5 - Document AI Agent Integration Details (Priority: P3)

As an AIKit user working with specific AI agents, I want detailed documentation about how AIKit integrates with different assistants, so I can optimize my setup for my preferred tools.

**Why this priority**: AI agent integration details help users get the most value from AIKit but aren't critical for basic functionality.

**Independent Test**: Can be fully tested by verifying agent-specific configurations work as documented.

**Acceptance Scenarios**:

1. **Given** user prefers Claude Code, **When** they read docs, **Then** they find Claude-specific folder paths (`.claude/commands/`) and configuration details
2. **Given** user wants to check available agents, **When** they run `aikit check`, **Then** docs explain what this command shows and how to interpret results
3. **Given** user needs Cursor integration, **When** they read docs, **Then** they find Cursor-specific arg placeholders (`{args}`) and folder structure

---

### Edge Cases

- Version-specific features require version notices in documentation
- Documentation updates needed when new features are added to maintain consistency
- Configuration file path changes require updating all references and error messages
- Examples with external dependencies (OpenAI API) need clear prerequisites and alternative approaches
- Concurrent documentation edits need conflict resolution strategy

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST document all actual CLI commands with accurate syntax and options: `init`, `check`, `install`, `update`, `remove`, `list`, `search`, `package` subcommands, `release`, `version`
- **FR-002**: System MUST standardize configuration file path references to use `.aikit/config.toml` as primary with `~/.aikit/config.toml` as global fallback
- **FR-003**: System MUST document the actual AikConfig structure with correct fields: version, install_dir, agents, registry, preferences
- **FR-004**: System MUST provide comprehensive documentation for package management workflows: creation, building, publishing, and installation
- **FR-005**: System MUST verify all CLI examples are runnable and include proper prerequisites (GitHub tokens, local directories, etc.)
- **FR-006**: System MUST document the actual `aikit.toml` package schema with correct fields and structure
- **FR-007**: System MUST document project template initialization with all supported AI agents and options
- **FR-008**: System MUST document the search and discovery functionality with realistic examples and expectations
- **FR-009**: System MUST document all 17+ supported AI agents (claude, copilot, cursor-agent, gemini, qwen, opencode, codex, windsurf, kilocode, auggie, roo, codebuddy, qoder, amp, shai, q, bob) with their specific configurations, folder structures, and requirements
- **FR-010**: System MUST include troubleshooting sections for common issues: GitHub token setup, permission errors, agent detection
- **FR-011**: System MUST document examples requiring external dependencies (GitHub API) with clear prerequisites, setup instructions, and security warnings about token permissions and secure storage
- **FR-012**: System MUST create 7-8 comprehensive new documentation files in webdocs/ directory: cli-commands.mdx, configuration.mdx, packages.mdx, agents.mdx, examples.mdx, troubleshooting.mdx, and enhanced index.mdx

### Key Entities *(include if feature involves data)*

- **Documentation Files**: MDX files in webdocs/ directory containing user-facing documentation for Mintlify
- **Configuration Files**: TOML files that control AIkit behavior (`.aikit/config.toml`, `~/.aikit/config.toml`, `aikit.toml` for packages)
- **CLI Commands**: Executable commands with their arguments and options (init, check, install, package subcommands, etc.)
- **AikConfig Struct**: Rust struct defining AIkit's global configuration options
- **Package Structure**: `aikit.toml` schema and directory layout for AI agent packages
- **Agent Configurations**: Per-agent settings for 17+ supported AI assistants (claude, copilot, cursor-agent, gemini, qwen, opencode, codex, windsurf, kilocode, auggie, roo, codebuddy, qoder, amp, shai, q, bob)

