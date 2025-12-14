# AIKIT Generalization: Universal Package Template System

## Overview

AIKIT must be generalized from a spec-driven development tool into a universal package template system that can distribute any kind of reusable content (prompts, templates, scripts, configurations) across different AI agents.

## Core Changes Required

### 1. Decouple from Spec-Driven Development

**Current State:**
- Hardcoded "specify" commands and spec-kit templates
- Tightly coupled to feature specification workflow
- Fixed directory structure assumptions

**Target State:**
- Generic package templates with no domain assumptions
- Configurable command sets and workflows
- Domain-agnostic artifact management

### 2. Universal Package Format

**Package Structure:**
```
package-root/
├── package.toml          # Package metadata and configuration
├── templates/            # Template files (configurable structure)
├── scripts/              # Executable scripts (bash, powershell)
├── assets/               # Static assets and resources
└── docs/                 # Documentation and examples
```

**Installation Structure (.aikit/):**
```
.aikit/
├── packages.toml         # Registry of installed packages
├── lock.toml            # Version lock file
├── templates/           # Installed template files
├── scripts/             # Installed executable scripts
├── assets/              # Installed static assets
└── cache/               # Downloaded package cache
```

**package.toml Structure:**
```toml
[package]
name = "my-package"
version = "1.0.0"
description = "Package description"
authors = ["Author Name"]
license = "MIT"
repository = "https://github.com/user/package"

[commands]
# Define available commands
analyze = { description = "Analyze something" }
generate = { description = "Generate content" }
validate = { description = "Validate results" }

# [dependencies] - Not supported (packages are self-contained)

[artifacts]
# Define what gets installed where
"templates/*.md" = ".aikit/templates/"
"scripts/*" = ".aikit/scripts/"
"assets/**/*" = ".aikit/assets/"
```

### 3. Package Management Commands

**New CLI Commands:**

```bash
# Package lifecycle management
aikit package init <name>                    # Initialize new package
aikit package build                          # Build package archive
aikit package publish <repo-url>             # Publish to GitHub release

# Package consumption
aikit install <repo-url>                     # Install package from GitHub
aikit install <repo-url> --yes               # Install without .gitignore prompts
aikit install <repo-url>@v1.2.3              # Install specific version
aikit update <package-name>                  # Update to latest version
aikit remove <package-name>                  # Uninstall package
aikit list                                   # List installed packages

# Package information
aikit show <package-name>                    # Show package details
aikit search <query>                         # Search available packages
```

### 4. Template System Overhaul

**Current Template Structure:**
- Fixed YAML frontmatter with hardcoded fields
- Specific to spec-driven development workflow
- Limited artifact types

**New Template System:**
- Generic YAML frontmatter with configurable fields
- Domain-agnostic content generation
- Support for any file types and structures
- Template inheritance and composition

**Example Generic Template:**
```yaml
---
name: "content-generator"
description: "Generate content using AI"
category: "writing"
tags: ["creative", "ai-assisted"]
version: "1.0.0"

# Configurable fields (not hardcoded)
metadata:
  author: "Package Author"
  license: "MIT"
  target_audience: "content-creators"

# Flexible artifact definitions
artifacts:
  - type: "template"
    source: "story-template.md"
    destination: ".aikit/templates/story.md"
    description: "Story generation template"

  - type: "script"
    source: "generate.sh"
    destination: ".aikit/scripts/generate.sh"
    permissions: "executable"
    platforms: ["linux", "macos"]

  - type: "config"
    source: "settings.json"
    destination: ".aikit/config/settings.json"
    merge_strategy: "deep_merge"

# Command definitions
commands:
  generate:
    description: "Generate content"
    script: "generate.sh"
    args_template: "--type {TYPE} --topic {TOPIC}"
    examples:
      - "generate --type story --topic adventure"
      - "generate --type article --topic technology"

# Dependencies and compatibility
requires:
  - "openai-api-key"
  - "python >= 3.8"

compatibility:
  agents: ["claude", "cursor-agent", "gemini", "copilot"]
  platforms: ["linux", "macos", "windows"]
---

# Content Generator Package

This package provides templates and tools for AI-assisted content generation.

## Features

- Story generation templates
- Article writing assistance
- Creative writing prompts
- Multi-format output support

## Usage

After installation, use the generated commands:

```bash
# Generate a story
generate --type story --topic "space adventure"

# Create an article
generate --type article --topic "future of AI"
```

## Customization

The package installs to `.aikit/` directory and can be customized by editing the installed templates and scripts.
```

### 5. Agent Adaptation System

