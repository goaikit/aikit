# AIKIT - Rust Spec Kit CLI

A complete Rust reimplementation of the GitHub Spec Kit CLI tool, providing behaviorally identical functionality to the Python-based `specify` command.

## Features

- **Initialize Projects**: Bootstrap new Spec-Driven Development projects with AI agent templates
- **Tool Checking**: Verify installed tools and AI agent CLIs
- **Package Generation**: Build template zip archives for GitHub releases
- **Release Management**: Create GitHub releases with package files
- **Cross-Platform**: Works on Linux, macOS, and Windows
- **Interactive TUI**: Arrow-key navigation for agent selection

## Installation

### From Source

```bash
git clone <repository-url>
cd aikit
cargo build --release
```

The binary will be available at `target/release/aikit`.

## Usage

### Initialize a New Project

```bash
# Initialize with default agent (Copilot)
aikit init my-project

# Initialize with specific agent
aikit init my-project --ai claude

# Initialize in current directory
aikit init --here

# Initialize with PowerShell scripts
aikit init my-project --script ps
```

### Check Installed Tools

```bash
# Check all tools and agents
aikit check
```

### Display Version

```bash
# Show version information
aikit version
```

### Package Templates

```bash
# Generate packages for all agents/scripts
aikit package v1.0.0

# Filter by agents
AGENTS=claude,gemini aikit package v1.0.0

# Filter by script types
SCRIPTS=sh aikit package v1.0.0
```

### Create Release

```bash
# Create GitHub release with packages
aikit release v1.0.0

# With custom release notes
aikit release v1.0.0 --notes-file CHANGELOG.md
```

## Supported AI Agents

AIKIT supports 17 AI agents:

- **CLI-based**: claude, gemini, qwen, opencode, codex, auggie, codebuddy, qoder, q, amp, shai
- **IDE-based**: copilot, cursor-agent, windsurf, kilocode, roo, bob

## Script Types

- **Bash** (`.sh`): Default on Unix-like systems
- **PowerShell** (`.ps1`): Default on Windows

## Options

### Global Options

- `--debug`: Enable verbose diagnostic output

### Init Command Options

- `--ai <AGENT>`: Specify AI agent to use
- `--script <sh|ps>`: Specify script type
- `--here`: Initialize in current directory
- `--force`: Skip confirmation prompts
- `--no-git`: Skip Git repository initialization
- `--ignore-agent-tools`: Skip CLI tool validation
- `--github-token <TOKEN>`: GitHub token for API requests
- `--skip-tls`: Skip TLS certificate verification (unsafe, for troubleshooting)

## Development

### Building

```bash
cargo build
```

### Testing

```bash
cargo test
```

### Code Quality

```bash
# Format code
cargo fmt

# Run clippy
cargo clippy -- -D warnings

# Run all checks
cargo fmt --check && cargo build --release && cargo clippy -- -D warnings && cargo test
```

## Project Structure

```
aikit/
├── src/
│   ├── cli/          # CLI command implementations
│   ├── core/         # Core business logic
│   ├── fs/           # File system operations
│   ├── github/       # GitHub API client
│   └── tui/          # Terminal UI components
├── specs/            # Specification documents
└── Cargo.toml        # Rust project manifest
```

## License

MIT

## Author

@aroff

## See Also

- [Specification](./specs/002-rust-spec-kit-complete/spec.md)
- [Implementation Plan](./specs/002-rust-spec-kit-complete/plan.md)
- [Quick Start Guide](./specs/002-rust-spec-kit-complete/quickstart.md)

