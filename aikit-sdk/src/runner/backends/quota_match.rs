//! Shared quota-signal matching infrastructure.
//!
//! The *pattern tables* are per-Backend and live in each `backends/<name>.rs`.
//! The matching *machinery* below is generic and shared. Behaviour is identical
//! to the former `runner/quota.rs`.

use crate::runner::types::{QuotaCategory, QuotaExceededInfo};

pub(crate) fn infer_quota_category(msg: &str) -> QuotaCategory {
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

pub(crate) fn truncate_message(msg: &str, max_len: usize) -> String {
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

pub(crate) fn extract_nested_rate_limit_error(val: &serde_json::Value) -> Option<String> {
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

fn find_gemini_error_object(val: &serde_json::Value) -> Option<String> {
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
// Pattern types (per-Backend tables are built from these)
// ---------------------------------------------------------------------------

pub(crate) enum RawPat {
    Any(&'static [&'static str]),
    All(&'static [&'static str]),
    KeywordJsonFallback(&'static str),
    StartsWithJsonFallback(&'static str),
    Contains429JsonRateLimit,
    AgentStructuredLog,
}

pub(crate) enum JsonPat {
    ErrorRateLimit,
    ErrorMsgAny(&'static [&'static str]),
    ResultErrorMsgAny(&'static [&'static str]),
    ArrayErrorRateLimit,
    GeminiErrorObject,
    NestedCode { code: &'static str },
    CodexJsonError,
}

fn try_json_rate_limit_from_text(text: &str, start: usize) -> Option<String> {
    let json_fragment = &text[start..];
    let val = serde_json::from_str::<serde_json::Value>(json_fragment).ok()?;
    extract_nested_rate_limit_error(&val)
}

pub(crate) fn match_raw_pat(
    pat: &RawPat,
    agent_key: &str,
    text: &str,
) -> Option<QuotaExceededInfo> {
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

pub(crate) fn match_json_pat(
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

/// Run a Backend's raw + json pattern tables against a payload.
pub(crate) fn match_quota(
    agent_key: &str,
    raw_pats: &[RawPat],
    json_pats: &[JsonPat],
    payload: &crate::runner::types::AgentEventPayload,
) -> Option<QuotaExceededInfo> {
    use crate::runner::types::AgentEventPayload;
    match payload {
        AgentEventPayload::RawLine(text) => {
            for pat in raw_pats {
                if let Some(info) = match_raw_pat(pat, agent_key, text) {
                    return Some(info);
                }
            }
            None
        }
        AgentEventPayload::JsonLine(val) => {
            for pat in json_pats {
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
    fn test_truncate_message_respects_char_boundary() {
        let s = "héllo wörld";
        let t = truncate_message(s, 3);
        assert!(s.starts_with(&t));
        assert!(t.len() <= 3);
    }
}
