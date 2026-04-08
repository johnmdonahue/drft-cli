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

/// Check whether a frontmatter value looks like a link target (file path or URI).
pub(crate) fn is_link_candidate(value: &str) -> bool {
    // URIs are always candidates — graph builder creates External nodes
    if crate::graph::is_uri(value) {
        return true;
    }
    // Explicit path prefixes are always candidates
    if value.starts_with("./") || value.starts_with("../") || value.starts_with('/') {
        return true;
    }
    // Prose contains spaces — file paths don't
    if value.contains(' ') {
        return false;
    }
    // Must have a plausible file extension: dot followed by 1-4 alphanumeric
    // chars that aren't all digits (rejects v2.0, e.g., Dr.)
    let basename = value.rsplit('/').next().unwrap_or(value);
    if let Some(dot_pos) = basename.rfind('.') {
        let ext = &basename[dot_pos + 1..];
        !ext.is_empty()
            && ext.len() <= 4
            && ext.chars().all(|c| c.is_ascii_alphanumeric())
            && !ext.chars().all(|c| c.is_ascii_digit())
    } else {
        false
    }
}

/// Strip all code content (fenced blocks and inline backtick spans),
/// replacing with spaces to preserve offsets.
pub(crate) fn strip_code(content: &str) -> String {
    // First strip fenced code blocks (``` and ~~~)
    let mut result = String::with_capacity(content.len());
    let mut in_code_block = false;
    let mut fence_marker = "";

    for line in content.lines() {
        let trimmed = line.trim_start();
        if !in_code_block {
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                in_code_block = true;
                fence_marker = if trimmed.starts_with("```") {
                    "```"
                } else {
                    "~~~"
                };
                result.push_str(&" ".repeat(line.len()));
            } else {
                result.push_str(line);
            }
        } else if trimmed.starts_with(fence_marker) && trimmed.trim() == fence_marker {
            in_code_block = false;
            result.push_str(&" ".repeat(line.len()));
        } else {
            result.push_str(&" ".repeat(line.len()));
        }
        result.push('\n');
    }

    // Then strip inline code spans (single and double backticks)
    let mut cleaned = String::with_capacity(result.len());
    let chars: Vec<char> = result.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '`' {
            // Count opening backticks
            let mut ticks = 0;
            while i + ticks < chars.len() && chars[i + ticks] == '`' {
                ticks += 1;
            }
            // Find matching closing backticks in the char array
            let after = i + ticks;
            let mut found = None;
            let mut j = after;
            while j + ticks <= chars.len() {
                if chars[j..j + ticks].iter().all(|c| *c == '`') {
                    found = Some(j);
                    break;
                }
                j += 1;
            }
            if let Some(close_start) = found {
                // Replace entire span (backticks + content + backticks) with spaces
                let total = close_start + ticks - i;
                for _ in 0..total {
                    cleaned.push(' ');
                }
                i += total;
            } else {
                // No closing — keep the backtick as-is
                cleaned.push(chars[i]);
                i += 1;
            }
        } else {
            cleaned.push(chars[i]);
            i += 1;
        }
    }

    cleaned
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
