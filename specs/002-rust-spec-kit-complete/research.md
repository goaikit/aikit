# Research: AIKIT Rust Implementation

**Date**: 2025-01-27  
**Feature**: 002-rust-spec-kit-complete  
**Purpose**: Resolve technical clarifications and research best practices for Rust CLI implementation

## Research Tasks

### 1. cli-framework Integration Strategy

**Question**: How should we integrate cli-framework for interactive TUI components while maintaining non-interactive CLI mode compatibility?

**Research Findings**:
- cli-framework is designed for single-binary TUI applications with async support
- Framework provides `AppBuilder` pattern for registering views and commands
- Can conditionally enable TUI mode based on `stdin.is_tty()` check
- Non-interactive mode can bypass TUI entirely and use CLI output utilities

**Decision**: Use cli-framework for interactive agent selection UI only. For non-interactive mode, use direct CLI output formatting (matching Python Rich library output). The framework's CLI output utilities can be used for formatted tables/panels in both modes.

**Rationale**: Maintains exact output compatibility with Python version while providing enhanced UX for interactive scenarios.

**Alternatives Considered**:
- Using `dialoguer` or `inquire` crates: Simpler but doesn't match Python's Rich output formatting
- Full TUI mode for all commands: Overkill for simple commands like `check` and `version`

---

### 2. GitHub API Rate Limit Handling

**Question**: How to implement rate limit detection and error formatting that matches Python version exactly?

**Research Findings**:
- GitHub API returns rate limit info in response headers: `X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset`
- `reqwest` provides access to response headers via `headers()` method
- Rate limit errors return HTTP 403 with JSON body containing error details
- `Retry-After` header may be present for secondary rate limits

**Decision**: Parse rate limit headers from `reqwest::Response`, format error messages with exact same structure as Python version (limit, remaining, reset time, troubleshooting tips). Use `chrono` crate for timestamp formatting.

**Rationale**: Ensures identical error messages and user experience across implementations.

**Alternatives Considered**:
- Using `octocrab` crate: Higher-level abstraction but may not provide exact header access needed
- Custom retry logic: Not needed - Python version doesn't retry automatically

---

### 3. Deep JSON Merge Implementation

**Question**: How to implement deep JSON merging for `.vscode/settings.json` that matches Python behavior exactly?

**Research Findings**:
- Python version uses recursive merge: nested objects merged, arrays replaced, scalars overwritten
- `serde_json::Value` provides recursive access to JSON structure
- Need to handle edge cases: null values, type mismatches, invalid JSON

**Decision**: Implement recursive merge function using `serde_json::Value`:
- For objects: recursively merge each key
- For arrays: replace entirely (Python behavior)
- For scalars: overwrite with new value
- Handle null values as valid scalars

**Rationale**: Matches Python implementation behavior exactly, ensuring identical merge results.

**Alternatives Considered**:
- Using `json-merge-patch` crate: Different merge semantics (RFC 7396), doesn't match Python behavior
- Shallow merge: Doesn't handle nested objects correctly

---

### 4. Cross-Platform Script Permission Handling

**Question**: How to set execute permissions on `.sh` files on Unix systems without affecting Windows?

**Research Findings**:
- Rust `std::fs` provides `set_permissions()` with `PermissionsExt` trait on Unix
- Windows has different permission model (no execute bit)
- Need to detect platform and only apply on Unix-like systems
- Should check for shebang (`#!/bin/bash` or similar) before setting permissions

**Decision**: Use `#[cfg(unix)]` conditional compilation and `std::os::unix::fs::PermissionsExt` to set execute permissions. Check for shebang before applying. Skip entirely on Windows.

**Rationale**: Matches Python version behavior exactly - only sets permissions on Unix systems for files with shebangs.

**Alternatives Considered**:
- Using `chmod` command via `std::process::Command`: Less portable, requires external command
- Setting permissions on all `.sh` files: May set permissions on non-executable scripts

---

### 5. ZIP Archive Flattening Logic

**Question**: How to handle ZIP extraction with single top-level directory flattening?

