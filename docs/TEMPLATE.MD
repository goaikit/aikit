# Creating AIKIT Template Packages

This guide explains how to create and publish AIKIT packages - reusable AI agent extensions that work across multiple AI assistants like Claude, Cursor, GitHub Copilot, and others.

## Overview

AIKIT packages are self-contained extensions that provide:
- **Commands**: Reusable AI prompts and workflows
- **Templates**: Structured content templates
- **Scripts**: Automation scripts
- **Documentation**: Usage guides and examples

Packages are distributed via GitHub and installed with a single command.

## Quick Start

### 1. Initialize a Package

```bash
aikit package init my-package \
  --description "My AI development tools" \
  --author "Your Name"
```

This creates:
```
my-package/
├── aikit.toml      # Package metadata and configuration
├── README.md         # Package documentation
├── templates/        # Template files for commands
│   └── help.md      # Example template
├── scripts/         # Automation scripts (optional)
└── docs/           # Additional documentation (optional)
```

### 2. Edit Package Configuration

The `aikit.toml` file defines your package:

```toml
[package]
name = "my-package"
version = "0.1.0"
description = "My AI development tools"
authors = ["Your Name"]

[commands.help]
description = "Show help information"
template = "help.md"

[artifacts]
"templates/*.md" = ".aikit/templates/"
"scripts/*" = ".aikit/scripts/"
```

### 3. Create Command Templates

Templates use Markdown with variable substitution. Create `templates/help.md`:

```markdown
# Help Command

This is a sample command for the {{package_name}} package.

**Description**: {{command_description}}

**Usage**: Run this command to get help information.

## Available Commands

- `{{package_name}}.help` - Show this help message
- Add more commands as needed

## Installation

This package provides AI agent extensions for {{package_name}}.

## Configuration

No special configuration required.
```

### 4. Build and Test

```bash
cd my-package
aikit package build
aikit package validate
```

### 5. Publish to GitHub

```bash
aikit package publish yourusername/my-package
```

Users can now install with:
```bash
aikit install yourusername/my-package
```

## Package Structure

### Required Files

- **`aikit.toml`**: Package metadata and configuration
- **`README.md`**: Package description and usage
- **`templates/`**: At least one template file

### Optional Files

- **`scripts/`**: Automation scripts
- **`docs/`**: Additional documentation
- **`LICENSE`**: License file

## Package.toml Configuration

### Package Metadata

```toml
[package]
name = "my-package"                    # Required: unique identifier
version = "0.1.0"                      # Required: semantic version
description = "Brief description"      # Required: package purpose
authors = ["Your Name"]                 # Optional: package authors
license = "MIT"                        # Optional: license type
homepage = "https://example.com"       # Optional: project website
repository = "https://github.com/user/repo"  # Optional: source repository
```

### Commands

Define available commands in your package:

```toml
[commands.help]
description = "Show help information"
template = "help.md"                   # Optional: defaults to commands/{name}.md

[commands.analyze]
description = "Analyze code for issues"
template = "analyze.md"

[commands.refactor]
description = "Refactor code structure"
template = "refactor.md"
```

### Artifacts

Map package files to installation destinations:

```toml
[artifacts]
"templates/*.md" = ".aikit/templates/"    # Template files
"scripts/*" = ".aikit/scripts/"          # Script files
"docs/*" = ".aikit/docs/"                # Documentation
```

### Agent-Specific Overrides

Customize behavior for specific AI agents:

```toml
[agents.claude]
# Claude-specific settings
command_prefix = "claude-"

[agents.cursor]
# Cursor-specific settings
template_format = "cursor"
```

## Template Variables

Templates support variable substitution:

| Variable | Description |
|----------|-------------|
| `{{package_name}}` | Package name from aikit.toml |
| `{{command_description}}` | Command description |
| `{{command_name}}` | Current command name |

Example usage in templates:
```markdown
# {{command_name}}

{{command_description}}

Package: {{package_name}}
```

## Command Templates

### Basic Structure

Templates should include:
1. **Header**: Command title and description
2. **Usage**: How to use the command
3. **Parameters**: Input requirements
4. **Examples**: Sample usage
5. **Output**: Expected results

### Template Example

```markdown
# Code Review Command

**Description**: {{command_description}}

**Package**: {{package_name}}

## Usage

Select code and run this command to get a comprehensive review.

## What This Command Does

- Analyzes code quality and style
- Identifies potential bugs and issues
- Suggests improvements and optimizations
- Provides actionable feedback

## Parameters

- **Code Selection**: Required - select the code to review
- **Review Type**: Optional - specify focus area (security, performance, etc.)

## Examples

### Basic Code Review
```javascript
function calculateTotal(items) {
  return items.reduce((sum, item) => sum + item.price, 0);
}
```

### Security-Focused Review
Specify security concerns when running the command.

## Output Format

The review will provide:
- Overall score (1-10)
- Issue summary with severity levels
- Specific recommendations
- Code improvement suggestions
```

## Scripts and Automation

### Script Organization

Place scripts in the `scripts/` directory:

