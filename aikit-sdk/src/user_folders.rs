//! Per-user folders an agent reads, and presence detection built on them.
//!
//! The deploy-layout catalog ([`crate::AgentConfig`]) only records
//! project-relative directories. This module records the matching per-user
//! layout for every catalog agent: its per-user configuration folder and
//! every per-user skills folder it reads, all relative to the user's home
//! directory. Several agents also read the shared [`SHARED_SKILLS_DIR`]
//! (`~/.agents/skills`), so one copy of a skill there reaches all of them.
//!
//! Rows here mirror the catalog's project paths under the home directory
//! unless an agent is known to use a different per-user location (for
//! example opencode's `~/.config/opencode`). Agents that are not known to
//! read the shared folder record `reads_shared: false`.
//!
//! Everything here is a pure path computation or filesystem check that takes
//! the home directory as a parameter, so callers (and tests) choose it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The shared per-user skills folder (relative to home) that several agents
/// read in addition to their own.
pub const SHARED_SKILLS_DIR: &str = ".agents/skills";

/// The per-user folders one agent uses, relative to the home directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserFolders {
    /// The agent's per-user configuration folder (e.g. `.claude`). Its
    /// existence is what [`detect_present_agents`] checks.
    pub config_dir: PathBuf,
    /// Every per-user skills folder the agent reads: its own folder first
    /// (when it has one), then [`SHARED_SKILLS_DIR`] when `reads_shared`.
    /// Empty when the agent has no known per-user skills folder.
    pub skills_dirs: Vec<PathBuf>,
    /// Whether the agent reads [`SHARED_SKILLS_DIR`].
    pub reads_shared: bool,
    /// Whether the agent seeing the same skill in two of its folders (e.g.
    /// its own folder and the shared one) is harmless. An agent that reads
    /// two folders holding the same skill lists it twice; this flag is set
    /// only after verifying, per agent, that the duplicate is tolerated.
    /// [`fewest_skill_dirs`] never chooses two folders the same agent reads
    /// unless this flag is `true` for that agent.
    pub duplicate_skill_harmless: bool,
}

/// One row of the per-user layout table.
struct UserRow {
    key: &'static str,
    config_dir: &'static str,
    /// The agent's own per-user skills folder, if it has one.
    own_skills: Option<&'static str>,
    reads_shared: bool,
    /// Verified per agent before enabling; see
    /// [`UserFolders::duplicate_skill_harmless`].
    duplicate_skill_harmless: bool,
}

const fn row(
    key: &'static str,
    config_dir: &'static str,
    own_skills: Option<&'static str>,
    reads_shared: bool,
) -> UserRow {
    UserRow {
        key,
        config_dir,
        own_skills,
        reads_shared,
        // No agent has been verified to tolerate seeing a skill in two of
        // its folders yet, so none is allowed to.
        duplicate_skill_harmless: false,
    }
}

/// Per-user layout for every catalog agent, in catalog order.
const USER_ROWS: &[UserRow] = &[
    row("claude", ".claude", Some(".claude/skills"), false),
    row("gemini", ".gemini", Some(".gemini/skills"), true),
    row("copilot", ".copilot", Some(".copilot/skills"), true),
    row("cursor", ".cursor", Some(".cursor/skills"), true),
    row("qwen", ".qwen", Some(".qwen/skills"), true),
    row("newton", ".newton", Some(".newton/skills"), false),
    row(
        "opencode",
        ".config/opencode",
        Some(".config/opencode/skills"),
        true,
    ),
    row("codex", ".codex", Some(".codex/skills"), true),
    row("pi", ".pi", Some(".pi/agent/skills"), true),
    row(
        "windsurf",
        ".codeium/windsurf",
        Some(".codeium/windsurf/skills"),
        false,
    ),
    row("kilocode", ".kilocode", Some(".kilocode/skills"), true),
    row("auggie", ".augment", Some(".augment/skills"), true),
    row("roo", ".roo", Some(".roo/skills"), true),
    row("codebuddy", ".codebuddy", None, false),
    row("qoder", ".qoder", None, false),
    row("amp", ".config/amp", None, true),
    row("shai", ".shai", None, false),
    row("q", ".amazonq", None, false),
    row("bob", ".bob", None, false),
];

fn folders_from_row(r: &UserRow) -> UserFolders {
    let mut skills_dirs: Vec<PathBuf> = r.own_skills.map(PathBuf::from).into_iter().collect();
    if r.reads_shared {
        skills_dirs.push(PathBuf::from(SHARED_SKILLS_DIR));
    }
    UserFolders {
        config_dir: PathBuf::from(r.config_dir),
        skills_dirs,
        reads_shared: r.reads_shared,
        duplicate_skill_harmless: r.duplicate_skill_harmless,
    }
}

fn find_row(key: &str) -> Option<&'static UserRow> {
    USER_ROWS.iter().find(|r| r.key == key)
}

