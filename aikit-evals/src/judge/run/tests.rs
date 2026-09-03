//! End-to-end judging against a mock OpenAI-compatible endpoint.
//!
//! Every test drives the real provider over HTTP (mockito), so what these
//! assert is what a provider actually receives — the spec's verification list
//! is about the wire, and a fake gateway could not witness it.

use super::*;
use crate::artifacts::{CaseSummary, SummaryResult, TokenBreakdown};
use crate::checks::CheckResult;
use crate::judge::config::{Criterion, CriterionKind};
use crate::judge::record::{read_judgments, Judgment};
use crate::judge::schema::{output_contract, rubric_text};
use crate::suite::EvalCase;
use serde_json::{json, Value};
use std::sync::{Arc as StdArc, Mutex as StdMutex};
use tempfile::TempDir;

const KEY_ENV: &str = "AIKIT_TEST_JUDGE_KEY";
const KEY: &str = "sk-test-judge-key";

fn set_key() {
    std::env::set_var(KEY_ENV, KEY);
}

/// Removes ambient credentials/endpoint for the body of one test and puts
/// them back. Tests run `--test-threads=1` in CI; no other test in this crate
/// reads these variables.
struct EnvGuard(Vec<(String, Option<String>)>);

impl EnvGuard {
    fn without(names: &[&str]) -> Self {
        let saved = names
            .iter()
            .map(|n| {
                let old = std::env::var(n).ok();
                std::env::remove_var(n);
                (n.to_string(), old)
            })
            .collect();
        Self(saved)
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (name, old) in &self.0 {
            match old {
                Some(v) => std::env::set_var(name, v),
                None => std::env::remove_var(name),
            }
        }
    }
}

// ---------------------------------------------------------------- fixture

struct Fixture {
    dir: TempDir,
    cases: Vec<CaseSummary>,
}

fn deterministic_check(passed: bool) -> CheckResult {
    CheckResult {
        check_name: "command_contains".to_string(),
        passed,
        required: true,
        message: None,
        not_observable: None,
        score: None,
    }
}

fn trial_result(trial_id: u32, status: CaseStatus, checks: Vec<CheckResult>) -> TrialResult {
    TrialResult {
        trial_id,
        status,
        command_count: None,
        input_tokens: None,
        output_tokens: None,
        check_results: checks,
        error_message: None,
        exit_code: None,
        terminal: None,
        cost_usd: None,
        tokens: TokenBreakdown::default(),
        skill_path: None,
        judge_excluded: false,
    }
}

fn final_message(text: &str) -> Value {
    json!({"type": "message", "role": "assistant", "text": text, "kind": "message", "phase": "final"})
}

