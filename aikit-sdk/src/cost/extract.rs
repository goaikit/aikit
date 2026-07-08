//! Cost extraction: the pull path from adapter-emitted `TokenEvent`s (spec
//! 010 §16).
//!
//! [`client_computed_cost`] is a **fallback only** — called when the
//! provider-side cost signal is absent AND `passive_capture` capability is
//! true. Produces a [`CostSnapshot`] with `ClientComputed` provenance and
//! `is_estimate == true`.

use crate::cost::{CostProvenance, CostSnapshot, PricingTable, Spend, SpendScope};
use crate::runner::backend::Backend;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Map a Backend to the ToolKind an adapter would carry.
/// Returns `None` for Backends with no adapter compiled in.
fn backend_to_tool_kind(b: Backend) -> Option<aikit_session_capture::ToolKind> {
    match b {
        #[cfg(feature = "claudecode")]
        Backend::Claude => Some(aikit_session_capture::ToolKind::ClaudeCode),
        #[cfg(feature = "codex")]
        Backend::Codex => Some(aikit_session_capture::ToolKind::Codex),
        #[cfg(feature = "opencode")]
        Backend::OpenCode => Some(aikit_session_capture::ToolKind::OpenCode),
        _ => None,
    }
}

/// Compute cost from adapter-emitted `TokenEvent`s. Fallback only — called
/// when the provider-side cost signal is absent (spec 010 §16).
///
/// Produces a [`CostSnapshot`] with `ClientComputed` provenance and
/// `is_estimate == true` (spec 009 §5 invariant).
pub async fn client_computed_cost(
    store: &dyn aikit_session_capture::EventStore,
    backend: Backend,
    session_id: &str,
    pricing: &PricingTable,
) -> Option<CostSnapshot> {
    let tool_kind = backend_to_tool_kind(backend)?;
    let tokens = store
        .token_events_for_session(tool_kind, session_id)
        .await
        .ok()?;
    if tokens.is_empty() {
        return None;
    }
    let spend_usd = pricing.estimate(&tokens)?;
    let per_model = pricing.per_model_breakdown(&tokens);

    Some(CostSnapshot {
        backend: backend.key().to_string(),
        captured_at_ms: now_ms(),
        windows: vec![],
        spend: Some(Spend {
            amount_usd: spend_usd,
            scope: SpendScope::Session,
            is_estimate: true,
        }),
        per_model,
        credits: None,
        provenance: CostProvenance::ClientComputed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cost::PricingTable;

    #[cfg(all(feature = "agent-adapters", feature = "claudecode"))]
    #[tokio::test]
    async fn client_computed_cost_produces_estimate() {
        use aikit_session_capture::{EventBatch, EventStore, InMemoryEventStore, TokenEvent};

        let store = InMemoryEventStore::new();
        let token_ev = TokenEvent {
            source_event_id: "tk_1".into(),
            session_id: "s1".into(),
            tool: aikit_session_capture::ToolKind::ClaudeCode,
            model: Some("claude-sonnet-4-20250514".into()),
            request_id: None,
            input_tokens: Some(1000),
            cache_read_tokens: Some(500),
            cache_creation_tokens: None,
            cache_creation_1h_tokens: None,
            output_tokens: Some(200),
            reasoning_tokens: None,
            captured_at_ms: 1000,
            captured_via: aikit_session_capture::CaptureSource::Transcript,
        };
        store
            .upsert_events(EventBatch {
                tool_events: vec![],
                token_events: vec![token_ev],
                cache_observations: vec![],
            })
            .await
            .unwrap();

        let pricing = PricingTable::default();
        let snapshot = client_computed_cost(&store, Backend::Claude, "s1", &pricing).await;

        assert!(snapshot.is_some(), "should produce a CostSnapshot");
        let snapshot = snapshot.unwrap();
        assert_eq!(
            snapshot.provenance,
            CostProvenance::ClientComputed,
            "provenance MUST be ClientComputed"
        );
        assert!(
            snapshot.spend.as_ref().unwrap().is_estimate,
            "is_estimate MUST be true for ClientComputed (spec 009 §5)"
        );
        assert!(
            snapshot.spend.as_ref().unwrap().amount_usd > 0.0,
            "cost should be non-zero"
        );
    }

    #[test]
    fn pricing_estimate_sums_all_events() {
        use aikit_session_capture::{CaptureSource, TokenEvent};

        let tokens = vec![
            TokenEvent {
                source_event_id: "1".into(),
                session_id: "s1".into(),
                tool: aikit_session_capture::ToolKind::ClaudeCode,
                model: Some("default".into()),
                request_id: None,
                input_tokens: Some(1_000_000),
                cache_read_tokens: None,
                cache_creation_tokens: None,
                cache_creation_1h_tokens: None,
                output_tokens: Some(1_000_000),
                reasoning_tokens: None,
                captured_at_ms: 0,
                captured_via: CaptureSource::Transcript,
            },
            TokenEvent {
                source_event_id: "2".into(),
                session_id: "s1".into(),
                tool: aikit_session_capture::ToolKind::ClaudeCode,
                model: Some("default".into()),
                request_id: None,
                input_tokens: Some(500_000),
                cache_read_tokens: None,
                cache_creation_tokens: None,
                cache_creation_1h_tokens: None,
                output_tokens: Some(500_000),
                reasoning_tokens: None,
                captured_at_ms: 0,
                captured_via: CaptureSource::Transcript,
            },
        ];
        let pricing = PricingTable::default();
        // 3M input * $3/Mtok + 3M output * $15/Mtok = $9 + $45 = $54 wait...
        // Actually: (1M+0.5M) input = 1.5M * $3 = $4.50
        //           (1M+0.5M) output = 1.5M * $15 = $22.50
        // Total = $27.00
        let cost = pricing.estimate(&tokens).unwrap();
        assert!(
            (cost - 27.0).abs() < 0.01,
            "expected $27.00, got ${cost:.2}"
        );
    }
}
