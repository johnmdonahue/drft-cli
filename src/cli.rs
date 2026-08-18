use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "drft",
    version,
    about = "Structural integrity checker for linked file systems"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Run as if started in <path>
    #[arg(short = 'C', global = true)]
    pub directory: Option<PathBuf>,

    /// Output format
    #[arg(long, global = true, default_value = "text")]
    pub format: OutputFormat,

    /// Colorize output
    #[arg(long, global = true, default_value = "auto")]
    pub color: ColorChoice,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create a drft.toml config file
    Init,

    /// Snapshot the current state to drft.lock
    Lock {
        /// Lock only these paths and their outbound edges (default: lock the whole
        /// graph). A path that is locked but no longer on disk is dropped — the way
        /// a removed-node finding is reviewed.
        paths: Vec<String>,
    },

    /// Export the dependency graph as composed JGF
    Graph {
        /// Emit the raw set of per-graph fragments instead of the composed graph
        #[arg(long)]
        raw: bool,
    },

    /// List the nodes that transitively depend on the given files
    Impact {
        /// Files to compute the impact of (at least one required)
        #[arg(required = true)]
        paths: Vec<String>,

        /// Limit traversal to this many hops
        #[arg(long)]
        depth: Option<usize>,

        /// Traversal direction
        #[arg(long, default_value = "inbound")]
        direction: Direction,
    },

    /// Check the composed graph against the lockfile for drift and structural findings
    Check,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Direction {
    Inbound,
    Outbound,
    Both,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}
