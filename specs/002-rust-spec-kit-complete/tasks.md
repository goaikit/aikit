# Tasks: AIKIT - Rust Spec Kit CLI Complete Reimplementation

**Input**: Design documents from `/specs/002-rust-spec-kit-complete/`
**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md, data-model.md, contracts/

**Tests**: Tests are OPTIONAL - not explicitly requested in feature specification. Focus on implementation tasks.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

## Path Conventions

- **Single project**: `src/`, `tests/` at repository root
- Paths shown below follow plan.md structure

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Project initialization and basic structure

- [X] T001 Create project structure per implementation plan in repository root
- [X] T002 Initialize Rust project with Cargo.toml and dependencies in repository root
- [X] T003 [P] Configure Cargo.toml with dependencies: cli-framework, clap, reqwest, zip, serde, serde_json, git2, anyhow, tokio, ratatui, crossterm, walkdir, toml, chrono, yaml-front-matter
- [X] T004 [P] Create src/main.rs with basic CLI structure and command dispatch
- [X] T005 [P] Create src/cli/mod.rs with command module structure
- [X] T006 [P] Create src/core/mod.rs with core module structure
- [X] T007 [P] Create src/tui/mod.rs with TUI module structure
- [X] T008 [P] Create src/github/mod.rs with GitHub module structure
- [X] T009 [P] Create src/fs/mod.rs with filesystem module structure
- [X] T010 [P] Create src/config/mod.rs with config module structure
- [X] T011 [P] Create tests/ directory structure: unit/, integration/, fixtures/
- [X] T012 [P] Configure rustfmt.toml and clippy lints in repository root

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core infrastructure that MUST be complete before ANY user story can be implemented

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T013 Create ScriptVariant enum in src/core/agent.rs
- [X] T014 Create OutputFormat enum in src/core/agent.rs (Markdown, TOML, AgentMd)
- [X] T015 [P] Create AgentConfig struct in src/core/agent.rs with all fields from data-model
- [X] T016 [P] Create hardcoded AGENT_CONFIG list with all 17 agents in src/core/agent.rs (claude, gemini, copilot, cursor-agent, qwen, opencode, codex, windsurf, kilocode, auggie, roo, codebuddy, qoder, amp, shai, q, bob)
- [X] T017 [P] Implement agent validation functions in src/core/agent.rs
- [X] T018 [P] Create ProjectPath struct in src/core/template.rs
- [X] T019 [P] Implement ProjectPath validation logic in src/core/template.rs
- [X] T020 [P] Create GitHubRateLimitInfo struct in src/github/rate_limit.rs
- [X] T021 [P] Implement rate limit header parsing in src/github/rate_limit.rs
- [X] T022 [P] Create error formatting functions for rate limit errors in src/github/rate_limit.rs
- [X] T023 [P] Create GitHub API client structure in src/github/api.rs
- [X] T024 [P] Implement GitHub token resolution (CLI arg, GH_TOKEN, GITHUB_TOKEN) in src/github/api.rs
- [X] T025 [P] Create TemplateAsset struct in src/core/template.rs
- [X] T026 [P] Implement template asset filename parsing in src/core/template.rs
- [X] T027 [P] Create file system utilities module in src/fs/mod.rs
- [X] T028 [P] Create output formatting utilities in src/tui/output.rs (panels, tables, trees matching Python Rich)
- [X] T029 [P] Create error handling infrastructure with anyhow in src/main.rs
- [X] T030 [P] Create configuration loading utilities in src/config/agent_config.rs

**Checkpoint**: Foundation ready - user story implementation can now begin in parallel

---

## Phase 3: User Story 1 - Initialize New Project (Priority: P1) 🎯 MVP

**Goal**: Initialize a new Spec-Driven Development project in a new directory by running `aikit init <project-name>`, so that users can start using the SDD workflow immediately.

**Independent Test**: Run `aikit init test-project --ai claude` and verify that:
- A new directory `test-project` is created
- Template files are downloaded and extracted correctly
- Agent-specific command files are present in the correct locations
- Git repository is initialized (if git is available)
- Script permissions are set correctly on Unix systems
- Output messages match Python version format

### Implementation for User Story 1

