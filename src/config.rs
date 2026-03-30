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

// ── Parser config ──────────────────────────────────────────────

/// Configuration for a single parser under `[parsers]`.
/// Supports shorthand (`markdown = true`, `markdown = ["frontmatter"]`)
/// and expanded table form (`[parsers.markdown]` with fields).
#[derive(Debug, Clone)]
pub struct ParserConfig {
    pub glob: Option<String>,
    pub types: Option<Vec<String>>,
    pub command: Option<String>,
    pub timeout: Option<u64>,
}

/// Serde helper: untagged enum to parse shorthand or table forms.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawParserValue {
    /// `markdown = true`
    Bool(bool),
    /// `markdown = ["frontmatter", "wikilink"]`
    Types(Vec<String>),
    /// `[parsers.markdown]` with fields
    Table {
        glob: Option<String>,
        types: Option<Vec<String>>,
        command: Option<String>,
        timeout: Option<u64>,
    },
}

impl From<RawParserValue> for Option<ParserConfig> {
    fn from(val: RawParserValue) -> Self {
        match val {
            RawParserValue::Bool(false) => None,
            RawParserValue::Bool(true) => Some(ParserConfig {
                glob: None,
                types: None,
                command: None,
                timeout: None,
            }),
            RawParserValue::Types(types) => Some(ParserConfig {
                glob: None,
                types: Some(types),
                command: None,
                timeout: None,
            }),
            RawParserValue::Table {
                glob,
                types,
                command,
                timeout,
            } => Some(ParserConfig {
                glob,
                types,
                command,
                timeout,
            }),
        }
    }
}

// ── Interface config ───────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct InterfaceConfig {
    pub nodes: Vec<String>,
}

// ── Rule config ────────────────────────────────────────────────

/// Configuration for a single rule under `[rules]`.
/// Supports shorthand (`cycle = "warn"`) and table form (`[rules.orphan]`).
#[derive(Debug, Clone)]
pub struct RuleConfig {
    pub severity: RuleSeverity,
    #[allow(dead_code)]
    pub ignore: Vec<String>,
    pub command: Option<String>,
    #[allow(dead_code)]
    pub timeout: Option<u64>,
    pub(crate) ignore_compiled: Option<GlobSet>,
}

impl RuleConfig {
    pub fn is_path_ignored(&self, path: &str) -> bool {
        if let Some(ref glob_set) = self.ignore_compiled {
            glob_set.is_match(path)
        } else {
            false
        }
    }
}

/// Serde helper: untagged enum for shorthand or table forms.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawRuleValue {
    /// `cycle = "warn"`
    Severity(RuleSeverity),
    /// `[rules.orphan]` with fields
    Table {
        #[serde(default = "default_warn")]
        severity: RuleSeverity,
        #[serde(default)]
        ignore: Vec<String>,
        command: Option<String>,
        timeout: Option<u64>,
    },
}

fn default_warn() -> RuleSeverity {
    RuleSeverity::Warn
}

// ── Config ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Config {
    /// Glob patterns declaring which filesystem paths become File nodes.
    /// Default: `["*.md"]`.
    pub include: Vec<String>,
    /// Glob patterns removed from the graph (applied after `include`).
    /// Also respects `.gitignore`.
    pub exclude: Vec<String>,
    pub interface: Option<InterfaceConfig>,
    pub parsers: HashMap<String, ParserConfig>,
    pub rules: HashMap<String, RuleConfig>,
    /// Directory containing the drft.toml this config was loaded from.
    pub config_dir: Option<std::path::PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RawConfig {
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    interface: Option<InterfaceConfig>,
    parsers: Option<HashMap<String, RawParserValue>>,
    rules: Option<HashMap<String, RawRuleValue>>,
    // v0.3 key — accepted as alias for `exclude`
    ignore: Option<Vec<String>>,
    // v0.2 keys — detected for migration warnings
    manifest: Option<toml::Value>,
    custom_rules: Option<toml::Value>,
    custom_analyses: Option<toml::Value>,
    custom_metrics: Option<toml::Value>,
    ignore_rules: Option<toml::Value>,
}

