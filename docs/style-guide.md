# AIKit Documentation Style Guide

**Version**: 1.0.0
**Last Updated**: 2025-01-06

## Purpose

This style guide ensures consistency across all AIKit documentation files. Consistent formatting, terminology, and structure improve readability and maintainability.

## General Principles

### Clarity First
- Write for users who may not be familiar with AIKit concepts
- Use simple, direct language
- Explain technical terms on first use
- Prefer active voice over passive

### Consistency
- Use consistent terminology throughout all documents
- Follow the same structure for similar content types
- Maintain consistent formatting and styling

### Maintenance
- Keep documentation current with code changes
- Use automated validation where possible
- Document maintenance procedures

## File Structure

### File Naming
- Use lowercase with hyphens: `cli-commands.md`, `configuration.md`
- Be descriptive but concise
- Use singular nouns for categories

### Frontmatter (Optional)
For documentation files that may be processed by static site generators:

```yaml
---
title: "Page Title"
description: "Brief description for SEO and navigation"
---
```

## Markdown Formatting

### Headings
- Use `#` for main title (H1) - only one per file
- Use `##` for major sections (H2)
- Use `###` for subsections (H3)
- Use `####` for sub-subsections (H4)
- Maintain logical hierarchy - don't skip levels

### Code Blocks
- Use language-specific highlighting:
  ```bash
  # For shell commands
  aikit install user/package
  ```

  ```toml
  # For configuration files
  [package]
  name = "example"
  ```

- Include language identifier for all code blocks
- Use `text` for plain text content
- Use `markdown` for template examples

### Inline Code
- Use backticks for commands: `aikit install`
- Use backticks for file names: `aikit.toml`
- Use backticks for code elements: `version = "1.0.0"`

### Links
- Use relative links within docs/: `[Configuration](configuration.md)`
- Use descriptive link text: `[Configure AIKit](configuration.md)`
- Avoid bare URLs in body text

### Lists
- Use bullet points for unordered lists
- Use numbered lists for sequential steps
- Maintain consistent indentation
- Start numbered lists at 1, not 0

### Tables
- Use for comparing options or showing structured data
- Include headers in all tables
- Align columns appropriately
- Keep tables simple and readable

## Content Structure

### CLI Command Documentation

Each command must follow this structure:

```markdown
### aikit command-name

[Brief description of primary function]

```bash
aikit command-name [required-args] [options]
```

**Description**
[What the command does and when to use it]

**Options**
| Option | Description | Required |
|--------|-------------|----------|
| `-o, --option` | Description | No |

**Examples**

#### Basic Usage
```bash
# Brief explanation
aikit command-name arg
```

#### Advanced Usage
```bash
# Explanation of advanced features
aikit command-name --option value
```
```

### Configuration Documentation

```markdown
## Configuration Overview

[Explain configuration hierarchy and fallback]

## File Locations

- Primary: `.aikit/config.toml`
- Global fallback: `~/.aikit/config.toml`
- Defaults: Built-in configuration

## Configuration Structure

### AikConfig Fields

```toml
[package]
version = "1.0"        # Configuration version
install_dir = ".aikit" # Package installation directory

[agents.claude]
# Agent-specific configuration
```

**Field Descriptions**
- `version`: Configuration format version
- `install_dir`: Directory for installed packages
```

### Example Documentation

```markdown
## Basic Usage Examples

### Installing a Package

```bash
# Install from GitHub
aikit install owner/package-name
```

**Prerequisites**
- GitHub token configured
- Network access to GitHub

**Expected Output**
```
Installing package: owner/package-name
Download complete. Installing...
Package installed successfully.
```

### Troubleshooting

**Error: "GitHub token required"**
- Ensure GITHUB_TOKEN environment variable is set
- Or use: `aikit install owner/package --token YOUR_TOKEN`
```

## Terminology Standards

### AIKit-Specific Terms

| Term | Usage | Example |
|------|-------|---------|
| Package | AI agent extension | "Install this package to add new commands" |
| Agent | AI assistant (Claude, Cursor, etc.) | "This works with Claude and Cursor agents" |
| Template | Reusable prompt/command structure | "The template generates consistent code" |
| Registry | Package source/repository | "Search the GitHub registry for packages" |

### Command References
- Always use full command syntax in examples
- Show actual flags and options
- Include realistic values, not placeholders

### File References
- Use correct file extensions: `.toml`, `.md`
- Reference actual directory structures
- Use consistent path formats

## Validation Rules

### Automated Checks
- All internal links resolve
- Code blocks have language identifiers
- Headings follow hierarchy rules
- File naming conventions followed

### Manual Reviews
- Content accuracy against current CLI
- Example runnability verification
- Terminology consistency
- Clarity and completeness

## Maintenance Guidelines

### Updating Documentation
1. Test all examples after CLI changes
2. Update version references as needed
3. Review cross-references for accuracy
4. Validate with `scripts/validate-docs.sh`

### Adding New Documentation
1. Follow this style guide
2. Add to appropriate category
3. Include examples and prerequisites
4. Test validation scripts

### Version Compatibility
- Note version-specific features
- Document breaking changes
- Provide migration guidance

---

This style guide ensures AIKit documentation remains consistent, accurate, and user-friendly. All contributors should review this guide before creating or updating documentation.