/// Returns the per-user folders of the agent `key`, or `None` when `key` is
/// not in the catalog.
pub fn user_folders(key: &str) -> Option<UserFolders> {
    find_row(key).map(folders_from_row)
}

/// Returns every per-user skills folder the agent `key` reads, joined onto
/// `home` (own folder first, then the shared one). Empty for an unknown key
/// or an agent with no known per-user skills folder.
pub fn user_skills_dirs(home: &Path, key: &str) -> Vec<PathBuf> {
    user_folders(key)
        .map(|f| f.skills_dirs.iter().map(|d| home.join(d)).collect())
        .unwrap_or_default()
}

/// Returns the keys of the agents present for the user at `home`, in catalog
/// order.
///
/// An agent is present when its per-user configuration folder exists under
/// `home`. This is a pure filesystem check, independent of whether the
/// agent's CLI is installed or runnable (see
/// [`crate::get_installed_agents`] for that question).
pub fn detect_present_agents(home: &Path) -> Vec<&'static str> {
    USER_ROWS
        .iter()
        .filter(|r| home.join(r.config_dir).is_dir())
        .map(|r| r.key)
        .collect()
}

/// [`detect_present_agents`] for the current user's home directory. Returns
/// an empty list when the home directory cannot be determined.
pub fn detect_present_agents_for_current_user() -> Vec<&'static str> {
    dirs::home_dir()
        .map(|home| detect_present_agents(&home))
        .unwrap_or_default()
}

