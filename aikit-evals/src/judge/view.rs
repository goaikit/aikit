//! The trial view (spec eval-judge R2): what a judge prompt can see of one
//! trial, read back from the run dir. Every variable either renders from
//! evidence the run dir holds or fails naming itself — never a silent blank.

use crate::artifacts::{CaseStatus, TrialResult};
use crate::suite::EvalCase;
use crate::trace::{TraceEvent, TracePayload};
use serde_json::{json, Value};
use std::fmt;
use std::path::{Path, PathBuf};

/// The literal a judge sees when the agent produced no final answer (R8).
pub const NO_FINAL_ANSWER: &str = "[no final answer]";

#[derive(Debug, thiserror::Error)]
pub enum ViewError {
    #[error("EVAL_JUDGE_TRIAL: {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("EVAL_JUDGE_TRIAL: {path} line {line}: {source}")]
    Trace {
        path: PathBuf,
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("EVAL_JUDGE_TRIAL: {path}: {source}")]
    Result {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

/// A template variable the run dir cannot supply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarError {
    pub variable: String,
    pub reason: String,
}

impl VarError {
    fn new(variable: &str, reason: impl Into<String>) -> Self {
        Self {
            variable: variable.to_string(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for VarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{{{{{}}}}}` cannot be rendered: {}",
            self.variable, self.reason
        )
    }
}

impl std::error::Error for VarError {}

#[derive(Debug, Clone)]
struct Msg {
    role: String,
    text: String,
    kind: Option<String>,
    phase: Option<String>,
}

#[derive(Debug, Clone)]
enum Item {
    Msg(Msg),
    ToolUse {
        call_id: String,
        tool_name: String,
        input: Value,
    },
    ToolResult {
        call_id: String,
        output: Value,
        is_error: bool,
    },
}

/// One trial as a judge prompt sees it.
#[derive(Debug, Clone)]
pub struct TrialView {
    trial_dir: PathBuf,
    skill_project_root: PathBuf,
    result: TrialResult,
    items: Vec<Item>,
    workspace_diff: Option<String>,
}

fn read(path: &Path) -> Result<String, ViewError> {
    std::fs::read_to_string(path).map_err(|source| ViewError::Io {
        path: path.to_path_buf(),
        source,
    })
}

impl TrialView {
    /// Read `result.json`, `trace.jsonl` and, when present, `workspace.diff`.
    /// `skill_project_root` is the run's, used when the trial recorded no
    /// staged skill path.
    pub fn load(trial_dir: &Path, skill_project_root: &Path) -> Result<Self, ViewError> {
        let result_path = trial_dir.join("result.json");
        let result: TrialResult =
            serde_json::from_str(&read(&result_path)?).map_err(|source| ViewError::Result {
                path: result_path.clone(),
                source,
            })?;

        let trace_path = trial_dir.join("trace.jsonl");
        let mut items = Vec::new();
        for (i, line) in read(&trace_path)?.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let raw: Value = serde_json::from_str(line).map_err(|source| ViewError::Trace {
                path: trace_path.clone(),
                line: i + 1,
                source,
            })?;
            let kind = raw
                .get("payload")
                .and_then(|p| p.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if !matches!(kind, "message" | "tool_use" | "tool_result") {
                // Other payloads — raw lines, usage, terminal, or types this
                // version does not know — carry nothing a prompt renders.
                continue;
            }
            let event: TraceEvent =
                serde_json::from_value(raw).map_err(|source| ViewError::Trace {
                    path: trace_path.clone(),
                    line: i + 1,
                    source,
                })?;
            match event.payload {
                TracePayload::Message {
                    text,
                    role,
                    kind,
                    phase,
                } => items.push(Item::Msg(Msg {
                    role,
                    text,
                    kind,
                    phase,
                })),
                TracePayload::ToolUse {
                    call_id,
                    tool_name,
                    input,
                } => items.push(Item::ToolUse {
                    call_id,
                    tool_name,
                    input,
                }),
                TracePayload::ToolResult {
                    call_id,
                    output,
                    is_error,
                } => items.push(Item::ToolResult {
                    call_id,
                    output,
                    is_error,
                }),
                _ => {}
            }
        }

        let diff_path = trial_dir.join("workspace.diff");
        let workspace_diff = match std::fs::read_to_string(&diff_path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(source) => {
                return Err(ViewError::Io {
                    path: diff_path,
                    source,
                })
            }
        };

        Ok(Self {
            trial_dir: trial_dir.to_path_buf(),
            skill_project_root: skill_project_root.to_path_buf(),
            result,
            items,
            workspace_diff,
        })
    }

    pub fn result(&self) -> &TrialResult {
        &self.result
    }

    pub fn trial_dir(&self) -> &Path {
        &self.trial_dir
    }

    /// Passed or failed: a trial a judge may look at (R8).
    pub fn is_judgeable(&self) -> bool {
        matches!(self.result.status, CaseStatus::Passed | CaseStatus::Failed)
    }

    fn messages(&self) -> impl Iterator<Item = &Msg> {
        self.items.iter().filter_map(|i| match i {
            Item::Msg(m) => Some(m),
            _ => None,
        })
    }

    /// A trace written before messages carried `phase` cannot say which
    /// message is the final answer; guessing would score the wrong text.
    fn require_phase(&self, variable: &str) -> Result<(), VarError> {
        let mut any = false;
        for m in self.messages() {
            any = true;
            if m.phase.is_some() {
                return Ok(());
            }
        }
        if any {
            return Err(VarError::new(
                variable,
                "trace.jsonl messages carry no `phase`; this run predates aikit's phase-tagged traces (PR #170) and must be re-run",
            ));
        }
        Ok(())
    }

    /// The agent's final answer: the last `final`-phase message of kind
    /// `message`. Empty or absent renders as [`NO_FINAL_ANSWER`].
    pub fn final_answer(&self) -> Result<String, VarError> {
        self.require_phase("trial.final_answer")?;
        let last = self
            .messages()
            .filter(|m| m.phase.as_deref() == Some("final"))
            .filter(|m| matches!(m.kind.as_deref(), None | Some("message")))
            .filter(|m| m.role != "user")
            .last();
        Ok(match last {
            Some(m) if !m.text.trim().is_empty() => m.text.clone(),
            _ => NO_FINAL_ANSWER.to_string(),
        })
    }

    /// One JSON object per line: `{seq, tool_name, input, output, is_error}`,
    /// results paired to calls by `call_id`. Empty when the agent called
    /// nothing.
    pub fn tool_calls(&self) -> String {
        let mut lines = Vec::new();
        let mut seq = 0usize;
        for item in &self.items {
            if let Item::ToolUse {
                call_id,
                tool_name,
                input,
            } = item
            {
                seq += 1;
                let result = self.items.iter().find_map(|r| match r {
                    Item::ToolResult {
                        call_id: rid,
                        output,
                        is_error,
                    } if rid == call_id => Some((output, *is_error)),
                    _ => None,
                });
                let (output, is_error) = result
                    .map(|(o, e)| (o.clone(), e))
                    .unwrap_or((Value::Null, false));
                lines.push(
                    json!({
                        "seq": seq,
                        "tool_name": tool_name,
                        "input": input,
                        "output": output,
                        "is_error": is_error,
                    })
                    .to_string(),
                );
            }
        }
        lines.join("\n")
    }

    /// The exchange as `role: text` blocks separated by blank lines, delta
    /// frames dropped, tool calls and results inline in order.
    pub fn transcript(&self) -> Result<String, VarError> {
        self.require_phase("trial.transcript")?;
        let mut blocks = Vec::new();
        for item in &self.items {
            match item {
                Item::Msg(m) => {
                    if m.phase.as_deref() == Some("delta") {
                        continue;
                    }
                    let label = match m.kind.as_deref() {
                        Some("reasoning") => format!("{} (reasoning)", m.role),
                        Some("status") => format!("{} (status)", m.role),
                        Some("tool_output") => format!("{} (tool output)", m.role),
                        _ => m.role.clone(),
                    };
                    blocks.push(format!("{}: {}", label, m.text));
                }
                Item::ToolUse {
                    tool_name, input, ..
                } => blocks.push(format!("tool_use: {} {}", tool_name, input)),
                Item::ToolResult {
                    output, is_error, ..
                } => {
                    let label = if *is_error {
                        "tool_error"
                    } else {
                        "tool_result"
                    };
                    let text = match output {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    blocks.push(format!("{label}: {text}"));
                }
            }
        }
        Ok(blocks.join("\n\n"))
    }

    /// `workspace.diff`, or an error when the trial ran without an isolated
    /// workspace or predates the diff.
    pub fn workspace_diff(&self) -> Result<String, VarError> {
        self.workspace_diff.clone().ok_or_else(|| {
            VarError::new(
                "trial.workspace_diff",
                format!(
                    "{} has no workspace.diff: the trial ran without an isolated workspace, or the run predates the diff",
                    self.trial_dir.display()
                ),
            )
        })
    }

    /// The skill document the trial was run against: the staged path the
    /// trial recorded, else the run's skill project root.
    pub fn skill_body(&self) -> Result<String, VarError> {
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(p) = &self.result.skill_path {
            candidates.push(p.clone());
            candidates.push(p.join("SKILL.md"));
        }
        candidates.push(self.skill_project_root.join("SKILL.md"));
        candidates.push(self.skill_project_root.clone());
        for path in &candidates {
            if path.is_file() {
                return std::fs::read_to_string(path).map_err(|e| {
                    VarError::new("skill.body", format!("{}: {}", path.display(), e))
                });
            }
        }
        Err(VarError::new(
            "skill.body",
            format!(
                "no skill document found (looked at {})",
                candidates
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ))
    }

    /// Resolve a `case.*`, `trial.*` or `skill.*` variable. Judge-level
    /// variables (`rubric`, `output_contract`) are the caller's.
    pub fn variable(&self, name: &str, case: &EvalCase) -> Result<String, VarError> {
        match name {
            "case.prompt" => Ok(case.prompt.clone()),
            "trial.final_answer" => self.final_answer(),
            "trial.tool_calls" => Ok(self.tool_calls()),
            "trial.transcript" => self.transcript(),
            "trial.workspace_diff" => self.workspace_diff(),
            "skill.body" => self.skill_body(),
            other => {
                if let Some(column) = other.strip_prefix("case.") {
                    return case.extra.get(column).cloned().ok_or_else(|| {
                        VarError::new(
                            other,
                            format!(
                                "prompts.csv has no column '{column}' for case '{}'",
                                case.id
                            ),
                        )
                    });
                }
                Err(VarError::new(other, "unknown template variable"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifacts::TokenBreakdown;
    use tempfile::TempDir;

    fn result(skill_path: Option<PathBuf>) -> TrialResult {
        TrialResult {
            trial_id: 1,
            status: CaseStatus::Passed,
            command_count: None,
            input_tokens: None,
            output_tokens: None,
            check_results: vec![],
            error_message: None,
            exit_code: None,
            terminal: None,
            cost_usd: None,
            tokens: TokenBreakdown::default(),
            skill_path,
            judge_excluded: false,
        }
    }

    fn write_trial(
        dir: &Path,
        trace_lines: &[Value],
        diff: Option<&str>,
        skill_path: Option<PathBuf>,
    ) {
        std::fs::create_dir_all(dir).unwrap();
        let trace: Vec<String> = trace_lines
            .iter()
            .enumerate()
            .map(|(i, p)| json!({"seq": i, "payload": p}).to_string())
            .collect();
        std::fs::write(dir.join("trace.jsonl"), trace.join("\n") + "\n").unwrap();
        std::fs::write(
            dir.join("result.json"),
            serde_json::to_string(&result(skill_path)).unwrap(),
        )
        .unwrap();
        if let Some(d) = diff {
            std::fs::write(dir.join("workspace.diff"), d).unwrap();
        }
    }

    fn msg(role: &str, text: &str, kind: Option<&str>, phase: Option<&str>) -> Value {
        let mut v = json!({"type": "message", "role": role, "text": text});
        if let Some(k) = kind {
            v["kind"] = json!(k);
        }
        if let Some(p) = phase {
            v["phase"] = json!(p);
        }
        v
    }

    fn case() -> EvalCase {
        EvalCase {
            id: "c1".into(),
            prompt: "the prompt".into(),
            should_trigger: true,
            tags: vec![],
            workspace_subdir: None,
            extra: [("expected".to_string(), "42".to_string())]
                .into_iter()
                .collect(),
        }
    }

    fn full_trace() -> Vec<Value> {
        vec![
            msg("assistant", "thinking…", Some("reasoning"), Some("delta")),
            json!({"type": "tool_use", "call_id": "c1", "tool_name": "bash", "input": {"cmd": "ls"}}),
            json!({"type": "tool_result", "call_id": "c1", "output": "a.txt", "is_error": false}),
            msg(
                "assistant",
                "Here is the answer",
                Some("message"),
                Some("delta"),
            ),
            msg(
                "assistant",
                "Here is the answer",
                Some("message"),
                Some("final"),
            ),
            json!({"type": "token_usage_line", "usage": {}, "source": "x", "raw_agent_line_seq": 1}),
            json!({"type": "some_future_type", "whatever": 1}),
        ]
    }

    #[test]
    fn final_answer_is_the_final_phase_message_not_the_delta_duplicate() {
        let dir = TempDir::new().unwrap();
        write_trial(dir.path(), &full_trace(), None, None);
        let view = TrialView::load(dir.path(), dir.path()).unwrap();
        assert_eq!(view.final_answer().unwrap(), "Here is the answer");
        assert!(view.is_judgeable());
    }

    #[test]
    fn trace_without_phase_errors_naming_the_variable() {
        let dir = TempDir::new().unwrap();
        write_trial(
            dir.path(),
            &[msg("assistant", "old style", None, None)],
            None,
            None,
        );
        let view = TrialView::load(dir.path(), dir.path()).unwrap();
        let err = view.final_answer().unwrap_err();
        assert_eq!(err.variable, "trial.final_answer");
        assert!(err.to_string().contains("{{trial.final_answer}}"), "{err}");
        assert!(err.reason.contains("phase"), "{err}");
        assert_eq!(view.transcript().unwrap_err().variable, "trial.transcript");
    }

    #[test]
    fn missing_or_empty_final_answer_renders_the_literal() {
        let dir = TempDir::new().unwrap();
        write_trial(
            dir.path(),
            &[msg("assistant", "   ", Some("message"), Some("final"))],
            None,
            None,
        );
        let view = TrialView::load(dir.path(), dir.path()).unwrap();
        assert_eq!(view.final_answer().unwrap(), NO_FINAL_ANSWER);
        let dir = TempDir::new().unwrap();
        write_trial(dir.path(), &[], None, None);
        let view = TrialView::load(dir.path(), dir.path()).unwrap();
        assert_eq!(view.final_answer().unwrap(), NO_FINAL_ANSWER);
    }

    #[test]
    fn tool_calls_pair_results_by_call_id_one_json_per_line() {
        let dir = TempDir::new().unwrap();
        let mut trace = full_trace();
        trace.push(
            json!({"type": "tool_use", "call_id": "c2", "tool_name": "read", "input": {"p": "x"}}),
        );
        write_trial(dir.path(), &trace, None, None);
        let view = TrialView::load(dir.path(), dir.path()).unwrap();
        let text = view.tool_calls();
        let lines: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["seq"], json!(1));
        assert_eq!(lines[0]["tool_name"], json!("bash"));
        assert_eq!(lines[0]["output"], json!("a.txt"));
        assert_eq!(lines[0]["is_error"], json!(false));
        assert_eq!(lines[1]["seq"], json!(2));
        assert_eq!(lines[1]["output"], Value::Null);
    }

    #[test]
    fn transcript_drops_deltas_and_labels_reasoning_and_tools() {
        let dir = TempDir::new().unwrap();
        write_trial(dir.path(), &full_trace(), None, None);
        let view = TrialView::load(dir.path(), dir.path()).unwrap();
        let text = view.transcript().unwrap();
        assert_eq!(
            text,
            "tool_use: bash {\"cmd\":\"ls\"}\n\ntool_result: a.txt\n\nassistant: Here is the answer"
        );
    }

    #[test]
    fn workspace_diff_is_the_file_or_an_error() {
        let dir = TempDir::new().unwrap();
        write_trial(dir.path(), &[], Some("--- a\n+++ b\n"), None);
        let view = TrialView::load(dir.path(), dir.path()).unwrap();
        assert_eq!(view.workspace_diff().unwrap(), "--- a\n+++ b\n");
        let dir = TempDir::new().unwrap();
        write_trial(dir.path(), &[], None, None);
        let view = TrialView::load(dir.path(), dir.path()).unwrap();
        assert_eq!(
            view.workspace_diff().unwrap_err().variable,
            "trial.workspace_diff"
        );
    }

    #[test]
    fn skill_body_prefers_the_staged_path_then_the_project_root() {
        let dir = TempDir::new().unwrap();
        let staged = dir.path().join("staged");
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::write(staged.join("SKILL.md"), "staged body").unwrap();
        let root = dir.path().join("root");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("SKILL.md"), "root body").unwrap();
        let trial = dir.path().join("trial-1");
        write_trial(&trial, &[], None, Some(staged.clone()));
        let view = TrialView::load(&trial, &root).unwrap();
        assert_eq!(view.skill_body().unwrap(), "staged body");

        let trial2 = dir.path().join("trial-2");
        write_trial(&trial2, &[], None, None);
        let view = TrialView::load(&trial2, &root).unwrap();
        assert_eq!(view.skill_body().unwrap(), "root body");

        let view = TrialView::load(&trial2, &dir.path().join("nowhere")).unwrap();
        assert_eq!(view.skill_body().unwrap_err().variable, "skill.body");
    }

    #[test]
    fn variables_resolve_case_columns_and_reject_unknown() {
        let dir = TempDir::new().unwrap();
        write_trial(dir.path(), &full_trace(), None, None);
        let view = TrialView::load(dir.path(), dir.path()).unwrap();
        let c = case();
        assert_eq!(view.variable("case.prompt", &c).unwrap(), "the prompt");
        assert_eq!(view.variable("case.expected", &c).unwrap(), "42");
        let err = view.variable("case.missing", &c).unwrap_err();
        assert!(err.reason.contains("no column 'missing'"), "{err}");
        assert!(view.variable("rubric", &c).is_err());
    }

    #[test]
    fn malformed_trace_line_is_a_load_error() {
        let dir = TempDir::new().unwrap();
        write_trial(dir.path(), &[], None, None);
        std::fs::write(
            dir.path().join("trace.jsonl"),
            "{\"seq\":0,\"payload\":{\"type\":\"message\"}}\n",
        )
        .unwrap();
        assert!(matches!(
            TrialView::load(dir.path(), dir.path()),
            Err(ViewError::Trace { line: 1, .. })
        ));
        std::fs::write(dir.path().join("trace.jsonl"), "not json\n").unwrap();
        assert!(matches!(
            TrialView::load(dir.path(), dir.path()),
            Err(ViewError::Trace { line: 1, .. })
        ));
    }

    #[test]
    fn missing_trace_is_a_load_error() {
        let dir = TempDir::new().unwrap();
        write_trial(dir.path(), &[], None, None);
        std::fs::remove_file(dir.path().join("trace.jsonl")).unwrap();
        assert!(matches!(
            TrialView::load(dir.path(), dir.path()),
            Err(ViewError::Io { .. })
        ));
    }
}
