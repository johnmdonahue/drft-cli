pub mod custom;
pub mod frontmatter;
pub mod markdown;

use crate::config::ParserConfig;
use std::collections::HashMap;

/// Combined output from parsing a single file: links + optional metadata.
/// Links are raw strings as they appear in the source — the graph builder handles
/// normalization (fragment stripping, anchor filtering, URI detection).
///
/// See [`docs/parsers`](../../docs/parsers/README.md) for details.
#[derive(Debug, Clone, Default)]
pub struct ParseResult {
    pub links: Vec<String>,
    /// Structured metadata extracted from the file, namespaced by parser on the node.
    pub metadata: Option<serde_json::Value>,
}

/// Trait implemented by all parsers (built-in and custom).
pub trait Parser {
    /// Parser name — used as provenance on edges.
    fn name(&self) -> &str;
    /// Check if this parser should run on a given file path.
    fn matches(&self, path: &str) -> bool;
    /// Parse a file's content and return discovered links + optional metadata.
    fn parse(&self, path: &str, content: &str) -> ParseResult;
    /// Parse multiple files in one call. Default falls back to per-file parsing.
    /// Custom parsers override this to spawn one process for all files.
    fn parse_batch(&self, files: &[(&str, &str)]) -> HashMap<String, ParseResult> {
        files
            .iter()
            .map(|(path, content)| (path.to_string(), self.parse(path, content)))
            .collect()
    }
}

/// Build a GlobSet from file patterns (for parser routing).
/// Returns None if no patterns → parser receives all File nodes.
fn build_file_filter(patterns: &Option<Vec<String>>, name: &str) -> Option<globset::GlobSet> {
    let patterns = patterns.as_ref()?;
    match crate::config::compile_globs(patterns) {
        Ok(set) => set,
        Err(e) => {
            eprintln!("warn: invalid glob in parser {name}.files: {e}");
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

        if let Some(ref command) = config.command {
            // Custom parser
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

            parsers.push(Box::new(custom::CustomParser {
                parser_name: name.clone(),
                file_filter,
                command: resolved_command,
                timeout_ms: config.timeout.unwrap_or(5000),
                scope_dir: root.to_path_buf(),
                options: config.options.clone(),
            }));
        } else {
            // Built-in parser
            match name.as_str() {
                "markdown" => {
                    parsers.push(Box::new(markdown::MarkdownParser { file_filter }));
                }
                "frontmatter" => {
                    parsers.push(Box::new(frontmatter::FrontmatterParser { file_filter }));
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
