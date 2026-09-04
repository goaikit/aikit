//! Eval runner implementation using aikit-sdk

use crate::artifacts::{
    aggregate_trials, CaseResult, CaseStatus, CaseTrialsResult, IsolationReport, ScopeFidelity,
    TerminalRecord, TokenBreakdown, TrialResult,
};
use crate::checks::{
    count_command_events, effective_checks, run_checks_in_context, suite_passes, CheckContext,
    CheckDefinition,
};
use crate::codex_home::CodexScratchHome;
use crate::suite::EvalCase;
use crate::trace::{
    agent_events_to_trace, terminal_cost_usd, terminal_outcome, trace_to_jsonl, TraceEvent,
    TracePayload,
};
use aikit_sdk::runner::{Backend, KnobSupport};
use aikit_sdk::{run_agent_events, AgentEvent, RunOptions, SkillIsolation, TerminalOutcome};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// Where an isolated case's `SKILL.md` comes from (spec 016 D1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillSource {
    /// SKILL.md content held in memory (the optimize loop already has it).
    Inline(String),
    /// A skill directory on disk containing `SKILL.md` (and support files) —
    /// the eval path reads it off the skill project.
    Dir(PathBuf),
}

/// Environment isolation for one eval case (spec 016 D1).
///
/// The skill identity lives *inside* the `Isolated` variant: the runner
/// cannot materialize a skill it cannot name, and `(Isolated, no skill)` is
/// unrepresentable by construction rather than a runtime error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IsolationMode {
    /// Per-case scratch workspace containing only this skill, plus
    /// per-backend user-scope suppression (spec 016 D2/D3).
    Isolated {
        skill_name: String,
        source: SkillSource,
    },
    /// Legacy behaviour: run in `project_root`, inherit everything.
    Inherit,
}

/// Options for running a single eval case.
///
/// Deliberately has **no `Default` impl** (spec 016 D1): adding a field is a
/// compile error at every construction site, forcing each caller to make an
/// explicit isolation decision instead of silently inheriting one.
#[derive(Debug, Clone)]
pub struct CaseRunOptions {
    /// Agent key (e.g. "codex", "claude")
    pub agent_key: String,
    /// Optional model override
    pub model: Option<String>,
    /// Skill project root. Under [`IsolationMode::Inherit`] this is the
    /// working directory; under `Isolated` it is only the source of declared
    /// `workspace_subdir` fixture contents (spec 016 D2).
    pub project_root: PathBuf,
    /// Timeout in seconds
    pub timeout_seconds: u64,
    /// Per-case trial aggregation pass threshold (0.0-1.0)
    pub pass_threshold: f64,
    /// Environment isolation for this case (spec 016 D1). See [`IsolationMode`].
    pub isolation: IsolationMode,
    /// Where to move a failed scratch workspace so it survives for debugging
    /// (spec 016 D2). `None` = always delete. The caller owns the policy; the
    /// runner owns the mechanism.
    pub retain_workspace_in: Option<PathBuf>,
}

/// A per-case scratch workspace handed back to the caller so post-run scoring
/// (e.g. `file_exists` in the optimize path) can still see it. A `Scratch`
/// workspace is deleted when this value is dropped; a `Retained` one was
/// moved under the caller's retention dir and survives.
#[derive(Debug)]
pub struct CaseWorkspace {
    root: WorkspaceRoot,
    working_dir: PathBuf,
    /// Every file under `root` as seeded, before the agent ran: the baseline
    /// `workspace.diff` is taken against (spec eval-judge R10).
    seed: crate::workspace_diff::TreeSnapshot,
}

#[derive(Debug)]
enum WorkspaceRoot {
    Scratch(tempfile::TempDir),
    Retained(PathBuf),
}

impl CaseWorkspace {
    /// Root of the workspace (the parent of e.g. `.claude/skills`).
    pub fn root(&self) -> &Path {
        match &self.root {
            WorkspaceRoot::Scratch(dir) => dir.path(),
            WorkspaceRoot::Retained(path) => path,
        }
    }

    /// The directory the agent ran in (`root` or `root/<workspace_subdir>`).
    pub fn working_dir(&self) -> &Path {
        &self.working_dir
    }

    /// True when the workspace was moved to a retention dir (failed case).
    pub fn is_retained(&self) -> bool {
        matches!(self.root, WorkspaceRoot::Retained(_))
    }

    /// Unified diff of the workspace as it stands now against its seeded
    /// state (spec eval-judge R10). Empty when the agent changed nothing;
    /// binary and oversized files are named without their contents.
    pub fn diff_from_seed(&self) -> std::io::Result<String> {
        crate::workspace_diff::diff_against_seed(&self.seed, self.root())
    }

    /// Move a failed scratch workspace under `retain_in` so it survives for
    /// debugging (spec 016 D2), printing the surviving path.
    fn retain_into(self, retain_in: &Path, case_id: &str) -> CaseWorkspace {
        let subdir_rel = self
            .working_dir
            .strip_prefix(self.root())
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        match self.root {
            WorkspaceRoot::Retained(_) => self,
            WorkspaceRoot::Scratch(dir) => {
                let mut dest = retain_in.join(format!("workspace-{case_id}"));
                let mut n = 1;
                while dest.exists() {
                    n += 1;
                    dest = retain_in.join(format!("workspace-{case_id}-{n}"));
                }
                let src = dir.keep();
                let root = match std::fs::create_dir_all(retain_in)
                    .and_then(|_| std::fs::rename(&src, &dest))
                {
                    Ok(()) => dest,
                    Err(_) => src, // cross-device or IO failure: keep in place
                };
                eprintln!(
                    "eval: retained failed isolated workspace for case '{case_id}' at {}",
                    root.display()
                );
                let working_dir = root.join(subdir_rel);
                CaseWorkspace {
                    root: WorkspaceRoot::Retained(root),
                    working_dir,
                    seed: self.seed,
                }
            }
        }
    }
}

/// Raw output from running a case
#[derive(Debug)]
pub struct CaseRunOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// The scratch workspace the case ran in, when isolated (spec 016 D2).
    /// Kept alive on the output so callers that score post-run (the optimize
    /// path) still see the filesystem; dropped ⇒ deleted (unless retained).
    pub workspace: Option<CaseWorkspace>,
    /// What environment the case actually got (spec 016 D6). Report-only —
    /// see [`IsolationReport`]; must never feed a `CheckResult`.
    pub isolation: Option<IsolationReport>,
    /// Unified diff of the scratch workspace against its seeded state, taken
    /// before the workspace is discarded (spec eval-judge R10). `None` when
    /// there was no seeded workspace (inherited or degraded environment): then
    /// no `workspace.diff` is written, because an empty diff would claim
    /// "nothing changed" about a tree nobody baselined.
    pub workspace_diff: Option<String>,
}

/// Errors during case execution
#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("EVAL_AGENT_UNAVAILABLE: Agent '{0}' is not available")]
    AgentUnavailable(String),
    #[error("EVAL_CASE_TIMEOUT: Case timed out after {0}s")]
    Timeout(u64),
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
}

/// Abstraction over eval case execution (default: aikit-backed).
#[async_trait]
pub trait EvalRunner: Send + Sync {
    /// Run one case, produce stdout/stderr capture, scored result, and canonical trace JSONL.
    async fn run_case(
        &self,
        case: &EvalCase,
        opts: &CaseRunOptions,
        checks: &[CheckDefinition],
    ) -> (CaseRunOutput, CaseResult, String);

    /// Run multiple trials for one case, returning the aggregated result.
    async fn run_case_trials(
        &self,
        case: &EvalCase,
        opts: &CaseRunOptions,
        checks: &[CheckDefinition],
        trial_count: u32,
        max_parallelism: Option<u32>,
    ) -> CaseTrialsResult;
}

/// Default runner: `aikit_sdk::run_agent_events` inside `spawn_blocking` with SDK timeout/cwd.
///
/// Holds the run-scoped codex scratch `CODEX_HOME` (spec 016 D3): allocated
/// lazily on the first isolated codex case, shared by every clone of this
/// runner, and dropped (write-back check + unconditional deletion — it holds
/// credentials) when the last clone goes away at the end of the eval run.
#[derive(Debug, Clone, Default)]
pub struct AikitEvalRunner {
    codex_home: Arc<OnceLock<Option<CodexScratchHome>>>,
}

impl AikitEvalRunner {
    pub fn new() -> Self {
        Self::default()
    }

    /// The scratch `CODEX_HOME` path, allocating it on first use. `None` when
    /// allocation failed (already warned loudly); user scope then degrades.
    fn codex_home_path(&self) -> Option<PathBuf> {
        self.codex_home
            .get_or_init(CodexScratchHome::allocate)
            .as_ref()
            .map(|h| h.path().to_path_buf())
    }
}

/// Result of agent execution within spawn_blocking
struct AgentExecutionResult {
    result: Result<aikit_sdk::RunResult, aikit_sdk::RunError>,
    events: Vec<AgentEvent>,
}

#[async_trait]
impl EvalRunner for AikitEvalRunner {
    async fn run_case(
        &self,
        case: &EvalCase,
        opts: &CaseRunOptions,
        checks: &[CheckDefinition],
    ) -> (CaseRunOutput, CaseResult, String) {
        self.run_case_inner(case, opts, checks).await
    }

