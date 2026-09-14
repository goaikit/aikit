//! Which captured sessions to act on: explicit ids (or unique prefixes), a
//! time window, or everything.

use aikit_session_capture::SessionSummary;

/// Shortest id prefix accepted, to keep a prefix from matching by accident.
pub const MIN_PREFIX: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    /// Session ids, exact or a prefix of at least [`MIN_PREFIX`] characters.
    pub ids: Vec<String>,
    /// Keep sessions whose last event is at or after this instant.
    pub since_ms: Option<i64>,
    pub all: bool,
}

#[non_exhaustive]
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SelectError {
    #[error("select sessions with --session <id>, --since <when> or --all")]
    Nothing,
    #[error("session id prefix '{0}' is shorter than {MIN_PREFIX} characters")]
    ShortPrefix(String),
    #[error("no captured session matches '{0}'")]
    Unknown(String),
    #[error("'{prefix}' matches more than one session: {}", matches.join(", "))]
    Ambiguous {
        prefix: String,
        matches: Vec<String>,
    },
}

#[non_exhaustive]
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SinceError {
    #[error("cannot parse '{0}' as a duration (e.g. 24h, 7d) or an RFC 3339 timestamp")]
    Unparseable(String),
}

/// Parse `--since`: a duration back from `now_ms` (`90m`, `24h`, `7d`,
/// `2w`), an RFC 3339 timestamp, or a `YYYY-MM-DD` date (UTC midnight).
pub fn parse_since(raw: &str, now_ms: i64) -> Result<i64, SinceError> {
    let s = raw.trim();
    if let Some((num, unit)) = s
        .char_indices()
        .find(|(_, c)| c.is_ascii_alphabetic())
        .map(|(i, _)| s.split_at(i))
    {
        if let Ok(n) = num.trim().parse::<i64>() {
            let unit_ms: Option<i64> = match unit {
                "s" => Some(1_000),
                "m" => Some(60_000),
                "h" => Some(3_600_000),
                "d" => Some(86_400_000),
                "w" => Some(7 * 86_400_000),
                _ => None,
            };
            if let Some(u) = unit_ms {
                return Ok(now_ms - n.saturating_mul(u));
            }
        }
    }
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(t.timestamp_millis());
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let dt = d.and_hms_opt(0, 0, 0).expect("midnight is valid");
        return Ok(dt.and_utc().timestamp_millis());
    }
    Err(SinceError::Unparseable(raw.to_string()))
}

