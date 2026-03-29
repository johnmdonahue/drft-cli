use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "drft",
    about = "Structural integrity checker for markdown directories"
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
        /// Verify lockfile is up to date without writing
        #[arg(long)]
        check: bool,

        /// Seal the scope by declaring a manifest file
        #[arg(long, value_name = "FILE")]
        manifest: Option<String>,

        /// Unseal the scope by removing the manifest
        #[arg(long)]
        no_manifest: bool,

        /// Lock child scopes recursively (bottom-up)
        #[arg(long, short = 'r')]
        recursive: bool,

        /// Max scope nesting depth for --recursive
        #[arg(long, requires = "recursive")]
        max_depth: Option<usize>,
    },

    /// Export the dependency graph
    Graph {
        /// Include child scope graphs
        #[arg(long, short = 'r')]
        recursive: bool,

        /// Max scope nesting depth for --recursive
        #[arg(long, requires = "recursive")]
        max_depth: Option<usize>,
    },

    /// Show what depends on the given files (transitively)
    Impact {
        /// Files to analyze (relative paths)
        #[arg(required = true)]
        files: Vec<String>,
    },

    /// Run graph analyses and output structured results
    Report {
        /// Analyses to run (defaults to all)
        #[arg(long = "analysis", conflicts_with = "metrics")]
        analyses: Vec<String>,

        /// Output extracted scalar metrics instead of full analysis results
        #[arg(long)]
        metrics: bool,
    },

    /// Check markdown structure for rule violations
    Check {
        /// Run only specific rules (can be repeated)
        #[arg(long = "rule")]
        rules: Vec<String>,

        /// Check child scopes recursively
        #[arg(long, short = 'r')]
        recursive: bool,

        /// Max scope nesting depth for --recursive
        #[arg(long, requires = "recursive")]
        max_depth: Option<usize>,

        /// Watch for changes and re-check
        #[arg(long, short = 'w')]
        watch: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
    Dot,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ColorChoice {
    Auto,
    Always,
    Never,
}