**Enhanced Agent Configuration:**
```rust
pub struct PackageConfig {
    pub name: String,
    pub version: String,
    pub commands: HashMap<String, CommandDefinition>,
    pub artifacts: Vec<ArtifactMapping>,
    pub compatibility: AgentCompatibility,
}

pub struct CommandDefinition {
    pub name: String,
    pub description: String,
    pub script_template: String,
    pub arg_template: String,
    pub output_format: OutputFormat,
    pub agent_overrides: HashMap<String, AgentOverride>,
}
```

**Agent Override System:**
```rust
pub struct AgentOverride {
    pub command_name: String,           // How command appears in this agent
    pub script_adaptation: String,      // Agent-specific script changes
    pub arg_format: String,            // Agent-specific argument format
    pub integration_points: Vec<String>, // Where/how it integrates
}
```

### 6. Installation and Management System

**Package Registry:**
- GitHub repositories as package sources
- Version management via Git tags/releases
- Dependency resolution system
- Package metadata caching

**Installation Process:**
1. Download package from GitHub release
2. Extract to temporary directory
3. Parse package.toml for configuration
4. **Check and update .gitignore** (if .aikit/ doesn't exist and .gitignore exists)
5. Create .aikit/ directory structure
6. Install artifacts to appropriate locations
7. Apply agent-specific adaptations
8. Generate agent-specific commands
9. Update package registry

**.gitignore Integration:**
- When first installing a package (creating .aikit/ directory), AIKIT automatically checks for .gitignore
- If .gitignore exists, AIKIT prompts user for permission to add `.aikit/` to it
- Permission prompt can be bypassed with `--yes` flag for automated scripts
- This prevents accidental commits of package artifacts to version control

**Example Installation:**
```bash
# Install writing assistant package
aikit install https://github.com/user/writing-assistant

# Creates .aikit/ directory with package artifacts
.aikit/
├── templates/
│   ├── story-template.md
│   └── article-template.md
├── scripts/
│   └── generate-content.sh
└── packages.toml

# Package gets adapted for current agent
# Cursor: Creates /writing-assistant.write commands
# Claude: Creates claude writing-assistant.write commands
# Gemini: Creates writing-assistant.write prompts

# Use the package
/writing-assistant.write --type story --genre fantasy
```

### 7. Backward Compatibility

**Migration Path:**
- Existing spec-kit packages continue to work
- Gradual migration of hardcoded logic to configurable system
- Deprecation warnings for old-style packages

**Spec-Kit as First Package:**
```toml
# spec-kit becomes just another package
[package]
name = "spec-kit"
description = "Spec-driven development toolkit"
version = "1.0.0"

[commands]
specify = { description = "Create feature specifications" }
plan = { description = "Generate technical plans" }
implement = { description = "Implementation guidance" }
```

## Implementation Phases

### Phase 1: Core Infrastructure
- [ ] Generic package.toml format
- [ ] Basic install/uninstall commands
- [ ] Template system decoupling

### Phase 2: Agent Adaptation
- [ ] Enhanced agent override system
- [ ] Multi-agent command generation
- [ ] Artifact mapping system

### Phase 3: Package Ecosystem
- [ ] GitHub integration for package distribution
- [ ] Package registry and search
- [ ] Dependency management

### Phase 4: Advanced Features
- [ ] Package composition and inheritance
- [ ] Version management and updates
- [ ] Cross-package dependencies

## Use Cases

### 1. Spec-Driven Development (Current)
```bash
aikit install https://github.com/org/spec-kit
# Creates /specify, /plan, /implement commands
```

### 2. Writing Assistant
```bash
aikit install https://github.com/aroff/aikit/examples/writing-assistant
# Creates /write, /edit, /review commands
```

### 3. Investment Strategy Development
```bash
aikit install https://github.com/aroff/aikit/examples/finance/investment-strategy-kit
# Creates /analyze, /backtest, /optimize commands
```

### 4. Movie Script Writing
```bash
aikit install https://github.com/aroff/aikit/examples/media/script-writing-kit
# Creates /outline, /character, /scene commands
```

## Benefits

1. **Universal Applicability**: Any domain can create packages
2. **Agent Agnostic**: Works across all supported AI agents
3. **Decoupled Architecture**: No domain-specific assumptions
4. **Extensible**: Easy to add new package types
5. **User Choice**: Users pick packages for their needs
6. **Version Management**: Proper package lifecycle management
7. **Automatic .gitignore Management**: Prevents accidental commits of package artifacts
8. **Ecosystem Growth**: Community can create and share packages

## Migration Impact

- **Breaking Changes**: Spec-kit specific logic removed
- **New Capabilities**: Any package type supported
- **Enhanced Flexibility**: Agent adaptation system
- **Better User Experience**: Clear package management commands