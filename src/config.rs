use anyhow::{Context, Result};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// Compile a list of glob patterns into a GlobSet. Returns None if patterns is
/// empty. Uses `literal_separator` so `*` matches a single path component and
/// `**` matches across directory boundaries.
pub fn compile_globs(patterns: &[String]) -> Result<Option<GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(GlobBuilder::new(pattern).literal_separator(true).build()?);
    }
    Ok(Some(builder.build()?))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleSeverity {
    Error,
    Warn,
    Off,
}

// ── Graph config ───────────────────────────────────────────────

/// A configured graph: a source, a file filter, and a builder. v0.8 ships the
/// `fs` source and the `fs`/`markdown`/`frontmatter` builders.
#[derive(Debug, Clone)]
pub struct GraphConfig {
    pub source: String,
    pub filter: Vec<String>,
    pub builder: String,
}

#[derive(Debug, Deserialize)]
struct RawGraph {
    source: Option<String>,
    filter: Option<Vec<String>>,
    builder: String,
}

// ── Rule config ────────────────────────────────────────────────

/// Per-rule configuration: a severity and a list of ignore globs matched against
/// the finding's subject path.
#[derive(Debug, Clone)]
pub struct RuleConfig {
    pub severity: RuleSeverity,
    pub ignore: Vec<String>,
    ignore_compiled: Option<GlobSet>,
}

impl RuleConfig {
    fn new(severity: RuleSeverity, ignore: Vec<String>) -> Result<Self> {
        let ignore_compiled = compile_globs(&ignore).context("failed to compile ignore globs")?;
        Ok(Self {
            severity,
            ignore,
            ignore_compiled,
        })
    }

    pub fn is_path_ignored(&self, path: &str) -> bool {
        self.ignore_compiled
            .as_ref()
            .is_some_and(|set| set.is_match(path))
    }
}

/// Serde helper: a rule is either a bare severity (`stale-node = "error"`) or a
/// table (`[rules.stale-node]` with `severity` and `ignore`).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawRuleValue {
    Severity(RuleSeverity),
    Table {
        #[serde(default = "default_warn")]
        severity: RuleSeverity,
        #[serde(default)]
        ignore: Vec<String>,
    },
}

fn default_warn() -> RuleSeverity {
    RuleSeverity::Warn
}

// ── Config ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Config {
    /// Glob patterns the `fs` walk removes from the graph (also respects
    /// `.gitignore`).
    pub ignore: Vec<String>,
    /// Configured graphs, keyed by name. `fs` is implicit and always built.
    pub graphs: BTreeMap<String, GraphConfig>,
    pub rules: HashMap<String, RuleConfig>,
    /// Directory containing the `drft.toml` this config was loaded from.
    pub config_dir: Option<std::path::PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct RawConfig {
    ignore: Option<Vec<String>>,
    graphs: Option<HashMap<String, RawGraph>>,
    rules: Option<HashMap<String, RawRuleValue>>,
}

/// Names of all built-in rules (for unknown-rule warnings).
const BUILTIN_RULES: &[&str] = &[
    "stale-node",
    "stale-edge",
    "new-edge",
    "removed-edge",
    "removed-node",
    "unresolved-edge",
    "detached-node",
];

const DEFAULT_FILTER: &str = "**/*.md";

impl Config {
    /// Defaults when no `drft.toml` overrides them: the `markdown` and
    /// `frontmatter` text graphs over markdown files, every rule at `warn`.
    pub fn defaults() -> Self {
        let mut graphs = BTreeMap::new();
        for builder in ["markdown", "frontmatter"] {
            graphs.insert(
                builder.to_string(),
                GraphConfig {
                    source: "fs".to_string(),
                    filter: vec![DEFAULT_FILTER.to_string()],
                    builder: builder.to_string(),
                },
            );
        }
        Config {
            ignore: Vec::new(),
            graphs,
            rules: HashMap::new(),
            config_dir: None,
        }
    }