/// Names of all built-in rules (for unknown-rule warnings).
const BUILTIN_RULES: &[&str] = &[
    "boundary-violation",
    "dangling-edge",
    "directed-cycle",
    "directory-edge",
    "encapsulation-violation",
    "fragility",
    "fragmentation",
    "layer-violation",
    "orphan-node",
    "redundant-edge",
    "stale",
    "symlink-edge",
];

impl Config {
    pub fn defaults() -> Self {
        // When no drft.toml exists, default to markdown parser enabled
        let mut parsers = HashMap::new();
        parsers.insert(
            "markdown".to_string(),
            ParserConfig {
                glob: None,
                types: None,
                command: None,
                timeout: None,
            },
        );

        let rules = [
            ("boundary-violation", RuleSeverity::Warn),
            ("dangling-edge", RuleSeverity::Warn),
            ("directed-cycle", RuleSeverity::Warn),
            ("directory-edge", RuleSeverity::Warn),
            ("encapsulation-violation", RuleSeverity::Warn),
            ("fragility", RuleSeverity::Warn),
            ("fragmentation", RuleSeverity::Warn),
            ("layer-violation", RuleSeverity::Warn),
            ("orphan-node", RuleSeverity::Warn),
            ("redundant-edge", RuleSeverity::Warn),
            ("stale", RuleSeverity::Warn),
            ("symlink-edge", RuleSeverity::Warn),
        ]
        .into_iter()
        .map(|(k, v)| {
            (
                k.to_string(),
                RuleConfig {
                    severity: v,
                    ignore: Vec::new(),
                    command: None,
                    timeout: None,
                    ignore_compiled: None,
                },
            )
        })
        .collect();

        Config {
            include: vec!["*.md".to_string()],
            exclude: Vec::new(),
            interface: None,
            parsers,
            rules,
            config_dir: None,
        }
    }

    pub fn load(root: &Path) -> Result<Self> {
        let config_path = Self::find_config(root);
        let config_path = match config_path {
            Some(p) => p,
            None => return Ok(Self::defaults()),
        };

        let content = std::fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;

        let raw: RawConfig = toml::from_str(&content)
            .with_context(|| format!("failed to parse {}", config_path.display()))?;

        // Warn about v0.2 config keys
        if raw.manifest.is_some() {
            eprintln!("warn: drft.toml uses v0.2 'manifest' key — migrate to [interface] section");
        }
        if raw.custom_rules.is_some() {
            eprintln!(
                "warn: drft.toml uses v0.2 [custom-rules] — migrate to [rules] with 'command' field"
            );
        }
        if raw.custom_analyses.is_some() {
            eprintln!(
                "warn: drft.toml uses v0.2 [custom-analyses] — custom analyses are no longer supported"
            );
        }
        if raw.custom_metrics.is_some() {
            eprintln!(
                "warn: drft.toml uses v0.2 [custom-metrics] — custom metrics are no longer supported"
            );
        }
        if raw.ignore_rules.is_some() {
            eprintln!(
                "warn: drft.toml uses v0.2 [ignore-rules] — migrate to per-rule 'ignore' field"
            );
        }

        let mut config = Self::defaults();
        config.config_dir = config_path.parent().map(|p| p.to_path_buf());

        if let Some(include) = raw.include {
            config.include = include;
        }

        // `ignore` is the v0.3 name for `exclude` — accept with warning
        if raw.ignore.is_some() && raw.exclude.is_some() {
            anyhow::bail!(
                "drft.toml has both 'ignore' and 'exclude' — remove 'ignore' (renamed to 'exclude' in v0.4)"
            );
        }
        if let Some(ignore) = raw.ignore {
            eprintln!("warn: drft.toml uses 'ignore' — rename to 'exclude' (v0.4)");
            config.exclude = ignore;
        }
        if let Some(exclude) = raw.exclude {
            config.exclude = exclude;
        }

        config.interface = raw.interface;

        // Parse parsers
        if let Some(raw_parsers) = raw.parsers {
            config.parsers.clear();
            for (name, value) in raw_parsers {
                if let Some(parser_config) = Option::<ParserConfig>::from(value) {
                    config.parsers.insert(name, parser_config);
                }
            }
        }

        // Parse rules (unified: built-in severities + table form + script rules)
        if let Some(raw_rules) = raw.rules {
            for (name, value) in raw_rules {
                let rule_config = match value {
                    RawRuleValue::Severity(severity) => RuleConfig {
                        severity,
                        ignore: Vec::new(),
                        command: None,
                        timeout: None,
                        ignore_compiled: None,
                    },
                    RawRuleValue::Table {
                        severity,
                        ignore,
                        command,
                        timeout,
                    } => {
                        let compiled = if ignore.is_empty() {
                            None
                        } else {
                            let mut builder = GlobSetBuilder::new();
                            for pattern in &ignore {
                                builder.add(Glob::new(pattern).with_context(|| {
                                    format!("invalid glob in rules.{name}.ignore")
                                })?);
                            }
                            Some(builder.build().with_context(|| {
                                format!("failed to compile globs for rules.{name}.ignore")
                            })?)
                        };
                        RuleConfig {
                            severity,
                            ignore,
                            command,
                            timeout,
                            ignore_compiled: compiled,
                        }
                    }
                };

                // Warn about unknown built-in rules (but allow script rules with command)
                if rule_config.command.is_none() && !BUILTIN_RULES.contains(&name.as_str()) {
                    eprintln!("warn: unknown rule \"{name}\" in drft.toml (ignored)");
                }

                config.rules.insert(name, rule_config);
            }
        }

        Ok(config)
    }

