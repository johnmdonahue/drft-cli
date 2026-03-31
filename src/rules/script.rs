use std::path::Path;
use std::process::Command;

use crate::analyses::EnrichedGraph;
use crate::config::{Config, RuleConfig};
use crate::diagnostic::Diagnostic;

/// Run all script rules defined in the config against the enriched graph.
/// Script rules are rules with a `command` field in `[rules]`.
/// Each script rule receives `{ graph, options }` as JSON on stdin —
/// the enriched graph (nodes, edges, analyses) plus the rule's options —
/// and emits diagnostics as newline-delimited JSON on stdout.
///
/// Expected output format per line:
/// {"message": "...", "source": "...", "target": "...", "node": "...", "fix": "..."}
///
/// All fields except `message` are optional. The `rule` and `severity` fields
/// are set by drft from the config — the script doesn't need to provide them.
pub fn run_script_rules(enriched: &EnrichedGraph, root: &Path, config: &Config) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let config_dir = config.config_dir.as_deref().unwrap_or(root);

    for (rule_name, rule_config) in config.script_rules() {
        match run_one(rule_name, rule_config, enriched, root, config_dir) {
            Ok(mut results) => diagnostics.append(&mut results),
            Err(e) => {
                eprintln!("warn: script rule \"{rule_name}\" failed: {e}");
                // Surface failures as diagnostics so JSON consumers see them
                diagnostics.push(Diagnostic {
                    rule: rule_name.to_string(),
                    severity: rule_config.severity,
                    message: format!("script rule failed: {e}"),
                    fix: Some(format!(
                        "script rule \"{rule_name}\" failed to execute — check the command path and script"
                    )),
                    ..Default::default()
                });
            }
        }
    }

    diagnostics
}

fn run_one(
    rule_name: &str,
    rule_config: &RuleConfig,
    enriched: &EnrichedGraph,
    root: &Path,
    config_dir: &Path,
) -> anyhow::Result<Vec<Diagnostic>> {
    let command = rule_config
        .command
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("rule \"{rule_name}\" has no command"))?;

    // Build the enriched graph + options JSON to pass on stdin
    let graph_json = build_enriched_json(enriched, rule_config.options.as_ref());

    // Parse command string (split on whitespace for simple commands)
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        anyhow::bail!("empty command");
    }

    // Resolve command path relative to config directory (where drft.toml lives)
    let cmd = if parts[0].starts_with("./") || parts[0].starts_with("../") {
        config_dir.join(parts[0]).to_string_lossy().to_string()
    } else {
        parts[0].to_string()
    };

    let output = Command::new(&cmd)
        .args(&parts[1..])
        .current_dir(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                let _ = stdin.write_all(graph_json.as_bytes());
            }
            child.wait_with_output()
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("exited with {}: {}", output.status, stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut diagnostics = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match serde_json::from_str::<CustomDiagnostic>(line) {
            Ok(cd) => {
                diagnostics.push(Diagnostic {
                    rule: rule_name.to_string(),
                    severity: rule_config.severity,
                    message: cd.message,
                    source: cd.source,
                    target: cd.target,
                    node: cd.node,
                    fix: cd.fix,
                    ..Default::default()
                });
            }
            Err(e) => {
                eprintln!("warn: script rule \"{rule_name}\": failed to parse output line: {e}");
            }
        }
    }

    Ok(diagnostics)
}

#[derive(serde::Deserialize)]
struct CustomDiagnostic {
    message: String,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    node: Option<String>,
    #[serde(default)]
    fix: Option<String>,
}

