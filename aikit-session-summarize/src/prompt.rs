//! The messages sent to the model and the reply parsed back.

use serde::Deserialize;

use crate::brief::TagAssignment;
use crate::tags::{ModelTag, TagList};

/// The one system message. Nothing else is injected (ADR 0023).
pub const SYSTEM_PROMPT: &str = "You summarize coding-agent sessions from a digest of what happened: \
the user's prompts, the files touched, the commands run and the assistant's final message. \
Write one short paragraph, in plain prose, saying what was worked on and how it ended. \
Then assign tags. Use only tags from the allowed list, exactly as spelled; assign only what the digest supports; \
do not repeat a tag that is already assigned. Reply with a single JSON object and nothing else: \
{\"summary\": \"...\", \"tags\": [{\"name\": \"...\", \"why\": \"one line of evidence from the digest\"}]}";

/// The user message: the digest, the allowed tags, and what is already
/// assigned. Its exact bytes are what the digest hash covers.
pub fn user_message(digest: &str, tags: &TagList, mechanical: &[TagAssignment]) -> String {
    let mut out = String::new();
    out.push_str(digest);
    out.push_str("\n## Allowed tags\n");
    for t in &tags.tags {
        match &t.description {
            Some(d) => out.push_str(&format!("- {}: {}\n", t.name, d)),
            None => out.push_str(&format!("- {}\n", t.name)),
        }
    }
    out.push_str("\n## Already assigned (decided from the events; do not repeat)\n");
    if mechanical.is_empty() {
        out.push_str("(none)\n");
    }
    for t in mechanical {
        out.push_str(&format!("- {}: {}\n", t.name, t.justification));
    }
    out.push_str("\nReply with the JSON object now.\n");
    out
}

/// The corrective turn after a reply named tags outside the list (or could
/// not be parsed). Appended as a user message after the model's own reply.
pub fn corrective_message(
    rejected: &[String],
    parse_error: Option<&str>,
    tags: &TagList,
) -> String {
    let mut out = String::new();
    if let Some(e) = parse_error {
        out.push_str(&format!(
            "Your reply could not be parsed as the JSON object ({e}). "
        ));
    }
    if !rejected.is_empty() {
        out.push_str(&format!(
            "These tags are not in the allowed list and were rejected: {}. ",
            rejected.join(", ")
        ));
    }
    out.push_str(&format!(
        "Reply again with a single JSON object {{\"summary\": ..., \"tags\": [...]}} using only these tags, spelled exactly: {}.",
        tags.names().join(", ")
    ));
    out
}

/// What the model is asked to return.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ModelReply {
    pub summary: String,
    #[serde(default)]
    pub tags: Vec<ModelTag>,
}

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ReplyError {
    #[error("empty reply")]
    Empty,
    #[error("no JSON object in reply")]
    NoObject,
    #[error("invalid JSON: {0}")]
    Json(String),
    #[error("summary is empty")]
    EmptySummary,
}

/// Parse the model's reply: tolerate a code fence and prose around the
/// object, take the outermost `{ ... }`.
pub fn parse_reply(text: &str) -> Result<ModelReply, ReplyError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(ReplyError::Empty);
    }
    let start = text.find('{').ok_or(ReplyError::NoObject)?;
    let end = text.rfind('}').ok_or(ReplyError::NoObject)?;
    if end < start {
        return Err(ReplyError::NoObject);
    }
    let body = &text[start..=end];
    let reply: ModelReply =
        serde_json::from_str(body).map_err(|e| ReplyError::Json(e.to_string()))?;
    if reply.summary.trim().is_empty() {
        return Err(ReplyError::EmptySummary);
    }
    Ok(ModelReply {
        summary: reply.summary.trim().to_string(),
        tags: reply.tags,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brief::TagSource;

    #[test]
    fn user_message_names_allowed_and_assigned_tags() {
        let tags = TagList::from_names(["feature", "test"]).unwrap();
        let already = vec![TagAssignment {
            name: "test".into(),
            source: TagSource::Mechanical,
            justification: "only test files were modified".into(),
        }];
        let msg = user_message("# Session x\n", &tags, &already);
        assert!(msg.starts_with("# Session x\n"));
        assert!(msg.contains("## Allowed tags\n- feature\n- test\n"));
        assert!(msg.contains("## Already assigned"));
        assert!(msg.contains("- test: only test files were modified"));
        let none = user_message("d", &tags, &[]);
        assert!(none.contains("(none)"));
    }

    #[test]
    fn parses_fenced_and_bare_replies() {
        let fenced = "Here you go:\n```json\n{\"summary\": \" Did a thing. \", \"tags\": [{\"name\": \"feature\", \"why\": \"added x\"}]}\n```";
        let r = parse_reply(fenced).unwrap();
        assert_eq!(r.summary, "Did a thing.");
        assert_eq!(r.tags[0].name, "feature");
        assert_eq!(r.tags[0].why, "added x");

        let bare = parse_reply("{\"summary\":\"s\"}").unwrap();
        assert!(bare.tags.is_empty());

        // `justification` and `reason` are accepted spellings of `why`.
        let alias =
            parse_reply("{\"summary\":\"s\",\"tags\":[{\"name\":\"a\",\"justification\":\"j\"}]}")
                .unwrap();
        assert_eq!(alias.tags[0].why, "j");

        assert!(matches!(parse_reply("  "), Err(ReplyError::Empty)));
        assert!(matches!(parse_reply("no json"), Err(ReplyError::NoObject)));
        assert!(matches!(
            parse_reply("{\"summary\": 1}"),
            Err(ReplyError::Json(_))
        ));
        assert!(matches!(
            parse_reply("{\"summary\": \" \"}"),
            Err(ReplyError::EmptySummary)
        ));
    }

    #[test]
    fn corrective_message_quotes_rejections() {
        let tags = TagList::from_names(["a", "b"]).unwrap();
        let m = corrective_message(&["zzz".into()], None, &tags);
        assert!(m.contains("rejected: zzz"));
        assert!(m.contains("spelled exactly: a, b"));
        let p = corrective_message(&[], Some("no JSON object in reply"), &tags);
        assert!(p.contains("could not be parsed"));
    }
}
