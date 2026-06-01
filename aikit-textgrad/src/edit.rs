//! Layer 1 — deterministic edit substrate.
//!
//! Zero dependency on `aikit-evals` or `aikit-sdk`. Pure string-transformation functions
//! with four edit operations, two-strategy anchor resolution, protected-region enforcement,
//! and complete skip reporting.

use serde::{Deserialize, Serialize};
use std::ops::Range;

/// Sentinel marking the start of the protected region.
pub const PROTECTED_BEGIN: &str = "<!-- SKILLOPT:PROTECTED:BEGIN -->";
/// Sentinel marking the end of the protected region.
pub const PROTECTED_END: &str = "<!-- SKILLOPT:PROTECTED:END -->";

/// A single proposed text edit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Edit {
    pub op: EditOp,
    pub target: Option<String>,
    pub content: Option<String>,
    pub impact: f64,
}

/// The operation to perform on the document.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EditOp {
    Append,
    InsertAfter,
    Replace,
    Delete,
}

/// An ordered sequence of edits applied sequentially.
pub type Patch = Vec<Edit>;

/// Result of applying a patch to a document.
#[derive(Debug, Clone, PartialEq)]
pub struct ApplyReport {
    /// Document text after all edits have been applied.
    pub result: String,
    /// Indices into the input patch of edits that were applied.
    pub applied: Vec<usize>,
    /// Edits that were skipped, with reasons.
    pub skipped: Vec<SkipRecord>,
}

/// Records a skipped edit.
#[derive(Debug, Clone, PartialEq)]
pub struct SkipRecord {
    pub index: usize,
    pub reason: SkipReason,
}

/// Why an edit was skipped.
#[derive(Debug, Clone, PartialEq)]
pub enum SkipReason {
    /// Anchor not found by exact or whitespace-normalized match.
    AnchorNotFound,
    /// Resolved anchor lies inside or at the protected region boundary.
    TargetsProtected,
    /// `Replace`, `Delete`, or `InsertAfter` has `target: None`.
    MissingTarget,
    /// `Append`, `Replace`, or `InsertAfter` has `content: None`.
    MissingContent,
}

/// Result of a budget-limited application of a ranked pool of edits.
#[derive(Debug, Clone)]
pub struct BudgetedApply {
    /// The `ApplyReport` produced by applying the chosen edits in declaration order.
    pub report: ApplyReport,
    /// Pool indices chosen, in application order.
    pub chosen: Vec<usize>,
    /// Pool entries dropped while filling the budget (anchor/content misses).
    pub intra_patch_skips: Vec<SkipRecord>,
}

/// Append the protected-region sentinels to `doc` if they are absent (idempotent).
pub fn ensure_protected_region(doc: &str) -> String {
    if doc.contains(PROTECTED_BEGIN) {
        doc.to_string()
    } else {
        let mut result = doc.to_string();
        if !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(PROTECTED_BEGIN);
        result.push_str("\n\n");
        result.push_str(PROTECTED_END);
        result
    }
}

/// Return the byte range of the entire protected block, or `None` if absent.
pub fn protected_span(doc: &str) -> Option<Range<usize>> {
    let begin_pos = doc.find(PROTECTED_BEGIN)?;
    let end_marker_pos = doc.find(PROTECTED_END)?;
    if end_marker_pos < begin_pos {
        return None;
    }
    Some(begin_pos..end_marker_pos + PROTECTED_END.len())
}

/// Return the byte offset of `PROTECTED_BEGIN`, or `doc.len()` if absent.
///
/// Edits must not place their anchor at or past this offset.
pub fn editable_end(doc: &str) -> usize {
    doc.find(PROTECTED_BEGIN).unwrap_or(doc.len())
}

/// Resolve an anchor in `doc` using exact match first, then whitespace-normalized fallback.
///
/// Returns the byte range of the first match, or `None` if no match is found.
pub(crate) fn resolve_anchor(doc: &str, target: &str) -> Option<Range<usize>> {
    if target.is_empty() {
        return None;
    }
    // Strategy 1: exact match
    if let Some(pos) = doc.find(target) {
        return Some(pos..pos + target.len());
    }
    // Strategy 2: whitespace-normalized
    whitespace_normalized_find(doc, target)
}

fn normalize_ws(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_ws = false;
    for c in s.chars() {
        if c.is_ascii_whitespace() {
            if !prev_ws {
                result.push(' ');
            }
            prev_ws = true;
        } else {
            result.push(c);
            prev_ws = false;
        }
    }
    result
}