fn unphased_message(text: &str) -> Value {
    json!({"type": "message", "role": "assistant", "text": text})
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("run")).unwrap();
        std::fs::create_dir_all(dir.path().join("skill")).unwrap();
        std::fs::write(dir.path().join("skill/SKILL.md"), "# the skill\nbody\n").unwrap();
        Self {
            dir,
            cases: Vec::new(),
        }
    }

    fn run_dir(&self) -> PathBuf {
        self.dir.path().join("run")
    }

    fn checks_path(&self) -> PathBuf {
        self.dir.path().join("checks.toml")
    }

    fn trial_dir(&self, case_id: &str, trial_id: u32) -> PathBuf {
        self.run_dir()
            .join(case_id)
            .join(format!("trial-{trial_id}"))
    }

    /// Write one trial's artifacts and remember it for `summary.json`.
    fn add_trial(
        &mut self,
        case_id: &str,
        trial_id: u32,
        status: CaseStatus,
        trace: &[Value],
        checks: Vec<CheckResult>,
    ) {
        let dir = self.trial_dir(case_id, trial_id);
        std::fs::create_dir_all(&dir).unwrap();
        let lines: Vec<String> = trace
            .iter()
            .enumerate()
            .map(|(i, p)| json!({"seq": i, "payload": p}).to_string())
            .collect();
        std::fs::write(dir.join("trace.jsonl"), lines.join("\n") + "\n").unwrap();
        let result = trial_result(trial_id, status.clone(), checks);
        std::fs::write(
            dir.join("result.json"),
            serde_json::to_string_pretty(&result).unwrap(),
        )
        .unwrap();

        match self.cases.iter_mut().find(|c| c.id == case_id) {
            Some(existing) => existing.trials.push(result),
            None => self.cases.push(CaseSummary {
                id: case_id.to_string(),
                status,
                command_count: None,
                input_tokens: None,
                output_tokens: None,
                pass_count: None,
                total_trials: None,
                pass_rate: None,
                error_count: None,
                scored_trials: None,
                should_trigger: Some(true),
                judge_excluded_count: None,
                scores: Default::default(),
                trials: vec![result_placeholder(trial_id)],
            }),
        }
    }

    /// A passing trial that answered `answer`, with one passing check.
    fn passing_trial(&mut self, case_id: &str, trial_id: u32, answer: &str) {
        self.add_trial(
            case_id,
            trial_id,
            CaseStatus::Passed,
            &[final_message(answer)],
            vec![deterministic_check(true)],
        );
    }

    fn write_checks(&self, toml: &str) {
        std::fs::write(self.checks_path(), toml).unwrap();
    }

    /// Write `summary.json` from the trials added so far.
    fn write_summary(&self) {
        let cases: Vec<CaseSummary> = self
            .cases
            .iter()
            .map(|c| {
                let trials: Vec<TrialResult> = trial_dirs(&self.run_dir().join(&c.id))
                    .unwrap()
                    .into_iter()
                    .map(|(_, d)| {
                        serde_json::from_str(
                            &std::fs::read_to_string(d.join("result.json")).unwrap(),
                        )
                        .unwrap()
                    })
                    .collect();
                let mut c = c.clone();
                c.total_trials = Some(trials.len() as u32);
                c.trials = trials;
                c
            })
            .collect();
        let passed = cases
            .iter()
            .filter(|c| c.status == CaseStatus::Passed)
            .count();
        let summary = SummaryResult {
            suite_pass: passed == cases.len(),
            suite_pass_rate: None,
            agent: "test-agent".to_string(),
            model: Some("target-model".to_string()),
            total_cases: cases.len(),
            passed,
            failed: cases.len() - passed,
            trials_per_case: Some(1),
            parallel: Some(2),
            pass_threshold: Some(1.0),
            run_dir: self.run_dir(),
            checks_path: Some(self.checks_path()),
            skill_project_root: self.dir.path().join("skill"),
            isolation: None,
            judge_errors: None,
            judge_skipped_trials: None,
            judge_tokens: None,
            judge_cost_usd: None,
            skill_git_sha: None,
            skill_dirty: None,
            cases,
        };
        write_summary(&self.run_dir(), &summary).unwrap();
    }

    fn summary(&self) -> SummaryResult {
        read_summary(&self.run_dir()).unwrap()
    }

    fn judgments(&self, case_id: &str, trial_id: u32) -> Vec<Judgment> {
        read_judgments(&self.trial_dir(case_id, trial_id)).unwrap()
    }

    fn trial(&self, case_id: &str, trial_id: u32) -> TrialResult {
        read_trial_result(&self.trial_dir(case_id, trial_id)).unwrap()
    }

    fn aggregated(&self, case_id: &str) -> CaseTrialsResult {
        read_aggregated(&self.run_dir().join(case_id))
            .unwrap()
            .unwrap()
    }
}

fn result_placeholder(trial_id: u32) -> TrialResult {
    trial_result(trial_id, CaseStatus::Passed, vec![])
}

fn suite(ids: &[&str]) -> EvalSuite {
    EvalSuite::new(
        ids.iter()
            .map(|id| EvalCase {
                id: (*id).to_string(),
                prompt: format!("prompt for {id}"),
                should_trigger: true,
                tags: vec![],
                workspace_subdir: None,
                extra: [("expected".to_string(), "42".to_string())]
                    .into_iter()
                    .collect(),
            })
            .collect(),
    )
}

/// A checks file with one two-criterion judge pointed at `base_url`.
fn checks_toml(base_url: &str, extra: &str) -> String {
    format!(
        r#"
[[check]]
name = "command_contains"
pattern = "fastskill"

[[judge]]
name = "quality"
model = "judge-1"
base_url = "{base_url}"
api_key_env = "{KEY_ENV}"
prompt = """Answer: {{{{trial.final_answer}}}}

{{{{rubric}}}}

{{{{output_contract}}}}"""
{extra}

[[judge.criterion]]
name = "clear"
kind = "scale"
description = "Is it clear?"

[[judge.criterion]]
name = "runs"
kind = "bool"
description = "Would it run?"
"#
    )
}

/// The criteria `checks_toml` declares, for sizing the generated variables.
fn fixture_criteria() -> Vec<Criterion> {
    vec![
        Criterion {
            name: "clear".to_string(),
            kind: CriterionKind::Scale,
            scale: 5,
            description: "Is it clear?".to_string(),
        },
        Criterion {
            name: "runs".to_string(),
            kind: CriterionKind::Bool,
            scale: 2,
            description: "Would it run?".to_string(),
        },
    ]
}

fn chat_response(content: &str) -> String {
    json!({
        "id": "cmpl-1",
        "model": "judge-1-actual",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": content}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
    })
    .to_string()
}

