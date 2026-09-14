//! The fixed tag list, the mechanical rules decided in code, and validation
//! of what a model returns.

use std::collections::BTreeSet;
use std::path::Path;

use aikit_session_capture::{ActionKind, TagAssignment, TagSource, ToolEvent};
use serde::Deserialize;

use crate::areas::{relative_target, touch_kind, Touch};

/// A rule a tag can be bound to. Closed set: a rule name outside it is a
/// config error, never a silent no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TagRule {
    /// At least one file was modified and every modified file is a test.
    OnlyTests,
    /// At least one file was modified and every modified file is docs.
    OnlyDocs,
    /// At least one file was modified and every modified file is config.
    OnlyConfig,
    /// Files were read or searched and nothing was modified.
    ReadOnly,
}

impl TagRule {
    /// The justification recorded on a mechanical tag.
    pub fn description(self) -> &'static str {
        match self {
            TagRule::OnlyTests => "only test files were modified",
            TagRule::OnlyDocs => "only documentation files were modified",
            TagRule::OnlyConfig => "only configuration files were modified",
            TagRule::ReadOnly => "files were read or searched and nothing was modified",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            TagRule::OnlyTests => "only_tests",
            TagRule::OnlyDocs => "only_docs",
            TagRule::OnlyConfig => "only_config",
            TagRule::ReadOnly => "read_only",
        }
    }
}

/// One tag in the list.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagDefinition {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub rule: Option<TagRule>,
}

/// The configured tag list. Names are unique and non-empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagList {
    pub tags: Vec<TagDefinition>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TagsFile {
    #[serde(default, rename = "tag")]
    tags: Vec<TagDefinition>,
}

#[derive(Debug, thiserror::Error)]
pub enum TagListError {
    #[error("tags file: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("tag list is empty")]
    Empty,
    #[error("tag name is empty")]
    EmptyName,
    #[error("duplicate tag '{0}'")]
    Duplicate(String),
}

impl TagList {
    /// The built-in list used when neither a file nor a flag names one.
    pub fn builtin() -> Self {
        let def = |name: &str, description: &str, rule: Option<TagRule>| TagDefinition {
            name: name.into(),
            description: Some(description.into()),
            rule,
        };
        Self {
            tags: vec![
                def("feature", "new behaviour was added", None),
                def("bugfix", "a defect was diagnosed or fixed", None),
                def(
                    "refactor",
                    "code was restructured without changing behaviour",
                    None,
                ),
                def(
                    "test",
                    "only test files were modified",
                    Some(TagRule::OnlyTests),
                ),
                def(
                    "docs",
                    "only documentation was modified",
                    Some(TagRule::OnlyDocs),
                ),
                def(
                    "config",
                    "only configuration was modified",
                    Some(TagRule::OnlyConfig),
                ),
                def(
                    "research",
                    "the codebase was read or searched without changes",
                    Some(TagRule::ReadOnly),
                ),
                def(
                    "chore",
                    "maintenance: dependencies, formatting, housekeeping",
                    None,
                ),
            ],
        }
    }

    /// Names only, as given on the command line (`--tags a,b,c`).
    pub fn from_names<I: IntoIterator<Item = S>, S: AsRef<str>>(
        names: I,
    ) -> Result<Self, TagListError> {
        let tags = names
            .into_iter()
            .map(|n| TagDefinition {
                name: n.as_ref().trim().to_string(),
                description: None,
                rule: None,
            })
            .collect();
        Self::validated(tags)
    }

    /// A tags TOML file (`[[tag]] name = "..." description = "..." rule = "..."`).
    pub fn from_toml(text: &str) -> Result<Self, TagListError> {
        let file: TagsFile = toml::from_str(text)?;
        Self::validated(file.tags)
    }

    fn validated(tags: Vec<TagDefinition>) -> Result<Self, TagListError> {
        if tags.is_empty() {
            return Err(TagListError::Empty);
        }
        let mut seen = BTreeSet::new();
        for t in &tags {
            if t.name.is_empty() {
                return Err(TagListError::EmptyName);
            }
            if !seen.insert(t.name.clone()) {
                return Err(TagListError::Duplicate(t.name.clone()));
            }
        }
        Ok(Self { tags })
    }

    pub fn contains(&self, name: &str) -> bool {
        self.tags.iter().any(|t| t.name == name)
    }

    pub fn names(&self) -> Vec<&str> {
        self.tags.iter().map(|t| t.name.as_str()).collect()
    }
}

// ── mechanical rules ──────────────────────────────────────────────────────────

/// Path shapes that count as a test file.
pub fn is_test_path(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    let name = Path::new(&lower)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(&name);
    lower.split('/').any(|seg| {
        matches!(
            seg,
            "test" | "tests" | "__tests__" | "spec" | "specs" | "testdata" | "fixtures"
        )
    }) || stem.starts_with("test_")
        || stem.ends_with("_test")
        || stem.ends_with(".test")
        || stem.ends_with(".spec")
        || stem.ends_with("_spec")
}

