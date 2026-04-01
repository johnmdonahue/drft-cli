use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Compile a list of glob patterns into a GlobSet. Returns None if patterns is empty.
pub fn compile_globs(patterns: &[String]) -> Result<Option<GlobSet>> {
    if patterns.is_empty() {
        return Ok(None);
    }
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern)?);
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

// ── Parser config ──────────────────────────────────────────────

/// Configuration for a single parser under `[parsers]`.
/// Supports shorthand (`markdown = true`) and expanded table form
/// (`[parsers.markdown]` with fields). Parser-specific options go
/// under `[parsers.<name>.options]` and are passed through to the parser.
#[derive(Debug, Clone, Serialize)]
pub struct ParserConfig {
    /// Which File nodes to send to this parser. None = all File nodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    /// Arbitrary options passed through to the parser (not interpreted by drft).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<toml::Value>,
}

/// Serde helper: untagged enum to parse shorthand or table forms.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawParserValue {
    /// `markdown = true`
    Bool(bool),
    /// `markdown = ["frontmatter", "wikilink"]` (v0.3 shorthand for types)
    Types(Vec<String>),
    /// `[parsers.markdown]` with fields
    Table {
        files: Option<Vec<String>>,
        command: Option<String>,
        timeout: Option<u64>,
        options: Option<toml::Value>,
        // v0.3 keys — accepted as migration aliases
        glob: Option<String>,
        types: Option<Vec<String>>,
    },
}

impl From<RawParserValue> for Option<ParserConfig> {
    fn from(val: RawParserValue) -> Self {
        match val {
            RawParserValue::Bool(false) => None,
            RawParserValue::Bool(true) => Some(ParserConfig {
                files: None,
                command: None,
                timeout: None,
                options: None,
            }),
            RawParserValue::Types(types) => {
                // v0.3 shorthand: `markdown = ["frontmatter"]` → options.types
                let options = toml::Value::Table(toml::map::Map::from_iter([(
                    "types".to_string(),
                    toml::Value::Array(types.into_iter().map(toml::Value::String).collect()),
                )]));
                Some(ParserConfig {
                    files: None,
                    command: None,
                    timeout: None,
                    options: Some(options),
                })
            }
            RawParserValue::Table {
                files,
                command,
                timeout,
                options,
                glob,
                types,
            } => {
                // Migrate v0.3 `glob` → `files`
                let files = if files.is_some() {
                    files
                } else if let Some(glob) = glob {
                    eprintln!("warn: parser 'glob' is deprecated — rename to 'files' (v0.4)");
                    Some(vec![glob])
                } else {
                    None
                };

                // Migrate v0.3 bare `types` → options.types
                let options = if let Some(types) = types {
                    eprintln!(
                        "warn: parser 'types' is deprecated — move to [parsers.<name>.options] (v0.4)"
                    );
                    let types_val =
                        toml::Value::Array(types.into_iter().map(toml::Value::String).collect());
                    match options {
                        Some(toml::Value::Table(mut tbl)) => {
                            tbl.entry("types").or_insert(types_val);
                            Some(toml::Value::Table(tbl))
                        }
                        None => {
                            let tbl = toml::map::Map::from_iter([("types".to_string(), types_val)]);
                            Some(toml::Value::Table(tbl))
                        }
                        other => other, // options exists but isn't a table — leave it
                    }
                } else {
                    options
                };

                Some(ParserConfig {
                    files,
                    command,
                    timeout,
                    options,
                })
            }
        }
    }
}

// ── Interface config ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceConfig {
    pub files: Vec<String>,
    #[serde(default)]
    pub ignore: Vec<String>,
}

// ── Rule config ────────────────────────────────────────────────

/// Configuration for a single rule under `[rules]`.
/// Supports shorthand (`cycle = "warn"`) and table form (`[rules.orphan]`).
#[derive(Debug, Clone)]
pub struct RuleConfig {
    pub severity: RuleSeverity,
    /// Scope which nodes the rule evaluates (default: all).
    pub files: Vec<String>,
    /// Exclude nodes from diagnostics (default: none).
    pub ignore: Vec<String>,
    pub command: Option<String>,
    /// Arbitrary structured data passed through to the rule. drft doesn't interpret it.
    pub options: Option<toml::Value>,
    pub(crate) files_compiled: Option<GlobSet>,
    pub(crate) ignore_compiled: Option<GlobSet>,
}

impl Serialize for RuleConfig {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        // Count how many fields to serialize (skip empty/default fields for cleaner output)
        let mut len = 1; // severity always present
        if !self.files.is_empty() {
            len += 1;
        }
        if !self.ignore.is_empty() {
            len += 1;
        }
        if self.command.is_some() {
            len += 1;
        }
        if self.options.is_some() {
            len += 1;
        }
        let mut map = serializer.serialize_map(Some(len))?;
        map.serialize_entry("severity", &self.severity)?;
        if !self.files.is_empty() {
            map.serialize_entry("files", &self.files)?;
        }
        if !self.ignore.is_empty() {
            map.serialize_entry("ignore", &self.ignore)?;
        }
        if let Some(ref command) = self.command {
            map.serialize_entry("command", command)?;
        }
        if let Some(ref options) = self.options {
            map.serialize_entry("options", options)?;
        }
        map.end()
    }
}

