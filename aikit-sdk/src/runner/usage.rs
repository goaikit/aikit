//! Source-based aggregation of token-usage entries.
//!
//! Per-Backend *extraction* lives in `backends/<name>.rs`; this module holds the
//! aggregation rule, which is keyed off [`UsageSource`] rather than the Backend.
//! Behaviour is identical to the former `runner/token_usage.rs`.

use super::types::{TokenUsage, UsageSource};

pub(super) fn sum_optional<'a>(vals: impl Iterator<Item = &'a Option<u64>>) -> Option<u64> {
    let collected: Vec<_> = vals.collect();
    if collected.iter().any(|v| v.is_some()) {
        Some(collected.iter().map(|v| v.unwrap_or(0)).sum())
    } else {
        None
    }
}

/// Aggregate a sequence of token usage entries using the per-source rule.
///
/// - **Codex**: sum all entries (multiple `turn.completed` messages)
/// - **All others**: take the last entry (final `result` / `step_finish`)
///
/// Returns `None` when `usage_entries` is empty.
pub fn aggregate_token_usage(
    usage_entries: &[(TokenUsage, UsageSource)],
    source: UsageSource,
) -> Option<TokenUsage> {
    if usage_entries.is_empty() {
        return None;
    }
    match source {
        UsageSource::Codex => {
            let input_tokens = usage_entries.iter().map(|(u, _)| u.input_tokens).sum();
            let output_tokens = usage_entries.iter().map(|(u, _)| u.output_tokens).sum();
            let total_tokens = sum_optional(usage_entries.iter().map(|(u, _)| &u.total_tokens));
            let cache_read_tokens =
                sum_optional(usage_entries.iter().map(|(u, _)| &u.cache_read_tokens));
            let cache_creation_tokens =
                sum_optional(usage_entries.iter().map(|(u, _)| &u.cache_creation_tokens));
            let reasoning_tokens =
                sum_optional(usage_entries.iter().map(|(u, _)| &u.reasoning_tokens));
            Some(TokenUsage {
                input_tokens,
                output_tokens,
                total_tokens,
                cache_read_tokens,
                cache_creation_tokens,
                reasoning_tokens,
            })
        }
        _ => usage_entries.last().map(|(u, _)| u.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aggregate_codex_sums_all_entries() {
        let entries = vec![
            (
                TokenUsage {
                    input_tokens: 100,
                    output_tokens: 10,
                    total_tokens: None,
                    cache_read_tokens: Some(50),
                    cache_creation_tokens: None,
                    reasoning_tokens: None,
                },
                UsageSource::Codex,
            ),
            (
                TokenUsage {
                    input_tokens: 200,
                    output_tokens: 20,
                    total_tokens: None,
                    cache_read_tokens: Some(75),
                    cache_creation_tokens: None,
                    reasoning_tokens: None,
                },
                UsageSource::Codex,
            ),
        ];
        let result = aggregate_token_usage(&entries, UsageSource::Codex).unwrap();
        assert_eq!(result.input_tokens, 300);
        assert_eq!(result.output_tokens, 30);
        assert_eq!(result.cache_read_tokens, Some(125));
    }

    #[test]
    fn test_aggregate_claude_takes_last() {
        let entries = vec![
            (
                TokenUsage {
                    input_tokens: 10,
                    output_tokens: 1,
                    total_tokens: None,
                    cache_read_tokens: None,
                    cache_creation_tokens: None,
                    reasoning_tokens: None,
                },
                UsageSource::Claude,
            ),
            (
                TokenUsage {
                    input_tokens: 99,
                    output_tokens: 7,
                    total_tokens: None,
                    cache_read_tokens: Some(500),
                    cache_creation_tokens: None,
                    reasoning_tokens: None,
                },
                UsageSource::Claude,
            ),
        ];
        let result = aggregate_token_usage(&entries, UsageSource::Claude).unwrap();
        assert_eq!(result.input_tokens, 99);
        assert_eq!(result.output_tokens, 7);
        assert_eq!(result.cache_read_tokens, Some(500));
    }

    #[test]
    fn test_aggregate_empty_returns_none() {
        assert!(aggregate_token_usage(&[], UsageSource::Codex).is_none());
        assert!(aggregate_token_usage(&[], UsageSource::Claude).is_none());
    }
}