- [X] T031 [US1] Create clap command definition for `aikit init` in src/cli/init.rs
- [X] T032 [US1] Implement argument parsing for init command (project_name, --ai, --script, --here, --force, --no-git, --github-token, --skip-tls, --debug, --ignore-agent-tools) in src/cli/init.rs
- [X] T033 [US1] Implement ProjectPath creation and validation in src/core/template.rs
- [X] T034 [US1] Implement agent selection resolution (--ai flag or default) in src/core/agent.rs
- [X] T035 [US1] Implement script variant detection (--script flag or platform default) in src/core/agent.rs
- [X] T036 [US1] Implement GitHub release API call to get latest release in src/github/api.rs
- [X] T037 [US1] Implement template asset selection by agent and script variant in src/core/template.rs
- [X] T038 [US1] Implement template zip download from GitHub in src/github/api.rs
- [X] T039 [US1] Implement ZIP archive extraction to temporary directory in src/core/template.rs
- [X] T040 [US1] Implement single top-level directory flattening logic in src/core/template.rs
- [X] T041 [US1] Implement file copying from temp to target directory in src/fs/mod.rs
- [X] T042 [US1] Implement directory creation for new projects in src/fs/mod.rs
- [X] T043 [US1] Implement Git repository initialization in src/core/git.rs
- [X] T044 [US1] Implement existing Git repository detection in src/core/git.rs
- [X] T045 [US1] Implement script permission setting for .sh files on Unix in src/fs/permissions.rs
- [X] T046 [US1] Implement agent-specific command file directory creation in src/core/template.rs
- [X] T047 [US1] Implement success message output matching Python format in src/tui/output.rs
- [X] T048 [US1] Implement Codex-specific setup instructions (CODEX_HOME environment variable) when codex agent is selected in src/core/agent.rs
- [X] T049 [US1] Implement agent folder security notice after successful initialization in src/tui/output.rs
- [X] T050 [US1] Implement error handling for all init failure cases in src/cli/init.rs
- [X] T051 [US1] Wire up init command in src/main.rs command dispatch

**Checkpoint**: At this point, User Story 1 should be fully functional and testable independently (without interactive selection)

---

## Phase 4: User Story 2 - Check Installed Tools (Priority: P1) 🎯 MVP

**Goal**: Run `aikit check` to verify that required tools (git, AI agent CLIs) are installed, so that users know what's available before starting a project.

**Independent Test**: Run `aikit check` and verify:
- All tools in AGENT_CONFIG are checked (CLI tools if `requires_cli=True`, skipped if IDE-based)
- Git is checked
- VS Code variants (`code`, `code-insiders`) are checked
- Output format matches Python version (tracker tree format)
- Exit code is 0 regardless of tool availability

### Implementation for User Story 2

- [X] T052 [US2] Create clap command definition for `aikit check` in src/cli/check.rs
- [X] T053 [US2] Create ToolCheckResult struct in src/core/tools.rs
- [X] T054 [US2] Implement tool detection function (check if executable exists on PATH) in src/core/tools.rs
- [X] T055 [US2] Implement Claude CLI special case detection (~/.claude/local/claude) in src/core/tools.rs
- [X] T056 [US2] Implement Git tool check in src/core/tools.rs
- [X] T057 [US2] Implement VS Code tool checks (code, code-insiders) in src/core/tools.rs
- [X] T058 [US2] Implement agent CLI tool checks (iterate through AGENT_CONFIG, check if requires_cli=True) in src/core/tools.rs
- [X] T059 [US2] Implement IDE-based agent detection (mark as "IDE-based, no CLI check") in src/core/tools.rs
- [X] T060 [US2] Implement tree format output for tool check results in src/tui/output.rs
- [X] T061 [US2] Implement check command main logic in src/cli/check.rs
- [X] T062 [US2] Wire up check command in src/main.rs command dispatch

**Checkpoint**: At this point, User Story 2 should be fully functional and testable independently

---

## Phase 5: User Story 3 - Display Version Information (Priority: P2)

**Goal**: Run `aikit version` to see the CLI version, template version, and system information, so that users can verify they're using the latest version and troubleshoot issues.

