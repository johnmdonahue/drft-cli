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
