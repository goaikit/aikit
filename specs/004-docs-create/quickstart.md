# AIKit Documentation Examples: Working Code Samples

**Date**: 2025-01-06
**Purpose**: Provide accurate, runnable examples for AIKit documentation

## Configuration Examples (TOML-based)

### Primary Configuration File

```toml
# .aikit/config.toml
version = "1.0"
install_dir = ".aikit"

[registry]
default_registry = "github"

[preferences]
auto_gitignore = true
verbose = false
network_timeout = 30

[agents.claude]
name = "Claude Code"
key = "claude"
folder = ".claude"
requires_cli = true
output_format = "Markdown"
output_dir = ".claude/commands"
arg_placeholder = "$ARGUMENTS"

[agents.cursor]
name = "Cursor"
key = "cursor"
folder = ".cursor"
requires_cli = false
output_format = "Markdown"
output_dir = ".cursor/commands"
arg_placeholder = "{args}"
```

## CLI Command Examples

### Project Initialization

```bash
# Initialize a new project with Claude Code
aikit init my-claude-project --ai claude

# Initialize in current directory with Cursor
aikit init --here --ai cursor

# Interactive project setup
aikit init my-project
```

### Package Management

```bash
# Create a new package
aikit package init my-tools --description "My AI development tools"

# Build the package
cd my-tools
aikit package build

# Publish to GitHub (requires GITHUB_TOKEN)
aikit package publish myusername/my-tools
```

### Package Installation and Management

```bash
# Install from GitHub
aikit install owner/package-name

# Install from local directory
aikit install .

# List installed packages
aikit list

# Update a package
aikit update package-name

# Remove a package
aikit remove package-name
```

### Search and Discovery

```bash
# Search for packages
aikit search "testing tools"

# Detailed search results
aikit search "code review" --detailed --limit 10
```

### Utility Commands

```bash
# Check available AI agents
aikit check

# Show version information
aikit version
```

## Prerequisites for Examples

### GitHub Token Setup
Many examples require a GitHub token for API access:

```bash
# Set environment variable
export GITHUB_TOKEN=your_github_token_here

# Or create .env file
echo "GITHUB_TOKEN=your_github_token_here" > .env
```

### Local Package Development
For local package installation, ensure the directory contains `aikit.toml`:

```bash
# Required structure for local packages
my-package/
├── aikit.toml      # Package metadata
└── templates/      # Agent-specific templates
    ├── claude/
    └── cursor/
```