impl RuleConfig {
    pub fn new(
        severity: RuleSeverity,
        files: Vec<String>,
        ignore: Vec<String>,
        command: Option<String>,
        options: Option<toml::Value>,
    ) -> Result<Self> {
        let files_compiled = compile_globs(&files).context("failed to compile files globs")?;
        let ignore_compiled = compile_globs(&ignore).context("failed to compile ignore globs")?;
        Ok(Self {
            severity,
            files,
            ignore,
            command,
            options,
            files_compiled,
            ignore_compiled,
        })
    }

    pub fn is_path_in_scope(&self, path: &str) -> bool {
        match self.files_compiled {
            Some(ref glob_set) => glob_set.is_match(path),
            None => true, // no files = all in scope
        }
    }

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
        files: Vec<String>,
        #[serde(default)]
        ignore: Vec<String>,
        command: Option<String>,
        options: Option<toml::Value>,
    },
}

fn default_warn() -> RuleSeverity {
    RuleSeverity::Warn
}

// ── Config ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Config {
    /// Glob patterns declaring which filesystem paths become File nodes.
    /// Default: `["*.md"]`.
    pub include: Vec<String>,
    /// Glob patterns removed from the graph (applied after `include`).
    /// Also respects `.gitignore`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface: Option<InterfaceConfig>,
    pub parsers: HashMap<String, ParserConfig>,
    pub rules: HashMap<String, RuleConfig>,
    /// Directory containing the drft.toml this config was loaded from.
    #[serde(skip)]
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
    "encapsulation-violation",
    "fragility",
    "fragmentation",
    "layer-violation",
    "orphan-node",
    "redundant-edge",
    "schema-violation",
    "stale",
    "symlink-edge",
    "untrackable-target",
];

impl Config {
    pub fn defaults() -> Self {
        // When no drft.toml exists, default to markdown parser enabled
        let mut parsers = HashMap::new();
        parsers.insert(
            "markdown".to_string(),
            ParserConfig {
                files: None,
                command: None,
                timeout: None,
                options: None,
            },
        );

        let rules = [
            ("boundary-violation", RuleSeverity::Warn),
            ("dangling-edge", RuleSeverity::Warn),
            ("directed-cycle", RuleSeverity::Warn),
            ("encapsulation-violation", RuleSeverity::Warn),
            ("fragility", RuleSeverity::Warn),
            ("fragmentation", RuleSeverity::Warn),
            ("layer-violation", RuleSeverity::Warn),
            ("orphan-node", RuleSeverity::Warn),
            ("redundant-edge", RuleSeverity::Warn),
            ("stale", RuleSeverity::Warn),
            ("symlink-edge", RuleSeverity::Warn),
            ("untrackable-target", RuleSeverity::Warn),
        ]
        .into_iter()
        .map(|(k, v)| {
            (
                k.to_string(),
                RuleConfig::new(v, Vec::new(), Vec::new(), None, None)
                    .expect("default rule config"),
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
            None => anyhow::bail!("no drft.toml found (run `drft init` to create one)"),
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

        // Parse rules (unified: built-in severities + table form + custom rules)
        if let Some(raw_rules) = raw.rules {
            for (name, value) in raw_rules {
                let rule_config = match value {
                    RawRuleValue::Severity(severity) => {
                        RuleConfig::new(severity, Vec::new(), Vec::new(), None, None)?
                    }
                    RawRuleValue::Table {
                        severity,
                        files,
                        ignore,
                        command,
                        options,
                    } => RuleConfig::new(severity, files, ignore, command, options)
                        .with_context(|| format!("invalid globs in rules.{name}"))?,
                };

                // Warn about unknown built-in rules (but allow custom rules with command)
                if rule_config.command.is_none() && !BUILTIN_RULES.contains(&name.as_str()) {
                    eprintln!("warn: unknown rule \"{name}\" in drft.toml (ignored)");
                }

                config.rules.insert(name, rule_config);
            }
        }

        Ok(config)
    }

    /// Find drft.toml in `root`. No directory walking — if it's not here, use defaults.
    fn find_config(root: &Path) -> Option<std::path::PathBuf> {
        let candidate = root.join("drft.toml");
        candidate.exists().then_some(candidate)
    }

    pub fn rule_severity(&self, name: &str) -> RuleSeverity {
        self.rules
            .get(name)
            .map(|r| r.severity)
            .unwrap_or(RuleSeverity::Off)
    }

    /// Check if a path is in scope for a specific rule (passes `files` filter).
    pub fn is_rule_in_scope(&self, rule: &str, path: &str) -> bool {
        self.rules
            .get(rule)
            .is_none_or(|r| r.is_path_in_scope(path))
    }

    /// Check if a path should be ignored for a specific rule.
    pub fn is_rule_ignored(&self, rule: &str, path: &str) -> bool {
        self.rules
            .get(rule)
            .is_some_and(|r| r.is_path_ignored(path))
    }

    /// Get a rule's options (the `[rules.<name>.options]` section).
    pub fn rule_options(&self, name: &str) -> Option<&toml::Value> {
        self.rules.get(name).and_then(|r| r.options.as_ref())
    }

    /// Get custom rules (rules with a command field).
    pub fn custom_rules(&self) -> impl Iterator<Item = (&str, &RuleConfig)> {
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
    fn errors_when_no_config() {
        let dir = TempDir::new().unwrap();
        let result = Config::load(dir.path());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no drft.toml found"),
        );
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
    fn loads_rule_with_options() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            r#"
[rules.schema-violation]
severity = "warn"

[rules.schema-violation.options]
required = ["title"]

[rules.schema-violation.options.schemas."observations/*.md"]
required = ["title", "date", "status"]
"#,
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        let opts = config.rule_options("schema-violation").unwrap();
        let required = opts.get("required").unwrap().as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0].as_str().unwrap(), "title");
        let schemas = opts.get("schemas").unwrap().as_table().unwrap();
        assert!(schemas.contains_key("observations/*.md"));
    }

    #[test]
    fn shorthand_rule_has_no_options() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[rules]\ndangling-edge = \"error\"\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert!(config.rule_options("dangling-edge").is_none());
    }

