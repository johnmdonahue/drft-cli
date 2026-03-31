use super::{ParseResult, Parser};
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Script-based parser. Runs an external command that receives a JSON
/// options envelope on line 1, then file paths (one per line) on stdin,
/// and emits NDJSON links on stdout.
pub struct ScriptParser {
    pub parser_name: String,
    /// File routing filter. None = receives all File nodes.
    pub file_filter: Option<globset::GlobSet>,
    pub command: String,
    pub timeout_ms: u64,
    pub scope_dir: std::path::PathBuf,
    /// Parser options from `[parsers.<name>.options]`. Sent as JSON on stdin line 1.
    pub options: Option<toml::Value>,
}

impl Parser for ScriptParser {
    fn name(&self) -> &str {
        &self.parser_name
    }

    fn matches(&self, path: &str) -> bool {
        match &self.file_filter {
            Some(set) => set.is_match(path),
            None => true, // No filter = receives all File nodes
        }
    }

    fn parse(&self, path: &str, _content: &str) -> ParseResult {
        // Single-file fallback — used when parse_batch isn't called
        match self.run_batch(&[path]) {
            Ok(mut results) => results.remove(path).unwrap_or_default(),
            Err(e) => {
                eprintln!("warn: parser {}: {path}: {e}", self.parser_name);
                ParseResult::default()
            }
        }
    }

    fn parse_batch(&self, files: &[(&str, &str)]) -> HashMap<String, ParseResult> {
        let paths: Vec<&str> = files.iter().map(|(path, _)| *path).collect();
        match self.run_batch(&paths) {
            Ok(results) => results,
            Err(e) => {
                eprintln!("warn: parser {}: batch failed: {e}", self.parser_name);
                HashMap::new()
            }
        }
    }
}

impl ScriptParser {
    fn run_batch(&self, paths: &[&str]) -> anyhow::Result<HashMap<String, ParseResult>> {
        if paths.is_empty() {
            return Ok(HashMap::new());
        }

        let mut child = Command::new("sh")
            .arg("-c")
            .arg(&self.command)
            .current_dir(&self.scope_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Send options envelope on line 1, then file paths
        if let Some(mut stdin) = child.stdin.take() {
            let options_json = match &self.options {
                Some(val) => serde_json::to_string(val).unwrap_or_else(|_| "{}".into()),
                None => "{}".into(),
            };
            let _ = writeln!(stdin, "{options_json}");
            for path in paths {
                let _ = writeln!(stdin, "{path}");
            }
        }

        // Wait with timeout
        let output = match wait_with_timeout(&mut child, Duration::from_millis(self.timeout_ms)) {
            Ok(output) => output,
            Err(_) => {
                let _ = child.kill();
                anyhow::bail!("timed out after {}ms", self.timeout_ms);
            }
        };

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            anyhow::bail!("exited with code {code}");
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut results: HashMap<String, ParseResult> = HashMap::new();

        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // Try as edge first ({ file, target, type }), then as metadata ({ file, metadata })
            let json: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!(
                        "warn: parser {}: malformed JSON line: {e}",
                        self.parser_name
                    );
                    continue;
                }
            };

            let file = match json.get("file").and_then(|v| v.as_str()) {
                Some(f) => f.to_string(),
                None => {
                    eprintln!(
                        "warn: parser {}: JSON line missing 'file' field",
                        self.parser_name
                    );
                    continue;
                }
            };

            if let Some(metadata) = json.get("metadata") {
                // Metadata line: { file, metadata }
                results.entry(file).or_default().metadata = Some(metadata.clone());
            } else if let Some(target) = json.get("target").and_then(|v| v.as_str()) {
                // Edge line: { file, target }
                results
                    .entry(file)
                    .or_default()
                    .links
                    .push(target.to_string());
            } else {
                eprintln!(
                    "warn: parser {}: JSON line has neither 'target' nor 'metadata'",
                    self.parser_name
                );
            }
        }

        Ok(results)
    }
}

fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<std::process::Output, ()> {
    // Take pipes before the poll loop so reader threads can drain them
    // concurrently. This prevents deadlock when a script fills the OS
    // pipe buffer (~64 KB) — without concurrent draining, the child
    // blocks on write while the parent blocks waiting for exit.
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let stdout_thread = std::thread::spawn(move || {
        stdout_pipe
            .map(|mut s| {
                let mut buf = Vec::new();
                std::io::Read::read_to_end(&mut s, &mut buf).ok();
                buf
            })
            .unwrap_or_default()
    });
    let stderr_thread = std::thread::spawn(move || {
        stderr_pipe
            .map(|mut s| {
                let mut buf = Vec::new();
                std::io::Read::read_to_end(&mut s, &mut buf).ok();
                buf
            })
            .unwrap_or_default()
    });

    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = stdout_thread.join().unwrap_or_default();
                let stderr = stderr_thread.join().unwrap_or_default();
                return Ok(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    // Join reader threads so they don't leak. The child
                    // will be killed by the caller, which closes the
                    // pipes and unblocks the readers.
                    let _ = stdout_thread.join();
                    let _ = stderr_thread.join();
                    return Err(());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                let _ = stdout_thread.join();
                let _ = stderr_thread.join();
                return Err(());
            }
        }
    }
}
