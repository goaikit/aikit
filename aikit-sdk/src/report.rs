//! Report rendering: Markdown and JSON output from validated agent data.

use crate::pipeline::PipelineError;
use crate::template::TemplateRenderer;

/// Renders agent results into Markdown or JSON reports.
pub struct ReportRenderer;

impl ReportRenderer {
    /// Render `data` into a Markdown report using `template`.
    ///
    /// Slot values are built from top-level keys of `data`:
    /// - Scalar values (string, number, bool, null) → their string representation
    /// - Non-scalar values (arrays, objects) → compact JSON
    /// - `report_body` is always injected as a pretty-printed JSON of `data`
    ///
    /// Returns `PipelineError::ReportRender { slot }` if a referenced slot is missing.
    pub fn render_markdown(
        template: &str,
        data: &serde_json::Value,
    ) -> Result<String, PipelineError> {
        let mut slot_entries: Vec<(String, String)> = Vec::new();

        // Build slots from top-level keys
        if let Some(obj) = data.as_object() {
            for (key, val) in obj {
                let slot_value = match val {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    serde_json::Value::Null => "null".to_string(),
                    // Arrays and objects → compact JSON
                    _ => serde_json::to_string(val).unwrap_or_else(|_| val.to_string()),
                };
                slot_entries.push((key.clone(), slot_value));
            }
        }

        // Always inject `report_body` as pretty-printed JSON
        let pretty = serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string());
        slot_entries.push(("report_body".to_string(), pretty));

        // Build &[(&str, &str)] from owned entries
        let slots_ref: Vec<(&str, &str)> = slot_entries
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();

        // Render and map TemplateSlotMissing → ReportRender
        TemplateRenderer::render(template, &slots_ref).map_err(|e| match e {
            PipelineError::TemplateSlotMissing { slot } => PipelineError::ReportRender { slot },
            other => other,
        })
    }

    /// Render `data` as pretty-printed JSON.
    pub fn render_json(data: &serde_json::Value) -> Result<String, PipelineError> {
        Ok(serde_json::to_string_pretty(data).unwrap_or_else(|_| data.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_render_json_basic() {
        let data = json!({"name": "Alice", "score": 42});
        let result = ReportRenderer::render_json(&data).unwrap();
        assert!(result.contains("\"name\""));
        assert!(result.contains("\"Alice\""));
    }

    #[test]
    fn test_render_markdown_scalar_slots() {
        let data = json!({"name": "Alice", "score": 42, "active": true});
        let tmpl = "Name: {{name}}, Score: {{score}}, Active: {{active}}";
        let result = ReportRenderer::render_markdown(tmpl, &data).unwrap();
        assert_eq!(result, "Name: Alice, Score: 42, Active: true");
    }

    #[test]
    fn test_render_markdown_report_body_injected() {
        let data = json!({"x": 1});
        let tmpl = "{{report_body}}";
        let result = ReportRenderer::render_markdown(tmpl, &data).unwrap();
        assert!(result.contains("\"x\""));
        assert!(result.contains("1"));
    }

    #[test]
    fn test_render_markdown_non_scalar_compact_json() {
        let data = json!({"tags": ["a", "b"]});
        let tmpl = "Tags: {{tags}}";
        let result = ReportRenderer::render_markdown(tmpl, &data).unwrap();
        assert!(result.contains("[\"a\",\"b\"]") || result.contains("[\"a\", \"b\"]"));
    }

    #[test]
    fn test_render_markdown_missing_slot_returns_report_render_error() {
        let data = json!({"name": "Alice"});
        let tmpl = "Name: {{name}}, Missing: {{does_not_exist}}";
        let err = ReportRenderer::render_markdown(tmpl, &data).unwrap_err();
        match err {
            PipelineError::ReportRender { slot } => {
                assert_eq!(slot, "does_not_exist");
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_render_markdown_null_value() {
        let data = json!({"val": null});
        let tmpl = "Value: {{val}}";
        let result = ReportRenderer::render_markdown(tmpl, &data).unwrap();
        assert_eq!(result, "Value: null");
    }
}
