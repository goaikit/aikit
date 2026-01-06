# Documentation Structure Contract

**Version**: 1.0.0
**Date**: 2025-01-06

## Overview

This contract defines the required structure and organization for the AIKit documentation files to be created in the docs/ directory.

## Required Documentation Files

All files should be created in the `webdocs/` directory using MDX format for Mintlify compatibility.

### 1. index.mdx - Main Documentation Page
**Purpose**: Main entry point for users discovering AIKit
**Required Sections**:
- Project description and key features
- Quick start examples (1-2 lines each)
- Installation instructions for all platforms
- Command overview with links to detailed docs
- Contributing guidelines
- License information

**Constraints**:
- Keep under 1000 words
- Include table of contents
- Use relative links to other docs

### 2. cli-commands.mdx - CLI Command Reference
**Purpose**: Comprehensive reference for all CLI commands
**Required Sections**:
- Command syntax overview
- Individual command documentation (init, check, install, etc.)
- Package subcommands (init, build, publish)
- Option descriptions and examples

**Format Requirements**:
- Consistent command format: `### aikit command-name`
- Syntax blocks: ```bash
- Options tables with required/optional indicators
- Multiple runnable examples per command

### 3. configuration.mdx - Configuration Guide
**Purpose**: Complete guide for AIKit configuration
**Required Sections**:
- Configuration file hierarchy (.aikit/config.toml → ~/.aikit/config.toml → defaults)
- AikConfig structure documentation
- Agent-specific configurations
- Registry configuration
- User preferences

**Format Requirements**:
- TOML code blocks with comments
- Field descriptions for all config options
- Examples for common configurations

### 4. packages.mdx - Package Management
**Purpose**: Guide for creating, building, and publishing packages
**Required Sections**:
- Package creation workflow
- aikit.toml schema documentation
- Building packages
- Publishing to GitHub
- Local vs remote package installation

**Format Requirements**:
- Step-by-step workflows
- Complete aikit.toml examples
- Prerequisites clearly stated

### 5. agents.mdx - AI Agent Integration
**Purpose**: Documentation for all supported AI agents
**Required Sections**:
- Supported agents overview
- Agent-specific configuration requirements
- Folder structures and file placements
- CLI requirements and installation links
- Template generation details

**Format Requirements**:
- Agent comparison table
- Per-agent setup instructions
- File path specifications

### 6. examples.mdx - Usage Examples
**Purpose**: Practical, runnable examples for common workflows
**Required Sections**:
- Basic usage scenarios
- Advanced workflows
- Integration examples
- Troubleshooting examples

**Format Requirements**:
- All examples immediately copy-paste runnable
- Prerequisites clearly marked
- Expected output shown where relevant

### 7. troubleshooting.mdx - Common Issues
**Purpose**: Solutions for frequent problems
**Required Sections**:
- GitHub token issues and setup
- Network and API problems
- Permission and file system errors
- Agent detection problems
- Package installation failures

**Format Requirements**:
- Problem → Solution structure
- Error messages with explanations
- Prevention tips

## Quality Standards

### Consistency Requirements
- Use "AIKit" (not "aikit") in prose
- Consistent heading hierarchy (H1 for title, H2 for main sections, H3 for subsections)
- Standard code block languages (bash, toml, markdown)
- Relative links within docs/ directory

### Content Requirements
- All examples tested against current CLI
- Configuration paths match actual implementation
- Command syntax verified against codebase
- External dependencies clearly documented

### Maintenance Requirements
- Version notices for version-specific features
- Clear ownership for updates
- Regular validation of examples
