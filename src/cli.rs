use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "drft",
    version,
    about = "A drift checker for linked files, built for LLMs and humans working in the same repo"
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
        /// Lock only these paths and their outbound edges (default: lock the whole graph)
        paths: Vec<String>,
    },

    /// Render the composed graph as text or JGF
    Graph {
        /// Emit the raw set of per-graph fragments instead of the composed graph
        /// (JSON only; ignores --format)
        #[arg(long)]
        raw: bool,
    },

    /// List the nodes that transitively depend on the given files
    Impact {
        /// Files to compute the impact of (at least one required)
        #[arg(required = true)]
        paths: Vec<String>,

        /// How many hops to traverse, or `all` for the full reachable set
        #[arg(long, default_value = "1")]
        depth: Depth,

        /// Traversal direction
        #[arg(long, default_value = "inbound")]
        direction: Direction,
    },

    /// Project nodes and their metadata, scoped by selector and narrowed by namespace/field
    Nodes {
        /// Globset patterns matched against node keys; a bare directory is its
        /// recursive subtree. With none, every node is returned.
        selectors: Vec<String>,

        /// Restrict to a declared graph namespace (repeatable). Accepts the bare
        /// name (`frontmatter`) or the prefixed key (`@frontmatter`).
        #[arg(long = "namespace")]
        namespaces: Vec<String>,

        /// Restrict returned metadata to named fields (repeatable).
        #[arg(long = "field")]
        fields: Vec<String>,
    },

    /// Project edges (matched on source), scoped by selector and narrowed by namespace/field
    Edges {
        /// Globset patterns matched against node keys (edge sources); a bare
        /// directory is its recursive subtree. With none, every edge is returned.
        selectors: Vec<String>,

        /// Restrict to a declared graph namespace (repeatable). Accepts the bare
        /// name (`markdown`) or the prefixed key (`@markdown`).
        #[arg(long = "namespace")]
        namespaces: Vec<String>,

        /// Restrict returned metadata to named fields (repeatable).
        #[arg(long = "field")]
        fields: Vec<String>,
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

/// How far `impact` walks. Defaults to one hop: the files that name the seed
/// directly, each of which is a promise someone wrote down. The full reachable
/// set answers a graph-theory question rather than the one asked at an edit, so
/// it has to be requested by name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Depth {
    All,
    Hops(usize),
}

impl Depth {
    /// The traversal bound: `None` is unbounded.
    pub fn max_hops(self) -> Option<usize> {
        match self {
            Depth::All => None,
            Depth::Hops(n) => Some(n),
        }
    }
}

impl std::str::FromStr for Depth {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("all") {
            return Ok(Depth::All);
        }
        match s.parse::<usize>() {
            // `0` reads as "no traversal", so it must not quietly mean the
            // opposite. Point at the spelling that does mean everything.
            Ok(0) => Err(
                "depth 0 would traverse nothing; use `--depth all` for the full reachable set"
                    .into(),
            ),
            Ok(n) => Ok(Depth::Hops(n)),
            Err(_) => Err(format!(
                "invalid depth `{s}` (expected a positive number or `all`)"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn depth_parses_hops_and_all() {
        assert_eq!(Depth::from_str("1").unwrap(), Depth::Hops(1));
        assert_eq!(Depth::from_str("3").unwrap(), Depth::Hops(3));
        assert_eq!(Depth::from_str("all").unwrap(), Depth::All);
        assert_eq!(Depth::from_str("ALL").unwrap(), Depth::All);
    }

    #[test]
    fn depth_zero_is_rejected_with_a_pointer_to_all() {
        // `0` reads as "no traversal"; it must not silently mean the opposite.
        let err = Depth::from_str("0").unwrap_err();
        assert!(err.contains("--depth all"), "got: {err}");
    }

    #[test]
    fn depth_rejects_nonsense() {
        assert!(Depth::from_str("banana").is_err());
        assert!(Depth::from_str("-1").is_err());
    }

    #[test]
    fn max_hops_maps_all_to_unbounded() {
        assert_eq!(Depth::All.max_hops(), None);
        assert_eq!(Depth::Hops(2).max_hops(), Some(2));
    }
}