**Independent Test**: Run `aikit version` and verify:
- CLI version is displayed (from Cargo.toml or package metadata)
- Template version is fetched from GitHub latest release API
- System information (OS, architecture, Rust version) is displayed
- Format matches Python version (panel with table)
- Graceful handling when GitHub API is unavailable (shows "unknown")

### Implementation for User Story 3

- [X] T063 [US3] Create clap command definition for `aikit version` in src/cli/version.rs
- [X] T064 [US3] Implement CLI version extraction from Cargo.toml in src/cli/version.rs
- [X] T065 [US3] Implement system information collection (OS, architecture, Rust version) in src/cli/version.rs
- [X] T066 [US3] Implement GitHub latest release API call for template version in src/github/api.rs
- [X] T067 [US3] Implement graceful error handling for GitHub API failures (show "unknown") in src/cli/version.rs
- [X] T068 [US3] Implement panel format output with table matching Python Rich format in src/tui/output.rs
- [X] T069 [US3] Implement version command main logic in src/cli/version.rs
- [X] T070 [US3] Wire up version command in src/main.rs command dispatch

**Checkpoint**: At this point, User Story 3 should be fully functional and testable independently

---

## Phase 6: User Story 4 - Interactive Agent Selection (Priority: P2)

**Goal**: Use arrow keys to interactively select an AI agent when `--ai` is not specified, so that users can choose from available options without remembering exact agent keys.

**Independent Test**: Run `aikit init my-project` (without `--ai`) in an interactive terminal and verify:
- Arrow key navigation works (↑/↓)
- Enter selects the highlighted option
- Esc or Ctrl+C cancels with exit code 1
- Default selection starts at "copilot"
- Output format matches Python version (cyan highlighting, gray descriptions)

### Implementation for User Story 4

- [X] T071 [US4] Create AgentSelection enum in src/core/agent.rs
- [X] T072 [US4] Implement TTY detection (stdin.is_tty()) in src/cli/init.rs
- [X] T073 [US4] Create interactive agent selection view using ratatui in src/tui/agent_select.rs
- [X] T074 [US4] Implement agent list display with descriptions in src/tui/agent_select.rs
- [X] T075 [US4] Implement arrow key navigation (↑/↓) in src/tui/agent_select.rs
- [X] T076 [US4] Implement Enter key selection in src/tui/agent_select.rs
- [X] T077 [US4] Implement Esc/Ctrl+C cancellation in src/tui/agent_select.rs
- [X] T078 [US4] Implement default selection highlighting (copilot) in src/tui/agent_select.rs
- [X] T079 [US4] Implement color formatting (cyan highlighting, gray descriptions) matching Python in src/tui/agent_select.rs
- [X] T080 [US4] Integrate interactive selection into init command flow in src/cli/init.rs
- [X] T081 [US4] Implement non-interactive fallback (default to copilot) in src/cli/init.rs

**Checkpoint**: At this point, User Story 4 should be fully functional and integrated with User Story 1

---

## Phase 7: User Story 5 - GitHub Rate Limit Handling (Priority: P2)

**Goal**: Provide clear error messages when GitHub API rate limits are hit, so that users understand what happened and how to resolve it.

**Independent Test**: Simulate rate limit responses and verify:
- Error message includes rate limit information (limit, remaining, reset time)
- Troubleshooting tips are provided
- Suggestion to use `--github-token` is included
- Format matches Python version exactly

### Implementation for User Story 5

- [X] T082 [US5] Enhance rate limit error detection in GitHub API responses in src/github/api.rs
- [X] T083 [US5] Implement rate limit error message formatting with limit, remaining, reset time in src/github/rate_limit.rs
- [X] T084 [US5] Implement troubleshooting tips in rate limit error messages in src/github/rate_limit.rs
- [X] T085 [US5] Implement Retry-After header parsing and inclusion in error messages in src/github/rate_limit.rs
- [X] T086 [US5] Implement authenticated vs unauthenticated rate limit messaging (5000 vs 60) in src/github/rate_limit.rs
- [X] T087 [US5] Integrate rate limit error handling into all GitHub API calls (init, version) in src/github/api.rs
- [X] T088 [US5] Test rate limit error formatting matches Python version exactly (implemented in github::rate_limit module)

**Checkpoint**: At this point, User Story 5 should be fully functional and integrated across all GitHub API operations

---