/// Path shapes that count as documentation.
pub fn is_docs_path(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    let ext = Path::new(&lower)
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    matches!(ext.as_str(), "md" | "mdx" | "rst" | "txt" | "adoc")
        || lower.starts_with("docs/")
        || lower.contains("/docs/")
}

/// Path shapes that count as configuration.
pub fn is_config_path(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    let name = Path::new(&lower)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = Path::new(&lower)
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    matches!(
        ext.as_str(),
        "toml" | "yaml" | "yml" | "json" | "ini" | "cfg" | "conf" | "env" | "properties"
    ) || name.starts_with('.')
        || name == "dockerfile"
        || name == "makefile"
}

/// The path-shaped evidence a rule looks at: modified and read files relative
/// to the git root, plus whether anything was searched.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Evidence {
    pub modified: Vec<String>,
    pub read: Vec<String>,
    pub searched: bool,
}

impl Evidence {
    pub fn from_events(events: &[ToolEvent], git_root: Option<&Path>) -> Self {
        let mut ev = Evidence::default();
        for e in events {
            match touch_kind(e.kind) {
                Some(Touch::Modify) => {
                    if let Some(t) = e.target.as_deref().filter(|t| !t.is_empty()) {
                        push_unique(&mut ev.modified, relative_target(t, git_root));
                    }
                }
                Some(Touch::Read) => {
                    if let Some(t) = e.target.as_deref().filter(|t| !t.is_empty()) {
                        push_unique(&mut ev.read, relative_target(t, git_root));
                    }
                }
                None => {
                    if matches!(
                        e.kind,
                        ActionKind::Grep | ActionKind::Glob | ActionKind::Search
                    ) {
                        ev.searched = true;
                    }
                }
            }
        }
        ev
    }

    fn only_modified(&self, pred: fn(&str) -> bool) -> bool {
        !self.modified.is_empty() && self.modified.iter().all(|p| pred(p))
    }

    pub fn satisfies(&self, rule: TagRule) -> bool {
        match rule {
            TagRule::OnlyTests => self.only_modified(is_test_path),
            TagRule::OnlyDocs => self.only_modified(is_docs_path),
            TagRule::OnlyConfig => self.only_modified(is_config_path),
            TagRule::ReadOnly => {
                self.modified.is_empty() && (!self.read.is_empty() || self.searched)
            }
        }
    }
}

fn push_unique(v: &mut Vec<String>, s: String) {
    if !v.contains(&s) {
        v.push(s);
    }
}

/// Decide every rule-bound tag from the evidence, in list order.
pub fn mechanical_tags(list: &TagList, evidence: &Evidence) -> Vec<TagAssignment> {
    list.tags
        .iter()
        .filter_map(|t| {
            let rule = t.rule?;
            evidence.satisfies(rule).then(|| TagAssignment {
                name: t.name.clone(),
                source: TagSource::Mechanical,
                justification: rule.description().to_string(),
            })
        })
        .collect()
}

// ── model reply validation ────────────────────────────────────────────────────

/// One tag as the model returned it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ModelTag {
    pub name: String,
    #[serde(default, alias = "justification", alias = "reason")]
    pub why: String,
}

/// The model's tags split into accepted assignments and rejected names.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Validated {
    pub accepted: Vec<TagAssignment>,
    pub rejected: Vec<String>,
}

