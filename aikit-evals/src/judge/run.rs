//! Judge a run dir end to end (spec eval-judge R8–R13).
//!
//! One `judge_run_dir` call: load and validate the judges, pre-flight every
//! identity before any request, judge each (case, trial, judge) triple that
//! is in scope and not already judged under the same `cache_key`, append
//! judgments, then flatten into `result.json`, reduce into `aggregated.json`
//! and roll up into `summary.json`. Every rewrite is additive.

use super::config::{resolve_judges, CriterionKind, ResolvedJudge, ValidationIssue};
use super::record::{
    self, append_judgment, cache_key, judge_hash, latest_for, read_judgments, AttemptRecord,
    Judgment, JudgmentIdentity, JudgmentUsage, RecordError, JUDGMENT_SCHEMA,
};
use super::schema::{output_contract, rubric_text, score_reply};
use super::template::{self, render_retry};
use super::view::{TrialView, ViewError};
use crate::artifacts::{
    aggregate_trials, read_summary, write_case_trials_summary, write_summary, ArtifactsError,
    CaseStatus, CaseTrialsResult, JudgeCaseScores, JudgeTokenTotals, TrialResult,
};
use crate::checks::{load_checks_file, suite_passes, CheckResult, ChecksError};
use crate::suite::{EvalCase, EvalSuite};
use aikit_sdk::llm::openai_compat::OpenAiCompatProvider;
use aikit_sdk::llm::{resolve_api_key, LlmGateway, LlmMessage, LlmRequest};
use aikit_sdk::{ConversationError, ConversationPipeline, Corrective};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Semaphore};
use tokio::task::JoinSet;

/// How the run's `suite_pass` is recomputed after judge rows change verdicts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SuitePassRule {
    /// Every scored case must pass.
    AllCases,
    /// The pass rate over cases must reach this fraction.
    RateAtLeast(f64),
}

#[derive(Debug, Clone)]
pub struct JudgeRunOptions {
    /// A checks file to use instead of the one `summary.json` names.
    pub checks_override: Option<PathBuf>,
    /// `--judge-model`: overrides every judge's model and is what gets recorded.
    pub judge_model: Option<String>,
    /// Concurrent judge calls; defaults to the run's `parallel`, else 1.
    pub parallel: Option<u32>,
    /// Judge again even when the same `cache_key` already has a judgment.
    pub rejudge: bool,
    /// Transport retries (429, 5xx, timeout) per attempt chain.
    pub transport_retries: u32,
    pub backoff_base_ms: u64,
    pub suite_rule: SuitePassRule,
}

