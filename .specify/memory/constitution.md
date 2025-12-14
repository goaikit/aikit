<!--
SYNC IMPACT REPORT - Constitution Amendment 2025-12-14
======================================================

Version change: template → 1.0.0 (MAJOR: First formal constitution with all core principles established)

Modified principles: None (all principles newly established)
- I. CLI-First: Added
- II. Template-Driven: Added
- III. Cross-Platform: Added
- IV. Test-First (NON-NEGOTIABLE): Added
- V. User-Centric: Added

Added sections:
- Technology Standards
- Development Workflow

Removed sections: None

Templates requiring updates:
✅ .specify/templates/tasks-template.md - Updated test requirements from OPTIONAL to MANDATORY per Test-First principle
✅ .specify/templates/plan-template.md - Added specific constitution check criteria aligned with all 5 principles

Follow-up TODOs: None - All placeholders resolved, no deferred items
-->

# AIKIT Constitution

## Core Principles

### I. CLI-First
Every component exposes functionality via command-line interface; Text I/O protocol enforced (stdin/args → stdout, errors → stderr); Support both JSON structured output and human-readable formats for all commands; Commands must be composable via standard Unix pipes and redirection.

### II. Template-Driven
All project setup and configuration through downloadable, versioned templates; Templates must be self-contained, independently testable, and documented; Each template serves a clear, specific purpose with explicit scope boundaries; Template selection and customization must be user-guided with sensible defaults.

### III. Cross-Platform
Native support for Windows, macOS, and Linux operating systems; Automatic detection and adaptation to platform-specific conventions; Support for both Bash and PowerShell scripting environments; Platform-specific binaries and installation methods provided; No platform-exclusive features or dependencies.

### IV. Test-First (NON-NEGOTIABLE)
TDD mandatory for all new functionality: Tests written → User approved → Tests fail → Then implement; Red-Green-Refactor cycle strictly enforced; Integration tests required for CLI commands, template downloads, and cross-platform compatibility; Test coverage must include error paths and edge cases.

### V. User-Centric
Design prioritizes developer experience and time-to-value; One-command project setup with intelligent defaults; Comprehensive error messages with actionable recovery steps; Progressive disclosure of complexity - simple commands for common cases, advanced options for power users; Documentation and troubleshooting guides must be accessible without leaving the terminal.

## Technology Standards

**Language**: Rust 1.70+ for all core functionality with stable channel enforcement
**Distribution**: Pre-compiled binaries for all supported platforms with automated releases
**Dependencies**: Minimal external dependencies; security audit required for all crates
**Build System**: Cargo with standardized workspace structure and cross-compilation support
**Packaging**: GitHub releases with checksums and automated platform detection

## Development Workflow

**Version Control**: Git with protected main branch and feature branch workflow
**Code Review**: All changes require review; PRs must demonstrate compliance with constitution principles
**Testing Gates**: Unit tests required; integration tests for CLI commands; cross-platform testing for releases
**Release Process**: Automated via GitHub Actions with semantic versioning (MAJOR.MINOR.PATCH)
**Documentation**: README-driven development; all features must be documented with usage examples

## Governance

Constitution supersedes all other practices and takes precedence over individual preferences; Amendments require justification, documentation, and demonstration of improved outcomes; Complexity must be explicitly justified with rejected simpler alternatives; Runtime development guidance maintained in `.specify/` directory; All PRs must verify constitution compliance through automated checks.

**Version**: 1.0.0 | **Ratified**: 2025-12-14 | **Last Amended**: 2025-12-14