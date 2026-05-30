use super::types::{AgentEventPayload, QuotaCategory, QuotaExceededInfo};

pub(super) fn infer_quota_category(msg: &str) -> QuotaCategory {
    let lower = msg.to_lowercase();
    if lower.contains("hour") {
        QuotaCategory::Hourly
    } else if lower.contains("month") || lower.contains("monthly") {
        QuotaCategory::Unknown
    } else if lower.contains("per day")
        || lower.contains("daily")
        || lower.contains(" day ")
        || lower.ends_with(" day")
        || lower.starts_with("day ")
        || lower.contains("day,")
    {
        QuotaCategory::Daily
    } else if lower.contains("week") {
        QuotaCategory::Weekly
    } else if lower.contains("long context") || lower.contains("token") {
        QuotaCategory::Tokens
    } else if lower.contains("request") {
        QuotaCategory::Requests
    } else {
        QuotaCategory::Unknown
    }
}

pub(super) fn truncate_message(msg: &str, max_len: usize) -> String {
    if msg.len() <= max_len {
        msg.to_string()
    } else {
        let mut end = max_len;
        while !msg.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        msg[..end].to_string()
    }
}

fn raw_info(agent_key: &str, text: &str) -> QuotaExceededInfo {
    QuotaExceededInfo {
        agent_key: agent_key.to_string(),
        category: infer_quota_category(text),
        raw_message: truncate_message(text, 500),
    }
}

fn msg_info(agent_key: &str, msg: &str) -> QuotaExceededInfo {
    QuotaExceededInfo {
        agent_key: agent_key.to_string(),
        category: infer_quota_category(msg),
        raw_message: truncate_message(msg, 500),
    }
}