## Phase 8: User Story 6 - Build Release Packages (Priority: P3)

**Goal**: Build template zip archives for all agent/script combinations using `aikit package <version>`, so that maintainers can publish releases with all template variants.

**Independent Test**: Run `aikit package v1.0.0` and verify:
- Template zip files are created for all agent/script combinations
- Files are generated with correct naming pattern: `spec-kit-template-<agent>-<script>-vX.Y.Z.zip`
- Command files are generated with correct placeholders replaced
- Path rewrites are applied correctly (memory/ → .specify/memory/, etc.)
- Agent-specific output directories and formats are correct

### Implementation for User Story 6

- [X] T089 [US6] Create clap command definition for `aikit package` in src/cli/package.rs
- [X] T090 [US6] Create PackageConfig struct in src/core/package.rs
- [X] T091 [US6] Implement version format validation (vX.Y.Z pattern) in src/core/package.rs
- [X] T092 [US6] Implement agent filter parsing from AGENTS environment variable in src/core/package.rs
- [X] T093 [US6] Implement script filter parsing from SCRIPTS environment variable in src/core/package.rs
- [X] T094 [US6] Create CommandTemplate struct in src/core/package.rs
- [X] T095 [US6] Implement YAML frontmatter parsing for command templates in src/core/package.rs
- [X] T096 [US6] Implement placeholder replacement ({SCRIPT}, {AGENT_SCRIPT}, {ARGS}, __AGENT__) in src/core/package.rs
- [X] T097 [US6] Implement path rewriting (memory/ → .specify/memory/, scripts/ → .specify/scripts/, templates/ → .specify/templates/) in src/core/package.rs
- [X] T098 [US6] Implement frontmatter script section removal in src/core/package.rs
- [X] T099 [US6] Implement base directory copying (memory, scripts, templates) with exclusions in src/core/package.rs
- [X] T100 [US6] Implement agent-specific output directory creation in src/core/package.rs
- [X] T101 [US6] Implement agent-specific output format handling (Markdown, TOML, agent.md) in src/core/package.rs
- [X] T102 [US6] Implement Copilot prompt file generation (.github/prompts/*.prompt.md) in src/core/package.rs
- [X] T103 [US6] Implement ZIP archive creation for each agent/script combination in src/core/package.rs
- [X] T104 [US6] Implement package naming: spec-kit-template-<agent>-<script>-<version>.zip in src/core/package.rs
- [X] T105 [US6] Implement output directory creation (.genreleases/) in src/cli/package.rs
- [X] T106 [US6] Implement package command main logic in src/cli/package.rs
- [X] T107 [US6] Wire up package command in src/main.rs command dispatch

**Checkpoint**: At this point, User Story 6 should be fully functional and testable independently

---

## Phase 9: File Merging & Deep JSON Merge

**Purpose**: Implement file merging logic for --here flag, including deep JSON merge for .vscode/settings.json

- [X] T108 [US1] Implement file existence detection before copy in src/fs/merge.rs
- [X] T109 [US1] Implement basic file merge logic (overwrite vs skip) in src/fs/merge.rs
- [X] T110 [US1] Implement deep JSON merge for .vscode/settings.json (nested objects merged, arrays replaced, scalars overwritten) in src/fs/merge.rs
- [X] T111 [US1] Implement invalid JSON handling in merge operations in src/fs/merge.rs
- [X] T112 [US1] Implement --here flag file merging in init command flow in src/cli/init.rs
- [X] T113 [US1] Implement non-empty directory detection and confirmation prompt in src/cli/init.rs
- [X] T114 [US1] Implement --force flag to skip confirmation in src/cli/init.rs

---

## Phase 10: Release Command (FR-044)

**Purpose**: Implement `aikit release` command to create GitHub releases with all package files attached

**Independent Test**: Run `aikit release v1.0.0` and verify:
- GitHub release is created with correct tag
- All package files from `.genreleases/` are attached as assets
- Release title matches pattern: "Spec Kit Templates - <version_without_v>"
- Release notes are included from file (if provided)

### Implementation for Release Command

- [X] T115 [US7] Create clap command definition for `aikit release` in src/cli/release.rs
- [X] T116 [US7] Implement version format validation (vX.Y.Z pattern) in src/cli/release.rs
- [X] T117 [US7] Implement package file discovery in .genreleases/ directory in src/cli/release.rs
- [X] T118 [US7] Implement GitHub CLI detection and availability check in src/cli/release.rs
- [X] T119 [US7] Implement GitHub release creation via `gh release create` command in src/cli/release.rs
- [X] T120 [US7] Implement release title formatting (Spec Kit Templates - <version>) in src/cli/release.rs
- [X] T121 [US7] Implement release notes file handling (--notes-file option) in src/cli/release.rs
- [X] T122 [US7] Implement asset attachment (all .zip files from .genreleases/) in src/cli/release.rs
- [X] T123 [US7] Implement error handling for release already exists case in src/cli/release.rs
- [X] T124 [US7] Implement error handling for missing package files in src/cli/release.rs
- [X] T125 [US7] Wire up release command in src/main.rs command dispatch

**Checkpoint**: At this point, Release command should be fully functional and testable independently

---

## Phase 11: Polish & Cross-Cutting Concerns

**Purpose**: Improvements that affect multiple user stories

- [ ] T126 [P] Implement comprehensive error messages with actionable suggestions across all commands
- [X] T127 [P] Implement --debug flag for verbose diagnostic output in src/cli/mod.rs
- [X] T128 [P] Implement --skip-tls flag for troubleshooting in src/github/api.rs
- [X] T129 [P] Implement cross-platform path handling (Windows vs Unix) in src/fs/mod.rs (normalize_path, path_to_string, join_paths, home_dir utilities)
- [X] T130 [P] Implement branch name validation against GitHub's 244-byte limit in src/core/git.rs
- [X] T131 [P] Implement . (dot) as project_name equivalent to --here in src/cli/init.rs
- [X] T132 [P] Implement --ignore-agent-tools flag to skip CLI tool validation in src/cli/init.rs
- [ ] T133 [P] Run quickstart.md validation and update if needed
- [X] T134 [P] Documentation updates in README.md (created comprehensive README with usage, features, installation, and examples)
- [X] T135 [P] Code cleanup and refactoring (formatting, unused imports removed, clippy warnings fixed)
- [ ] T136 [P] Performance optimization (startup time, download speed, extraction speed)
- [ ] T137 [P] Cross-platform testing (Linux, macOS, Windows)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies - can start immediately
- **Foundational (Phase 2)**: Depends on Setup completion - BLOCKS all user stories
- **User Stories (Phase 3+)**: All depend on Foundational phase completion
  - User stories can then proceed in parallel (if staffed)
  - Or sequentially in priority order (P1 → P2 → P3)
- **File Merging (Phase 9)**: Depends on User Story 1 completion
- **Release Command (Phase 10)**: Depends on User Story 6 completion (packages must exist)
- **Polish (Phase 11)**: Depends on all desired user stories being complete

### User Story Dependencies

- **User Story 1 (P1)**: Can start after Foundational (Phase 2) - No dependencies on other stories
- **User Story 2 (P1)**: Can start after Foundational (Phase 2) - Independent, can run parallel with US1
- **User Story 3 (P2)**: Can start after Foundational (Phase 2) - Independent, can run parallel with US1/US2
- **User Story 4 (P2)**: Depends on User Story 1 completion - Enhances US1 with interactive selection
- **User Story 5 (P2)**: Can start after Foundational (Phase 2) - Cross-cutting, affects US1 and US3
- **User Story 6 (P3)**: Can start after Foundational (Phase 2) - Independent, can run parallel with other stories
- **Release Command (US7)**: Depends on User Story 6 completion - Requires packages to exist before creating release

### Within Each User Story

- Models/structs before services
- Services before CLI commands
- Core implementation before integration
- Story complete before moving to next priority

### Parallel Opportunities

- All Setup tasks marked [P] can run in parallel
- All Foundational tasks marked [P] can run in parallel (within Phase 2)
- Once Foundational phase completes, User Stories 1, 2, 3, 5, 6 can start in parallel (if team capacity allows)
- User Story 4 must wait for User Story 1 completion
- File merging tasks (Phase 9) can run in parallel within the phase
- All Polish tasks marked [P] can run in parallel

---

## Parallel Example: User Story 1

```bash
# Launch foundational structs/models in parallel:
Task: "Create ScriptVariant enum in src/core/agent.rs"
Task: "Create OutputFormat enum in src/core/agent.rs"
Task: "Create AgentConfig struct in src/core/agent.rs"
Task: "Create ProjectPath struct in src/core/template.rs"
Task: "Create GitHubRateLimitInfo struct in src/github/rate_limit.rs"
Task: "Create TemplateAsset struct in src/core/template.rs"

# Launch GitHub API implementation in parallel:
Task: "Create GitHub API client structure in src/github/api.rs"
Task: "Implement GitHub token resolution in src/github/api.rs"
Task: "Implement rate limit header parsing in src/github/rate_limit.rs"
```

---

## Parallel Example: User Story 2

```bash
# Launch tool detection functions in parallel:
Task: "Implement tool detection function in src/core/tools.rs"
Task: "Implement Claude CLI special case detection in src/core/tools.rs"
Task: "Implement Git tool check in src/core/tools.rs"
Task: "Implement VS Code tool checks in src/core/tools.rs"
```

---

## Implementation Strategy

### MVP First (User Stories 1 & 2 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL - blocks all stories)
3. Complete Phase 3: User Story 1 (Initialize New Project)
4. Complete Phase 4: User Story 2 (Check Installed Tools)
5. **STOP and VALIDATE**: Test both stories independently
6. Deploy/demo if ready

### Incremental Delivery

1. Complete Setup + Foundational → Foundation ready
2. Add User Story 1 → Test independently → Deploy/Demo (MVP!)
3. Add User Story 2 → Test independently → Deploy/Demo
4. Add User Story 3 → Test independently → Deploy/Demo
5. Add User Story 4 → Test independently → Deploy/Demo (enhances US1)
6. Add User Story 5 → Test independently → Deploy/Demo (cross-cutting)
7. Add User Story 6 → Test independently → Deploy/Demo (maintainer tool)
8. Each story adds value without breaking previous stories

### Parallel Team Strategy

With multiple developers:

1. Team completes Setup + Foundational together
2. Once Foundational is done:
   - Developer A: User Story 1 (Initialize)
   - Developer B: User Story 2 (Check Tools)
   - Developer C: User Story 3 (Version Info)
3. After US1 complete:
   - Developer A: User Story 4 (Interactive Selection)
   - Developer B: User Story 5 (Rate Limit Handling)
4. Developer C: User Story 6 (Package Generation)
5. Stories complete and integrate independently

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story for traceability
- Each user story should be independently completable and testable
- Commit after each task or logical group
- Stop at any checkpoint to validate story independently
- Avoid: vague tasks, same file conflicts, cross-story dependencies that break independence
- User Story 4 enhances User Story 1 but should be implemented as separate phase
- User Story 5 is cross-cutting and affects multiple commands
- User Story 6 is for maintainers, not end users
- **Version Management Utilities (FR-045)**: get-next-version, update-version, generate-release-notes, check-release-exists are primarily for CI/CD workflows. These may be implemented as separate commands or scripts in a future phase. For MVP, focus on core CLI commands.
- **Edge Cases**: See `edge-cases-mapping.md` for detailed mapping of edge cases to requirements and tasks.

---

## Summary

- **Total Tasks**: 137
- **Tasks per User Story**:
  - User Story 1: 21 tasks (T031-T051, includes Codex setup and security notice)
  - User Story 2: 11 tasks (T052-T062)
  - User Story 3: 8 tasks (T063-T070)
  - User Story 4: 11 tasks (T071-T081)
  - User Story 5: 7 tasks (T082-T088)
  - User Story 6: 19 tasks (T089-T107)
  - File Merging: 7 tasks (T108-T114)
  - Release Command (US7): 11 tasks (T115-T125)
  - Polish: 12 tasks (T126-T137)
- **Parallel Opportunities**: Many tasks marked [P] can run in parallel within their phases
- **Independent Test Criteria**: Each user story has clear independent test criteria
- **Suggested MVP Scope**: User Stories 1 & 2 (P1 priorities) - Initialize and Check commands
- **Format Validation**: All tasks follow checklist format with checkbox, ID, optional [P] marker, optional [Story] label, and file paths

