//! Deterministic check engine for eval artifact scoring

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

use crate::trace::{TraceEvent, TracePayload};

/// A deterministic check definition loaded from checks.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "name")]
pub enum CheckDefinition {
    /// Legacy substring check over canonical trace JSONL.
    ///
    /// Prefer `skill_invoked` when asserting that a skill ran. This check ignores
    /// raw stdout so agent startup capability listings do not satisfy it.
    #[serde(rename = "trigger_expectation")]
    TriggerExpectation {
        pattern: String,
        /// true = pattern must appear; false = pattern must NOT appear
        expected: bool,
        #[serde(default = "default_required")]
        required: bool,
        /// Case ids this check applies to. Absent (the default) means every
        /// case in the suite, so files written before the selector existed
        /// parse and behave unchanged.
        #[serde(default)]
        cases: Option<Vec<String>>,
    },
    /// Check whether canonical trace JSONL contains this pattern.
    ///
    /// Assistant text is represented in trace messages, so answer-text matches
    /// remain available without consulting raw stdout.
    #[serde(rename = "command_contains")]
    CommandContains {
        pattern: String,
        #[serde(default = "default_required")]
        required: bool,
        /// Case ids this check applies to. Absent (the default) means every
        /// case in the suite, so files written before the selector existed
        /// parse and behave unchanged.
        #[serde(default)]
        cases: Option<Vec<String>>,
    },
    /// Check whether the trace shows the agent consulting the skill.
    ///
    /// Two shapes count, because only one backend has a typed skill tool:
    ///
    /// 1. A tool use named `Skill` (Claude Code). When `skill` is present, an
    ///    identifying field of the input (`skill`, `name`, or `skillName`)
    ///    must equal that name exactly — a substring match over the serialized
    ///    input would hit JSON keys, argument text and longer skill names.
    /// 2. **Any** tool use whose input references the skill document's path.
    ///    On pi a skill read arrives as `read_file` with the path in its
    ///    arguments; on codex every call is `shell` or `file_change`. Keying
    ///    on the tool *name* therefore measured Claude Code and nothing else.
    ///
    /// The path comes from where the runner staged the skill, so no
    /// configuration is needed; `path` overrides it.
    ///
    /// `expected = false` asserts no such invocation occurred.
    ///
    /// Reads the trace only. Nothing derived from the agent's *environment*
    /// may feed this verdict: the capability listing an agent prints at
    /// startup produced false passes once already (spec 015).
    #[serde(rename = "skill_invoked")]
    SkillInvoked {
        skill: Option<String>,
        /// Path of the skill document, matched against the *input* of any
        /// tool use. Overrides the path the runner supplies from the staged
        /// skill location. See [`skill_invoked`] for why a path is needed.
        #[serde(default)]
        path: Option<String>,
        #[serde(default = "default_expected")]
        expected: bool,
        #[serde(default = "default_required")]
        required: bool,
        /// Case ids this check applies to. Absent (the default) means every
        /// case in the suite, so files written before the selector existed
        /// parse and behave unchanged.
        #[serde(default)]
        cases: Option<Vec<String>>,
    },
    /// Check whether a file exists in the working directory after execution
    #[serde(rename = "file_exists")]
    FileExists {
        path: PathBuf,
        #[serde(default = "default_required")]
        required: bool,
        /// Case ids this check applies to. Absent (the default) means every
        /// case in the suite, so files written before the selector existed
        /// parse and behave unchanged.
        #[serde(default)]
        cases: Option<Vec<String>>,
    },
    /// Check that the number of decoded tool calls does not exceed a limit
    #[serde(rename = "max_tool_calls", alias = "max_command_count")]
    MaxToolCalls {
        limit: usize,
        #[serde(default = "default_required")]
        required: bool,
        /// Case ids this check applies to. Absent (the default) means every
        /// case in the suite, so files written before the selector existed
        /// parse and behave unchanged.
        #[serde(default)]
        cases: Option<Vec<String>>,
    },
}

fn default_required() -> bool {
    true
}

fn default_expected() -> bool {
    true
}

impl CheckDefinition {
    pub fn name(&self) -> &str {
        match self {
            CheckDefinition::TriggerExpectation { .. } => "trigger_expectation",
            CheckDefinition::CommandContains { .. } => "command_contains",
            CheckDefinition::SkillInvoked { .. } => "skill_invoked",
            CheckDefinition::FileExists { .. } => "file_exists",
            CheckDefinition::MaxToolCalls { .. } => "max_tool_calls",
        }
    }

    pub fn is_required(&self) -> bool {
        match self {
            CheckDefinition::TriggerExpectation { required, .. } => *required,
            CheckDefinition::CommandContains { required, .. } => *required,
            CheckDefinition::SkillInvoked { required, .. } => *required,
            CheckDefinition::FileExists { required, .. } => *required,
            CheckDefinition::MaxToolCalls { required, .. } => *required,
        }
    }

    /// Case ids this check is scoped to, or `None` for "every case".
    pub fn cases(&self) -> Option<&[String]> {
        let cases = match self {
            CheckDefinition::TriggerExpectation { cases, .. } => cases,
            CheckDefinition::CommandContains { cases, .. } => cases,
            CheckDefinition::SkillInvoked { cases, .. } => cases,
            CheckDefinition::FileExists { cases, .. } => cases,
            CheckDefinition::MaxToolCalls { cases, .. } => cases,
        };
        cases.as_deref()
    }

    /// Does this check run for `case_id`?
    ///
    /// A check with no `cases` list applies to every case, which is how a
    /// suite written before the selector existed keeps behaving.
    pub fn applies_to(&self, case_id: &str) -> bool {
        match self.cases() {
            None => true,
            Some(ids) => ids.iter().any(|id| id == case_id),
        }
    }