fn good_reply() -> String {
    chat_response(
        &json!({"criteria": [
            {"name": "clear", "reasoning": "clear enough", "answer": 4},
            {"name": "runs", "reasoning": "it would", "answer": true}
        ]})
        .to_string(),
    )
}

fn reply_with(clear: i64, runs: bool) -> String {
    chat_response(
        &json!({"criteria": [
            {"name": "clear", "reasoning": "r", "answer": clear},
            {"name": "runs", "reasoning": "r", "answer": runs}
        ]})
        .to_string(),
    )
}

/// A reply that omits a declared criterion: the schema rejects it.
fn short_reply() -> String {
    chat_response(
        &json!({"criteria": [{"name": "clear", "reasoning": "r", "answer": 4}]}).to_string(),
    )
}

type Bodies = StdArc<StdMutex<Vec<Value>>>;

fn recorder() -> Bodies {
    StdArc::new(StdMutex::new(Vec::new()))
}

/// Match every chat completion, recording the body it carried.
fn recording(server: &mut mockito::ServerGuard, bodies: &Bodies) -> mockito::Mock {
    recording_when(server, bodies, |_| true)
}

/// Match the chat completions whose body satisfies `want`, recording them.
///
/// mockito evaluates every registered matcher against every request, so a
/// matcher that records unconditionally records once per mock. A sequence of
/// mocks therefore has to discriminate on the body, not on arrival order.
fn recording_when(
    server: &mut mockito::ServerGuard,
    bodies: &Bodies,
    want: impl Fn(&Value) -> bool + Send + Sync + 'static,
) -> mockito::Mock {
    let bodies = bodies.clone();
    server
        .mock("POST", "/chat/completions")
        .match_request(move |req| {
            let text = req.utf8_lossy_body().unwrap_or_default().to_string();
            match serde_json::from_str::<Value>(&text) {
                Ok(v) if want(&v) => {
                    bodies.lock().unwrap().push(v);
                    true
                }
                _ => false,
            }
        })
}

/// How many messages a request body carries.
fn turns(body: &Value) -> usize {
    body["messages"].as_array().map(|m| m.len()).unwrap_or(0)
}

fn fast_opts() -> JudgeRunOptions {
    JudgeRunOptions {
        backoff_base_ms: 1,
        ..Default::default()
    }
}

// ------------------------------------------------------------------ tests

#[tokio::test]
async fn judges_a_trial_and_records_scores_rows_and_totals() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let mock = recording(&mut server, &bodies)
        .with_status(200)
        .with_body(good_reply())
        .expect(1)
        .create();

    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, "the final answer");
    fx.write_checks(&checks_toml(&server.url(), "min_score = 0.5"));
    fx.write_summary();

    let report = judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .expect("judging succeeds");
    mock.assert();

    assert_eq!(report.judges, vec!["quality"]);
    assert_eq!(report.judged, 1);
    assert_eq!(report.errors, 0);
    assert!(report.suite_pass);
    assert_eq!(
        report.tokens,
        JudgeTokenTotals {
            input: 10,
            output: 5,
            total: 15
        }
    );

    // R5: the engine computes the scores, 4/5 and true.
    let judgments = fx.judgments("c1", 1);
    assert_eq!(judgments.len(), 1);
    let j = &judgments[0];
    let scores = j.scores.clone().unwrap();
    assert_eq!(scores["clear"], 0.75);
    assert_eq!(scores["runs"], 1.0);
    assert_eq!(scores["overall"], 0.875);
    assert_eq!(j.schema, "aikit.judgment/1");
    assert_eq!(j.identity.model, "judge-1");
    assert_eq!(j.identity.model_reported.as_deref(), Some("judge-1-actual"));
    assert_eq!(j.identity.endpoint_host, server.host_with_port());
    assert_eq!(j.attempts.len(), 1);
    assert_eq!(j.attempts[0].kind, aikit_sdk::AttemptKind::Validation);
    assert!(j.error.is_none());
    assert!(j.truncated.is_empty());

    // R11: the artifact never carries the credential.
    let raw = std::fs::read_to_string(fx.trial_dir("c1", 1).join("judgments.json")).unwrap();
    assert!(!raw.contains(KEY), "judgments.json leaked the api key");
    assert!(
        !raw.contains("Bearer"),
        "judgments.json leaked a bearer header"
    );
    assert!(!raw.contains("api_key"));

    // R9: a gated judge's row carries the score and is required.
    let trial = fx.trial("c1", 1);
    let row = trial
        .check_results
        .iter()
        .find(|r| r.check_name == "judge:quality")
        .expect("judge row");
    assert_eq!(row.score, Some(0.875));
    assert!(row.passed && row.required);
    assert_eq!(trial.status, CaseStatus::Passed);
    assert!(!trial.judge_excluded);

    // R12: the case and the run carry the reduction.
    let agg = fx.aggregated("c1");
    assert_eq!(agg.scores["quality"].overall, 0.875);
    assert_eq!(agg.scores["quality"].judged_trials, 1);
    assert_eq!(agg.scores["quality"].criteria["runs"], 1.0);
    let summary = fx.summary();
    assert_eq!(summary.cases[0].scores["quality"].overall, 0.875);
    assert_eq!(summary.judge_errors, Some(0));
    assert_eq!(
        summary.judge_tokens,
        Some(JudgeTokenTotals {
            input: 10,
            output: 5,
            total: 15
        })
    );
    assert_eq!(summary.judge_cost_usd, None, "cost is only ever reported");
    assert!(summary.suite_pass);
}