    /// Find the nearest drft.toml by walking up from `root`.
    fn find_config(root: &Path) -> Option<std::path::PathBuf> {
        let mut current = root.to_path_buf();
        loop {
            let candidate = current.join("drft.toml");
            if candidate.exists() {
                return Some(candidate);
            }
            if !current.pop() {
                return None;
            }
        }
    }

    pub fn rule_severity(&self, name: &str) -> RuleSeverity {
        self.rules
            .get(name)
            .map(|r| r.severity)
            .unwrap_or(RuleSeverity::Off)
    }

    /// Check if a path should be ignored for a specific rule.
    pub fn is_rule_ignored(&self, rule: &str, path: &str) -> bool {
        self.rules
            .get(rule)
            .is_some_and(|r| r.is_path_ignored(path))
    }

    /// Get script rules (rules with a command field).
    pub fn script_rules(&self) -> impl Iterator<Item = (&str, &RuleConfig)> {
        self.rules
            .iter()
            .filter(|(_, r)| r.command.is_some())
            .map(|(name, config)| (name.as_str(), config))
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
        assert_eq!(config.rule_severity("dangling-edge"), RuleSeverity::Warn);
        assert_eq!(config.rule_severity("orphan-node"), RuleSeverity::Warn);
        assert_eq!(config.include, vec!["*.md"]);
        assert!(config.exclude.is_empty());
        assert!(config.parsers.contains_key("markdown"));
    }

