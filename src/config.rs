use crate::hints::Hint;
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
    /// `frontmatter` only: the keys whose values yield edges. Empty means the
    /// graph emits none — a supported shape, since a frontmatter graph may exist
    /// purely to seed node metadata.
    pub edge_keys: Vec<String>,
}

/// `deny_unknown_fields` so a key the parser does not support is a hard error
/// rather than a silent discard. A graph table that parses is read as a graph
/// that works — a speculative `edge_keys = [...]` must not exit 0 doing nothing.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGraph {
    files: Option<Vec<String>>,
    parser: String,
    edge_keys: Option<Vec<String>>,
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
///
/// The table variant cannot use `deny_unknown_fields` — an untagged enum reports
/// a rejected variant as "data did not match any variant", which names neither
/// the bad key nor the known set. Capturing the leftovers instead lets `load`
/// raise the same precise error the graph tables give.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawRuleValue {
    Severity(RuleSeverity),
    Table {
        #[serde(default = "default_warn")]
        severity: RuleSeverity,
        #[serde(default)]
        ignore: Vec<String>,
        #[serde(flatten)]
        unknown: BTreeMap<String, toml::Value>,
    },
}

/// Fields a `[rules.*]` table accepts, for the unknown-key error.
const RULE_TABLE_FIELDS: &str = "`severity` or `ignore`";

fn default_warn() -> RuleSeverity {
    RuleSeverity::Warn
}