#[tokio::test]
async fn the_request_is_one_user_message_with_no_tools_and_no_streaming() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let mock = recording(&mut server, &bodies)
        .with_status(200)
        .with_body(good_reply())
        .expect(1)
        .create();

    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, "the final answer");
    fx.write_checks(&checks_toml(&server.url(), ""));
    fx.write_summary();
    judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .unwrap();
    mock.assert();

    let sent = bodies.lock().unwrap();
    assert_eq!(sent.len(), 1);
    let body = &sent[0];
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1, "no system_prompt declared: {body}");
    assert_eq!(messages[0]["role"], json!("user"));
    let text = messages[0]["content"].as_str().unwrap();
    assert!(text.contains("Answer: the final answer"), "{text}");
    assert!(text.contains("clear (1–5)"), "rubric missing: {text}");
    assert!(
        text.contains("\"maximum\": 5"),
        "output contract missing: {text}"
    );
    assert_eq!(body["temperature"], json!(0.0));
    assert_eq!(body["stream"], json!(false));
    assert_eq!(body["max_tokens"], json!(4096));
    assert_eq!(body["model"], json!("judge-1"));
    assert!(body.get("tools").is_none(), "a judge call carries no tools");
    assert!(body.get("tool_choice").is_none());
    assert!(
        body.get("top_p").is_none(),
        "top_p is sent only when declared"
    );
    assert!(body.get("agent").is_none());
}

#[tokio::test]
async fn a_declared_system_prompt_adds_exactly_one_message() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let mock = recording(&mut server, &bodies)
        .with_status(200)
        .with_body(good_reply())
        .expect(1)
        .create();

    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, "the final answer");
    fx.write_checks(&checks_toml(
        &server.url(),
        "system_prompt = \"You grade skills for case {{case.prompt}}.\"",
    ));
    fx.write_summary();
    judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .unwrap();
    mock.assert();

    let sent = bodies.lock().unwrap();
    let messages = sent[0]["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], json!("system"));
    assert_eq!(
        messages[0]["content"],
        json!("You grade skills for case prompt for c1.")
    );
    assert_eq!(messages[1]["role"], json!("user"));
}

#[tokio::test]
async fn a_rejected_reply_is_retried_once_with_the_rendered_retry_prompt() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let first = recording_when(&mut server, &bodies, |b| turns(b) == 1)
        .with_status(200)
        .with_body(short_reply())
        .expect(1)
        .create();
    let second = recording_when(&mut server, &bodies, |b| turns(b) == 3)
        .with_status(200)
        .with_body(good_reply())
        .expect(1)
        .create();

    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, "the final answer");
    fx.write_checks(&checks_toml(
        &server.url(),
        "retry_prompt = \"Your reply was rejected: {{validation_error}}\"",
    ));
    fx.write_summary();
    let report = judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .unwrap();
    first.assert();
    second.assert();
    assert_eq!(report.judged, 1);
    assert_eq!(report.errors, 0);

    // R7: the corrective turn is the rejected reply as assistant, then the
    // rendered retry_prompt as user. Nothing else is injected.
    let sent = bodies.lock().unwrap();
    assert_eq!(sent.len(), 2);
    let messages = sent[1]["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["role"], json!("user"));
    assert_eq!(messages[1]["role"], json!("assistant"));
    assert!(messages[1]["content"]
        .as_str()
        .unwrap()
        .contains("\"clear\""));
    assert_eq!(messages[2]["role"], json!("user"));
    let corrective = messages[2]["content"].as_str().unwrap();
    assert!(
        corrective.starts_with("Your reply was rejected: "),
        "{corrective}"
    );
    assert!(corrective.len() > "Your reply was rejected: ".len());

    let j = &fx.judgments("c1", 1)[0];
    assert_eq!(j.attempts.len(), 2);
    assert!(j
        .attempts
        .iter()
        .all(|a| a.kind == aikit_sdk::AttemptKind::Validation));
    assert!(j.attempts[0].error.is_some(), "the rejection is recorded");
    assert_eq!(j.usage.total, 30, "both attempts' tokens are counted");
}

