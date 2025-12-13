# Spec Kit (github/spec-kit) Feature Inventory

This document enumerates **all user-facing and automation-facing features** present in the `aikit/spec-kit` codebase, at a detail level intended to support a **behaviorally identical reimplementation** (e.g., in Rust).

Scope notes:
- This repository is primarily a **template + workflow generator**. Many “features” are *instructions* embedded in command templates that downstream AI agents execute, plus helper scripts those templates invoke.
- When this document says “MUST/SHOULD”, it reflects **current behavior** or **hard constraints** encoded by the repo (CLI code, scripts, packaging scripts, templates).

---

## 1) Repository high-level product features

### 1.1 Spec-Driven Development (SDD) toolkit
- **Purpose**: Provide a repeatable, structured workflow to go from “feature idea” → “spec” → “plan” → “tasks” → “implementation”.
- **Primary artifacts** (created inside a target project):
  - `.specify/` directory containing:
    - `memory/` (constitution and other persistent guidance)
    - `scripts/` (bash + PowerShell automation)
    - `templates/` (templates used by the workflow)
  - `specs/###-feature-name/` directories for each feature branch/feature directory.
- **Supported AI agents**: multiple agent types, each with its own directory structure + command format (Markdown vs TOML, etc.).

### 1.2 “Specify CLI” (Python) for project bootstrapping
- **Entry point**: installed console script `specify` (configured in `pyproject.toml`).
- **Primary capability**: `specify init` downloads and merges/extracts a template package and (optionally) initializes a Git repo.
- **Secondary capability**: `specify check` reports installed tools.
- **Secondary capability**: `specify version` prints CLI version, platform info, and template latest version (from GitHub).

### 1.3 Release packaging pipeline
- **Goal**: build and publish per-agent, per-shell template zip archives named like:
  - `spec-kit-template-<agent>-<script>-vX.Y.Z.zip`
- **Agents** are packaged separately and differ in generated command file locations and formats.

---

## 2) Python CLI: executable behavior surface (`src/specify_cli/__init__.py`)

### 2.1 CLI application model
- Uses **Typer** for CLI definition and **Rich** for UI.
- `specify` is a Typer app that shows an ASCII banner if invoked without subcommands.
- Commands:
  - **`init`**
  - **`check`**
  - **`version`**

### 2.2 Global/Shared behaviors

#### 2.2.1 GitHub token detection and header behavior
- Token sources (precedence order):
  - CLI arg `--github-token`
  - env `GH_TOKEN`
  - env `GITHUB_TOKEN`
- Empty/whitespace-only tokens are treated as **no token**.
- If token exists: Authorization header `Bearer <token>` is included.
- If no token: Authorization header omitted entirely.

#### 2.2.2 GitHub rate limit error formatting
- When GitHub API returns non-200 status, CLI formats a message including:
  - status code + URL
  - parsed headers (when available): `X-RateLimit-Limit`, `X-RateLimit-Remaining`, `X-RateLimit-Reset`, `Retry-After`
  - local reset time formatting when reset epoch is provided
  - troubleshooting tips explaining unauth vs auth rate limits

#### 2.2.3 TLS verification options
- Default: TLS verification enabled using a `truststore`-backed SSL context.
- `--skip-tls` disables TLS verification (`httpx.Client(verify=False)` equivalent).
- Intended for troubleshooting; described as not recommended.

#### 2.2.4 Tool detection (`check_tool`)
- Uses `shutil.which(tool)` with special handling:
  - For `claude`: if `~/.claude/local/claude` exists, treat as installed even if not on `PATH`.
- When used with a tracker, marks each tool as `available`/`not found`.

### 2.3 Agent configuration (supported AI assistants)

`AGENT_CONFIG` is the single source of truth for:
- **agent key** (must match the actual CLI executable name for CLI-based agents)
- **display name**
- **folder** within generated project (e.g., `.claude/`, `.cursor/`, etc.)
- **install_url** (may be `None` for IDE-based)
- **requires_cli** boolean for whether `specify init` should enforce tool presence (unless overridden)

Current keys include (non-exhaustive grouping):
- CLI-required (examples): `claude`, `gemini`, `qwen`, `opencode`, `codex`, `auggie`, `codebuddy`, `qoder`, `q`, `amp`, `shai`
- IDE-based / no CLI check: `copilot`, `cursor-agent`, `windsurf`, `kilocode`, `roo`, `bob`

