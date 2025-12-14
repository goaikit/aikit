# Feature Specification: AIKIT Universal Package System

**Feature Branch**: `1-aikit-generalize`
**Created**: 2024-12-14
**Status**: Draft
**Input**: User description: "please, create a complete specification for aikit, based on generalize.md @aikit/specs/generalize.md"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Package Developer Creates Custom Package (Priority: P1)

As a developer creating specialized AI agent extensions, I want to package my templates, scripts, and configurations into a reusable format so that other developers can easily install and use my AI agent extensions.

**Why this priority**: This is the core value proposition - enabling the community to create and share AI agent extensions. Without this, the ecosystem cannot grow.

**Independent Test**: Can be tested by creating a simple package (e.g., writing templates), publishing it to GitHub, and having another user successfully install and use it.

**Acceptance Scenarios**:

1. **Given** a developer has custom templates and scripts, **When** they run `aikit package init my-package`, **Then** a package.toml file is created with proper structure
2. **Given** a package.toml file exists, **When** they run `aikit package build`, **Then** a distributable ZIP file is created
3. **Given** a package ZIP exists on GitHub, **When** another user runs `aikit install <repo-url>`, **Then** the package installs and creates agent-specific commands

---

### User Story 2 - Agent User Installs Packages (Priority: P1)

As an AI agent user, I want to easily discover and install packages that extend my agent's capabilities so that I can customize my AI experience with community-created extensions.

**Why this priority**: This enables users to benefit from the ecosystem - they can install packages that enhance their AI agents for specific use cases.

**Independent Test**: Can be tested by installing a package and verifying that new commands become available in the user's AI agent environment.

**Acceptance Scenarios**:

1. **Given** a package exists on GitHub, **When** user runs `aikit install <repo-url>`, **Then** package downloads, installs to .aikit/, and generates agent-specific commands
2. **Given** multiple packages are installed, **When** user runs `aikit list`, **Then** all installed packages are displayed with versions
3. **Given** a package has updates, **When** user runs `aikit update <package>`, **Then** package updates to latest version

---

### User Story 3 - Package Publisher Shares Work (Priority: P2)

As a package creator, I want to publish my packages to a central location so that the community can discover and use my AI agent extensions.

**Why this priority**: This completes the ecosystem by enabling sharing and discovery, though it's secondary to the core create/install functionality.

**Independent Test**: Can be tested by publishing a package to GitHub and having it appear in search results or be installable by URL.

**Acceptance Scenarios**:

1. **Given** a package is ready, **When** creator runs `aikit package publish <repo>`, **Then** package is uploaded as GitHub release
2. **Given** packages exist on GitHub, **When** user runs `aikit search <query>`, **Then** relevant packages are found and displayed
3. **Given** package has a README, **When** user runs `aikit show <package>`, **Then** package details and documentation are displayed

---

### Edge Cases

- What happens when package installation fails midway?
- How does system handle conflicting package names (namespace collision)?
- What happens when user tries to install package requiring unsupported agent?
- What happens when .gitignore update is denied by user?
- How does system handle private GitHub repositories?

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST support universal package format with package.toml configuration file
- **FR-002**: System MUST provide CLI commands for package lifecycle management (init, build, publish, install, update, remove, list)
- **FR-003**: System MUST automatically adapt packages for different AI agents based on agent configurations
- **FR-004**: System MUST install package artifacts to .aikit/ directory structure
- **FR-005**: System MUST generate agent-specific commands from package definitions using package namespace prefixes
- **FR-006**: System MUST support version management for packages via GitHub releases/tags with non-breaking update safety
- **FR-007**: System MUST automatically add .aikit/ to .gitignore with user permission
- **FR-008**: System MUST maintain backward compatibility with existing spec-kit packages
- **FR-009**: System MUST support package metadata including compatibility (packages are self-contained with no dependencies)
- **FR-010**: System MUST provide search and discovery capabilities: search by package name/description/tags, return max 20 results sorted by relevance score, support exact and fuzzy matching

### Key Entities *(include if feature involves data)*

- **Package**: A distributable unit containing templates, scripts, assets, and metadata
- **Package.toml**: Configuration file defining package structure, commands, and compatibility
- **Agent Config**: Configuration defining how each AI agent handles commands and file formats
- **Installation Registry**: Tracks installed packages and their versions in .aikit/packages.toml

## Assumptions *(mandatory)*

- GitHub will be used as the primary package distribution mechanism
- All target AI agents have existing configurations in AIKIT's agent registry
- Users have appropriate permissions to install packages and modify .gitignore
- Package creators will follow the established package.toml format
- Network connectivity is available for package downloads and GitHub API access

## Success Criteria *(mandatory)*

- **SC-001**: Users can successfully install any compatible package from GitHub using `aikit install <url>`
- **SC-002**: All existing AIKIT CLI commands (`aikit init`, `aikit check`, etc.) function identically; existing spec-kit templates load and convert without errors; all 17 supported AI agents continue to work with existing templates
- **SC-003**: Package installation completes with exit code 0 and creates expected .aikit/ directory structure
- **SC-004**: All generated agent commands execute without errors and produce expected output files in .{agent}/commands/ directories
- **SC-005**: .aikit/ directory is properly added to .gitignore when requested
- **SC-006**: Package search returns results ranked by relevance (name matches > description matches > tag matches) with no false positives
- **SC-007**: No breaking changes to existing AIKIT installations during migration
- **SC-008**: Package updates never break existing functionality (non-breaking updates only)

## Dependencies *(mandatory)*

- GitHub API access for package distribution and discovery
- Existing AIKIT agent configurations for all supported AI agents
- File system permissions for creating .aikit/ directory and modifying .gitignore
- Network connectivity for downloading packages

## Scope *(mandatory)*

### In Scope
- Universal package format (package.toml)
- Package lifecycle management (install, update, remove, list)
- Agent-specific command generation
- GitHub-based package distribution
- .gitignore automation
- Backward compatibility with spec-kit

### Out of Scope
- Package registry server (GitHub is sufficient)
- GUI package manager interface
- Package signing/verification
- Cross-package dependency resolution (packages are self-contained)
- Offline package installation

## Implementation Impact *(mandatory)*

### Breaking Changes
- Spec-kit specific logic will be removed from core AIKIT
- Some CLI command behaviors may change for backward compatibility

### Migration Path
- Phase 1: Dual support (old + new systems coexist)
- Phase 2: Spec-kit becomes a package
- Phase 3: Complete migration to package system

### Performance Impact
- Package downloads may increase network usage (no specific performance targets)
- .aikit/ directory will consume additional disk space
- Command generation adds startup time for AI agents

### Update Stability
- Non-breaking update policy ensures system stability
- Users are protected from unexpected breaking changes
- Clear warnings for potentially breaking updates

### Security Considerations
- Packages are trusted template collections - no executable security sandboxing required
- .gitignore automation requires file system write access
- GitHub token may be required for private repositories

## Clarifications

### Session 2025-12-14
- Q: Package Dependency Resolution Strategy → A: No dependencies (packages are self-contained)
- Q: CLI Command Conflict Resolution → A: Package namespace prefixes (e.g., `writing-assistant.generate`, `spec-kit.specify`)
- Q: Package Update Strategy When Updates Break Compatibility → B: Non-breaking updates only (warn about breaking changes)
- Q: Security Model for Package Installation → A: Full trust (execute packages without restrictions)
- Q: Performance Targets for Package Operations → C: No specific targets (performance as-is)

## Open Questions

- Should packages support pre/post-install scripts?
- Should there be a central package index beyond GitHub search?
