//! Single-pass `{{slot}}` template renderer.
//!
//! Scan rules (left-to-right, single pass):
//! - `\{{` → emit literal `{{`
//! - `\}}` → emit literal `}}`
//! - `{{` → scan forward for matching `}}`, trim slot name, look up in slots
//!   - found: emit value verbatim (no rescan)
//!   - not found: return `Err(PipelineError::TemplateSlotMissing { slot })`
//! - Unused slots are silently ignored.

use crate::pipeline::PipelineError;

/// Renders `{{slot}}` templates with a single-pass algorithm.
pub struct TemplateRenderer;

impl TemplateRenderer {
    /// Render `template` by substituting all `{{slot}}` occurrences.
    ///
    /// Returns the rendered string, or `PipelineError::TemplateSlotMissing`
    /// if a referenced slot is not present in `slots`.
    pub fn render(template: &str, slots: &[(&str, &str)]) -> Result<String, PipelineError> {
        let mut output = String::with_capacity(template.len());
        let bytes = template.as_bytes();
        let len = bytes.len();
        let mut i = 0;

        while i < len {
            // Check for escape sequences \{{ and \}}
            if i + 2 < len && bytes[i] == b'\\' {
                if bytes[i + 1] == b'{' && bytes[i + 2] == b'{' {
                    output.push_str("{{");
                    i += 3;
                    continue;
                }
                if bytes[i + 1] == b'}' && bytes[i + 2] == b'}' {
                    output.push_str("}}");
                    i += 3;
                    continue;
                }
            }

            // Check for `{{`
            if i + 1 < len && bytes[i] == b'{' && bytes[i + 1] == b'{' {
                // Search for matching `}}`
                let start = i + 2;
                let mut j = start;
                let mut found_close = false;
                while j + 1 < len {
                    if bytes[j] == b'}' && bytes[j + 1] == b'}' {
                        found_close = true;
                        break;
                    }
                    j += 1;
                }
                if found_close {
                    let slot_name = template[start..j].trim();
                    match slots.iter().find(|(k, _)| *k == slot_name) {
                        Some((_, value)) => {
                            output.push_str(value);
                        }
                        None => {
                            return Err(PipelineError::TemplateSlotMissing {
                                slot: slot_name.to_string(),
                            });
                        }
                    }
                    i = j + 2;
                } else {
                    // No closing `}}` found — treat as literal
                    output.push(bytes[i] as char);
                    i += 1;
                }
                continue;
            }

            // Regular character — push as UTF-8 char boundary safe slice
            let ch_len = utf8_char_len(bytes[i]);
            if i + ch_len <= len {
                output.push_str(&template[i..i + ch_len]);
                i += ch_len;
            } else {
                output.push(bytes[i] as char);
                i += 1;
            }
        }

        Ok(output)
    }
}

/// Returns the byte-length of a UTF-8 character given its leading byte.
fn utf8_char_len(b: u8) -> usize {
    if b & 0x80 == 0 {
        1
    } else if b & 0xE0 == 0xC0 {
        2
    } else if b & 0xF0 == 0xE0 {
        3
    } else {
        4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_substitution() {
        let result = TemplateRenderer::render("Hello, {{name}}!", &[("name", "world")]).unwrap();
        assert_eq!(result, "Hello, world!");
    }

    #[test]
    fn test_multiple_slots() {
        let result = TemplateRenderer::render(
            "{{greeting}}, {{name}}!",
            &[("greeting", "Hi"), ("name", "Alice")],
        )
        .unwrap();
        assert_eq!(result, "Hi, Alice!");
    }

    #[test]
    fn test_missing_slot_returns_error() {
        let err = TemplateRenderer::render("Hello, {{name}}!", &[]).unwrap_err();
        match err {
            PipelineError::TemplateSlotMissing { slot } => assert_eq!(slot, "name"),
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_unused_slot_ignored() {
        let result = TemplateRenderer::render(
            "Hello, {{name}}!",
            &[("name", "world"), ("unused", "ignored")],
        )
        .unwrap();
        assert_eq!(result, "Hello, world!");
    }

    #[test]
    fn test_escape_open_brace() {
        let result = TemplateRenderer::render("literal \\{{ brace", &[]).unwrap();
        assert_eq!(result, "literal {{ brace");
    }

    #[test]
    fn test_escape_close_brace() {
        let result = TemplateRenderer::render("literal \\}} brace", &[]).unwrap();
        assert_eq!(result, "literal }} brace");
    }

    #[test]
    fn test_escape_sequences_not_interpolated() {
        // \{{ should not start an interpolation
        let result = TemplateRenderer::render("\\{{name}}", &[("name", "world")]).unwrap();
        // \{{ emits {{ then "name}}" is literal
        assert_eq!(result, "{{name}}");
    }

    #[test]
    fn test_single_pass_invariant_value_not_rescanned() {
        // If a slot value itself contains `{{other}}`, it must NOT be processed.
        let result =
            TemplateRenderer::render("{{a}}", &[("a", "{{b}}"), ("b", "should-not-appear")])
                .unwrap();
        assert_eq!(result, "{{b}}");
    }

    #[test]
    fn test_no_slots_in_template() {
        let result = TemplateRenderer::render("plain text", &[]).unwrap();
        assert_eq!(result, "plain text");
    }

    #[test]
    fn test_slot_with_whitespace_trimmed() {
        let result = TemplateRenderer::render("{{ name }}", &[("name", "trimmed")]).unwrap();
        assert_eq!(result, "trimmed");
    }
}
