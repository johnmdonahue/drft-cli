use std::process::Command;

pub fn drft_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_drft"))
}

/// Declares the markdown + frontmatter graphs — the former built-in defaults,
/// now declared explicitly since there are no default graphs. Not every test
/// binary uses it.
#[allow(dead_code)]
pub const DEFAULT_CONFIG: &str = "\
[graphs.markdown]
parser = \"markdown\"
files = [\"**/*.md\"]

[graphs.frontmatter]
parser = \"frontmatter\"
files = [\"**/*.md\"]
edge_keys = [\"sources\"]
";

/// Markdown only. For tests asserting an exact hint set: a frontmatter graph
/// declaring `edge_keys` over a fixture whose files carry no such frontmatter
/// raises `edge-keys-matched-nothing`, which is correct and has nothing to do
/// with what those tests are about.
#[allow(dead_code)]
pub const MARKDOWN_ONLY_CONFIG: &str = "\
[graphs.markdown]
parser = \"markdown\"
files = [\"**/*.md\"]
";
