# AIKIT - Universal Package Manager for AI Agent Extensions

AIKIT is a universal package manager for AI agent extensions. Create, share, and discover reusable AI commands and templates across 17+ AI assistants including Claude, Cursor, GitHub Copilot, and Gemini.

## Installation

### Linux (GNU)
```bash
curl -L https://github.com/goaikit/aikit/releases/latest/download/aikit-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv aikit /usr/local/bin/
```

### Linux (MUSL/Universal)
```bash
curl -L https://github.com/goaikit/aikit/releases/latest/download/aikit-x86_64-unknown-linux-musl.tar.gz | tar xz
sudo mv aikit /usr/local/bin/
```

### Homebrew (Linux)
```bash
brew install goaikit/cli/aikit
```

### Scoop (Windows)
```powershell
scoop bucket add goaikit https://github.com/goaikit/scoop-bucket
scoop install aikit
```

### Windows
Download from [GitHub Releases](https://github.com/goaikit/aikit/releases/latest) and add to PATH.

## Quick Start

```bash
# Create a new AI-powered project
aikit init my-project --ai claude

# Install AI commands from the community
aikit install username/package-name

# See what AI assistants are available
aikit check

# List installed packages
aikit list
```

## Commands

| Command | Description |
|---------|-------------|
| `aikit init <name>` | Create new project with AI assistant templates |
| `aikit install <pkg>` | Install packages from GitHub or local directories |
| `aikit list` | Show installed packages |
| `aikit update <pkg>` | Update a package to latest version |
| `aikit remove <pkg>` | Uninstall a package |
| `aikit check` | Check available AI assistants |
| `aikit version` | Show version |

## Creating Packages

```bash
# Create a new package
aikit package init my-tools

# Build and publish to GitHub
aikit package build
aikit package publish username/my-tools
```

## Configuration

Create a `.env` file in your project for GitHub authentication:

```bash
GITHUB_TOKEN=your_github_token_here
```

Or use the `--token` flag when installing packages.

## Supported AI Assistants

AIKIT supports 17+ AI assistants:

**CLI-Based:** Claude, Gemini, Qwen, OpenCode, Codex, Auggie, CodeBuddy, Qoder, Q, Amp, Shai

**IDE-Based:** GitHub Copilot, Cursor, Windsurf, KiloCode, Roo, Bob

Run `aikit check` to see which are installed on your system.

## License

Apache License 2.0 - See [LICENSE](LICENSE)

Need help? [Open an issue](https://github.com/goaikit/aikit/issues)
