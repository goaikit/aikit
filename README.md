# AIKIT - Universal Package Manager for AI Agent Extensions

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

AIKIT is a universal package manager for AI agent extensions that enables developers to create, share, and discover reusable AI commands and templates. It streamlines AI-powered development workflows by providing a standardized way to package and distribute AI tools across 17+ AI assistants including Claude, Cursor, GitHub Copilot, and Gemini. The tool reduces setup time, ensures consistency across projects, and fosters a community-driven ecosystem of AI development resources.

## Features

### Package Management System
- Create packages with reusable AI commands and templates
- Share packages via GitHub with one command
- Discover and install community packages
- **Local development**: Install packages from local directories (`aikit install .`)
- **Hierarchical discovery**: Automatically finds `.aikit` directory in parent folders
- Universal compatibility with 17+ AI agents

### Project Templates
- One-command setup for new AI-powered projects
- Support for 17 AI assistants (Claude, Cursor, Copilot, Gemini, etc.)
- Interactive selection and automatic Git initialization
- Cross-platform support (Windows, Linux)

### Technical Features
- **Environment configuration**: Automatic `.env` file loading for GitHub tokens
- **Smart template loading**: Loads actual template content, not placeholders
- **Correct command placement**: Agent commands created at project root level
- Type-safe Rust implementation with comprehensive error handling
- Fast performance: <30s package installation, <1s command generation
- Extensible architecture for custom packages and agents
- Backward compatible with existing functionality

## Installation

### Option 1: Release Binaries (Recommended)
Download the latest release from [GitHub Releases](https://github.com/goaikit/aikit/releases):
- Linux GNU: `aikit-x86_64-unknown-linux-gnu.tar.gz`
- Linux MUSL: `aikit-x86_64-unknown-linux-musl.tar.gz`

Extract and add to your PATH, then make executable on Unix systems.

### Option 2: Homebrew (Linux)
```bash
brew install goaikit/cli/aikit
```

### Option 3: Scoop (Windows)
```powershell
scoop bucket add goaikit https://github.com/goaikit/scoop-bucket
scoop install aikit
```

### Configuration
Create a `.env` file in your project root for GitHub authentication:
```bash
# .env
GITHUB_TOKEN=your_github_token_here
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
aikit install user/cool-package                     # Install from GitHub
aikit install .                                     # Install from local directory
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

# Local development workflow
cd my-package && aikit install .           # Install from local directory
aikit list                                 # Verify installation
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

### `aikit install` - Install Packages
Install AIKIT packages from GitHub or local directories.

```bash
aikit install user/package-name              # Install from GitHub
aikit install .                             # Install from current directory
aikit install --token TOKEN user/package    # Use specific GitHub token
```

Options: `--version <ver>`, `--token <token>`, `--force`, `--yes`

### `aikit list` - List Installed Packages
Show all installed AIKIT packages.

```bash
aikit list
aikit list --author username               # Filter by author
aikit list --detailed                      # Show detailed information
```

### `aikit update` - Update Packages
Update installed packages to their latest versions.

```bash
aikit update package-name
```

### `aikit remove` - Remove Packages
Uninstall AIKIT packages.

```bash
aikit remove package-name
aikit remove package-name --force          # Skip confirmation
```

### `aikit search` - Search Packages
Discover packages in the AIKIT ecosystem.

```bash
aikit search "testing"
aikit search "code review"
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
- **GitHub token**: Create `.env` file with `GITHUB_TOKEN=your_token` or use `--token` flag
- Verify GitHub access for private repositories

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
# Edit aikit.toml and add templates
aikit package build
aikit package publish yourusername/code-review-tools
```

### Using Community Packages
```bash
aikit search "testing"
aikit install awesome-org/test-helpers    # Install from GitHub
aikit install .                          # Install from local directory
aikit list                               # Show installed packages
aikit update test-helpers                # Update package
aikit remove old-package                 # Remove package
```

### Local Package Development
```bash
# Develop packages locally
aikit package init my-tools
cd my-tools
# Edit aikit.toml and templates/
aikit install .                         # Test installation locally
aikit package build                     # Build for distribution
aikit package publish myorg/my-tools    # Publish to GitHub
```

**Notes:**
- `.aikit` directory is automatically discovered by searching up the directory hierarchy
- Local installation supports both `aikit.toml` and `package.toml` configuration files
- GitHub authentication via `.env` file, environment variables, or `--token` flag
- Agent command files are created at the project root level (same level as `.aikit`)
- Templates are loaded with actual content, not placeholder text

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

- Version: `aikit --version` or `aikit version`
- Available tools: `aikit check`
- Installed packages: `aikit list`
- Command help: `aikit <command> --help`
- Package discovery: `aikit search "keyword"`

## License

Apache License, Version 2.0 - See [LICENSE](LICENSE) file for details.

Need help? Open an issue on [GitHub](https://github.com/goaikit/aikit/issues).
# Test release trigger