    #[test]
    fn loads_parser_shorthand_bool() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("drft.toml"), "[parsers]\nmarkdown = true\n").unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert!(config.parsers.contains_key("markdown"));
        let p = &config.parsers["markdown"];
        assert!(p.files.is_none());
        assert!(p.options.is_none());
        assert!(p.command.is_none());
    }

    #[test]
    fn loads_parser_shorthand_types_migrates_to_options() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[parsers]\nmarkdown = [\"frontmatter\", \"wikilink\"]\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        let p = &config.parsers["markdown"];
        // v0.3 shorthand types → options.types
        let opts = p.options.as_ref().unwrap();
        let types = opts.get("types").unwrap().as_array().unwrap();
        assert_eq!(types.len(), 2);
        assert_eq!(types[0].as_str().unwrap(), "frontmatter");
        assert_eq!(types[1].as_str().unwrap(), "wikilink");
    }

    #[test]
    fn loads_parser_table_with_files() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[parsers.tsx]\nfiles = [\"*.tsx\", \"*.ts\"]\ncommand = \"./parse.sh\"\ntimeout = 10000\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        let p = &config.parsers["tsx"];
        assert_eq!(
            p.files.as_deref(),
            Some(&["*.tsx".to_string(), "*.ts".to_string()][..])
        );
        assert_eq!(p.command.as_deref(), Some("./parse.sh"));
        assert_eq!(p.timeout, Some(10000));
    }

    #[test]
    fn loads_parser_glob_migrates_to_files() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[parsers.tsx]\nglob = \"*.tsx\"\ncommand = \"./parse.sh\"\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        let p = &config.parsers["tsx"];
        assert_eq!(p.files.as_deref(), Some(&["*.tsx".to_string()][..]));
    }

    #[test]
    fn loads_parser_options() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[parsers.markdown]\nfiles = [\"*.md\"]\n\n[parsers.markdown.options]\ntypes = [\"inline\"]\nextract_metadata = true\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        let p = &config.parsers["markdown"];
        let opts = p.options.as_ref().unwrap();
        assert!(opts.get("types").is_some());
        assert_eq!(opts.get("extract_metadata").unwrap().as_bool(), Some(true));
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
            "[interface]\nfiles = [\"overview.md\", \"api/*.md\"]\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        let iface = config.interface.unwrap();
        assert_eq!(iface.files, vec!["overview.md", "api/*.md"]);
    }

    #[test]
    fn loads_custom_rule() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[rules.my-check]\ncommand = \"./check.sh\"\nseverity = \"warn\"\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        let custom_rules: Vec<_> = config.custom_rules().collect();
        assert_eq!(custom_rules.len(), 1);
        assert_eq!(custom_rules[0].0, "my-check");
        assert_eq!(custom_rules[0].1.command.as_deref(), Some("./check.sh"));
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
    fn child_without_config_errors() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("drft.toml"),
            "[rules]\norphan-node = \"error\"\n",
        )
        .unwrap();

        let child = dir.path().join("child");
        fs::create_dir(&child).unwrap();

        let result = Config::load(&child);
        assert!(result.is_err());
    }
}