    /// Whether this check's evidence can exist at all on a backend with the
    /// given decoding ability.
    ///
    /// `skill_invoked` and `max_tool_calls` both read decoded tool frames. On a
    /// backend whose decoder emits none, `skill_invoked` can only ever fail and
    /// `max_tool_calls` can only ever pass — neither is a measurement. Saying
    /// so is the whole point: a vacuous pass is indistinguishable from a real
    /// one once it reaches a report.
    pub fn observability(&self, ctx: &CheckContext<'_>) -> Option<NotObservable> {
        match self {
            CheckDefinition::SkillInvoked { .. } if !ctx.structured_tools => Some(NotObservable {
                reason: format!(
                    "backend '{}' decodes no tool-use frames, so skill invocation \
                         cannot be observed",
                    ctx.backend
                ),
            }),
            CheckDefinition::SkillInvoked { skill, path, .. }
                if !ctx.typed_skill_tool
                    && skill.is_none()
                    && path.is_none()
                    && ctx.skill_path.is_none() =>
            {
                Some(NotObservable {
                    reason: format!(
                        "backend '{}' has no `Skill` tool and no document path was given, \
                         so there is nothing to match; set `skill` or `path` on the check, \
                         or run the case with skill isolation",
                        ctx.backend
                    ),
                })
            }
            CheckDefinition::MaxToolCalls { .. } if !ctx.structured_tools => Some(NotObservable {
                reason: format!(
                    "backend '{}' decodes no tool-use frames, so the tool count is \
                         always zero and the limit cannot be exceeded",
                    ctx.backend
                ),
            }),
            _ => None,
        }
    }
}

/// What the checks engine knows about the run it is scoring.
///
/// Constructed by the runner, which is the only thing that knows which backend
/// produced the trace and where the skill was staged.
#[derive(Debug, Clone)]
pub struct CheckContext<'a> {
    /// Agent key, used in not-observable messages.
    pub backend: &'a str,
    /// Does this backend's decoder emit `tool_use` frames?
    pub structured_tools: bool,
    /// Does this backend expose a tool literally named `Skill`?
    ///
    /// Only Claude Code does. Elsewhere a skill read arrives as `read_file`,
    /// `shell` or `file_change`, so the only way to recognise it is the
    /// document path — and with no path, `skill_invoked` has nothing to
    /// match on.
    pub typed_skill_tool: bool,
    /// Where the skill document was staged for this run, for path matching.
    pub skill_path: Option<&'a str>,
}

impl Default for CheckContext<'_> {
    /// Lenient: an unknown backend is assumed to decode tool frames.
    ///
    /// `structured_tools: false` is a positive claim that the decoder cannot
    /// produce the evidence, and it un-scores every tool-dependent check
    /// (and, under R10, refuses the suite outright). "We were not told which
    /// backend produced this trace" is not that claim. The runner, which does
    /// know, passes the real capability; only callers with no backend at hand
    /// — [`run_checks`], [`ChecksScorer`](crate::scoring::ChecksScorer) —
    /// land here.
    fn default() -> Self {
        Self {
            backend: "unknown",
            structured_tools: true,
            typed_skill_tool: true,
            skill_path: None,
        }
    }
}

/// A check whose evidence the backend cannot produce.
///
/// Not a pass and not a fail. Both would be lies: the check asserts nothing
/// about the agent, only about the decoder. [`suite_passes`] skips these, and
/// callers refuse a suite up front when a *required* check lands here
/// (paying for trials that cannot be scored is waste).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotObservable {
    pub reason: String,
}

/// Result of a single check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    /// The check's TYPE (e.g. `"trigger_expectation"`), not a unique id: a
    /// suite with two same-typed checks yields two results with the same
    /// `check_name`. Result vectors are ordered like the check list they ran,
    /// so identity is positional — keying results by name collapses same-typed
    /// checks (see `scoring::score_cases`).
    pub check_name: String,
    /// Only meaningful when `not_observable` is `None`. A check that could not
    /// be observed reports `false` here for older readers and must not be
    /// counted as a failure by anything that understands the field.
    pub passed: bool,
    #[serde(default = "default_required")]
    pub required: bool,
    pub message: Option<String>,
    /// Set when the backend cannot produce this check's evidence. Absent means
    /// the check ran; it never means "observable" was not checked.
    #[serde(default)]
    pub not_observable: Option<NotObservable>,
    /// A judge's `overall` on a `judge:<name>` row (spec eval-judge R9).
    /// `None` on every deterministic check, and on a gated judge row whose
    /// judgment errored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

impl CheckResult {
    /// Did this check run at all?
    pub fn is_observable(&self) -> bool {
        self.not_observable.is_none()
    }
}

/// TOML file structure for checks configuration
#[derive(Debug, Deserialize)]
pub struct ChecksToml {
    #[serde(rename = "check", default)]
    pub checks: Vec<CheckDefinition>,
    /// `[[judge]]` tables (spec eval-judge R5). Unknown keys inside one are a
    /// parse error, so a judge cannot be quietly ignored by a misspelling.
    #[serde(rename = "judge", default)]
    pub judges: Vec<crate::judge::JudgeDefinition>,
    /// `[judge_defaults]` (spec eval-judge R3).
    #[serde(default)]
    pub judge_defaults: Option<crate::judge::JudgeDefaults>,
}

/// Errors loading checks configuration
#[derive(Debug, Error)]
pub enum ChecksError {
    #[error("EVAL_CHECKS_INVALID: Failed to read checks file: {0}")]
    Io(#[from] std::io::Error),
    #[error("EVAL_CHECKS_INVALID: Failed to parse checks TOML: {0}")]
    Parse(#[from] toml::de::Error),
}

/// Load check definitions from a TOML file
/// Load the whole checks file: deterministic checks and judge declarations.
pub fn load_checks_file(path: &std::path::Path) -> Result<ChecksToml, ChecksError> {
    let content = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&content)?)
}

pub fn load_checks(path: &std::path::Path) -> Result<Vec<CheckDefinition>, ChecksError> {
    let content = std::fs::read_to_string(path)?;
    let parsed: ChecksToml = toml::from_str(&content)?;
    Ok(parsed.checks)
}

/// Run all checks against the canonical trace JSONL and working directory.
///
/// `stdout_content` is accepted for API stability but **no check type consults it**, and that
/// is deliberate — not an oversight. Raw stdout for the `claude` backend opens with a
/// `system`/`init` event enumerating every skill installed in the environment, so any
/// substring check against stdout passes on a skill's own name whether or not the skill ever
/// ran. Scoring reads the parsed trace, where that capability listing never appears.
///
/// **Do not wire `stdout_content` back into a check** without re-reading that rationale;
/// `test_trigger_expectation_ignores_stdout_capability_listing` exists to catch exactly that
/// regression.
pub fn run_checks(
    checks: &[CheckDefinition],
    _stdout_content: &str,
    trace_jsonl: &str,
    working_dir: &std::path::Path,
) -> Vec<CheckResult> {
    run_checks_in_context(checks, trace_jsonl, working_dir, &CheckContext::default())
}

