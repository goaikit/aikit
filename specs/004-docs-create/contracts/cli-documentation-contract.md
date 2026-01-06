# CLI Documentation Contract

**Version**: 1.0.0
**Date**: 2025-01-06

## Overview

This contract defines the required format and content for documenting AIKit CLI commands to ensure consistency and completeness across all command documentation.

## Command Documentation Structure

Each CLI command MUST follow this structure:

### Command Header
```markdown
### aikit command-name

[Brief description of primary function, max 2 sentences]
```

### Syntax Block
```bash
aikit command-name [required-args] [options]
```

### Description Section
- What the command does
- When to use it
- What it affects/changes
- Any important caveats

### Options Table
| Option | Description | Required |
|--------|-------------|----------|
| `-s, --short` | Description of option effect | No |
| `--long-option <value>` | Description with parameter type | Yes |

### Examples Section
Multiple examples showing different use cases:

#### Basic Usage
```bash
# Brief explanation of what this example demonstrates
aikit command-name basic-arg
```

#### With Options
```bash
# Explanation of advanced usage
aikit command-name --option value advanced-arg
```

#### Error Cases (if applicable)
```bash
# This will produce an error - showing what NOT to do
aikit command-name invalid-arg
# Error: specific error message
```

## Command Categories

### Core Commands
**Commands**: `init`, `check`, `version`
- Focus on setup and diagnostics
- Include platform-specific notes where relevant

### Package Management Commands
**Commands**: `install`, `update`, `remove`, `list`
- Include GitHub token prerequisites
- Document local vs remote workflows
- Show registry interactions

### Search Commands
**Commands**: `search`
- Document search heuristics and limitations
- Include result interpretation guidance
- Show filtering options

### Package Creation Commands
**Commands**: `package init`, `package build`, `package publish`
- Step-by-step workflow documentation
- Complete prerequisite setup
- GitHub integration details

## Content Requirements

### Completeness
- ALL command-line options MUST be documented
- ALL error conditions commonly encountered MUST be covered
- ALL use cases supported by the CLI MUST have examples
- ALL prerequisites MUST be clearly stated

### Accuracy
- Command syntax MUST match actual CLI implementation
- Option flags MUST match actual implementation exactly
- Examples MUST be runnable as written
- Error messages MUST reflect actual CLI output

### Clarity
- One concept per section to avoid confusion
- Technical terms defined on first use
- Examples preceded by explanatory context
- Complex workflows broken into numbered steps

### Prerequisites Documentation
Examples requiring setup MUST include prerequisites:

```markdown
## Prerequisites

- AIKit CLI installed and in PATH
- GitHub token configured (see configuration.md)
- Local git repository (for package commands)
- Write permissions in target directory
```

## Validation Rules

### Syntax Validation
- Command names match actual CLI implementation
- Option flags match actual implementation exactly
- Required arguments clearly marked in syntax
- Default values documented where applicable

### Example Validation
- All examples tested for runnability on clean environment
- Prerequisites verified before example execution
- Expected output documented when user-visible
- Error cases include both problem and solution

### Completeness Validation
- All commands in CLI have corresponding documentation sections
- All documented commands exist in current CLI
- All options for each command are documented
- All common use cases have runnable examples

## Maintenance Requirements

### Version Updates
- Document version-specific features with version notices
- Update examples when CLI syntax changes
- Maintain backward compatibility examples where possible

### Testing Requirements
- Examples validated after CLI changes
- New command options documented immediately
- Error messages updated when CLI error text changes

### Review Requirements
- Regular review of example runnability (monthly)
- User feedback incorporation for unclear sections
- Cross-validation with actual CLI help output
