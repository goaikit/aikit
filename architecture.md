# AIKIT Architecture Documentation

## Overview

AIKIT is a complete Rust reimplementation of the GitHub Spec Kit CLI tool, providing behaviorally identical functionality to the Python-based `specify` command. The application is designed as a modular CLI tool that bootstraps Spec-Driven Development (SDD) projects with AI agent templates, manages tool checking, and handles GitHub release packaging.

**Key Characteristics:**
- **Type**: Command-line interface (CLI) application
- **Language**: Rust (edition 2021, MSRV 1.75)
- **Paradigm**: Modular, functional with async support
- **Distribution**: Single binary executable

## System Architecture

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    User Interface                        │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌─────────┐│
│  │   CLI    │  │   TUI    │  │  Output  │  │  Error  ││
│  │  (clap)  │  │(ratatui) │  │ Formatting│ │ Handling││
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬────┘│
└───────┼──────────────┼──────────────┼─────────────┼─────┘
        │              │              │             │
        └──────────────┼──────────────┼─────────────┘
                       │              │
        ┌──────────────▼──────────────▼──────────────┐
        │           Command Layer (src/cli/)          │
        │  init │ check │ version │ package │ release │
        └───────┬────────────────────────────────────┘
                │
        ┌───────▼──────────────────────────────────────┐
        │         Core Business Logic                  │
        │  ┌──────────┐  ┌──────────┐  ┌──────────┐  │
        │  │  Agent   │  │ Template │  │  Package │  │
        │  │  Config  │  │ Download │  │ Generation│ │
        │  └──────────┘  └──────────┘  └──────────┘  │
        │  ┌──────────┐  ┌──────────┐  ┌──────────┐  │
        │  │   Git    │  │  Tools  │  │   FS     │  │
        │  │  Ops     │  │  Check  │  │  Utils   │  │
        │  └──────────┘  └──────────┘  └──────────┘  │
        └───────┬──────────────────────────────────────┘
                │
        ┌───────▼──────────────────────────────────────┐
        │      External Services & File System          │
        │  ┌──────────┐  ┌──────────┐  ┌──────────┐  │
        │  │ GitHub   │  │   Git    │  │   Local  │  │
        │  │   API    │  │  Repo    │  │   FS     │  │
        │  └──────────┘  └──────────┘  └──────────┘  │
        └──────────────────────────────────────────────┘
```

### Core Components

#### CLI Module (`src/cli/`)

**Purpose**: Command-line interface implementation and command dispatch

**Responsibilities**:
- Parse command-line arguments using `clap`
- Route commands to appropriate handlers
- Handle global flags (`--debug`)
- Provide command-specific argument structures

**Key Files**:
- `mod.rs`: Main CLI structure and command routing
- `init.rs`: Project initialization command
- `check.rs`: Tool checking command
- `version.rs`: Version display command
- `package.rs`: Package generation command
- `release.rs`: Release creation command

**Interfaces**:
- Commands are async functions that return `anyhow::Result<()>`
- All commands follow the pattern: `pub async fn execute(args: CommandArgs) -> Result<()>`

#### Core Module (`src/core/`)

**Purpose**: Core business logic and domain models

**Responsibilities**:
- Agent configuration management (17 AI agents)
- Template download and extraction
- Git repository operations
- Package generation logic
- Tool detection and validation

**Key Components**:

1. **Agent Configuration** (`agent.rs`):
   - Defines all 17 supported AI agents
   - Manages agent metadata (key, name, folder, install URL, etc.)
   - Validates agent keys
   - Handles script variant selection (Bash/PowerShell)

2. **Template Management** (`template.rs`):
   - Downloads templates from GitHub releases
   - Extracts and flattens ZIP archives
   - Validates template assets
   - Manages project path validation

3. **Git Operations** (`git.rs`):
   - Repository initialization
   - Repository detection
   - Initial commit creation
   - Branch name validation (GitHub's 244-byte limit)

4. **Package Generation** (`package.rs`):
   - Loads command templates
   - Processes YAML frontmatter
   - Replaces placeholders in templates
   - Generates ZIP archives for releases

5. **Tool Detection** (`tools.rs`):
   - Checks tool availability on PATH
   - Special handling for Claude CLI (`~/.claude/local/claude`)
   - Validates agent CLI requirements

#### File System Module (`src/fs/`)

**Purpose**: Cross-platform file system operations

**Responsibilities**:
- Directory copying with exclusions
- File merging (especially JSON deep merge)
- Cross-platform path handling
- File permission management

**Key Features**:
- **Deep JSON Merge** (`merge.rs`): Merges nested JSON objects, replaces arrays, preserves existing values
- **Cross-Platform Paths** (`mod.rs`): Tilde expansion, path normalization, consistent path display
- **Permissions** (`permissions.rs`): Unix execute permission setting for `.sh` files

#### GitHub Module (`src/github/`)

**Purpose**: GitHub API integration

**Responsibilities**:
- Release and asset downloads
- Authentication handling
- Rate limit detection and error formatting
- API request/response management

**Key Components**:
- **API Client** (`api.rs`): HTTP client using `reqwest`, token resolution, request building
- **Rate Limiting** (`rate_limit.rs`): Detects rate limit errors, formats user-friendly error messages

#### TUI Module (`src/tui/`)

**Purpose**: Interactive terminal user interface

**Responsibilities**:
- Interactive agent selection with arrow keys
- Formatted output (tree structures, panels)
- Terminal rendering using `ratatui` and `crossterm`

**Key Components**:
- **Agent Selection** (`agent_select.rs`): Interactive list with navigation, selection, cancellation
- **Output Formatting** (`output.rs`): Tree structures, formatted text output

## Data Flow

### Initialization Flow

```
User Input (CLI args)
  ↓