/// Serde helper for the `[rules]` table: a global `ignore` applied to every rule,
/// plus the per-rule entries (`stale-node = "error"`, `[rules.detached-node]`, …)
/// captured by flatten. `ignore` is therefore a reserved key under `[rules]`.
#[derive(Debug, Deserialize, Default)]
struct RawRules {
    #[serde(default)]
    ignore: Vec<String>,
    #[serde(flatten)]
    rules: HashMap<String, RawRuleValue>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUsage {
    #[serde(default)]
    enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExperimental {
    usage: Option<RawUsage>,
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
    /// Whether experimental local usage collection was explicitly enabled.
    pub usage_enabled: bool,
    /// BLAKE3 fingerprint of the exact config bytes, present only when usage is enabled.
    pub usage_config_fingerprint: Option<crate::usage::event::ConfigFingerprint>,
    /// Globs from `[rules].ignore` — subjects suppressed across *every* rule
    /// (configured or not), unioned with each rule's own `ignore`. Unlike the
    /// top-level `ignore`, the paths stay in the graph; only findings are dropped.
    rule_ignore: Option<GlobSet>,
    /// Graph root containing the `.drft/config.toml` this config was loaded from.
    pub config_dir: Option<std::path::PathBuf>,
    /// Advisories raised while loading — a misspelled rule name, say. Carried on
    /// the config rather than printed at the point of discovery so the caller
    /// decides where they land: stderr in text, the result document in JSON.
    pub hints: Vec<Hint>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct RawConfig {
    ignore: Option<Vec<String>>,
    graphs: Option<HashMap<String, RawGraph>>,
    rules: Option<RawRules>,
    experimental: Option<RawExperimental>,
}

/// Names of all built-in rules (for the `unknown-rule` hint).
const BUILTIN_RULES: &[&str] = &[
    "stale-node",
    "stale-edge",
    "new-edge",
    "removed-edge",
    "removed-node",
    "unresolved-edge",
    "unresolved-fragment",
    "detached-node",
    "unlocked-node",
    "no-baseline",
    "unreadable-frontmatter",
    "unreadable-text",
];

/// Parsers a graph may declare. `fs` is the implicit base graph (a provider, not
/// a parser) and is intentionally absent — `parser = "fs"` is rejected.
const KNOWN_PARSERS: &[&str] = &["markdown", "frontmatter"];

/// Parsers that publish the `#fragment` addresses a file answers to. Metadata
/// from any other graph — frontmatter especially, which is the author's own
/// YAML — is not a claim about what a file can be cited by.
const PARSERS_WITH_ANCHORS: &[&str] = &["markdown"];

/// Parsers that accept `edge_keys`. Markdown has no keyed structure to scope.
const PARSERS_WITH_EDGE_KEYS: &[&str] = &["frontmatter"];

/// Graph names reserved for drft's implicit graphs. Declaring one would collide
/// with the core `@fs` namespace at compose and overwrite its `type`/`hash`.
const RESERVED_GRAPH_NAMES: &[&str] = &["fs"];

/// A graph's `files` scope defaults to markdown when omitted.
const DEFAULT_FILES: &str = "**/*.md";

impl Config {
    /// The base config: no graphs (the project config declares the full set), no
    /// ignores, every rule at `warn`. `fs` is always built regardless.
    pub fn defaults() -> Self {
        Config {
            ignore: Vec::new(),
            graphs: BTreeMap::new(),
            rules: HashMap::new(),
            usage_enabled: false,
            usage_config_fingerprint: None,
            rule_ignore: None,
            config_dir: None,
            hints: Vec::new(),
        }
    }

    /// The `@<graph>` namespaces whose parser publishes anchors — the ones a
    /// fragment may be checked against.
    pub fn anchor_namespaces(&self) -> Vec<String> {
        self.graphs
            .iter()
            .filter(|(_, graph)| PARSERS_WITH_ANCHORS.contains(&graph.parser.as_str()))
            .map(|(name, _)| crate::model::namespace(name))
            .collect()
    }

    pub fn load(root: &Path) -> Result<Self> {
        crate::layout::reject_legacy(root)?;
        let config_path = crate::layout::config_path(root);
        if !crate::layout::current_config_exists(root)? {
            anyhow::bail!("no .drft/config.toml found (run `drft init` to create one)");
        }

        let content = std::fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        let raw: RawConfig = toml::from_str(&content)
            .with_context(|| format!("failed to parse {}", config_path.display()))?;
        let usage_enabled = raw
            .experimental
            .as_ref()
            .and_then(|experimental| experimental.usage.as_ref())
            .is_some_and(|usage| usage.enabled);

        let mut config = Self::defaults();
        config.config_dir = Some(root.to_path_buf());

        if let Some(ignore) = raw.ignore {
            config.ignore = ignore;
        }

        // The project config declares the full graph set — there are no defaults.
        if let Some(raw_graphs) = raw.graphs {
            for (name, raw) in raw_graphs {
                crate::model::validate_label(&name)
                    .map_err(|e| anyhow::anyhow!("invalid graph name in .drft/config.toml: {e}"))?;
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
                // `edge_keys` scopes a keyed structure; only the frontmatter
                // parser has one. Accepting it elsewhere would reintroduce exactly
                // the silent no-op that made it unfindable in the first place (#71).
                if raw.edge_keys.is_some() && !PARSERS_WITH_EDGE_KEYS.contains(&raw.parser.as_str())
                {
                    anyhow::bail!(
                        "`edge_keys` is not supported by the \"{}\" parser in graph \"{name}\" (supported: {})",
                        raw.parser,
                        PARSERS_WITH_EDGE_KEYS.join(", ")
                    );
                }
                config.graphs.insert(
                    name,
                    GraphConfig {
                        files: raw.files.unwrap_or_else(|| vec![DEFAULT_FILES.to_string()]),
                        parser: raw.parser,
                        edge_keys: raw.edge_keys.unwrap_or_default(),
                    },
                );
            }
        }

        if let Some(raw_rules) = raw.rules {
            // The global rule-ignore applies to every rule, including ones with
            // no explicit entry below.
            config.rule_ignore = compile_globs(&raw_rules.ignore)
                .context("failed to compile [rules].ignore globs")?;
            for (name, value) in raw_rules.rules {
                let rule_config = match value {
                    RawRuleValue::Severity(severity) => RuleConfig::new(severity, Vec::new())?,
                    RawRuleValue::Table {
                        severity,
                        ignore,
                        unknown,
                    } => {
                        // Raised after parsing, so it carries the config path the
                        // serde-level errors get from the `failed to parse` context.
                        if let Some(key) = unknown.keys().next() {
                            anyhow::bail!(
                                "failed to parse {}: unknown field `{key}` in rules.{name}, expected {RULE_TABLE_FIELDS}",
                                config_path.display()
                            );
                        }
                        RuleConfig::new(severity, ignore)
                            .with_context(|| format!("invalid globs in rules.{name}"))?
                    }
                };
                if !BUILTIN_RULES.contains(&name.as_str()) {
                    config.hints.push(
                        Hint::new(
                            "unknown-rule",
                            "not a built-in rule, so this severity configures nothing",
                        )
                        .at(format!("rules.{name}"))
                        .with_next(format!(
                            "correct the spelling or remove it — built-in rules are {}",
                            BUILTIN_RULES.join(", ")
                        )),
                    );
                }
                config.rules.insert(name, rule_config);
            }
        }

        if usage_enabled {
            config.usage_enabled = true;
            config.usage_config_fingerprint = Some(
                crate::usage::event::ConfigFingerprint::from_parsed_bytes(content.as_bytes()),
            );
        }

        Ok(config)
    }

    /// Glob patterns the `fs` walk removes from the graph.
    pub fn ignore_patterns(&self) -> &[String] {
        &self.ignore
    }

    /// Whether `path` is ignored for `rule`.
    pub fn is_rule_ignored(&self, rule: &str, path: &str) -> bool {
        self.rule_ignore
            .as_ref()
            .is_some_and(|set| set.is_match(path))
            || self
                .rules
                .get(rule)
                .is_some_and(|r| r.is_path_ignored(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn config_path(root: &Path) -> std::path::PathBuf {
        std::fs::create_dir_all(crate::layout::state_dir(root)).unwrap();
        crate::layout::config_path(root)
    }

    #[test]
    fn errors_when_no_config() {
        let dir = TempDir::new().unwrap();
        let result = Config::load(dir.path());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no .drft/config.toml found")
        );
    }

    #[test]
    fn defaults_have_no_graphs() {
        // No runtime defaults — the project config declares the full set.
        assert!(Config::defaults().graphs.is_empty());
        assert!(!Config::defaults().usage_enabled);
        assert!(Config::defaults().usage_config_fingerprint.is_none());
    }

    #[test]
    fn usage_is_disabled_when_setting_is_absent_or_false() {
        for contents in [
            "",
            "[experimental]\n",
            "[experimental.usage]\n",
            "[experimental.usage]\nenabled = false\n",
        ] {
            let dir = TempDir::new().unwrap();
            fs::write(config_path(dir.path()), contents).unwrap();
            let config = Config::load(dir.path()).unwrap();
            assert!(!config.usage_enabled, "contents: {contents:?}");
            assert!(config.usage_config_fingerprint.is_none());
        }
    }

    #[test]
    fn usage_enabled_fingerprints_exact_config_bytes() {
        // Comments, CRLFs, and trailing whitespace are part of the exact parsed
        // input and therefore part of the fingerprint.
        let contents = b"# opt in\r\n[experimental.usage]\r\nenabled = true\r\n \r\n";
        let dir = TempDir::new().unwrap();
        let path = config_path(dir.path());
        fs::write(&path, contents).unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert!(config.usage_enabled);
        let fingerprint =
            serde_json::to_string(config.usage_config_fingerprint.as_ref().unwrap()).unwrap();
        assert_eq!(
            fingerprint,
            format!("\"b3:{}\"", blake3::hash(contents).to_hex())
        );

        fs::write(&path, "[experimental.usage]\nenabled = false\n").unwrap();
        assert_eq!(
            fingerprint,
            serde_json::to_string(config.usage_config_fingerprint.as_ref().unwrap()).unwrap()
        );
    }

    #[test]
    fn usage_table_rejects_unknown_keys_and_types_even_when_disabled() {
        for contents in [
            "experimental = true\n",
            "experimental = []\n",
            "experimental = { usage = false }\n",
            "[experimental.usage]\nenabled = 1\n",
            "[experimental.usage]\nenabled = []\n",
            "[experimental]\nunknown = true\n",
            "[experimental.usage]\nunknown = true\n",
            "[experimental.usage]\nenabled = \"false\"\n",
            "[experimental.usage]\nenabled = false\nunknown = true\n",
            "[experimental.other]\nenabled = false\n",
            "[experimental.usage.extra]\nenabled = false\n",
        ] {
            let dir = TempDir::new().unwrap();
            fs::write(config_path(dir.path()), contents).unwrap();
            assert!(Config::load(dir.path()).is_err(), "contents: {contents:?}");
        }
    }

    #[test]
    fn invalid_config_cannot_return_enabled_config() {
        let dir = TempDir::new().unwrap();
        fs::write(
            config_path(dir.path()),
            "[experimental.usage]\nenabled = true\n\n[graphs.bad]\nparser = \"unknown\"\n",
        )
        .unwrap();
        assert!(Config::load(dir.path()).is_err());
    }

    #[test]
    fn loads_ignore() {
        let dir = TempDir::new().unwrap();
        fs::write(config_path(dir.path()), "ignore = [\"target/**\"]\n").unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.ignore, vec!["target/**"]);
    }

    #[test]
    fn declares_graphs() {
        let dir = TempDir::new().unwrap();
        fs::write(
            config_path(dir.path()),
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
            config_path(dir.path()),
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
            config_path(dir.path()),
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
        fs::write(config_path(dir.path()), "[graphs.x]\nparser = \"fs\"\n").unwrap();
        assert!(Config::load(dir.path()).is_err());
    }

    #[test]
    fn reserved_graph_name_fs_errors() {
        // Naming a graph `fs` would clobber the implicit base graph's @fs block.
        let dir = TempDir::new().unwrap();
        fs::write(
            config_path(dir.path()),
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
            config_path(dir.path()),
            "[graphs._internal]\nparser = \"markdown\"\n",
        )
        .unwrap();
        assert!(Config::load(dir.path()).is_err());
    }

    #[test]
    fn loads_rule_severity_and_ignore() {
        let dir = TempDir::new().unwrap();
        fs::write(
            config_path(dir.path()),
            "[rules]\nstale-node = \"error\"\n\n[rules.detached-node]\nignore = [\"README.md\"]\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.rules["stale-node"].severity, RuleSeverity::Error);
        assert!(config.is_rule_ignored("detached-node", "README.md"));
        assert!(!config.is_rule_ignored("detached-node", "other.md"));
    }

    #[test]
    fn global_rule_ignore_applies_to_every_rule() {
        let dir = TempDir::new().unwrap();
        fs::write(
            config_path(dir.path()),
            "[rules]\nignore = [\"vendor/**\"]\n\n[rules.stale-node]\nseverity = \"error\"\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        // The flattened per-rule entry still parses alongside the global ignore.
        assert_eq!(config.rules["stale-node"].severity, RuleSeverity::Error);
        // Global ignore hits a configured rule and an unconfigured one alike.
        assert!(config.is_rule_ignored("stale-node", "vendor/x.md"));
        assert!(config.is_rule_ignored("unresolved-edge", "vendor/x.md"));
        // It does not touch paths outside the group.
        assert!(!config.is_rule_ignored("stale-node", "yours.md"));
    }

    #[test]
    fn unknown_graph_key_errors() {
        // A key the parser does not support must not parse and do nothing — the
        // near-miss spellings (`fields`, `include_keys`) are the likely case, so
        // the error names the key and the accepted set.
        let dir = TempDir::new().unwrap();
        fs::write(
            config_path(dir.path()),
            "[graphs.x]\nparser = \"frontmatter\"\ninclude_keys = [\"sources\"]\n",
        )
        .unwrap();
        let err = format!("{:#}", Config::load(dir.path()).unwrap_err());
        assert!(err.contains("unknown field `include_keys`"), "got: {err}");
        assert!(err.contains("files"), "expected set not named: {err}");
    }

    #[test]
    fn frontmatter_graph_accepts_edge_keys() {
        let dir = TempDir::new().unwrap();
        fs::write(
            config_path(dir.path()),
            "[graphs.fm]\nparser = \"frontmatter\"\nedge_keys = [\"sources\"]\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.graphs["fm"].edge_keys, vec!["sources".to_string()]);
    }

    #[test]
    fn declaring_edge_keys_raises_no_hint() {
        let dir = TempDir::new().unwrap();
        fs::write(
            config_path(dir.path()),
            "[graphs.fm]\nparser = \"frontmatter\"\nedge_keys = [\"sources\"]\n",
        )
        .unwrap();
        assert!(Config::load(dir.path()).unwrap().hints.is_empty());
    }

    #[test]
    fn omitting_edge_keys_is_a_metadata_only_graph() {
        // A frontmatter graph may exist purely to seed node metadata, so this
        // loads and tracks no edges. Nothing is reported: the graph is as
        // configured, and the config layer has nothing to say about it.
        let dir = TempDir::new().unwrap();
        fs::write(
            config_path(dir.path()),
            "[graphs.fm]\nparser = \"frontmatter\"\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert!(config.graphs["fm"].edge_keys.is_empty());
        assert!(config.hints.is_empty(), "got: {:?}", config.hints);
    }

    #[test]
    fn edge_keys_on_markdown_parser_errors() {
        // Markdown has no keyed structure — accepting `edge_keys` there would be
        // the silent no-op this option exists to remove.
        let dir = TempDir::new().unwrap();
        fs::write(
            config_path(dir.path()),
            "[graphs.md]\nparser = \"markdown\"\nedge_keys = [\"sources\"]\n",
        )
        .unwrap();
        let err = format!("{:#}", Config::load(dir.path()).unwrap_err());
        assert!(
            err.contains("not supported by the \"markdown\" parser"),
            "got: {err}"
        );
    }

    #[test]
    fn empty_edge_keys_is_the_same_state_as_omitting_it() {
        // An empty set names nowhere to look, which is what omitting the field
        // already means. Treating one as a mistake and the other as a shape would
        // make the config say two things with one meaning.
        let dir = TempDir::new().unwrap();
        fs::write(
            config_path(dir.path()),
            "[graphs.fm]\nparser = \"frontmatter\"\nedge_keys = []\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert!(config.graphs["fm"].edge_keys.is_empty());
        assert!(config.hints.is_empty(), "got: {:?}", config.hints);
    }

    #[test]
    fn unknown_top_level_key_errors() {
        let dir = TempDir::new().unwrap();
        fs::write(config_path(dir.path()), "ignores = [\"target/**\"]\n").unwrap();
        let err = format!("{:#}", Config::load(dir.path()).unwrap_err());
        assert!(err.contains("unknown field `ignores`"), "got: {err}");
    }

    #[test]
    fn unknown_rule_table_key_errors() {
        // The untagged enum accepts any table (both fields default), so a typo'd
        // key silently parsed as an all-defaults rule before the leftovers were
        // captured. Distinct from an unknown *rule name*, which only warns.
        let dir = TempDir::new().unwrap();
        fs::write(
            config_path(dir.path()),
            "[rules.stale-node]\nseverty = \"error\"\n",
        )
        .unwrap();
        let err = format!("{:#}", Config::load(dir.path()).unwrap_err());
        assert!(err.contains("unknown field `severty`"), "got: {err}");
        assert!(err.contains("rules.stale-node"), "got: {err}");
    }

    #[test]
    fn known_rule_table_keys_still_parse() {
        // Guard against the flatten capture swallowing the real fields.
        let dir = TempDir::new().unwrap();
        fs::write(
            config_path(dir.path()),
            "[rules.detached-node]\nseverity = \"error\"\nignore = [\"README.md\"]\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        assert_eq!(config.rules["detached-node"].severity, RuleSeverity::Error);
        assert!(config.is_rule_ignored("detached-node", "README.md"));
    }

    #[test]
    fn invalid_toml_errors() {
        let dir = TempDir::new().unwrap();
        fs::write(config_path(dir.path()), "not valid toml {{{{").unwrap();
        assert!(Config::load(dir.path()).is_err());
    }

    #[test]
    fn unknown_rule_name_becomes_a_hint_on_the_config() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            config_path(dir.path()),
            "[rules]\nstale-nodes = \"error\"\n",
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap();
        // The rule still lands in the map — it configures nothing, which is
        // exactly why the silence needs a hint rather than an error.
        assert!(config.rules.contains_key("stale-nodes"));
        let hint = config.hints.first().expect("expected a hint");
        assert_eq!(hint.name, "unknown-rule");
        assert_eq!(hint.locus.as_deref(), Some("rules.stale-nodes"));
    }

    #[test]
    fn a_valid_config_raises_no_hints() {
        let dir = TempDir::new().unwrap();
        std::fs::write(config_path(dir.path()), "[rules]\nstale-node = \"error\"\n").unwrap();
        assert!(Config::load(dir.path()).unwrap().hints.is_empty());
    }
}
