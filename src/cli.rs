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
    Lock,

    /// Show raw parser output (edges and metadata)
    Parse {
        /// Run only a specific parser
        #[arg(long)]
        parser: Option<String>,
    },

    /// Export the dependency graph as composed JGF
    Graph {
        /// Emit the raw set of per-graph fragments instead of the composed graph
        #[arg(long)]
        raw: bool,
    },

    /// Show what depends on the given files (transitively)
    Impact {
        /// Files to analyze (relative paths)
        #[arg(required = true)]
        files: Vec<String>,

        /// Only include edges from this parser
        #[arg(long)]
        parser: Option<String>,
    },

    /// [unstable] Run graph analyses and health metrics
    Report {
        /// Analyses or metrics to include (defaults to all)
        names: Vec<String>,
    },

    /// Show resolved configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Check the composed graph against the lockfile for drift and structural findings
    Check,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Print the resolved configuration (defaults filled in)
    Show,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}