fn whitespace_normalized_find(doc: &str, target: &str) -> Option<Range<usize>> {
    let norm_target = normalize_ws(target);
    if norm_target.is_empty() {
        return None;
    }

    // Build normalized document and a byte-level mapping: norm_byte → orig_byte_start
    let mut norm_doc = String::with_capacity(doc.len());
    // For each byte position in norm_doc, stores the corresponding byte offset in doc.
    let mut orig_starts: Vec<usize> = Vec::with_capacity(doc.len());

    let mut prev_ws = false;
    let mut orig_byte: usize = 0;
    for ch in doc.chars() {
        let char_len = ch.len_utf8();
        if ch.is_ascii_whitespace() {
            if !prev_ws {
                norm_doc.push(' ');
                orig_starts.push(orig_byte);
            }
            prev_ws = true;
        } else {
            // Record the original byte start for every byte of this char in norm_doc.
            for _ in 0..char_len {
                orig_starts.push(orig_byte);
            }
            norm_doc.push(ch);
            prev_ws = false;
        }
        orig_byte += char_len;
    }

    let match_start = norm_doc.find(norm_target.as_str())?;
    let match_end = match_start + norm_target.len();

    let orig_start = *orig_starts.get(match_start)?;
    let orig_end = if match_end < orig_starts.len() {
        orig_starts[match_end]
    } else {
        doc.len()
    };

    Some(orig_start..orig_end)
}

fn apply_one_edit(text: &str, edit: &Edit, index: usize) -> Result<String, SkipRecord> {
    let edit_end = editable_end(text);
    match &edit.op {
        EditOp::Append => {
            let content = edit.content.as_deref().ok_or(SkipRecord {
                index,
                reason: SkipReason::MissingContent,
            })?;
            let mut result = text.to_string();
            result.insert_str(edit_end, content);
            Ok(result)
        }
        EditOp::InsertAfter => {
            let target = edit.target.as_deref().ok_or(SkipRecord {
                index,
                reason: SkipReason::MissingTarget,
            })?;
            let content = edit.content.as_deref().ok_or(SkipRecord {
                index,
                reason: SkipReason::MissingContent,
            })?;
            let anchor = resolve_anchor(text, target).ok_or(SkipRecord {
                index,
                reason: SkipReason::AnchorNotFound,
            })?;
            if anchor.start >= edit_end {
                return Err(SkipRecord {
                    index,
                    reason: SkipReason::TargetsProtected,
                });
            }
            let mut result = text.to_string();
            result.insert_str(anchor.end, content);
            Ok(result)
        }
        EditOp::Replace => {
            let target = edit.target.as_deref().ok_or(SkipRecord {
                index,
                reason: SkipReason::MissingTarget,
            })?;
            let content = edit.content.as_deref().ok_or(SkipRecord {
                index,
                reason: SkipReason::MissingContent,
            })?;
            let anchor = resolve_anchor(text, target).ok_or(SkipRecord {
                index,
                reason: SkipReason::AnchorNotFound,
            })?;
            if anchor.start >= edit_end {
                return Err(SkipRecord {
                    index,
                    reason: SkipReason::TargetsProtected,
                });
            }
            let mut result = text.to_string();
            result.replace_range(anchor, content);
            Ok(result)
        }
        EditOp::Delete => {
            let target = edit.target.as_deref().ok_or(SkipRecord {
                index,
                reason: SkipReason::MissingTarget,
            })?;
            let anchor = resolve_anchor(text, target).ok_or(SkipRecord {
                index,
                reason: SkipReason::AnchorNotFound,
            })?;
            if anchor.start >= edit_end {
                return Err(SkipRecord {
                    index,
                    reason: SkipReason::TargetsProtected,
                });
            }
            let mut result = text.to_string();
            result.replace_range(anchor, "");
            Ok(result)
        }
    }
}

/// Apply a patch sequentially; each edit operates on the result of the previous edit.
pub fn apply_patch(doc: &str, patch: &[Edit]) -> ApplyReport {
    let mut text = doc.to_string();
    let mut applied = Vec::new();
    let mut skipped = Vec::new();

    for (idx, edit) in patch.iter().enumerate() {
        match apply_one_edit(&text, edit, idx) {
            Ok(new_text) => {
                text = new_text;
                applied.push(idx);
            }
            Err(skip) => {
                skipped.push(skip);
            }
        }
    }

    ApplyReport {
        result: text,
        applied,
        skipped,
    }
}