/// Run checks knowing which backend produced the trace and where the skill
/// lives.
///
/// The plain [`run_checks`] defaults every context field, which makes
/// `skill_invoked` fall back to matching the typed `Skill` tool only. Callers
/// that know the backend should use this, so an unobservable check says so
/// instead of passing or failing vacuously.
///
/// Case selection is **not** applied here: the caller filters the list with
/// [`CheckDefinition::applies_to`] first, because a result vector's identity is
/// positional and dropping entries mid-run would misalign it.
pub fn run_checks_in_context(
    checks: &[CheckDefinition],
    trace_jsonl: &str,
    working_dir: &std::path::Path,
    ctx: &CheckContext<'_>,
) -> Vec<CheckResult> {
    checks
        .iter()
        .map(|check| match check.observability(ctx) {
            Some(not_observable) => CheckResult {
                check_name: check.name().to_string(),
                passed: false,
                required: check.is_required(),
                message: Some(format!("not observable: {}", not_observable.reason)),
                not_observable: Some(not_observable),
                score: None,
            },
            None => run_single_check(check, trace_jsonl, working_dir, ctx),
        })
        .collect()
}

/// The checks that actually run for one case: those scoped to it, plus the
/// skill-invocation check implied by the case's `should_trigger` column.
///
/// `should_trigger` used to be parsed and read by nothing, so a case marked
/// `false` asserted nothing at all while every reader of the CSV assumed
/// otherwise. It now generates a check with matching polarity.
///
/// The generated check is structural (`skill_invoked`), not a text expectation:
/// a text expectation would need a pattern nobody supplies.
pub fn effective_checks(
    checks: &[CheckDefinition],
    case_id: &str,
    should_trigger: bool,
) -> Vec<CheckDefinition> {
    let mut out: Vec<CheckDefinition> = checks
        .iter()
        .filter(|c| c.applies_to(case_id))
        .cloned()
        .collect();
    // An explicit skill_invoked for this case wins: the operator said something
    // more specific than the column. `validate_case_checks` rejects the case
    // when the two disagree, so reaching here means they agree.
    let has_explicit = out
        .iter()
        .any(|c| matches!(c, CheckDefinition::SkillInvoked { .. }));
    if !has_explicit {
        out.push(CheckDefinition::SkillInvoked {
            skill: None,
            path: None,
            expected: should_trigger,
            required: true,
            cases: Some(vec![case_id.to_string()]),
        });
    }
    out
}

/// Reject a case whose explicit checks contradict its `should_trigger` column.
///
/// Letting one silently win would put an explicit input back in the position
/// `should_trigger` was already in: typed by a human, obeyed by nothing.
pub fn validate_case_checks(
    checks: &[CheckDefinition],
    case_id: &str,
    should_trigger: bool,
) -> Result<(), String> {
    for check in checks.iter().filter(|c| c.applies_to(case_id)) {
        if let CheckDefinition::SkillInvoked { expected, .. } = check {
            if *expected != should_trigger {
                return Err(format!(
                    "EVAL_CHECKS_INVALID: case '{case_id}' sets should_trigger={should_trigger} \
                     but a skill_invoked check on it expects {expected}"
                ));
            }
        }
    }
    Ok(())
}

/// Count trace events with payload type `raw_json`.
pub fn count_raw_json_events(trace_jsonl: &str) -> usize {
    count_matching(trace_jsonl, |payload| {
        matches!(payload, TracePayload::RawJson { .. })
    })
}

/// Count the tool invocations an agent issued during a run.
///
/// This intentionally preserves the Phase 0 artifact field name
/// `command_count`: the counter is a structured tool invocation (`tool_use`),
/// plus any `raw_json` line for backends that still emit tool calls as raw JSON
/// rather than decoded `ToolUse` events. Text output, token-usage events and
/// unmodelled SDK event variants are explicitly not counted.
pub fn count_command_events(trace_jsonl: &str) -> usize {
    count_matching(trace_jsonl, |payload| {
        matches!(
            payload,
            TracePayload::ToolUse { .. } | TracePayload::RawJson { .. }
        )
    })
}

fn count_matching(trace_jsonl: &str, predicate: impl Fn(&TracePayload) -> bool) -> usize {
    trace_jsonl
        .lines()
        .filter_map(|line| serde_json::from_str::<TraceEvent>(line).ok())
        .filter(|event| predicate(&event.payload))
        .count()
}