ProjectPath (validation)
  ↓
AgentSelection (resolve agent - interactive or default)
  ↓
TemplateAsset (download from GitHub)
  ↓
TemplateAsset (extract to temp)
  ↓
FileSystem (merge/copy to target)
  ↓
GitRepository (initialize if needed)
  ↓
ProjectPath (finalize)
```

### Package Generation Flow

```
PackageConfig (validate)
  ↓
AgentConfig list (filter if needed)
  ↓
CommandTemplate list (load templates)
  ↓
CommandTemplate (process placeholders)
  ↓
FileSystem (copy base directories)
  ↓
FileSystem (write command files)
  ↓
ZipArchive (create zip files)
  ↓
PackageConfig (output to .genreleases/)
```

### Release Creation Flow

```
ReleaseArgs (validate version)
  ↓
PackageFile discovery (.genreleases/)
  ↓
GitHub CLI detection
  ↓
gh release create (with assets)
  ↓
Release created
```

## Design Decisions

### Decision: Rust Implementation

**Context**: Original tool is Python-based, but Rust offers better performance and type safety.

**Decision**: Complete Rust reimplementation with 100% functional parity.

**Rationale**:
- Better performance (compiled binary vs interpreted)
- Strong type safety catches errors at compile time
- Single binary distribution (no Python runtime required)
- Better memory safety guarantees
- Cross-platform support built-in

**Alternatives Considered**:
- Keep Python: Maintains existing codebase but slower and requires Python runtime
- Hybrid approach: Too complex, maintenance burden

**Consequences**:
- Requires Rust knowledge for contributors
- Longer compile times during development
- Better runtime performance for end users

### Decision: Modular Architecture

**Context**: Need to organize code for maintainability and testability.

**Decision**: Split into logical modules (cli, core, fs, github, tui).

**Rationale**:
- Clear separation of concerns
- Easy to test individual components
- Follows Rust best practices
- Makes codebase navigable

**Alternatives Considered**:
- Monolithic structure: Harder to maintain and test
- Microservices: Overkill for CLI application

**Consequences**:
- Clear module boundaries
- Easy to locate functionality
- Some cross-module dependencies (acceptable)

### Decision: Async Runtime (Tokio)

**Context**: Need to handle async operations (HTTP requests, file I/O).

**Decision**: Use Tokio for async runtime, but keep most operations synchronous where possible.

**Rationale**:
- Required for `reqwest` HTTP client
- Allows non-blocking I/O for better performance
- Standard Rust async solution

**Alternatives Considered**:
- Synchronous only: Simpler but slower for network operations
- Other async runtimes: Tokio is the standard

**Consequences**:
- Some complexity in async/sync boundaries
- Better performance for network operations

### Decision: Deep JSON Merge for .vscode/settings.json

**Context**: When using `--here` flag, need to merge existing files rather than overwrite.

**Decision**: Implement deep merge for JSON files: nested objects merged, arrays replaced, scalars overwritten.

**Rationale**:
- Preserves user's existing VS Code settings
- Matches Python implementation behavior
- Better user experience

**Alternatives Considered**:
- Overwrite: Loses user settings
- Shallow merge: Doesn't handle nested objects properly

**Consequences**:
- More complex merge logic
- Better user experience

### Decision: Interactive TUI for Agent Selection

**Context**: When `--ai` flag not provided, need user-friendly agent selection.

**Decision**: Use `ratatui` for interactive arrow-key navigation.

**Rationale**:
- Better UX than typing agent names
- Matches Python Rich library behavior
- Standard Rust TUI solution

**Alternatives Considered**:
- Text prompt: Less user-friendly
- No interaction: Forces users to know agent keys

**Consequences**:
- Additional dependency (`ratatui`, `crossterm`)
- Better user experience

## Technology Stack

### Language and Runtime

- **Language**: Rust (edition 2021, MSRV 1.75)
- **Async Runtime**: Tokio 1.x
- **Build System**: Cargo

### Core Dependencies

- **CLI Framework**: `clap` 4.5 (argument parsing)
- **HTTP Client**: `reqwest` 0.12 (GitHub API)
- **Archive Handling**: `zip` 0.6 (ZIP extraction/creation)
- **Serialization**: `serde` 1.0, `serde_json` 1.0, `serde_yaml` 0.9
- **Git Operations**: `git2` 0.18 (libgit2 bindings)
- **Error Handling**: `anyhow` 1.0
- **TUI**: `ratatui` 0.27, `crossterm` 0.28
- **File System**: `walkdir` 2, `tempfile` 3.10
- **Utilities**: `which` 6.0 (tool detection), `atty` 0.2 (TTY detection), `toml` 0.8, `chrono` 0.4, `yaml-front-matter` 0.1

### Development Tools

- **Formatting**: `rustfmt` (via `rustfmt.toml`)
- **Linting**: `clippy` (via `.clippy.toml`)
- **Testing**: Built-in Rust test framework

## Data Model

### Key Entities

1. **AgentConfig**: Represents an AI agent configuration
   - Fields: key, name, folder, install_url, requires_cli, output_format, output_dir, arg_placeholder

2. **ProjectPath**: Represents a target project location
   - Fields: path, is_here, exists, is_empty
   - Methods: validate(), new()

3. **TemplateAsset**: Represents a downloadable template asset
   - Fields: filename, download_url, size, release_tag, agent, script_variant
   - Methods: from_filename(), validate()

4. **PackageConfig**: Represents packaging configuration
   - Fields: version, agents (filter), scripts (filter), output_dir
   - Methods: validate(), parse_agents_env(), parse_scripts_env()

5. **CommandTemplate**: Represents a command template file
   - Fields: name, description, script_commands, agent_script_commands, body, frontmatter
   - Methods: from_file(), generate_content(), output_filename()

### Data Storage

- **Configuration**: Hardcoded in Rust (agent configs)
- **Templates**: Downloaded from GitHub releases (ZIP archives)
- **Generated Files**: Written to project directories
- **Packages**: Generated in `.genreleases/` directory

## Security Architecture

### Authentication

- **GitHub API**: Uses personal access tokens (from CLI arg, `GH_TOKEN`, or `GITHUB_TOKEN` env vars)
- **Token Resolution**: Precedence order documented in code
- **Token Storage**: Never stored, only used for API requests

### Input Validation

- **Agent Keys**: Validated against known agent list
- **Version Strings**: Validated against semantic version pattern (vX.Y.Z)
- **Branch Names**: Validated against GitHub's 244-byte limit and invalid character rules
- **Paths**: Validated for existence and permissions

### File System Safety

- **Path Traversal**: Uses Rust's `Path` and `PathBuf` for safe path handling
- **Permission Setting**: Only sets execute permissions on `.sh` files with shebangs
- **File Merging**: Validates JSON before merging to prevent corruption

## Scalability and Performance

### Performance Optimizations

- **Compiled Binary**: Single optimized binary (release mode with LTO)
- **Async I/O**: Non-blocking network requests
- **Streaming**: ZIP extraction uses streaming where possible
- **Caching**: Template downloads cached in temp directories

### Scalability Considerations

- **Template Size**: Handles templates of various sizes
- **Package Generation**: Generates 34 packages per release (17 agents × 2 script types)
- **Concurrent Operations**: Uses async for parallelizable operations

### Known Limitations

- **Template Download**: Requires internet connection
- **GitHub Rate Limits**: 60 requests/hour unauthenticated, 5000/hour authenticated
- **Large Templates**: Memory usage scales with template size

## Deployment Architecture

### Build Process

1. **Development**: `cargo build` (debug mode)
2. **Release**: `cargo build --release` (optimized with LTO)
3. **Distribution**: Single binary executable

### Distribution

- **Source**: GitHub repository
- **Binaries**: (Future) GitHub releases with pre-built binaries
- **Platforms**: Linux, macOS, Windows

### Installation Methods

1. **From Source**: `cargo install --path .`
2. **From Crate** (future): `cargo install aikit`
3. **Binary Download** (future): Download from GitHub releases

## Future Considerations

### Planned Improvements

- **Performance**: Further optimization of template download/extraction
- **Error Messages**: More comprehensive error messages with actionable suggestions
- **Documentation**: Additional examples and tutorials
- **Testing**: More comprehensive integration tests
- **Cross-Platform**: Enhanced Windows support testing

### Technical Debt

- **TLS Skipping**: Currently not fully implemented (requires native-tls backend)
- **Error Handling**: Some error messages could be more actionable
- **Test Coverage**: Could be expanded for edge cases

### Known Limitations

- **GitHub CLI Required**: Release command requires `gh` CLI tool
- **Template Dependency**: Requires GitHub access for template downloads
- **Python Parity**: Some edge cases may need verification against Python version