pub(super) fn extract_nested_rate_limit_error(val: &serde_json::Value) -> Option<String> {
    let error_obj = val.get("error")?;
    if error_obj.get("type").and_then(|v| v.as_str()) == Some("rate_limit_error") {
        return error_obj
            .get("message")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    if error_obj.get("code").and_then(|v| v.as_str()) == Some("rate_limit_error") {
        return error_obj
            .get("message")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    None
}

pub(super) fn find_gemini_error_object(val: &serde_json::Value) -> Option<String> {
    let error = if val.is_array() {
        val.get(0)?.get("error")
    } else {
        val.get("error")
    };
    let error = error?;
    let code_429 = error.get("code").and_then(|v| v.as_u64()) == Some(429);
    let status_exhausted =
        error.get("status").and_then(|v| v.as_str()) == Some("RESOURCE_EXHAUSTED");
    if !code_429 && !status_exhausted {
        return None;
    }
    let msg = error.get("message").and_then(|v| v.as_str()).unwrap_or("");
    Some(if msg.is_empty() {
        error.to_string()
    } else {
        msg.to_string()
    })
}

// ---------------------------------------------------------------------------
// Pattern types
// ---------------------------------------------------------------------------

enum RawPat {
    Any(&'static [&'static str]),
    All(&'static [&'static str]),
    KeywordJsonFallback(&'static str),
    StartsWithJsonFallback(&'static str),
    Contains429JsonRateLimit,
    AgentStructuredLog,
}

enum JsonPat {
    ErrorRateLimit,
    ErrorMsgAny(&'static [&'static str]),
    ResultErrorMsgAny(&'static [&'static str]),
    ArrayErrorRateLimit,
    GeminiErrorObject,
    NestedCode { code: &'static str },
    CodexJsonError,
}

struct AgentQuotaSpec {
    raw_pats: &'static [RawPat],
    json_pats: &'static [JsonPat],
}

static CLAUDE_RAW: &[RawPat] = &[
    RawPat::KeywordJsonFallback("Failed to load usage data"),
    RawPat::Contains429JsonRateLimit,
    RawPat::All(&["api error:", "rate limit reached"]),
    RawPat::All(&["api error:", "rate limited"]),
    RawPat::All(&["api error:", "request rejected", "429"]),
    RawPat::Any(&["you've hit your limit", "you've hit your usage limit"]),
    RawPat::All(&["hit your limit", "reset"]),
    RawPat::Any(&["http 429"]),
    RawPat::All(&["429", "rate_limit_error"]),
    RawPat::StartsWithJsonFallback("Error: 429"),
    RawPat::Any(&["usage limit", "rate limit"]),
];

static CLAUDE_JSON: &[JsonPat] = &[
    JsonPat::ErrorRateLimit,
    JsonPat::ResultErrorMsgAny(&["usage", "limit"]),
    JsonPat::ArrayErrorRateLimit,
];

static CODEX_RAW: &[RawPat] = &[RawPat::Any(&[
    "rate limit reached",
    "tokens per min",
    "429 too many requests",
    "rate_limit_exceeded",
])];

static CODEX_JSON: &[JsonPat] = &[JsonPat::CodexJsonError];

static GEMINI_RAW: &[RawPat] = &[
    RawPat::Any(&["resource_exhausted"]),
    RawPat::Any(&["rate limit exceeded"]),
    RawPat::All(&["429", "quota exceeded"]),
    RawPat::All(&["429", "rate limit"]),
    RawPat::All(&["error", "429", "rate limit"]),
    RawPat::All(&["error", "429", "'code'"]),
];

static GEMINI_JSON: &[JsonPat] = &[JsonPat::GeminiErrorObject];

static OPENCODE_RAW: &[RawPat] = &[
    RawPat::Any(&[
        "rate-limited",
        "daily token quota exceeded",
        "insufficient_quota",
    ]),
    RawPat::All(&["too many requests", "quota exceeded"]),
];

static OPENCODE_JSON: &[JsonPat] = &[
    JsonPat::NestedCode {
        code: "insufficient_quota",
    },
    JsonPat::ErrorMsgAny(&["quota", "rate limit", "insufficient_quota", "429"]),
];

static AGENT_RAW: &[RawPat] = &[
    RawPat::AgentStructuredLog,
    RawPat::Any(&["you've hit your usage limit", "usage limit for"]),
];

static AGENT_JSON: &[JsonPat] = &[JsonPat::ErrorMsgAny(&[
    "rate limit",
    "quota exceeded",
    "usage limit",
])];

static QUOTA_SPECS: &[(&str, AgentQuotaSpec)] = &[
    (
        "claude",
        AgentQuotaSpec {
            raw_pats: CLAUDE_RAW,
            json_pats: CLAUDE_JSON,
        },
    ),
    (
        "codex",
        AgentQuotaSpec {
            raw_pats: CODEX_RAW,
            json_pats: CODEX_JSON,
        },
    ),
    (
        "gemini",
        AgentQuotaSpec {
            raw_pats: GEMINI_RAW,
            json_pats: GEMINI_JSON,
        },
    ),
    (
        "opencode",
        AgentQuotaSpec {
            raw_pats: OPENCODE_RAW,
            json_pats: OPENCODE_JSON,
        },
    ),
    (
        "agent",
        AgentQuotaSpec {
            raw_pats: AGENT_RAW,
            json_pats: AGENT_JSON,
        },
    ),
];

fn get_quota_spec(key: &str) -> Option<&'static AgentQuotaSpec> {
    QUOTA_SPECS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, spec)| spec)
}

// ---------------------------------------------------------------------------
// Pattern matching
// ---------------------------------------------------------------------------

fn try_json_rate_limit_from_text(text: &str, start: usize) -> Option<String> {
    let json_fragment = &text[start..];
    let val = serde_json::from_str::<serde_json::Value>(json_fragment).ok()?;
    extract_nested_rate_limit_error(&val)
}

