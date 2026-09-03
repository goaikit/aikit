//! `[[judge]]` / `[judge_defaults]` (spec eval-judge R3, R5, R14).

use super::template::{self, Scope};
use crate::checks::ChecksToml;
use crate::suite::EvalSuite;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

pub const DEFAULT_SCALE: u32 = 5;
pub const DEFAULT_TEMPERATURE: f64 = 0.0;
pub const DEFAULT_MAX_TOKENS: u32 = 4096;
pub const DEFAULT_MAX_VAR_BYTES: usize = 32 * 1024;
pub const DEFAULT_MAX_RETRIES: u32 = 2;
pub const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// Environment variable that supplies the endpoint when neither the judge nor
/// `[judge_defaults]` names one. Read only when set explicitly: the gateway's
/// own api.openai.com fallback never applies to a judge (R3).
pub const ENDPOINT_ENV: &str = "AIKIT_LLM_URL";

/// Criterion name the engine reserves for the mean (R5): a rubric cannot
/// declare it.
pub const OVERALL: &str = "overall";

/// One `[[judge]]` table as written. Unknown keys are a parse error.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JudgeDefinition {
    pub name: String,
    /// Exact case ids; absent means every case.
    #[serde(default)]
    pub cases: Option<Vec<String>>,
    #[serde(default)]
    pub prompt: Option<String>,
    /// Relative to checks.toml.
    #[serde(default)]
    pub prompt_file: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub system_prompt_file: Option<String>,
    #[serde(default)]
    pub retry_prompt: Option<String>,
    #[serde(default)]
    pub retry_prompt_file: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// Gate: the trial passes this judge when `overall >= min_score`. Absent
    /// means advisory.
    #[serde(default)]
    pub min_score: Option<f64>,
    #[serde(rename = "criterion", default)]
    pub criteria: Vec<CriterionDefinition>,
}

/// One `[[judge.criterion]]` table as written.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CriterionDefinition {
    pub name: String,
    pub kind: CriterionKind,
    /// Scale criteria only; default 5, at least 2.
    #[serde(default)]
    pub scale: Option<u32>,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CriterionKind {
    Scale,
    Bool,
}

/// `[judge_defaults]`. Unknown keys are a parse error.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JudgeDefaults {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub max_var_bytes: Option<usize>,
    #[serde(default)]
    pub max_retries: Option<u32>,
    #[serde(default)]
    pub timeout: Option<u64>,
}

/// A criterion with its defaults applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Criterion {
    pub name: String,
    pub kind: CriterionKind,
    /// Always set; 2 for bool criteria (yes / no) so the field means one thing.
    pub scale: u32,
    pub description: String,
}

/// The resolved identity of a judge (R3): what goes on the wire, minus the key.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JudgeIdentity {
    pub model: String,
    /// `None` only while validating; judging requires it (R3).
    pub base_url: Option<String>,
    /// Environment variable holding the key; `None` means the gateway's order.
    pub api_key_env: Option<String>,
    pub temperature: f64,
    pub top_p: Option<f64>,
    pub max_tokens: u32,
}

/// A judge after resolution: prompt files read, defaults applied, identity
/// resolved. What [`judge_run_dir`](super::run::judge_run_dir) runs.
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedJudge {
    pub name: String,
    pub cases: Option<Vec<String>>,
    pub prompt: String,
    pub system_prompt: Option<String>,
    pub retry_prompt: Option<String>,
    pub min_score: Option<f64>,
    pub criteria: Vec<Criterion>,
    pub identity: JudgeIdentity,
    pub max_var_bytes: usize,
    pub max_retries: u32,
    pub timeout_secs: u64,
}

impl ResolvedJudge {
    pub fn applies_to(&self, case_id: &str) -> bool {
        match &self.cases {
            None => true,
            Some(ids) => ids.iter().any(|id| id == case_id),
        }
    }

