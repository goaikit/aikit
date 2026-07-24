use aikit_session_capture::ToolKind;
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};

const PATH_SEGMENT_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

pub fn percent_encode_segment(segment: &str) -> String {
    utf8_percent_encode(segment, PATH_SEGMENT_ENCODE_SET).to_string()
}

pub fn percent_decode_segment(segment: &str) -> Result<String, std::str::Utf8Error> {
    percent_decode_str(segment)
        .decode_utf8()
        .map(|s| s.into_owned())
}

pub fn object_key(
    prefix: &str,
    owner: &str,
    tool: ToolKind,
    session_id: &str,
    content_hash: &str,
) -> String {
    let prefix = prefix.trim_matches('/');
    let session = percent_encode_segment(session_id);
    if prefix.is_empty() {
        format!(
            "{}/{}/{}/{}.jsonl",
            owner,
            tool.as_str(),
            session,
            content_hash
        )
    } else {
        format!(
            "{}/{}/{}/{}/{}.jsonl",
            prefix,
            owner,
            tool.as_str(),
            session,
            content_hash
        )
    }
}

pub fn decode_session_id_from_key(key: &str) -> Result<String, std::str::Utf8Error> {
    let parts: Vec<_> = key.split('/').collect();
    if parts.len() < 5 {
        return percent_decode_segment("");
    }
    percent_decode_segment(parts[parts.len() - 2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_scheme_round_trips_adversarial_session_ids() {
        for session_id in [
            "plain",
            "has/slash",
            "has spaces",
            "unicode-雪",
            "%2F-literal",
        ] {
            let key = object_key("sessions/", "owner", ToolKind::Codex, session_id, "abc123");
            assert_eq!(decode_session_id_from_key(&key).unwrap(), session_id);
            assert_eq!(key.split('/').count(), 5);
        }
    }
}
