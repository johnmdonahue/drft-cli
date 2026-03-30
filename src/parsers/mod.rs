pub mod markdown;
pub mod script;

use crate::config::ParserConfig;
use std::collections::HashMap;

/// A raw link emitted by a parser — just target path and bare link type.
/// The full EdgeType (`parser:type`) is constructed by the graph builder.
///
/// See `docs/parsers/README.md` for details.
#[derive(Debug, Clone)]
pub struct RawLink {
    pub target: String,
    /// Link type within this parser's vocabulary (e.g., "inline", "frontmatter").
    pub link_type: String,
    /// Whether this is an external URL (http/https).
    pub is_external: bool,
}

/// Trait implemented by all parsers (built-in and script-based).
pub trait Parser {
    /// Parser name (used as the namespace in EdgeType, e.g., "markdown").
    fn name(&self) -> &str;
    /// Check if this parser should run on a given file path.
    fn matches(&self, path: &str) -> bool;
    /// Parse a file's content and return discovered links.
    fn parse(&self, path: &str, content: &str) -> Vec<RawLink>;
}

/// Default glob patterns for built-in parsers.
fn default_glob(parser_name: &str) -> Option<&'static str> {
    match parser_name {
        "markdown" => Some("*.md"),
        _ => None,
    }
}

/// Build the parser registry from config.
/// Returns a list of boxed parsers ready to run.
pub fn build_parsers(
    parsers_config: &HashMap<String, ParserConfig>,
    config_dir: Option<&std::path::Path>,
) -> Vec<Box<dyn Parser>> {
    let mut parsers: Vec<Box<dyn Parser>> = Vec::new();

    for (name, config) in parsers_config {
        let glob_pattern = config
            .glob
            .as_deref()
            .or_else(|| default_glob(name))
            .unwrap_or("*");

        let glob = match globset::Glob::new(glob_pattern) {
            Ok(g) => g.compile_matcher(),
            Err(e) => {
                eprintln!("warn: invalid glob for parser {name}: {e}");
                continue;
            }
        };

        let type_filter = config.types.clone();

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
                glob,
                type_filter,
                command: resolved_command,
                timeout_ms: config.timeout.unwrap_or(5000),
            }));
        } else {
            // Built-in parser
            match name.as_str() {
                "markdown" => {
                    parsers.push(Box::new(markdown::MarkdownParser { glob, type_filter }));
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