```
scripts/
├── setup.sh          # Installation/setup scripts
├── validate.sh       # Validation scripts
└── utils.sh          # Utility functions
```

### Script Integration

Reference scripts in `aikit.toml`:

```toml
[artifacts]
"scripts/*.sh" = ".aikit/scripts/"
```

Scripts can be called from templates or used for package setup.

## Best Practices

### Package Naming

- Use lowercase with hyphens: `code-analysis-tools`
- Be descriptive but concise
- Avoid generic names like "utils" or "tools"

### Command Design

- **Single Responsibility**: Each command should do one thing well
- **Clear Descriptions**: Explain what the command does and when to use it
- **Consistent Naming**: Use verbs for actions (analyze, generate, refactor)
- **Progressive Disclosure**: Start simple, offer advanced options

### Template Quality

- **Structured Content**: Use clear headings and sections
- **Actionable Guidance**: Provide specific, implementable advice
- **Examples**: Include concrete examples wherever possible
- **Error Handling**: Anticipate common issues and edge cases

### Documentation

- **README.md**: Include installation, usage, and examples
- **Command Documentation**: Each template should be self-documenting
- **Changelog**: Track changes and version updates
- **Contributing Guide**: How others can contribute

## Validation and Testing

### Package Validation

```bash
aikit package validate
```

Validates:
- Package structure and required files
- TOML syntax and required fields
- Template file references
- Artifact mappings

### Manual Testing

1. **Build the package**: `aikit package build`
2. **Install locally**: Test installation process
3. **Test commands**: Verify each command works as expected
4. **Cross-agent testing**: Test with different AI assistants

## Publishing Workflow

### GitHub Setup

1. **Create repository**: `yourusername/your-package-name`
2. **Initialize Git**: `git init && git add . && git commit -m "Initial package"`
3. **Push to GitHub**: `git remote add origin ... && git push -u origin main`

### Publishing Commands

```bash
# Build the package
aikit package build

# Create GitHub release
aikit package publish yourusername/your-package-name

# Or create release manually and upload the .zip file from dist/
```

### Version Management

- **Semantic versioning**: `major.minor.patch`
- **Breaking changes**: Increment major version
- **New features**: Increment minor version
- **Bug fixes**: Increment patch version

## Advanced Features

### Multi-Command Packages

Group related functionality:

```toml
[commands.analyze-code]
description = "Analyze code quality"

[commands.analyze-security]
description = "Security vulnerability analysis"

[commands.analyze-performance]
description = "Performance optimization suggestions"
```

### Conditional Templates

Use different templates based on context:

```toml
[commands.generate-component]
description = "Generate React component"
template = "react-component.md"

[commands.generate-api]
description = "Generate API endpoint"
template = "api-endpoint.md"
```

### Package Dependencies

Reference other packages (future feature):

```toml
[dependencies]
base-tools = "1.0.0"
```

## Community and Distribution

### Package Discovery

Users find packages through:
- **GitHub search**: Search for "aikit package"
- **Community lists**: Curated collections
- **Organization repositories**: Company tool collections

### Package Categories

Common package types:
- **Code Analysis**: Linting, security, performance
- **Code Generation**: Components, APIs, tests
- **Documentation**: README generation, API docs
- **Project Management**: Issue tracking, PR templates
- **DevOps**: CI/CD, deployment scripts

### Contributing to Ecosystem

- **Share packages**: Publish useful tools
- **Collaborate**: Contribute to existing packages
- **Document patterns**: Share best practices
- **Report issues**: Help improve the ecosystem

## Troubleshooting

### Common Issues

**"Template not found" error**
- Check template file exists in `templates/` directory
- Verify `template` path in `aikit.toml` is correct
- Ensure file has `.md` extension

**"Package validation failed"**
- Check `aikit.toml` syntax
- Verify all required fields are present
- Ensure package name follows naming rules

**"Command not available"**
- Reinstall the package: `aikit install --force your-package`
- Check AI agent configuration
- Verify package was built correctly

### Getting Help

- **Documentation**: Check this guide and README.md
- **Community**: Open issues on GitHub
- **Debugging**: Use `aikit package validate --verbose`

## Examples

### Simple Utility Package

```toml
[package]
name = "dev-utils"
version = "1.0.0"
description = "Developer utility commands"
authors = ["Dev Team"]

[commands.format-code]
description = "Format code according to style guide"

[commands.generate-docs]
description = "Generate API documentation"

[artifacts]
"templates/*.md" = ".aikit/templates/"
```

### Specialized Analysis Package

```toml
[package]
name = "security-tools"
version = "2.1.0"
description = "Security analysis and vulnerability detection"
authors = ["Security Team"]
license = "MIT"

[commands.security-audit]
description = "Comprehensive security audit"

[commands.dependency-check]
description = "Check dependencies for vulnerabilities"

[commands.secret-scan]
description = "Scan for hardcoded secrets"

[artifacts]
"templates/*.md" = ".aikit/templates/"
"scripts/scan.sh" = ".aikit/scripts/"
```

This comprehensive guide covers everything needed to create, test, and publish AIKIT packages. Start with a simple package and expand functionality as you become familiar with the system.