/// Haystack for substring pattern checks (`trigger_expectation`,
/// `command_contains`): the trace JSONL with `Unknown` payloads' `raw` field
/// blanked out.
///
/// `Unknown.raw` preserves the full raw payload text of unmodelled SDK
/// variants, so if `emit_raw_transport` were enabled a raw `system`/`init`
/// line enumerating every installed skill would land there verbatim and
/// re-open the stdout/init false-pass that spec 015 closed by keeping checks
/// off raw stdout. Blanking only that one field is the narrower change: every
/// modelled payload line still matches byte-identically, and the `Unknown`
/// event itself (with its `payload_type`) stays visible in the haystack.
fn pattern_haystack(trace_jsonl: &str) -> String {
    trace_jsonl
        .lines()
        .map(|line| match serde_json::from_str::<TraceEvent>(line) {
            Ok(mut event) => {
                if let TracePayload::Unknown { raw, .. } = &mut event.payload {
                    raw.clear();
                    serde_json::to_string(&event).unwrap_or_default()
                } else {
                    line.to_string()
                }
            }
            Err(_) => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn run_single_check(
    check: &CheckDefinition,
    trace_jsonl: &str,
    working_dir: &std::path::Path,
    ctx: &CheckContext<'_>,
) -> CheckResult {
    let required = check.is_required();
    match check {
        CheckDefinition::TriggerExpectation {
            pattern, expected, ..
        } => {
            let found = pattern_haystack(trace_jsonl).contains(pattern.as_str());
            let passed = found == *expected;
            let message = if passed {
                None
            } else if *expected {
                Some(format!("Pattern '{}' not found but was expected", pattern))
            } else {
                Some(format!("Pattern '{}' found but was not expected", pattern))
            };
            CheckResult {
                check_name: "trigger_expectation".to_string(),
                passed,
                required,
                message,
                not_observable: None,
                score: None,
            }
        }
        CheckDefinition::CommandContains { pattern, .. } => {
            let passed = pattern_haystack(trace_jsonl).contains(pattern.as_str());
            let message = if passed {
                None
            } else {
                Some(format!("Pattern '{}' not found in trace", pattern))
            };
            CheckResult {
                check_name: "command_contains".to_string(),
                passed,
                required,
                message,
                not_observable: None,
                score: None,
            }
        }
        CheckDefinition::SkillInvoked {
            skill,
            path,
            expected,
            ..
        } => {
            let path = path.as_deref().or(ctx.skill_path);
            let found = skill_invoked(trace_jsonl, skill.as_deref(), path);
            let passed = found == *expected;
            let target = skill.as_deref().or(path).unwrap_or("any skill");
            let message = if passed {
                None
            } else if *expected {
                Some(format!("Skill invocation '{}' not found", target))
            } else {
                Some(format!(
                    "Skill invocation '{}' found but was not expected",
                    target
                ))
            };
            CheckResult {
                check_name: "skill_invoked".to_string(),
                passed,
                required,
                message,
                not_observable: None,
                score: None,
            }
        }
        CheckDefinition::FileExists { path, .. } => {
            let full_path = working_dir.join(path);
            let passed = full_path.exists();
            let message = if passed {
                None
            } else {
                Some(format!("File '{}' does not exist", path.display()))
            };
            CheckResult {
                check_name: "file_exists".to_string(),
                passed,
                required,
                message,
                not_observable: None,
                score: None,
            }
        }
        CheckDefinition::MaxToolCalls { limit, .. } => {
            let count = count_command_events(trace_jsonl);
            let passed = count <= *limit;
            let message = if passed {
                None
            } else {
                Some(format!("Command count {} exceeds limit {}", count, limit))
            };
            CheckResult {
                check_name: "max_tool_calls".to_string(),
                passed,
                required,
                message,
                not_observable: None,
                score: None,
            }
        }
    }
}

/// Did the agent consult the skill?
///
/// Matches either a typed `Skill` tool use (Claude Code only) or any tool use
/// whose input references `skill_path`. The path arm is what makes the check
/// mean anything on pi, where a skill read arrives as `read_file` with a path
/// argument, or codex, where every call is `shell` or `file_change`.
fn skill_invoked(trace_jsonl: &str, skill: Option<&str>, skill_path: Option<&str>) -> bool {
    trace_jsonl
        .lines()
        .filter_map(|line| serde_json::from_str::<TraceEvent>(line).ok())
        .any(|event| match event.payload {
            TracePayload::ToolUse {
                tool_name, input, ..
            } => {
                if tool_name == "Skill" {
                    let named = match skill {
                        // Compare the identifying field exactly: a substring
                        // match over the serialized input would false-match
                        // "foo" against "foo-bar" and match JSON keys or
                        // argument text.
                        Some(skill) => ["skill", "name", "skillName"]
                            .iter()
                            .any(|f| input.get(f).and_then(|v| v.as_str()) == Some(skill)),
                        None => true,
                    };
                    if named {
                        return true;
                    }
                }
                // Path arm: the skill document's location, anywhere in the
                // tool's input. Serializing the input is safe here in a way it
                // is not for a *name* match — a filesystem path is specific
                // enough that an incidental hit is not a realistic worry, while
                // a bare skill name is not.
                match skill_path {
                    Some(p) if !p.is_empty() => input.to_string().contains(p),
                    _ => false,
                }
            }
            _ => false,
        })
}

/// Aggregate check results: suite passes if all required, observable checks pass.
///
/// A not-observable check contributes nothing. It cannot fail the suite (the
/// backend, not the agent, is what failed to produce evidence) and it cannot
/// pass it either — the caller is expected to have refused the
/// suite-and-backend combination before spending tokens on it.
pub fn suite_passes(results: &[CheckResult]) -> bool {
    results
        .iter()
        .filter(|r| r.required && r.is_observable())
        .all(|r| r.passed)
}

/// Required checks that cannot be observed on this backend.
///
/// `eval validate` and `eval run` use this to refuse a suite before spending a
/// single token, which is the loud failure. Returning the reasons rather than a
/// bool keeps the refusal message specific.
pub fn unobservable_required(
    checks: &[CheckDefinition],
    ctx: &CheckContext<'_>,
) -> Vec<(String, NotObservable)> {
    checks
        .iter()
        .filter(|c| c.is_required())
        .filter_map(|c| c.observability(ctx).map(|n| (c.name().to_string(), n)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn test_trigger_expectation_passes_when_pattern_found() {
        let check = CheckDefinition::TriggerExpectation {
            pattern: "fastskill".to_string(),
            expected: true,
            required: true,
            cases: None,
        };
        let trace = r#"{"seq":0,"payload":{"type":"message","text":"fastskill triggered","role":"assistant"}}"#;
        let results = run_checks(&[check], "", trace, Path::new("/tmp"));
        assert!(results[0].passed);
    }

    #[test]
    fn test_trigger_expectation_fails_when_pattern_missing() {
        let check = CheckDefinition::TriggerExpectation {
            pattern: "fastskill".to_string(),
            expected: true,
            required: true,
            cases: None,
        };
        let results = run_checks(&[check], "nothing here", "", Path::new("/tmp"));
        assert!(!results[0].passed);
    }

    #[test]
    fn test_trigger_expectation_negative_passes_when_pattern_absent() {
        let check = CheckDefinition::TriggerExpectation {
            pattern: "fastskill".to_string(),
            expected: false,
            required: true,
            cases: None,
        };
        let results = run_checks(&[check], "no match", "", Path::new("/tmp"));
        assert!(results[0].passed);
    }

    #[test]
    fn test_trigger_expectation_negative_fails_when_pattern_found() {
        let check = CheckDefinition::TriggerExpectation {
            pattern: "fastskill".to_string(),
            expected: false,
            required: true,
            cases: None,
        };
        let trace = r#"{"seq":0,"payload":{"type":"message","text":"fastskill triggered","role":"assistant"}}"#;
        let results = run_checks(&[check], "", trace, Path::new("/tmp"));
        assert!(!results[0].passed);
        assert_eq!(
            results[0].message.as_deref(),
            Some("Pattern 'fastskill' found but was not expected")
        );
    }

    #[test]
    fn test_trigger_expectation_ignores_stdout_capability_listing() {
        let stdout = include_str!("../../aikit-sdk/tests/fixtures/recorded_case01/claude.jsonl");
        assert!(
            stdout.contains("\"fastskill\""),
            "fixture must contain the false-positive skill listing"
        );
        let trace = r#"{"seq":0,"payload":{"type":"message","text":"ok.","role":"assistant"}}"#;
        let check = CheckDefinition::TriggerExpectation {
            pattern: "fastskill".to_string(),
            expected: true,
            required: true,
            cases: None,
        };

        let results = run_checks(&[check], stdout, trace, Path::new("/tmp"));

        assert!(
            !results[0].passed,
            "stdout-only skill listings must not satisfy trigger_expectation"
        );
        assert_eq!(
            results[0].message.as_deref(),
            Some("Pattern 'fastskill' not found but was expected")
        );
    }

    #[test]
    fn test_trigger_expectation_negative_ignores_stdout_capability_listing() {
        let stdout = include_str!("../../aikit-sdk/tests/fixtures/recorded_case01/claude.jsonl");
        let trace = r#"{"seq":0,"payload":{"type":"message","text":"ok.","role":"assistant"}}"#;
        let check = CheckDefinition::TriggerExpectation {
            pattern: "fastskill".to_string(),
            expected: false,
            required: true,
            cases: None,
        };

        let results = run_checks(&[check], stdout, trace, Path::new("/tmp"));

        assert!(
            results[0].passed,
            "expected=false should pass when the trace has no matching trigger"
        );
    }

    #[test]
    fn test_trigger_expectation_ignores_unknown_payload_raw_text() {
        use crate::trace::trace_to_jsonl;
        // Regression guard: `Unknown.raw` embeds the full raw payload text, so
        // with emit_raw_transport enabled a raw `system`/`init` line listing
        // every installed skill would land there verbatim. Pattern checks must
        // not match against it, or the stdout/init false-pass returns.
        let events = vec![TraceEvent {
            seq: 0,
            payload: TracePayload::Unknown {
                payload_type: "raw_transport_line".to_string(),
                raw: r#"RawTransportLine { raw: "{\"type\":\"system\",\"subtype\":\"init\",\"skills\":[\"fastskill\"]}" }"#.to_string(),
            },
        }];
        let trace = trace_to_jsonl(&events);
        assert!(
            trace.contains("fastskill"),
            "trace text must contain the skill name for this guard to be meaningful"
        );

        let check = CheckDefinition::TriggerExpectation {
            pattern: "fastskill".to_string(),
            expected: true,
            required: true,
            cases: None,
        };
        let results = run_checks(&[check], "", &trace, Path::new("/tmp"));

        assert!(
            !results[0].passed,
            "a raw init line inside Unknown.raw must not satisfy trigger_expectation"
        );
    }

    #[test]
    fn test_command_contains_ignores_unknown_payload_raw_text() {
        use crate::trace::trace_to_jsonl;
        let events = vec![TraceEvent {
            seq: 0,
            payload: TracePayload::Unknown {
                payload_type: "raw_transport_line".to_string(),
                raw: "raw text mentioning fastskill".to_string(),
            },
        }];
        let trace = trace_to_jsonl(&events);

        let check = CheckDefinition::CommandContains {
            pattern: "fastskill".to_string(),
            required: true,
            cases: None,
        };
        let results = run_checks(&[check], "", &trace, Path::new("/tmp"));

        assert!(
            !results[0].passed,
            "Unknown.raw content must not satisfy command_contains"
        );
    }

    #[test]
    fn test_command_contains_uses_trace_not_stdout() {
        let check = CheckDefinition::CommandContains {
            pattern: "trace-only".to_string(),
            required: true,
            cases: None,
        };

        let stdout_only = run_checks(
            std::slice::from_ref(&check),
            "trace-only",
            r#"{"seq":0,"payload":{"type":"message","text":"different","role":"assistant"}}"#,
            Path::new("/tmp"),
        );
        let trace_match = run_checks(
            &[check],
            "",
            r#"{"seq":0,"payload":{"type":"message","text":"trace-only","role":"assistant"}}"#,
            Path::new("/tmp"),
        );

        assert!(!stdout_only[0].passed);
        assert!(trace_match[0].passed);
    }

    #[test]
    fn test_check_definition_names_and_required_flags() {
        let checks = [
            CheckDefinition::TriggerExpectation {
                pattern: "a".to_string(),
                expected: true,
                required: false,
                cases: None,
            },
            CheckDefinition::CommandContains {
                pattern: "b".to_string(),
                required: true,
                cases: None,
            },
            CheckDefinition::SkillInvoked {
                skill: Some("d".to_string()),
                expected: true,
                required: false,
                cases: None,
                path: None,
            },
            CheckDefinition::FileExists {
                path: PathBuf::from("c"),
                required: false,
                cases: None,
            },
            CheckDefinition::MaxToolCalls {
                limit: 1,
                required: true,
                cases: None,
            },
        ];

        assert_eq!(checks[0].name(), "trigger_expectation");
        assert!(!checks[0].is_required());
        assert_eq!(checks[1].name(), "command_contains");
        assert!(checks[1].is_required());
        assert_eq!(checks[2].name(), "skill_invoked");
        assert!(!checks[2].is_required());
        assert_eq!(checks[3].name(), "file_exists");
        assert!(!checks[3].is_required());
        assert_eq!(checks[4].name(), "max_tool_calls");
        assert!(checks[4].is_required());
    }

    #[test]
    fn test_skill_invoked_matches_any_skill_tool_use() {
        let check = CheckDefinition::SkillInvoked {
            skill: None,
            expected: true,
            required: true,
            cases: None,
            path: None,
        };
        let trace = r#"{"seq":0,"payload":{"type":"tool_use","call_id":"call_1","tool_name":"Skill","input":{"skill":"greeting-helper"}}}"#;

        let results = run_checks(&[check], "", trace, Path::new("/tmp"));

        assert!(results[0].passed);
        assert_eq!(results[0].check_name, "skill_invoked");
    }

    #[test]
    fn test_skill_invoked_matches_named_skill_input() {
        let check = CheckDefinition::SkillInvoked {
            skill: Some("greeting-helper".to_string()),
            expected: true,
            required: true,
            cases: None,
            path: None,
        };
        let trace = r#"{"seq":0,"payload":{"type":"tool_use","call_id":"call_1","tool_name":"Skill","input":{"name":"greeting-helper"}}}"#;

        let results = run_checks(&[check], "", trace, Path::new("/tmp"));

        assert!(results[0].passed);
    }

    #[test]
    fn test_skill_invoked_rejects_other_tools_and_stdout_listing() {
        let stdout = include_str!("../../aikit-sdk/tests/fixtures/recorded_case01/claude.jsonl");
        let check = CheckDefinition::SkillInvoked {
            skill: Some("fastskill".to_string()),
            expected: true,
            required: true,
            cases: None,
            path: None,
        };
        let bash_trace = r#"{"seq":0,"payload":{"type":"tool_use","call_id":"call_1","tool_name":"Bash","input":{"command":"fastskill"}}}"#;
        let no_tool_trace =
            r#"{"seq":0,"payload":{"type":"message","text":"fastskill","role":"assistant"}}"#;

        let bash_results = run_checks(
            std::slice::from_ref(&check),
            "",
            bash_trace,
            Path::new("/tmp"),
        );
        let no_tool_results = run_checks(&[check], stdout, no_tool_trace, Path::new("/tmp"));

        assert!(!bash_results[0].passed);
        assert!(
            !no_tool_results[0].passed,
            "stdout skill listings and assistant prose are not Skill tool invocations"
        );
    }

    #[test]
    fn test_skill_invoked_requires_exact_name_not_substring() {
        let check = CheckDefinition::SkillInvoked {
            skill: Some("foo".to_string()),
            expected: true,
            required: true,
            cases: None,
            path: None,
        };
        // "foo" is a substring of the invoked skill "foo-bar" but not its name.
        let trace = r#"{"seq":0,"payload":{"type":"tool_use","call_id":"call_1","tool_name":"Skill","input":{"skill":"foo-bar"}}}"#;

        let results = run_checks(&[check], "", trace, Path::new("/tmp"));

        assert!(
            !results[0].passed,
            "'foo' must not match an invocation of 'foo-bar'"
        );
    }

    #[test]
    fn test_skill_invoked_does_not_match_json_keys_or_other_fields() {
        let check = CheckDefinition::SkillInvoked {
            skill: Some("greeting-helper".to_string()),
            expected: true,
            required: true,
            cases: None,
            path: None,
        };
        // The skill name appears as a JSON key and inside an unrelated field,
        // but no identifying field names it.
        let trace = r#"{"seq":0,"payload":{"type":"tool_use","call_id":"call_1","tool_name":"Skill","input":{"greeting-helper":true,"args":"use greeting-helper please"}}}"#;

        let results = run_checks(&[check], "", trace, Path::new("/tmp"));

        assert!(
            !results[0].passed,
            "JSON keys and non-identifying fields must not satisfy skill_invoked"
        );
    }

    #[test]
    fn test_skill_invoked_exact_match_on_each_identifying_field() {
        for field in ["skill", "name", "skillName"] {
            let check = CheckDefinition::SkillInvoked {
                skill: Some("greeting-helper".to_string()),
                expected: true,
                required: true,
                cases: None,
                path: None,
            };
            let trace = format!(
                r#"{{"seq":0,"payload":{{"type":"tool_use","call_id":"call_1","tool_name":"Skill","input":{{"{}":"greeting-helper"}}}}}}"#,
                field
            );

            let results = run_checks(&[check], "", &trace, Path::new("/tmp"));

            assert!(
                results[0].passed,
                "exact match on identifying field '{}' must pass",
                field
            );
        }
    }

    #[test]
    fn test_skill_invoked_expected_false_passes_when_absent() {
        let check = CheckDefinition::SkillInvoked {
            skill: Some("greeting-helper".to_string()),
            expected: false,
            required: true,
            cases: None,
            path: None,
        };
        let trace = r#"{"seq":0,"payload":{"type":"tool_use","call_id":"call_1","tool_name":"Bash","input":{"command":"echo greeting-helper"}}}"#;

        let results = run_checks(&[check], "", trace, Path::new("/tmp"));

        assert!(results[0].passed);
    }

    #[test]
    fn test_file_exists_check_passes() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("output.txt");
        std::fs::write(&file_path, "content").unwrap();

        let check = CheckDefinition::FileExists {
            path: PathBuf::from("output.txt"),
            required: true,
            cases: None,
        };
        let results = run_checks(&[check], "", "", dir.path());
        assert!(results[0].passed);
    }

    #[test]
    fn test_file_exists_check_fails() {
        let dir = TempDir::new().unwrap();
        let check = CheckDefinition::FileExists {
            path: PathBuf::from("missing.txt"),
            required: true,
            cases: None,
        };
        let results = run_checks(&[check], "", "", dir.path());
        assert!(!results[0].passed);
    }

    #[test]
    fn test_max_tool_calls_passes() {
        let check = CheckDefinition::MaxToolCalls {
            limit: 5,
            required: true,
            cases: None,
        };
        let trace = r#"{"seq":0,"payload":{"type":"raw_json","data":{"cmd":"a"}}}
{"seq":1,"payload":{"type":"raw_json","data":{"cmd":"b"}}}
{"seq":2,"payload":{"type":"raw_line","line":"ok"}}"#;
        let results = run_checks(&[check], "", trace, Path::new("/tmp"));
        assert!(results[0].passed);
        assert_eq!(results[0].check_name, "max_tool_calls");
    }

    #[test]
    fn test_max_tool_calls_fails() {
        let check = CheckDefinition::MaxToolCalls {
            limit: 1,
            required: true,
            cases: None,
        };
        let trace = r#"{"seq":0,"payload":{"type":"raw_json","data":{"cmd":"a"}}}
{"seq":1,"payload":{"type":"raw_json","data":{"cmd":"b"}}}
{"seq":2,"payload":{"type":"raw_json","data":{"cmd":"c"}}}"#;
        let results = run_checks(&[check], "", trace, Path::new("/tmp"));
        assert!(!results[0].passed);
        assert_eq!(results[0].check_name, "max_tool_calls");
    }

    #[test]
    fn test_max_tool_calls_name_is_canonical() {
        let check = CheckDefinition::MaxToolCalls {
            limit: 1,
            required: true,
            cases: None,
        };
        assert_eq!(check.name(), "max_tool_calls");

        let json = serde_json::to_string(&check).unwrap();
        assert!(
            json.contains("\"name\":\"max_tool_calls\""),
            "serialized check must use canonical spelling, got {json}"
        );
        assert!(
            !json.contains("max_command_count"),
            "deprecated spelling must not be emitted, got {json}"
        );
    }

    #[test]
    fn test_legacy_max_command_count_alias_parses_and_behaves_like_max_tool_calls() {
        let toml = r#"
[[check]]
name = "max_command_count"
limit = 1
"#;
        let parsed: ChecksToml = toml::from_str(toml).unwrap();
        assert_eq!(parsed.checks.len(), 1);
        assert!(matches!(
            parsed.checks[0],
            CheckDefinition::MaxToolCalls {
                limit: 1,
                required: true,
                ..
            }
        ));

        let trace = r#"{"seq":0,"payload":{"type":"tool_use","call_id":"call_1","tool_name":"shell","input":{"command":"ls"}}}
{"seq":1,"payload":{"type":"tool_use","call_id":"call_2","tool_name":"shell","input":{"command":"pwd"}}}"#;
        let results = run_checks(&parsed.checks, "", trace, Path::new("/tmp"));
        assert!(!results[0].passed);
        assert_eq!(results[0].check_name, "max_tool_calls");
    }

    #[test]
    fn test_skill_invoked_toml_defaults_expected_and_required_to_true() {
        let toml = r#"
[[check]]
name = "skill_invoked"
skill = "greeting-helper"
"#;
        let parsed: ChecksToml = toml::from_str(toml).unwrap();
        assert!(matches!(
            &parsed.checks[0],
            CheckDefinition::SkillInvoked {
                skill: Some(skill),
                expected: true,
                required: true,
                ..
            } if skill == "greeting-helper"
        ));
    }

    #[test]
    fn test_count_raw_json_events_ignores_substring_only() {
        let trace = r#"{"seq":0,"payload":{"type":"raw_line","line":"mentions raw_json text"}}
{"seq":1,"payload":{"type":"raw_json","data":{"cmd":"x"}}}"#;
        assert_eq!(count_raw_json_events(trace), 1);
    }

    #[test]
    fn test_suite_passes_all_passed() {
        let results = vec![
            CheckResult {
                check_name: "a".to_string(),
                passed: true,
                required: true,
                message: None,
                not_observable: None,
                score: None,
            },
            CheckResult {
                check_name: "b".to_string(),
                passed: true,
                required: true,
                message: None,
                not_observable: None,
                score: None,
            },
        ];
        assert!(suite_passes(&results));
    }

    #[test]
    fn test_suite_passes_any_failed() {
        let results = vec![
            CheckResult {
                check_name: "a".to_string(),
                passed: true,
                required: true,
                message: None,
                not_observable: None,
                score: None,
            },
            CheckResult {
                check_name: "b".to_string(),
                passed: false,
                required: true,
                message: Some("failed".to_string()),
                not_observable: None,
                score: None,
            },
        ];
        assert!(!suite_passes(&results));
    }

    #[test]
    fn test_suite_passes_ignores_optional_failures() {
        let results = vec![
            CheckResult {
                check_name: "required".to_string(),
                passed: true,
                required: true,
                message: None,
                not_observable: None,
                score: None,
            },
            CheckResult {
                check_name: "optional".to_string(),
                passed: false,
                required: false,
                message: Some("advisory".to_string()),
                not_observable: None,
                score: None,
            },
        ];

        assert!(suite_passes(&results));
    }

    #[test]
    fn test_check_result_missing_required_deserializes_as_required() {
        let json = r#"{"check_name":"legacy","passed":true,"message":null}"#;
        let result: CheckResult = serde_json::from_str(json).unwrap();
        assert!(result.required);
    }

    #[test]
    fn test_load_checks_file_not_found() {
        let path = Path::new("/nonexistent/path/checks.toml");
        let result = load_checks(path);
        assert!(matches!(result, Err(ChecksError::Io(_))));
    }

    #[test]
    fn test_load_checks_valid_file_returns_checks() {
        let dir = TempDir::new().unwrap();
        let checks_file = dir.path().join("checks.toml");
        std::fs::write(
            &checks_file,
            r#"
[[check]]
name = "max_tool_calls"
limit = 2
"#,
        )
        .unwrap();

        let checks = load_checks(&checks_file).unwrap();
        assert!(matches!(
            checks.as_slice(),
            [CheckDefinition::MaxToolCalls {
                limit: 2,
                required: true,
                ..
            }]
        ));
    }

    #[test]
    fn test_load_checks_invalid_toml() {
        let dir = TempDir::new().unwrap();
        let checks_file = dir.path().join("checks.toml");
        std::fs::write(&checks_file, "this is not valid toml [[[[").unwrap();
        let result = load_checks(&checks_file);
        assert!(matches!(result, Err(ChecksError::Parse(_))));
    }

    // ── R6: per-case check selection ───────────────────────────────────────

    #[test]
    fn test_cases_selector_scopes_a_check_to_the_named_cases() {
        let scoped = CheckDefinition::FileExists {
            path: PathBuf::from("out.txt"),
            required: true,
            cases: Some(vec!["b".to_string(), "c".to_string()]),
        };
        assert!(!scoped.applies_to("a"));
        assert!(scoped.applies_to("b"));
        assert!(scoped.applies_to("c"));
    }

    #[test]
    fn test_absent_cases_selector_applies_to_every_case() {
        let global = CheckDefinition::FileExists {
            path: PathBuf::from("out.txt"),
            required: true,
            cases: None,
        };
        assert!(global.applies_to("a"));
        assert!(global.applies_to("anything-at-all"));
    }

    #[test]
    fn test_effective_checks_drops_checks_scoped_to_other_cases() {
        let checks = vec![
            CheckDefinition::FileExists {
                path: PathBuf::from("only-a.txt"),
                required: true,
                cases: Some(vec!["a".to_string()]),
            },
            CheckDefinition::FileExists {
                path: PathBuf::from("everywhere.txt"),
                required: true,
                cases: None,
            },
        ];
        let for_b = effective_checks(&checks, "b", true);
        let names: Vec<_> = for_b
            .iter()
            .filter_map(|c| match c {
                CheckDefinition::FileExists { path, .. } => Some(path.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec![PathBuf::from("everywhere.txt")]);
    }

    // ── R7: `should_trigger` is scored ─────────────────────────────────────

    #[test]
    fn test_should_trigger_generates_a_skill_invoked_check_with_matching_polarity() {
        for should_trigger in [true, false] {
            let generated = effective_checks(&[], "c1", should_trigger);
            assert_eq!(generated.len(), 1, "{generated:?}");
            match &generated[0] {
                CheckDefinition::SkillInvoked {
                    expected, required, ..
                } => {
                    assert_eq!(*expected, should_trigger);
                    assert!(*required, "the column is an assertion, not a hint");
                }
                other => panic!("expected skill_invoked, got {other:?}"),
            }
        }
    }

    #[test]
    fn test_explicit_skill_invoked_check_is_not_duplicated_by_the_column() {
        let explicit = vec![CheckDefinition::SkillInvoked {
            skill: Some("greeting-helper".to_string()),
            path: None,
            expected: true,
            required: true,
            cases: None,
        }];
        let generated = effective_checks(&explicit, "c1", true);
        assert_eq!(
            generated
                .iter()
                .filter(|c| matches!(c, CheckDefinition::SkillInvoked { .. }))
                .count(),
            1,
            "the operator's own check wins: {generated:?}"
        );
    }

    #[test]
    fn test_validate_rejects_a_check_contradicting_should_trigger() {
        let checks = vec![CheckDefinition::SkillInvoked {
            skill: None,
            path: None,
            expected: true,
            required: true,
            cases: None,
        }];
        let err = validate_case_checks(&checks, "c1", false).unwrap_err();
        assert!(err.contains("EVAL_CHECKS_INVALID"), "{err}");
        assert!(err.contains("c1"), "{err}");
        // The agreeing case is accepted.
        assert!(validate_case_checks(&checks, "c1", true).is_ok());
    }

    #[test]
    fn test_validate_ignores_a_contradiction_scoped_to_another_case() {
        let checks = vec![CheckDefinition::SkillInvoked {
            skill: None,
            path: None,
            expected: true,
            required: true,
            cases: Some(vec!["other".to_string()]),
        }];
        assert!(validate_case_checks(&checks, "c1", false).is_ok());
    }

    // ── R8: skill invocation is a path match ───────────────────────────────

    #[test]
    fn test_skill_invoked_matches_the_document_path_in_any_tool_input() {
        let check = CheckDefinition::SkillInvoked {
            skill: None,
            path: Some("/ws/.claude/skills/greeting-helper/SKILL.md".to_string()),
            expected: true,
            required: true,
            cases: None,
        };
        // pi reads a skill with `read_file`; there is no `Skill` tool anywhere.
        let trace = r#"{"seq":0,"payload":{"type":"tool_use","call_id":"c1","tool_name":"read_file","input":{"path":"/ws/.claude/skills/greeting-helper/SKILL.md"}}}"#;
        let results = run_checks(&[check], "", trace, Path::new("/tmp"));
        assert!(results[0].passed, "{:?}", results[0]);
    }

    #[test]
    fn test_skill_invoked_path_comes_from_the_context_when_the_check_omits_it() {
        let check = CheckDefinition::SkillInvoked {
            skill: None,
            path: None,
            expected: true,
            required: true,
            cases: None,
        };
        let trace = r#"{"seq":0,"payload":{"type":"tool_use","call_id":"c1","tool_name":"shell","input":{"command":"cat /ws/.claude/skills/greeting-helper/SKILL.md"}}}"#;
        let ctx = CheckContext {
            backend: "codex",
            structured_tools: true,
            typed_skill_tool: false,
            skill_path: Some("/ws/.claude/skills/greeting-helper/SKILL.md"),
        };
        let results = run_checks_in_context(&[check], trace, Path::new("/tmp"), &ctx);
        assert!(results[0].passed, "{:?}", results[0]);
    }

    #[test]
    fn test_skill_invoked_does_not_match_an_unrelated_path() {
        let check = CheckDefinition::SkillInvoked {
            skill: None,
            path: Some("/ws/.claude/skills/greeting-helper/SKILL.md".to_string()),
            expected: true,
            required: true,
            cases: None,
        };
        let trace = r#"{"seq":0,"payload":{"type":"tool_use","call_id":"c1","tool_name":"read_file","input":{"path":"/ws/README.md"}}}"#;
        let results = run_checks(&[check], "", trace, Path::new("/tmp"));
        assert!(!results[0].passed, "{:?}", results[0]);
    }

    // ── R9: a check that cannot be observed says so ────────────────────────

    fn text_only_ctx() -> CheckContext<'static> {
        CheckContext {
            backend: "opencode",
            structured_tools: false,
            typed_skill_tool: false,
            skill_path: None,
        }
    }

    #[test]
    fn test_tool_dependent_checks_are_not_observable_without_tool_frames() {
        let checks = vec![
            CheckDefinition::MaxToolCalls {
                limit: 1,
                required: true,
                cases: None,
            },
            CheckDefinition::SkillInvoked {
                skill: None,
                path: None,
                expected: true,
                required: true,
                cases: None,
            },
        ];
        // A trace with two tool calls: over-limit if it could be counted.
        let trace = concat!(
            r#"{"seq":0,"payload":{"type":"tool_use","call_id":"c1","tool_name":"a","input":{}}}"#,
            "\n",
            r#"{"seq":1,"payload":{"type":"tool_use","call_id":"c2","tool_name":"b","input":{}}}"#
        );
        let results = run_checks_in_context(&checks, trace, Path::new("/tmp"), &text_only_ctx());
        for r in &results {
            assert!(!r.is_observable(), "{r:?}");
            assert!(
                !r.passed,
                "a not-observable check must not report a pass: {r:?}"
            );
            let reason = &r.not_observable.as_ref().unwrap().reason;
            assert!(reason.contains("opencode"), "{reason}");
        }
    }

    #[test]
    fn test_suite_passes_ignores_a_not_observable_required_check() {
        let results = vec![
            CheckResult {
                check_name: "file_exists".to_string(),
                passed: true,
                required: true,
                message: None,
                not_observable: None,
                score: None,
            },
            CheckResult {
                check_name: "max_tool_calls".to_string(),
                passed: false,
                required: true,
                message: None,
                not_observable: Some(NotObservable {
                    reason: "no tool frames".to_string(),
                }),
                score: None,
            },
        ];
        assert!(
            suite_passes(&results),
            "the backend failed to produce evidence; the agent did not fail"
        );
    }

    #[test]
    fn test_unobservable_required_names_the_checks_to_refuse_the_suite_on() {
        let checks = vec![
            CheckDefinition::MaxToolCalls {
                limit: 1,
                required: true,
                cases: None,
            },
            CheckDefinition::MaxToolCalls {
                limit: 1,
                required: false,
                cases: None,
            },
            CheckDefinition::FileExists {
                path: PathBuf::from("out.txt"),
                required: true,
                cases: None,
            },
        ];
        let refused = unobservable_required(&checks, &text_only_ctx());
        assert_eq!(refused.len(), 1, "{refused:?}");
        assert_eq!(refused[0].0, "max_tool_calls");
        // Nothing is refused on a backend that decodes tool frames.
        assert!(unobservable_required(&checks, &CheckContext::default()).is_empty());
    }

    #[test]
    fn test_unknown_backend_is_not_treated_as_unobservable() {
        // "we were not told which backend" is not a claim that the decoder
        // cannot produce the evidence, and treating it as one would silently
        // un-score every tool-dependent check.
        let check = CheckDefinition::MaxToolCalls {
            limit: 5,
            required: true,
            cases: None,
        };
        let results = run_checks(&[check], "", "", Path::new("/tmp"));
        assert!(results[0].is_observable(), "{:?}", results[0]);
        assert!(results[0].passed);
    }
}