fn match_raw_pat(pat: &RawPat, agent_key: &str, text: &str) -> Option<QuotaExceededInfo> {
    let lower = text.to_lowercase();
    match pat {
        RawPat::Any(kws) => {
            if kws.iter().any(|k| lower.contains(k)) {
                Some(raw_info(agent_key, text))
            } else {
                None
            }
        }
        RawPat::All(kws) => {
            if kws.iter().all(|k| lower.contains(k)) {
                Some(raw_info(agent_key, text))
            } else {
                None
            }
        }
        RawPat::KeywordJsonFallback(keyword) => {
            let idx = text.find(keyword)?;
            if let Some(brace) = text[idx..].find('{') {
                if let Some(msg) = try_json_rate_limit_from_text(text, idx + brace) {
                    return Some(msg_info(agent_key, &msg));
                }
            }
            Some(raw_info(agent_key, text))
        }
        RawPat::StartsWithJsonFallback(prefix) => {
            if !text.starts_with(prefix) {
                return None;
            }
            if let Some(brace) = text.find('{') {
                if let Some(msg) = try_json_rate_limit_from_text(text, brace) {
                    return Some(msg_info(agent_key, &msg));
                }
            }
            Some(raw_info(agent_key, text))
        }
        RawPat::Contains429JsonRateLimit => {
            if text.contains("429") {
                if let Some(brace) = text.find('{') {
                    if let Some(msg) = try_json_rate_limit_from_text(text, brace) {
                        return Some(msg_info(agent_key, &msg));
                    }
                }
            }
            None
        }
        RawPat::AgentStructuredLog => {
            if !text.contains("structured-log.info") {
                return None;
            }
            let brace = text.find('{')?;
            let val = serde_json::from_str::<serde_json::Value>(&text[brace..]).ok()?;
            let metadata = val.get("metadata")?;
            if metadata.get("outcome").and_then(|v| v.as_str()) != Some("error") {
                return None;
            }
            let grpc = metadata
                .get("grpc_code")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            let err_txt = metadata
                .get("error_text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            if grpc.contains("resource_exhausted")
                || err_txt.contains("usage limit")
                || (err_txt.contains("limit")
                    && (err_txt.contains("slow pool") || err_txt.contains("opus")))
            {
                let raw_msg = metadata
                    .get("error_text")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&text[brace..]);
                Some(raw_info(agent_key, raw_msg))
            } else {
                None
            }
        }
    }
}

