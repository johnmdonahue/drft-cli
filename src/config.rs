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

/// A configured graph: a file scope and the parser that interprets it. v0.8
/// ships the `markdown` and `frontmatter` parsers. `fs` is the implicit base
/// graph (a provider, not a parser) and is not configured here.
#[derive(Debug, Clone)]
pub struct GraphConfig {
    pub files: Vec<String>,
    pub parser: String,
}

#[derive(Debug, Deserialize)]
struct RawGraph {
    files: Option<Vec<String>>,
    parser: String,
}

// ── Rule config ────────────────────────────────────────────────

/// Per-rule configuration: a severity and a list of ignore globs matched against
/// the finding's subject path.
#[derive(Debug, Clone)]
pub struct RuleConfig {
    pub severity: RuleSeverity,
    ignore_compiled: Option<GlobSet>,
}

impl RuleConfig {
    fn new(severity: RuleSeverity, ignore: Vec<String>) -> Result<Self> {
        let ignore_compiled = compile_globs(&ignore).context("failed to compile ignore globs")?;
        Ok(Self {
            severity,
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

/// Parsers a graph may declare. `fs` is the implicit base graph (a provider, not
/// a parser) and is intentionally absent — `parser = "fs"` is rejected.
const KNOWN_PARSERS: &[&str] = &["markdown", "frontmatter"];

/// Graph names reserved for drft's implicit graphs. Declaring one would collide
/// with the core `@fs` namespace at compose and overwrite its `type`/`hash`.
const RESERVED_GRAPH_NAMES: &[&str] = &["fs"];

/// A graph's `files` scope defaults to markdown when omitted.
const DEFAULT_FILES: &str = "**/*.md";

impl Config {
    /// The base config: no graphs (the `drft.toml` declares the full set), no
    /// ignores, every rule at `warn`. `fs` is always built regardless.
    pub fn defaults() -> Self {
        Config {
            ignore: Vec::new(),
            graphs: BTreeMap::new(),
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

        // The drft.toml declares the full graph set — there are no defaults.
        if let Some(raw_graphs) = raw.graphs {
            for (name, raw) in raw_graphs {
                crate::model::validate_label(&name)
                    .map_err(|e| anyhow::anyhow!("invalid graph name in drft.toml: {e}"))?;
                if RESERVED_GRAPH_NAMES.contains(&name.as_str()) {
                    anyhow::bail!("graph name \"{name}\" is reserved (the implicit base graph)");
                }
                if !KNOWN_PARSERS.contains(&raw.parser.as_str()) {
                    anyhow::bail!(
                        "unknown parser \"{}\" for graph \"{name}\" (known: {})",
                        raw.parser,
                        KNOWN_PARSERS.join(", ")
                    );
                }
                config.graphs.insert(
                    name,
                    GraphConfig {
                        files: raw.files.unwrap_or_else(|| vec![DEFAULT_FILES.to_string()]),
                        parser: raw.parser,
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
    fn defaults_have_no_graphs() {
        // No runtime defaults — the drft.toml declares the full set.
        assert!(Config::defaults().graphs.is_empty());
    }

    #[test]
    fn loads_ignore() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.toml"), "ignore = [\"target/**\"]\n").unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.ignore, vec!["target/**"]);
    }

    #[test]
    fn declares_graphs() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[graphs.docs]\nparser = \"markdown\"\nfiles = [\"docs/**/*.md\"]\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.graphs.len(), 1);
        assert_eq!(config.graphs["docs"].parser, "markdown");
        assert_eq!(config.graphs["docs"].files, vec!["docs/**/*.md"]);
    }

    #[test]
    fn files_defaults_to_markdown_when_omitted() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[graphs.markdown]\nparser = \"markdown\"\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.graphs["markdown"].files, vec!["**/*.md"]);
    }

    #[test]
    fn unknown_parser_errors() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[graphs.x]\nparser = \"markdwn\"\n",
        )
        .unwrap();
        let err = Config::load(dir.path()).unwrap_err().to_string();
        assert!(err.contains("unknown parser"), "got: {err}");
    }

    #[test]
    fn parser_fs_value_errors() {
        // `fs` is a provider, not a parser, so it's not a valid parser value.
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[graphs.x]\nparser = \"fs\"\n",
        )
        .unwrap();
        assert!(Config::load(dir.path()).is_err());
    }

    #[test]
    fn reserved_graph_name_fs_errors() {
        // Naming a graph `fs` would clobber the implicit base graph's @fs block.
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[graphs.fs]\nparser = \"markdown\"\n",
        )
        .unwrap();
        let err = Config::load(dir.path()).unwrap_err().to_string();
        assert!(err.contains("reserved"), "got: {err}");
    }

    #[test]
    fn invalid_graph_name_errors() {
        // Leading underscore is reserved.
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[graphs._internal]\nparser = \"markdown\"\n",
        )
        .unwrap();
        assert!(Config::load(dir.path()).is_err());
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