/// Apply a selection to the captured sessions. The result is newest first.
/// The same id under two tools (a session captured by both) matches both.
pub fn select_sessions(
    sessions: Vec<SessionSummary>,
    sel: &Selection,
) -> Result<Vec<SessionSummary>, SelectError> {
    if sel.ids.is_empty() && sel.since_ms.is_none() && !sel.all {
        return Err(SelectError::Nothing);
    }
    let mut chosen: Vec<SessionSummary> = if sel.ids.is_empty() {
        sessions
    } else {
        let mut out: Vec<SessionSummary> = Vec::new();
        for id in &sel.ids {
            let id = id.trim();
            let exact: Vec<&SessionSummary> =
                sessions.iter().filter(|s| s.session_id == id).collect();
            let matched: Vec<&SessionSummary> = if !exact.is_empty() {
                exact
            } else {
                if id.chars().count() < MIN_PREFIX {
                    return Err(SelectError::ShortPrefix(id.to_string()));
                }
                let by_prefix: Vec<&SessionSummary> = sessions
                    .iter()
                    .filter(|s| s.session_id.starts_with(id))
                    .collect();
                let mut distinct: Vec<String> =
                    by_prefix.iter().map(|s| s.session_id.clone()).collect();
                distinct.sort();
                distinct.dedup();
                if distinct.len() > 1 {
                    return Err(SelectError::Ambiguous {
                        prefix: id.to_string(),
                        matches: distinct,
                    });
                }
                by_prefix
            };
            if matched.is_empty() {
                return Err(SelectError::Unknown(id.to_string()));
            }
            for m in matched {
                if !out
                    .iter()
                    .any(|s| s.tool == m.tool && s.session_id == m.session_id)
                {
                    out.push(m.clone());
                }
            }
        }
        out
    };
    if let Some(since) = sel.since_ms {
        chosen.retain(|s| s.last_event_at_ms >= since);
    }
    chosen.sort_by(|a, b| {
        b.last_event_at_ms
            .cmp(&a.last_event_at_ms)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    Ok(chosen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_session_capture::ToolKind;
    use std::path::PathBuf;

    fn s(tool: ToolKind, id: &str, last: i64) -> SessionSummary {
        SessionSummary {
            tool,
            session_id: id.into(),
            source_file: PathBuf::from("/x"),
            first_event_at_ms: last - 10,
            last_event_at_ms: last,
            action_count: 1,
            tool_kinds: vec![],
            git_root: None,
        }
    }

    fn pool() -> Vec<SessionSummary> {
        vec![
            s(ToolKind::ClaudeCode, "aaaaaaaa-1111", 100),
            s(ToolKind::ClaudeCode, "aaaaaaaa-2222", 300),
            s(ToolKind::Codex, "bbbbbbbb-0000", 200),
            s(ToolKind::Codex, "aaaaaaaa-1111", 250),
        ]
    }

    #[test]
    fn since_parses_durations_and_timestamps() {
        let now = 1_000_000_000_000;
        assert_eq!(parse_since("24h", now), Ok(now - 86_400_000));
        assert_eq!(parse_since(" 7d ", now), Ok(now - 7 * 86_400_000));
        assert_eq!(parse_since("90m", now), Ok(now - 90 * 60_000));
        assert_eq!(parse_since("2w", now), Ok(now - 14 * 86_400_000));
        assert_eq!(
            parse_since("2026-01-15T10:00:00Z", now),
            Ok(1_768_471_200_000)
        );
        assert_eq!(parse_since("2026-01-15", now), Ok(1_768_435_200_000));
        assert!(matches!(
            parse_since("yesterday", now),
            Err(SinceError::Unparseable(_))
        ));
        assert!(matches!(
            parse_since("5x", now),
            Err(SinceError::Unparseable(_))
        ));
    }

    #[test]
    fn nothing_selected_is_an_error() {
        assert!(matches!(
            select_sessions(pool(), &Selection::default()),
            Err(SelectError::Nothing)
        ));
    }

    #[test]
    fn all_and_since_filter_and_sort_newest_first() {
        let all = select_sessions(
            pool(),
            &Selection {
                all: true,
                ..Default::default()
            },
        )
        .unwrap();
        let lasts: Vec<i64> = all.iter().map(|s| s.last_event_at_ms).collect();
        assert_eq!(lasts, vec![300, 250, 200, 100]);

        let since = select_sessions(
            pool(),
            &Selection {
                since_ms: Some(200),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(since.len(), 3);
    }

    #[test]
    fn ids_match_exactly_or_by_unique_prefix() {
        let exact = select_sessions(
            pool(),
            &Selection {
                ids: vec!["aaaaaaaa-1111".into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(exact.len(), 2, "same id under two tools matches both");

        let prefix = select_sessions(
            pool(),
            &Selection {
                ids: vec!["bbbbbbbb".into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(prefix[0].session_id, "bbbbbbbb-0000");

        assert!(matches!(
            select_sessions(
                pool(),
                &Selection {
                    ids: vec!["aaaaaaaa".into()],
                    ..Default::default()
                }
            ),
            Err(SelectError::Ambiguous { .. })
        ));
        assert!(matches!(
            select_sessions(
                pool(),
                &Selection {
                    ids: vec!["bbb".into()],
                    ..Default::default()
                }
            ),
            Err(SelectError::ShortPrefix(_))
        ));
        assert!(matches!(
            select_sessions(
                pool(),
                &Selection {
                    ids: vec!["cccccccc-0000".into()],
                    ..Default::default()
                }
            ),
            Err(SelectError::Unknown(_))
        ));
    }
}
