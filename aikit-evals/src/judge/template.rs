//! `{{variable}}` templates for judge prompts (spec eval-judge R2).
//!
//! The grammar is deliberately tiny: `{{name}}` with optional whitespace
//! inside the braces, no filters, no conditionals, no escapes. A judge prompt
//! is a document the author reads back verbatim; anything cleverer would be
//! a place for behaviour to hide.

use std::fmt;

/// Where a template is used, which decides which variables it may name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// `prompt` and `system_prompt`.
    Prompt,
    /// `retry_prompt`: only `{{validation_error}}`.
    Retry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateError {
    /// `{{` without a matching `}}`.
    Unclosed { offset: usize },
    /// The text between the braces is not a variable name.
    Malformed { raw: String },
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TemplateError::Unclosed { offset } => {
                write!(f, "unclosed `{{{{` at byte {offset}")
            }
            TemplateError::Malformed { raw } => {
                write!(f, "malformed placeholder `{{{{{raw}}}}}`")
            }
        }
    }
}

impl std::error::Error for TemplateError {}

fn is_name(name: &str) -> bool {
    let mut parts = name.split('.');
    let Some(head) = parts.next() else {
        return false;
    };
    let ident = |s: &str| {
        let mut chars = s.chars();
        matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    if !ident(head) {
        return false;
    }
    let mut count = 1;
    for part in parts {
        count += 1;
        if count > 2 || !ident(part) {
            return false;
        }
    }
    true
}

/// Every placeholder in a template, in order, duplicates included.
pub fn placeholders(template: &str) -> Result<Vec<String>, TemplateError> {
    let mut out = Vec::new();
    let mut rest = template;
    let mut offset = 0;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            return Err(TemplateError::Unclosed {
                offset: offset + start,
            });
        };
        let raw = &after[..end];
        let name = raw.trim();
        if !is_name(name) {
            return Err(TemplateError::Malformed {
                raw: raw.to_string(),
            });
        }
        out.push(name.to_string());
        let consumed = start + 2 + end + 2;
        offset += consumed;
        rest = &rest[consumed..];
    }
    Ok(out)
}

/// Is `name` a variable the engine can supply in this scope (R2)?
/// `Err` carries the message a validator should print.
pub fn check_placeholder(name: &str, scope: Scope) -> Result<(), String> {
    let known_prompt = matches!(
        name,
        "case.prompt"
            | "trial.final_answer"
            | "trial.tool_calls"
            | "trial.transcript"
            | "trial.workspace_diff"
            | "skill.body"
            | "rubric"
            | "output_contract"
    ) || name
        .strip_prefix("case.")
        .map(|col| !col.is_empty())
        .unwrap_or(false);
    match scope {
        Scope::Prompt if known_prompt => Ok(()),
        Scope::Prompt if name == "validation_error" => Err(format!(
            "unknown template variable `{{{{{name}}}}}`: `{{{{validation_error}}}}` is available in `retry_prompt` only"
        )),
        Scope::Prompt => Err(format!("unknown template variable `{{{{{name}}}}}`")),
        Scope::Retry if name == "validation_error" => Ok(()),
        Scope::Retry => Err(format!(
            "unknown template variable `{{{{{name}}}}}`: `retry_prompt` may use `{{{{validation_error}}}}` only"
        )),
    }
}

/// A rendered template plus the variables that hit the byte cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    pub text: String,
    /// Variable names truncated, each once, in order of first truncation.
    pub truncated: Vec<String>,
}

/// Cut `value` to at most `max_bytes` on a char boundary and say how much
/// was cut (R2). Untouched when it fits.
pub fn cap(value: String, max_bytes: usize) -> (String, Option<usize>) {
    if value.len() <= max_bytes {
        return (value, None);
    }
    let mut cut = max_bytes;
    while cut > 0 && !value.is_char_boundary(cut) {
        cut -= 1;
    }
    let removed = value.len() - cut;
    let mut text = value[..cut].to_string();
    text.push_str(&format!("[truncated {removed} bytes]"));
    (text, Some(removed))
}

/// Render `template`, resolving each placeholder through `lookup`. The
/// lookup's error is the render's error: a variable that cannot be supplied
/// never renders blank. Each value is capped at `max_var_bytes`.
pub fn render(
    template: &str,
    max_var_bytes: usize,
    mut lookup: impl FnMut(&str) -> Result<String, String>,
) -> Result<Rendered, String> {
    let mut text = String::with_capacity(template.len());
    let mut truncated: Vec<String> = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        text.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find("}}")
            .ok_or_else(|| TemplateError::Unclosed { offset: 0 }.to_string())?;
        let name = after[..end].trim();
        if !is_name(name) {
            return Err(TemplateError::Malformed {
                raw: after[..end].to_string(),
            }
            .to_string());
        }
        let value = lookup(name)?;
        let (value, cut) = cap(value, max_var_bytes);
        if cut.is_some() && !truncated.iter().any(|t| t == name) {
            truncated.push(name.to_string());
        }
        text.push_str(&value);
        rest = &after[end + 2..];
    }
    text.push_str(rest);
    Ok(Rendered { text, truncated })
}