    #[test]
    fn loads_rule_severities() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[rules]\ndangling-edge = \"error\"\norphan-node = \"warn\"\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.rule_severity("dangling-edge"), RuleSeverity::Error);
        assert_eq!(config.rule_severity("orphan-node"), RuleSeverity::Warn);
        assert_eq!(config.rule_severity("directed-cycle"), RuleSeverity::Warn);
    }

    #[test]
    fn loads_rule_with_ignore() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[rules.orphan-node]\nseverity = \"warn\"\nignore = [\"README.md\", \"index.md\"]\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.rule_severity("orphan-node"), RuleSeverity::Warn);
        assert!(config.is_rule_ignored("orphan-node", "README.md"));
        assert!(config.is_rule_ignored("orphan-node", "index.md"));
        assert!(!config.is_rule_ignored("orphan-node", "other.md"));
        assert!(!config.is_rule_ignored("dangling-edge", "README.md"));
    }

    #[test]
    fn loads_parser_shorthand_bool() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.toml"), "[parsers]\nmarkdown = true\n").unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert!(config.parsers.contains_key("markdown"));
        let p = &config.parsers["markdown"];
        assert!(p.glob.is_none());
        assert!(p.types.is_none());
        assert!(p.command.is_none());
    }

    #[test]
    fn loads_parser_shorthand_types() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[parsers]\nmarkdown = [\"frontmatter\", \"wikilink\"]\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        let p = &config.parsers["markdown"];
        assert_eq!(
            p.types.as_deref(),
            Some(vec!["frontmatter".to_string(), "wikilink".to_string()]).as_deref()
        );
    }

    #[test]
    fn loads_parser_table() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[parsers.tsx]\nglob = \"*.tsx\"\ncommand = \"./parse.sh\"\ntimeout = 10000\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        let p = &config.parsers["tsx"];
        assert_eq!(p.glob.as_deref(), Some("*.tsx"));
        assert_eq!(p.command.as_deref(), Some("./parse.sh"));
        assert_eq!(p.timeout, Some(10000));
    }

    #[test]
    fn parser_false_disables() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[parsers]\nmarkdown = false\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert!(!config.parsers.contains_key("markdown"));
    }

    #[test]
    fn loads_interface() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[interface]\nnodes = [\"overview.md\", \"api/*.md\"]\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        let iface = config.interface.unwrap();
        assert_eq!(iface.nodes, vec!["overview.md", "api/*.md"]);
    }

    #[test]
    fn loads_script_rule() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[rules.my-check]\ncommand = \"./check.sh\"\nseverity = \"warn\"\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        let script_rules: Vec<_> = config.script_rules().collect();
        assert_eq!(script_rules.len(), 1);
        assert_eq!(script_rules[0].0, "my-check");
        assert_eq!(script_rules[0].1.command.as_deref(), Some("./check.sh"));
    }

    #[test]
    fn loads_include_exclude() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "include = [\"*.md\", \"*.yaml\"]\nexclude = [\"drafts/*\"]\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.include, vec!["*.md", "*.yaml"]);
        assert_eq!(config.exclude, vec!["drafts/*"]);
    }

    #[test]
    fn ignore_migrates_to_exclude() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.toml"), "ignore = [\"drafts/*\"]\n").unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.exclude, vec!["drafts/*"]);
    }

    #[test]
    fn ignore_and_exclude_conflicts() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "ignore = [\"a/*\"]\nexclude = [\"b/*\"]\n",
        )
        .unwrap();
        assert!(Config::load(dir.path()).is_err());
    }

    #[test]
    fn invalid_toml_returns_error() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.toml"), "not valid toml {{{{").unwrap();
        assert!(Config::load(dir.path()).is_err());
    }

    #[test]
    fn inherits_config_from_parent() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[rules]\norphan-node = \"error\"\n",
        )
        .unwrap();

        let child = dir.path().join("child");
        fs::create_dir(&child).unwrap();

        let config = Config::load(&child).unwrap();
        assert_eq!(config.rule_severity("orphan-node"), RuleSeverity::Error);
    }

    #[test]
    fn child_config_overrides_parent() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[rules]\norphan-node = \"error\"\n",
        )
        .unwrap();

        let child = dir.path().join("child");
        fs::create_dir(&child).unwrap();
        fs::write(child.join("drft.toml"), "[rules]\norphan-node = \"off\"\n").unwrap();

        let config = Config::load(&child).unwrap();
        assert_eq!(config.rule_severity("orphan-node"), RuleSeverity::Off);
    }
}