#[tokio::test]
async fn without_a_retry_prompt_a_rejected_reply_is_asked_once_and_recorded_as_an_error() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let mock = recording(&mut server, &bodies)
        .with_status(200)
        .with_body(short_reply())
        .expect(1)
        .create();

    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, "the final answer");
    fx.write_checks(&checks_toml(&server.url(), "min_score = 0.5"));
    fx.write_summary();
    let report = judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .unwrap();
    mock.assert();

    assert_eq!(report.judged, 0);
    assert_eq!(report.errors, 1);
    assert!(!report.is_clean());
    let j = &fx.judgments("c1", 1)[0];
    assert_eq!(j.attempts.len(), 1);
    assert!(j.scores.is_none());
    assert!(
        j.error.as_ref().unwrap().contains("rejected"),
        "{:?}",
        j.error
    );
}

#[tokio::test]
async fn an_unauthorized_response_fails_after_exactly_one_attempt() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let mock = recording(&mut server, &bodies)
        .with_status(401)
        .with_body("{\"error\": \"bad key\"}")
        .expect(1)
        .create();

    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, "the final answer");
    fx.write_checks(&checks_toml(&server.url(), "min_score = 0.5"));
    fx.write_summary();
    let report = judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .unwrap();
    mock.assert();

    assert_eq!(report.errors, 1);
    let j = &fx.judgments("c1", 1)[0];
    assert_eq!(j.attempts.len(), 1, "401 is not retried");
    assert_eq!(j.attempts[0].kind, aikit_sdk::AttemptKind::Transport);
    assert!(j.error.as_ref().unwrap().contains("401"), "{:?}", j.error);
}

#[tokio::test]
async fn a_transient_failure_is_retried_and_recorded_as_transport() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let first = server
        .mock("POST", "/chat/completions")
        .with_status(503)
        .with_body("upstream down")
        .expect(1)
        .create();
    let second = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_body(good_reply())
        .expect(1)
        .create();

    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, "the final answer");
    fx.write_checks(&checks_toml(&server.url(), ""));
    fx.write_summary();
    let report = judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .unwrap();
    first.assert();
    second.assert();
    assert_eq!(report.judged, 1);

    let j = &fx.judgments("c1", 1)[0];
    assert_eq!(j.attempts.len(), 2);
    assert_eq!(j.attempts[0].kind, aikit_sdk::AttemptKind::Transport);
    assert_eq!(j.attempts[1].kind, aikit_sdk::AttemptKind::Validation);
    assert_eq!(j.overall(), Some(0.875));
}

#[tokio::test]
async fn an_errored_trial_is_skipped_without_a_request() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let mock = recording(&mut server, &bodies)
        .with_status(200)
        .expect(0)
        .create();

    let mut fx = Fixture::new();
    fx.add_trial(
        "c1",
        1,
        CaseStatus::Error,
        &[final_message("never mind")],
        vec![],
    );
    fx.write_checks(&checks_toml(&server.url(), "min_score = 0.5"));
    fx.write_summary();
    let report = judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .unwrap();
    mock.assert();

    assert_eq!(report.judged, 0);
    assert_eq!(report.errors, 0, "an errored trial is not a judge error");
    assert_eq!(report.skipped_error_trials, 1);
    assert!(matches!(
        report.per_trial[0].outcome,
        JudgeOutcome::SkippedError
    ));
    assert!(fx.judgments("c1", 1).is_empty());
    assert!(fx
        .trial("c1", 1)
        .check_results
        .iter()
        .all(|r| r.check_name != "judge:quality"));
}

#[tokio::test]
async fn an_empty_final_answer_is_sent_as_the_literal() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let mock = recording(&mut server, &bodies)
        .with_status(200)
        .with_body(good_reply())
        .expect(1)
        .create();

    let mut fx = Fixture::new();
    fx.add_trial(
        "c1",
        1,
        CaseStatus::Failed,
        &[final_message("   ")],
        vec![deterministic_check(false)],
    );
    fx.write_checks(&checks_toml(&server.url(), ""));
    fx.write_summary();
    judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .unwrap();
    mock.assert();

    let sent = bodies.lock().unwrap();
    let text = sent[0]["messages"][0]["content"].as_str().unwrap();
    assert!(text.contains("Answer: [no final answer]"), "{text}");
}

