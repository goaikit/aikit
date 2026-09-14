//! Areas touched: group a session's file targets by directory (or by a
//! user-supplied prefix mapping), distinguishing reads from modifications and
//! attributing time from event timestamps. Deterministic; no model involved.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use aikit_session_capture::{ActionKind, ToolEvent};

use crate::brief::AreaTouch;
use serde::Deserialize;

/// Idle gaps longer than this are not attributed to any area: the user
/// probably walked away, and honesty beats a big number.
pub const MAX_GAP_MS: u64 = 5 * 60 * 1000;

/// Distinct files kept per area on the brief.
pub const MAX_FILES_PER_AREA: usize = 20;

/// One `[[area]]` entry of an areas file: the longest matching `prefix`
/// wins; a path no prefix matches falls back to directory grouping.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AreaRule {
    pub name: String,
    pub prefix: String,
}

/// How targets are grouped into areas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaMapping {
    /// Path components of the parent directory that make an area when no
    /// rule matches. `2` turns `src/cli/serve/capture.rs` into `src/cli`.
    pub depth: usize,
    pub rules: Vec<AreaRule>,
}

impl Default for AreaMapping {
    fn default() -> Self {
        Self {
            depth: 2,
            rules: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AreasFile {
    #[serde(default, rename = "area")]
    areas: Vec<AreaRule>,
}

impl AreaMapping {
    /// Parse an areas TOML file (`[[area]] name = "..." prefix = "..."`).
    pub fn from_toml(text: &str, depth: usize) -> Result<Self, toml::de::Error> {
        let file: AreasFile = toml::from_str(text)?;
        Ok(Self {
            depth,
            rules: file.areas,
        })
    }

    /// The area a repository-relative path belongs to.
    pub fn area_for(&self, rel: &str) -> String {
        let best = self
            .rules
            .iter()
            .filter(|r| rel.starts_with(&r.prefix))
            .max_by_key(|r| r.prefix.len());
        if let Some(rule) = best {
            return rule.name.clone();
        }
        let parent = Path::new(rel).parent().unwrap_or(Path::new(""));
        let parts: Vec<String> = parent
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                _ => None,
            })
            .take(self.depth.max(1))
            .collect();
        if parts.is_empty() {
            ".".to_string()
        } else {
            parts.join("/")
        }
    }
}

/// Whether an event is a file touch, and of which kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Touch {
    Read,
    Modify,
}

pub fn touch_kind(kind: ActionKind) -> Option<Touch> {
    match kind {
        ActionKind::Read => Some(Touch::Read),
        ActionKind::Write | ActionKind::Edit | ActionKind::Delete => Some(Touch::Modify),
        _ => None,
    }
}

/// Make a target path relative to the git root when it lies under it.
/// Absolute paths outside the root and relative paths pass through, with
/// leading `./` stripped.
pub fn relative_target(target: &str, git_root: Option<&Path>) -> String {
    let path = Path::new(target);
    let rel: PathBuf = match git_root.and_then(|root| path.strip_prefix(root).ok()) {
        Some(stripped) => stripped.to_path_buf(),
        None => path.to_path_buf(),
    };
    let s = rel.to_string_lossy().into_owned();
    s.strip_prefix("./").map(str::to_string).unwrap_or(s)
}

#[derive(Default)]
struct Acc {
    reads: u64,
    modifications: u64,
    time_ms: u64,
    files: Vec<String>,
}

/// Group a session's events into areas. `events` may be in any order; they
/// are sorted by `started_at_ms` for time attribution. Areas are returned
/// sorted by modifications, then reads, then name, so the busiest area comes
/// first and the order is stable.
pub fn group_areas(
    events: &[ToolEvent],
    git_root: Option<&Path>,
    mapping: &AreaMapping,
) -> Vec<AreaTouch> {
    let mut ordered: Vec<&ToolEvent> = events.iter().collect();
    ordered.sort_by_key(|e| (e.started_at_ms.unwrap_or(0), e.source_event_id.clone()));

    let mut areas: BTreeMap<String, Acc> = BTreeMap::new();
    for (i, ev) in ordered.iter().enumerate() {
        let Some(touch) = touch_kind(ev.kind) else {
            continue;
        };
        let Some(target) = ev.target.as_deref().filter(|t| !t.is_empty()) else {
            continue;
        };
        let rel = relative_target(target, git_root);
        let area = mapping.area_for(&rel);
        let acc = areas.entry(area).or_default();
        match touch {
            Touch::Read => acc.reads += 1,
            Touch::Modify => acc.modifications += 1,
        }
        if !acc.files.contains(&rel) && acc.files.len() < MAX_FILES_PER_AREA {
            acc.files.push(rel);
        }
        acc.time_ms += attributed_time(ev, ordered.get(i + 1).copied());
    }

    let mut out: Vec<AreaTouch> = areas
        .into_iter()
        .map(|(area, acc)| AreaTouch {
            area,
            reads: acc.reads,
            modifications: acc.modifications,
            time_ms: acc.time_ms,
            files: acc.files,
        })
        .collect();
    out.sort_by(|a, b| {
        b.modifications
            .cmp(&a.modifications)
            .then_with(|| b.reads.cmp(&a.reads))
            .then_with(|| a.area.cmp(&b.area))
    });
    out
}