    pub fn load(root: &Path) -> Result<Self> {
        let config_path = match Self::find_config(root) {
            Some(p) => p,
            None => anyhow::bail!("no drft.toml found (run `drft init` to create one)"),
        };

        let content = std::fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        let raw: RawConfig = toml::from_str(&content)
            .with_context(|| format!("failed to parse {}", config_path.display()))?;

        let mut config = Self::defaults();
        config.config_dir = config_path.parent().map(|p| p.to_path_buf());

        if let Some(ignore) = raw.ignore {
            config.ignore = ignore;
        }

        // Declaring any graph replaces the defaults.
        if let Some(raw_graphs) = raw.graphs {
            config.graphs.clear();
            for (name, raw) in raw_graphs {
                config.graphs.insert(
                    name,
                    GraphConfig {
                        source: raw.source.unwrap_or_else(|| "fs".to_string()),
                        filter: raw
                            .filter
                            .unwrap_or_else(|| vec![DEFAULT_FILTER.to_string()]),
                        builder: raw.builder,
                    },
                );
            }
        }

        if let Some(raw_rules) = raw.rules {
            for (name, value) in raw_rules {
                let rule_config = match value {
                    RawRuleValue::Severity(severity) => RuleConfig::new(severity, Vec::new())?,
                    RawRuleValue::Table { severity, ignore } => {
                        RuleConfig::new(severity, ignore)
                            .with_context(|| format!("invalid globs in rules.{name}"))?
                    }
                };
                if !BUILTIN_RULES.contains(&name.as_str()) {
                    eprintln!("warn: unknown rule \"{name}\" in drft.toml (ignored)");
                }
                config.rules.insert(name, rule_config);
            }
        }

        Ok(config)
    }

    /// Find `drft.toml` in `root`. No directory walking — if it's not here, the
    /// caller falls back to defaults or errors.
    fn find_config(root: &Path) -> Option<std::path::PathBuf> {
        let candidate = root.join("drft.toml");
        candidate.exists().then_some(candidate)
    }

    /// Glob patterns the `fs` walk removes from the graph.
    pub fn ignore_patterns(&self) -> &[String] {
        &self.ignore
    }

    /// Whether `path` is ignored for `rule`.
    pub fn is_rule_ignored(&self, rule: &str, path: &str) -> bool {
        self.rules
            .get(rule)
            .is_some_and(|r| r.is_path_ignored(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn errors_when_no_config() {
        let dir = TempDir::new().unwrap();
        let result = Config::load(dir.path());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no drft.toml found")
        );
    }

    #[test]
    fn defaults_enable_text_graphs() {
        let config = Config::defaults();
        assert_eq!(config.graphs["markdown"].builder, "markdown");
        assert_eq!(config.graphs["frontmatter"].builder, "frontmatter");
    }

    #[test]
    fn loads_ignore() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.toml"), "ignore = [\"target/**\"]\n").unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.ignore, vec!["target/**"]);
    }

    #[test]
    fn declaring_graphs_replaces_defaults() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[graphs.markdown]\nsource = \"fs\"\nfilter = [\"docs/**/*.md\"]\nbuilder = \"markdown\"\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.graphs.len(), 1);
        assert_eq!(config.graphs["markdown"].filter, vec!["docs/**/*.md"]);
        assert!(!config.graphs.contains_key("frontmatter"));
    }

    #[test]
    fn loads_rule_severity_and_ignore() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[rules]\nstale-node = \"error\"\n\n[rules.detached-node]\nignore = [\"README.md\"]\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.rules["stale-node"].severity, RuleSeverity::Error);
        assert!(config.is_rule_ignored("detached-node", "README.md"));
        assert!(!config.is_rule_ignored("detached-node", "other.md"));
    }

    #[test]
    fn invalid_toml_errors() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.toml"), "not valid toml {{{{").unwrap();
        assert!(Config::load(dir.path()).is_err());
    }
}