#[tokio::test]
async fn a_trace_without_phase_is_a_judge_error_naming_the_variable() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let mock = recording(&mut server, &bodies)
        .with_status(200)
        .expect(0)
        .create();

    let mut fx = Fixture::new();
    fx.add_trial(
        "c1",
        1,
        CaseStatus::Passed,
        &[unphased_message("an answer from an older run")],
        vec![deterministic_check(true)],
    );
    fx.write_checks(&checks_toml(&server.url(), "min_score = 0.5"));
    fx.write_summary();
    let report = judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .unwrap();
    mock.assert();

    assert_eq!(report.errors, 1);
    let j = &fx.judgments("c1", 1)[0];
    assert!(j.attempts.is_empty(), "nothing was asked");
    let error = j.error.clone().unwrap();
    assert!(error.contains("{{trial.final_answer}}"), "{error}");
    assert!(error.contains("phase"), "{error}");
}

#[tokio::test]
async fn a_gated_judge_that_renders_nothing_excludes_the_trial_and_errors_the_case() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let mock = recording(&mut server, &bodies)
        .with_status(500)
        .with_body("boom")
        .expect(4)
        .create();

    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, "the final answer");
    fx.write_checks(&checks_toml(&server.url(), "min_score = 0.5"));
    fx.write_summary();
    let report = judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .unwrap();
    mock.assert();

    assert_eq!(report.errors, 1);
    assert!(!report.suite_pass);
    let trial = fx.trial("c1", 1);
    assert!(trial.judge_excluded);
    let row = trial
        .check_results
        .iter()
        .find(|r| r.check_name == "judge:quality")
        .unwrap();
    assert_eq!(row.score, None);
    assert!(!row.passed && row.required);
    assert_eq!(
        trial.status,
        CaseStatus::Passed,
        "an excluded row does not fail the trial, it removes it"
    );

    let agg = fx.aggregated("c1");
    assert_eq!(agg.judge_excluded_count, 1);
    assert_eq!(agg.judge_excluded_count, agg.total_trials);
    assert_eq!(agg.scored_trials, 0);
    assert_eq!(agg.aggregated_status, CaseStatus::Error);
    assert!(agg.scores.is_empty());
    let summary = fx.summary();
    assert_eq!(summary.cases[0].judge_excluded_count, Some(1));
    assert_eq!(summary.judge_errors, Some(1));
}

#[tokio::test]
async fn an_advisory_judge_never_changes_a_verdict() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    // Worst possible scores, advisory: the verdict must not move.
    let mock = recording(&mut server, &bodies)
        .with_status(200)
        .with_body(reply_with(1, false))
        .expect(1)
        .create();

    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, "the final answer");
    fx.write_checks(&checks_toml(&server.url(), ""));
    fx.write_summary();
    let report = judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .unwrap();
    mock.assert();

    let trial = fx.trial("c1", 1);
    let row = trial
        .check_results
        .iter()
        .find(|r| r.check_name == "judge:quality")
        .unwrap();
    assert_eq!(row.score, Some(0.0));
    assert!(row.passed, "an advisory row always passes");
    assert!(!row.required);
    assert_eq!(trial.status, CaseStatus::Passed);
    assert!(!trial.judge_excluded);
    assert_eq!(fx.aggregated("c1").aggregated_status, CaseStatus::Passed);
    assert!(report.suite_pass);
    // The score is still recorded — advisory means ungated, not unmeasured.
    assert_eq!(fx.summary().cases[0].scores["quality"].overall, 0.0);
}

#[tokio::test]
async fn a_gated_judge_below_min_score_fails_the_trial_and_the_case() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let mock = recording(&mut server, &bodies)
        .with_status(200)
        .with_body(reply_with(2, false))
        .expect(1)
        .create();

    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, "the final answer");
    fx.write_checks(&checks_toml(&server.url(), "min_score = 0.5"));
    fx.write_summary();
    let report = judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .unwrap();
    mock.assert();

    // clear 2/5 = 0.25, runs false = 0.0 → overall 0.125 < 0.5
    let trial = fx.trial("c1", 1);
    let row = trial
        .check_results
        .iter()
        .find(|r| r.check_name == "judge:quality")
        .unwrap();
    assert_eq!(row.score, Some(0.125));
    assert!(!row.passed && row.required);
    assert_eq!(trial.status, CaseStatus::Failed);
    assert!(!trial.judge_excluded, "a scored judge never excludes");
    assert_eq!(fx.aggregated("c1").aggregated_status, CaseStatus::Failed);
    assert!(!fx.summary().suite_pass);
    assert!(!report.suite_pass);
    assert_eq!(report.errors, 0, "a low score is a verdict, not an error");
}