impl Default for JudgeRunOptions {
    fn default() -> Self {
        Self {
            checks_override: None,
            judge_model: None,
            parallel: None,
            rejudge: false,
            transport_retries: 3,
            backoff_base_ms: 1000,
            suite_rule: SuitePassRule::AllCases,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JudgeOutcome {
    /// A new judgment with scores.
    Judged { overall: f64 },
    /// Same `cache_key` as the latest judgment: nothing asked.
    Cached,
    /// The trial errored, so there is nothing to judge (R8).
    SkippedError,
    /// A judgment was appended without scores; `message` says why.
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrialJudgeOutcome {
    pub case_id: String,
    pub trial_id: u32,
    pub judge: String,
    pub outcome: JudgeOutcome,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JudgeRunReport {
    /// Judge names in file order.
    pub judges: Vec<String>,
    pub judged: u32,
    /// Trial-judge pairs whose latest judgment carries no scores.
    pub errors: u32,
    pub skipped_cached: u32,
    pub skipped_error_trials: u32,
    /// The run's `suite_pass` after rewriting.
    pub suite_pass: bool,
    pub per_trial: Vec<TrialJudgeOutcome>,
    pub tokens: JudgeTokenTotals,
}

impl JudgeRunReport {
    /// `eval judge` exits non-zero on any judge error or a failed rewritten
    /// verdict (R13).
    pub fn is_clean(&self) -> bool {
        self.errors == 0 && self.suite_pass
    }
}

#[derive(Debug, thiserror::Error)]
pub enum JudgeError {
    #[error("EVAL_JUDGE_RUN_DIR: {0}")]
    RunDir(String),
    #[error(transparent)]
    Artifacts(#[from] ArtifactsError),
    #[error(transparent)]
    Checks(#[from] ChecksError),
    #[error("EVAL_JUDGE_INVALID:\n{}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))]
    Invalid(Vec<ValidationIssue>),
    #[error("EVAL_JUDGE_PREFLIGHT: {0}")]
    Preflight(String),
    #[error(transparent)]
    View(#[from] ViewError),
    #[error(transparent)]
    Record(#[from] RecordError),
    #[error("EVAL_JUDGE_IO: {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("EVAL_JUDGE_TASK: {0}")]
    Task(String),
}

fn io(path: &Path, source: std::io::Error) -> JudgeError {
    JudgeError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// A judge with what pre-flight resolved for it: the gateway and the key.
struct Armed {
    judge: Arc<ResolvedJudge>,
    hash: String,
    base_url: String,
    api_key: String,
    gateway: Arc<dyn LlmGateway>,
}

/// Trial directories of a case, ascending by trial id.
fn trial_dirs(case_dir: &Path) -> Result<Vec<(u32, PathBuf)>, JudgeError> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(case_dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(io(case_dir, e)),
    };
    for entry in entries {
        let entry = entry.map_err(|e| io(case_dir, e))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if let Some(n) = name.strip_prefix("trial-") {
            if let Ok(id) = n.parse::<u32>() {
                if path.join("result.json").is_file() {
                    out.push((id, path));
                }
            }
        }
    }
    out.sort_by_key(|(id, _)| *id);
    Ok(out)
}

fn read_trial_result(trial_dir: &Path) -> Result<TrialResult, JudgeError> {
    let path = trial_dir.join("result.json");
    let text = std::fs::read_to_string(&path).map_err(|e| io(&path, e))?;
    serde_json::from_str(&text).map_err(|e| JudgeError::RunDir(format!("{}: {e}", path.display())))
}

fn write_trial_result(trial_dir: &Path, result: &TrialResult) -> Result<(), JudgeError> {
    let path = trial_dir.join("result.json");
    let text = serde_json::to_string_pretty(result)
        .map_err(|e| JudgeError::RunDir(format!("{}: {e}", path.display())))?;
    std::fs::write(&path, text).map_err(|e| io(&path, e))
}

fn read_aggregated(case_dir: &Path) -> Result<Option<CaseTrialsResult>, JudgeError> {
    let path = case_dir.join("aggregated.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| JudgeError::RunDir(format!("{}: {e}", path.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(&path, e)),
    }
}

fn message(role: &str, text: String) -> LlmMessage {
    LlmMessage {
        role: role.to_string(),
        content: Some(text),
        tool_calls: None,
        tool_call_id: None,
    }
}

/// Render the messages for one (judge, case, trial): the system message only
/// when declared, then the one user message (R2).
fn render_messages(
    judge: &ResolvedJudge,
    case: &EvalCase,
    view: &TrialView,
) -> Result<(Vec<LlmMessage>, Vec<String>), String> {
    let rubric = rubric_text(&judge.criteria);
    let contract = output_contract(&judge.criteria);
    let mut truncated: Vec<String> = Vec::new();
    let mut lookup = |name: &str| -> Result<String, String> {
        match name {
            "rubric" => Ok(rubric.clone()),
            "output_contract" => Ok(contract.clone()),
            other => view.variable(other, case).map_err(|e| e.to_string()),
        }
    };
    let mut messages = Vec::with_capacity(2);
    if let Some(system) = &judge.system_prompt {
        let rendered = template::render(system, judge.max_var_bytes, &mut lookup)?;
        truncated.extend(rendered.truncated);
        messages.push(message("system", rendered.text));
    }
    let rendered = template::render(&judge.prompt, judge.max_var_bytes, &mut lookup)?;
    for t in rendered.truncated {
        if !truncated.contains(&t) {
            truncated.push(t);
        }
    }
    messages.push(message("user", rendered.text));
    Ok((messages, truncated))
}

fn identity_of(judge: &ResolvedJudge, base_url: &str) -> JudgmentIdentity {
    JudgmentIdentity {
        model: judge.identity.model.clone(),
        model_reported: None,
        endpoint_host: record::endpoint_host(base_url),
        temperature: judge.identity.temperature,
        top_p: judge.identity.top_p,
        max_tokens: judge.identity.max_tokens,
    }
}

fn error_judgment(
    judge: &ResolvedJudge,
    hash: &str,
    base_url: &str,
    key: String,
    error: String,
    truncated: Vec<String>,
) -> Judgment {
    Judgment {
        schema: JUDGMENT_SCHEMA.to_string(),
        judge: judge.name.clone(),
        judge_hash: hash.to_string(),
        cache_key: key,
        identity: identity_of(judge, base_url),
        attempts: vec![],
        scores: None,
        error: Some(error),
        usage: JudgmentUsage::default(),
        cost_usd: None,
        truncated,
        judged_at: record::now_rfc3339(),
    }
}

/// Make the call and turn the pipeline's outcome into a judgment. Blocking:
/// the gateway blocks on its own runtime, so this runs under `spawn_blocking`.
fn judge_once(
    armed: &Armed,
    messages: Vec<LlmMessage>,
    key: String,
    truncated: Vec<String>,
    transport_retries: u32,
    backoff_base: Duration,
) -> Judgment {
    let judge = &armed.judge;
    let schema = super::schema::reply_schema(&judge.criteria).to_string();
    let pipeline = ConversationPipeline::new(schema)
        .max_retries(judge.max_retries)
        .transport_retries(transport_retries)
        .backoff_base(backoff_base);
    let request = LlmRequest {
        model: judge.identity.model.clone(),
        base_url: armed.base_url.clone(),
        api_key: armed.api_key.clone(),
        messages,
        tools: Vec::new(),
        tool_choice: None,
        temperature: Some(judge.identity.temperature),
        top_p: judge.identity.top_p,
        max_tokens: Some(judge.identity.max_tokens),
        stream: false,
    };
    let retry = judge.retry_prompt.clone();
    let corrective = retry.map(|tpl| move |errors: &[String]| render_retry(&tpl, errors));
    let corrective_ref: Option<Corrective> = corrective
        .as_ref()
        .map(|f| f as &dyn Fn(&[String]) -> String);

    let outcome = pipeline.run(&request, armed.gateway.as_ref(), corrective_ref);
    let (attempts, data, error) = match outcome {
        Ok(result) => (result.attempts, Some(result.data), None),
        Err(ConversationError::Schema(msg)) => (vec![], None, Some(format!("reply schema: {msg}"))),
        Err(ConversationError::Transport { attempts, source }) => {
            (attempts, None, Some(format!("transport: {source}")))
        }
        Err(ConversationError::ValidationExhausted { attempts, errors }) => {
            let n = attempts.len();
            (
                attempts,
                None,
                Some(format!(
                    "reply rejected after {n} attempt(s): {}",
                    errors.join("; ")
                )),
            )
        }
    };

    let mut usage = JudgmentUsage::default();
    let mut model_reported = None;
    let mut records = Vec::with_capacity(attempts.len());
    for a in attempts {
        if let Some(u) = &a.usage {
            usage.add(u);
        }
        if a.model_reported.is_some() {
            model_reported = a.model_reported.clone();
        }
        records.push(AttemptRecord {
            kind: a.kind,
            request: record::request_record(judge, &a.messages),
            response_text: a.response_text,
            finish_reason: a.finish_reason,
            usage: a.usage,
            error: a.error,
        });
    }

    let (scores, error) = match (data, error) {
        (Some(data), None) => match score_reply(&judge.criteria, &data) {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(format!("scoring: {e}"))),
        },
        (_, err) => (None, err),
    };

    let mut identity = identity_of(judge, &armed.base_url);
    identity.model_reported = model_reported;
    Judgment {
        schema: JUDGMENT_SCHEMA.to_string(),
        judge: judge.name.clone(),
        judge_hash: armed.hash.clone(),
        cache_key: key,
        identity,
        attempts: records,
        scores,
        error,
        usage,
        cost_usd: None,
        truncated,
        judged_at: record::now_rfc3339(),
    }
}

/// The `judge:<name>` row for a trial from its latest judgment (R9).
/// Returns the row and whether a gated judge left the trial unjudged.
fn flatten_row(judge: &ResolvedJudge, latest: Option<&Judgment>) -> (CheckResult, bool) {
    let name = format!("judge:{}", judge.name);
    match latest.and_then(Judgment::overall) {
        Some(overall) => {
            let (passed, message) = match judge.min_score {
                Some(min) => (
                    overall >= min,
                    format!(
                        "judge '{}' scored {:.3} (min_score {:.3})",
                        judge.name, overall, min
                    ),
                ),
                None => (
                    true,
                    format!("judge '{}' scored {:.3} (advisory)", judge.name, overall),
                ),
            };
            (
                CheckResult {
                    check_name: name,
                    passed,
                    required: judge.is_gated(),
                    message: Some(message),
                    not_observable: None,
                    score: Some(overall),
                },
                false,
            )
        }
        None => {
            let why = latest
                .and_then(|j| j.error.clone())
                .unwrap_or_else(|| "no judgment".to_string());
            (
                CheckResult {
                    check_name: name,
                    passed: !judge.is_gated(),
                    required: judge.is_gated(),
                    message: Some(format!(
                        "judge '{}' rendered no judgment: {}",
                        judge.name, why
                    )),
                    not_observable: None,
                    score: None,
                },
                judge.is_gated(),
            )
        }
    }
}

/// A gated judge row that carries no score is an exclusion, not a failure:
/// it leaves the trial's verdict alone and takes the trial out of the rate.
fn is_excluding_row(row: &CheckResult) -> bool {
    row.check_name.starts_with("judge:") && row.required && row.score.is_none()
}

/// Rewrite one trial's `result.json` from its judgments (R9).
fn flatten_trial(
    trial_dir: &Path,
    judges: &[Arc<ResolvedJudge>],
    case_id: &str,
) -> Result<TrialResult, JudgeError> {
    let mut result = read_trial_result(trial_dir)?;
    if !matches!(result.status, CaseStatus::Passed | CaseStatus::Failed) {
        return Ok(result);
    }
    let judgments = read_judgments(trial_dir)?;
    let mut excluded = false;
    for judge in judges.iter().filter(|j| j.applies_to(case_id)) {
        let (row, excludes) = flatten_row(judge, latest_for(&judgments, &judge.name));
        excluded |= excludes;
        match result
            .check_results
            .iter_mut()
            .find(|r| r.check_name == row.check_name)
        {
            Some(existing) => *existing = row,
            None => result.check_results.push(row),
        }
    }
    let rows: Vec<CheckResult> = result
        .check_results
        .iter()
        .filter(|r| !is_excluding_row(r))
        .cloned()
        .collect();
    result.status = if suite_passes(&rows) {
        CaseStatus::Passed
    } else {
        CaseStatus::Failed
    };
    result.judge_excluded = excluded;
    write_trial_result(trial_dir, &result)?;
    Ok(result)
}

/// Reduce a judge's latest judgments over a case's trials (R12).
fn reduce_scores(judge: &ResolvedJudge, latest: &[&Judgment]) -> Option<JudgeCaseScores> {
    let scored: Vec<&BTreeMap<String, f64>> =
        latest.iter().filter_map(|j| j.scores.as_ref()).collect();
    if scored.is_empty() {
        return None;
    }
    let n = scored.len() as f64;
    let overall = scored.iter().filter_map(|s| s.get("overall")).sum::<f64>() / n;
    let mut criteria = BTreeMap::new();
    for c in &judge.criteria {
        let values: Vec<f64> = scored
            .iter()
            .filter_map(|s| s.get(&c.name))
            .copied()
            .collect();
        if values.is_empty() {
            continue;
        }
        let reduced = match c.kind {
            CriterionKind::Scale => values.iter().sum::<f64>() / values.len() as f64,
            CriterionKind::Bool => {
                let yes = values.iter().filter(|v| **v >= 1.0).count();
                if yes * 2 > values.len() {
                    1.0
                } else {
                    0.0
                }
            }
        };
        criteria.insert(c.name.clone(), reduced);
    }
    Some(JudgeCaseScores {
        overall,
        judged_trials: scored.len() as u32,
        criteria,
    })
}

/// Judge every in-scope trial of a run dir and rewrite its artifacts.
///
/// `suite` is the prompts the run was made from: it supplies `{{case.*}}`
/// and lets validation refuse a `cases` list naming nothing.
pub async fn judge_run_dir(
    run_dir: &Path,
    suite: &EvalSuite,
    opts: &JudgeRunOptions,
) -> Result<JudgeRunReport, JudgeError> {
    let mut summary = read_summary(run_dir)?;

    let checks_path = match &opts.checks_override {
        Some(p) => p.clone(),
        None => summary.checks_path.clone().ok_or_else(|| {
            JudgeError::RunDir(
                "summary.json records no checks_path; pass --checks <file>".to_string(),
            )
        })?,
    };
    let checks_dir = checks_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let file = load_checks_file(&checks_path)?;

    let judges = resolve_judges(&file, &checks_dir, Some(suite), opts.judge_model.as_deref())
        .map_err(JudgeError::Invalid)?;
    let mut report = JudgeRunReport {
        judges: judges.iter().map(|j| j.name.clone()).collect(),
        suite_pass: summary.suite_pass,
        ..Default::default()
    };
    if judges.is_empty() {
        return Ok(report);
    }

    // R3: every identity resolves before any request goes out.
    let mut armed: Vec<Arc<Armed>> = Vec::with_capacity(judges.len());
    for judge in judges {
        let base_url = judge.identity.base_url.clone().ok_or_else(|| {
            JudgeError::Preflight(format!(
                "judge '{}' has no endpoint: set `base_url` on [[judge]] or [judge_defaults], or export {}",
                judge.name,
                super::config::ENDPOINT_ENV
            ))
        })?;
        let api_key = resolve_api_key(judge.identity.api_key_env.as_deref())
            .map_err(|e| JudgeError::Preflight(format!("judge '{}': {}", judge.name, e)))?;
        let provider = OpenAiCompatProvider::new(judge.timeout_secs, 30)
            .map_err(|e| JudgeError::Preflight(format!("judge '{}': {}", judge.name, e)))?;
        let hash = judge_hash(&judge);
        armed.push(Arc::new(Armed {
            judge: Arc::new(judge),
            hash,
            base_url,
            api_key,
            gateway: Arc::new(provider),
        }));
    }
    let judge_list: Vec<Arc<ResolvedJudge>> = armed.iter().map(|a| a.judge.clone()).collect();

    // Work items: (case, trial, judge) for judgeable trials in scope.
    struct Work {
        case_id: String,
        trial_id: u32,
        trial_dir: PathBuf,
        armed: Arc<Armed>,
        messages: Vec<LlmMessage>,
        key: String,
        truncated: Vec<String>,
    }
    let mut work: Vec<Work> = Vec::new();
    let mut per_trial: Vec<TrialJudgeOutcome> = Vec::new();

    for case_summary in &summary.cases {
        let case_id = case_summary.id.clone();
        let in_scope: Vec<&Arc<Armed>> = armed
            .iter()
            .filter(|a| a.judge.applies_to(&case_id))
            .collect();
        if in_scope.is_empty() {
            continue;
        }
        let case = suite.cases.iter().find(|c| c.id == case_id).ok_or_else(|| {
            JudgeError::RunDir(format!(
                "case '{}' is in summary.json but not in prompts.csv; judge against the prompts the run was made from",
                case_id
            ))
        })?;
        for (trial_id, trial_dir) in trial_dirs(&run_dir.join(&case_id))? {
            let view = TrialView::load(&trial_dir, &summary.skill_project_root)?;
            if !view.is_judgeable() {
                for a in &in_scope {
                    per_trial.push(TrialJudgeOutcome {
                        case_id: case_id.clone(),
                        trial_id,
                        judge: a.judge.name.clone(),
                        outcome: JudgeOutcome::SkippedError,
                    });
                }
                report.skipped_error_trials += 1;
                continue;
            }
            let existing = read_judgments(&trial_dir)?;
            for a in &in_scope {
                match render_messages(&a.judge, case, &view) {
                    Ok((messages, truncated)) => {
                        let key = cache_key(&a.hash, &messages);
                        let cached = latest_for(&existing, &a.judge.name)
                            .map(|j| j.cache_key == key && j.is_scored())
                            .unwrap_or(false);
                        if cached && !opts.rejudge {
                            report.skipped_cached += 1;
                            per_trial.push(TrialJudgeOutcome {
                                case_id: case_id.clone(),
                                trial_id,
                                judge: a.judge.name.clone(),
                                outcome: JudgeOutcome::Cached,
                            });
                            continue;
                        }
                        work.push(Work {
                            case_id: case_id.clone(),
                            trial_id,
                            trial_dir: trial_dir.clone(),
                            armed: (*a).clone(),
                            messages,
                            key,
                            truncated,
                        });
                    }
                    Err(reason) => {
                        // R2: a variable the run dir cannot supply is a judge
                        // error, recorded, never a blank.
                        let judgment = error_judgment(
                            &a.judge,
                            &a.hash,
                            &a.base_url,
                            cache_key(&a.hash, &[]),
                            reason.clone(),
                            vec![],
                        );
                        append_judgment(&trial_dir, &judgment)?;
                        per_trial.push(TrialJudgeOutcome {
                            case_id: case_id.clone(),
                            trial_id,
                            judge: a.judge.name.clone(),
                            outcome: JudgeOutcome::Error { message: reason },
                        });
                    }
                }
            }
        }
    }

    // Judge concurrently; append under one lock so two judges on the same
    // trial never interleave a write.
    let parallel = opts.parallel.or(summary.parallel).unwrap_or(1).max(1) as usize;
    let sem = Arc::new(Semaphore::new(parallel));
    let write_lock = Arc::new(Mutex::new(()));
    let transport_retries = opts.transport_retries;
    let backoff = Duration::from_millis(opts.backoff_base_ms);
    let mut set: JoinSet<Result<TrialJudgeOutcome, JudgeError>> = JoinSet::new();
    for item in work {
        let sem = sem.clone();
        let write_lock = write_lock.clone();
        set.spawn(async move {
            let _permit = sem
                .acquire_owned()
                .await
                .map_err(|e| JudgeError::Task(e.to_string()))?;
            let armed = item.armed.clone();
            let judgment = tokio::task::spawn_blocking(move || {
                judge_once(
                    &armed,
                    item.messages,
                    item.key,
                    item.truncated,
                    transport_retries,
                    backoff,
                )
            })
            .await
            .map_err(|e| JudgeError::Task(e.to_string()))?;
            let outcome = match (judgment.overall(), &judgment.error) {
                (Some(overall), _) => JudgeOutcome::Judged { overall },
                (None, err) => JudgeOutcome::Error {
                    message: err.clone().unwrap_or_else(|| "no scores".to_string()),
                },
            };
            {
                let _guard = write_lock.lock().await;
                append_judgment(&item.trial_dir, &judgment)?;
            }
            Ok(TrialJudgeOutcome {
                case_id: item.case_id,
                trial_id: item.trial_id,
                judge: item.armed.judge.name.clone(),
                outcome,
            })
        });
    }
    while let Some(joined) = set.join_next().await {
        let outcome = joined.map_err(|e| JudgeError::Task(e.to_string()))??;
        if matches!(outcome.outcome, JudgeOutcome::Judged { .. }) {
            report.judged += 1;
        }
        per_trial.push(outcome);
    }
    per_trial.sort_by(|a, b| {
        (&a.case_id, a.trial_id, &a.judge).cmp(&(&b.case_id, b.trial_id, &b.judge))
    });

    // Flatten, reduce, roll up (R9, R12).
    let pass_threshold = summary.pass_threshold.unwrap_or(1.0);
    let mut judge_errors = 0u32;
    let mut tokens = JudgeTokenTotals::default();
    let mut cost: Option<f64> = None;
    for case_summary in summary.cases.iter_mut() {
        let case_id = case_summary.id.clone();
        let case_dir = run_dir.join(&case_id);
        let dirs = trial_dirs(&case_dir)?;
        if dirs.is_empty() {
            continue;
        }
        let in_scope: Vec<Arc<ResolvedJudge>> = judge_list
            .iter()
            .filter(|j| j.applies_to(&case_id))
            .cloned()
            .collect();
        let mut trials = Vec::with_capacity(dirs.len());
        let mut per_judge: BTreeMap<String, Vec<Judgment>> = BTreeMap::new();
        for (_, trial_dir) in &dirs {
            let result = flatten_trial(trial_dir, &in_scope, &case_id)?;
            let judgeable = matches!(result.status, CaseStatus::Passed | CaseStatus::Failed);
            trials.push(result);
            let judgments = read_judgments(trial_dir)?;
            for j in &judgments {
                tokens.input += j.usage.input;
                tokens.output += j.usage.output;
                tokens.total += j.usage.total;
                if let Some(c) = j.cost_usd {
                    cost = Some(cost.unwrap_or(0.0) + c);
                }
            }
            for judge in &in_scope {
                if let Some(latest) = latest_for(&judgments, &judge.name) {
                    if judgeable && !latest.is_scored() {
                        judge_errors += 1;
                    }
                    per_judge
                        .entry(judge.name.clone())
                        .or_default()
                        .push(latest.clone());
                }
            }
        }
        let previous = read_aggregated(&case_dir)?;
        let total_trials = previous
            .as_ref()
            .map(|p| p.total_trials)
            .or(summary.trials_per_case)
            .unwrap_or(trials.len() as u32)
            .max(trials.len() as u32);
        let mut aggregated = aggregate_trials(&case_id, trials, total_trials, pass_threshold);
        for judge in &in_scope {
            let latest: Vec<&Judgment> = per_judge
                .get(&judge.name)
                .map(|v| v.iter().collect())
                .unwrap_or_default();
            if let Some(scores) = reduce_scores(judge, &latest) {
                aggregated.scores.insert(judge.name.clone(), scores);
            }
        }
        write_case_trials_summary(run_dir, &case_id, &aggregated)?;

        case_summary.status = aggregated.aggregated_status;
        case_summary.pass_count = Some(aggregated.pass_count);
        case_summary.total_trials = Some(aggregated.total_trials);
        case_summary.pass_rate = Some(aggregated.pass_rate);
        case_summary.error_count = Some(aggregated.error_count);
        case_summary.scored_trials = Some(aggregated.scored_trials);
        case_summary.judge_excluded_count = Some(aggregated.judge_excluded_count);
        case_summary.scores = aggregated.scores.clone();
        case_summary.trials = aggregated.trials;
    }

    let passed = summary
        .cases
        .iter()
        .filter(|c| c.status == CaseStatus::Passed)
        .count();
    let scored_cases = summary
        .cases
        .iter()
        .filter(|c| matches!(c.status, CaseStatus::Passed | CaseStatus::Failed))
        .count();
    summary.passed = passed;
    summary.failed = summary.total_cases.saturating_sub(passed);
    let rate = if scored_cases == 0 {
        0.0
    } else {
        passed as f64 / scored_cases as f64
    };
    summary.suite_pass_rate = Some(rate);
    summary.suite_pass = match opts.suite_rule {
        SuitePassRule::AllCases => scored_cases > 0 && passed == scored_cases,
        SuitePassRule::RateAtLeast(min) => scored_cases > 0 && rate >= min,
    };
    summary.judge_errors = Some(judge_errors);
    summary.judge_skipped_trials = Some(report.skipped_error_trials);
    summary.judge_tokens = Some(tokens);
    summary.judge_cost_usd = cost;
    write_summary(run_dir, &summary)?;

    report.errors = judge_errors;
    report.suite_pass = summary.suite_pass;
    report.per_trial = per_trial;
    report.tokens = tokens;
    Ok(report)
}

#[cfg(test)]
mod tests;
