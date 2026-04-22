use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Smartest,
    Smart,
    Normal,
    Simple,
}

impl Tier {
    #[allow(dead_code)]
    pub fn all_names() -> &'static [&'static str] {
        &["smartest", "smart", "normal", "simple"]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Tier::Smartest => "smartest",
            Tier::Smart => "smart",
            Tier::Normal => "normal",
            Tier::Simple => "simple",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentModelPair {
    pub agent: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierEntry {
    pub pairs: Vec<AgentModelPair>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FallbackConfig {
    #[serde(default)]
    pub tiers: HashMap<String, TierEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AutoSection {
    #[serde(default)]
    auto: AutoTiers,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AutoTiers {
    #[serde(default)]
    tiers: HashMap<String, TierEntry>,
}

#[derive(Debug, thiserror::Error)]
pub enum FallbackError {
    #[error("Unknown tier '{0}'. Valid tiers: smartest, smart, normal, simple")]
    UnknownTier(String),

    #[error("No runnable agent found for tier '{tier}'. Checked: {checked}",
        checked = .checked.join(", "))]
    NoRunnableAgent { tier: String, checked: Vec<String> },

    #[error("Tier '{0}' has no candidates configured")]
    EmptyTier(String),

    #[error("Auto tier configuration not found. Define [auto.tiers.*] in .aikit/config.toml or ~/.aikit/config.toml")]
    ConfigNotFound,

    #[error("Failed to parse auto tier config at '{path}': {reason}")]
    ConfigParse { path: String, reason: String },

    #[error("Tier '{0}' is not configured. Configure it under [auto.tiers.{0}]")]
    TierNotConfigured(String),
}

pub fn parse_tier(s: &str) -> Result<Tier, FallbackError> {
    match s.to_lowercase().as_str() {
        "smartest" => Ok(Tier::Smartest),
        "smart" => Ok(Tier::Smart),
        "normal" => Ok(Tier::Normal),
        "simple" => Ok(Tier::Simple),
        other => Err(FallbackError::UnknownTier(other.to_string())),
    }
}

pub fn load_fallback_config() -> Result<FallbackConfig, FallbackError> {
    load_fallback_config_from_paths(&[
        crate::models::config::ConfigPaths::local_config(),
        crate::models::config::ConfigPaths::global_config(),
    ])
}

fn load_fallback_config_from_paths(
    paths: &[std::path::PathBuf],
) -> Result<FallbackConfig, FallbackError> {
    for path in paths {
        if path.exists() {
            let content =
                std::fs::read_to_string(path).map_err(|e| FallbackError::ConfigParse {
                    path: path.display().to_string(),
                    reason: e.to_string(),
                })?;
            let auto_section: AutoSection =
                toml::from_str(&content).map_err(|e| FallbackError::ConfigParse {
                    path: path.display().to_string(),
                    reason: e.to_string(),
                })?;
            if !auto_section.auto.tiers.is_empty() {
                return Ok(FallbackConfig {
                    tiers: auto_section.auto.tiers,
                });
            }
        }
    }
    Err(FallbackError::ConfigNotFound)
}

pub fn resolve_auto(tier: &Tier, config: &FallbackConfig) -> Result<AgentModelPair, FallbackError> {
    let tier_name = tier.as_str();
    let entry = config
        .tiers
        .get(tier_name)
        .ok_or_else(|| FallbackError::TierNotConfigured(tier_name.to_string()))?;

    if entry.pairs.is_empty() {
        return Err(FallbackError::EmptyTier(tier_name.to_string()));
    }

    let mut checked: Vec<String> = Vec::new();
    for pair in &entry.pairs {
        checked.push(pair.agent.clone());
        if aikit_sdk::is_runnable(&pair.agent) {
            return Ok(pair.clone());
        }
    }

    Err(FallbackError::NoRunnableAgent {
        tier: tier_name.to_string(),
        checked,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_parse_tier_smartest() {
        assert_eq!(parse_tier("smartest").unwrap(), Tier::Smartest);
    }

    #[test]
    fn test_parse_tier_smart() {
        assert_eq!(parse_tier("smart").unwrap(), Tier::Smart);
    }

    #[test]
    fn test_parse_tier_normal() {
        assert_eq!(parse_tier("normal").unwrap(), Tier::Normal);
    }

    #[test]
    fn test_parse_tier_simple() {
        assert_eq!(parse_tier("simple").unwrap(), Tier::Simple);
    }

    #[test]
    fn test_parse_tier_uppercase() {
        assert_eq!(parse_tier("SMART").unwrap(), Tier::Smart);
    }

    #[test]
    fn test_parse_tier_mixed_case() {
        assert_eq!(parse_tier("Smartest").unwrap(), Tier::Smartest);
    }

    #[test]
    fn test_parse_tier_unknown() {
        let err = parse_tier("bogus").unwrap_err();
        match err {
            FallbackError::UnknownTier(name) => assert_eq!(name, "bogus"),
            other => panic!("expected UnknownTier, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_tier_empty_string() {
        let err = parse_tier("").unwrap_err();
        match err {
            FallbackError::UnknownTier(name) => assert_eq!(name, ""),
            other => panic!("expected UnknownTier, got {:?}", other),
        }
    }

    #[test]
    fn test_load_fallback_config_no_files() {
        let tmp = tempfile::tempdir().unwrap();
        let p1 = tmp.path().join("nonexistent1.toml");
        let p2 = tmp.path().join("nonexistent2.toml");
        let result = load_fallback_config_from_paths(&[p1, p2]);
        assert!(matches!(result, Err(FallbackError::ConfigNotFound)));
    }

    #[test]
    fn test_load_fallback_config_parse_error() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("config.toml");
        let mut f = std::fs::File::create(&file_path).unwrap();
        write!(f, "[auto\ntiers = {{}}").unwrap();
        let result = load_fallback_config_from_paths(std::slice::from_ref(&file_path));
        match result {
            Err(FallbackError::ConfigParse { path, .. }) => {
                assert!(path.contains("config.toml"));
            }
            other => panic!("expected ConfigParse, got {:?}", other),
        }
    }

    #[test]
    fn test_load_fallback_config_empty_auto_section() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("config.toml");
        let mut f = std::fs::File::create(&file_path).unwrap();
        writeln!(f, "[auto]").unwrap();
        let other_path = tmp.path().join("other.toml");
        let result = load_fallback_config_from_paths(&[file_path, other_path]);
        assert!(matches!(result, Err(FallbackError::ConfigNotFound)));
    }

    #[test]
    fn test_load_fallback_config_valid() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("config.toml");
        let mut f = std::fs::File::create(&file_path).unwrap();
        write!(
            f,
            "[auto.tiers.smartest]\npairs = [\n  {{ agent = \"claude\", model = \"opus\" }},\n]\n"
        )
        .unwrap();
        let result = load_fallback_config_from_paths(&[file_path]);
        let config = result.unwrap();
        assert!(config.tiers.contains_key("smartest"));
        let entry = config.tiers.get("smartest").unwrap();
        assert_eq!(entry.pairs.len(), 1);
        assert_eq!(entry.pairs[0].agent, "claude");
        assert_eq!(entry.pairs[0].model, "opus");
    }

    #[test]
    fn test_resolve_auto_first_candidate_available() {
        let mut config = FallbackConfig::default();
        let entry = TierEntry {
            pairs: vec![
                AgentModelPair {
                    agent: "claude".to_string(),
                    model: "opus".to_string(),
                },
                AgentModelPair {
                    agent: "codex".to_string(),
                    model: "gpt5.4".to_string(),
                },
            ],
        };
        config.tiers.insert("smartest".to_string(), entry);
        let result = resolve_auto(&Tier::Smartest, &config).unwrap();
        assert_eq!(result.agent, "claude");
        assert_eq!(result.model, "opus");
    }

    #[test]
    fn test_resolve_auto_second_candidate_used() {
        let mut config = FallbackConfig::default();
        let entry = TierEntry {
            pairs: vec![
                AgentModelPair {
                    agent: "nonexistent_agent".to_string(),
                    model: "x".to_string(),
                },
                AgentModelPair {
                    agent: "claude".to_string(),
                    model: "sonnet".to_string(),
                },
            ],
        };
        config.tiers.insert("smart".to_string(), entry);
        let result = resolve_auto(&Tier::Smart, &config).unwrap();
        assert_eq!(result.agent, "claude");
        assert_eq!(result.model, "sonnet");
    }

    #[test]
    fn test_resolve_auto_no_runnable() {
        let mut config = FallbackConfig::default();
        let entry = TierEntry {
            pairs: vec![
                AgentModelPair {
                    agent: "nonexistent1".to_string(),
                    model: "a".to_string(),
                },
                AgentModelPair {
                    agent: "nonexistent2".to_string(),
                    model: "b".to_string(),
                },
            ],
        };
        config.tiers.insert("simple".to_string(), entry);
        let result = resolve_auto(&Tier::Simple, &config);
        match result {
            Err(FallbackError::NoRunnableAgent { tier, checked }) => {
                assert_eq!(tier, "simple");
                assert_eq!(checked, vec!["nonexistent1", "nonexistent2"]);
            }
            other => panic!("expected NoRunnableAgent, got {:?}", other),
        }
    }

    #[test]
    fn test_resolve_auto_empty_pairs() {
        let mut config = FallbackConfig::default();
        config
            .tiers
            .insert("normal".to_string(), TierEntry { pairs: vec![] });
        let result = resolve_auto(&Tier::Normal, &config);
        match result {
            Err(FallbackError::EmptyTier(name)) => assert_eq!(name, "normal"),
            other => panic!("expected EmptyTier, got {:?}", other),
        }
    }

    #[test]
    fn test_resolve_auto_tier_not_configured() {
        let config = FallbackConfig::default();
        let result = resolve_auto(&Tier::Smartest, &config);
        match result {
            Err(FallbackError::TierNotConfigured(name)) => assert_eq!(name, "smartest"),
            other => panic!("expected TierNotConfigured, got {:?}", other),
        }
    }

    #[test]
    fn test_tier_as_str() {
        assert_eq!(Tier::Smartest.as_str(), "smartest");
        assert_eq!(Tier::Smart.as_str(), "smart");
        assert_eq!(Tier::Normal.as_str(), "normal");
        assert_eq!(Tier::Simple.as_str(), "simple");
    }

    #[test]
    fn test_tier_all_names() {
        let names = Tier::all_names();
        assert_eq!(names, &["smartest", "smart", "normal", "simple"]);
    }
}