    async fn run_case_trials(
        &self,
        case: &EvalCase,
        opts: &CaseRunOptions,
        checks: &[CheckDefinition],
        trial_count: u32,
        max_parallelism: Option<u32>,
    ) -> CaseTrialsResult {
        let max_parallel = max_parallelism
            .unwrap_or_else(|| num_cpus::get().max(1) as u32)
            .max(1) as usize;
        let semaphore = Arc::new(Semaphore::new(max_parallel));
        let mut join_set: JoinSet<TrialResult> = JoinSet::new();

        for trial_id in 1..=trial_count {
            let permit = Arc::clone(&semaphore);
            let case_clone = case.clone();
            let opts_clone = opts.clone();
            let checks_vec = checks.to_vec();
            let runner = self.clone();

            join_set.spawn(async move {
                let Ok(_permit) = permit.acquire().await else {
                    return TrialResult {
                        trial_id,
                        status: CaseStatus::Error,
                        command_count: None,
                        input_tokens: None,
                        output_tokens: None,
                        check_results: vec![],
                        error_message: Some(
                            "EVAL_PARALLEL_EXHAUSTION: semaphore closed".to_string(),
                        ),
                        exit_code: None,
                        terminal: None,
                        cost_usd: None,
                        tokens: TokenBreakdown::default(),
                        skill_path: None,
                        judge_excluded: false,
                    };
                };
                let (_output, case_result, _trace) = runner
                    .run_case_inner(&case_clone, &opts_clone, &checks_vec)
                    .await;
                TrialResult {
                    trial_id,
                    status: case_result.status,
                    command_count: case_result.command_count,
                    input_tokens: case_result.input_tokens,
                    output_tokens: case_result.output_tokens,
                    check_results: case_result.check_results,
                    error_message: case_result.error_message,
                    exit_code: case_result.exit_code,
                    terminal: case_result.terminal,
                    cost_usd: case_result.cost_usd,
                    tokens: case_result.tokens,
                    skill_path: case_result.skill_path,
                    judge_excluded: false,
                }
            });
        }

        let mut trials = Vec::with_capacity(trial_count as usize);
        while let Some(res) = join_set.join_next().await {
            match res {
                Ok(trial) => trials.push(trial),
                Err(e) => {
                    // Join errors are treated as failed trials.
                    let next_id = (trials.len() as u32) + 1;
                    trials.push(TrialResult {
                        trial_id: next_id,
                        status: CaseStatus::Error,
                        command_count: None,
                        input_tokens: None,
                        output_tokens: None,
                        check_results: vec![],
                        error_message: Some(format!("EVAL_PARALLEL_EXHAUSTION: {}", e)),
                        exit_code: None,
                        terminal: None,
                        cost_usd: None,
                        tokens: TokenBreakdown::default(),
                        skill_path: None,
                        judge_excluded: false,
                    });
                }
            }
        }

        aggregate_trials(&case.id, trials, trial_count, opts.pass_threshold)
    }
}

/// Outcome of preparing an isolated workspace (spec 016 D2).
enum IsolationSetup {
    /// Scratch workspace ready; `payload` is the spec-016 D3 envelope knob.
    Ready {
        workspace: CaseWorkspace,
        payload: SkillIsolation,
    },
    /// Isolation could not be achieved on this backend (e.g. opencode has no
    /// skills deploy path); the case degrades to `Inherit` with the recorded
    /// reason — it never silently claims isolation (spec 016 D4).
    Degraded { reason: String },
}

/// Materialize the skill under test into a fresh scratch workspace (spec 016
/// D2): deploy via the agent catalog's canonical skills path, then honor
/// `workspace_subdir` *inside* the scratch root by copying the declared
/// fixture contents from `project_root/<subdir>`.
fn setup_isolated_workspace(
    agent_key: &str,
    skill_name: &str,
    source: &SkillSource,
    workspace_subdir: Option<&Path>,
    project_root: &Path,
    codex_home: Option<PathBuf>,
) -> Result<IsolationSetup, String> {
    let skill_md = match source {
        SkillSource::Inline(text) => text.clone(),
        SkillSource::Dir(dir) => std::fs::read_to_string(dir.join("SKILL.md")).map_err(|e| {
            format!(
                "EVAL_ISOLATION_SOURCE_UNREADABLE: cannot read {}: {e}",
                dir.join("SKILL.md").display()
            )
        })?,
    };

    let scratch = tempfile::TempDir::new()
        .map_err(|e| format!("EVAL_ISOLATION_WORKSPACE: cannot create scratch workspace: {e}"))?;

    // The in-process aikit agent is not a deploy-catalog row; its skills root
    // is `<workdir>/.aikit/skills` (AgentConfig::from_env) and isolation is
    // emulated by pointing `AgentConfig.skills_dirs` at exactly that root.
    let skill_md_path = if agent_key == "aikit" {
        let dir = scratch.path().join(".aikit/skills").join(skill_name);
        std::fs::create_dir_all(&dir)
            .and_then(|_| {
                let p = dir.join("SKILL.md");
                std::fs::write(&p, &skill_md).map(|_| p)
            })
            .map_err(|e| format!("EVAL_ISOLATION_WORKSPACE: cannot materialize skill: {e}"))?
    } else {
        match aikit_sdk::deploy_skill(agent_key, scratch.path(), skill_name, &skill_md, None) {
            Ok(p) => p,
            Err(aikit_sdk::DeployError::UnsupportedConcept { .. }) => {
                return Ok(IsolationSetup::Degraded {
                    reason: format!(
                        "agent '{agent_key}' has no skills path in the deploy catalog; \
                         running with the inherited environment instead"
                    ),
                });
            }
            Err(aikit_sdk::DeployError::AgentNotFound(_)) => {
                return Ok(IsolationSetup::Degraded {
                    reason: format!(
                        "agent '{agent_key}' is not in the deploy catalog; \
                         running with the inherited environment instead"
                    ),
                });
            }
            Err(e) => {
                return Err(format!(
                    "EVAL_ISOLATION_WORKSPACE: cannot materialize skill: {e}"
                ));
            }
        }
    };
    let skill_dir = skill_md_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| skill_md_path.clone());

    // A Dir source may ship support files next to SKILL.md — copy them too.
    if let SkillSource::Dir(dir) = source {
        copy_dir_contents(dir, &skill_dir, &["SKILL.md"])
            .map_err(|e| format!("EVAL_ISOLATION_WORKSPACE: cannot copy skill files: {e}"))?;
    }

    // `workspace_subdir` keeps its meaning — a relative path inside the run
    // workspace — but under isolation the run workspace is the scratch dir,
    // not `project_root` (the one user-visible semantic shift, spec 016 D2).
    let working_dir = match workspace_subdir {
        Some(subdir) => {
            let dest = scratch.path().join(subdir);
            std::fs::create_dir_all(&dest)
                .map_err(|e| format!("EVAL_ISOLATION_WORKSPACE: cannot create subdir: {e}"))?;
            let fixture_src = project_root.join(subdir);
            if fixture_src.is_dir() {
                copy_dir_contents(&fixture_src, &dest, &[])
                    .map_err(|e| format!("EVAL_ISOLATION_WORKSPACE: cannot copy fixtures: {e}"))?;
            }
            dest
        }
        None => scratch.path().to_path_buf(),
    };

    // Baseline for `workspace.diff` (spec eval-judge R10): taken once the skill
    // and fixtures are in place and before the agent runs.
    let seed = crate::workspace_diff::snapshot_tree(scratch.path())
        .map_err(|e| format!("EVAL_ISOLATION_WORKSPACE: cannot snapshot seeded workspace: {e}"))?;

    let payload = SkillIsolation {
        workspace_root: scratch.path().to_path_buf(),
        skill_path: skill_dir,
        skill_name: skill_name.to_string(),
        codex_home,
    };
    Ok(IsolationSetup::Ready {
        workspace: CaseWorkspace {
            root: WorkspaceRoot::Scratch(scratch),
            working_dir,
            seed,
        },
        payload,
    })
}