/// Apply at most `budget` edits from `ranked_pool`, skipping those that cannot be applied.
///
/// Walks `ranked_pool` in order (best-impact-first). Each chosen edit is applied to the
/// evolving working text. Skipped edits are recorded in `intra_patch_skips` and do not
/// count toward the budget. Stops when `budget` edits have been applied or the pool is
/// exhausted.
pub fn apply_budgeted(doc: &str, ranked_pool: &[Edit], budget: usize) -> BudgetedApply {
    let mut working_text = doc.to_string();
    let mut chosen: Vec<usize> = Vec::new();
    let mut intra_patch_skips: Vec<SkipRecord> = Vec::new();
    let mut applied_count: usize = 0;

    for (pool_idx, edit) in ranked_pool.iter().enumerate() {
        if applied_count >= budget {
            break;
        }
        match apply_one_edit(&working_text, edit, pool_idx) {
            Ok(new_text) => {
                working_text = new_text;
                chosen.push(pool_idx);
                applied_count += 1;
            }
            Err(skip) => {
                intra_patch_skips.push(skip);
            }
        }
    }

    // Re-apply chosen edits to original doc to produce the canonical ApplyReport.
    let chosen_edits: Vec<Edit> = chosen.iter().map(|&i| ranked_pool[i].clone()).collect();
    let report = apply_patch(doc, &chosen_edits);

    BudgetedApply {
        report,
        chosen,
        intra_patch_skips,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- helpers ----

    fn append(content: &str, impact: f64) -> Edit {
        Edit {
            op: EditOp::Append,
            target: None,
            content: Some(content.to_string()),
            impact,
        }
    }

    fn insert_after(target: &str, content: &str, impact: f64) -> Edit {
        Edit {
            op: EditOp::InsertAfter,
            target: Some(target.to_string()),
            content: Some(content.to_string()),
            impact,
        }
    }

    fn replace(target: &str, content: &str, impact: f64) -> Edit {
        Edit {
            op: EditOp::Replace,
            target: Some(target.to_string()),
            content: Some(content.to_string()),
            impact,
        }
    }

    fn delete(target: &str, impact: f64) -> Edit {
        Edit {
            op: EditOp::Delete,
            target: Some(target.to_string()),
            content: None,
            impact,
        }
    }

    // ---- AC1: all four ops apply on byte-exact anchors ----

    #[test]
    fn test_append_inserts_before_protected_begin() {
        let doc = format!("hello\n{PROTECTED_BEGIN}\n\n{PROTECTED_END}");
        let patch = vec![append(" world", 1.0)];
        let report = apply_patch(&doc, &patch);
        assert_eq!(report.applied, vec![0]);
        // " world" must appear before PROTECTED_BEGIN
        let protected_pos = report.result.find(PROTECTED_BEGIN).unwrap();
        let world_pos = report.result.find(" world").unwrap();
        assert!(
            world_pos < protected_pos,
            "content must be before protected region"
        );
        // Protected region must remain intact
        assert!(report.result.contains(PROTECTED_BEGIN));
        assert!(report.result.contains(PROTECTED_END));
    }

    #[test]
    fn test_insert_after_exact_anchor() {
        let doc = "hello world";
        let patch = vec![insert_after("hello", " there", 1.0)];
        let report = apply_patch(doc, &patch);
        assert_eq!(report.result, "hello there world");
        assert_eq!(report.applied, vec![0]);
    }

    #[test]
    fn test_replace_exact_anchor() {
        let doc = "hello world";
        let patch = vec![replace("world", "earth", 1.0)];
        let report = apply_patch(doc, &patch);
        assert_eq!(report.result, "hello earth");
        assert_eq!(report.applied, vec![0]);
    }

    #[test]
    fn test_delete_exact_anchor() {
        let doc = "hello world";
        let patch = vec![delete("world", 1.0)];
        let report = apply_patch(doc, &patch);
        assert_eq!(report.result, "hello ");
        assert_eq!(report.applied, vec![0]);
    }

    // ---- AC2: sequential semantics ----

    #[test]
    fn test_sequential_edit_b_sees_edit_a_result() {
        let doc = "hello world";
        // A: replace "world" → "earth earth"
        // B: replace "earth earth" → "mars" (would not match if B saw original)
        let patch = vec![
            replace("world", "earth earth", 1.0),
            replace("earth earth", "mars", 1.0),
        ];
        let report = apply_patch(doc, &patch);
        assert_eq!(report.result, "hello mars");
        assert_eq!(report.applied, vec![0, 1]);
    }

    // ---- AC3: whitespace-normalized fallback ----

    #[test]
    fn test_whitespace_normalized_match() {
        let doc = "hello\n  world";
        // target has double space; doc has newline+spaces
        let patch = vec![replace("hello\n  world", "hi earth", 1.0)];
        let report = apply_patch(doc, &patch);
        assert_eq!(report.result, "hi earth");
        assert_eq!(report.applied, vec![0]);
    }

    #[test]
    fn test_whitespace_normalized_collapsed_tabs() {
        let doc = "foo\t\tbar";
        let patch = vec![replace("foo  bar", "baz", 1.0)];
        let report = apply_patch(doc, &patch);
        assert_eq!(report.applied, vec![0]);
        assert_eq!(report.result, "baz");
    }

    // ---- AC4: absent anchor yields AnchorNotFound ----

    #[test]
    fn test_absent_anchor_yields_anchor_not_found() {
        let doc = "hello world";
        let patch = vec![replace("xyz", "abc", 1.0)];
        let report = apply_patch(doc, &patch);
        assert_eq!(report.applied, Vec::<usize>::new());
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].reason, SkipReason::AnchorNotFound);
    }

    // ---- AC5: near-but-not-normalized-equal is NOT matched ----

    #[test]
    fn test_no_fuzzy_matching_different_non_ws_char() {
        let doc = "hello world";
        // "helXo world" differs by a non-whitespace character; must not match
        let patch = vec![replace("helXo world", "abc", 1.0)];
        let report = apply_patch(doc, &patch);
        assert_eq!(report.skipped[0].reason, SkipReason::AnchorNotFound);
    }

    // ---- AC6: Append never touches protected region ----

    #[test]
    fn test_append_lands_before_protected_region() {
        let doc = format!("content\n{PROTECTED_BEGIN}\nprotected\n{PROTECTED_END}");
        let patch = vec![append("NEW", 1.0)];
        let report = apply_patch(&doc, &patch);
        let result = &report.result;
        let protected_pos = result.find(PROTECTED_BEGIN).unwrap();
        let new_pos = result.find("NEW").unwrap();
        assert!(
            new_pos < protected_pos,
            "NEW must appear before PROTECTED_BEGIN"
        );
    }

    // ---- AC7: anchor inside protected region → TargetsProtected ----

    #[test]
    fn test_anchor_in_protected_region_yields_targets_protected() {
        let doc = format!("{PROTECTED_BEGIN}\nprotected_anchor\n{PROTECTED_END}");
        let patch = vec![replace("protected_anchor", "new", 1.0)];
        let report = apply_patch(&doc, &patch);
        assert_eq!(report.skipped[0].reason, SkipReason::TargetsProtected);
    }

    // ---- AC8: ensure_protected_region is idempotent ----

    #[test]
    fn test_ensure_protected_region_idempotent() {
        let doc = "some content";
        let once = ensure_protected_region(doc);
        let twice = ensure_protected_region(&once);
        assert_eq!(once, twice);
        assert!(once.contains(PROTECTED_BEGIN));
        assert!(once.contains(PROTECTED_END));
    }

    // ---- AC9: apply_budgeted fills exactly `budget` edits ----

    #[test]
    fn test_apply_budgeted_fills_budget() {
        let doc = "a b c d e";
        let pool = vec![
            replace("a", "A", 1.0),
            replace("b", "B", 0.9),
            replace("c", "C", 0.8),
            replace("d", "D", 0.7),
        ];
        let result = apply_budgeted(doc, &pool, 2);
        assert_eq!(result.chosen.len(), 2);
        assert_eq!(result.report.applied.len(), 2);
    }

    // ---- AC10: apply_budgeted backfills past skipped entries ----

    #[test]
    fn test_apply_budgeted_backfills_past_skipped() {
        let doc = "a b c";
        // Pool: first edit targets missing anchor (skip), second and third apply
        let pool = vec![
            replace("MISSING", "x", 1.0), // will skip
            replace("a", "A", 0.9),
            replace("b", "B", 0.8),
        ];
        let result = apply_budgeted(doc, &pool, 2);
        assert_eq!(result.chosen.len(), 2, "should have 2 chosen (backfilled)");
        assert_eq!(result.intra_patch_skips.len(), 1);
        assert_eq!(
            result.intra_patch_skips[0].reason,
            SkipReason::AnchorNotFound
        );
    }

    // ---- AC11: pool exhausted before budget filled ----

    #[test]
    fn test_apply_budgeted_pool_exhausted() {
        let doc = "a b";
        let pool = vec![
            replace("MISSING1", "x", 1.0),
            replace("MISSING2", "y", 0.9),
            replace("a", "A", 0.8),
        ];
        let result = apply_budgeted(doc, &pool, 5); // budget=5, only 1 can apply
        assert_eq!(result.chosen.len(), 1);
        assert_eq!(result.intra_patch_skips.len(), 2);
    }

    // ---- AC12: Edit serde round-trip ----

    #[test]
    fn test_edit_serde_round_trip() {
        let edit = Edit {
            op: EditOp::Replace,
            target: Some("foo".to_string()),
            content: Some("bar".to_string()),
            impact: 0.75,
        };
        let json = serde_json::to_string(&edit).unwrap();
        let restored: Edit = serde_json::from_str(&json).unwrap();
        assert_eq!(edit, restored);
    }

    #[test]
    fn test_editop_serde_snake_case() {
        let op = EditOp::InsertAfter;
        let json = serde_json::to_string(&op).unwrap();
        assert_eq!(json, r#""insert_after""#);
    }

    // ---- AC13: Layer 1 compiles without aikit-evals / aikit-sdk ----
    // Verified by the absence of those imports in this module.

    // ---- Additional helpers tests ----

    #[test]
    fn test_protected_span_present() {
        let doc = format!("prefix\n{PROTECTED_BEGIN}\ncontent\n{PROTECTED_END}suffix");
        let span = protected_span(&doc).unwrap();
        assert_eq!(
            &doc[span.clone()],
            format!("{PROTECTED_BEGIN}\ncontent\n{PROTECTED_END}")
        );
    }

    #[test]
    fn test_protected_span_absent() {
        assert!(protected_span("no sentinels").is_none());
    }

    #[test]
    fn test_editable_end_with_region() {
        let doc = format!("editable{PROTECTED_BEGIN}protected");
        let end = editable_end(&doc);
        assert_eq!(end, "editable".len());
    }

    #[test]
    fn test_editable_end_without_region() {
        let doc = "no protected region here";
        assert_eq!(editable_end(doc), doc.len());
    }

    #[test]
    fn test_missing_content_on_append() {
        let edit = Edit {
            op: EditOp::Append,
            target: None,
            content: None,
            impact: 1.0,
        };
        let report = apply_patch("hello", &[edit]);
        assert_eq!(report.skipped[0].reason, SkipReason::MissingContent);
    }

    #[test]
    fn test_missing_target_on_replace() {
        let edit = Edit {
            op: EditOp::Replace,
            target: None,
            content: Some("x".to_string()),
            impact: 1.0,
        };
        let report = apply_patch("hello", &[edit]);
        assert_eq!(report.skipped[0].reason, SkipReason::MissingTarget);
    }

    #[test]
    fn test_missing_target_on_delete() {
        let edit = Edit {
            op: EditOp::Delete,
            target: None,
            content: None,
            impact: 1.0,
        };
        let report = apply_patch("hello", &[edit]);
        assert_eq!(report.skipped[0].reason, SkipReason::MissingTarget);
    }

    #[test]
    fn test_missing_target_on_insert_after() {
        let edit = Edit {
            op: EditOp::InsertAfter,
            target: None,
            content: Some("x".to_string()),
            impact: 1.0,
        };
        let report = apply_patch("hello", &[edit]);
        assert_eq!(report.skipped[0].reason, SkipReason::MissingTarget);
    }

    #[test]
    fn test_apply_patch_empty_patch() {
        let doc = "unchanged";
        let empty: Vec<Edit> = vec![];
        let report = apply_patch(doc, &empty);
        assert_eq!(report.result, doc);
        assert!(report.applied.is_empty());
        assert!(report.skipped.is_empty());
    }

    #[test]
    fn test_apply_budgeted_zero_budget() {
        let doc = "a b c";
        let pool = vec![replace("a", "A", 1.0)];
        let result = apply_budgeted(doc, &pool, 0);
        assert!(result.chosen.is_empty());
        assert_eq!(result.report.result, doc);
    }

    #[test]
    fn test_append_no_protected_region() {
        let doc = "hello";
        let patch = vec![append(" world", 1.0)];
        let report = apply_patch(doc, &patch);
        assert_eq!(report.result, "hello world");
    }

    #[test]
    fn test_resolve_anchor_empty_target_returns_none() {
        assert!(resolve_anchor("anything", "").is_none());
    }
}
