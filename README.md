# AIKIT - Universal Package Manager for AI Agent Extensions

[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

**AIKIT has evolved!** Now a universal package manager for AI agent extensions, plus your favorite project templates. Create, share, and discover AI tools that work across Claude, Cursor, GitHub Copilot, and 17+ other AI assistants.

## What is AIKIT?

AIKIT is a comprehensive ecosystem for AI-powered development:

### 🆕 **Universal Package System** (New!)
- **Create packages** with reusable AI commands and templates
- **Share packages** via GitHub with one command
- **Discover packages** from the community
- **Install packages** that work across all major AI agents

### 📁 **Project Templates** (Original)
- **17 AI assistants** supported (Claude, Cursor, Copilot, Gemini, etc.)
- **One-command setup** for new projects
- **Battle-tested templates** for rapid development

## Features

### 🆕 Package Management System
- **📦 Create**: `aikit package init` - Build packages with TOML metadata
- **🔍 Discover**: `aikit search` - Find packages on GitHub
- **⬇️ Install**: `aikit install <repo>` - One-command installation
- **📋 Manage**: `aikit list/update/remove` - Full lifecycle management
- **🚀 Publish**: `aikit package publish` - Share via GitHub releases
- **🤖 Universal**: Works with 17+ AI agents (Claude, Cursor, Copilot, Gemini, etc.)

### 📁 Project Templates (Original)
- **17 AI Assistants**: Claude, Cursor, Copilot, Gemini, Continue, Windsurf, etc.
- **One-Command Setup**: `aikit init my-project`
- **Interactive Selection**: Choose your preferred AI assistant
- **Git Integration**: Automatic repository initialization
- **Cross-Platform**: Windows, macOS, Linux support

### 🔧 Technical Excellence
- **Type-Safe**: Full Rust implementation with comprehensive error handling
- **Performance**: <30s package installation, <1s command generation
- **Extensible**: Clean architecture for custom packages and agents
- **Backward Compatible**: Existing functionality preserved

## Installation

### Quick Install (Recommended)

**Download the latest release:**
1. Visit the [Releases page](https://github.com/aroff/aikit/releases)
2. Download the binary for your operating system:
   - **Windows**: `aikit-x86_64-pc-windows-msvc.zip`
   - **macOS**: `aikit-x86_64-apple-darwin.zip` or `aikit-aarch64-apple-darwin.zip` (for Apple Silicon)
   - **Linux**: `aikit-x86_64-unknown-linux-gnu.zip`
3. Extract the zip file
4. Move the `aikit` executable to a folder in your PATH (like `/usr/local/bin` on macOS/Linux or `C:\Windows\System32` on Windows)
5. Make it executable (macOS/Linux): `chmod +x aikit`

### Verify Installation

Open a terminal and run:

```bash
aikit version
```

If you see version information, you're all set!

## Quick Start

### 🆕 Package Ecosystem (Recommended)

#### Create Your First Package
```bash
# Initialize a new package
aikit package init my-tools --description "My AI development tools"

# Build it
cd my-tools
aikit package build

# Publish to GitHub
aikit package publish myorg/my-tools
```

#### Discover & Install Packages
```bash
# Search for packages
aikit search "code analysis"

# Install from GitHub
aikit install myorg/useful-tools

# Manage your packages
aikit list
aikit update useful-tools
```

### 📁 Traditional Project Setup

#### Create Your First Project
```bash
aikit init my-project
```

This will:
- Create a new folder called `my-project`
- Show you an interactive menu to choose your AI assistant
- Download and set up all the necessary files
- Initialize a Git repository (if Git is installed)

### Choose Your AI Assistant

AIKIT supports 17 different AI assistants:

**Popular Options:**
- **GitHub Copilot** - Built into VS Code and GitHub
- **Claude** - Anthropic's AI assistant
- **Cursor** - AI-powered code editor
- **Gemini** - Google's AI assistant

**Full List:**
Claude, Gemini, GitHub Copilot, Cursor Agent, Qwen, OpenCode, Codex, Windsurf, KiloCode, Auggie, Roo, CodeBuddy, Qoder, Amp, Shai, Q, Bob

### Common Use Cases

#### Start a New Project with Claude

```bash
aikit init my-project --ai claude
```

#### Start a Project in the Current Folder

```bash
cd existing-project
aikit init --here --ai copilot
```

#### Use PowerShell Scripts (Windows)

```bash
aikit init my-project --ai claude --script ps
```

#### Check What Tools You Have Installed

```bash
aikit check
```

This shows you which AI assistants and tools are available on your system.

## Commands

### `aikit init` - Create a New Project

Creates a new project folder with all the templates and configuration files you need.

**Basic Usage:**
```bash
aikit init project-name
```

**Options:**
- `--ai <name>` - Choose a specific AI assistant (e.g., `claude`, `copilot`, `gemini`)
- `--here` - Set up in the current folder instead of creating a new one
- `--force` - Skip confirmation prompts
- `--no-git` - Don't initialize a Git repository
- `--script <sh|ps>` - Choose script type (`sh` for Bash, `ps` for PowerShell)

**Examples:**
```bash
# Interactive setup (choose AI assistant from menu)
aikit init my-project

# Quick setup with Claude
aikit init my-project --ai claude

# Set up in current folder
aikit init --here --ai copilot

# Set up without Git
aikit init my-project --ai gemini --no-git
```

### `aikit check` - Check Your Tools

See which AI assistants and development tools are installed on your computer.

```bash
aikit check
```

This will show you:
- ✓ Which tools are found
- ✗ Which tools are missing
- Where tools are located

### `aikit version` - Check Version

See what version of AIKIT you're using and check for updates.

```bash
aikit version
```

## Troubleshooting

### "Command not found" Error

**Problem:** Terminal says `aikit` command not found.

**Solution:**
1. Make sure you downloaded the binary for your operating system
2. Make sure the `aikit` file is in a folder that's in your PATH
3. On macOS/Linux, make sure the file is executable: `chmod +x aikit`

### "Failed to download template" Error

**Problem:** AIKIT can't download templates from GitHub.

**Solutions:**
1. Check your internet connection
2. If you're behind a firewall, you may need to configure proxy settings
3. Try using a GitHub token: `aikit init my-project --github-token YOUR_TOKEN`
4. Check if GitHub is accessible: Visit https://github.com in your browser

### "Agent not found" Error

**Problem:** You specified an AI assistant that doesn't exist.

**Solution:**
- Use `aikit check` to see available assistants
- Make sure you're using the correct name (e.g., `claude`, not `Claude`)
- Run `aikit init` without `--ai` to see the interactive menu

### Template Download is Slow

**Problem:** Downloading templates takes a long time.

**Solutions:**
- This is normal for the first time - templates are downloaded from GitHub
- Subsequent projects in the same session may be faster
- Using a GitHub token can help: `aikit init my-project --github-token YOUR_TOKEN`

### Permission Errors on macOS/Linux

**Problem:** Getting "permission denied" errors.

**Solution:**
- Make the file executable: `chmod +x aikit`
- Make sure you have write permissions in the folder where you're creating the project

## What Gets Created?

When you run `aikit init`, it creates:

- **Project folder structure** - Organized directories for your AI assistant
- **Configuration files** - Settings for your chosen AI assistant
- **Script files** - Helper scripts for common tasks
- **Template files** - Starting templates for your workflow
- **Git repository** - Initialized Git repo (unless you use `--no-git`)

All files are ready to use immediately - no additional setup required!

## Package Usage Examples

### Creating a Custom Package
```bash
# Create a package for code review tools
aikit package init code-review-tools \
  --description "AI-powered code review and analysis tools" \
  --author "Your Name"

cd code-review-tools

# Edit package.toml and add templates
# Build your package
aikit package build

# Publish to GitHub
aikit package publish yourusername/code-review-tools
```

### Using Community Packages
```bash
# Discover useful packages
aikit search "testing"

# Install a testing package
aikit install awesome-org/test-helpers

# The package commands are now available in your AI agent:
# - test-helpers.generate-tests
# - test-helpers.analyze-coverage
# - test-helpers.mock-data
```

### Managing Your Package Collection
```bash
# List installed packages
aikit list

# List packages by author
aikit list --author "awesome-org"

# Update all packages
aikit list | xargs -I {} aikit update {}

# Remove unused packages
aikit remove old-package
```

## Supported AI Assistants

AIKIT works with **17+ AI coding assistants** across both package commands and project templates:

### 🤖 Universal Support (Package Commands)
All packages work seamlessly with:
- **Claude Code** - Anthropic's AI assistant
- **Cursor** - AI-first code editor
- **GitHub Copilot** - Microsoft's AI pair programmer
- **Google Gemini** - Multimodal AI assistant
- **Continue** - Open-source AI coding assistant
- **Windsurf** - AI-enhanced development environment
- **KiloCode** - AI coding companion
- **Roo Code** - AI development assistant
- **Bolt.new** - AI-powered web development
- **Lovable** - AI web development platform
- **V0** - AI UI generation
- **Grok** - xAI's helpful assistant
- **Aider** - AI pair programming tool
- **OpenRouter** - Universal AI API
- **Marvin** - AI for software development
- **Cody** - Sourcegraph's AI coding assistant
- **Plus more!** - Framework extensible for new assistants

### 📁 Template Compatibility
**CLI-Based** (require command-line tools):
- Claude, Gemini, Qwen, OpenCode, Codex, Auggie, CodeBuddy, Qoder, Q, Amp, Shai

**IDE-Based** (work in your code editor):
- GitHub Copilot, Cursor Agent, Windsurf, KiloCode, Roo, Bob

Use `aikit check` to see which assistants are available on your system.

## Script Types

AIKIT supports two script types:

- **Bash** (`.sh`) - Default on macOS and Linux
- **PowerShell** (`.ps1`) - Default on Windows

You can override the default with the `--script` option.

## Getting Help

- **Check version:** `aikit version`
- **See available tools:** `aikit check`
- **Get help:** Most commands support `--help` flag (e.g., `aikit init --help`)

## License

MIT License - See [LICENSE](LICENSE) file for details.

## Author

Created by [@aroff](https://github.com/aroff)

---

**Need help?** Open an issue on [GitHub](https://github.com/aroff/aikit/issues) or check the [documentation](https://github.com/aroff/aikit).