/// Time for one event: the gap to the next event, capped at [`MAX_GAP_MS`];
/// for the last event, its own duration if reported.
fn attributed_time(ev: &ToolEvent, next: Option<&ToolEvent>) -> u64 {
    match (ev.started_at_ms, next.and_then(|n| n.started_at_ms)) {
        (Some(start), Some(next_start)) if next_start >= start => {
            ((next_start - start) as u64).min(MAX_GAP_MS)
        }
        (Some(_), Some(_)) => 0,
        _ => ev.duration_ms.unwrap_or(0).min(MAX_GAP_MS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_session_capture::{ActionStatus, ToolKind};

    fn ev(id: &str, kind: ActionKind, target: &str, at: i64) -> ToolEvent {
        ToolEvent {
            source_event_id: id.into(),
            source_file: PathBuf::from("/tmp/s.jsonl"),
            session_id: "s".into(),
            tool: ToolKind::ClaudeCode,
            kind,
            target: Some(target.into()),
            input: None,
            output: None,
            status: ActionStatus::Success,
            error_message: None,
            started_at_ms: Some(at),
            duration_ms: None,
            git_root: Some(PathBuf::from("/repo")),
            metadata: serde_json::Value::Null,
        }
    }

    #[test]
    fn groups_by_parent_directory_at_depth() {
        let m = AreaMapping::default();
        assert_eq!(m.area_for("src/cli/serve/capture.rs"), "src/cli");
        assert_eq!(m.area_for("README.md"), ".");
        assert_eq!(m.area_for("docs/adr/0001.md"), "docs/adr");
        let deep = AreaMapping {
            depth: 3,
            rules: vec![],
        };
        assert_eq!(deep.area_for("src/cli/serve/capture.rs"), "src/cli/serve");
    }

    #[test]
    fn longest_prefix_rule_wins_and_unmatched_falls_back() {
        let m = AreaMapping::from_toml(
            r#"
            [[area]]
            name = "capture"
            prefix = "aikit-session-capture/"
            [[area]]
            name = "capture-tests"
            prefix = "aikit-session-capture/tests/"
            "#,
            2,
        )
        .unwrap();
        assert_eq!(m.area_for("aikit-session-capture/src/lib.rs"), "capture");
        assert_eq!(
            m.area_for("aikit-session-capture/tests/fixtures/x.jsonl"),
            "capture-tests"
        );
        assert_eq!(m.area_for("src/main.rs"), "src");
    }

    #[test]
    fn relative_target_strips_git_root_only_when_under_it() {
        let root = Path::new("/repo");
        assert_eq!(relative_target("/repo/src/a.rs", Some(root)), "src/a.rs");
        assert_eq!(relative_target("/other/b.rs", Some(root)), "/other/b.rs");
        assert_eq!(relative_target("./c.rs", None), "c.rs");
    }

    #[test]
    fn reads_and_modifications_are_counted_separately_with_time_attributed() {
        let events = vec![
            ev("1", ActionKind::Read, "/repo/src/a.rs", 0),
            ev("2", ActionKind::Edit, "/repo/src/a.rs", 1_000),
            ev("3", ActionKind::Bash, "cargo test", 3_000),
            ev("4", ActionKind::Read, "/repo/docs/x.md", 4_000),
            // Idle for an hour: capped at MAX_GAP_MS.
            ev("5", ActionKind::Write, "/repo/docs/y.md", 3_604_000),
        ];
        let areas = group_areas(&events, Some(Path::new("/repo")), &AreaMapping::default());
        assert_eq!(areas.len(), 2);
        let src = areas.iter().find(|a| a.area == "src").unwrap();
        assert_eq!((src.reads, src.modifications), (1, 1));
        // 1 s (read → edit) + 2 s (edit → bash): the bash gap belongs to the
        // edit's area; the bash event itself is not a touch.
        assert_eq!(src.time_ms, 3_000);
        assert_eq!(src.files, vec!["src/a.rs"]);
        let docs = areas.iter().find(|a| a.area == "docs").unwrap();
        assert_eq!((docs.reads, docs.modifications), (1, 1));
        // 4_000 → 3_604_000 is capped; the last event has no duration.
        assert_eq!(docs.time_ms, MAX_GAP_MS);
        // Busiest first: equal modifications, equal reads, then by name.
        assert_eq!(areas[0].area, "docs");
    }

    #[test]
    fn last_event_uses_its_own_duration() {
        let mut e = ev("1", ActionKind::Read, "/repo/a.rs", 10);
        e.duration_ms = Some(250);
        let areas = group_areas(&[e], Some(Path::new("/repo")), &AreaMapping::default());
        assert_eq!(areas[0].time_ms, 250);
    }
}
