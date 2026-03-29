use std::path::Path;
use std::process::Command;

use crate::config::{Config, CustomAnalysisConfig};
use crate::graph::Graph;
use crate::rules::custom::build_graph_json;

/// Run a custom analysis script. Receives graph as JGF JSON on stdin,
/// returns arbitrary JSON on stdout.
pub fn run_custom_analysis(
    _name: &str,
    analysis_config: &CustomAnalysisConfig,
    graph: &Graph,
    root: &Path,
    config_dir: &Path,
) -> anyhow::Result<serde_json::Value> {
    let graph_json = build_graph_json(graph);

    let parts: Vec<&str> = analysis_config.command.split_whitespace().collect();
    if parts.is_empty() {
        anyhow::bail!("empty command");
    }

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
    let value: serde_json::Value = serde_json::from_str(&stdout)?;
    Ok(value)
}

/// Run all custom analyses defined in the config.
pub fn run_all_custom_analyses(
    graph: &Graph,
    root: &Path,
    config: &Config,
) -> Vec<(String, serde_json::Value)> {
    let config_dir = config.config_dir.as_deref().unwrap_or(root);
    let mut results = Vec::new();

    for (name, analysis_config) in &config.custom_analyses {
        match run_custom_analysis(name, analysis_config, graph, root, config_dir) {
            Ok(value) => results.push((name.clone(), value)),
            Err(e) => {
                eprintln!("warn: custom analysis \"{name}\" failed: {e}");
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Edge, EdgeType, Graph, Node, NodeType};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn runs_custom_analysis() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("my-analysis.sh");
        fs::write(
            &script,
            "#!/bin/sh\necho '{\"custom_property\": [1, 2, 3]}'\n",
        )
        .unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let config = CustomAnalysisConfig {
            command: script.to_string_lossy().to_string(),
        };

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "a.md".into(),
            node_type: NodeType::Document,
            hash: None,
        });

        let result =
            run_custom_analysis("my-analysis", &config, &graph, dir.path(), dir.path()).unwrap();
        assert_eq!(result["custom_property"], serde_json::json!([1, 2, 3]));
    }

    #[test]
    fn handles_failing_analysis() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("bad.sh");
        fs::write(&script, "#!/bin/sh\nexit 1\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let config = CustomAnalysisConfig {
            command: script.to_string_lossy().to_string(),
        };

        let graph = Graph::new();
        let result = run_custom_analysis("bad", &config, &graph, dir.path(), dir.path());
        assert!(result.is_err());
    }
}
