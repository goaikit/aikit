# Technical Research: AIKIT Universal Package System

**Feature**: `1-aikit-generalize` | **Date**: 2024-12-14
**Status**: Phase 0 Research Complete

## Research Objectives

Validate the technical approach for transforming AIKIT from a spec-driven development tool into a universal package template system. Focus on:

1. Package format design and validation
2. Agent adaptation architecture
3. GitHub integration patterns
4. Backward compatibility strategies
5. Security and trust models

## Package Format Analysis

### Existing AIKIT Template Format
```yaml
---
description: Create feature specifications...
scripts:
  sh: scripts/bash/create-new-feature.sh --json "{ARGS}"
---
# Template content here
```

**Strengths**:
- Simple YAML frontmatter + Markdown body
- Agent-specific script sections
- Placeholder substitution ({ARGS}, {SCRIPT})

**Limitations**:
- Hardcoded field names (description, scripts)
- Specific to command generation
- No package metadata or dependencies

### Proposed package.toml Format
```toml
[package]
name = "my-package"
version = "1.0.0"
description = "Package description"
authors = ["Author Name"]

[commands]
analyze = { description = "Analyze something" }
generate = { description = "Generate content" }

[artifacts]
"templates/*.md" = ".aikit/templates/"
"scripts/*" = ".aikit/scripts/"
```

**Advantages**:
- Structured metadata with clear schema
- Flexible artifact mapping
- Standard TOML format (widely supported)
- Extensible for future features

**Decision**: Use TOML for package metadata, keep existing YAML frontmatter for templates

## Agent Adaptation Architecture

### Current AIKIT Approach
- AgentConfig defines agent-specific properties
- Template.generate_content() adapts content per agent
- Hardcoded agent list with specific attributes

### Extended Architecture for Packages
```rust
pub struct Package {
    metadata: PackageMetadata,
    commands: HashMap<String, CommandDefinition>,
    artifacts: Vec<ArtifactMapping>,
    agent_overrides: HashMap<String, AgentOverride>,
}

pub struct AgentOverride {
    command_name: String,
    script_template: String,
    arg_format: String,
}
```

**Key Design Decisions**:
1. **Package-centric**: Packages define their own agent compatibility
2. **Override system**: Packages can specify agent-specific behavior
3. **Namespace isolation**: Commands prefixed by package name (e.g., `package.command`)
4. **Backward compatibility**: Existing templates converted automatically

## GitHub Integration Patterns

### Package Distribution
- **GitHub Releases**: Primary distribution mechanism
- **ZIP Archives**: Standard package format
- **Version Tags**: Semantic versioning via Git tags
- **Repository URLs**: Direct `https://github.com/user/repo` support

### Implementation Approaches
1. **Simple**: Direct GitHub API calls for releases
2. **Robust**: Git clone + build approach for complex packages
3. **Hybrid**: Releases for distribution, clone for development

**Decision**: Start with GitHub Releases API for simplicity, add clone support later if needed

### Authentication & Access
- **Public repos**: No authentication required
- **Private repos**: GitHub token support via environment variables
- **Rate limiting**: Respect GitHub API limits (5000/hour authenticated)

## Directory Structure & Management

### .aikit/ Directory Structure
```
.aikit/
├── packages.toml      # Installed package registry
├── lock.toml         # Version lock file
├── cache/            # Downloaded package cache
├── templates/        # Shared templates across agents
├── scripts/          # Executable scripts
├── assets/           # Static resources
└── [package-name]/   # Package-specific data
```

### Package Isolation Strategy
1. **Shared artifacts**: Templates and scripts available to all agents
2. **Agent-specific generation**: Commands generated per agent in .{agent}/ directories
3. **Package-specific data**: Isolated per-package subdirectories
4. **Version management**: Lock file prevents conflicts

**Decision**: Shared artifact approach with agent-specific command generation

## Backward Compatibility Strategy

### Migration Layers
1. **Detection**: Identify old vs new usage patterns
2. **Translation**: Convert old templates to new format automatically
3. **Warnings**: Deprecation notices for old-style usage
4. **Dual support**: Both systems work during transition

### Spec-Kit Compatibility
- Existing `aikit init` continues working
- Spec-kit becomes a special "built-in" package
- Gradual migration with clear upgrade path
- Zero breaking changes for existing users

**Decision**: Full backward compatibility with transparent migration

## Security & Trust Model

### Threat Analysis
- **Package content**: Templates, scripts, configuration files
- **Execution context**: Local development environment
- **Distribution**: GitHub repositories (trusted source)

### Security Decisions
1. **Full trust model**: As clarified, packages are trusted template collections
2. **No sandboxing**: Unlike executable packages, templates don't need isolation
3. **User responsibility**: Clear documentation about package source verification
4. **Audit capability**: All package contents visible and reviewable

**Decision**: Trust-based model with user education and transparency

## Performance Considerations

### Target Performance Metrics
- **Installation**: <30 seconds for typical packages
- **Command generation**: <1 second per agent
- **Search**: <2 seconds for result retrieval
- **Update checks**: <5 seconds for version checking

### Optimization Strategies
1. **Caching**: Downloaded packages cached locally
2. **Incremental updates**: Only update changed artifacts
3. **Lazy loading**: Load package content on demand
4. **Parallel processing**: Concurrent agent adaptation

## Implementation Feasibility Assessment

### Technical Risks - Low
- **TOML parsing**: Well-established Rust ecosystem support
- **GitHub API**: Mature and stable APIs
- **Agent adaptation**: Proven existing AIKIT patterns
- **File system operations**: Standard Rust capabilities

### Integration Risks - Medium
- **Backward compatibility**: Requires careful testing of existing workflows
- **Agent coverage**: All 17 agents must work with new system
- **GitHub dependency**: Service availability and API changes

### Operational Risks - Low
- **Package conflicts**: Namespace prefixes prevent issues
- **Storage growth**: .aikit/ directory managed with cleanup commands
- **Version management**: Standard semantic versioning practices

## Alternative Architecture Considerations

### Considered Alternatives

1. **JSON instead of TOML**: Rejected - TOML more human-readable for package metadata
2. **Package registry server**: Rejected - GitHub sufficient, adds complexity
3. **Sandboxed execution**: Rejected - Unnecessary for template-based packages
4. **Global command space**: Rejected - Namespace prefixes prevent conflicts

### Architectural Principles
- **Simplicity**: Minimal new concepts, leverage existing AIKIT patterns
- **Compatibility**: Zero breaking changes for existing users
- **Extensibility**: Design for future package types and features
- **User experience**: Intuitive CLI with helpful error messages

## Conclusion & Recommendations

### Technical Approach: VALIDATED ✅
The proposed architecture successfully extends AIKIT's proven agent adaptation system to work with generic packages while maintaining all existing functionality.

### Key Strengths
- **Leverages existing code**: Builds on proven AIKIT patterns
- **Minimal complexity**: Uses familiar TOML/JSON/YAML formats
- **Strong compatibility**: Backward compatibility fully preserved
- **Scalable design**: Supports future package types and features

### Implementation Confidence: HIGH
- **Low technical risk**: Uses established Rust ecosystem components
- **Proven patterns**: Agent adaptation system already working
- **Incremental approach**: Can be implemented in phases
- **Testable design**: Clear interfaces and contracts throughout

### Recommended Next Steps
1. **Phase 0 Complete**: Technical approach validated
2. **Begin Phase 1**: Start with core package data structures
3. **Early testing**: Implement basic package parsing and validation
4. **Continuous validation**: Regular compatibility testing with existing AIKIT

**Ready to proceed with implementation** 🚀
