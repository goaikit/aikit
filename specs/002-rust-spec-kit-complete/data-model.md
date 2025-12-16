# Data Model: AIKIT Rust Implementation

**Date**: 2025-01-27  
**Feature**: 002-rust-spec-kit-complete

## Core Entities

### AgentConfig

Represents an AI agent configuration with all metadata needed for initialization and tool checking.

```rust
pub struct AgentConfig {
    pub key: String,              // Executable name (e.g., "claude", "gemini")
    pub name: String,             // Display name (e.g., "Claude", "Google Gemini")
    pub folder: String,           // Project directory (e.g., ".claude", ".gemini")
    pub install_url: Option<String>, // Optional installation URL
    pub requires_cli: bool,        // Whether agent requires CLI tool check
    pub output_format: OutputFormat, // Command file format (Markdown, TOML, AgentMd)
    pub output_dir: String,       // Output directory for command files
    pub arg_placeholder: String,  // Argument placeholder format ("$ARGUMENTS" or "{{args}}")
}
```

**Validation Rules**:
- `key` must be non-empty and valid identifier (alphanumeric + hyphens)
- `name` must be non-empty
- `folder` must be valid directory name (no path separators)
- `output_dir` must be valid relative path

**Relationships**:
- One-to-many with `CommandTemplate` (each agent has multiple command templates)

---

### CommandTemplate

Represents a command template file with metadata and body content.

```rust
pub struct CommandTemplate {
    pub name: String,              // Template filename (e.g., "specify.md")
    pub description: String,        // Description from YAML frontmatter
    pub script_commands: HashMap<ScriptVariant, String>, // Script commands per variant
    pub agent_script_commands: Option<HashMap<ScriptVariant, String>>, // Optional agent-specific scripts
    pub body: String,               // Template body content (after frontmatter)
}
```

**Validation Rules**:
- `name` must end with `.md` or `.toml` depending on agent format
- `description` must be non-empty
- `script_commands` must contain entries for both `Sh` and `Ps` variants
- `body` must contain placeholder markers (`{SCRIPT}`, `{AGENT_SCRIPT}`, `{ARGS}`, `__AGENT__`)

**State Transitions**:
- `Parsed` → `Processed` (after placeholder replacement)
- `Processed` → `Written` (after file generation)

---

### ProjectPath

Represents a target project location with validation.

```rust
pub struct ProjectPath {
    pub path: PathBuf,             // Absolute or relative path
    pub is_here: bool,             // Whether using --here flag
    pub exists: bool,              // Whether path already exists
    pub is_empty: bool,            // Whether existing directory is empty
}
```

**Validation Rules**:
- If `!is_here` and `exists`, validation fails (unless `--force`)
- Path must be valid filesystem path
- If `is_here`, path must be directory (or current directory)

**State Transitions**:
- `Validated` → `Created` (for new directories)
- `Validated` → `Merged` (for --here with existing content)

---

### TemplateAsset

Represents a downloadable template zip file from GitHub releases.

```rust
pub struct TemplateAsset {
    pub filename: String,          // Asset filename (e.g., "spec-kit-template-copilot-sh-v1.0.0.zip")
    pub size: u64,                 // File size in bytes
    pub release_tag: String,        // Release tag (e.g., "v1.0.0")
    pub download_url: String,      // GitHub API download URL
    pub agent: String,             // Extracted agent key
    pub script_variant: ScriptVariant, // Extracted script variant
}
```

**Validation Rules**:
- `filename` must match pattern: `spec-kit-template-<agent>-<script>-v<version>.zip`
- `download_url` must be valid HTTPS URL
- `release_tag` must match semantic version pattern `vX.Y.Z`

---

### GitHubRateLimitInfo

Represents rate limit information parsed from GitHub API headers.

```rust
pub struct GitHubRateLimitInfo {
    pub limit: u32,                // Total rate limit (60 or 5000)
    pub remaining: u32,            // Remaining requests
    pub reset_epoch: i64,          // Reset time as Unix timestamp
    pub reset_time: DateTime<Utc>, // Reset time as DateTime
    pub retry_after_seconds: Option<u64>, // Optional Retry-After header value
}
```

