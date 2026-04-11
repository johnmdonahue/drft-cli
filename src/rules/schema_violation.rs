use crate::diagnostic::Diagnostic;
use crate::graph::NodeType;
use crate::rules::{Rule, RuleContext};

pub struct SchemaViolationRule;

impl Rule for SchemaViolationRule {
    fn name(&self) -> &str {
        "schema-violation"
    }

    fn evaluate(&self, ctx: &RuleContext) -> Vec<Diagnostic> {
        let options = match ctx.options {
            Some(opts) => opts,
            None => return vec![],
        };

        let global_required = extract_string_array(options, "required");
        let schemas = options
            .get("schemas")
            .and_then(|v| v.as_table())
            .cloned()
            .unwrap_or_default();

        let mut diagnostics = Vec::new();

        // Pre-compile glob matchers for schemas
        let mut compiled_schemas: Vec<(globset::GlobMatcher, SchemaSpec)> = Vec::new();
        for (pattern, value) in &schemas {
            match globset::Glob::new(pattern) {
                Ok(glob) => {
                    let spec = SchemaSpec::from_toml(value);
                    compiled_schemas.push((glob.compile_matcher(), spec));
                }
                Err(e) => {
                    diagnostics.push(Diagnostic {
                        rule: "schema-violation".into(),
                        message: format!("invalid schema glob \"{pattern}\": {e}"),
                        fix: Some(format!(
                            "fix the glob pattern \"{pattern}\" in [rules.schema-violation.options.schemas]"
                        )),
                        ..Default::default()
                    });
                }
            }
        }

        for (path, node) in &ctx.graph.graph.nodes {
            if node.node_type != NodeType::File {
                continue;
            }

            // Collect all metadata across parser namespaces into one merged view
            let metadata = merge_metadata(&node.metadata);
            let source = metadata_source(&node.metadata);

            // Check global required fields
            for field in &global_required {
                if !has_field(&metadata, field) {
                    diagnostics.push(Diagnostic {
                        rule: "schema-violation".into(),
                        message: format!("missing required field \"{field}\""),
                        node: Some(path.clone()),
                        fix: Some(format!("add \"{field}\" to {source} in {path}")),
                        ..Default::default()
                    });
                }
            }

            // Check per-glob schemas
            for (matcher, spec) in &compiled_schemas {
                if !matcher.is_match(path) {
                    continue;
                }

                for field in &spec.required {
                    if !has_field(&metadata, field) {
                        diagnostics.push(Diagnostic {
                            rule: "schema-violation".into(),
                            message: format!("missing required field \"{field}\""),
                            node: Some(path.clone()),
                            fix: Some(format!("add \"{field}\" to {source} in {path}")),
                            ..Default::default()
                        });
                    }
                }

                for (field, allowed_values) in &spec.allowed {
                    if let Some(value) = get_field(&metadata, field)
                        && let Some(s) = value_as_string(value)
                        && !allowed_values.iter().any(|av| av == &s)
                    {
                        diagnostics.push(Diagnostic {
                            rule: "schema-violation".into(),
                            message: format!(
                                "field \"{field}\" has value \"{s}\", allowed: [{}]",
                                allowed_values.join(", ")
                            ),
                            node: Some(path.clone()),
                            fix: Some(format!(
                                "change \"{field}\" in {path} to one of: {}",
                                allowed_values.join(", ")
                            )),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        diagnostics.sort_by(|a, b| a.node.cmp(&b.node));
        diagnostics
    }
}

struct SchemaSpec {
    required: Vec<String>,
    allowed: Vec<(String, Vec<String>)>,
}

impl SchemaSpec {
    fn from_toml(value: &toml::Value) -> Self {
        let required = extract_string_array(value, "required");
        let allowed = value
            .get("allowed")
            .and_then(|v| v.as_table())
            .map(|table| {
                table
                    .iter()
                    .map(|(k, v)| (k.clone(), extract_string_array_direct(v)))
                    .collect()
            })
            .unwrap_or_default();
        Self { required, allowed }
    }
}

fn extract_string_array(value: &toml::Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn extract_string_array_direct(value: &toml::Value) -> Vec<String> {
    value
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Merge metadata from all parser namespaces into a single flat JSON object.
/// Namespaces are merged in alphabetical order — later namespaces override earlier
/// ones for conflicting keys (e.g., "markdown" overrides "frontmatter").
fn merge_metadata(
    metadata: &std::collections::HashMap<String, serde_json::Value>,
) -> serde_json::Value {
    let mut merged = serde_json::Map::new();
    let mut keys: Vec<&String> = metadata.keys().collect();
    keys.sort();
    for key in keys {
        if let serde_json::Value::Object(map) = &metadata[key] {
            for (k, v) in map {
                merged.insert(k.clone(), v.clone());
            }
        }
    }
    serde_json::Value::Object(merged)
}

/// Return a human-readable label for the metadata source.
/// When a single parser contributed metadata, name it (e.g. "frontmatter").
/// Otherwise fall back to the generic "metadata".
fn metadata_source(metadata: &std::collections::HashMap<String, serde_json::Value>) -> String {
    let keys: Vec<&String> = metadata.keys().collect();
    if keys.len() == 1 {
        keys[0].clone()
    } else {
        "metadata".to_string()
    }
}

fn has_field(metadata: &serde_json::Value, field: &str) -> bool {
    metadata.get(field).is_some_and(|v| !v.is_null())
}

fn get_field<'a>(metadata: &'a serde_json::Value, field: &str) -> Option<&'a serde_json::Value> {
    metadata.get(field).filter(|v| !v.is_null())
}

fn value_as_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::test_helpers::make_enriched;
    use crate::graph::{Graph, Node, NodeType};
    use crate::rules::RuleContext;
    use std::collections::HashMap;

    fn node_with_metadata(path: &str, metadata: serde_json::Value) -> Node {
        let mut meta_map = HashMap::new();
        meta_map.insert("frontmatter".to_string(), metadata);
        Node {
            path: path.into(),
            node_type: NodeType::File,
            hash: None,
            metadata: meta_map,
        }
    }

    #[test]
    fn detects_missing_required_field() {
        let mut graph = Graph::new();
        graph.add_node(node_with_metadata(
            "doc.md",
            serde_json::json!({"status": "draft"}),
        ));

        let enriched = make_enriched(graph);
        let options: toml::Value = toml::from_str("required = [\"title\"]").unwrap();
        let ctx = RuleContext {
            graph: &enriched,
            options: Some(&options),
        };
        let diagnostics = SchemaViolationRule.evaluate(&ctx);

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("title"));
        assert_eq!(diagnostics[0].node.as_deref(), Some("doc.md"));
        // Fix message names the parser when there's a single contributor
        let fix = diagnostics[0].fix.as_ref().unwrap();
        assert!(
            fix.contains("frontmatter"),
            "fix should name the parser: {fix}"
        );
    }

    #[test]
    fn passes_when_required_field_present() {
        let mut graph = Graph::new();
        graph.add_node(node_with_metadata(
            "doc.md",
            serde_json::json!({"title": "Hello"}),
        ));

        let enriched = make_enriched(graph);
        let options: toml::Value = toml::from_str("required = [\"title\"]").unwrap();
        let ctx = RuleContext {
            graph: &enriched,
            options: Some(&options),
        };
        let diagnostics = SchemaViolationRule.evaluate(&ctx);

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn detects_per_glob_required() {
        let mut graph = Graph::new();
        graph.add_node(node_with_metadata(
            "observations/note.md",
            serde_json::json!({"title": "Note"}),
        ));
        // This file matches *.md but not observations/*.md
        graph.add_node(node_with_metadata(
            "readme.md",
            serde_json::json!({"title": "README"}),
        ));

        let enriched = make_enriched(graph);
        let options: toml::Value = toml::from_str(
            r#"
            [schemas."observations/*.md"]
            required = ["title", "date", "status"]
            "#,
        )
        .unwrap();
        let ctx = RuleContext {
            graph: &enriched,
            options: Some(&options),
        };
        let diagnostics = SchemaViolationRule.evaluate(&ctx);

        // observations/note.md missing "date" and "status"
        assert_eq!(diagnostics.len(), 2);
        let messages: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert!(messages.iter().any(|m| m.contains("date")));
        assert!(messages.iter().any(|m| m.contains("status")));
    }

    #[test]
    fn detects_disallowed_value() {
        let mut graph = Graph::new();
        graph.add_node(node_with_metadata(
            "observations/note.md",
            serde_json::json!({"title": "Note", "status": "invalid"}),
        ));

        let enriched = make_enriched(graph);
        let options: toml::Value = toml::from_str(
            r#"
            [schemas."observations/*.md"]
            required = ["title"]
            allowed.status = ["draft", "review", "final"]
            "#,
        )
        .unwrap();
        let ctx = RuleContext {
            graph: &enriched,
            options: Some(&options),
        };
        let diagnostics = SchemaViolationRule.evaluate(&ctx);

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("invalid"));
        assert!(diagnostics[0].message.contains("allowed"));
    }

    #[test]
    fn allowed_value_passes() {
        let mut graph = Graph::new();
        graph.add_node(node_with_metadata(
            "observations/note.md",
            serde_json::json!({"title": "Note", "status": "draft"}),
        ));

        let enriched = make_enriched(graph);
        let options: toml::Value = toml::from_str(
            r#"
            [schemas."observations/*.md"]
            allowed.status = ["draft", "review", "final"]
            "#,
        )
        .unwrap();
        let ctx = RuleContext {
            graph: &enriched,
            options: Some(&options),
        };
        let diagnostics = SchemaViolationRule.evaluate(&ctx);

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn no_options_no_diagnostics() {
        let mut graph = Graph::new();
        graph.add_node(node_with_metadata(
            "doc.md",
            serde_json::json!({"title": "Hello"}),
        ));

        let enriched = make_enriched(graph);
        let ctx = RuleContext {
            graph: &enriched,
            options: None,
        };
        let diagnostics = SchemaViolationRule.evaluate(&ctx);

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn skips_nodes_without_metadata() {
        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "no-frontmatter.md".into(),
            node_type: NodeType::File,
            hash: None,
            metadata: HashMap::new(),
        });

        let enriched = make_enriched(graph);
        let options: toml::Value = toml::from_str("required = [\"title\"]").unwrap();
        let ctx = RuleContext {
            graph: &enriched,
            options: Some(&options),
        };
        let diagnostics = SchemaViolationRule.evaluate(&ctx);

        // No metadata means no fields — should flag the missing required field
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("title"));
    }
}
