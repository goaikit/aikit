//! Cost monitoring types + adapter-backed cost extraction (spec 009/010 §16).
//!
//! When the run loop aggregates a `RunResult` and a Backend's `extract_cost`
//! returned `None` (no provider-side cost signal), the engine falls back to
//! adapter-emitted `TokenEvent`s via [`client_computed_cost`]. This is a
//! **fallback only**, never a replacement — provider-side cost always wins.

pub mod extract;

use serde::{Deserialize, Serialize};

/// Provenance of a [`CostSnapshot`]. Determines authority level.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostProvenance {
    /// Sourced from the API reverse proxy (most authoritative).
    Proxy,
    /// Sourced from the provider's own cost API.
    Provider,
    /// Computed client-side from on-disk transcript token counts.
    /// Always pairs with `Spend.is_estimate = true`.
    ClientComputed,
}

/// One cost snapshot for one session/run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostSnapshot {
    pub backend: String,
    pub captured_at_ms: i64,
    pub windows: Vec<CostWindow>,
    pub spend: Option<Spend>,
    pub per_model: Vec<ModelSpend>,
    pub credits: Option<CreditBalance>,
    pub provenance: CostProvenance,
}

/// A named time-window slice of cost data.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostWindow {
    pub label: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub amount_usd: f64,
}

/// Spend estimate for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spend {
    pub amount_usd: f64,
    pub scope: SpendScope,
    /// Always `true` when provenance is `ClientComputed` (spec 009 §5 invariant).
    pub is_estimate: bool,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpendScope {
    Session,
    Window,
    Daily,
}

/// Per-model cost breakdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSpend {
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub amount_usd: f64,
}

/// Credit balance info (when available from the provider).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditBalance {
    pub remaining_usd: f64,
    pub currency: String,
}

/// Pricing table for computing cost from token counts. Populated by the
/// host from their pricing config; the `client_computed_cost` function uses
/// it to estimate spend from adapter `TokenEvent`s.
#[derive(Debug, Clone, Default)]
pub struct PricingTable {
    /// Map of model name → per-million-token prices.
    pub models: std::collections::HashMap<String, ModelPricing>,
}

/// Per-model pricing (USD per million tokens).
#[derive(Debug, Clone)]
pub struct ModelPricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: f64,
}

impl Default for ModelPricing {
    fn default() -> Self {
        Self {
            input_per_mtok: 3.0,
            output_per_mtok: 15.0,
            cache_read_per_mtok: 0.30,
        }
    }
}

impl PricingTable {
    /// Estimate total spend from a set of token events.
    pub fn estimate(&self, tokens: &[aikit_session_capture::TokenEvent]) -> Option<f64> {
        let total: f64 = tokens
            .iter()
            .map(|ev| {
                let pricing = self
                    .models
                    .get(ev.model.as_deref().unwrap_or("default"))
                    .cloned()
                    .unwrap_or_default();
                let input = ev.input_tokens.unwrap_or(0) as f64 / 1_000_000.0;
                let output = ev.output_tokens.unwrap_or(0) as f64 / 1_000_000.0;
                let cache_read = ev.cache_read_tokens.unwrap_or(0) as f64 / 1_000_000.0;
                input * pricing.input_per_mtok
                    + output * pricing.output_per_mtok
                    + cache_read * pricing.cache_read_per_mtok
            })
            .sum();
        Some(total)
    }

    /// Per-model breakdown for a set of token events.
    pub fn per_model_breakdown(
        &self,
        tokens: &[aikit_session_capture::TokenEvent],
    ) -> Vec<ModelSpend> {
        let mut by_model: std::collections::HashMap<String, (u64, u64, u64, f64)> =
            std::collections::HashMap::new();
        for ev in tokens {
            let model = ev.model.clone().unwrap_or_else(|| "unknown".to_string());
            let pricing = self.models.get(&model).cloned().unwrap_or_default();
            let input = ev.input_tokens.unwrap_or(0);
            let output = ev.output_tokens.unwrap_or(0);
            let cache_read = ev.cache_read_tokens.unwrap_or(0);
            let cost = (input as f64 / 1_000_000.0) * pricing.input_per_mtok
                + (output as f64 / 1_000_000.0) * pricing.output_per_mtok
                + (cache_read as f64 / 1_000_000.0) * pricing.cache_read_per_mtok;
            let entry = by_model.entry(model).or_insert((0, 0, 0, 0.0));
            entry.0 += input;
            entry.1 += output;
            entry.2 += cache_read;
            entry.3 += cost;
        }
        by_model
            .into_iter()
            .map(|(model, (input, output, cache_read, amount))| ModelSpend {
                model,
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: cache_read,
                amount_usd: amount,
            })
            .collect()
    }
}