/// Recursively copy the contents of `src` into `dst`, skipping top-level
/// names in `skip`.
fn copy_dir_contents(src: &Path, dst: &Path, skip: &[&str]) -> std::io::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if skip.iter().any(|s| name == std::ffi::OsStr::new(s)) {
            continue;
        }
        let target = dst.join(&name);
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_dir_contents(&entry.path(), &target, &[])?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Best-effort parse of claude's `system`/`init` event skills list from
/// captured stdout (spec 016 D6).
///
/// **REPORT-ONLY.** The capability listing is worthless as evidence of skill
/// *invocation* — feeding it to scoring produced the spec-015 false-pass bug.
/// It is exactly the right evidence of *environment*, and that is all it may
/// ever be used for: nothing derived from this function may feed a
/// `CheckResult`.
pub fn parse_claude_ambient_skills(stdout: &str) -> Vec<String> {
    claude_init_event(stdout)
        .and_then(|init| {
            init.get("skills").and_then(|s| s.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|e| {
                        e.as_str()
                            .map(str::to_string)
                            .or_else(|| e.get("name").and_then(|n| n.as_str()).map(str::to_string))
                    })
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// Best-effort agent version from claude's init event (report-only, see
/// [`parse_claude_ambient_skills`]).
fn parse_claude_agent_version(stdout: &str) -> Option<String> {
    claude_init_event(stdout)?
        .get("version")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn claude_init_event(stdout: &str) -> Option<serde_json::Value> {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line.trim()).ok())
        .find(|v| {
            v.get("type").and_then(|t| t.as_str()) == Some("system")
                && v.get("subtype").and_then(|t| t.as_str()) == Some("init")
        })
}

/// The per-backend user-scope mechanism label for the report (spec 016 D6).
fn isolation_mechanism(backend: Backend) -> Option<&'static str> {
    match backend.skill_isolation_support() {
        KnobSupport::Unsupported => None,
        _ => Some(match backend {
            Backend::Claude => "--setting-sources project",
            Backend::Codex => "scratch CODEX_HOME",
            Backend::Pi => "--no-skills --skill",
            Backend::Aikit => "AgentConfig.skills_dirs override",
            Backend::Gemini | Backend::Cursor | Backend::OpenCode => unreachable!(),
        }),
    }
}

impl AikitEvalRunner {
    async fn run_case_inner(
        &self,
        case: &EvalCase,
        opts: &CaseRunOptions,
        checks: &[CheckDefinition],
    ) -> (CaseRunOutput, CaseResult, String) {
        let agent_key = opts.agent_key.clone();
        let model = opts.model.clone();
        let prompt = case.prompt.clone();
        let timeout_secs = opts.timeout_seconds;
        let requested = matches!(opts.isolation, IsolationMode::Isolated { .. });

        // ── spec 016 D2: prepare the per-case scratch workspace ─────────────
        // The workspace is bound to `workspace` (a local that outlives the
        // agent run below) — a dropped TempDir deletes the dir mid-run.
        let mut workspace: Option<CaseWorkspace> = None;
        let mut payload: Option<SkillIsolation> = None;
        let mut degrade_reason: Option<String> = None;

        if let IsolationMode::Isolated { skill_name, source } = &opts.isolation {
            let codex_home = if agent_key == "codex" {
                self.codex_home_path()
            } else {
                None
            };
            match setup_isolated_workspace(
                &agent_key,
                skill_name,
                source,
                case.workspace_subdir.as_deref(),
                &opts.project_root,
                codex_home,
            ) {
                Ok(IsolationSetup::Ready {
                    workspace: ws,
                    payload: p,
                }) => {
                    workspace = Some(ws);
                    payload = Some(p);
                }
                Ok(IsolationSetup::Degraded { reason }) => {
                    eprintln!(
                        "warning: skill isolation degraded to inherit on agent '{agent_key}': \
                         {reason}"
                    );
                    degrade_reason = Some(reason);
                }
                Err(message) => {
                    // A broken isolation setup must be an Error, not a silent
                    // fall-through to the ambient environment.
                    let output = CaseRunOutput {
                        stdout: vec![],
                        stderr: message.clone().into_bytes(),
                        exit_code: None,
                        timed_out: false,
                        workspace: None,
                        isolation: None,
                        workspace_diff: None,
                    };
                    let result = CaseResult {
                        id: case.id.clone(),
                        status: CaseStatus::Error,
                        command_count: None,
                        input_tokens: None,
                        output_tokens: None,
                        check_results: vec![],
                        error_message: Some(message),
                        exit_code: None,
                        terminal: None,
                        cost_usd: None,
                        tokens: TokenBreakdown::default(),
                        skill_path: None,
                    };
                    return (output, result, String::new());
                }
            }
            // D4: backends without a user-scope mechanism still run — project
            // scope is isolated, and the gap is said out loud plus recorded.
            if workspace.is_some()
                && Backend::from_key(&agent_key)
                    .map(|b| b.skill_isolation_support() == KnobSupport::Unsupported)
                    .unwrap_or(true)
            {
                eprintln!(
                    "warning: agent '{agent_key}' cannot isolate user-scope skills; project \
                     scope is isolated, user scope is not (recorded in the isolation report)"
                );
            }
        }

        let working_dir = workspace
            .as_ref()
            .map(|w| w.working_dir().to_path_buf())
            .unwrap_or_else(|| match &case.workspace_subdir {
                Some(subdir) => opts.project_root.join(subdir),
                None => opts.project_root.clone(),
            });

        let mut run_opts = RunOptions::new()
            .with_yolo(true)
            .with_stream(true)
            .with_timeout(Duration::from_secs(timeout_secs))
            .with_current_dir(working_dir.clone())
            .with_emit_token_usage_events(true);
        if let Some(p) = payload.clone() {
            run_opts = run_opts.with_skill_isolation(p);
        }
        if let Some(model_name) = model {
            if !model_name.trim().is_empty() {
                run_opts = run_opts.with_model(model_name);
            }
        }

        let spawn_result = tokio::task::spawn_blocking(move || {
            let mut events: Vec<AgentEvent> = Vec::new();
            let result = run_agent_events(&agent_key, &prompt, run_opts, |ev| {
                events.push(ev.clone());
            });
            AgentExecutionResult { result, events }
        });

        // Captured agent exec/spawn failure. When set, the case is an Error no
        // matter what the checks say: an empty trace makes negative-expectation
        // and limit checks pass vacuously, which must not mask a run that never
        // produced output.
        let mut exec_error: Option<String> = None;

        let (mut run_output, trace_events, token_usage) = match spawn_result.await {
            Ok(exec_result) => match exec_result.result {
                Ok(run_result) => {
                    let token_usage = run_result.token_usage.clone();
                    let exit_code = run_result.exit_code();
                    let output = CaseRunOutput {
                        stdout: run_result.stdout,
                        stderr: run_result.stderr,
                        exit_code,
                        timed_out: false,
                        workspace: None,
                        isolation: None,
                        workspace_diff: None,
                    };
                    let trace = agent_events_to_trace(&exec_result.events);
                    (output, trace, token_usage)
                }
                Err(aikit_sdk::RunError::TimedOut {
                    timeout, stderr, ..
                }) => {
                    let mut trace = agent_events_to_trace(&exec_result.events);
                    trace.push(TraceEvent {
                        seq: trace.len(),
                        payload: TracePayload::Timeout,
                    });
                    let output = CaseRunOutput {
                        stdout: vec![],
                        stderr,
                        exit_code: None,
                        timed_out: true,
                        workspace: None,
                        isolation: None,
                        workspace_diff: None,
                    };
                    if output.stderr.is_empty() {
                        let fallback = format!("Case timed out after {}s", timeout.as_secs());
                        let output = CaseRunOutput {
                            stdout: vec![],
                            stderr: fallback.into_bytes(),
                            exit_code: None,
                            timed_out: true,
                            workspace: None,
                            isolation: None,
                            workspace_diff: None,
                        };
                        (output, trace, None)
                    } else {
                        (output, trace, None)
                    }
                }
                Err(e) => {
                    let message = format!("Agent execution failed: {}", e);
                    exec_error = Some(message.clone());
                    let trace = agent_events_to_trace(&exec_result.events);
                    let output = CaseRunOutput {
                        stdout: vec![],
                        stderr: message.into_bytes(),
                        exit_code: None,
                        timed_out: false,
                        workspace: None,
                        isolation: None,
                        workspace_diff: None,
                    };
                    (output, trace, None)
                }
            },
            Err(e) => {
                let message = format!("spawn_blocking failed: {}", e);
                exec_error = Some(message.clone());
                let output = CaseRunOutput {
                    stdout: vec![],
                    stderr: message.into_bytes(),
                    exit_code: None,
                    timed_out: false,
                    workspace: None,
                    isolation: None,
                    workspace_diff: None,
                };
                (output, vec![], None)
            }
        };

        let trace_jsonl = trace_to_jsonl(&trace_events);
        let stdout_str = String::from_utf8_lossy(&run_output.stdout).to_string();
        let command_count = count_command_events(&trace_jsonl);

        let backend = Backend::from_key(&opts.agent_key);
        let capabilities = backend.map(|b| b.capabilities());
        let skill_path = payload
            .as_ref()
            .map(|p| p.skill_path.to_string_lossy().to_string());
        let ctx = CheckContext {
            backend: &opts.agent_key,
            structured_tools: capabilities.map(|c| c.structured_tools).unwrap_or(true),
            // `Option::is_none_or` is stable since 1.82; the workspace MSRV
            // is 1.75.
            typed_skill_tool: match backend {
                Some(b) => b == Backend::Claude,
                None => true,
            },
            skill_path: skill_path.as_deref(),
        };
        // Per-case selection plus the check implied by `should_trigger`.
        let case_checks = effective_checks(checks, &case.id, case.should_trigger);
        let check_results = run_checks_in_context(&case_checks, &trace_jsonl, &working_dir, &ctx);
        let all_passed = suite_passes(&check_results);

        // ── the agent's own verdict ─────────────────────────────────────────
        // Recorded whether or not it decides the status, because it is the only
        // record of *why* a run that exited zero produced nothing.
        let terminal =
            terminal_outcome(&trace_events).map(|(outcome, reason, message)| TerminalRecord {
                outcome,
                reason,
                message,
            });
        let cost_usd = terminal_cost_usd(&trace_events);

        // A run that produced no measurement is `error`, never `failed` and
        // never `passed`. Over an empty trace a negative expectation passes and
        // a tool-call ceiling passes, so an outage would score as perfect
        // restraint; on a positive suite the same outage scores as total
        // failure. Polarity decides the direction of the lie, which is why no
        // default is safe and the transport signal has to decide.
        let no_measurement = if run_output.timed_out || exec_error.is_some() {
            // Transport failure: nothing reached us.
            true
        } else if run_output.exit_code.is_some_and(|c| c != 0) {
            // The process itself failed.
            true
        } else if terminal
            .as_ref()
            .is_some_and(|t| t.outcome == TerminalOutcome::Error)
        {
            // The agent said so. pi retries a provider timeout three times,
            // gives up, and still exits zero — this is the only signal.
            true
        } else {
            // Stream ended with no terminal event. Only an error on a backend
            // that declares it emits one: three backends are wrapped as
            // text-only and emit no structured frames at all, and marking every
            // one of their trials `error` would be a lie about them rather than
            // a measurement (their flag flips the day their decoder is fixed).
            terminal.is_none() && capabilities.is_some_and(|c| c.terminal_event)
        };

        let status = if no_measurement {
            CaseStatus::Error
        } else if case_checks.is_empty() {
            if run_output.exit_code == Some(0) {
                CaseStatus::Passed
            } else {
                CaseStatus::Failed
            }
        } else if all_passed {
            CaseStatus::Passed
        } else {
            CaseStatus::Failed
        };

        let case_result = CaseResult {
            id: case.id.clone(),
            status: status.clone(),
            command_count: Some(command_count),
            input_tokens: token_usage.as_ref().map(|u| u.input_tokens),
            output_tokens: token_usage.as_ref().map(|u| u.output_tokens),
            check_results,
            error_message: if run_output.timed_out {
                Some(format!(
                    "EVAL_CASE_TIMEOUT: Case timed out after {}s",
                    timeout_secs
                ))
            } else if let Some(message) = exec_error {
                Some(message)
            } else if status == CaseStatus::Error {
                Some(no_measurement_reason(
                    &run_output,
                    terminal.as_ref(),
                    &opts.agent_key,
                ))
            } else {
                None
            },
            exit_code: run_output.exit_code,
            terminal,
            cost_usd,
            tokens: TokenBreakdown {
                total_tokens: token_usage.as_ref().and_then(|u| u.total_tokens),
                cache_read_tokens: token_usage.as_ref().and_then(|u| u.cache_read_tokens),
                cache_creation_tokens: token_usage.as_ref().and_then(|u| u.cache_creation_tokens),
                reasoning_tokens: token_usage.as_ref().and_then(|u| u.reasoning_tokens),
            },
            // Recorded per trial because each isolated trial stages the skill
            // into its own scratch directory: this is the path that appears in
            // *this* trace, and no other trial's.
            skill_path: payload.as_ref().map(|p| p.skill_path.clone()),
        };

        // ── spec eval-judge R10: what the agent wrote, taken before the
        // workspace is discarded. A passing trial's workspace does not survive,
        // and this is the only record of it.
        let workspace_diff = workspace.as_ref().and_then(|ws| match ws.diff_from_seed() {
            Ok(diff) => Some(diff),
            Err(e) => {
                eprintln!(
                    "warning: could not diff the workspace for case '{}': {e}; \
                     no workspace.diff will be written for this trial",
                    case.id
                );
                None
            }
        });

        // ── spec 016 D2 retention: delete on success (via drop), move under
        // the caller's retention dir on failure so the case is debuggable.
        let workspace = match workspace {
            Some(ws) if status != CaseStatus::Passed => match &opts.retain_workspace_in {
                Some(retain_in) => Some(ws.retain_into(retain_in, &case.id)),
                None => Some(ws),
            },
            other => other,
        };

        // ── spec 016 D6: record what the case actually got (report-only).
        run_output.isolation = Some(build_isolation_report(
            &opts.agent_key,
            requested,
            degrade_reason,
            workspace.as_ref().map(|w| w.root().to_path_buf()),
            payload.as_ref(),
            &stdout_str,
        ));
        run_output.workspace = workspace;
        run_output.workspace_diff = workspace_diff;

        (run_output, case_result, trace_jsonl)
    }
}

/// Say *why* a trial produced no measurement, in the artifact rather than only
/// in a log line nobody reads back.
fn no_measurement_reason(
    output: &CaseRunOutput,
    terminal: Option<&TerminalRecord>,
    agent_key: &str,
) -> String {
    if let Some(code) = output.exit_code.filter(|c| *c != 0) {
        return format!("EVAL_TRIAL_ERROR: agent exited {code}");
    }
    match terminal {
        Some(t) if t.outcome == TerminalOutcome::Error => {
            let detail = t
                .message
                .as_deref()
                .or(t.reason.as_deref())
                .unwrap_or("no detail");
            format!("EVAL_TRIAL_ERROR: agent reported failure: {detail}")
        }
        _ => format!("EVAL_TRIAL_ERROR: agent '{agent_key}' stream ended with no terminal event"),
    }
}

/// Assemble the spec-016 D6 [`IsolationReport`] for one case. Pure and
/// report-only: nothing here may feed a `CheckResult`.
fn build_isolation_report(
    agent_key: &str,
    requested: bool,
    degrade_reason: Option<String>,
    workspace_root: Option<PathBuf>,
    payload: Option<&SkillIsolation>,
    stdout: &str,
) -> IsolationReport {
    let backend = Backend::from_key(agent_key);
    let project_scope = if requested && workspace_root.is_some() {
        ScopeFidelity::Isolated
    } else {
        ScopeFidelity::Inherited
    };
    let (user_scope, mechanism, degrade_reason) = if !requested || workspace_root.is_none() {
        (ScopeFidelity::Inherited, None, degrade_reason)
    } else {
        match backend.map(|b| b.skill_isolation_support()) {
            Some(KnobSupport::Unsupported) | None => (ScopeFidelity::Unsupported, None, {
                Some(degrade_reason.unwrap_or_else(|| {
                    format!("agent '{agent_key}' has no user-scope isolation mechanism")
                }))
            }),
            Some(_) => {
                // codex's mechanism is the scratch CODEX_HOME; if it could
                // not be allocated the user scope silently inheriting would
                // be a lie — record it.
                let codex_degraded = backend == Some(Backend::Codex)
                    && payload.map(|p| p.codex_home.is_none()).unwrap_or(true);
                if codex_degraded {
                    (
                        ScopeFidelity::Inherited,
                        None,
                        Some(
                            "scratch CODEX_HOME could not be allocated; codex user scope not \
                             isolated"
                                .to_string(),
                        ),
                    )
                } else {
                    (
                        ScopeFidelity::Isolated,
                        backend.and_then(isolation_mechanism).map(str::to_string),
                        degrade_reason,
                    )
                }
            }
        }
    };
    let ambient_skills = if backend == Some(Backend::Claude) {
        parse_claude_ambient_skills(stdout)
    } else {
        Vec::new()
    };
    IsolationReport {
        requested,
        project_scope,
        user_scope,
        mechanism,
        agent_version: parse_claude_agent_version(stdout),
        ambient_skills,
        workspace_root,
        degrade_reason,
    }
}

/// Run a single eval case using the default [`AikitEvalRunner`].
pub async fn run_eval_case(
    case: &EvalCase,
    opts: &CaseRunOptions,
    checks: &[CheckDefinition],
) -> (CaseRunOutput, CaseResult, String) {
    AikitEvalRunner::new().run_case(case, opts, checks).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::CaseStatus;
    use std::path::Path;

    #[cfg(unix)]
    fn write_fake_agent(dir: &Path, name: &str, body: &str) {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let path = dir.join(name);
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "#!/bin/sh\n{body}").unwrap();
        let mut perms = file.metadata().unwrap().permissions();
        perms.set_mode(0o755);
        file.set_permissions(perms).unwrap();
    }

    #[cfg(unix)]
    fn write_fake_claude(dir: &Path) {
        write_fake_agent(
            dir,
            "claude",
            r#"printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"ok","session_id":"stub-session"}'"#,
        );
    }

    /// Prepend `dir` to PATH for the duration of the test (nextest runs each
    /// test in its own process, so this cannot race another test).
    #[cfg(unix)]
    fn prepend_path(dir: &Path) {
        let previous = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{}", dir.display(), previous));
    }

    fn isolated_opts(agent_key: &str, project_root: PathBuf) -> CaseRunOptions {
        CaseRunOptions {
            agent_key: agent_key.to_string(),
            model: None,
            project_root,
            timeout_seconds: 10,
            pass_threshold: 1.0,
            isolation: IsolationMode::Isolated {
                skill_name: "my-skill".to_string(),
                source: SkillSource::Inline("# My Skill\n".to_string()),
            },
            retain_workspace_in: None,
        }
    }

    /// A case for tests about plumbing (staging, workspaces, exit codes), run
    /// against a fake agent that never reads the skill.
    ///
    /// `should_trigger` is `false` because under R7 the column is scored: it
    /// generates a `skill_invoked` check with matching polarity, and claiming
    /// `true` here would assert an invocation the stub cannot make. Tests
    /// about triggering itself set the column explicitly.
    fn simple_case(id: &str) -> EvalCase {
        EvalCase {
            id: id.to_string(),
            prompt: "p".to_string(),
            should_trigger: false,
            tags: vec![],
            workspace_subdir: None,
            extra: Default::default(),
        }
    }

    /// Stub runner for trait wiring tests (no aikit).
    struct StubEvalRunner;

    #[async_trait]
    impl EvalRunner for StubEvalRunner {
        async fn run_case(
            &self,
            case: &EvalCase,
            _opts: &CaseRunOptions,
            _checks: &[CheckDefinition],
        ) -> (CaseRunOutput, CaseResult, String) {
            let trace_jsonl =
                r#"{"seq":0,"payload":{"type":"raw_line","line":"stub"}}"#.to_string();
            let out = CaseRunOutput {
                stdout: b"ok".to_vec(),
                stderr: vec![],
                exit_code: Some(0),
                timed_out: false,
                workspace: None,
                isolation: None,
                workspace_diff: None,
            };
            let result = CaseResult {
                id: case.id.clone(),
                status: CaseStatus::Passed,
                command_count: Some(0),
                input_tokens: Some(100),
                output_tokens: Some(50),
                check_results: vec![],
                error_message: None,
                cost_usd: None,
                exit_code: None,
                terminal: None,
                tokens: Default::default(),
                skill_path: None,
            };
            (out, result, trace_jsonl)
        }

        async fn run_case_trials(
            &self,
            case: &EvalCase,
            opts: &CaseRunOptions,
            checks: &[CheckDefinition],
            trial_count: u32,
            _max_parallelism: Option<u32>,
        ) -> CaseTrialsResult {
            let mut trials = Vec::new();
            for trial_id in 1..=trial_count {
                let (_out, result, _trace) = self.run_case(case, opts, checks).await;
                trials.push(TrialResult {
                    trial_id,
                    status: result.status,
                    command_count: result.command_count,
                    input_tokens: result.input_tokens,
                    output_tokens: result.output_tokens,
                    check_results: result.check_results,
                    error_message: result.error_message,
                    cost_usd: None,
                    exit_code: None,
                    terminal: None,
                    tokens: Default::default(),
                    skill_path: None,
                    judge_excluded: false,
                });
            }
            aggregate_trials(&case.id, trials, trial_count, opts.pass_threshold)
        }
    }

    #[tokio::test]
    async fn test_eval_runner_trait_stub_returns_expected_trace() {
        let case = EvalCase {
            id: "c1".to_string(),
            prompt: "p".to_string(),
            should_trigger: true,
            tags: vec![],
            workspace_subdir: None,
            extra: Default::default(),
        };
        let opts = CaseRunOptions {
            agent_key: "agent".to_string(),
            model: None,
            project_root: PathBuf::from("/tmp"),
            timeout_seconds: 1,
            pass_threshold: 1.0,
            isolation: IsolationMode::Inherit,
            retain_workspace_in: None,
        };
        let runner = StubEvalRunner;
        let (out, res, trace) = runner.run_case(&case, &opts, &[]).await;
        assert_eq!(out.exit_code, Some(0));
        assert_eq!(res.id, "c1");
        assert!(trace.contains("raw_line"));
    }

    #[test]
    fn test_case_run_options_builder() {
        let opts = CaseRunOptions {
            agent_key: "codex".to_string(),
            model: Some("gpt-4".to_string()),
            project_root: PathBuf::from("/tmp"),
            timeout_seconds: 300,
            pass_threshold: 1.0,
            isolation: IsolationMode::Inherit,
            retain_workspace_in: None,
        };
        assert_eq!(opts.agent_key, "codex");
        assert_eq!(opts.model, Some("gpt-4".to_string()));
    }

    #[test]
    fn test_runner_error_display() {
        let err = RunnerError::AgentUnavailable("codex".to_string());
        assert!(err.to_string().contains("codex"));
        assert!(err.to_string().contains("EVAL_AGENT_UNAVAILABLE"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_run_case_inner_status_uses_required_checks() {
        let command_dir = tempfile::tempdir().unwrap();
        write_fake_claude(command_dir.path());
        let previous_path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var(
            "PATH",
            format!("{}:{}", command_dir.path().display(), previous_path),
        );

        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("present.txt"), "ok").unwrap();
        let opts = CaseRunOptions {
            agent_key: "claude".to_string(),
            model: None,
            project_root: project.path().to_path_buf(),
            timeout_seconds: 5,
            pass_threshold: 1.0,
            isolation: IsolationMode::Inherit,
            retain_workspace_in: None,
        };
        let case = EvalCase {
            id: "required-matrix".to_string(),
            prompt: "p".to_string(),
            // Not about triggering: the fake agent reads no skill, and under
            // R7 a `true` here would add a `skill_invoked` check that fails.
            should_trigger: false,
            tags: vec![],
            workspace_subdir: None,
            extra: Default::default(),
        };
        let runner = AikitEvalRunner::new();

        let required_fail_checks = vec![CheckDefinition::FileExists {
            path: PathBuf::from("missing-required.txt"),
            required: true,
            cases: None,
        }];
        let (_out, required_fail, _trace) = runner
            .run_case_inner(&case, &opts, &required_fail_checks)
            .await;
        assert_eq!(required_fail.status, CaseStatus::Failed);

        let optional_fail_checks = vec![
            CheckDefinition::FileExists {
                path: PathBuf::from("present.txt"),
                required: true,
                cases: None,
            },
            CheckDefinition::FileExists {
                path: PathBuf::from("missing-optional.txt"),
                required: false,
                cases: None,
            },
        ];
        let (_out, optional_fail, _trace) = runner
            .run_case_inner(&case, &opts, &optional_fail_checks)
            .await;
        assert_eq!(optional_fail.status, CaseStatus::Passed);
        assert!(
            optional_fail
                .check_results
                .iter()
                .any(|r| !r.required && !r.passed),
            "optional failure must remain visible in check_results"
        );

        let all_optional_failing_checks = vec![CheckDefinition::FileExists {
            path: PathBuf::from("missing-optional-only.txt"),
            required: false,
            cases: None,
        }];
        let (_out, all_optional, _trace) = runner
            .run_case_inner(&case, &opts, &all_optional_failing_checks)
            .await;
        assert_eq!(all_optional.status, CaseStatus::Passed);

        std::env::set_var("PATH", previous_path);
    }

    #[tokio::test]
    async fn test_run_case_inner_agent_failure_is_error_despite_vacuous_checks() {
        // An unavailable agent key makes run_agent_events fail before any
        // output exists. The trace is empty, so every check below passes
        // vacuously — that must not turn an execution failure into Passed.
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("preexisting.txt"), "ok").unwrap();
        let opts = CaseRunOptions {
            agent_key: "definitely-not-a-real-agent".to_string(),
            model: None,
            project_root: project.path().to_path_buf(),
            timeout_seconds: 5,
            pass_threshold: 1.0,
            isolation: IsolationMode::Inherit,
            retain_workspace_in: None,
        };
        let case = EvalCase {
            id: "agent-failure".to_string(),
            prompt: "p".to_string(),
            should_trigger: true,
            tags: vec![],
            workspace_subdir: None,
            extra: Default::default(),
        };
        let checks = vec![
            CheckDefinition::TriggerExpectation {
                pattern: "never-happens".to_string(),
                expected: false,
                required: true,
                cases: None,
            },
            CheckDefinition::MaxToolCalls {
                limit: 10,
                required: true,
                cases: None,
            },
            CheckDefinition::FileExists {
                path: PathBuf::from("preexisting.txt"),
                required: true,
                cases: None,
            },
        ];
        let runner = AikitEvalRunner::new();

        let (_out, result, _trace) = runner.run_case_inner(&case, &opts, &checks).await;

        assert_eq!(
            result.status,
            CaseStatus::Error,
            "agent execution failure must be Error even when checks pass vacuously"
        );
        assert!(
            result.error_message.is_some(),
            "error_message must carry the captured execution error"
        );
    }

    #[tokio::test]
    async fn test_run_case_trials_all_errored_yields_case_verdict_error() {
        // Aggregation over trials must treat exec-failure Errors as it treats
        // timeout Errors. With every trial errored there is no scored trial
        // left, so the case verdict is `error` and not `failed` (R4): calling
        // it `failed` would blame the skill for an outage.
        let opts = CaseRunOptions {
            agent_key: "definitely-not-a-real-agent".to_string(),
            model: None,
            project_root: PathBuf::from("/tmp"),
            timeout_seconds: 5,
            pass_threshold: 1.0,
            isolation: IsolationMode::Inherit,
            retain_workspace_in: None,
        };
        let case = EvalCase {
            id: "agent-failure-trials".to_string(),
            prompt: "p".to_string(),
            should_trigger: true,
            tags: vec![],
            workspace_subdir: None,
            extra: Default::default(),
        };
        let checks = vec![CheckDefinition::TriggerExpectation {
            pattern: "never-happens".to_string(),
            expected: false,
            required: true,
            cases: None,
        }];
        let runner = AikitEvalRunner::new();

        let trials_result = runner
            .run_case_trials(&case, &opts, &checks, 2, Some(1))
            .await;

        assert_eq!(trials_result.pass_count, 0);
        assert_eq!(trials_result.aggregated_status, CaseStatus::Error);
        assert_eq!(trials_result.error_count, 2);
        assert_eq!(trials_result.scored_trials, 0);
        assert_eq!(
            trials_result.pass_rate, 0.0,
            "no scored trial means no rate to report, never a division by zero"
        );
        for trial in &trials_result.trials {
            assert_eq!(trial.status, CaseStatus::Error);
            assert!(trial.error_message.is_some());
        }
    }

    #[tokio::test]
    async fn test_stub_runner_returns_non_null_token_fields() {
        let case = EvalCase {
            id: "tok-case".to_string(),
            prompt: "p".to_string(),
            should_trigger: true,
            tags: vec![],
            workspace_subdir: None,
            extra: Default::default(),
        };
        let opts = CaseRunOptions {
            agent_key: "agent".to_string(),
            model: None,
            project_root: PathBuf::from("/tmp"),
            timeout_seconds: 1,
            pass_threshold: 1.0,
            isolation: IsolationMode::Inherit,
            retain_workspace_in: None,
        };
        let runner = StubEvalRunner;
        let (_out, res, _trace) = runner.run_case(&case, &opts, &[]).await;
        assert_eq!(
            res.input_tokens,
            Some(100),
            "stub must return non-null input_tokens"
        );
        assert_eq!(
            res.output_tokens,
            Some(50),
            "stub must return non-null output_tokens"
        );
    }

    #[tokio::test]
    async fn test_stub_runner_trials_propagate_token_fields() {
        let case = EvalCase {
            id: "tok-trial".to_string(),
            prompt: "p".to_string(),
            should_trigger: true,
            tags: vec![],
            workspace_subdir: None,
            extra: Default::default(),
        };
        let opts = CaseRunOptions {
            agent_key: "agent".to_string(),
            model: None,
            project_root: PathBuf::from("/tmp"),
            timeout_seconds: 1,
            pass_threshold: 1.0,
            isolation: IsolationMode::Inherit,
            retain_workspace_in: None,
        };
        let runner = StubEvalRunner;
        let trials_result = runner.run_case_trials(&case, &opts, &[], 2, None).await;
        for trial in &trials_result.trials {
            assert_eq!(trial.input_tokens, Some(100));
            assert_eq!(trial.output_tokens, Some(50));
        }
    }

    // ───────────────────────── spec 016 isolation tests ─────────────────────────

    #[cfg(unix)]
    #[tokio::test]
    async fn test_isolated_case_deploys_skill_workspace_lives_and_deletes_on_success() {
        let bin = tempfile::tempdir().unwrap();
        // The fake agent proves the workspace existed AT INVOCATION TIME by
        // writing a marker into its cwd (a dropped TempDir would make this
        // fail like a flaky agent), and emits a real-shaped init event.
        write_fake_agent(
            bin.path(),
            "claude",
            concat!(
                "touch invoked-here.txt\n",
                r#"printf '%s\n' '{"type":"system","subtype":"init","skills":["my-skill"],"version":"9.9.9"}'"#,
                "\n",
                r#"printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"ok","session_id":"s"}'"#,
            ),
        );
        prepend_path(bin.path());

        let project = tempfile::tempdir().unwrap();
        let opts = isolated_opts("claude", project.path().to_path_buf());
        let runner = AikitEvalRunner::new();
        let (output, result, _trace) = runner
            .run_case_inner(&simple_case("iso-1"), &opts, &[])
            .await;

        assert_eq!(
            result.status,
            CaseStatus::Passed,
            "err: {:?}",
            result.error_message
        );
        let ws = output
            .workspace
            .as_ref()
            .expect("isolated case must carry its workspace");
        assert_ne!(ws.root(), project.path(), "must not run in project_root");
        // Skill-under-test survival: deploy path exists inside the scratch root.
        let skill_md = ws.root().join(".claude/skills/my-skill/SKILL.md");
        assert!(
            skill_md.exists(),
            "skill under test must be materialized in the scratch root"
        );
        assert_eq!(std::fs::read_to_string(&skill_md).unwrap(), "# My Skill\n");
        // Workspace lifetime: the agent's cwd existed when it ran.
        assert!(
            ws.working_dir().join("invoked-here.txt").exists(),
            "scratch dir must exist at the moment the agent is invoked"
        );
        // D6 report.
        let report = output
            .isolation
            .as_ref()
            .expect("isolation report must be present");
        assert!(report.requested);
        assert_eq!(report.project_scope, ScopeFidelity::Isolated);
        assert_eq!(report.user_scope, ScopeFidelity::Isolated);
        assert_eq!(
            report.mechanism.as_deref(),
            Some("--setting-sources project")
        );
        assert_eq!(report.agent_version.as_deref(), Some("9.9.9"));
        assert_eq!(report.ambient_skills, vec!["my-skill".to_string()]);
        assert_eq!(report.workspace_root.as_deref(), Some(ws.root()));
        assert!(report.degrade_reason.is_none());

        // Delete on success: dropping the output removes the scratch dir.
        let root = ws.root().to_path_buf();
        drop(output);
        assert!(
            !root.exists(),
            "successful case's scratch workspace must be deleted"
        );
    }

    /// The discriminating test (spec 016 Testing Decisions): skill-a lives in
    /// the scratch workspace, skill-b only in the ambient environment. The
    /// fake agent reports skill-b exactly when the isolation mechanism
    /// (`--setting-sources project`) is absent from its argv — so skill-b
    /// showing up under Isolated would prove the flag never reached the
    /// agent. This is the only test that proves isolation *happened* rather
    /// than that a flag was *passed*.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_discriminating_ambient_skill_absent_under_isolated_present_under_inherit() {
        let bin = tempfile::tempdir().unwrap();
        write_fake_agent(
            bin.path(),
            "claude",
            concat!(
                "case \"$*\" in\n",
                "  *\"--setting-sources project\"*) skills='[\"skill-a\"]' ;;\n",
                "  *) skills='[\"skill-a\",\"skill-b\"]' ;;\n",
                "esac\n",
                "printf '{\"type\":\"system\",\"subtype\":\"init\",\"skills\":%s}\\n' \"$skills\"\n",
                r#"printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"ok","session_id":"s"}'"#,
            ),
        );
        prepend_path(bin.path());
        let project = tempfile::tempdir().unwrap();
        let runner = AikitEvalRunner::new();

        let mut opts = isolated_opts("claude", project.path().to_path_buf());
        let (isolated_out, _r, _t) = runner
            .run_case_inner(&simple_case("disc"), &opts, &[])
            .await;
        let isolated_skills = &isolated_out.isolation.as_ref().unwrap().ambient_skills;
        assert!(
            !isolated_skills.iter().any(|s| s == "skill-b"),
            "ambient skill-b must be absent under Isolated: {isolated_skills:?}"
        );
        assert!(isolated_skills.iter().any(|s| s == "skill-a"));

        opts.isolation = IsolationMode::Inherit;
        let (inherit_out, _r, _t) = runner
            .run_case_inner(&simple_case("disc"), &opts, &[])
            .await;
        let inherit_skills = &inherit_out.isolation.as_ref().unwrap().ambient_skills;
        assert!(
            inherit_skills.iter().any(|s| s == "skill-b"),
            "ambient skill-b must be present under Inherit: {inherit_skills:?}"
        );
        let inherit_report = inherit_out.isolation.as_ref().unwrap();
        assert!(!inherit_report.requested);
        assert_eq!(inherit_report.project_scope, ScopeFidelity::Inherited);
        assert_eq!(inherit_report.user_scope, ScopeFidelity::Inherited);
    }

    /// spec 016 D2's one user-visible semantic shift: `workspace_subdir`
    /// resolves inside the SCRATCH root under isolation, with the declared
    /// fixture contents copied over from `project_root/<subdir>`.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_workspace_subdir_resolves_inside_scratch_root_with_fixtures() {
        let bin = tempfile::tempdir().unwrap();
        write_fake_claude(bin.path());
        prepend_path(bin.path());

        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join("sub/nested")).unwrap();
        std::fs::write(project.path().join("sub/fixture.txt"), "f").unwrap();
        std::fs::write(project.path().join("sub/nested/deep.txt"), "d").unwrap();

        let opts = isolated_opts("claude", project.path().to_path_buf());
        let case = EvalCase {
            workspace_subdir: Some(PathBuf::from("sub")),
            extra: Default::default(),
            ..simple_case("subdir")
        };
        let checks = vec![CheckDefinition::FileExists {
            path: PathBuf::from("fixture.txt"),
            required: true,
            cases: None,
        }];
        let runner = AikitEvalRunner::new();
        let (output, result, _trace) = runner.run_case_inner(&case, &opts, &checks).await;

        assert_eq!(
            result.status,
            CaseStatus::Passed,
            "fixture must be visible to file_exists: {:?}",
            result.check_results
        );
        let ws = output.workspace.as_ref().unwrap();
        assert!(
            ws.working_dir().starts_with(ws.root()),
            "subdir must be INSIDE the scratch root"
        );
        assert!(ws.working_dir().ends_with("sub"));
        assert_ne!(
            ws.working_dir(),
            project.path().join("sub"),
            "must not run in project_root/sub"
        );
        assert!(
            ws.working_dir().join("nested/deep.txt").exists(),
            "nested fixtures must be copied"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_failed_isolated_case_workspace_is_retained_and_printed_path_survives() {
        let bin = tempfile::tempdir().unwrap();
        write_fake_claude(bin.path());
        prepend_path(bin.path());

        let project = tempfile::tempdir().unwrap();
        let retain = tempfile::tempdir().unwrap();
        let mut opts = isolated_opts("claude", project.path().to_path_buf());
        opts.retain_workspace_in = Some(retain.path().to_path_buf());
        let checks = vec![CheckDefinition::FileExists {
            path: PathBuf::from("never-created.txt"),
            required: true,
            cases: None,
        }];
        let runner = AikitEvalRunner::new();
        let (output, result, _trace) = runner
            .run_case_inner(&simple_case("fail-1"), &opts, &checks)
            .await;

        assert_eq!(result.status, CaseStatus::Failed);
        let ws = output
            .workspace
            .as_ref()
            .expect("failed workspace must survive on the output");
        assert!(ws.is_retained());
        assert!(
            ws.root().starts_with(retain.path()),
            "retained under the caller's dir: {:?}",
            ws.root()
        );
        assert!(ws.root().join(".claude/skills/my-skill/SKILL.md").exists());
        let root = ws.root().to_path_buf();
        drop(output);
        assert!(root.exists(), "retained workspace must outlive the output");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_failed_isolated_case_without_retain_dir_is_deleted() {
        let bin = tempfile::tempdir().unwrap();
        write_fake_claude(bin.path());
        prepend_path(bin.path());

        let project = tempfile::tempdir().unwrap();
        let opts = isolated_opts("claude", project.path().to_path_buf());
        let checks = vec![CheckDefinition::FileExists {
            path: PathBuf::from("never-created.txt"),
            required: true,
            cases: None,
        }];
        let runner = AikitEvalRunner::new();
        let (output, result, _trace) = runner
            .run_case_inner(&simple_case("fail-2"), &opts, &checks)
            .await;
        assert_eq!(result.status, CaseStatus::Failed);
        let root = output.workspace.as_ref().unwrap().root().to_path_buf();
        drop(output);
        assert!(
            !root.exists(),
            "retain_workspace_in: None must always delete"
        );
    }

    /// spec 016 D4: opencode has no skills path in the deploy catalog —
    /// isolation degrades to Inherit with an explicit recorded reason, and
    /// never silently claims isolation.
    #[tokio::test]
    async fn test_opencode_isolation_degrades_with_recorded_reason() {
        let project = tempfile::tempdir().unwrap();
        let opts = isolated_opts("opencode", project.path().to_path_buf());
        let runner = AikitEvalRunner::new();
        let (output, _result, _trace) = runner.run_case_inner(&simple_case("oc"), &opts, &[]).await;

        assert!(
            output.workspace.is_none(),
            "no scratch workspace can hold the skill"
        );
        let report = output
            .isolation
            .as_ref()
            .expect("degraded run must still report");
        assert!(report.requested);
        assert_eq!(
            report.project_scope,
            ScopeFidelity::Inherited,
            "must not claim isolation"
        );
        assert!(
            report
                .degrade_reason
                .as_deref()
                .unwrap_or("")
                .contains("deploy catalog"),
            "degradation reason must be recorded: {:?}",
            report.degrade_reason
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_dir_source_copies_skill_md_and_support_files() {
        let bin = tempfile::tempdir().unwrap();
        write_fake_claude(bin.path());
        prepend_path(bin.path());

        let source = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("SKILL.md"), "# From Dir\n").unwrap();
        std::fs::create_dir_all(source.path().join("scripts")).unwrap();
        std::fs::write(source.path().join("scripts/helper.py"), "print('hi')\n").unwrap();

        let project = tempfile::tempdir().unwrap();
        let mut opts = isolated_opts("claude", project.path().to_path_buf());
        opts.isolation = IsolationMode::Isolated {
            skill_name: "dir-skill".to_string(),
            source: SkillSource::Dir(source.path().to_path_buf()),
        };
        let runner = AikitEvalRunner::new();
        let (output, result, _trace) = runner
            .run_case_inner(&simple_case("dir-1"), &opts, &[])
            .await;

        assert_eq!(
            result.status,
            CaseStatus::Passed,
            "err: {:?}",
            result.error_message
        );
        let ws = output.workspace.as_ref().unwrap();
        let skill_dir = ws.root().join(".claude/skills/dir-skill");
        assert_eq!(
            std::fs::read_to_string(skill_dir.join("SKILL.md")).unwrap(),
            "# From Dir\n"
        );
        assert!(
            skill_dir.join("scripts/helper.py").exists(),
            "support files must be copied"
        );
    }

    #[tokio::test]
    async fn test_dir_source_missing_skill_md_is_case_error_not_silent_inherit() {
        let source = tempfile::tempdir().unwrap(); // no SKILL.md
        let project = tempfile::tempdir().unwrap();
        let mut opts = isolated_opts("claude", project.path().to_path_buf());
        opts.isolation = IsolationMode::Isolated {
            skill_name: "ghost".to_string(),
            source: SkillSource::Dir(source.path().to_path_buf()),
        };
        let runner = AikitEvalRunner::new();
        let (_output, result, _trace) = runner
            .run_case_inner(&simple_case("ghost"), &opts, &[])
            .await;
        assert_eq!(result.status, CaseStatus::Error);
        assert!(
            result
                .error_message
                .as_deref()
                .unwrap_or("")
                .contains("EVAL_ISOLATION_SOURCE_UNREADABLE"),
            "explicit user input must fail loudly: {:?}",
            result.error_message
        );
    }

    #[test]
    fn test_codex_scratch_home_allocated_once_per_runner() {
        let fake_home = tempfile::tempdir().unwrap();
        std::fs::write(fake_home.path().join("auth.json"), b"{}").unwrap();
        std::env::set_var("CODEX_HOME", fake_home.path());

        let runner = AikitEvalRunner::new();
        let first = runner.codex_home_path().expect("allocation must succeed");
        let second = runner.codex_home_path().expect("second call must reuse");
        assert_eq!(
            first, second,
            "scratch CODEX_HOME must be allocated ONCE per run"
        );
        assert!(first.join("auth.json").exists());
        assert_ne!(
            first,
            fake_home.path(),
            "must be a scratch copy, not the real home"
        );

        let clone = runner.clone();
        drop(runner);
        assert!(first.exists(), "still alive while a clone exists");
        drop(clone);
        assert!(
            !first.exists(),
            "credentials dir must be deleted when the run ends"
        );
        std::env::remove_var("CODEX_HOME");
    }

    // ---- build_isolation_report fidelity matrix (spec 016 D4/D6) ----

    #[test]
    fn test_report_unsupported_backends_isolate_project_scope_only() {
        // gemini/cursor: no user-scope mechanism — run anyway, project scope
        // isolated, user scope recorded as unsupported (never claimed).
        for agent in ["gemini", "cursor"] {
            let payload = SkillIsolation {
                workspace_root: PathBuf::from("/scratch"),
                skill_path: PathBuf::from("/scratch/skills/s"),
                skill_name: "s".to_string(),
                codex_home: None,
            };
            let report = build_isolation_report(
                agent,
                true,
                None,
                Some(PathBuf::from("/scratch")),
                Some(&payload),
                "",
            );
            assert_eq!(report.project_scope, ScopeFidelity::Isolated, "{agent}");
            assert_eq!(report.user_scope, ScopeFidelity::Unsupported, "{agent}");
            assert!(report.mechanism.is_none(), "{agent}");
            assert!(
                report.degrade_reason.is_some(),
                "{agent}: the unsupported user scope must be recorded"
            );
        }
    }

    #[test]
    fn test_report_codex_without_scratch_home_is_inherited_user_scope() {
        let payload = SkillIsolation {
            workspace_root: PathBuf::from("/scratch"),
            skill_path: PathBuf::from("/scratch/.codex/skills/s"),
            skill_name: "s".to_string(),
            codex_home: None, // allocation failed
        };
        let report = build_isolation_report(
            "codex",
            true,
            None,
            Some(PathBuf::from("/scratch")),
            Some(&payload),
            "",
        );
        assert_eq!(report.project_scope, ScopeFidelity::Isolated);
        assert_eq!(
            report.user_scope,
            ScopeFidelity::Inherited,
            "codex without a scratch CODEX_HOME must not claim user-scope isolation"
        );
        assert!(report
            .degrade_reason
            .as_deref()
            .unwrap_or("")
            .contains("CODEX_HOME"));
    }

    #[test]
    fn test_report_codex_with_scratch_home_is_isolated() {
        let payload = SkillIsolation {
            workspace_root: PathBuf::from("/scratch"),
            skill_path: PathBuf::from("/scratch/.codex/skills/s"),
            skill_name: "s".to_string(),
            codex_home: Some(PathBuf::from("/scratch-home")),
        };
        let report = build_isolation_report(
            "codex",
            true,
            None,
            Some(PathBuf::from("/scratch")),
            Some(&payload),
            "",
        );
        assert_eq!(report.user_scope, ScopeFidelity::Isolated);
        assert_eq!(report.mechanism.as_deref(), Some("scratch CODEX_HOME"));
    }

    // ---- parse_claude_ambient_skills (report-only, spec 016 D6) ----

    #[test]
    fn test_parse_claude_ambient_skills_string_and_object_forms() {
        let strings = r#"{"type":"system","subtype":"init","skills":["a","b"]}"#;
        assert_eq!(parse_claude_ambient_skills(strings), vec!["a", "b"]);

        let objects = r#"{"type":"system","subtype":"init","skills":[{"name":"x"},{"name":"y"}]}"#;
        assert_eq!(parse_claude_ambient_skills(objects), vec!["x", "y"]);

        let mixed_stream = "not json\n{\"type\":\"assistant\"}\n{\"type\":\"system\",\"subtype\":\"init\",\"skills\":[\"only\"]}\n";
        assert_eq!(parse_claude_ambient_skills(mixed_stream), vec!["only"]);
    }

    #[test]
    fn test_parse_claude_ambient_skills_empty_when_unobservable() {
        assert!(parse_claude_ambient_skills("").is_empty());
        assert!(
            parse_claude_ambient_skills("{\"type\":\"system\",\"subtype\":\"init\"}").is_empty()
        );
        assert!(parse_claude_ambient_skills("{\"type\":\"result\"}").is_empty());
    }

    // ── R1 / R3: a trial that produced no measurement is `error` ───────────

    /// Run one case against a throwaway agent script and return its result.
    #[cfg(unix)]
    async fn run_one_case_with(agent: &str, script: &str) -> CaseResult {
        let bin = tempfile::tempdir().unwrap();
        write_fake_agent(bin.path(), agent, script);
        prepend_path(bin.path());
        let project = tempfile::tempdir().unwrap();
        let opts = CaseRunOptions {
            agent_key: agent.to_string(),
            model: None,
            project_root: project.path().to_path_buf(),
            timeout_seconds: 5,
            pass_threshold: 1.0,
            isolation: IsolationMode::Inherit,
            retain_workspace_in: None,
        };
        let runner = AikitEvalRunner::new();
        let (_out, result, _trace) = runner.run_case_inner(&simple_case("m1"), &opts, &[]).await;
        result
    }

    /// R8: an offline re-score has only the artifacts, so the staged skill
    /// directory has to be one of them.
    ///
    /// Without it, `skill_invoked` on a backend with no typed `Skill` tool has
    /// nothing to match on, and the scorer silently disagrees with the run that
    /// wrote the trace. Per trial, not per run: each isolated trial stages into
    /// its own scratch directory.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_an_isolated_run_records_the_staged_skill_path_on_the_trial() {
        let bin = tempfile::tempdir().unwrap();
        write_fake_claude(bin.path());
        prepend_path(bin.path());
        let project = tempfile::tempdir().unwrap();
        let opts = isolated_opts("claude", project.path().to_path_buf());

        let runner = AikitEvalRunner::new();
        let (_out, result, _trace) = runner.run_case_inner(&simple_case("m1"), &opts, &[]).await;

        let staged = result
            .skill_path
            .expect("an isolated run stages the skill somewhere");
        assert_eq!(
            staged.file_name().and_then(|n| n.to_str()),
            Some("my-skill"),
            "the recorded path is the staged skill directory: {staged:?}"
        );
    }

    /// Every trial of a multi-trial case carries its own staged path.
    ///
    /// The per-case fold is where the path could quietly be dropped: the
    /// default-trial runner maps a `CaseResult` onto a `TrialResult` field by
    /// field, and a field left off that map is invisible until a scorer
    /// disagrees with the run months later.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_every_trial_of_a_case_carries_its_own_staged_path() {
        let bin = tempfile::tempdir().unwrap();
        write_fake_claude(bin.path());
        prepend_path(bin.path());
        let project = tempfile::tempdir().unwrap();
        let opts = isolated_opts("claude", project.path().to_path_buf());

        let runner = AikitEvalRunner::new();
        let trials_result = runner
            .run_case_trials(&simple_case("m1"), &opts, &[], 2, Some(1))
            .await;

        assert_eq!(trials_result.trials.len(), 2);
        let staged: Vec<_> = trials_result
            .trials
            .iter()
            .map(|t| {
                t.skill_path
                    .clone()
                    .unwrap_or_else(|| panic!("trial {} lost its staged path", t.trial_id))
            })
            .collect();
        for path in &staged {
            assert_eq!(path.file_name().and_then(|n| n.to_str()), Some("my-skill"));
        }
        assert_ne!(
            staged[0], staged[1],
            "each trial stages into its own scratch dir, which is why the path \
             is recorded per trial and not once per run"
        );
    }

    /// The negative half: nothing was staged, so `None` is the honest answer.
    /// A placeholder here would be worse than nothing — the scorer would match
    /// traces against a path that never existed.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_an_inheriting_run_records_no_staged_skill_path() {
        let inherited = run_one_case_with(
            "claude",
            r#"printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"ok","session_id":"s"}'"#,
        )
        .await;

        assert!(
            inherited.skill_path.is_none(),
            "IsolationMode::Inherit stages nothing: {:?}",
            inherited.skill_path
        );
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_agent_reported_failure_is_error_not_failed() {
        // The defect in one test: the agent exits zero and says it failed.
        // Before R1 this scored `failed` and was averaged in as a wrong
        // answer; the four dead trials in the 210-trial sweep were exactly
        // this shape.
        let errored = run_one_case_with(
                "claude",
                r#"printf '%s\n' '{"type":"result","subtype":"error_during_execution","is_error":true,"result":"Request timed out.","session_id":"s"}'"#,
            )
        .await;

        assert_eq!(errored.status, CaseStatus::Error, "{errored:?}");
        assert_eq!(errored.exit_code, Some(0), "the process itself exited fine");
        let message = errored.error_message.unwrap();
        assert!(message.starts_with("EVAL_TRIAL_ERROR:"), "{message}");
        let terminal = errored.terminal.expect("the agent's verdict is recorded");
        assert_eq!(terminal.outcome, TerminalOutcome::Error);
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_the_same_trial_without_the_error_flag_is_not_error() {
        // The negative half: identical shape, `is_error` false. If this also
        // scored `error`, the rule above would be measuring nothing.
        let clean = run_one_case_with(
                "claude",
                r#"printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"ok","session_id":"s"}'"#,
            )
        .await;

        assert_ne!(clean.status, CaseStatus::Error, "{clean:?}");
        assert_eq!(
            clean.terminal.map(|t| t.outcome),
            Some(TerminalOutcome::Success)
        );
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_clean_exit_with_no_answer_is_failed_not_error() {
        // A skill failure, not an outage: the agent completed and said
        // nothing useful. Text absence must never be the discriminator.
        let empty = run_one_case_with(
                "claude",
                r#"printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"","session_id":"s"}'"#,
            )
        .await;

        assert_ne!(empty.status, CaseStatus::Error, "{empty:?}");
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_nonzero_exit_is_error() {
        // Deliberately `gemini`, which declares no terminal event: on `claude`
        // the missing-terminal rule would reach `error` on its own and this
        // test would pass with the exit-code rule deleted.
        let failed = run_one_case_with("gemini", "echo boom >&2\nexit 3").await;

        assert_eq!(failed.status, CaseStatus::Error, "{failed:?}");
        assert_eq!(failed.exit_code, Some(3));
        assert!(
            failed.error_message.unwrap().contains("exited 3"),
            "the reason names the exit code"
        );
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_the_same_backend_exiting_zero_is_not_error() {
        // The negative half of the test above: same backend, same silence on
        // stdout, exit 0. Without this the exit-code rule could be "always
        // error on gemini" and nothing would notice.
        let clean = run_one_case_with("gemini", "echo boom >&2\nexit 0").await;

        assert_ne!(clean.status, CaseStatus::Error, "{clean:?}");
        assert_eq!(clean.exit_code, Some(0));
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_missing_terminal_event_is_error_only_where_one_is_declared() {
        // claude declares `terminal_event`, so a stream that ends without one
        // did not complete.
        let claude = run_one_case_with("claude", r#"printf '%s\n' 'just some text'"#).await;
        assert_eq!(claude.status, CaseStatus::Error, "{claude:?}");
        assert!(claude
            .error_message
            .unwrap()
            .contains("stream ended with no terminal event"));

        // gemini is wrapped as text-only and declares nothing, so the same
        // stream says nothing about the outcome. Marking it `error` would be a
        // statement about the decoder, not the run.
        let gemini = run_one_case_with("gemini", r#"printf '%s\n' 'just some text'"#).await;
        assert_ne!(gemini.status, CaseStatus::Error, "{gemini:?}");
        assert!(gemini.terminal.is_none());
    }

    #[test]
    fn test_only_backends_that_decode_a_terminal_frame_declare_the_flag() {
        // ADR 0019: the flag describes the decoder, never the roadmap.
        for backend in aikit_sdk::runner::backend::ALL {
            let declared = backend.capabilities().terminal_event;
            let expected = matches!(backend, Backend::Claude | Backend::Codex | Backend::Pi);
            assert_eq!(
                declared, expected,
                "{:?} declares terminal_event={declared}",
                backend
            );
        }
    }

    // ───────────────── spec eval-judge R10: workspace.diff ─────────────────

    /// A passing trial's scratch workspace is deleted, so the diff is the only
    /// record of what the agent wrote. It must be taken before the workspace
    /// goes, and it must cover both new files and edits to seeded ones.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_isolated_trial_diffs_what_the_agent_wrote_before_the_workspace_goes() {
        let bin = tempfile::tempdir().unwrap();
        write_fake_agent(
            bin.path(),
            "claude",
            concat!(
                "printf 'hello from the agent\\n' > note.txt\n",
                "printf 'edited\\n' >> .claude/skills/my-skill/SKILL.md\n",
                r#"printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"ok","session_id":"s"}'"#,
            ),
        );
        prepend_path(bin.path());

        let project = tempfile::tempdir().unwrap();
        let opts = isolated_opts("claude", project.path().to_path_buf());
        let (output, result, _trace) = AikitEvalRunner::new()
            .run_case_inner(&simple_case("diff-1"), &opts, &[])
            .await;
        assert_eq!(
            result.status,
            CaseStatus::Passed,
            "err: {:?}",
            result.error_message
        );

        let diff = output
            .workspace_diff
            .as_deref()
            .expect("an isolated trial must carry its workspace diff");
        assert!(
            diff.contains("--- /dev/null\n+++ b/note.txt\n"),
            "a new file must be listed:\n{diff}"
        );
        assert!(diff.contains("\n+hello from the agent\n"), "{diff}");
        assert!(
            diff.contains(
                "--- a/.claude/skills/my-skill/SKILL.md\n+++ b/.claude/skills/my-skill/SKILL.md\n"
            ),
            "an edited seeded file must be diffed against its seeded content:\n{diff}"
        );
        assert!(diff.contains("\n+edited\n"), "{diff}");
    }

    /// An untouched workspace is an empty diff, not a missing one: the file
    /// exists and says "nothing changed", which is a measurement.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_isolated_trial_that_touches_nothing_has_an_empty_diff() {
        let bin = tempfile::tempdir().unwrap();
        write_fake_claude(bin.path());
        prepend_path(bin.path());

        let project = tempfile::tempdir().unwrap();
        let opts = isolated_opts("claude", project.path().to_path_buf());
        let (output, result, _trace) = AikitEvalRunner::new()
            .run_case_inner(&simple_case("diff-2"), &opts, &[])
            .await;
        assert_eq!(
            result.status,
            CaseStatus::Passed,
            "err: {:?}",
            result.error_message
        );
        assert_eq!(
            output.workspace_diff.as_deref(),
            Some(""),
            "an untouched workspace must diff to exactly nothing"
        );
    }

    /// With no seeded state there is nothing to diff against; the runner must
    /// say so with `None` rather than hand over an empty diff that would read
    /// as "nothing changed".
    #[cfg(unix)]
    #[tokio::test]
    async fn test_inheriting_run_has_no_workspace_diff() {
        let bin = tempfile::tempdir().unwrap();
        write_fake_claude(bin.path());
        prepend_path(bin.path());

        let project = tempfile::tempdir().unwrap();
        let mut opts = isolated_opts("claude", project.path().to_path_buf());
        opts.isolation = IsolationMode::Inherit;
        let (output, _result, _trace) = AikitEvalRunner::new()
            .run_case_inner(&simple_case("diff-3"), &opts, &[])
            .await;
        assert!(
            output.workspace_diff.is_none(),
            "no seeded state means no diff, never an empty one"
        );
    }
}