fn match_json_pat(
    pat: &JsonPat,
    agent_key: &str,
    val: &serde_json::Value,
) -> Option<QuotaExceededInfo> {
    match pat {
        JsonPat::ErrorRateLimit => {
            if val.get("type").and_then(|v| v.as_str()) == Some("error") {
                extract_nested_rate_limit_error(val).map(|msg| msg_info(agent_key, &msg))
            } else {
                None
            }
        }
        JsonPat::ErrorMsgAny(kws) => {
            if val.get("type").and_then(|v| v.as_str()) != Some("error") {
                return None;
            }
            let msg = val.get("message").and_then(|v| v.as_str()).unwrap_or("");
            let msg_lower = msg.to_lowercase();
            if kws.iter().any(|k| msg_lower.contains(k)) {
                Some(QuotaExceededInfo {
                    agent_key: agent_key.to_string(),
                    category: infer_quota_category(msg),
                    raw_message: if msg.is_empty() {
                        truncate_message(&val.to_string(), 500)
                    } else {
                        truncate_message(msg, 500)
                    },
                })
            } else {
                None
            }
        }
        JsonPat::ResultErrorMsgAny(kws) => {
            if val.get("type").and_then(|v| v.as_str()) != Some("result") {
                return None;
            }
            let is_error = val.get("subtype").and_then(|v| v.as_str()) == Some("error")
                || val.get("is_error").and_then(|v| v.as_bool()) == Some(true);
            if !is_error {
                return None;
            }
            if let Some(msg) = val.get("message").and_then(|v| v.as_str()) {
                let ml = msg.to_lowercase();
                if kws.iter().any(|k| ml.contains(k)) {
                    return Some(msg_info(agent_key, msg));
                }
            }
            if let Some(r) = val.get("result").and_then(|v| v.as_str()) {
                let rl = r.to_lowercase();
                if kws.iter().any(|k| rl.contains(k)) {
                    return Some(msg_info(agent_key, r));
                }
            }
            None
        }
        JsonPat::ArrayErrorRateLimit => {
            if !val.is_array() {
                return None;
            }
            let first = val.get(0)?;
            let error_val = first.get("error")?;
            if error_val.is_string() {
                if let Ok(parsed) =
                    serde_json::from_str::<serde_json::Value>(error_val.as_str().unwrap_or(""))
                {
                    if let Some(m) = extract_nested_rate_limit_error(&parsed) {
                        return Some(msg_info(agent_key, &m));
                    }
                }
            } else if let Some(m) = extract_nested_rate_limit_error(error_val) {
                return Some(msg_info(agent_key, &m));
            }
            None
        }
        JsonPat::GeminiErrorObject => {
            find_gemini_error_object(val).map(|o| msg_info(agent_key, &o))
        }
        JsonPat::NestedCode { code } => {
            if val.get("type").and_then(|v| v.as_str()) != Some("error") {
                return None;
            }
            let error = val.get("error")?;
            if error.get("type").and_then(|v| v.as_str()) == Some(code)
                || error.get("code").and_then(|v| v.as_str()) == Some(code)
            {
                let msg = error.get("message").and_then(|v| v.as_str()).unwrap_or("");
                return Some(QuotaExceededInfo {
                    agent_key: agent_key.to_string(),
                    category: infer_quota_category(msg),
                    raw_message: if msg.is_empty() {
                        truncate_message(&val.to_string(), 500)
                    } else {
                        truncate_message(msg, 500)
                    },
                });
            }
            None
        }
        JsonPat::CodexJsonError => {
            if val.get("type").and_then(|v| v.as_str()) != Some("error") {
                return None;
            }
            let code_match =
                val.get("code").and_then(|v| v.as_str()) == Some("rate_limit_exceeded");
            let msg = val.get("message").and_then(|v| v.as_str()).unwrap_or("");
            let msg_match = msg.to_lowercase().contains("rate limit");
            if code_match || msg_match {
                let raw = if msg.is_empty() {
                    truncate_message(&val.to_string(), 500)
                } else {
                    truncate_message(msg, 500)
                };
                return Some(QuotaExceededInfo {
                    agent_key: agent_key.to_string(),
                    category: infer_quota_category(msg),
                    raw_message: raw,
                });
            }
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn extract_quota_signal(
    agent_key: &str,
    payload: &AgentEventPayload,
) -> Option<QuotaExceededInfo> {
    let spec = get_quota_spec(agent_key)?;
    match payload {
        AgentEventPayload::RawLine(text) => {
            for pat in spec.raw_pats {
                if let Some(info) = match_raw_pat(pat, agent_key, text) {
                    return Some(info);
                }
            }
            None
        }
        AgentEventPayload::JsonLine(val) => {
            for pat in spec.json_pats {
                if let Some(info) = match_json_pat(pat, agent_key, val) {
                    return Some(info);
                }
            }
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::types::{QuotaCategory, QuotaExceededInfo, RunError, RunResult};

    #[cfg(unix)]
    use std::os::unix::process::ExitStatusExt;
    #[cfg(windows)]
    use std::os::windows::process::ExitStatusExt;

    #[test]
    fn test_quota_category_serde_roundtrip() {
        let cats = vec![
            QuotaCategory::Hourly,
            QuotaCategory::Daily,
            QuotaCategory::Weekly,
            QuotaCategory::Requests,
            QuotaCategory::Tokens,
            QuotaCategory::Unknown,
        ];
        for cat in &cats {
            let json = serde_json::to_string(cat).unwrap();
            let back: QuotaCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(*cat, back);
        }
    }

    #[test]
    fn test_quota_exceeded_info_serde_roundtrip() {
        let info = QuotaExceededInfo {
            agent_key: "claude".to_string(),
            category: QuotaCategory::Hourly,
            raw_message: "usage limit".to_string(),
        };
        let json = serde_json::to_string(&info).unwrap();
        let back: QuotaExceededInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info, back);
    }

    #[test]
    fn test_run_result_new_has_quota_exceeded_none() {
        let status = std::process::ExitStatus::from_raw(0);
        let result = RunResult::new(status, vec![], vec![]);
        assert!(result.quota_exceeded.is_none());
    }

    #[test]
    fn test_run_error_quota_exceeded_display() {
        let info = QuotaExceededInfo {
            agent_key: "claude".to_string(),
            category: QuotaCategory::Hourly,
            raw_message: "limit reached".to_string(),
        };
        let err = RunError::QuotaExceeded(info);
        let msg = format!("{}", err);
        assert!(msg.contains("claude"));
        assert!(msg.contains("quota exceeded"));
        assert!(msg.contains("limit reached"));
    }

    #[test]
    fn test_infer_quota_category_hourly() {
        assert_eq!(
            infer_quota_category("hourly limit reached"),
            QuotaCategory::Hourly
        );
        assert_eq!(
            infer_quota_category("reset in 1 hour"),
            QuotaCategory::Hourly
        );
    }

    #[test]
    fn test_infer_quota_category_weekly() {
        assert_eq!(
            infer_quota_category("weekly quota exceeded"),
            QuotaCategory::Weekly
        );
        assert_eq!(
            infer_quota_category("resets next week"),
            QuotaCategory::Weekly
        );
    }

    #[test]
    fn test_infer_quota_category_long_context_tokens() {
        assert_eq!(
            infer_quota_category("Extra usage is required for long context requests"),
            QuotaCategory::Tokens
        );
    }

    #[test]
    fn test_infer_quota_category_unknown() {
        assert_eq!(
            infer_quota_category("something went wrong"),
            QuotaCategory::Unknown
        );
        assert_eq!(
            infer_quota_category("monthly billing cycle"),
            QuotaCategory::Unknown
        );
    }

    #[test]
    fn test_extract_quota_signal_claude_rawline_usage_limit() {
        let payload = AgentEventPayload::RawLine(
            "Claude usage limit reached. Your limit will reset at 5 PM.".to_string(),
        );
        let info = extract_quota_signal("claude", &payload).unwrap();
        assert_eq!(info.agent_key, "claude");
        assert_eq!(info.category, QuotaCategory::Unknown);
    }

    #[test]
    fn test_extract_quota_signal_claude_rawline_rate_limit_hourly() {
        let payload = AgentEventPayload::RawLine("Rate limit hit for hourly usage".to_string());
        let info = extract_quota_signal("claude", &payload).unwrap();
        assert_eq!(info.agent_key, "claude");
        assert_eq!(info.category, QuotaCategory::Hourly);
    }

    #[test]
    fn test_extract_quota_signal_claude_failed_to_load_usage_data() {
        let payload = AgentEventPayload::RawLine(
            r#"Error: Failed to load usage data: {"error":{"type":"rate_limit_error","message":"Rate limited. Please try again later."}}"#.to_string(),
        );
        let info = extract_quota_signal("claude", &payload).unwrap();
        assert_eq!(info.agent_key, "claude");
        assert!(info.raw_message.contains("Rate limited"));
    }

    #[test]
    fn test_extract_quota_signal_claude_api_error_rate_limit_reached() {
        let payload = AgentEventPayload::RawLine("API Error: Rate limit reached".to_string());
        let info = extract_quota_signal("claude", &payload).unwrap();
        assert_eq!(info.agent_key, "claude");
    }

    #[test]
    fn test_extract_quota_signal_claude_api_error_429() {
        let payload = AgentEventPayload::RawLine(
            "API Error: Request rejected (429) · Rate limited".to_string(),
        );
        let info = extract_quota_signal("claude", &payload).unwrap();
        assert_eq!(info.agent_key, "claude");
    }

    #[test]
    fn test_extract_quota_signal_claude_hit_your_limit() {
        let payload = AgentEventPayload::RawLine(
            "⎿ You've hit your limit · resets 10am (Asia/Manila)".to_string(),
        );
        let info = extract_quota_signal("claude", &payload).unwrap();
        assert_eq!(info.agent_key, "claude");
    }

    #[test]
    fn test_extract_quota_signal_claude_http_429_rate_limit_error() {
        let payload = AgentEventPayload::RawLine(
            "HTTP 429: rate_limit_error: This request would exceed your account's rate limit."
                .to_string(),
        );
        let info = extract_quota_signal("claude", &payload).unwrap();
        assert_eq!(info.agent_key, "claude");
    }

    #[test]
    fn test_extract_quota_signal_claude_error_429_json() {
        let payload = AgentEventPayload::RawLine(
            r#"Error: 429 {"type":"error","error":{"type":"rate_limit_error","message":"Extra usage is required for long context requests."},"request_id":"req_abc123"}"#.to_string(),
        );
        let info = extract_quota_signal("claude", &payload).unwrap();
        assert_eq!(info.agent_key, "claude");
        assert_eq!(info.category, QuotaCategory::Tokens);
    }

    #[test]
    fn test_extract_quota_signal_claude_json_type_error_rate_limit() {
        let payload = AgentEventPayload::JsonLine(serde_json::from_str(
            r#"{"type":"error","error":{"type":"rate_limit_error","message":"Rate limited. Please try again later."}}"#,
        ).unwrap());
        let info = extract_quota_signal("claude", &payload).unwrap();
        assert_eq!(info.agent_key, "claude");
    }

    #[test]
    fn test_extract_quota_signal_claude_json_result_error_usage() {
        let payload = AgentEventPayload::JsonLine(
            serde_json::from_str(
                r#"{"type":"result","subtype":"error","message":"usage limit reached"}"#,
            )
            .unwrap(),
        );
        let info = extract_quota_signal("claude", &payload).unwrap();
        assert_eq!(info.agent_key, "claude");
    }

    #[test]
    fn test_extract_quota_signal_codex_rate_limit_code() {
        let payload = AgentEventPayload::JsonLine(serde_json::from_str(
            r#"{"type":"error","code":"rate_limit_exceeded","message":"You have exceeded your request rate limit"}"#,
        ).unwrap());
        let info = extract_quota_signal("codex", &payload).unwrap();
        assert_eq!(info.agent_key, "codex");
    }

    #[test]
    fn test_extract_quota_signal_codex_rawline_tpm() {
        let payload = AgentEventPayload::RawLine(
            "stream disconnected before completion: Rate limit reached for organization org-abc on tokens per min (TPM): Limit 250000, Used 250000".to_string(),
        );
        let info = extract_quota_signal("codex", &payload).unwrap();
        assert_eq!(info.agent_key, "codex");
    }

    #[test]
    fn test_extract_quota_signal_codex_rawline_429() {
        let payload = AgentEventPayload::RawLine(
            "error: http 429 Too Many Requests: rate_limit_exceeded".to_string(),
        );
        let info = extract_quota_signal("codex", &payload).unwrap();
        assert_eq!(info.agent_key, "codex");
    }

    #[test]
    fn test_extract_quota_signal_gemini_resource_exhausted() {
        let payload = AgentEventPayload::JsonLine(serde_json::from_str(
            r#"{"error":{"code":429,"status":"RESOURCE_EXHAUSTED","message":"Quota exceeded"}}"#,
        ).unwrap());
        let info = extract_quota_signal("gemini", &payload).unwrap();
        assert_eq!(info.agent_key, "gemini");
        assert_eq!(info.category, QuotaCategory::Unknown);
    }

    #[test]
    fn test_extract_quota_signal_gemini_rawline_error_429() {
        let payload = AgentEventPayload::RawLine(
            "prompt 1: ERROR {'code': 429, 'message': 'Rate limit exceeded. Try again later.'}"
                .to_string(),
        );
        let info = extract_quota_signal("gemini", &payload).unwrap();
        assert_eq!(info.agent_key, "gemini");
    }

    #[test]
    fn test_extract_quota_signal_opencode_quota_message() {
        let payload = AgentEventPayload::JsonLine(
            serde_json::from_str(r#"{"type":"error","message":"weekly quota exceeded"}"#).unwrap(),
        );
        let info = extract_quota_signal("opencode", &payload).unwrap();
        assert_eq!(info.agent_key, "opencode");
        assert_eq!(info.category, QuotaCategory::Weekly);
    }

    #[test]
    fn test_extract_quota_signal_opencode_insufficient_quota_json() {
        let payload = AgentEventPayload::JsonLine(serde_json::from_str(
            r#"{"type":"error","sequence_number":2,"error":{"type":"insufficient_quota","code":"insufficient_quota","message":"You exceeded your current quota.","param":null}}"#,
        ).unwrap());
        let info = extract_quota_signal("opencode", &payload).unwrap();
        assert_eq!(info.agent_key, "opencode");
    }

    #[test]
    fn test_extract_quota_signal_opencode_rawline_daily_token() {
        let payload = AgentEventPayload::RawLine("Your daily token quota exceeded".to_string());
        let info = extract_quota_signal("opencode", &payload).unwrap();
        assert_eq!(info.agent_key, "opencode");
        assert_eq!(info.category, QuotaCategory::Daily);
    }

    #[test]
    fn test_extract_quota_signal_opencode_rawline_rate_limited() {
        let payload = AgentEventPayload::RawLine("You are rate-limited".to_string());
        let info = extract_quota_signal("opencode", &payload).unwrap();
        assert_eq!(info.agent_key, "opencode");
    }

    #[test]
    fn test_extract_quota_signal_agent_rate_limit() {
        let payload = AgentEventPayload::JsonLine(
            serde_json::from_str(
                r#"{"type":"error","message":"Rate limit exceeded for hourly requests"}"#,
            )
            .unwrap(),
        );
        let info = extract_quota_signal("agent", &payload).unwrap();
        assert_eq!(info.agent_key, "agent");
        assert_eq!(info.category, QuotaCategory::Hourly);
    }

    #[test]
    fn test_extract_quota_signal_agent_structured_log_resource_exhausted() {
        let payload = AgentEventPayload::RawLine(
            r#"structured-log.info {"message":"agent_cli.turn.outcome","metadata":{"outcome":"error","grpc_code":"resource_exhausted","error_text":"Usage limit for slow pool"}}"#.to_string(),
        );
        let info = extract_quota_signal("agent", &payload).unwrap();
        assert_eq!(info.agent_key, "agent");
    }

    #[test]
    fn test_extract_quota_signal_agent_rawline_usage_limit() {
        let payload = AgentEventPayload::RawLine(
            "b: You've hit your usage limit for Opus. Switch to Auto.".to_string(),
        );
        let info = extract_quota_signal("agent", &payload).unwrap();
        assert_eq!(info.agent_key, "agent");
    }

    #[test]
    fn test_extract_quota_signal_no_match_returns_none() {
        let payload = AgentEventPayload::RawLine("Normal output line".to_string());
        assert!(extract_quota_signal("claude", &payload).is_none());
        assert!(extract_quota_signal("codex", &payload).is_none());
        assert!(extract_quota_signal("gemini", &payload).is_none());
        assert!(extract_quota_signal("opencode", &payload).is_none());
        assert!(extract_quota_signal("agent", &payload).is_none());
    }

    #[test]
    fn test_extract_quota_signal_unknown_agent_returns_none() {
        let payload = AgentEventPayload::RawLine("Rate limit reached".to_string());
        assert!(extract_quota_signal("copilot", &payload).is_none());
    }
}