    pub fn is_gated(&self) -> bool {
        self.min_score.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueLevel {
    Error,
    Warning,
}

/// One finding of [`validate_judges`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub level: IssueLevel,
    /// The judge concerned; `None` for file-level findings.
    pub judge: Option<String>,
    pub message: String,
}

impl ValidationIssue {
    fn error(judge: &str, message: impl Into<String>) -> Self {
        Self {
            level: IssueLevel::Error,
            judge: Some(judge.to_string()),
            message: message.into(),
        }
    }

    fn warning(judge: &str, message: impl Into<String>) -> Self {
        Self {
            level: IssueLevel::Warning,
            judge: Some(judge.to_string()),
            message: message.into(),
        }
    }

    pub fn is_error(&self) -> bool {
        self.level == IssueLevel::Error
    }
}

impl fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.level {
            IssueLevel::Error => "error",
            IssueLevel::Warning => "warning",
        };
        match &self.judge {
            Some(judge) => write!(f, "{}: judge '{}': {}", level, judge, self.message),
            None => write!(f, "{}: {}", level, self.message),
        }
    }
}

/// Exactly one of an inline text and a file reference, the file read relative
/// to `checks_dir`.
fn text_or_file(
    judge: &str,
    what: &str,
    inline: &Option<String>,
    file: &Option<String>,
    checks_dir: &Path,
    required: bool,
    issues: &mut Vec<ValidationIssue>,
) -> Option<String> {
    match (inline, file) {
        (Some(_), Some(_)) => {
            issues.push(ValidationIssue::error(
                judge,
                format!("both `{what}` and `{what}_file` are set; declare exactly one"),
            ));
            None
        }
        (Some(text), None) => Some(text.clone()),
        (None, Some(rel)) => {
            let path = checks_dir.join(rel);
            match std::fs::read_to_string(&path) {
                Ok(text) => Some(text),
                Err(e) => {
                    issues.push(ValidationIssue::error(
                        judge,
                        format!("`{what}_file` {}: {}", path.display(), e),
                    ));
                    None
                }
            }
        }
        (None, None) => {
            if required {
                issues.push(ValidationIssue::error(
                    judge,
                    format!("neither `{what}` nor `{what}_file` is set; declare exactly one"),
                ));
            }
            None
        }
    }
}