/// Returns the smallest set of per-user skills folders (joined onto `home`)
/// that together reach every agent in `keys`, each paired with the given
/// agents that read that folder.
///
/// The choice is a deterministic greedy set cover: at each step the folder
/// reaching the most still-unreached agents wins, ties going to
/// [`SHARED_SKILLS_DIR`] first and then to the lexicographically smallest
/// folder. Results are in choice order; agent keys within a folder follow
/// catalog order.
///
/// An agent that reads two chosen folders sees the same skill twice. A folder
/// is therefore never chosen when an agent in `keys` reads both it and an
/// already-chosen folder, unless that agent's
/// [`UserFolders::duplicate_skill_harmless`] flag is `true`.
///
/// Unknown keys, duplicate keys, agents with no per-user skills folder, and
/// agents only reachable through a folder the rule above excludes are left
/// out of the result.
pub fn fewest_skill_dirs(home: &Path, keys: &[&str]) -> Vec<(PathBuf, Vec<&'static str>)> {
    // Requested agents, deduplicated, in catalog order.
    let wanted: BTreeSet<&str> = keys.iter().copied().collect();
    let agents: Vec<(&'static str, UserFolders)> = USER_ROWS
        .iter()
        .filter(|r| wanted.contains(r.key))
        .map(|r| (r.key, folders_from_row(r)))
        .collect();

    let candidates: BTreeSet<&Path> = agents
        .iter()
        .flat_map(|(_, f)| f.skills_dirs.iter().map(PathBuf::as_path))
        .collect();

    let readers = |dir: &Path| -> Vec<usize> {
        agents
            .iter()
            .enumerate()
            .filter(|(_, (_, f))| f.skills_dirs.iter().any(|d| d == dir))
            .map(|(i, _)| i)
            .collect()
    };

    let mut chosen: Vec<&Path> = Vec::new();
    let mut reached = vec![false; agents.len()];

    loop {
        let mut best: Option<(&Path, usize)> = None;
        for &dir in &candidates {
            if chosen.contains(&dir) {
                continue;
            }
            let dir_readers = readers(dir);
            let allowed = dir_readers.iter().all(|&i| {
                let (_, f) = &agents[i];
                f.duplicate_skill_harmless
                    || !f.skills_dirs.iter().any(|d| chosen.contains(&d.as_path()))
            });
            if !allowed {
                continue;
            }
            let gain = dir_readers.iter().filter(|&&i| !reached[i]).count();
            if gain == 0 {
                continue;
            }
            let better = match best {
                None => true,
                Some((best_dir, best_gain)) => {
                    gain > best_gain
                        || (gain == best_gain
                            && dir == Path::new(SHARED_SKILLS_DIR)
                            && best_dir != Path::new(SHARED_SKILLS_DIR))
                }
            };
            if better {
                best = Some((dir, gain));
            }
        }
        let Some((dir, _)) = best else { break };
        for i in readers(dir) {
            reached[i] = true;
        }
        chosen.push(dir);
    }

    chosen
        .into_iter()
        .map(|dir| {
            let keys = readers(dir).into_iter().map(|i| agents[i].0).collect();
            (home.join(dir), keys)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const SHARED_READERS: &[&str] = &[
        "cursor", "codex", "gemini", "qwen", "copilot", "auggie", "roo", "opencode", "amp", "pi",
        "kilocode",
    ];

    #[test]
    fn every_catalog_agent_has_a_user_row_and_no_extra_rows() {
        let catalog: Vec<String> = crate::all_agents().into_iter().map(|a| a.key).collect();
        let rows: Vec<&str> = USER_ROWS.iter().map(|r| r.key).collect();
        assert_eq!(catalog, rows, "user rows must follow catalog order");
    }

    #[test]
    fn claude_reads_only_its_own_folder() {
        let f = user_folders("claude").unwrap();
        assert_eq!(f.config_dir, PathBuf::from(".claude"));
        assert_eq!(f.skills_dirs, vec![PathBuf::from(".claude/skills")]);
        assert!(!f.reads_shared);
        assert!(!f.duplicate_skill_harmless);
    }

    #[test]
    fn shared_readers_list_own_folder_first_then_shared() {
        for key in SHARED_READERS {
            let f = user_folders(key).unwrap();
            assert!(f.reads_shared, "{key}");
            assert!(!f.duplicate_skill_harmless, "{key}");
            assert_eq!(
                f.skills_dirs.last().unwrap(),
                Path::new(SHARED_SKILLS_DIR),
                "{key}"
            );
        }
        let cursor = user_folders("cursor").unwrap();
        assert_eq!(
            cursor.skills_dirs,
            vec![
                PathBuf::from(".cursor/skills"),
                PathBuf::from(SHARED_SKILLS_DIR)
            ]
        );
        assert_eq!(
            user_folders("opencode").unwrap().config_dir,
            PathBuf::from(".config/opencode")
        );
    }

    #[test]
    fn non_readers_do_not_read_shared() {
        for key in ["claude", "codebuddy", "qoder", "bob", "windsurf", "newton"] {
            let f = user_folders(key).unwrap();
            assert!(!f.reads_shared, "{key}");
            assert!(!f
                .skills_dirs
                .iter()
                .any(|d| d == Path::new(SHARED_SKILLS_DIR)));
        }
    }

    #[test]
    fn unknown_key_has_no_folders() {
        assert!(user_folders("not-an-agent").is_none());
        assert!(user_skills_dirs(Path::new("/h"), "not-an-agent").is_empty());
    }

    #[test]
    fn user_skills_dirs_joins_home() {
        let home = Path::new("/home/u");
        assert_eq!(
            user_skills_dirs(home, "codex"),
            vec![home.join(".codex/skills"), home.join(".agents/skills")]
        );
        assert!(user_skills_dirs(home, "bob").is_empty());
    }

    #[test]
    fn presence_is_config_folder_existence() {
        let home = TempDir::new().unwrap();
        assert!(detect_present_agents(home.path()).is_empty());

        std::fs::create_dir_all(home.path().join(".claude")).unwrap();
        std::fs::create_dir_all(home.path().join(".config/opencode")).unwrap();
        // A plain file named like a config folder does not count.
        std::fs::write(home.path().join(".cursor"), b"").unwrap();
        // The shared folder alone makes no agent present.
        std::fs::create_dir_all(home.path().join(".agents/skills")).unwrap();

        assert_eq!(
            detect_present_agents(home.path()),
            vec!["claude", "opencode"]
        );
    }

    #[test]
    fn fewest_prefers_shared_and_adds_own_folders_for_the_rest() {
        let home = Path::new("/h");
        let got = fewest_skill_dirs(home, &["claude", "cursor", "codex", "gemini"]);
        assert_eq!(
            got,
            vec![
                (
                    home.join(".agents/skills"),
                    vec!["gemini", "cursor", "codex"]
                ),
                (home.join(".claude/skills"), vec!["claude"]),
            ]
        );
    }

    #[test]
    fn fewest_prefers_shared_even_for_a_single_reader() {
        let home = Path::new("/h");
        assert_eq!(
            fewest_skill_dirs(home, &["cursor"]),
            vec![(home.join(".agents/skills"), vec!["cursor"])]
        );
    }

    #[test]
    fn fewest_skips_unknown_and_unreachable_and_dedups() {
        let home = Path::new("/h");
        assert_eq!(
            fewest_skill_dirs(home, &["bob", "nope", "claude", "claude"]),
            vec![(home.join(".claude/skills"), vec!["claude"])]
        );
        assert!(fewest_skill_dirs(home, &[]).is_empty());
    }

    #[test]
    fn fewest_never_doubles_up_an_agent_without_the_flag() {
        // Every requested agent reaches exactly one chosen folder unless its
        // flag allows duplicates.
        let home = Path::new("/h");
        let all: Vec<&str> = USER_ROWS.iter().map(|r| r.key).collect();
        let got = fewest_skill_dirs(home, &all);
        for key in &all {
            let f = user_folders(key).unwrap();
            let hits = got
                .iter()
                .filter(|(dir, _)| f.skills_dirs.iter().any(|d| home.join(d) == *dir))
                .count();
            if !f.duplicate_skill_harmless {
                assert!(hits <= 1, "{key} reads {hits} chosen folders");
            }
            if !f.skills_dirs.is_empty() {
                assert!(hits >= 1, "{key} unreached");
            }
        }
        // Deterministic: same input, same output.
        assert_eq!(got, fewest_skill_dirs(home, &all));
    }
}