### 2.4 Interactive selection UI (arrow-key selector)
- Used when `--ai` or `--script` not provided (and stdin is a TTY).
- Behavior:
  - Renders a list with a cursor, uses ↑/↓ to navigate, Enter to select.
  - Esc or Ctrl+C cancels with exit code 1.
- Default AI selection starts at `copilot`.
- Default script selection: `ps` on Windows; `sh` on non-Windows.
- If stdin not TTY, script selection defaults automatically without prompting.

### 2.5 `specify init` command

#### 2.5.1 Inputs and flags
- `project_name` arg:
  - If `.` then implies `--here`.
  - If omitted and not `--here`, error.
- Flags/options:
  - `--ai <agent_key>`
  - `--script <sh|ps>`
  - `--ignore-agent-tools` (skips agent CLI checks)
  - `--no-git` (skip git initialization)
  - `--here` (initialize in current directory)
  - `--force` (when `--here` in non-empty dir, skip confirmation)
  - `--skip-tls`
  - `--debug` (extra diagnostic output for failures)
  - `--github-token` (auth for GitHub API + downloads)

#### 2.5.2 Target path selection and validation
- If `--here`:
  - Uses `project_path = Path.cwd()`.
  - If directory not empty:
    - Warns it will merge and may overwrite.
    - If `--force` not set: prompts confirm; if declined, exits 0.
- If not `--here`:
  - Uses `project_path = Path(project_name).resolve()`.
  - If directory exists: exits with an error panel.

#### 2.5.3 Git behavior
- Detect if `git` executable exists:
  - If missing: prints warning and skips init.
- If `--no-git`: skip init.
- If target is already a git repo (`git rev-parse --is-inside-work-tree`): skip init with “existing repo detected”.
- Else if git is available: initialize:
  - `git init`
  - `git add .`
  - `git commit -m "Initial commit from Specify template"`
- If git init fails: project still created; CLI prints a warning panel with error details and manual commands.

#### 2.5.4 Agent tool enforcement
- If `--ai` given:
  - Must be a key in `AGENT_CONFIG` else error.
- If no `--ai`: interactive selector chooses.
- If `--ignore-agent-tools` is NOT set:
  - If selected agent has `requires_cli=True`:
    - CLI checks if tool exists (using `check_tool`).
    - If missing: exits with an “Agent Detection Error” panel and suggests `--ignore-agent-tools`.

#### 2.5.5 Template download + extraction
- Downloads **latest GitHub release** from `github/spec-kit`.
- Chooses asset by substring match:
  - pattern: `spec-kit-template-<ai_assistant>-<script_type>` and must end with `.zip`
- If no matching asset:
  - prints available assets and exits 1.
- Download uses streaming, with a progress indicator when not using the Live tracker.
- Extraction behavior:
  - If `--here`:
    - Extract into temp dir; if the zip contains a single top-level directory, “flatten” it.
    - Copy/merge into current directory:
      - Directories: if destination exists, merges file-by-file recursively.
      - Files: overwrites existing.
      - Special handling for `.vscode/settings.json`: deep-merge JSON rather than overwrite.
  - If new directory:
    - Create dir, extract into it.
    - If it results in exactly one nested top-level directory, flatten by moving.
- Cleanup: deletes downloaded zip file at end.
- Failure behavior:
  - If extraction fails and not `--here`: removes created project directory.
  - Exits with code 1.

#### 2.5.6 `.vscode/settings.json` merge behavior
- If destination `.vscode/settings.json` exists:
  - JSON deep-merge:
    - nested dicts merged recursively
    - lists replaced (not merged)
    - scalars overwritten by new content
- If JSON parsing fails: copies new file over with warning.

#### 2.5.7 Script permission normalization
- On non-Windows only:
  - Recursively finds `.specify/scripts/**/*.sh` with a `#!` shebang.
  - Ensures execute bits are set, roughly mirroring read-permission to execute-permission for owner/group/other, but always ensures owner execute.
  - Reports how many updated and how many failed.

#### 2.5.8 Post-init output behaviors
- Always prints “Project ready.”
- Always prints “Next Steps” panel describing `/speckit.*` commands.
- If selected AI is `codex`: prints instructions to set `CODEX_HOME` env var to `<project_path>/.codex`.
- Prints **Agent Folder Security** notice warning to consider `.gitignore` for agent folder.

