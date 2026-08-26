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
    },
    /// Check whether the trace contains a structured `Skill` tool invocation.
    ///
    /// When `skill` is omitted, any `Skill` tool use matches. When present, the
    /// serialized `Skill` input must contain that skill name. `expected = false`
    /// asserts no matching skill invocation occurred.
    #[serde(rename = "skill_invoked")]
    SkillInvoked {
        skill: Option<String>,
        #[serde(default = "default_expected")]
        expected: bool,
        #[serde(default = "default_required")]
        required: bool,
    },
    /// Check whether a file exists in the working directory after execution
    #[serde(rename = "file_exists")]
    FileExists {
        path: PathBuf,
        #[serde(default = "default_required")]
        required: bool,
    },
    /// Check that the number of decoded tool calls does not exceed a limit
    #[serde(rename = "max_tool_calls", alias = "max_command_count")]
    MaxToolCalls {
        limit: usize,
        #[serde(default = "default_required")]
        required: bool,
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
}

/// Result of a single check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub check_name: String,
    pub passed: bool,
    #[serde(default = "default_required")]
    pub required: bool,
    pub message: Option<String>,
}

/// TOML file structure for checks configuration
#[derive(Debug, Deserialize)]
pub struct ChecksToml {
    #[serde(rename = "check", default)]
    pub checks: Vec<CheckDefinition>,
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
pub fn load_checks(path: &std::path::Path) -> Result<Vec<CheckDefinition>, ChecksError> {
    let content = std::fs::read_to_string(path)?;
    let parsed: ChecksToml = toml::from_str(&content)?;
    Ok(parsed.checks)
}

/// Run all checks against captured stdout content and working directory
pub fn run_checks(
    checks: &[CheckDefinition],
    stdout_content: &str,
    trace_jsonl: &str,
    working_dir: &std::path::Path,
) -> Vec<CheckResult> {
    checks
        .iter()
        .map(|check| run_single_check(check, stdout_content, trace_jsonl, working_dir))
        .collect()
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

fn run_single_check(
    check: &CheckDefinition,
    stdout_content: &str,
    trace_jsonl: &str,
    working_dir: &std::path::Path,
) -> CheckResult {
    let required = check.is_required();
    match check {
        CheckDefinition::TriggerExpectation {
            pattern, expected, ..
        } => {
            let _ = stdout_content;
            let found = trace_jsonl.contains(pattern.as_str());
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
            }
        }
        CheckDefinition::CommandContains { pattern, .. } => {
            let _ = stdout_content;
            let passed = trace_jsonl.contains(pattern.as_str());
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
            }
        }
        CheckDefinition::SkillInvoked {
            skill, expected, ..
        } => {
            let found = skill_invoked(trace_jsonl, skill.as_deref());
            let passed = found == *expected;
            let target = skill.as_deref().unwrap_or("any skill");
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
            }
        }
    }
}

fn skill_invoked(trace_jsonl: &str, skill: Option<&str>) -> bool {
    trace_jsonl
        .lines()
        .filter_map(|line| serde_json::from_str::<TraceEvent>(line).ok())
        .any(|event| match event.payload {
            TracePayload::ToolUse {
                tool_name, input, ..
            } if tool_name == "Skill" => match skill {
                Some(skill) => {
                    serde_json::to_string(&input).is_ok_and(|serialized| serialized.contains(skill))
                }
                None => true,
            },
            _ => false,
        })
}

/// Aggregate check results: suite passes if all required checks pass
pub fn suite_passes(results: &[CheckResult]) -> bool {
    results.iter().filter(|r| r.required).all(|r| r.passed)
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
        };

        let results = run_checks(&[check], stdout, trace, Path::new("/tmp"));

        assert!(
            results[0].passed,
            "expected=false should pass when the trace has no matching trigger"
        );
    }

    #[test]
    fn test_command_contains_uses_trace_not_stdout() {
        let check = CheckDefinition::CommandContains {
            pattern: "trace-only".to_string(),
            required: true,
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
            },
            CheckDefinition::CommandContains {
                pattern: "b".to_string(),
                required: true,
            },
            CheckDefinition::SkillInvoked {
                skill: Some("d".to_string()),
                expected: true,
                required: false,
            },
            CheckDefinition::FileExists {
                path: PathBuf::from("c"),
                required: false,
            },
            CheckDefinition::MaxToolCalls {
                limit: 1,
                required: true,
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
    fn test_skill_invoked_expected_false_passes_when_absent() {
        let check = CheckDefinition::SkillInvoked {
            skill: Some("greeting-helper".to_string()),
            expected: false,
            required: true,
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
        };
        let results = run_checks(&[check], "", "", dir.path());
        assert!(!results[0].passed);
    }

    #[test]
    fn test_max_tool_calls_passes() {
        let check = CheckDefinition::MaxToolCalls {
            limit: 5,
            required: true,
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
                required: true
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
                required: true
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
            },
            CheckResult {
                check_name: "b".to_string(),
                passed: true,
                required: true,
                message: None,
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
            },
            CheckResult {
                check_name: "b".to_string(),
                passed: false,
                required: true,
                message: Some("failed".to_string()),
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
            },
            CheckResult {
                check_name: "optional".to_string(),
                passed: false,
                required: false,
                message: Some("advisory".to_string()),
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
                required: true
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
}