/// Build the JSON envelope sent to script rules: `{ graph, options }`.
///
/// The `graph` object contains the full enriched graph — nodes, edges,
/// and all analysis results. `options` carries the rule's `[rules.<name>.options]`.
fn build_enriched_json(enriched: &EnrichedGraph, options: Option<&toml::Value>) -> String {
    let graph = &enriched.graph;

    let mut nodes = serde_json::Map::new();
    for (path, node) in &graph.nodes {
        let mut meta = serde_json::Map::new();
        meta.insert("type".into(), serde_json::json!(node.node_type));
        if let Some(h) = &node.hash {
            meta.insert("hash".into(), serde_json::json!(h));
        }
        nodes.insert(path.clone(), serde_json::json!({ "metadata": meta }));
    }

    let edges: Vec<serde_json::Value> = graph
        .edges
        .iter()
        .filter(|e| graph.nodes.contains_key(&e.target))
        .map(|e| {
            let mut edge = serde_json::json!({
                "source": e.source,
                "target": e.target,
                "parser": e.parser,
            });
            if let Some(ref r) = e.link {
                edge["link"] = serde_json::json!(r);
            }
            edge
        })
        .collect();

    let analyses = serde_json::json!({
        "betweenness": enriched.betweenness,
        "bridges": enriched.bridges,
        "change_propagation": enriched.change_propagation,
        "connected_components": enriched.connected_components,
        "degree": enriched.degree,
        "depth": enriched.depth,
        "graph_boundaries": enriched.graph_boundaries,
        "graph_stats": enriched.graph_stats,
        "impact_radius": enriched.impact_radius,
        "pagerank": enriched.pagerank,
        "scc": enriched.scc,
        "transitive_reduction": enriched.transitive_reduction,
    });

    let output = serde_json::json!({
        "graph": {
            "directed": true,
            "nodes": nodes,
            "edges": edges,
            "analyses": analyses,
        },
        "options": options.unwrap_or(&toml::Value::Table(Default::default())),
    });

    serde_json::to_string(&output).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyses::enrich_graph;
    use crate::graph::{Edge, Graph, Node, NodeType};
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;

    fn make_enriched(dir: &Path) -> EnrichedGraph {
        let mut g = Graph::new();
        g.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::File,
            hash: Some("b3:aaa".into()),
            graph: None,
            metadata: HashMap::new(),
        });
        g.add_node(Node {
            path: "setup.md".into(),
            node_type: NodeType::File,
            hash: Some("b3:bbb".into()),
            graph: None,
            metadata: HashMap::new(),
        });
        g.add_edge(Edge {
            source: "index.md".into(),
            target: "setup.md".into(),
            link: None,
            parser: "markdown".into(),
        });
        let config = crate::config::Config {
            include: vec!["*.md".into()],
            exclude: vec![],
            interface: None,
            parsers: std::collections::HashMap::new(),
            rules: std::collections::HashMap::new(),
            config_dir: None,
        };
        enrich_graph(g, dir, &config, None)
    }

    #[test]
    fn runs_custom_script() {
        let dir = TempDir::new().unwrap();

        // Write a simple script that emits one diagnostic
        let script = dir.path().join("my-rule.sh");
        fs::write(
            &script,
            "#!/bin/sh\necho '{\"message\": \"custom issue\", \"node\": \"index.md\", \"fix\": \"do something\"}'\n",
        )
        .unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let config = RuleConfig {
            command: Some(script.to_string_lossy().to_string()),
            severity: crate::config::RuleSeverity::Warn,
            ignore: Vec::new(),
            options: None,
            ignore_compiled: None,
        };

        let enriched = make_enriched(dir.path());
        let diagnostics = run_one("my-rule", &config, &enriched, dir.path(), dir.path()).unwrap();

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].rule, "my-rule");
        assert_eq!(diagnostics[0].message, "custom issue");
        assert_eq!(diagnostics[0].node.as_deref(), Some("index.md"));
        assert_eq!(diagnostics[0].fix.as_deref(), Some("do something"));
    }

    #[test]
    fn handles_failing_script() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("bad-rule.sh");
        fs::write(&script, "#!/bin/sh\nexit 1\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let config = RuleConfig {
            command: Some(script.to_string_lossy().to_string()),
            severity: crate::config::RuleSeverity::Warn,
            ignore: Vec::new(),
            options: None,
            ignore_compiled: None,
        };

        let enriched = make_enriched(dir.path());
        let result = run_one("bad-rule", &config, &enriched, dir.path(), dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn resolves_command_relative_to_config_dir() {
        let dir = TempDir::new().unwrap();

        // config_dir is the parent, root is a child subdirectory
        let config_dir = dir.path();
        let root = dir.path().join("docs");
        fs::create_dir_all(&root).unwrap();

        // Script lives relative to config_dir, not root
        let scripts_dir = config_dir.join("scripts");
        fs::create_dir_all(&scripts_dir).unwrap();
        let script = scripts_dir.join("check.sh");
        fs::write(
            &script,
            "#!/bin/sh\necho '{\"message\": \"found issue\", \"node\": \"index.md\"}'\n",
        )
        .unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let config = RuleConfig {
            command: Some("./scripts/check.sh".to_string()),
            severity: crate::config::RuleSeverity::Warn,
            ignore: Vec::new(),
            options: None,
            ignore_compiled: None,
        };

        let enriched = make_enriched(dir.path());
        // config_dir != root — script should resolve relative to config_dir
        let diagnostics = run_one("my-rule", &config, &enriched, &root, config_dir).unwrap();

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "found issue");
    }

    #[test]
    fn passes_options_to_script() {
        let dir = TempDir::new().unwrap();

        // Script reads stdin, parses the JSON, and echoes back whether options were received
        let script = dir.path().join("options-rule.sh");
        fs::write(
            &script,
            r#"#!/bin/sh
INPUT=$(cat)
# Check if options.threshold exists in the JSON
HAS_OPTIONS=$(echo "$INPUT" | grep -c '"threshold"')
if [ "$HAS_OPTIONS" -gt 0 ]; then
  echo '{"message": "got options"}'
else
  echo '{"message": "no options"}'
fi
"#,
        )
        .unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let options: toml::Value = toml::from_str("threshold = 5").unwrap();
        let config = RuleConfig {
            command: Some(script.to_string_lossy().to_string()),
            severity: crate::config::RuleSeverity::Warn,
            ignore: Vec::new(),
            options: Some(options),
            ignore_compiled: None,
        };

        let enriched = make_enriched(dir.path());
        let diagnostics =
            run_one("options-rule", &config, &enriched, dir.path(), dir.path()).unwrap();

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "got options");
    }

    #[test]
    fn includes_analyses_in_graph_json() {
        let dir = TempDir::new().unwrap();

        // Script checks that analyses are present in the graph JSON
        let script = dir.path().join("analyses-rule.sh");
        fs::write(
            &script,
            r#"#!/bin/sh
INPUT=$(cat)
HAS_ANALYSES=$(echo "$INPUT" | grep -c '"analyses"')
if [ "$HAS_ANALYSES" -gt 0 ]; then
  echo '{"message": "has analyses"}'
else
  echo '{"message": "no analyses"}'
fi
"#,
        )
        .unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let config = RuleConfig {
            command: Some(script.to_string_lossy().to_string()),
            severity: crate::config::RuleSeverity::Warn,
            ignore: Vec::new(),
            options: None,
            ignore_compiled: None,
        };

        let enriched = make_enriched(dir.path());
        let diagnostics =
            run_one("analyses-rule", &config, &enriched, dir.path(), dir.path()).unwrap();

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "has analyses");
    }
}