#[tokio::test]
async fn judging_twice_asks_nothing_and_rejudge_appends_one_more() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let mock = recording(&mut server, &bodies)
        .with_status(200)
        .with_body(good_reply())
        .expect(2)
        .create();

    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, "the final answer");
    fx.write_checks(&checks_toml(&server.url(), "min_score = 0.5"));
    fx.write_summary();

    let first = judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .unwrap();
    assert_eq!(first.judged, 1);
    assert_eq!(fx.judgments("c1", 1).len(), 1);

    let second = judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .unwrap();
    assert_eq!(second.judged, 0);
    assert_eq!(second.skipped_cached, 1);
    assert_eq!(fx.judgments("c1", 1).len(), 1, "nothing appended");

    let opts = JudgeRunOptions {
        rejudge: true,
        ..fast_opts()
    };
    let third = judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &opts)
        .await
        .unwrap();
    assert_eq!(third.judged, 1);
    let judgments = fx.judgments("c1", 1);
    assert_eq!(judgments.len(), 2, "append-only");
    assert_eq!(judgments[0].cache_key, judgments[1].cache_key);
    mock.assert();
}

#[tokio::test]
async fn a_changed_prompt_invalidates_the_cache() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let mock = recording(&mut server, &bodies)
        .with_status(200)
        .with_body(good_reply())
        .expect(2)
        .create();

    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, "the final answer");
    fx.write_checks(&checks_toml(&server.url(), "min_score = 0.5"));
    fx.write_summary();
    judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .unwrap();

    fx.write_checks(&checks_toml(
        &server.url(),
        "min_score = 0.5\nsystem_prompt = \"Be strict.\"",
    ));
    let second = judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .unwrap();
    mock.assert();
    assert_eq!(second.judged, 1);
    assert_eq!(second.skipped_cached, 0);
    let judgments = fx.judgments("c1", 1);
    assert_ne!(judgments[0].judge_hash, judgments[1].judge_hash);
    assert_ne!(judgments[0].cache_key, judgments[1].cache_key);
}

#[tokio::test]
async fn a_judge_scoped_to_cases_leaves_the_others_alone() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let mock = recording(&mut server, &bodies)
        .with_status(200)
        .with_body(good_reply())
        .expect(1)
        .create();

    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, "first answer");
    fx.passing_trial("c2", 1, "second answer");
    fx.write_checks(&checks_toml(
        &server.url(),
        "cases = [\"c2\"]\nmin_score = 0.5",
    ));
    fx.write_summary();
    judge_run_dir(&fx.run_dir(), &suite(&["c1", "c2"]), &fast_opts())
        .await
        .unwrap();
    mock.assert();

    assert!(fx.judgments("c1", 1).is_empty());
    assert_eq!(fx.judgments("c2", 1).len(), 1);
    assert!(fx
        .trial("c1", 1)
        .check_results
        .iter()
        .all(|r| r.check_name != "judge:quality"));
    let sent = bodies.lock().unwrap();
    assert!(sent[0]["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("second answer"));
}

#[tokio::test]
async fn every_trial_of_a_case_is_judged() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let mock = recording(&mut server, &bodies)
        .with_status(200)
        .with_body(good_reply())
        .expect(2)
        .create();

    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, "first try");
    fx.passing_trial("c1", 2, "second try");
    fx.write_checks(&checks_toml(&server.url(), "min_score = 0.5"));
    fx.write_summary();
    let report = judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .unwrap();
    mock.assert();

    assert_eq!(report.judged, 2);
    assert_eq!(fx.aggregated("c1").scores["quality"].judged_trials, 2);
    assert_eq!(fx.summary().judge_tokens.unwrap().total, 30);
}

#[tokio::test]
async fn no_endpoint_anywhere_fails_before_any_request() {
    set_key();
    let _guard = EnvGuard::without(&["AIKIT_LLM_URL"]);
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let mock = recording(&mut server, &bodies)
        .with_status(200)
        .expect(0)
        .create();

    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, "the final answer");
    let toml = checks_toml(&server.url(), "min_score = 0.5")
        .replace(&format!("base_url = \"{}\"\n", server.url()), "");
    assert!(!toml.contains("base_url"));
    fx.write_checks(&toml);
    fx.write_summary();

    let err = judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .expect_err("no endpoint is a pre-flight failure");
    mock.assert();
    assert!(matches!(err, JudgeError::Preflight(_)), "{err}");
    assert!(err.to_string().contains("endpoint"), "{err}");
    assert!(fx.judgments("c1", 1).is_empty());
}

