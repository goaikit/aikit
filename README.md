# AIKIT - Universal Package Manager for AI Agent Extensions

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

AIKIT is a universal package manager for AI agent extensions that enables developers to create, share, and discover reusable AI commands and templates. It streamlines AI-powered development workflows by providing a standardized way to package and distribute AI tools across 17+ AI assistants including Claude, Cursor, GitHub Copilot, and Gemini. The tool reduces setup time, ensures consistency across projects, and fosters a community-driven ecosystem of AI development resources.

## Features

### Package Management System
- Create packages with reusable AI commands and templates
- Share packages via GitHub with one command
- Discover and install community packages
- Universal compatibility with 17+ AI agents

### Project Templates
- One-command setup for new AI-powered projects
- Support for 17 AI assistants (Claude, Cursor, Copilot, Gemini, etc.)
- Interactive selection and automatic Git initialization
- Cross-platform support (Windows, Linux)

### Technical Features
- Type-safe Rust implementation with comprehensive error handling
- Fast performance: <30s package installation, <1s command generation
- Extensible architecture for custom packages and agents
- Backward compatible with existing functionality

## Installation

### Option 1: Release Binaries (Recommended)
Download the latest release from [GitHub Releases](https://github.com/goaikit/aikit/releases):
- Windows: `aikit-x86_64-pc-windows-msvc.zip`
- Linux: `aikit-x86_64-unknown-linux-gnu.zip`

Extract and add to your PATH, then make executable on Unix systems.

### Option 2: Homebrew (Linux)
```bash
brew install goaikit/aikit/aikit
```

### Option 3: Scoop (Windows)
```powershell
scoop bucket add goaikit https://github.com/goaikit/scoop-bucket
scoop install aikit
```

### Verify Installation
```bash
aikit version
```

## Quick Start

### 1-Line Examples
```bash
aikit init my-project --ai claude                    # Start Claude project
aikit package init my-tools --description "Tools"   # Create package
aikit search "testing"                              # Find packages
aikit install user/cool-package                     # Install package
aikit list                                          # Show installed packages
```

### Package Ecosystem
```bash
# Create and publish a package
aikit package init my-tools --description "My AI development tools"
cd my-tools && aikit package build
aikit package publish myorg/my-tools

# Discover and install community packages
aikit search "code analysis"
aikit install myorg/useful-tools
```

### Project Templates
```bash
# Interactive project setup
aikit init my-project

# Direct setup with specific AI assistant
aikit init my-project --ai claude
cd existing-project && aikit init --here --ai copilot
```

### Check Available Tools
```bash
aikit check    # See which AI assistants are available
```

## Commands

### `aikit init` - Create New Projects
Creates project folders with AI assistant templates and configuration.

```bash
aikit init project-name                    # Interactive setup
aikit init project-name --ai claude       # Specific AI assistant
aikit init --here --ai copilot           # Setup in current folder
```

Options: `--ai <name>`, `--here`, `--force`, `--no-git`, `--script <sh|ps>`

### `aikit check` - Check Available Tools
Shows which AI assistants and development tools are installed.

```bash
aikit check
```

### `aikit version` - Check Version
Displays current AIKIT version and checks for updates.

```bash
aikit version
```

## Troubleshooting

### Command Not Found
- Ensure binary is in your PATH
- Make executable on Unix: `chmod +x aikit`

### Template Download Issues
- Check internet connection
- Try GitHub token: `--github-token YOUR_TOKEN`
- Verify GitHub access

### Agent Not Found
- Use `aikit check` to see available assistants
- Use correct case (e.g., `claude`, not `Claude`)
- Run `aikit init` interactively for menu

### Permission Errors
- Make file executable: `chmod +x aikit`
- Check write permissions in target folder

## Package Management

### Creating Packages
```bash
aikit package init code-review-tools --description "AI-powered code review tools"
cd code-review-tools
# Edit package.toml and add templates
aikit package build
aikit package publish yourusername/code-review-tools
```

### Using Community Packages
```bash
aikit search "testing"
aikit install awesome-org/test-helpers
aikit list
aikit update test-helpers
aikit remove old-package
```

## Supported AI Assistants

AIKIT works with 17+ AI coding assistants for both packages and project templates.

### Universal Package Support
All packages work with: Claude Code, Cursor, GitHub Copilot, Google Gemini, Continue, Windsurf, KiloCode, Roo Code, Bolt.new, Lovable, V0, Grok, Aider, OpenRouter, Marvin, Cody, and more.

### Template Support
CLI-Based: Claude, Gemini, Qwen, OpenCode, Codex, Auggie, CodeBuddy, Qoder, Q, Amp, Shai
IDE-Based: GitHub Copilot, Cursor Agent, Windsurf, KiloCode, Roo, Bob

Run `aikit check` to see available assistants.

### Script Types
- Bash (.sh) - Default on Linux
- PowerShell (.ps1) - Default on Windows
Override with `--script` option.

## Getting Help

- Version: `aikit version`
- Available tools: `aikit check`
- Command help: `aikit <command> --help`

## License

Apache License, Version 2.0 - See [LICENSE](LICENSE) file for details.

Need help? Open an issue on [GitHub](https://github.com/goaikit/aikit/issues).
