//! `SessionBrief`: the persisted result of summarizing one captured session.
//!
//! A brief is produced by `aikit-session-summarize` and stored through the
//! three brief methods on [`EventStore`](crate::EventStore). The record lives
//! here, beside [`SessionSummary`](crate::SessionSummary), because the store
//! contract owns what it persists; how a brief is produced is the
//! summarizer's business.
//!
//! The record is an additive-only contract (ADR 0020, ADR 0022): fields are
//! added with `#[serde(default)]`, never renamed or removed.

use serde::{Deserialize, Serialize};

use crate::models::ToolKind;

/// Where a tag assignment came from.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TagSource {
    /// Decided in code from the events, before any model call.
    Mechanical,
    /// Assigned by the model, from the configured list.
    Model,
}

/// One tag on a brief, with the reason it was assigned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagAssignment {
    pub name: String,
    pub source: TagSource,
    /// One line: the rule's description for a mechanical tag, the model's
    /// own justification for a model tag.
    pub justification: String,
}

/// One area of the repository a session touched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AreaTouch {
    /// Directory relative to the git root, or the user-mapped area name.
    pub area: String,
    /// `Read` events on files in the area.
    pub reads: u64,
    /// `Write`, `Edit` and `Delete` events on files in the area.
    pub modifications: u64,
    /// Time attributed to the area from event timestamps, in milliseconds.
    pub time_ms: u64,
    /// Distinct files touched, relative to the git root, capped by the
    /// producer.
    #[serde(default)]
    pub files: Vec<String>,
}

/// The persisted summary, areas and tags of one captured session.
///
/// Keyed by `(tool, session_id)`; `EventStore::put_brief` replaces the row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionBrief {
    pub tool: ToolKind,
    pub session_id: String,
    /// The short paragraph the model wrote.
    pub summary: String,
    pub areas: Vec<AreaTouch>,
    /// Mechanical tags first, then model tags, in assignment order. The
    /// first entry is the primary tag.
    pub tags: Vec<TagAssignment>,
    /// The model that was asked for.
    pub model: String,
    /// SHA-256 (hex) of the exact user message sent to the model. Same hash
    /// means the same bytes were sent; the summarizer skips a session whose
    /// stored hash matches unless forced.
    pub digest_hash: String,
    pub generated_at_ms: i64,
    /// The model the provider reported answering with, when it said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_reported: Option<String>,
    /// Tag names the model returned that are not in the configured list and
    /// were dropped after one corrective retry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected_tags: Vec<String>,
    /// Where the prompts in the digest came from: `"history"` (the history
    /// reader), `"events"` (prompt events in the store), or absent when the
    /// digest carried no prompts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_source: Option<String>,
}

impl SessionBrief {
    /// The first tag, which the summarizer mirrors into the backend's tag
    /// slot when one exists.
    pub fn primary_tag(&self) -> Option<&str> {
        self.tags.first().map(|t| t.name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brief_serde_round_trip_and_defaults() {
        let brief = SessionBrief {
            tool: ToolKind::ClaudeCode,
            session_id: "s1".into(),
            summary: "Fixed the thing.".into(),
            areas: vec![AreaTouch {
                area: "src/cli".into(),
                reads: 2,
                modifications: 1,
                time_ms: 1500,
                files: vec!["src/cli/mod.rs".into()],
            }],
            tags: vec![TagAssignment {
                name: "bugfix".into(),
                source: TagSource::Model,
                justification: "the prompt described a failing case".into(),
            }],
            model: "gpt-4o".into(),
            digest_hash: "abc".into(),
            generated_at_ms: 1_700_000_000_000,
            model_reported: None,
            rejected_tags: vec![],
            prompt_source: Some("events".into()),
        };
        let json = serde_json::to_string(&brief).unwrap();
        let back: SessionBrief = serde_json::from_str(&json).unwrap();
        assert_eq!(brief, back);
        assert_eq!(brief.primary_tag(), Some("bugfix"));

        // An older writer that never recorded the optional fields still
        // deserializes (ADR 0020).
        let old = r#"{"tool":"codex","session_id":"s","summary":"x","areas":[],
            "tags":[],"model":"m","digest_hash":"h","generated_at_ms":1}"#;
        let back: SessionBrief = serde_json::from_str(old).unwrap();
        assert_eq!(back.model_reported, None);
        assert!(back.rejected_tags.is_empty());
        assert_eq!(back.primary_tag(), None);
    }
}