#[tokio::test]
async fn a_missing_api_key_variable_fails_before_any_request() {
    let _guard = EnvGuard::without(&["OPENAI_API_KEY", "AIKIT_API_KEY", "AIKIT_TEST_ABSENT_KEY"]);
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let mock = recording(&mut server, &bodies)
        .with_status(200)
        .expect(0)
        .create();

    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, "the final answer");
    fx.write_checks(
        &checks_toml(&server.url(), "min_score = 0.5").replace(KEY_ENV, "AIKIT_TEST_ABSENT_KEY"),
    );
    fx.write_summary();

    let err = judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .expect_err("a missing key is a pre-flight failure");
    mock.assert();
    assert!(matches!(err, JudgeError::Preflight(_)), "{err}");
    assert!(err.to_string().contains("AIKIT_TEST_ABSENT_KEY"), "{err}");
}

#[tokio::test]
async fn an_invalid_judge_is_refused_before_any_request() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let mock = recording(&mut server, &bodies)
        .with_status(200)
        .expect(0)
        .create();

    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, "the final answer");
    fx.write_checks(&checks_toml(&server.url(), "cases = [\"ghost\"]"));
    fx.write_summary();

    let err = judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .expect_err("a cases list naming nothing is invalid");
    mock.assert();
    assert!(matches!(err, JudgeError::Invalid(_)), "{err}");
    assert!(err.to_string().contains("ghost"), "{err}");
}

#[tokio::test]
async fn a_checks_file_without_judges_changes_nothing() {
    set_key();
    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, "the final answer");
    fx.write_checks("[[check]]\nname = \"command_contains\"\npattern = \"fastskill\"\n");
    fx.write_summary();
    let before = std::fs::read_to_string(fx.trial_dir("c1", 1).join("result.json")).unwrap();

    let report = judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .unwrap();
    assert!(report.judges.is_empty());
    assert_eq!(report.judged, 0);
    assert!(report.suite_pass);
    let after = std::fs::read_to_string(fx.trial_dir("c1", 1).join("result.json")).unwrap();
    assert_eq!(before, after, "no judges: not a byte of the run changes");
    assert!(fx.summary().judge_tokens.is_none());
}

#[tokio::test]
async fn the_judge_model_override_is_what_gets_sent_and_recorded() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let mock = recording(&mut server, &bodies)
        .with_status(200)
        .with_body(good_reply())
        .expect(1)
        .create();

    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, "the final answer");
    fx.write_checks(&checks_toml(&server.url(), "min_score = 0.5"));
    fx.write_summary();
    let opts = JudgeRunOptions {
        judge_model: Some("override-model".to_string()),
        ..fast_opts()
    };
    judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &opts)
        .await
        .unwrap();
    mock.assert();

    assert_eq!(bodies.lock().unwrap()[0]["model"], json!("override-model"));
    assert_eq!(fx.judgments("c1", 1)[0].identity.model, "override-model");
}

#[tokio::test]
async fn a_case_column_renders_into_the_prompt() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let mock = recording(&mut server, &bodies)
        .with_status(200)
        .with_body(good_reply())
        .expect(1)
        .create();

    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, "the final answer");
    fx.write_checks(&checks_toml(&server.url(), "").replace(
        "Answer: {{trial.final_answer}}",
        "Expected {{case.expected}}, got {{trial.final_answer}}",
    ));
    fx.write_summary();
    judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .unwrap();
    mock.assert();

    let sent = bodies.lock().unwrap();
    let text = sent[0]["messages"][0]["content"].as_str().unwrap();
    assert!(text.contains("Expected 42, got the final answer"), "{text}");
}

#[tokio::test]
async fn a_truncated_variable_is_capped_and_listed() {
    set_key();
    let mut server = mockito::Server::new_async().await;
    let bodies = recorder();
    let mock = recording(&mut server, &bodies)
        .with_status(200)
        .with_body(good_reply())
        .expect(1)
        .create();

    // Every rendered variable is capped (R2), the engine-generated ones
    // included, so the cap has to clear them for this to isolate the answer.
    let cap = output_contract(&fixture_criteria())
        .len()
        .max(rubric_text(&fixture_criteria()).len())
        + 64;
    let long = "x".repeat(cap * 4);
    let mut fx = Fixture::new();
    fx.passing_trial("c1", 1, &long);
    fx.write_checks(&format!(
        "{}\n[judge_defaults]\nmax_var_bytes = {cap}\n",
        checks_toml(&server.url(), "")
    ));
    fx.write_summary();
    judge_run_dir(&fx.run_dir(), &suite(&["c1"]), &fast_opts())
        .await
        .unwrap();
    mock.assert();

    let sent = bodies.lock().unwrap();
    let text = sent[0]["messages"][0]["content"].as_str().unwrap();
    assert!(
        text.contains(&format!("[truncated {} bytes]", long.len() - cap)),
        "{text}"
    );
    assert!(!text.contains(&long));
    assert_eq!(
        fx.judgments("c1", 1)[0].truncated,
        vec!["trial.final_answer".to_string()]
    );
}
