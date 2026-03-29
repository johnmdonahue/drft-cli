use std::path::Path;
use std::process::Command;

use super::{Metric, MetricKind};
use crate::config::{Config, CustomMetricConfig};
use crate::graph::Graph;
use crate::rules::custom::build_graph_json;

/// Run a custom metric script. Receives graph as JGF JSON on stdin,
/// emits metrics as NDJSON on stdout.
///
/// Expected output format per line:
/// {"name": "my_metric", "value": 0.42, "kind": "ratio", "dimension": "completeness"}
pub fn run_custom_metrics(
    name: &str,
    metric_config: &CustomMetricConfig,
    graph: &Graph,
    root: &Path,
    config_dir: &Path,
) -> anyhow::Result<Vec<Metric>> {
    let graph_json = build_graph_json(graph);

    let parts: Vec<&str> = metric_config.command.split_whitespace().collect();
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
    let mut metrics = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match serde_json::from_str::<RawCustomMetric>(line) {
            Ok(raw) => {
                let kind = match raw.kind.as_str() {
                    "count" => MetricKind::Count,
                    "score" => MetricKind::Score,
                    _ => MetricKind::Ratio,
                };
                metrics.push(Metric {
                    name: raw.name,
                    value: raw.value,
                    kind,
                    dimension: raw.dimension,
                });
            }
            Err(e) => {
                eprintln!("warn: custom metrics \"{name}\": failed to parse output line: {e}");
            }
        }
    }

    Ok(metrics)
}

/// Run all custom metrics defined in the config.
pub fn run_all_custom_metrics(graph: &Graph, root: &Path, config: &Config) -> Vec<Metric> {
    let config_dir = config.config_dir.as_deref().unwrap_or(root);
    let mut all_metrics = Vec::new();

    for (name, metric_config) in &config.custom_metrics {
        match run_custom_metrics(name, metric_config, graph, root, config_dir) {
            Ok(mut metrics) => all_metrics.append(&mut metrics),
            Err(e) => {
                eprintln!("warn: custom metrics \"{name}\" failed: {e}");
            }
        }
    }

    all_metrics
}

#[derive(serde::Deserialize)]
struct RawCustomMetric {
    name: String,
    value: f64,
    #[serde(default = "default_ratio")]
    kind: String,
    #[serde(default = "default_custom")]
    dimension: String,
}

fn default_ratio() -> String {
    "ratio".into()
}

fn default_custom() -> String {
    "custom".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Graph, Node, NodeType};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn runs_custom_metric_script() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("my-metrics.sh");
        fs::write(
            &script,
            "#!/bin/sh\necho '{\"name\": \"custom_score\", \"value\": 0.75, \"kind\": \"score\", \"dimension\": \"custom\"}'\n",
        )
        .unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let config = CustomMetricConfig {
            command: script.to_string_lossy().to_string(),
        };

        let mut graph = Graph::new();
        graph.add_node(Node {
            path: "a.md".into(),
            node_type: NodeType::Document,
            hash: None,
        });

        let metrics =
            run_custom_metrics("my-metrics", &config, &graph, dir.path(), dir.path()).unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].name, "custom_score");
        assert_eq!(metrics[0].value, 0.75);
    }

    #[test]
    fn handles_failing_script() {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("bad.sh");
        fs::write(&script, "#!/bin/sh\nexit 1\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let config = CustomMetricConfig {
            command: script.to_string_lossy().to_string(),
        };

        let graph = Graph::new();
        let result = run_custom_metrics("bad", &config, &graph, dir.path(), dir.path());
        assert!(result.is_err());
    }
}
