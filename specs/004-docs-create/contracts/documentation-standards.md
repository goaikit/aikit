# Documentation Standards Contract

**Version**: 1.0.0
**Date**: 2025-01-06

## Overview

This contract defines the standards and formats for FastSkill documentation content, ensuring consistency, accuracy, and usability across all documentation files.

## File Format Standards

### Markdown Structure
All documentation files MUST use the following structure:

```markdown
---
title: "Page Title"
description: "Brief description for SEO and navigation"
---

# Page Title

[Content sections with proper heading hierarchy]
```

### Frontmatter Requirements
- `title`: Required, matches H1 heading
- `description`: Required, 50-160 characters, SEO-optimized

## Content Standards

### Command Documentation Format
CLI commands MUST be documented with this structure:

```markdown
### fastskill command-name

[Brief description of what the command does]

```bash
fastskill command-name [options] [arguments]
```

**Options:**
- `-o, --option`: Description of option
- `--another-option <value>`: Description with parameter

**Examples:**

```bash
# Basic usage
fastskill command-name

# With options
fastskill command-name --option value
```
```

### Configuration Documentation Format
Configuration files MUST be documented as:

```yaml
# .fastskill/config.yaml
embedding:
  openai_base_url: "https://api.openai.com/v1"
  embedding_model: "text-embedding-3-small"

skills_directory: ".claude/skills"
```

**Field Descriptions:**
- `embedding.openai_base_url`: Base URL for OpenAI API calls
- `embedding.embedding_model`: Model to use for semantic search
- `skills_directory`: Directory where skills are installed

### Code Example Standards

#### Prerequisites Section
Examples requiring setup MUST include prerequisites:

```markdown
## Prerequisites

- FastSkill CLI installed
- OpenAI API key configured
- Git repository cloned
```

#### Runnable Examples
Examples MUST be immediately copy-paste runnable:

```bash
# ✅ Good: Complete, runnable example
export OPENAI_API_KEY="your-key-here"
fastskill init
fastskill add https://github.com/org/skill.git
fastskill install
fastskill search "text processing"
```

#### External Dependencies
Examples with external dependencies MUST be clearly marked:

```bash
# ⚠️  Requires OpenAI API Key
# Set your API key first:
export OPENAI_API_KEY="your-key-here"

fastskill search "web scraping"
```

## Link Standards

### Internal Links
MUST use relative paths within webdocs/:

```markdown
[Installation Guide](/installation)
[CLI Reference](/cli-reference/overview)
```

### External Links
MUST include protocol and be validated:

```markdown
[GitHub Repository](https://github.com/gofastskill/fastskill)
[OpenAI API](https://platform.openai.com/docs)
```

## Terminology Standards

### Consistent Terms
- Use "FastSkill" (not "fastskill" in prose)
- Use "skills" for the packages, "Skills" for the section title
- Use "semantic search" for the feature
- Use "CLI" for command-line interface

### Command References
- Use backticks for command names: `fastskill add`
- Use code blocks for multi-line commands
- Include actual command syntax, not placeholders

## Quality Standards

### Completeness Requirements
- All CLI commands MUST be documented
- All configuration options MUST be explained
- All examples MUST include prerequisites
- All internal links MUST resolve

### Accuracy Requirements
- Command syntax MUST match implementation
- Configuration paths MUST match code expectations
- Examples MUST be tested and runnable
- Version references MUST be current

### Usability Requirements
- Maximum 300 words per section
- Include table of contents for long pages
- Use expandable sections for optional content
- Provide troubleshooting sections for common issues

## Validation Rules

### Automated Checks
- All internal links resolve
- All frontmatter fields present
- All code blocks have language specified
- No broken external links (checked quarterly)

### Manual Reviews
- Example runnability tested
- Command syntax verified against codebase
- Configuration paths validated
- Content clarity and completeness assessed