**Research Findings**:
- Python version checks if extracted root has exactly one directory item
- If so, flattens by moving contents up one level
- `zip` crate provides `ZipArchive` with `by_name()` iterator
- Need to detect single top-level directory and adjust paths accordingly

**Decision**: Extract ZIP to temp directory, check if root contains exactly one directory, if so move its contents to target directory. Otherwise, extract directly to target.

**Rationale**: Matches Python behavior exactly, handling GitHub release zip structure correctly.

**Alternatives Considered**:
- Always extracting to target: Doesn't handle nested directory structure from GitHub releases
- Using `zip-rs` with custom extraction: More complex, `zip` crate is sufficient

---

### 6. Agent Configuration Management

**Question**: How to structure and parse agent configurations (AGENT_CONFIG equivalent)?

**Research Findings**:
- Python version uses hardcoded dictionary structure
- Each agent has: `key`, `name`, `folder`, `install_url` (optional), `requires_cli` (boolean)
- Need to support 17 agents with different output formats (Markdown, TOML, agent.md)

**Decision**: Define `AgentConfig` struct with `serde` derive for serialization. Hardcode agent list in Rust code (similar to Python), or load from TOML config file. Use enum for output format (Markdown, Toml, AgentMd).

**Rationale**: Type-safe configuration with compile-time validation. Can be extended to support config file loading in future.

**Alternatives Considered**:
- Loading from external config file: Adds complexity, Python version uses hardcoded config
- Using database: Overkill for static configuration data

---

### 7. Template Command File Generation

**Question**: How to generate agent-specific command files from templates with placeholder replacement?

**Research Findings**:
- Python version uses YAML frontmatter parsing and string replacement
- Placeholders: `{SCRIPT}`, `{AGENT_SCRIPT}`, `{ARGS}`, `__AGENT__`
- Different argument formats: `$ARGUMENTS` (Markdown) vs `{{args}}` (TOML)
- Need to remove `scripts:` and `agent_scripts:` sections from frontmatter

**Decision**: Use `yaml-front-matter` crate or manual parsing for YAML frontmatter. Use string replacement for placeholders. Parse frontmatter, extract script commands, replace placeholders in body, remove script sections from frontmatter, write final file.

**Rationale**: Matches Python implementation exactly, ensuring identical generated files.

**Alternatives Considered**:
- Using template engine (Handlebars, Tera): More complex, Python version uses simple string replacement
- Generating from AST: Overkill for simple text replacement

---

### 8. Output Formatting Compatibility

**Question**: How to match Python Rich library output formatting (panels, tables, trees) in Rust?

**Research Findings**:
- Python Rich provides: Panel, Table, Tree, Progress, Console
- cli-framework provides CLI output utilities for tables and formatted output
- `ratatui` widgets can be used for TUI rendering
- For non-interactive mode, need text-based formatting

**Decision**: Use cli-framework's CLI output utilities for formatted output. For exact compatibility, may need custom formatting functions that match Rich's output style (box drawing, colors, etc.). Use `termcolor` or `colored` crate for ANSI color codes.

**Rationale**: Ensures output matches Python version exactly, maintaining user experience consistency.

**Alternatives Considered**:
- Using `tabled` crate: Good for tables but doesn't match Rich panel/tree formatting
- Using `comfy-table`: Similar to tabled, still need custom panel/tree formatting

---

## Summary

All technical clarifications resolved. Key decisions:
1. Use cli-framework for interactive UI, direct CLI output for non-interactive mode
2. Parse GitHub API headers for rate limit info, format errors identically to Python
3. Implement recursive JSON merge using `serde_json::Value`
4. Use platform-specific permissions API for Unix script permissions
5. Implement ZIP flattening logic matching Python behavior
6. Define `AgentConfig` struct with hardcoded agent list (type-safe)
7. Use YAML frontmatter parsing and string replacement for template generation
8. Use cli-framework CLI utilities + custom formatting for Rich-compatible output

All decisions prioritize exact behavioral compatibility with Python implementation while leveraging Rust's type safety and performance benefits.

