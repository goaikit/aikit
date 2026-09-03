//! The `aikit.judgment/1` record (spec eval-judge R11).
//!
//! `trial-N/judgments.json` is an append-only array. The latest element per
//! judge name is the one that flattens and reduces; earlier ones are history.
//! The record never holds the API key: the request is rebuilt field by field,
//! not serialized from the gateway's request type.

use super::config::ResolvedJudge;
use aikit_sdk::llm::{LlmMessage, LlmUsage};
use aikit_sdk::AttemptKind;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const JUDGMENT_SCHEMA: &str = "aikit.judgment/1";

#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    #[error("EVAL_JUDGMENTS_IO: {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("EVAL_JUDGMENTS_CORRUPT: {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

/// Who judged: enough to know whether two judgments are comparable, never
/// enough to reach the endpoint. Host only, no path, no key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgmentIdentity {
    /// The model the request named.
    pub model: String,
    /// The model the reply named, when the provider reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_reported: Option<String>,
    pub endpoint_host: String,
    pub temperature: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    pub max_tokens: u32,
}

/// One request/reply pair. `request` is the body as sent, which carries no
/// credential (the bearer travels in a header the record never sees).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttemptRecord {
    pub kind: AttemptKind,
    pub request: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<LlmUsage>,
    /// Why this attempt did not yield a validated reply, when it did not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JudgmentUsage {
    pub input: u64,
    pub output: u64,
    pub total: u64,
}

impl JudgmentUsage {
    pub fn add(&mut self, usage: &LlmUsage) {
        self.input += usage.input_tokens;
        self.output += usage.output_tokens;
        self.total += usage
            .total_tokens
            .unwrap_or(usage.input_tokens + usage.output_tokens);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Judgment {
    pub schema: String,
    pub judge: String,
    pub judge_hash: String,
    pub cache_key: String,
    pub identity: JudgmentIdentity,
    #[serde(default)]
    pub attempts: Vec<AttemptRecord>,
    /// Per-criterion scores plus `overall`; absent when no attempt produced a
    /// validated reply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scores: Option<std::collections::BTreeMap<String, f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub usage: JudgmentUsage,
    /// Only when the provider reported one. Never estimated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub truncated: Vec<String>,
    /// RFC 3339, UTC.
    pub judged_at: String,
}

impl Judgment {
    /// A judgment that produced scores.
    pub fn is_scored(&self) -> bool {
        self.scores
            .as_ref()
            .map(|s| s.contains_key("overall"))
            .unwrap_or(false)
    }

    pub fn overall(&self) -> Option<f64> {
        self.scores.as_ref().and_then(|s| s.get("overall").copied())
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

/// `host[:port]` of an endpoint: scheme, userinfo, path and query dropped.
pub fn endpoint_host(base_url: &str) -> String {
    let rest = base_url.trim();
    let rest = rest.split_once("://").map(|(_, r)| r).unwrap_or(rest);
    let rest = rest.rsplit_once('@').map(|(_, r)| r).unwrap_or(rest);
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    rest[..end].to_string()
}

/// Identity of a judge's *definition*: everything that could change a score
/// (criteria, prompts, model, host, sampling, the byte cap). Excludes `name`,
/// `cases` and `min_score`, which change what is judged or how it gates, not
/// what the model is asked.
pub fn judge_hash(judge: &ResolvedJudge) -> String {
    let canonical = json!({
        "criteria": judge.criteria,
        "prompt": judge.prompt,
        "system_prompt": judge.system_prompt,
        "retry_prompt": judge.retry_prompt,
        "model": judge.identity.model,
        "endpoint_host": judge.identity.base_url.as_deref().map(endpoint_host),
        "temperature": judge.identity.temperature,
        "top_p": judge.identity.top_p,
        "max_tokens": judge.identity.max_tokens,
        "max_var_bytes": judge.max_var_bytes,
    });
    sha256_hex(canonical.to_string().as_bytes())
}

/// Identity of one judging: the judge hash plus the exact messages sent.
/// Same key ⇒ nothing to re-ask.
pub fn cache_key(judge_hash: &str, messages: &[LlmMessage]) -> String {
    let rendered = serde_json::to_string(messages).unwrap_or_default();
    sha256_hex(format!("{judge_hash}\n{rendered}").as_bytes())
}

/// The request body as the provider saw it, built explicitly so the key can
/// never leak in through a derive.
pub fn request_record(judge: &ResolvedJudge, messages: &[LlmMessage]) -> Value {
    let mut body = json!({
        "model": judge.identity.model,
        "messages": messages,
        "temperature": judge.identity.temperature,
        "max_tokens": judge.identity.max_tokens,
        "stream": false,
    });
    if let Some(top_p) = judge.identity.top_p {
        body["top_p"] = json!(top_p);
    }
    body
}

pub fn judgments_path(trial_dir: &Path) -> PathBuf {
    trial_dir.join("judgments.json")
}

/// Every judgment on a trial, oldest first. A trial never judged has none.
pub fn read_judgments(trial_dir: &Path) -> Result<Vec<Judgment>, RecordError> {
    let path = judgments_path(trial_dir);
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(RecordError::Io { path, source }),
    };
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&content).map_err(|source| RecordError::Json { path, source })
}

/// Append one judgment; existing elements are never rewritten.
pub fn append_judgment(trial_dir: &Path, judgment: &Judgment) -> Result<(), RecordError> {
    let mut all = read_judgments(trial_dir)?;
    all.push(judgment.clone());
    let path = judgments_path(trial_dir);
    let text = serde_json::to_string_pretty(&all).map_err(|source| RecordError::Json {
        path: path.clone(),
        source,
    })?;
    std::fs::write(&path, text).map_err(|source| RecordError::Io { path, source })
}

/// The judgment that counts for a judge on a trial: the last one appended.
pub fn latest_for<'a>(judgments: &'a [Judgment], judge: &str) -> Option<&'a Judgment> {
    judgments.iter().rev().find(|j| j.judge == judge)
}

pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::judge::config::{Criterion, CriterionKind, JudgeIdentity};
    use tempfile::TempDir;

    fn msg(role: &str, text: &str) -> LlmMessage {
        LlmMessage {
            role: role.to_string(),
            content: Some(text.to_string()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn judge() -> ResolvedJudge {
        ResolvedJudge {
            name: "q".into(),
            cases: None,
            prompt: "{{rubric}} {{output_contract}}".into(),
            system_prompt: None,
            retry_prompt: None,
            min_score: Some(0.5),
            criteria: vec![Criterion {
                name: "c".into(),
                kind: CriterionKind::Scale,
                scale: 5,
                description: "d".into(),
            }],
            identity: JudgeIdentity {
                model: "m".into(),
                base_url: Some("https://user:pw@api.example.com:8443/v1/chat".into()),
                api_key_env: Some("K".into()),
                temperature: 0.0,
                top_p: None,
                max_tokens: 100,
            },
            max_var_bytes: 1000,
            max_retries: 2,
            timeout_secs: 5,
        }
    }

    fn judgment(name: &str, overall: Option<f64>) -> Judgment {
        Judgment {
            schema: JUDGMENT_SCHEMA.into(),
            judge: name.into(),
            judge_hash: "h".into(),
            cache_key: "k".into(),
            identity: JudgmentIdentity {
                model: "m".into(),
                model_reported: None,
                endpoint_host: "h".into(),
                temperature: 0.0,
                top_p: None,
                max_tokens: 1,
            },
            attempts: vec![],
            scores: overall.map(|o| [("overall".to_string(), o)].into_iter().collect()),
            error: None,
            usage: JudgmentUsage::default(),
            cost_usd: None,
            truncated: vec![],
            judged_at: now_rfc3339(),
        }
    }

    #[test]
    fn endpoint_host_strips_scheme_userinfo_path() {
        assert_eq!(
            endpoint_host("https://user:pw@api.example.com:8443/v1/chat?x=1"),
            "api.example.com:8443"
        );
        assert_eq!(endpoint_host("http://127.0.0.1:1234"), "127.0.0.1:1234");
        assert_eq!(endpoint_host("localhost:9/v1"), "localhost:9");
    }

    #[test]
    fn judge_hash_ignores_name_cases_min_score_but_not_prompt_or_model() {
        let base = judge();
        let h = judge_hash(&base);
        let mut same = base.clone();
        same.name = "other".into();
        same.cases = Some(vec!["c1".into()]);
        same.min_score = None;
        assert_eq!(judge_hash(&same), h);
        let mut prompt = base.clone();
        prompt.prompt.push('!');
        assert_ne!(judge_hash(&prompt), h);
        let mut model = base.clone();
        model.identity.model = "m2".into();
        assert_ne!(judge_hash(&model), h);
        let mut host = base.clone();
        host.identity.base_url = Some("https://other.example.com/v1".into());
        assert_ne!(judge_hash(&host), h);
        let mut path_only = base;
        path_only.identity.base_url = Some("https://api.example.com:8443/other".into());
        assert_eq!(judge_hash(&path_only), h, "path is not identity");
    }

    #[test]
    fn cache_key_changes_with_messages() {
        let m1 = vec![msg("user", "a")];
        let m2 = vec![msg("user", "b")];
        assert_ne!(cache_key("h", &m1), cache_key("h", &m2));
        assert_eq!(cache_key("h", &m1), cache_key("h", &m1));
    }

    #[test]
    fn request_record_has_no_key_and_omits_absent_top_p() {
        let j = judge();
        let rec = request_record(&j, &[msg("user", "hi")]);
        let text = rec.to_string();
        assert!(!text.contains("api_key"));
        assert!(!text.contains("base_url"));
        assert_eq!(rec["stream"], json!(false));
        assert_eq!(rec["temperature"], json!(0.0));
        assert!(rec.get("top_p").is_none());
        assert!(rec.get("tools").is_none());
        let mut with = j;
        with.identity.top_p = Some(0.9);
        assert_eq!(request_record(&with, &[])["top_p"], json!(0.9));
    }

    #[test]
    fn judgments_append_and_latest_wins() {
        let dir = TempDir::new().unwrap();
        assert!(read_judgments(dir.path()).unwrap().is_empty());
        append_judgment(dir.path(), &judgment("a", Some(0.2))).unwrap();
        append_judgment(dir.path(), &judgment("b", None)).unwrap();
        append_judgment(dir.path(), &judgment("a", Some(0.9))).unwrap();
        let all = read_judgments(dir.path()).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(latest_for(&all, "a").unwrap().overall(), Some(0.9));
        assert!(!latest_for(&all, "b").unwrap().is_scored());
        assert!(latest_for(&all, "zzz").is_none());
    }

    #[test]
    fn corrupt_judgments_file_is_an_error_not_empty() {
        let dir = TempDir::new().unwrap();
        std::fs::write(judgments_path(dir.path()), "{not json").unwrap();
        assert!(matches!(
            read_judgments(dir.path()),
            Err(RecordError::Json { .. })
        ));
    }

    #[test]
    fn usage_adds_with_fallback_total() {
        let mut u = JudgmentUsage::default();
        u.add(&LlmUsage {
            input_tokens: 3,
            output_tokens: 4,
            total_tokens: None,
        });
        u.add(&LlmUsage {
            input_tokens: 1,
            output_tokens: 0,
            total_tokens: Some(10),
        });
        assert_eq!(
            u,
            JudgmentUsage {
                input: 4,
                output: 4,
                total: 17
            }
        );
    }
}