### 2.6 `specify check` command
- Checks:
  - `git`
  - all agents in `AGENT_CONFIG`:
    - if `requires_cli=True`: checks tool presence
    - else: marks as skipped (“IDE-based, no CLI check”)
  - `code` and `code-insiders`
- Prints tracker tree then “Specify CLI is ready to use!”
- Tips:
  - if git missing: suggests installing git
  - if no agent tools found: suggests installing an AI assistant

### 2.7 `specify version` command
- Prints:
  - CLI version from installed package metadata (`importlib.metadata.version("specify-cli")`)
  - fallback to `pyproject.toml` version if running from source
  - template version and release date from GitHub latest release API
  - Python version, OS platform, architecture, OS version
- GitHub API call is best-effort; failures keep “unknown”.

---

## 3) Template assets (content that is copied into bootstrapped projects)

These templates live in `templates/` in this repo, but in a generated project they appear under `.specify/templates/`.

### 3.1 `templates/spec-template.md`
- Defines the required structure for per-feature `spec.md`:
  - feature name, branch, date, status, input
  - user stories with priorities and independently testable acceptance scenarios
  - edge cases
  - functional requirements with IDs (FR-###), supports explicit “[NEEDS CLARIFICATION: …]”
  - key entities (optional)
  - success criteria (SC-###)

### 3.2 `templates/plan-template.md`
- Defines the required structure for per-feature `plan.md`:
  - technical context fields (Language/Version, dependencies, storage, testing, platform, etc.)
  - constitution check gate section
  - expected feature documentation tree under `specs/<feature>/`
  - expected source layout options (single project vs web app vs mobile+api)
  - complexity tracking table

### 3.3 `templates/tasks-template.md`
- Defines the required structure for per-feature `tasks.md`:
  - strict task checklist item format: `- [ ] T### [P?] [USn?] ...`
  - tasks grouped by user story phases; setup + foundational phases before story phases
  - tests are optional and only included if requested
  - explicit dependency and parallelization guidance

### 3.4 `templates/checklist-template.md`
- Defines the structure for “requirements quality checklists” (unit tests for English):
  - `- [ ] CHK### ...` items grouped into categories

### 3.5 `templates/agent-file-template.md`
- A template for agent context files (e.g. `CLAUDE.md`, `AGENTS.md`, etc.):
  - Active Technologies
  - Project Structure (text tree)
  - Commands
  - Code Style
  - Recent Changes
  - Manual additions preserved between markers

### 3.6 `templates/vscode-settings.json`
- Intended for Copilot/VS Code experience:
  - recommends prompt files
  - auto-approves terminal for `.specify/scripts/bash/` and `.specify/scripts/powershell/`

---

## 4) “Slash command” templates (`templates/commands/*.md`)

These files are not executed directly by this repo at runtime; they are **packaged** into agent-specific command directories and used by AI agent tools.

Important: the packaging pipeline rewrites references:
- `memory/` → `.specify/memory/`
- `scripts/` → `.specify/scripts/`
- `templates/` → `.specify/templates/`

### 4.1 `/speckit.specify` (`templates/commands/specify.md`)
- **Purpose**: create/update a feature spec from natural language.
- **Script**:
  - sh: `scripts/bash/create-new-feature.sh --json "{ARGS}"`
  - ps: `scripts/powershell/create-new-feature.ps1 -Json "{ARGS}"`
- **Key requirements in template**:
  - Generate short branch name (2–4 words).
  - Ensure uniqueness by scanning:
    - remote branches
    - local branches
    - specs directories
  - Run feature creation script with computed `--number` and `--short-name`.
  - Fill `spec.md` from `spec-template.md`.
  - Limit `[NEEDS CLARIFICATION]` markers to **max 3**.
  - Create `checklists/requirements.md` quality checklist and validate spec iteratively.
  - Report branch name + paths and readiness for clarify/plan.

### 4.2 `/speckit.plan` (`templates/commands/plan.md`)
- **Purpose**: generate plan workflow artifacts from a spec.
- **Scripts**:
  - sh: `scripts/bash/setup-plan.sh --json`
  - ps: `scripts/powershell/setup-plan.ps1 -Json`
- **Agent context update**:
  - sh: `scripts/bash/update-agent-context.sh __AGENT__`
  - ps: `scripts/powershell/update-agent-context.ps1 -AgentType __AGENT__`
- **Phases**:
  - Phase 0: research to resolve NEEDS CLARIFICATION
  - Phase 1: data model, contracts, quickstart; run agent context update
  - Must gate on constitution check

### 4.3 `/speckit.tasks` (`templates/commands/tasks.md`)
- **Purpose**: produce `tasks.md` from available design documents.
- **Scripts**:
  - sh: `scripts/bash/check-prerequisites.sh --json`
  - ps: `scripts/powershell/check-prerequisites.ps1 -Json`
- **Rules**:
  - Tasks grouped by user story, ordered by priority.
  - Tests only if explicitly requested.
  - Every task must include file paths; strict formatting enforced.
  - Output includes counts and parallelization.

### 4.4 `/speckit.implement` (`templates/commands/implement.md`)
- **Purpose**: execute implementation by following `tasks.md`.
- **Scripts**:
  - sh: `scripts/bash/check-prerequisites.sh --json --require-tasks --include-tasks`
  - ps: `scripts/powershell/check-prerequisites.ps1 -Json -RequireTasks -IncludeTasks`
- **Checklist gating**:
  - If feature has `checklists/`, compute completion status (checkbox parsing).
  - If any incomplete: must ask user whether to proceed.
- **Project setup verification** (requirements described in template):
  - create/verify ignore files based on detected tech.
  - append missing essential patterns when ignore file exists.
  - detection heuristics include `.gitignore` for git repos, docker/eslint/prettier/npm/terraform/helm patterns, etc.
- **Execution**:
  - execute tasks phase-by-phase, respect `[P]` parallel marker.
  - mark completed tasks as `[X]` in `tasks.md`.

### 4.5 `/speckit.clarify` (`templates/commands/clarify.md`)
- **Purpose**: interactive, sequential clarification of spec ambiguity, up to 5 questions.
- **Scripts**:
  - sh: `scripts/bash/check-prerequisites.sh --json --paths-only`
  - ps: `scripts/powershell/check-prerequisites.ps1 -Json -PathsOnly`
- **Behavior**:
  - loads spec and performs category coverage scan
  - asks one question at a time; each question must be answerable quickly
  - updates spec after each answer by adding/updating `## Clarifications` and applying changes into relevant sections
  - after finishing: reports summary and next command suggestion

### 4.6 `/speckit.analyze` (`templates/commands/analyze.md`)
- **Purpose**: read-only cross-artifact consistency report after tasks generated.
- **Scripts**:
  - sh/ps check-prerequisites with tasks required.
- **Constraints**:
  - MUST NOT modify files.
  - Constitution is non-negotiable; any conflict is CRITICAL.
  - Output a markdown report with findings table, coverage summary, metrics, and next actions.

### 4.7 `/speckit.checklist` (`templates/commands/checklist.md`)
- **Purpose**: generate “unit tests for requirements writing” checklists.
- **Scripts**:
  - sh/ps check-prerequisites.
- **Behavior**:
  - ask up to 3 clarifying questions about checklist purpose; optional up to 2 follow-ups
  - create `checklists/` folder if missing
  - choose filename by domain (e.g., `ux.md`, `security.md`)
  - item IDs start at CHK001
  - each run creates a new file (template text states “never overwrites”, but also says “append if exists”; the intended behavior is: avoid clobbering and preserve prior content)

### 4.8 `/speckit.constitution` (`templates/commands/constitution.md`)
- **Purpose**: update `/memory/constitution.md` by filling placeholders and syncing dependent templates.
- **Behavior**:
  - identify placeholder tokens like `[PROJECT_NAME]`
  - fill from user input or infer from repo context
  - bump constitution version semver based on change magnitude
  - propagate consistency across templates and command files
  - produce a “sync impact report” as an HTML comment at top

### 4.9 `/speckit.taskstoissues` (`templates/commands/taskstoissues.md`)
- **Purpose**: convert tasks into GitHub issues (guarded).
- **Scripts**:
  - sh/ps check-prerequisites with tasks required.
- **Guardrails**:
  - MUST confirm git remote is a GitHub URL.
  - MUST only create issues in the repository that matches the remote.
  - Uses a GitHub MCP tool in environments that support it.

---

## 5) Bash automation scripts (`scripts/bash/*`)

These scripts are copied into generated projects as `.specify/scripts/bash/*`.

### 5.1 `common.sh`
- **Repository root detection**:
  - If git repo: `git rev-parse --show-toplevel`
  - Else: fallback to script location (three levels up).
- **Current feature detection**:
  - If `SPECIFY_FEATURE` env var set: use it.
  - Else if git available: use current branch.
  - Else: find highest-numbered directory under `specs/` matching `^\d{3}-` and use it.
  - Else: fallback `"main"`.
- **Feature directory mapping**:
  - Uses prefix-based matching:
    - Extract `NNN` from branch `NNN-*`
    - find `specs/NNN-*` directory; if exactly one, use it.
    - if none: uses `specs/<branch_name>`
    - if multiple: prints error but still returns `specs/<branch_name>`
- **Outputs** `get_feature_paths` as shell assignments:
  - `REPO_ROOT`, `CURRENT_BRANCH`, `HAS_GIT`, `FEATURE_DIR`, `FEATURE_SPEC`, `IMPL_PLAN`, `TASKS`, `RESEARCH`, `DATA_MODEL`, `QUICKSTART`, `CONTRACTS_DIR`
- **Feature branch validation**:
  - In git repos only: branch must match `^\d{3}-`, else error.
  - Non-git repos: warning, but does not fail.

### 5.2 `check-prerequisites.sh`
- Consolidated prerequisite checker with modes:
  - `--json`: outputs `{"FEATURE_DIR": "...", "AVAILABLE_DOCS":[...]}`
  - `--require-tasks`: fail if tasks missing
  - `--include-tasks`: include tasks in AVAILABLE_DOCS if present
  - `--paths-only`: output minimal path variables (optionally JSON if combined)
- Validations:
  - feature directory exists
  - `plan.md` exists
  - `tasks.md` exists if required
- `AVAILABLE_DOCS` includes optional docs present:
  - `research.md`, `data-model.md`, `contracts/` (only if non-empty), `quickstart.md`, `tasks.md` (optional inclusion)

### 5.3 `create-new-feature.sh`
- Creates a new `specs/NNN-<suffix>/spec.md` and (if git is present) creates/checks out a corresponding git branch.
- Input parsing:
  - supports `--json`, `--short-name <name>`, `--number <N>`, plus the feature description.
  - errors if feature description missing.
- Repo root:
  - if git repo: uses git root
  - else: searches upward for `.git` or `.specify` markers.
- Branch suffix generation:
  - if `--short-name`: uses it after cleaning.
  - else: “smart” generation from description:
    - lowercase, strip non-alphanumeric, split words
    - remove stop words and words <3 chars unless they were uppercase acronyms in original
    - take first 3–4 meaningful words, join with `-`
- Branch number selection:
  - if `--number` provided: use it
  - else:
    - git present: `git fetch --all --prune` best-effort
    - compute highest numeric prefix across *all* branches (local + remote) and *all* `specs/*` directories; next number = max + 1
    - non-git: use highest in `specs/*` directories + 1
- Branch name formatting:
  - `NNN-suffix` where NNN is zero-padded to 3 digits.
  - enforce GitHub branch name 244-byte limit by truncating suffix and warning.
- Output:
  - JSON: `{"BRANCH_NAME":"...","SPEC_FILE":"...","FEATURE_NUM":"..."}`
  - Also exports `SPECIFY_FEATURE` in the process environment.
- Spec file initialization:
  - If `.specify/templates/spec-template.md` exists: copy into new spec file; else create empty file.

### 5.4 `setup-plan.sh`
- Creates/copies `plan.md` in the active feature directory.
- Requires being on a feature branch (in git repos).
- Copies `.specify/templates/plan-template.md` if present; else creates empty file.
- Output JSON includes `FEATURE_SPEC`, `IMPL_PLAN`, `SPECS_DIR` (feature dir), `BRANCH`, `HAS_GIT`.

### 5.5 `update-agent-context.sh`
- Updates agent context files based on `plan.md` fields:
  - `**Language/Version**:`
  - `**Primary Dependencies**:`
  - `**Storage**:`
  - `**Project Type**:`
  - Ignores `NEEDS CLARIFICATION` and `N/A`.
- Supported agent types (argument or “update all existing”):
  - `claude`, `gemini`, `copilot`, `cursor-agent`, `qwen`, `opencode`, `codex`, `windsurf`, `kilocode`, `auggie`, `roo`, `codebuddy`, `qoder`, `amp`, `shai`, `q`, `bob`
- File destinations differ per agent, e.g.:
  - `CLAUDE.md`, `GEMINI.md`, `AGENTS.md`, `.github/agents/copilot-instructions.md`, `.cursor/rules/specify-rules.mdc`, `.windsurf/rules/specify-rules.md`, etc.
- Create-or-update behavior:
  - If target file missing: create from `.specify/templates/agent-file-template.md`.
  - If present: update in-place while preserving manual additions.
- Update semantics:
  - Active Technologies: add new tech entries if not already present.
  - Recent Changes: add a new entry at top; keep only a limited number of prior entries (effectively “last 3” including the new one).
  - Update date stamp if present.

---

## 6) PowerShell automation scripts (`scripts/powershell/*`)

These mirror bash scripts and are copied into generated projects as `.specify/scripts/powershell/*`.

### 6.1 `common.ps1`
- Mirrors `common.sh` behaviors:
  - repo root detection via git else fallback to script dir
  - feature selection via `SPECIFY_FEATURE`, git branch, else highest specs directory
  - feature branch validation when git repo exists
  - path object containing `FEATURE_DIR`, `FEATURE_SPEC`, `IMPL_PLAN`, `TASKS`, etc.

### 6.2 `check-prerequisites.ps1`
- Mirrors `check-prerequisites.sh` flags:
  - `-Json`, `-RequireTasks`, `-IncludeTasks`, `-PathsOnly`
- Same validations and JSON payload shape.

### 6.3 `create-new-feature.ps1`
- Mirrors `create-new-feature.sh`:
  - `-Json`, `-ShortName`, `-Number`
  - same stop words filtering and branch name truncation limit
  - same “max of branch prefixes and specs directories” numbering logic

### 6.4 `setup-plan.ps1`
- Mirrors `setup-plan.sh`.

### 6.5 `update-agent-context.ps1`
- Mirrors `update-agent-context.sh`:
  - `-AgentType` validate-set of agent keys
  - create new agent file from template or update existing
  - update active technologies and recent changes

---

## 7) Release packaging and CI scripts (`.github/workflows/scripts/*`)

### 7.1 Template packaging (Bash): `create-release-packages.sh`
- Builds `.genreleases/spec-kit-template-<agent>-<script>-vX.Y.Z.zip` for each agent and script type.
- Supports `AGENTS=` and `SCRIPTS=` env vars as subsets (comma or space separated).
- Copies base directories into package under `.specify/`:
  - `memory/` → `.specify/memory/`
  - `scripts/bash` or `scripts/powershell` → `.specify/scripts/<variant>/`
  - `templates/` (excluding `templates/commands/*` and `templates/vscode-settings.json`) → `.specify/templates/`
- Generates agent command files from `templates/commands/*.md`:
  - Extracts YAML frontmatter fields:
    - `description:`
    - `scripts: {sh|ps}: ...`
    - optional `agent_scripts: {sh|ps}: ...`
  - Replaces placeholders:
    - `{SCRIPT}` with the correct script command line for the chosen variant
    - `{AGENT_SCRIPT}` if present
    - `{ARGS}` with agent-specific argument placeholder format
    - `__AGENT__` with the agent key
  - Removes `scripts:` and `agent_scripts:` sections from frontmatter in the final output.
  - Rewrites `memory/`, `scripts/`, `templates/` paths to `.specify/...` equivalents.
- Argument placeholder per format:
  - Markdown agents: `$ARGUMENTS`
  - TOML agents: `{{args}}`
- Agent-specific output directories:
  - `claude`: `.claude/commands` (md)
  - `gemini`: `.gemini/commands` (toml)
  - `copilot`: `.github/agents` (agent.md) and also generates `.github/prompts/*.prompt.md`
  - `cursor-agent`: `.cursor/commands` (md)
  - `qwen`: `.qwen/commands` (toml)
  - `opencode`: `.opencode/command` (md)
  - `windsurf`: `.windsurf/workflows` (md)
  - `codex`: `.codex/prompts` (md)
  - `kilocode`: `.kilocode/workflows` (md)
  - `auggie`: `.augment/commands` (md)
  - `roo`: `.roo/commands` (md)
  - `codebuddy`: `.codebuddy/commands` (md)
  - `qoder`: `.qoder/commands` (md)
  - `amp`: `.agents/commands` (md)
  - `shai`: `.shai/commands` (md)
  - `q`: `.amazonq/prompts` (md)
  - `bob`: `.bob/commands` (md)
  - (and others included in the all-agent list)

### 7.2 Template packaging (PowerShell): `create-release-packages.ps1`
- Mirrors `create-release-packages.sh` behavior:
  - same version validation
  - same base copy logic
  - same placeholder rewriting and output directory conventions
  - generates zip via `Compress-Archive`

### 7.3 GitHub release creator: `create-github-release.sh`
- Uses `gh release create <version>` and attaches all `.genreleases/spec-kit-template-...zip` files.
- Uses release title `Spec Kit Templates - <version_without_v>`.
- Uses `--notes-file release_notes.md`.

### 7.4 Version and release note utilities
- `get-next-version.sh`:
  - finds latest tag `vX.Y.Z` or defaults to `v0.0.0`
  - increments patch version only
  - writes GitHub Actions outputs `latest_tag` and `new_version`
- `update-version.sh`:
  - updates `pyproject.toml` version to `X.Y.Z` (strip leading v)
  - intended “for release artifacts only”
- `generate-release-notes.sh`:
  - generates `release_notes.md` from git log since last tag (or last up to 10 commits if first release)
- `check-release-exists.sh`:
  - checks if `gh release view <version>` succeeds and writes GHA output `exists=true|false`

---

## 8) Devcontainer support (`.devcontainer/*`)
- Provides a development container image (Python 3.13 Debian Trixie) with:
  - common utils, dotnet, git, node
  - forwards port 8080 (docs site)
  - installs various VS Code extensions and prompt file recommendations (notably for Copilot chat modes)

---

## 9) Behavioral edge cases and “must-match” details for a Rust reimplementation

This section highlights details that often get missed but are required for “exact same behavior”.

### 9.1 `specify init` behavioral corner cases
- `specify init .` is equivalent to `--here` (and must behave identically).
- `--here` + non-empty dir:
  - prompt unless `--force`
  - merges directories recursively and overwrites files
  - deep-merges `.vscode/settings.json` if present
- ZIP flattening rules:
  - flatten only when extracted root has exactly one directory item.
- Git init:
  - commits immediately with a fixed message.
  - failure does not abort the project creation; it becomes a warning panel after “Project ready.”
- `--skip-tls` changes TLS verify behavior.
- GitHub asset selection:
  - substring match `spec-kit-template-<ai>-<script>` and endswith `.zip`.
- Rate limit messaging: must include reset times and token guidance.

### 9.2 Cross-platform behaviors
- Script type defaults to `ps` on Windows, `sh` otherwise.
- Script chmod normalization is non-Windows only, and only applies to `.sh` with shebang.

### 9.3 Feature/branch numbering rules in scripts
- `create-new-feature.*` determines next feature number by taking the **maximum numeric prefix across**:
  - ALL branches (local + remote), not limited to same suffix
  - ALL `specs/` directories, not limited to same suffix
- It still supports manual override via `--number` / `-Number`.

### 9.4 Non-git repository support (`SPECIFY_FEATURE`)
- `SPECIFY_FEATURE` env var overrides feature selection in scripts.
- Scripts provide fallback behaviors when git isn’t present:
  - “main” as ultimate fallback current feature
  - repository root derived from script location or by searching for `.specify` marker

---

## 10) Index of “feature surfaces” (quick navigation)

### 10.1 CLI commands
- `specify init` (download/extract/merge/git-init/permissions)
- `specify check` (tool detection)
- `specify version` (CLI + template version reporting)

### 10.2 Workflow templates (AI agent commands)
- `/speckit.constitution`
- `/speckit.specify`
- `/speckit.clarify`
- `/speckit.plan`
- `/speckit.tasks`
- `/speckit.analyze`
- `/speckit.checklist`
- `/speckit.implement`
- `/speckit.taskstoissues`

### 10.3 Automation scripts shipped into bootstrapped projects
- `.specify/scripts/bash/common.sh`
- `.specify/scripts/bash/check-prerequisites.sh`
- `.specify/scripts/bash/create-new-feature.sh`
- `.specify/scripts/bash/setup-plan.sh`
- `.specify/scripts/bash/update-agent-context.sh`
- `.specify/scripts/powershell/common.ps1`
- `.specify/scripts/powershell/check-prerequisites.ps1`
- `.specify/scripts/powershell/create-new-feature.ps1`
- `.specify/scripts/powershell/setup-plan.ps1`
- `.specify/scripts/powershell/update-agent-context.ps1`


