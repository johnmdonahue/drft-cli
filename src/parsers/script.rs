use super::{Parser, RawLink};
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Script-based parser. Runs an external command that receives file paths
/// on stdin (one per line) and emits NDJSON links on stdout.
pub struct ScriptParser {
    pub parser_name: String,
    pub glob: globset::GlobMatcher,
    pub type_filter: Option<Vec<String>>,
    pub command: String,
    pub timeout_ms: u64,
    pub scope_dir: std::path::PathBuf,
}

impl Parser for ScriptParser {
    fn name(&self) -> &str {
        &self.parser_name
    }

    fn matches(&self, path: &str) -> bool {
        let filename = path.rsplit('/').next().unwrap_or(path);
        self.glob.is_match(filename)
    }

    fn parse(&self, path: &str, _content: &str) -> Vec<RawLink> {
        // Single-file fallback — used when parse_batch isn't called
        match self.run_batch(&[path]) {
            Ok(mut results) => results.remove(path).unwrap_or_default(),
            Err(e) => {
                eprintln!("warn: parser {}: {path}: {e}", self.parser_name);
                Vec::new()
            }
        }
    }

    fn parse_batch(&self, files: &[(&str, &str)]) -> HashMap<String, Vec<RawLink>> {
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
    fn run_batch(&self, paths: &[&str]) -> anyhow::Result<HashMap<String, Vec<RawLink>>> {
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

        // Send all file paths on stdin, one per line
        if let Some(mut stdin) = child.stdin.take() {
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
        let mut results: HashMap<String, Vec<RawLink>> = HashMap::new();

        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match serde_json::from_str::<ScriptLink>(line) {
                Ok(sl) => {
                    let link = RawLink {
                        target: sl.target,
                        link_type: sl.link_type,
                        is_external: false,
                    };

                    // Apply type filter
                    if let Some(ref types) = self.type_filter
                        && !types.iter().any(|t| t == &link.link_type)
                    {
                        continue;
                    }

                    results.entry(sl.file).or_default().push(link);
                }
                Err(e) => {
                    eprintln!(
                        "warn: parser {}: malformed JSON line: {e}",
                        self.parser_name
                    );
                }
            }
        }

        Ok(results)
    }
}

#[derive(serde::Deserialize)]
struct ScriptLink {
    file: String,
    target: String,
    #[serde(rename = "type")]
    link_type: String,
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
                    return Err(());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return Err(()),
        }
    }
}
