# Quickstart: Creating AIKIT Packages

This guide will help you create your first AIKIT package - a reusable collection of templates, scripts, and commands that can be shared across different AI agents.

## What is an AIKIT Package?

An AIKIT package is a standardized bundle containing:
- **Templates**: Markdown files with commands and prompts for AI agents
- **Scripts**: Utility scripts for automation
- **Metadata**: Package information in `package.toml`
- **Documentation**: README and usage instructions

## Quick Start

### 1. Initialize a New Package

```bash
# Create a new package
aikit package init my-awesome-package

# Or with custom description and author
aikit package init my-awesome-package \
  --description "Tools for awesome development" \
  --author "Your Name"
```

This creates:
```
my-awesome-package/
├── package.toml      # Package metadata
├── README.md         # Documentation
├── templates/        # Command templates
│   └── help.md
├── scripts/          # Utility scripts
└── docs/            # Additional documentation
```

### 2. Customize Your Package

Edit `package.toml`:

```toml
[package]
name = "my-awesome-package"
version = "0.1.0"
description = "Tools for awesome development"
authors = ["Your Name"]

[commands]
analyze = { description = "Analyze code quality" }
refactor = { description = "Suggest refactoring improvements" }

[artifacts]
"templates/*.md" = ".aikit/templates/"
"scripts/*" = ".aikit/scripts/"
```

### 3. Create Command Templates

Add templates in the `templates/` directory:

```markdown
<!-- templates/analyze.md -->
# Code Analysis

**Description**: Analyze code quality and provide suggestions

**Usage**: Run `my-awesome-package.analyze` in your AI agent

## Analysis Results

- **Complexity**: {analysis_complexity}
- **Issues Found**: {analysis_issues}
- **Suggestions**: {analysis_suggestions}
```

### 4. Build Your Package

```bash
cd my-awesome-package
aikit package build
```

This creates `my-awesome-package-0.1.0.zip` ready for distribution.

### 5. Test Locally

```bash
# Install locally for testing
aikit install ./my-awesome-package-0.1.0.zip

# Or install from current directory
aikit install .
```

### 6. Publish and Share

1. Upload your ZIP to GitHub Releases
2. Share the GitHub URL: `https://github.com/yourusername/my-awesome-package/releases/download/v0.1.0/my-awesome-package-0.1.0.zip`
3. Others can install with: `aikit install <your-github-url>`

## Package Structure Details

### package.toml Format

```toml
[package]
name = "your-package-name"        # Required: lowercase, alphanumeric, hyphens, underscores
version = "1.0.0"                # Required: semantic versioning (X.Y.Z)
description = "What it does"     # Required: human-readable description
authors = ["Your Name"]          # Optional: package authors
license = "MIT"                  # Optional: license identifier
homepage = "https://..."         # Optional: project homepage
repository = "https://..."       # Optional: source repository

[commands]
# Define available commands
help = { description = "Show help" }
analyze = { description = "Analyze something", template = "analyze.md" }

[artifacts]
# Map source files to installation destinations
"templates/*.md" = ".aikit/templates/"    # Templates go here
"scripts/*" = ".aikit/scripts/"           # Scripts go here
"config/*" = ".aikit/config/"             # Config files

# Agent-specific overrides
[agents.claude]
script_template = "claude_script.sh"

[agents.cursor]
script_template = "cursor_script.ps1"
```

### Template Variables

Use these variables in your templates:

- `{package_name}` - Your package name
- `{command_description}` - Command description
- `{args}` - Argument placeholder (customized per agent)

### File Organization

```
your-package/
├── package.toml           # Package definition
├── README.md             # User documentation
├── LICENSE               # License file
├── templates/            # Command templates
│   ├── command1.md
│   ├── command2.md
│   └── help.md
├── scripts/              # Utility scripts
│   ├── setup.sh
│   └── utils.py
└── docs/                 # Additional docs
    └── api.md
```

## Best Practices

### Naming
- Use lowercase package names
- Separate words with hyphens: `code-analyzer`
- Keep names descriptive but concise

### Commands
- Start with action verbs: `analyze`, `generate`, `refactor`
- Use consistent naming across packages
- Provide clear descriptions

### Templates
- Use consistent formatting
- Include usage examples
- Handle edge cases gracefully

### Versioning
- Follow semantic versioning
- Increment major for breaking changes
- Increment minor for new features
- Increment patch for bug fixes

## Advanced Features

### Agent-Specific Overrides

Customize behavior for different AI agents:

```toml
[agents.claude]
script_template = "claude_setup.sh"
artifacts = { "claude-config/*" = ".claude/config/" }

[agents.cursor]
script_template = "cursor_setup.ps1"
arg_format = "--args={args}"
```

### Dependencies

Currently, packages are self-contained. Future versions may support dependencies.

### Validation

Always validate your package before publishing:

```bash
aikit package validate
```

## Troubleshooting

### Common Issues

**"Package name contains invalid characters"**
- Use only lowercase letters, numbers, hyphens, and underscores
- Example: `my-package` ✅, `My Package` ❌

**"Template file not found"**
- Ensure template files exist in the `templates/` directory
- Check file paths in `package.toml`

**"Version must follow semantic versioning"**
- Use format: `MAJOR.MINOR.PATCH` (e.g., `1.2.3`)
- All numbers must be non-negative integers

### Getting Help

- Check existing packages for examples
- Validate your `package.toml` syntax
- Test locally before publishing

## Examples

### Simple Code Review Package

```bash
aikit package init code-review --description "AI-powered code review tools"
```

**package.toml:**
```toml
[package]
name = "code-review"
version = "1.0.0"
description = "AI-powered code review tools"
authors = ["Dev Team"]

[commands]
review = { description = "Review code changes" }
suggest = { description = "Suggest improvements" }

[artifacts]
"templates/*.md" = ".aikit/templates/"
```

### Multi-Agent Writing Assistant

```bash
aikit package init writing-assistant --description "Writing tools for AI agents"
```

**package.toml:**
```toml
[package]
name = "writing-assistant"
version = "1.0.0"
description = "Writing tools for AI agents"

[commands]
grammar = { description = "Check grammar and style" }
outline = { description = "Generate document outline" }

[artifacts]
"templates/*.md" = ".aikit/templates/"

[agents.claude]
script_template = "claude_writing.sh"

[agents.cursor]
script_template = "cursor_writing.ps1"
```

## Next Steps

1. Create your first package using this guide
2. Join the AIKIT community to share your packages
3. Contribute improvements to the package system
4. Explore advanced features as they're added

Happy packaging! 📦✨