/// Resolve one judge, collecting every problem rather than stopping at the
/// first. `Err` carries the issues; `Ok` means none were errors.
fn resolve_one(
    def: &JudgeDefinition,
    defaults: &JudgeDefaults,
    checks_dir: &Path,
    judge_model: Option<&str>,
) -> Result<ResolvedJudge, Vec<ValidationIssue>> {
    let mut issues = Vec::new();
    let name = def.name.as_str();

    let prompt = text_or_file(
        name,
        "prompt",
        &def.prompt,
        &def.prompt_file,
        checks_dir,
        true,
        &mut issues,
    );
    let system_prompt = text_or_file(
        name,
        "system_prompt",
        &def.system_prompt,
        &def.system_prompt_file,
        checks_dir,
        false,
        &mut issues,
    );
    let retry_prompt = text_or_file(
        name,
        "retry_prompt",
        &def.retry_prompt,
        &def.retry_prompt_file,
        checks_dir,
        false,
        &mut issues,
    );

    // R3: --judge-model overrides everything and is what the record names.
    let model = judge_model
        .map(str::to_string)
        .or_else(|| def.model.clone())
        .or_else(|| defaults.model.clone());
    if model.is_none() {
        issues.push(ValidationIssue::error(
            name,
            "no `model`: set it on [[judge]] or [judge_defaults], or pass --judge-model",
        ));
    }

    // R3: endpoint from the judge, then the defaults, then an explicit
    // AIKIT_LLM_URL. No default endpoint.
    let base_url = def
        .base_url
        .clone()
        .or_else(|| defaults.base_url.clone())
        .or_else(|| {
            std::env::var(ENDPOINT_ENV)
                .ok()
                .filter(|v| !v.trim().is_empty())
        });

    let criteria: Vec<Criterion> = def
        .criteria
        .iter()
        .map(|c| Criterion {
            name: c.name.clone(),
            kind: c.kind,
            scale: match c.kind {
                CriterionKind::Scale => c.scale.unwrap_or(DEFAULT_SCALE),
                CriterionKind::Bool => 2,
            },
            description: c.description.clone(),
        })
        .collect();

    if criteria.is_empty() {
        issues.push(ValidationIssue::error(
            name,
            "no [[judge.criterion]]: a judge with nothing to score renders no judgment",
        ));
    }
    let mut seen = BTreeSet::new();
    for c in &def.criteria {
        if !seen.insert(c.name.as_str()) {
            issues.push(ValidationIssue::error(
                name,
                format!("duplicate criterion name '{}'", c.name),
            ));
        }
        if c.name == OVERALL {
            issues.push(ValidationIssue::error(
                name,
                format!("criterion name '{OVERALL}' is reserved for the engine's mean"),
            ));
        }
        if c.kind == CriterionKind::Scale {
            if let Some(scale) = c.scale {
                if scale < 2 {
                    issues.push(ValidationIssue::error(
                        name,
                        format!(
                            "criterion '{}': scale must be at least 2, got {}",
                            c.name, scale
                        ),
                    ));
                }
            }
        } else if c.scale.is_some() {
            issues.push(ValidationIssue::error(
                name,
                format!(
                    "criterion '{}': `scale` applies to kind = \"scale\" only",
                    c.name
                ),
            ));
        }
    }

    if let Some(min) = def.min_score {
        if !(0.0..=1.0).contains(&min) || min.is_nan() {
            issues.push(ValidationIssue::error(
                name,
                format!("min_score must be within [0, 1], got {}", min),
            ));
        }
    }

    let temperature = def
        .temperature
        .or(defaults.temperature)
        .unwrap_or(DEFAULT_TEMPERATURE);
    let top_p = def.top_p.or(defaults.top_p);
    let max_tokens = def
        .max_tokens
        .or(defaults.max_tokens)
        .unwrap_or(DEFAULT_MAX_TOKENS);
    let max_var_bytes = defaults.max_var_bytes.unwrap_or(DEFAULT_MAX_VAR_BYTES);
    if max_var_bytes == 0 {
        issues.push(ValidationIssue::error(
            name,
            "max_var_bytes must be positive",
        ));
    }

    if issues.iter().any(ValidationIssue::is_error) {
        return Err(issues);
    }

    Ok(ResolvedJudge {
        name: def.name.clone(),
        cases: def.cases.clone(),
        prompt: prompt.unwrap_or_default(),
        system_prompt,
        retry_prompt,
        min_score: def.min_score,
        criteria,
        identity: JudgeIdentity {
            model: model.unwrap_or_default(),
            base_url,
            api_key_env: def
                .api_key_env
                .clone()
                .or_else(|| defaults.api_key_env.clone()),
            temperature,
            top_p,
            max_tokens,
        },
        max_var_bytes,
        max_retries: defaults.max_retries.unwrap_or(DEFAULT_MAX_RETRIES),
        timeout_secs: defaults.timeout.unwrap_or(DEFAULT_TIMEOUT_SECS),
    })
}