/// Render a `retry_prompt`: its only variable is the validation error text.
pub fn render_retry(template: &str, errors: &[String]) -> String {
    let joined = errors.join("; ");
    render(template, usize::MAX, |name| {
        if name == "validation_error" {
            Ok(joined.clone())
        } else {
            Err(format!("`{{{{{name}}}}}` is not available in retry_prompt"))
        }
    })
    .map(|r| r.text)
    .unwrap_or_else(|e| format!("{template}\n\n[retry_prompt could not be rendered: {e}]"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_found_in_order_with_duplicates() {
        let vars = placeholders("a {{ case.prompt }} b {{rubric}} c {{case.prompt}}").unwrap();
        assert_eq!(vars, vec!["case.prompt", "rubric", "case.prompt"]);
        assert!(placeholders("no vars").unwrap().is_empty());
    }

    #[test]
    fn unclosed_and_malformed_placeholders_are_errors() {
        assert_eq!(
            placeholders("x {{rubric").unwrap_err(),
            TemplateError::Unclosed { offset: 2 }
        );
        assert!(matches!(
            placeholders("{{ru bric}}").unwrap_err(),
            TemplateError::Malformed { .. }
        ));
        assert!(matches!(
            placeholders("{{a.b.c}}").unwrap_err(),
            TemplateError::Malformed { .. }
        ));
        assert!(matches!(
            placeholders("{{}}").unwrap_err(),
            TemplateError::Malformed { .. }
        ));
    }

    #[test]
    fn column_names_may_carry_hyphens_and_digits() {
        assert_eq!(
            placeholders("{{case.expected-2}}").unwrap(),
            vec!["case.expected-2"]
        );
    }

    #[test]
    fn scope_rules() {
        for ok in [
            "case.prompt",
            "case.expected",
            "trial.final_answer",
            "trial.tool_calls",
            "trial.transcript",
            "trial.workspace_diff",
            "skill.body",
            "rubric",
            "output_contract",
        ] {
            assert!(check_placeholder(ok, Scope::Prompt).is_ok(), "{ok}");
            assert!(check_placeholder(ok, Scope::Retry).is_err(), "{ok}");
        }
        assert!(check_placeholder("validation_error", Scope::Retry).is_ok());
        let err = check_placeholder("validation_error", Scope::Prompt).unwrap_err();
        assert!(err.contains("retry_prompt"), "{err}");
        assert!(check_placeholder("trial.answer", Scope::Prompt).is_err());
        assert!(check_placeholder("skill.name", Scope::Prompt).is_err());
    }

    #[test]
    fn render_substitutes_and_reports_truncation() {
        let big = "é".repeat(100); // 200 bytes, 2 per char
        let out = render("A={{a}} B={{b}} A2={{a}}", 15, |name| match name {
            "a" => Ok(big.clone()),
            "b" => Ok("small".to_string()),
            other => Err(format!("no {other}")),
        })
        .unwrap();
        // 15 bytes is not a char boundary of a 2-byte sequence: cut to 14.
        let expect_a = format!("{}[truncated 186 bytes]", "é".repeat(7));
        assert_eq!(out.text, format!("A={expect_a} B=small A2={expect_a}"));
        assert_eq!(out.truncated, vec!["a"]);
    }

    #[test]
    fn render_fails_on_lookup_error_instead_of_rendering_blank() {
        let err = render("x {{trial.final_answer}} y", 100, |name| {
            Err(format!("cannot supply {name}"))
        })
        .unwrap_err();
        assert!(err.contains("trial.final_answer"), "{err}");
    }

    #[test]
    fn render_leaves_text_without_placeholders_alone() {
        let out = render("plain { text } }}", 10, |_| Ok(String::new())).unwrap();
        assert_eq!(out.text, "plain { text } }}");
        assert!(out.truncated.is_empty());
    }

    #[test]
    fn cap_at_exact_size_is_untouched() {
        let (t, cut) = cap("abc".to_string(), 3);
        assert_eq!(t, "abc");
        assert!(cut.is_none());
        let (t, cut) = cap("abcd".to_string(), 3);
        assert_eq!(t, "abc[truncated 1 bytes]");
        assert_eq!(cut, Some(1));
    }

    #[test]
    fn retry_rendering_joins_errors() {
        let text = render_retry(
            "Fix: {{validation_error}}",
            &["missing 'a'".to_string(), "bad 'b'".to_string()],
        );
        assert_eq!(text, "Fix: missing 'a'; bad 'b'");
    }
}
