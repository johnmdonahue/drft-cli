use std::path::Path;
use std::process::Command;

use crate::config::{Config, CustomRuleConfig};
use crate::diagnostic::Diagnostic;
use crate::graph::Graph;

/// Run all custom rules defined in the config against the graph.
/// Each custom rule receives the graph as JGF JSON on stdin and
/// emits diagnostics as newline-delimited JSON on stdout.
///
/// Expected output format per line:
/// {"message": "...", "source": "...", "target": "...", "node": "...", "fix": "..."}
///
/// All fields except `message` are optional. The `rule` and `severity` fields
/// are set by drft from the config — the script doesn't need to provide them.
pub fn run_custom_rules(graph: &Graph, root: &Path, config: &Config) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let config_dir = config.config_dir.as_deref().unwrap_or(root);

    for (rule_name, rule_config) in &config.custom_rules {
        match run_one(rule_name, rule_config, graph, root, config_dir) {
            Ok(mut results) => diagnostics.append(&mut results),
            Err(e) => {
                eprintln!("warn: custom rule \"{rule_name}\" failed: {e}");
                // Surface failures as diagnostics so JSON consumers see them
                diagnostics.push(Diagnostic {
                    rule: rule_name.clone(),
                    severity: rule_config.severity,
                    message: format!("custom rule failed: {e}"),
                    fix: Some(format!(
                        "custom rule \"{rule_name}\" failed to execute — check the command path and script"
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
    rule_config: &CustomRuleConfig,
    graph: &Graph,
    root: &Path,
    config_dir: &Path,
) -> anyhow::Result<Vec<Diagnostic>> {
    // Build the graph JSON to pass on stdin
    let graph_json = build_graph_json(graph);

    // Parse command string (split on whitespace for simple commands)
    let parts: Vec<&str> = rule_config.command.split_whitespace().collect();
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
                eprintln!("warn: custom rule \"{rule_name}\": failed to parse output line: {e}");
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

pub(crate) fn build_graph_json(graph: &Graph) -> String {
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
            serde_json::json!({
                "source": e.source,
                "target": e.target,
                "relation": e.edge_type,
            })
        })
        .collect();

    let output = serde_json::json!({
        "graph": {
            "directed": true,
            "nodes": nodes,
            "edges": edges,
        }
    });

    serde_json::to_string(&output).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, EdgeType, Graph, Node, NodeType};
    use std::fs;
    use tempfile::TempDir;

    fn make_graph() -> Graph {
        let mut g = Graph::new();
        g.add_node(Node {
            path: "index.md".into(),
            node_type: NodeType::Document,
            hash: Some("b3:aaa".into()),
        });
        g.add_node(Node {
            path: "setup.md".into(),
            node_type: NodeType::Document,
            hash: Some("b3:bbb".into()),
        });
        g.add_edge(Edge {
            source: "index.md".into(),
            target: "setup.md".into(),
            edge_type: EdgeType::Inline,
        });
        g
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

        let config = CustomRuleConfig {
            command: script.to_string_lossy().to_string(),
            severity: crate::config::RuleSeverity::Warn,
        };

        let graph = make_graph();
        let diagnostics = run_one("my-rule", &config, &graph, dir.path(), dir.path()).unwrap();

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

        let config = CustomRuleConfig {
            command: script.to_string_lossy().to_string(),
            severity: crate::config::RuleSeverity::Warn,
        };

        let graph = make_graph();
        let result = run_one("bad-rule", &config, &graph, dir.path(), dir.path());
        assert!(result.is_err());
    }
}