**Validation Rules**:
- `limit` must be positive
- `remaining` must be <= `limit`
- `reset_epoch` must be valid Unix timestamp

**State Transitions**:
- `Parsed` → `Exceeded` (when `remaining == 0` and current time < `reset_time`)

---

### PackageConfig

Represents packaging configuration for release builds.

```rust
pub struct PackageConfig {
    pub version: String,           // Version with 'v' prefix (e.g., "v1.0.0")
    pub agents: Option<Vec<String>>, // Optional agent filter list
    pub scripts: Option<Vec<ScriptVariant>>, // Optional script type filter
    pub output_dir: PathBuf,        // Output directory (default: ".genreleases/")
}
```

**Validation Rules**:
- `version` must match pattern `vX.Y.Z` (semantic version)
- If `agents` provided, all must be valid agent keys
- If `scripts` provided, must contain valid `ScriptVariant` values
- `output_dir` must be writable directory

---

### ScriptVariant

Enum representing script type (bash or PowerShell).

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScriptVariant {
    Sh,  // Bash script (.sh)
    Ps,  // PowerShell script (.ps1)
}
```

**State Transitions**: None (enum, no state)

**Default Selection**:
- Windows: `Ps`
- Unix-like: `Sh`

---

## Derived/Computed Entities

### AgentSelection

Represents user's agent selection (interactive or CLI argument).

```rust
pub enum AgentSelection {
    Selected(String),    // Agent key selected
    Interactive,        // Trigger interactive selection
    Default,            // Use default (copilot)
}
```

**State Transitions**:
- `Interactive` → `Selected(String)` (after user selection)
- `Default` → `Selected("copilot")` (implicit selection)

---

### ToolCheckResult

Represents result of tool availability check.

```rust
pub struct ToolCheckResult {
    pub tool_name: String,
    pub available: bool,
    pub path: Option<PathBuf>,     // Path where tool was found
    pub is_ide_based: bool,        // Whether tool is IDE-based (skip check)
    pub message: String,           // Status message for display
}
```

**State Transitions**: None (result type, no state)

---

### MergeResult

Represents result of file merging operation.

```rust
pub enum MergeResult {
    Created,            // File was created (didn't exist)
    Merged,             // File was merged (existed, merged successfully)
    Overwritten,        // File was overwritten (existed, no merge logic)
    Skipped,            // File was skipped (conflict resolution)
}
```

**State Transitions**: None (result type, no state)

---

## Validation Rules Summary

### Cross-Entity Validation

1. **Agent-Template Consistency**: All `CommandTemplate` instances for an agent must use the agent's `output_format`
2. **Path-Project Consistency**: `ProjectPath.is_here` must be consistent with path being current directory or explicitly set
3. **Package-Agent Consistency**: `PackageConfig.agents` filter must only contain valid agent keys from `AgentConfig` list

### State Machine Rules

1. **Template Processing**: Must follow `Parsed` → `Processed` → `Written` sequence
2. **Project Initialization**: Must follow `Validated` → `Created/Merged` → `Initialized` sequence
3. **Rate Limit Handling**: Must check `GitHubRateLimitInfo` before API calls, handle `Exceeded` state gracefully

---

## Data Flow

### Initialization Flow

```
User Input (CLI args)
  → ProjectPath (validation)
  → AgentSelection (resolve agent)
  → TemplateAsset (download from GitHub)
  → TemplateAsset (extract to temp)
  → FileSystem (merge/copy to target)
  → GitRepository (initialize if needed)
  → ProjectPath (finalize)
```

### Package Generation Flow

```
PackageConfig (validate)
  → AgentConfig list (filter if needed)
  → CommandTemplate list (load templates)
  → CommandTemplate (process placeholders)
  → FileSystem (copy base directories)
  → FileSystem (write command files)
  → ZipArchive (create zip files)
  → PackageConfig (output to .genreleases/)
```