/// Template findings for one resolved judge (R2, R14).
fn template_issues(judge: &ResolvedJudge, suite: Option<&EvalSuite>) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let name = judge.name.as_str();
    let mut prompt_vars: Vec<String> = Vec::new();

    for (what, text, scope) in [
        ("prompt", Some(&judge.prompt), Scope::Prompt),
        ("system_prompt", judge.system_prompt.as_ref(), Scope::Prompt),
        ("retry_prompt", judge.retry_prompt.as_ref(), Scope::Retry),
    ] {
        let Some(text) = text else { continue };
        match template::placeholders(text) {
            Err(e) => issues.push(ValidationIssue::error(name, format!("`{what}`: {e}"))),
            Ok(vars) => {
                for var in &vars {
                    if let Err(reason) = template::check_placeholder(var, scope) {
                        issues.push(ValidationIssue::error(name, format!("`{what}`: {reason}")));
                        continue;
                    }
                    if let (Some(suite), Some(column)) = (suite, var.strip_prefix("case.")) {
                        if column != "prompt"
                            && !suite.cases.iter().any(|c| c.extra.contains_key(column))
                        {
                            issues.push(ValidationIssue::error(
                                name,
                                format!(
                                    "`{what}`: unknown template variable `{{{{{var}}}}}`: prompts.csv has no column '{column}'"
                                ),
                            ));
                        }
                    }
                }
                if scope == Scope::Prompt {
                    prompt_vars.extend(vars);
                }
            }
        }
    }

    if !prompt_vars.iter().any(|v| v == "output_contract") {
        issues.push(ValidationIssue::error(
            name,
            "prompt does not use `{{output_contract}}`; the model must be told the reply shape",
        ));
    }
    if !prompt_vars.iter().any(|v| v == "rubric") {
        issues.push(ValidationIssue::warning(
            name,
            "prompt does not use `{{rubric}}`; the model will score criteria it was never shown",
        ));
    }
    issues
}

/// Validate every judge in a checks file without touching the network or any
/// key (R14). `suite` enables the `cases` and `{{case.<column>}}` checks;
/// `target_model` enables the same-model warning.
pub fn validate_judges(
    file: &ChecksToml,
    checks_dir: &Path,
    suite: Option<&EvalSuite>,
    judge_model: Option<&str>,
    target_model: Option<&str>,
) -> Vec<ValidationIssue> {
    let defaults = file.judge_defaults.clone().unwrap_or_default();
    let mut issues = Vec::new();
    let mut names = BTreeSet::new();

    for def in &file.judges {
        if def.name.trim().is_empty() {
            issues.push(ValidationIssue {
                level: IssueLevel::Error,
                judge: None,
                message: "a [[judge]] has an empty name".to_string(),
            });
        }
        if !names.insert(def.name.as_str()) {
            issues.push(ValidationIssue::error(
                &def.name,
                "duplicate judge name; judgments are keyed by it",
            ));
        }
        if let (Some(suite), Some(ids)) = (suite, &def.cases) {
            for id in ids {
                if !suite.cases.iter().any(|c| &c.id == id) {
                    issues.push(ValidationIssue::error(
                        &def.name,
                        format!(
                            "`cases` names '{}', which matches no case in prompts.csv",
                            id
                        ),
                    ));
                }
            }
        }
        match resolve_one(def, &defaults, checks_dir, judge_model) {
            Err(mut errs) => issues.append(&mut errs),
            Ok(judge) => {
                issues.append(&mut template_issues(&judge, suite));
                if let Some(target) = target_model {
                    if judge.identity.model == target {
                        issues.push(ValidationIssue::warning(
                            &def.name,
                            format!(
                                "judge model '{}' is the model under test; a model grading itself is a weak witness",
                                target
                            ),
                        ));
                    }
                }
            }
        }
    }
    issues
}