/// Keep only tags from the list, dropping duplicates and anything already
/// assigned mechanically. Names are compared exactly after trimming: the
/// list is the user's, and `Refactor` is not `refactor`.
pub fn validate_model_tags(
    list: &TagList,
    already: &[TagAssignment],
    model_tags: &[ModelTag],
) -> Validated {
    let mut out = Validated::default();
    for t in model_tags {
        let name = t.name.trim();
        if name.is_empty() {
            continue;
        }
        if !list.contains(name) {
            if !out.rejected.iter().any(|r| r == name) {
                out.rejected.push(name.to_string());
            }
            continue;
        }
        let dup =
            already.iter().any(|a| a.name == name) || out.accepted.iter().any(|a| a.name == name);
        if dup {
            continue;
        }
        out.accepted.push(TagAssignment {
            name: name.to_string(),
            source: TagSource::Model,
            justification: t.why.trim().to_string(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_session_capture::{ActionStatus, ToolKind};
    use std::path::PathBuf;

    fn ev(kind: ActionKind, target: &str) -> ToolEvent {
        ToolEvent {
            source_event_id: format!("{}:{target}", kind.as_str()),
            source_file: PathBuf::from("/tmp/s.jsonl"),
            session_id: "s".into(),
            tool: ToolKind::Codex,
            kind,
            target: Some(target.into()),
            input: None,
            output: None,
            status: ActionStatus::Success,
            error_message: None,
            started_at_ms: Some(0),
            duration_ms: None,
            git_root: None,
            metadata: serde_json::Value::Null,
        }
    }

    #[test]
    fn path_classifiers() {
        assert!(is_test_path("tests/cli/spec013_invocation_test.rs"));
        assert!(is_test_path("src/foo_test.go"));
        assert!(is_test_path("web/app.test.ts"));
        assert!(is_test_path("lib/test_helpers.py"));
        assert!(!is_test_path("src/testing_utils.rs"));
        assert!(is_docs_path("README.md"));
        assert!(is_docs_path("docs/adr/0001.md"));
        assert!(!is_docs_path("src/lib.rs"));
        assert!(is_config_path("Cargo.toml"));
        assert!(is_config_path(".github/workflows/ci.yml"));
        assert!(is_config_path(".gitignore"));
        assert!(!is_config_path("src/main.rs"));
    }

    #[test]
    fn only_tests_requires_every_modified_file_to_be_a_test() {
        let list = TagList::builtin();
        let only = Evidence::from_events(
            &[
                ev(ActionKind::Read, "src/lib.rs"),
                ev(ActionKind::Edit, "tests/a_test.rs"),
                ev(ActionKind::Write, "tests/b_test.rs"),
            ],
            None,
        );
        let names: Vec<String> = mechanical_tags(&list, &only)
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, vec!["test"]);

        let mixed = Evidence::from_events(
            &[
                ev(ActionKind::Edit, "tests/a_test.rs"),
                ev(ActionKind::Edit, "src/lib.rs"),
            ],
            None,
        );
        assert!(mechanical_tags(&list, &mixed).is_empty());
    }

    #[test]
    fn docs_config_and_read_only_rules() {
        let list = TagList::builtin();
        let docs = Evidence::from_events(&[ev(ActionKind::Edit, "README.md")], None);
        assert_eq!(mechanical_tags(&list, &docs)[0].name, "docs");

        let config = Evidence::from_events(&[ev(ActionKind::Edit, "Cargo.toml")], None);
        let names: Vec<String> = mechanical_tags(&list, &config)
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, vec!["config"]);

        let research = Evidence::from_events(
            &[
                ev(ActionKind::Read, "src/a.rs"),
                ev(ActionKind::Grep, "foo"),
            ],
            None,
        );
        let tags = mechanical_tags(&list, &research);
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "research");
        assert_eq!(tags[0].source, TagSource::Mechanical);
        assert_eq!(tags[0].justification, TagRule::ReadOnly.description());

        // No touches at all (only bash): no rule fires.
        let bash = Evidence::from_events(&[ev(ActionKind::Bash, "ls")], None);
        assert!(mechanical_tags(&list, &bash).is_empty());
    }

    #[test]
    fn tag_list_parsing_rejects_bad_input() {
        assert!(matches!(
            TagList::from_names(Vec::<String>::new()),
            Err(TagListError::Empty)
        ));
        assert!(matches!(
            TagList::from_names(["a", "a"]),
            Err(TagListError::Duplicate(_))
        ));
        assert!(matches!(
            TagList::from_names(["a", " "]),
            Err(TagListError::EmptyName)
        ));
        let file = TagList::from_toml(
            r#"
            [[tag]]
            name = "test"
            rule = "only_tests"
            [[tag]]
            name = "feature"
            description = "new behaviour"
            "#,
        )
        .unwrap();
        assert_eq!(file.names(), vec!["test", "feature"]);
        assert_eq!(file.tags[0].rule, Some(TagRule::OnlyTests));
        assert!(TagList::from_toml("[[tag]]\nname = \"x\"\nrule = \"bogus\"").is_err());
        assert!(TagList::from_toml("[[tag]]\nname = \"x\"\ncolour = \"red\"").is_err());
    }

    #[test]
    fn model_tags_are_validated_against_the_list() {
        let list = TagList::from_names(["feature", "bugfix", "test"]).unwrap();
        let already = vec![TagAssignment {
            name: "test".into(),
            source: TagSource::Mechanical,
            justification: "x".into(),
        }];
        let reply = vec![
            ModelTag {
                name: "bugfix".into(),
                why: " fixed a panic ".into(),
            },
            ModelTag {
                name: "Bugfix".into(),
                why: "case differs".into(),
            },
            ModelTag {
                name: "test".into(),
                why: "already mechanical".into(),
            },
            ModelTag {
                name: "bugfix".into(),
                why: "duplicate".into(),
            },
            ModelTag {
                name: "".into(),
                why: "".into(),
            },
        ];
        let v = validate_model_tags(&list, &already, &reply);
        assert_eq!(v.accepted.len(), 1);
        assert_eq!(v.accepted[0].name, "bugfix");
        assert_eq!(v.accepted[0].justification, "fixed a panic");
        assert_eq!(v.accepted[0].source, TagSource::Model);
        assert_eq!(v.rejected, vec!["Bugfix"]);
    }
}
