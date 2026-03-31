pub mod markdown;
pub mod script;

use crate::config::ParserConfig;
use std::collections::HashMap;

/// A raw link emitted by a parser — just target path and bare link type.
/// The full EdgeType (`parser:type`) is constructed by the graph builder.
///
/// See [`docs/parsers`](../../docs/parsers/README.md) for details.
#[derive(Debug, Clone)]
pub struct RawLink {
    pub target: String,
    /// Link type within this parser's vocabulary (e.g., "inline", "frontmatter").
    pub link_type: String,
    /// Whether this is an external URL (http/https).
    pub is_external: bool,
}

/// Combined output from parsing a single file: edges + optional metadata.
#[derive(Debug, Clone, Default)]
pub struct ParseResult {
    pub links: Vec<RawLink>,
    /// Structured metadata extracted from the file, namespaced by parser on the node.
    pub metadata: Option<serde_json::Value>,
}

/// Trait implemented by all parsers (built-in and script-based).
pub trait Parser {
    /// Parser name (used as the namespace in EdgeType, e.g., "markdown").
    fn name(&self) -> &str;
    /// Check if this parser should run on a given file path.
    fn matches(&self, path: &str) -> bool;
    /// Parse a file's content and return discovered links + optional metadata.
    fn parse(&self, path: &str, content: &str) -> ParseResult;
    /// Parse multiple files in one call. Default falls back to per-file parsing.
    /// Script parsers override this to spawn one process for all files.
    fn parse_batch(&self, files: &[(&str, &str)]) -> HashMap<String, ParseResult> {
        files
            .iter()
            .map(|(path, content)| (path.to_string(), self.parse(path, content)))
            .collect()
    }
}

/// Extract the `types` array from parser options, if present.
fn extract_type_filter(options: &Option<toml::Value>) -> Option<Vec<String>> {
    options
        .as_ref()
        .and_then(|v| v.get("types"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
}

/// Build a GlobSet from file patterns (for parser routing).
/// Returns None if no patterns → parser receives all File nodes.
fn build_file_filter(patterns: &Option<Vec<String>>, name: &str) -> Option<globset::GlobSet> {
    let patterns = patterns.as_ref()?;
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        match globset::Glob::new(pattern) {
            Ok(g) => {
                builder.add(g);
            }
            Err(e) => {
                eprintln!("warn: invalid glob in parser {name}.files: {e}");
            }
        }
    }
    match builder.build() {
        Ok(set) => Some(set),
        Err(e) => {
            eprintln!("warn: failed to compile globs for parser {name}.files: {e}");
            None
        }
    }
}

/// Build the parser registry from config.
/// Returns a list of boxed parsers ready to run.
pub fn build_parsers(
    parsers_config: &HashMap<String, ParserConfig>,
    config_dir: Option<&std::path::Path>,
    root: &std::path::Path,
) -> Vec<Box<dyn Parser>> {
    let mut parsers: Vec<Box<dyn Parser>> = Vec::new();

    for (name, config) in parsers_config {
        let file_filter = build_file_filter(&config.files, name);
        let type_filter = extract_type_filter(&config.options);

        if let Some(ref command) = config.command {
            // Script-based parser
            let resolved_command = if let Some(dir) = config_dir {
                let cmd_path = dir.join(command);
                if cmd_path.exists() {
                    cmd_path.to_string_lossy().to_string()
                } else {
                    command.clone()
                }
            } else {
                command.clone()
            };

            parsers.push(Box::new(script::ScriptParser {
                parser_name: name.clone(),
                file_filter,
                type_filter,
                command: resolved_command,
                timeout_ms: config.timeout.unwrap_or(5000),
                scope_dir: root.to_path_buf(),
                options: config.options.clone(),
            }));
        } else {
            // Built-in parser
            match name.as_str() {
                "markdown" => {
                    let extract_metadata = config
                        .options
                        .as_ref()
                        .and_then(|v| v.get("extract_metadata"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    parsers.push(Box::new(markdown::MarkdownParser {
                        file_filter,
                        type_filter,
                        extract_metadata,
                    }));
                }
                _ => {
                    eprintln!(
                        "warn: unknown built-in parser \"{name}\" (use 'command' field for custom parsers)"
                    );
                }
            }
        }
    }

    parsers
}