/// Resolve every judge in a checks file. Fails with the full issue list when
/// any judge has an error-level finding, so nothing half-resolved runs.
pub fn resolve_judges(
    file: &ChecksToml,
    checks_dir: &Path,
    suite: Option<&EvalSuite>,
    judge_model: Option<&str>,
) -> Result<Vec<ResolvedJudge>, Vec<ValidationIssue>> {
    let issues = validate_judges(file, checks_dir, suite, judge_model, None);
    if issues.iter().any(ValidationIssue::is_error) {
        return Err(issues);
    }
    let defaults = file.judge_defaults.clone().unwrap_or_default();
    file.judges
        .iter()
        .map(|def| resolve_one(def, &defaults, checks_dir, judge_model))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::load_checks_file;
    use crate::suite::EvalCase;
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    fn write_checks(dir: &Path, toml: &str) -> std::path::PathBuf {
        let path = dir.join("checks.toml");
        std::fs::write(&path, toml).unwrap();
        path
    }

    const GOOD: &str = r#"
[[judge]]
name = "quality"
prompt = "Answer: {{trial.final_answer}}\n{{rubric}}\n{{output_contract}}"
retry_prompt = "Rejected: {{validation_error}}"
model = "judge-1"
min_score = 0.5

[[judge.criterion]]
name = "clear"
kind = "scale"
description = "Is it clear?"

[[judge.criterion]]
name = "runs"
kind = "bool"
description = "Would it run?"
"#;

    fn suite_with(columns: &[(&str, &str)]) -> EvalSuite {
        let mut extra = BTreeMap::new();
        for (k, v) in columns {
            extra.insert(k.to_string(), v.to_string());
        }
        EvalSuite::new(vec![EvalCase {
            id: "c1".to_string(),
            prompt: "p".to_string(),
            should_trigger: true,
            tags: vec![],
            workspace_subdir: None,
            extra,
        }])
    }

    fn errors(issues: &[ValidationIssue]) -> Vec<String> {
        issues
            .iter()
            .filter(|i| i.is_error())
            .map(|i| i.message.clone())
            .collect()
    }

    #[test]
    fn good_file_resolves_with_defaults_applied() {
        let dir = TempDir::new().unwrap();
        let file = load_checks_file(&write_checks(dir.path(), GOOD)).unwrap();
        let judges = resolve_judges(&file, dir.path(), None, None).expect("valid");
        assert_eq!(judges.len(), 1);
        let j = &judges[0];
        assert_eq!(j.identity.model, "judge-1");
        assert_eq!(j.identity.temperature, 0.0);
        assert_eq!(j.identity.max_tokens, 4096);
        assert_eq!(j.criteria[0].scale, 5);
        assert_eq!(j.criteria[1].scale, 2);
        assert_eq!(j.max_retries, 2);
        assert_eq!(j.timeout_secs, 120);
        assert_eq!(j.max_var_bytes, 32 * 1024);
        assert!(j.is_gated());
        assert!(j.applies_to("anything"));
    }

    #[test]
    fn unknown_key_on_judge_fails_parse() {
        let dir = TempDir::new().unwrap();
        let toml = GOOD.replacen(
            "model = \"judge-1\"",
            "model = \"judge-1\"\nagent = \"claude\"",
            1,
        );
        let err = load_checks_file(&write_checks(dir.path(), &toml)).expect_err("agent is unknown");
        assert!(err.to_string().contains("agent"), "{err}");
    }

    #[test]
    fn unknown_key_on_defaults_and_criterion_fail_parse() {
        let dir = TempDir::new().unwrap();
        let toml = format!("{GOOD}\n[judge_defaults]\nseed = 1\n");
        assert!(load_checks_file(&write_checks(dir.path(), &toml)).is_err());
        let toml = GOOD.replacen("kind = \"bool\"", "kind = \"bool\"\nweight = 2", 1);
        assert!(load_checks_file(&write_checks(dir.path(), &toml)).is_err());
    }

    #[test]
    fn model_resolves_from_defaults_and_override_wins() {
        let dir = TempDir::new().unwrap();
        let toml = format!(
            "{}\n[judge_defaults]\nmodel = \"from-defaults\"\n",
            GOOD.replacen("model = \"judge-1\"\n", "", 1)
        );
        let file = load_checks_file(&write_checks(dir.path(), &toml)).unwrap();
        let judges = resolve_judges(&file, dir.path(), None, None).unwrap();
        assert_eq!(judges[0].identity.model, "from-defaults");
        let judges = resolve_judges(&file, dir.path(), None, Some("cli-model")).unwrap();
        assert_eq!(judges[0].identity.model, "cli-model");
    }

    #[test]
    fn missing_model_is_an_error() {
        let dir = TempDir::new().unwrap();
        let toml = GOOD.replacen("model = \"judge-1\"\n", "", 1);
        let file = load_checks_file(&write_checks(dir.path(), &toml)).unwrap();
        let issues = validate_judges(&file, dir.path(), None, None, None);
        assert!(
            errors(&issues).iter().any(|m| m.contains("no `model`")),
            "{issues:?}"
        );
    }

    #[test]
    fn prompt_file_is_read_relative_to_checks_dir() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("p.md"), "{{rubric}} {{output_contract}}").unwrap();
        let toml = GOOD.replacen(
            "prompt = \"Answer: {{trial.final_answer}}\\n{{rubric}}\\n{{output_contract}}\"",
            "prompt_file = \"p.md\"",
            1,
        );
        let file = load_checks_file(&write_checks(dir.path(), &toml)).unwrap();
        let judges = resolve_judges(&file, dir.path(), None, None).unwrap();
        assert_eq!(judges[0].prompt, "{{rubric}} {{output_contract}}");
    }

    #[test]
    fn both_prompt_and_prompt_file_is_an_error_and_so_is_neither() {
        let dir = TempDir::new().unwrap();
        let toml = GOOD.replacen("model =", "prompt_file = \"p.md\"\nmodel =", 1);
        let file = load_checks_file(&write_checks(dir.path(), &toml)).unwrap();
        let issues = validate_judges(&file, dir.path(), None, None, None);
        assert!(
            errors(&issues).iter().any(|m| m.contains("both `prompt`")),
            "{issues:?}"
        );

        let toml = GOOD.replacen(
            "prompt = \"Answer: {{trial.final_answer}}\\n{{rubric}}\\n{{output_contract}}\"\n",
            "",
            1,
        );
        let file = load_checks_file(&write_checks(dir.path(), &toml)).unwrap();
        let issues = validate_judges(&file, dir.path(), None, None, None);
        assert!(
            errors(&issues)
                .iter()
                .any(|m| m.contains("neither `prompt`")),
            "{issues:?}"
        );
    }

    #[test]
    fn unknown_template_variable_is_an_error() {
        let dir = TempDir::new().unwrap();
        let toml = GOOD.replacen("{{trial.final_answer}}", "{{trial.answer}}", 1);
        let file = load_checks_file(&write_checks(dir.path(), &toml)).unwrap();
        let issues = validate_judges(&file, dir.path(), None, None, None);
        assert!(
            errors(&issues)
                .iter()
                .any(|m| m.contains("{{trial.answer}}")),
            "{issues:?}"
        );
    }

    #[test]
    fn case_column_must_exist_when_the_suite_is_known() {
        let dir = TempDir::new().unwrap();
        let toml = GOOD.replacen("{{trial.final_answer}}", "{{case.expected}}", 1);
        let file = load_checks_file(&write_checks(dir.path(), &toml)).unwrap();
        let with = suite_with(&[("expected", "x")]);
        assert!(errors(&validate_judges(&file, dir.path(), Some(&with), None, None)).is_empty());
        let without = suite_with(&[]);
        let issues = validate_judges(&file, dir.path(), Some(&without), None, None);
        assert!(
            errors(&issues)
                .iter()
                .any(|m| m.contains("no column 'expected'")),
            "{issues:?}"
        );
    }

    #[test]
    fn retry_prompt_accepts_only_validation_error() {
        let dir = TempDir::new().unwrap();
        let toml = GOOD.replacen("Rejected: {{validation_error}}", "Rejected: {{rubric}}", 1);
        let file = load_checks_file(&write_checks(dir.path(), &toml)).unwrap();
        let issues = validate_judges(&file, dir.path(), None, None, None);
        assert!(
            errors(&issues).iter().any(|m| m.contains("retry_prompt")),
            "{issues:?}"
        );
    }

    #[test]
    fn cases_must_name_real_cases() {
        let dir = TempDir::new().unwrap();
        let toml = GOOD.replacen(
            "min_score = 0.5",
            "min_score = 0.5\ncases = [\"c1\", \"ghost\"]",
            1,
        );
        let file = load_checks_file(&write_checks(dir.path(), &toml)).unwrap();
        let issues = validate_judges(&file, dir.path(), Some(&suite_with(&[])), None, None);
        assert!(
            errors(&issues).iter().any(|m| m.contains("'ghost'")),
            "{issues:?}"
        );
    }

    #[test]
    fn rubric_rules_duplicate_reserved_scale_min_score() {
        let dir = TempDir::new().unwrap();
        let toml = GOOD
            .replacen("name = \"runs\"", "name = \"clear\"", 1)
            .replacen("min_score = 0.5", "min_score = 1.5", 1);
        let file = load_checks_file(&write_checks(dir.path(), &toml)).unwrap();
        let errs = errors(&validate_judges(&file, dir.path(), None, None, None));
        assert!(
            errs.iter().any(|m| m.contains("duplicate criterion")),
            "{errs:?}"
        );
        assert!(errs.iter().any(|m| m.contains("min_score")), "{errs:?}");

        let toml = GOOD
            .replacen("name = \"clear\"", "name = \"overall\"", 1)
            .replacen("kind = \"scale\"", "kind = \"scale\"\nscale = 1", 1);
        let file = load_checks_file(&write_checks(dir.path(), &toml)).unwrap();
        let errs = errors(&validate_judges(&file, dir.path(), None, None, None));
        assert!(errs.iter().any(|m| m.contains("reserved")), "{errs:?}");
        assert!(errs.iter().any(|m| m.contains("at least 2")), "{errs:?}");
    }

    #[test]
    fn missing_output_contract_is_error_and_missing_rubric_is_warning() {
        let dir = TempDir::new().unwrap();
        let toml = GOOD.replacen("\\n{{rubric}}\\n{{output_contract}}", "", 1);
        let file = load_checks_file(&write_checks(dir.path(), &toml)).unwrap();
        let issues = validate_judges(&file, dir.path(), None, None, None);
        assert!(
            errors(&issues)
                .iter()
                .any(|m| m.contains("output_contract")),
            "{issues:?}"
        );
        assert!(
            issues
                .iter()
                .any(|i| i.level == IssueLevel::Warning && i.message.contains("rubric")),
            "{issues:?}"
        );
    }

    #[test]
    fn judge_model_equal_to_target_is_a_warning_only() {
        let dir = TempDir::new().unwrap();
        let file = load_checks_file(&write_checks(dir.path(), GOOD)).unwrap();
        let issues = validate_judges(&file, dir.path(), None, None, Some("judge-1"));
        assert!(errors(&issues).is_empty());
        assert!(issues
            .iter()
            .any(|i| i.level == IssueLevel::Warning && i.message.contains("model under test")));
    }

    #[test]
    fn duplicate_judge_names_are_an_error() {
        let dir = TempDir::new().unwrap();
        let toml = format!("{GOOD}\n{GOOD}");
        let file = load_checks_file(&write_checks(dir.path(), &toml)).unwrap();
        let issues = validate_judges(&file, dir.path(), None, None, None);
        assert!(
            errors(&issues)
                .iter()
                .any(|m| m.contains("duplicate judge name")),
            "{issues:?}"
        );
    }

    #[test]
    fn checks_file_without_judges_still_loads_its_checks() {
        let dir = TempDir::new().unwrap();
        let file = load_checks_file(&write_checks(
            dir.path(),
            "[[check]]\nname = \"command_contains\"\npattern = \"x\"\n",
        ))
        .unwrap();
        assert_eq!(file.checks.len(), 1);
        assert!(file.judges.is_empty());
        assert!(validate_judges(&file, dir.path(), None, None, None).is_empty());
    }
}
