# CLI API Contracts: AIKIT

**Date**: 2025-01-27  
**Feature**: 002-rust-spec-kit-complete

## Overview

This document defines the CLI API contracts for the `aikit` binary. All commands must match the Python `specify` CLI behavior exactly.

## Command: `aikit init`

Initialize a new Spec-Driven Development project.

### Signature

```bash
aikit init [PROJECT_NAME] [OPTIONS]
```

### Arguments

- `PROJECT_NAME` (optional): Name of the project directory to create. If `.` is provided, equivalent to `--here`. If omitted and `--here` not set, error.

### Options

- `--ai <AGENT>`: Specify AI agent to use (e.g., `claude`, `gemini`, `copilot`). If omitted and stdin is TTY, show interactive selection.
- `--script <TYPE>`: Specify script type (`sh` or `ps`). Defaults to `ps` on Windows, `sh` on Unix.
- `--here`: Initialize in current directory instead of creating new directory.
- `--force`: Skip confirmation prompt when merging into non-empty directory.
- `--no-git`: Skip Git repository initialization.
- `--github-token <TOKEN>`: GitHub personal access token for API requests.
- `--skip-tls`: Skip TLS certificate verification (not recommended).
- `--debug`: Enable verbose diagnostic output.
- `--ignore-agent-tools`: Skip CLI tool validation for selected agent.

### Environment Variables

- `GH_TOKEN`: GitHub token (used if `--github-token` not provided)
- `GITHUB_TOKEN`: Alternative GitHub token variable

### Input Contract

- **Preconditions**:
  - If `PROJECT_NAME` provided and not `.`, directory must not exist (unless `--here`)
  - If `--here` used, current directory must exist
  - If `--ai` not provided, stdin must be TTY for interactive mode

### Output Contract

- **Success** (exit code 0):
  - Project directory created/initialized
  - Template files extracted
  - Git repository initialized (unless `--no-git`)
  - Agent-specific command files present
  - Success message displayed

- **Failure** (exit code 1):
  - Error message to stderr
  - No partial state left (or clearly documented)

### Error Cases

- Directory already exists (unless `--here` or `--force`)
- Invalid agent key
- GitHub API rate limit exceeded
- Network error during template download
- Invalid zip archive
- Git initialization failure (warning, not fatal)

---

## Command: `aikit check`

Check installed tools and AI agent CLIs.

### Signature

```bash
aikit check [OPTIONS]
```

### Options

- `--json`: Output results in JSON format (future enhancement, not in Python version)

### Input Contract

- **Preconditions**: None (always valid)

### Output Contract

- **Success** (exit code 0):
  - Tool availability status displayed
  - Format: Tree structure with checkmarks/crosses
  - IDE-based agents marked as "IDE-based, no CLI check"

- **Failure**: Never fails (always exit code 0)

### Error Cases

- None (command never fails)

---

## Command: `aikit version`

Display version information.

### Signature

```bash
aikit version [OPTIONS]
```

### Options

- `--github-token <TOKEN>`: GitHub token for API requests (optional)

### Input Contract

- **Preconditions**: None

### Output Contract

- **Success** (exit code 0):
  - CLI version (from Cargo.toml)
  - Template version (from GitHub latest release, or "unknown" if unavailable)
  - System information (OS, architecture, Rust version)
  - Format: Panel with table

- **Failure**: Never fails (always exit code 0, shows "unknown" for unavailable data)

### Error Cases

- GitHub API unavailable (shows "unknown" for template version, continues)

---

## Command: `aikit package`

Build template zip archives for GitHub releases.

### Signature

```bash
aikit package <VERSION> [OPTIONS]
```

### Arguments

- `VERSION` (required): Version string with 'v' prefix (e.g., `v1.0.0`). Must match pattern `vX.Y.Z`.

### Options

- `--output-dir <DIR>`: Output directory for zip files (default: `.genreleases/`)

### Environment Variables

- `AGENTS`: Comma or space-separated list of agent keys to package (filters to specific agents)
- `SCRIPTS`: Comma or space-separated list of script types (`sh` or `ps`) to package

### Input Contract

- **Preconditions**:
  - `VERSION` must match semantic version pattern `vX.Y.Z`
  - Must be run from repository root (where templates exist)

### Output Contract

- **Success** (exit code 0):
  - Zip archives created in output directory
  - Naming: `spec-kit-template-<agent>-<script>-<version>.zip`
  - All agent/script combinations packaged (or filtered by env vars)

- **Failure** (exit code 1):
  - Error message to stderr
  - Invalid version format
  - Missing template files
  - File system errors

### Error Cases

- Invalid version format
- Template files missing
- Output directory not writable
- Zip creation failure

---

## Command: `aikit release`

Create GitHub release with package files.

### Signature

```bash
aikit release <VERSION> [OPTIONS]
```

### Arguments

- `VERSION` (required): Version string with 'v' prefix (e.g., `v1.0.0`)

### Options

- `--notes-file <FILE>`: Path to release notes file (default: `release_notes.md`)
- `--github-token <TOKEN>`: GitHub token for API requests

### Input Contract

- **Preconditions**:
  - `VERSION` must match semantic version pattern
  - Package files must exist in `.genreleases/` directory
  - GitHub CLI (`gh`) must be available OR GitHub token provided

### Output Contract

- **Success** (exit code 0):
  - GitHub release created
  - All package files attached as assets
  - Release title: `Spec Kit Templates - <version_without_v>`

- **Failure** (exit code 1):
  - Error message to stderr
  - Release already exists
  - GitHub API errors
  - Missing package files

### Error Cases

- Release already exists
- GitHub API authentication failure
- Missing package files
- GitHub CLI not available (if token not provided)

---

## Common Error Format

All commands use consistent error formatting:

```
Error: <brief description>

<detailed explanation>

<actionable suggestion>
```

Example:
```
Error: Rate limit exceeded

GitHub API rate limit reached (60/60 requests used).
Reset time: 2025-01-27 15:30:00 UTC

To resolve:
- Wait until reset time, or
- Use --github-token to increase limit to 5000/hour
```

---

## Interactive Mode Contract

When `--ai` not provided and stdin is TTY:

1. Display interactive selection UI
2. Show all available agents with descriptions
3. Highlight default selection (copilot)
4. Arrow keys (↑/↓) navigate
5. Enter selects highlighted option
6. Esc or Ctrl+C cancels (exit code 1)

**Output Format**: Matches Python Rich library formatting (cyan highlighting, gray descriptions)

---

## Non-Interactive Mode Contract

When stdin is not TTY:

1. Use default agent (copilot) if `--ai` not provided
2. No interactive prompts
3. All output to stdout/stderr (no TUI)
4. JSON output option available (future)

---

## Exit Codes

- `0`: Success
- `1`: Error (user error, validation failure, operation failure)
- `2`: Internal error (unexpected panic, should not occur)

