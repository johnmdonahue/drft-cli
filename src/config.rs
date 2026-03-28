use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleSeverity {
    Error,
    Warn,
    Off,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CustomRuleConfig {
    pub command: String,
    #[serde(default = "default_warn")]
    pub severity: RuleSeverity,
}

fn default_warn() -> RuleSeverity {
    RuleSeverity::Warn
}

#[derive(Debug, Clone)]
pub struct Config {
    pub ignore: Vec<String>,
    pub rules: HashMap<String, RuleSeverity>,
    pub ignore_rules: HashMap<String, Vec<String>>,
    pub custom_rules: HashMap<String, CustomRuleConfig>,
    ignore_rules_compiled: HashMap<String, Option<GlobSet>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RawConfig {
    ignore: Option<Vec<String>>,
    rules: Option<HashMap<String, RuleSeverity>>,
    ignore_rules: Option<HashMap<String, Vec<String>>>,
    custom_rules: Option<HashMap<String, CustomRuleConfig>>,
}

impl Config {
    pub fn defaults() -> Self {
        let rules = [
            ("broken-link", RuleSeverity::Warn),
            ("containment", RuleSeverity::Warn),
            ("cycle", RuleSeverity::Warn),
            ("directory-link", RuleSeverity::Warn),
            ("encapsulation", RuleSeverity::Warn),
            ("indirect-link", RuleSeverity::Off),
            ("orphan", RuleSeverity::Off),
            ("stale", RuleSeverity::Warn),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();

        Config {
            ignore: Vec::new(),
            rules,
            ignore_rules: HashMap::new(),
            custom_rules: HashMap::new(),
            ignore_rules_compiled: HashMap::new(),
        }
    }

    pub fn load(root: &Path) -> Result<Self> {
        let config_path = root.join("drft.toml");
        if !config_path.exists() {
            return Ok(Self::defaults());
        }

        let content = std::fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;

        let raw: RawConfig = toml::from_str(&content)
            .with_context(|| format!("failed to parse {}", config_path.display()))?;

        let mut config = Self::defaults();

        if let Some(ignore) = raw.ignore {
            config.ignore = ignore;
        }

        if let Some(custom_rules) = raw.custom_rules {
            config.custom_rules = custom_rules;
        }

        if let Some(ignore_rules) = raw.ignore_rules {
            for (rule_name, patterns) in &ignore_rules {
                let mut builder = GlobSetBuilder::new();
                for pattern in patterns {
                    builder
                        .add(Glob::new(pattern).with_context(|| {
                            format!("invalid glob in ignore-rules.{rule_name}")
                        })?);
                }
                let compiled = builder.build().with_context(|| {
                    format!("failed to compile globs for ignore-rules.{rule_name}")
                })?;
                config
                    .ignore_rules_compiled
                    .insert(rule_name.clone(), Some(compiled));
            }
            config.ignore_rules = ignore_rules;
        }

        if let Some(rules) = raw.rules {
            let known_rules: Vec<&str> = config.rules.keys().map(|s| s.as_str()).collect();
            for name in rules.keys() {
                if !known_rules.contains(&name.as_str()) {
                    eprintln!("warn: unknown rule \"{name}\" in drft.toml (ignored)");
                }
            }
            for (name, severity) in rules {
                config.rules.insert(name, severity);
            }
        }

        Ok(config)
    }

    pub fn rule_severity(&self, name: &str) -> RuleSeverity {
        self.rules.get(name).copied().unwrap_or(RuleSeverity::Off)
    }

    /// Check if a path should be ignored for a specific rule.
    pub fn is_rule_ignored(&self, rule: &str, path: &str) -> bool {
        if let Some(Some(glob_set)) = self.ignore_rules_compiled.get(rule) {
            glob_set.is_match(path)
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn defaults_when_no_config() {
        let dir = TempDir::new().unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.rule_severity("broken-link"), RuleSeverity::Warn);
        assert_eq!(config.rule_severity("orphan"), RuleSeverity::Off);
        assert!(config.ignore.is_empty());
    }

    #[test]
    fn loads_config_file() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[rules]\nbroken-link = \"error\"\norphan = \"warn\"\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.rule_severity("broken-link"), RuleSeverity::Error);
        assert_eq!(config.rule_severity("orphan"), RuleSeverity::Warn);
        assert_eq!(config.rule_severity("cycle"), RuleSeverity::Warn);
    }

    #[test]
    fn loads_ignore_rules() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[rules]\norphan = \"warn\"\n\n[ignore-rules]\norphan = [\"README.md\", \"index.md\"]\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert!(config.is_rule_ignored("orphan", "README.md"));
        assert!(config.is_rule_ignored("orphan", "index.md"));
        assert!(!config.is_rule_ignored("orphan", "other.md"));
        assert!(!config.is_rule_ignored("broken-link", "README.md"));
    }

    #[test]
    fn ignore_rules_supports_globs() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[ignore-rules]\nbroken-link = [\"drafts/*\"]\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert!(config.is_rule_ignored("broken-link", "drafts/wip.md"));
        assert!(!config.is_rule_ignored("broken-link", "index.md"));
    }

    #[test]
    fn invalid_toml_returns_error() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.toml"), "not valid toml {{{{").unwrap();
        assert!(Config::load(dir.path()).is_err());
    }
}
