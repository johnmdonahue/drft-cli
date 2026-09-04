//! Installed-binary guidance, joining clap syntax with exhaustive operational metadata.
use anyhow::{Context, Result, ensure};
use clap::{Arg, ArgAction, Command, CommandFactory, Parser};
use serde::Serialize;
use serde_json::Value;

use crate::cli::{Cli, Commands};
use crate::policy::{ExitStatus, OUTPUT_GUARD, OutputGuard, ResultShape};

#[derive(Serialize)]
pub struct Guide {
    schema_version: &'static str,
    drft_version: &'static str,
    schema_policy: &'static str,
    workflow: Workflow,
    exit_codes: Vec<ExitCode>,
    output: Value,
    global_controls: Vec<SyntaxItem>,
    commands: Vec<CommandGuide>,
}

#[derive(Serialize)]
struct Workflow {
    steps: Vec<Step>,
    whole_graph_lock: &'static str,
    preexisting_staleness: &'static str,
    empty_scope: &'static str,
    baseline: &'static str,
}
#[derive(Serialize)]
struct Step {
    phase: &'static str,
    command: Option<&'static str>,
    action: &'static str,
    widen_with: Vec<&'static str>,
}
#[derive(Serialize)]
struct ExitCode {
    code: i32,
    meaning: &'static str,
}
#[derive(Serialize)]
struct CommandGuide {
    name: String,
    syntax: Syntax,
    #[serde(flatten)]
    operational: Operational,
}
#[derive(Serialize)]
struct Syntax {
    summary: String,
    usage: String,
    inherits_global_controls: bool,
    arguments: Vec<SyntaxItem>,
    controls: Vec<SyntaxItem>,
    constraints: Vec<Constraint>,
}
#[derive(Serialize)]
struct SyntaxItem {
    kind: &'static str,
    name: String,
    short: Option<String>,
    long: Option<String>,
    value: Option<String>,
    help: String,
    global: bool,
    required: bool,
    repeatable: bool,
    default: Option<String>,
    possible_values: Vec<String>,
}
#[derive(Serialize)]
struct Constraint {
    kind: &'static str,
    subject: String,
    related: Vec<String>,
}
#[derive(Serialize)]
struct Operational {
    role: &'static str,
    capability: &'static str,
    reads: Vec<&'static str>,
    writes: Vec<&'static str>,
    operands: Vec<Semantics>,
    controls: Vec<Semantics>,
    success_document: Option<SuccessDocument>,
    exit_codes: Vec<i32>,
    boundary: Vec<&'static str>,
    output_guard: Option<OutputGuard>,
    example: &'static str,
}
#[derive(Serialize)]
struct Semantics {
    argument: &'static str,
    kind: &'static str,
    meaning: &'static str,
}
#[derive(Serialize)]
struct SuccessDocument {
    kind: &'static str,
    fields: Vec<&'static str>,
}

// These are examples, not a parallel declaration of command syntax. Parsing them
// produces the enum values consumed by the exhaustive operational match below.
const EXAMPLES: &[&str] = &[
    "drft init",
    "drft config --show-ignores --format json",
    "drft lock src/lib.rs docs/guide.md --format json",
    "drft graph --format json --max-bytes 65536",
    "drft impact src/lib.rs --depth 1 --direction inbound --format json",
    "drft nodes docs/ --namespace frontmatter --field purpose --format json",
    "drft edges docs/guide.md --namespace markdown --field line --format json",
    "drft check --format json",
    "drft guide --format json",
];

fn semantics(argument: &'static str, kind: &'static str, meaning: &'static str) -> Semantics {
    Semantics {
        argument,
        kind,
        meaning,
    }
}
fn result(kind: &'static str, shape: ResultShape) -> Option<SuccessDocument> {
    Some(SuccessDocument {
        kind,
        fields: shape.fields(),
    })
}
fn operational(command: &Commands, example: &'static str) -> Operational {
    let mut record = Operational {
        role: "",
        capability: "read",
        reads: vec!["composed-graph"],
        writes: vec![],
        operands: vec![],
        controls: vec![],
        success_document: None,
        exit_codes: vec![ExitStatus::Success.code(), ExitStatus::Failure.code()],
        boundary: vec![],
        output_guard: None,
        example,
    };
    match command {
        Commands::Init => {
            record.role = "configure";
            record.capability = "write-configuration";
            record.reads = vec![];
            record.writes = vec!["drft.toml"];
            record.boundary = vec![
                "Refuses when drft.toml exists; never overwrites it.",
                "Accepts the global format control but emits no success document.",
            ];
        }
        Commands::Config { .. } => {
            record.role = "inspect-configuration";
            record.reads = vec!["drft.toml", "repository-.gitignore"];
            record.controls.push(semantics(
                "show_ignores",
                "inspection-mode",
                "Report the ignore sources used for filesystem discovery.",
            ));
            record.success_document = Some(SuccessDocument {
                kind: "ignore-source-report",
                fields: vec![],
            });
            record.boundary = vec!["Does not build the graph or update drft.lock."];
        }
        Commands::Lock { .. } => {
            record.role = "record-reviewed-state";
            record.capability = "write-baseline";
            record.reads.push("drft.lock");
            record.writes = vec!["drft.lock"];
            record.operands.push(exact_paths());
            record.controls.push(semantics("all", "whole-graph-acknowledgement", "Affirm every lockable node after whole-graph review, or deliberately establish or rebuild the whole baseline."));
            record.success_document = result("lock-result", ResultShape::Lock);
            record.boundary = vec![
                "Never widen an empty scoped lock.",
                "Resolves the complete batch before writing.",
                "Refuses a scoped merge into an unreadable baseline.",
                "A missing or file-now-directory operand may drop its stale entry.",
                "Whole-graph locking rebuilds the complete baseline and reports dropped entries.",
                "Lock every path whose current content or outbound promises were reviewed, including unchanged reviewed dependents.",
                "Do not lock unrelated pre-existing staleness.",
                "No .md fallback, directory expansion, subtree expansion, or glob expansion.",
            ];
        }
        Commands::Graph { .. } => {
            record.role = "read-complete-graph";
            record.controls.push(semantics(
                "raw",
                "raw-graph-set",
                "Emit the raw JSON graph set; ignores the output format control.",
            ));
            record.success_document = Some(SuccessDocument {
                kind: "text-graph-or-bare-jgf-or-raw-graph-set-json",
                fields: vec![],
            });
            record.boundary = vec![
                "Has no narrowing controls.",
                "Bare JGF and raw-graph hints use stderr; the selected format controls their encoding.",
            ];
            guard(&mut record);
        }
        Commands::Impact { .. } => {
            record.role = "traverse";
            record.operands.push(exact_paths());
            record.controls.push(semantics("depth", "traversal-bound", "A positive integer bounds hops; the unbounded form traverses the full reachable set."));
            record.controls.push(semantics("direction", "traversal-direction", "Inbound follows dependents; outbound follows dependencies; both follows either direction."));
            record.success_document = result("impact-result", ResultShape::Impact);
            record.reads.push("drft.lock");
            record.boundary = vec![
                "No .md fallback, directory expansion, subtree expansion, or glob expansion. Seeds must exist in the current graph.",
                "Diagnostics include construction findings from all configured graphs, including disconnected files and metadata-only graphs. They identify read failures, not inferred dependencies on seeds.",
                "Traversal diagnostics cover every edge inspected within the requested direction and depth, including cycles and edges between seeds.",
                "Historical pairs in the optional lock qualify losses beside the current expansion frontier; they never extend traversal, ranking, or total. Missing and empty baselines are quiet.",
                "Configured severity and subject ignores apply. Only construction, unresolved-edge, unresolved-fragment, removed-edge, and applicable removed-node findings are included. A completed read exits 0 even with error diagnostics; check remains the rule gate.",
                "Construction diagnostic scope is global: narrowing seeds, depth, or direction only shrinks traversal output. Increase --max-bytes or repair read failures when those diagnostics exceed the budget.",
            ];
            guard(&mut record);
        }
        Commands::Nodes { .. } => {
            projection(&mut record, false);
        }
        Commands::Edges { .. } => {
            projection(&mut record, true);
        }
        Commands::Check => {
            record.role = "gate";
            record.reads.extend(["configuration", "drft.lock"]);
            record.success_document = result("check-result", ResultShape::Check);
            record.exit_codes.insert(1, ExitStatus::Violations.code());
            record.boundary = vec![
                "Read every finding; warnings do not cause the violations exit status and must not be ignored or suppressed.",
                "A missing, empty, or unusable baseline is a promotable finding/configuration state requiring review, not authority to lock.",
            ];
        }
        Commands::Guide => {
            record.role = "describe-installed-binary";
            record.reads = vec!["compiled-command-metadata", "compiled-operational-contract"];
            record.success_document = Some(SuccessDocument {
                kind: "this-contract",
                fields: vec![],
            });
            record.boundary = vec![
                "Requires no repository or config.",
                "Does not walk the repository, load drft.toml, read drft.lock, or build the graph.",
            ];
        }
    }
    record
}
fn exact_paths() -> Semantics {
    semantics(
        "paths",
        "exact-node-path",
        "Exact cwd-relative, graph-root-contained node paths; no fallback or expansion.",
    )
}
fn guard(record: &mut Operational) {
    record.output_guard = Some(OUTPUT_GUARD);
    record.controls.push(semantics(
        "max_bytes",
        "output-guard",
        "Refuse the complete stdout document when it exceeds the byte limit.",
    ));
    record.boundary.extend([
        "Never truncates output.",
        "The output byte guard refuses atomically before stdout.",
    ]);
}
fn projection(record: &mut Operational, edges: bool) {
    record.role = if edges {
        "project-outbound-edges"
    } else {
        "project-nodes"
    };
    record.operands.push(semantics("selectors", "read-selector", if edges { "Match edge sources, not targets: exact path, recursive bare directory, or globset pattern." } else { "Match nodes: exact path, recursive bare directory, or globset pattern." }));
    record.controls.extend([
        semantics(
            "all",
            "whole-graph-acknowledgement",
            "Select the whole graph.",
        ),
        semantics(
            "namespaces",
            "metadata-filter",
            "Restrict to declared graph namespaces; accept bare or prefixed namespace names.",
        ),
        semantics(
            "fields",
            "metadata-filter",
            "Restrict returned metadata to named fields.",
        ),
    ]);
    record.success_document = if edges {
        result("edges-result", ResultShape::Edges)
    } else {
        result("nodes-result", ResultShape::Nodes)
    };
    record.boundary = vec![
        "An exact miss fails; a glob or field miss may return an empty result.",
        "Unknown namespaces fail.",
        "Never widen an empty scope.",
    ];
    if edges {
        record
            .boundary
            .push("Selectors match edge sources, not targets.");
    }
    guard(record);
}

fn visible(arg: &&Arg) -> bool {
    !arg.is_hide_set()
        && !matches!(
            arg.get_action(),
            ArgAction::Help | ArgAction::HelpShort | ArgAction::HelpLong | ArgAction::Version
        )
}
fn syntax_item(arg: &Arg) -> SyntaxItem {
    let positional = arg.get_index().is_some();
    let takes_values = arg.get_action().takes_values();
    SyntaxItem {
        kind: if positional {
            "positional"
        } else if takes_values {
            "option"
        } else {
            "flag"
        },
        name: arg.get_id().to_string(),
        short: arg.get_short().map(|s| s.to_string()),
        long: arg.get_long().map(str::to_owned),
        value: takes_values.then(|| {
            arg.get_value_names()
                .map(|names| {
                    names
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_else(|| arg.get_id().to_string().to_uppercase())
        }),
        help: arg.get_help().map(ToString::to_string).unwrap_or_default(),
        global: arg.is_global_set(),
        required: arg.is_required_set(),
        repeatable: matches!(arg.get_action(), ArgAction::Append | ArgAction::Count)
            || arg.get_num_args().is_some_and(|n| n.max_values() > 1),
        default: (!arg.get_default_values().is_empty()).then(|| {
            arg.get_default_values()
                .iter()
                .map(|v| v.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ")
        }),
        possible_values: if takes_values {
            arg.get_value_parser()
                .possible_values()
                .map(|values| {
                    values
                        .filter(|v| !v.is_hide_set())
                        .map(|v| v.get_name().to_owned())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            vec![]
        },
    }
}
fn syntax(command: &mut Command) -> Syntax {
    let local: Vec<_> = command
        .get_arguments()
        .filter(visible)
        .filter(|arg| !arg.is_global_set())
        .collect();
    let mut constraints = vec![];
    for group in command.get_groups().filter(|group| group.is_required_set()) {
        constraints.push(Constraint {
            kind: "requires_one_of",
            subject: "command".into(),
            related: group.get_args().map(ToString::to_string).collect(),
        });
    }
    for arg in &local {
        // Required positional cardinality is already represented by `required`.
        if arg.is_required_set() && arg.get_index().is_none() {
            constraints.push(Constraint {
                kind: "requires_one_of",
                subject: "command".into(),
                related: vec![arg.get_id().to_string()],
            });
        }
        let related = command
            .get_arg_conflicts_with(arg)
            .iter()
            .map(|a| a.get_id().to_string())
            .collect::<Vec<_>>();
        if !related.is_empty() {
            constraints.push(Constraint {
                kind: "conflicts_with",
                subject: arg.get_id().to_string(),
                related,
            });
        }
    }
    let arguments = local
        .iter()
        .filter(|a| a.get_index().is_some())
        .map(|a| syntax_item(a))
        .collect();
    let controls = local
        .iter()
        .filter(|a| a.get_index().is_none())
        .map(|a| syntax_item(a))
        .collect();
    Syntax {
        summary: command
            .get_about()
            .map(ToString::to_string)
            .unwrap_or_default(),
        usage: command
            .render_usage()
            .to_string()
            .trim_start_matches("Usage: ")
            .to_owned(),
        inherits_global_controls: true,
        arguments,
        controls,
        constraints,
    }
}

pub fn document() -> Result<Guide> {
    document_from(Cli::command(), EXAMPLES)
}
fn document_from(mut root: Command, examples: &[&'static str]) -> Result<Guide> {
    root.build();
    let global_controls: Vec<_> = root
        .get_arguments()
        .filter(visible)
        .filter(|a| a.is_global_set())
        .map(syntax_item)
        .collect();
    let mut output = crate::policy::output_contract();
    let format = global_controls
        .iter()
        .find(|a| a.name == "format")
        .context("format control missing")?;
    output["default_format"] = serde_json::to_value(&format.default)?;
    output["formats"] = serde_json::to_value(&format.possible_values)?;
    let mut commands = vec![];
    for &example in examples {
        let parsed = Cli::try_parse_from(example.split_whitespace())
            .with_context(|| format!("invalid guide example: {example}"))?;
        let matches = Cli::command().try_get_matches_from(example.split_whitespace())?;
        let name = matches
            .subcommand_name()
            .context("example has no command")?;
        let subcommand = root
            .find_subcommand_mut(name)
            .context("operational command absent from clap")?;
        commands.push(CommandGuide {
            name: name.into(),
            syntax: syntax(subcommand),
            operational: operational(&parsed.command, example),
        });
    }
    let expected: std::collections::BTreeSet<_> = root
        .get_subcommands()
        .filter(|s| !s.is_hide_set() && s.get_name() != "help")
        .map(|s| s.get_name().to_owned())
        .collect();
    let actual: std::collections::BTreeSet<_> = commands.iter().map(|c| c.name.clone()).collect();
    ensure!(
        actual == expected && actual.len() == commands.len(),
        "guide operational records must cover every visible command exactly once"
    );
    Ok(Guide {
        schema_version: "drft-guide/1",
        drft_version: env!("CARGO_PKG_VERSION"),
        schema_policy: "Unknown schema versions require a hard consumer stop. The binary version is informational and does not substitute for the schema version.",
        workflow: Workflow {
            steps: vec![
                Step {
                    phase: "before-edit",
                    command: Some("drft impact <path>... --format json"),
                    action: "Review direct dependents before editing.",
                    widen_with: vec!["--depth <positive-integer>", "--depth all"],
                },
                Step {
                    phase: "edit",
                    command: None,
                    action: "Edit only the intended files.",
                    widen_with: vec![],
                },
                Step {
                    phase: "check",
                    command: Some("drft check --format json"),
                    action: "Read every finding. Warnings may exit successfully; do not ignore them or suppress a rule.",
                    widen_with: vec![],
                },
                Step {
                    phase: "review",
                    command: None,
                    action: "Review each changed file and every dependent whose current outbound promise the lock will affirm.",
                    widen_with: vec![],
                },
                Step {
                    phase: "record",
                    command: Some("drft lock <reviewed-path>... --format json"),
                    action: "Record every reviewed path, including unchanged dependents whose outbound promises were reviewed; the scope is not merely edited files.",
                    widen_with: vec![],
                },
            ],
            whole_graph_lock: "Use --all only after reviewing every lockable node, or deliberately establishing or rebuilding the whole baseline.",
            preexisting_staleness: "Do not lock unrelated pre-existing staleness.",
            empty_scope: "An empty scoped lock never widens.",
            baseline: "A missing or unusable baseline requires review; it does not grant authority to regenerate it.",
        },
        exit_codes: [
            ExitStatus::Success,
            ExitStatus::Violations,
            ExitStatus::Failure,
        ]
        .into_iter()
        .map(|status| ExitCode {
            code: status.code(),
            meaning: status.meaning(),
        })
        .collect(),
        output,
        global_controls,
        commands,
    })
}

/// Render all contract properties as labeled nested records. Scalar values retain
/// JSON quoting so null, strings, and empty collections remain distinguishable.
pub fn render_text(guide: &Guide) -> Result<String> {
    fn render(value: &Value, indent: usize, output: &mut String) -> Result<()> {
        match value {
            Value::Object(fields) if !fields.is_empty() => {
                // Lead with identity and workflow, keeping syntax beside each
                // command. Unlisted fields still render, so additions cannot vanish.
                const FIRST: &[&str] = &[
                    "name",
                    "role",
                    "capability",
                    "example",
                    "phase",
                    "command",
                    "action",
                    "steps",
                    "summary",
                    "usage",
                    "schema_version",
                    "drft_version",
                    "schema_policy",
                    "workflow",
                    "exit_codes",
                    "output",
                    "global_controls",
                    "commands",
                ];
                let ordered = FIRST
                    .iter()
                    .filter_map(|key| fields.get_key_value(*key))
                    .chain(
                        fields
                            .iter()
                            .filter(|(key, _)| !FIRST.contains(&key.as_str())),
                    );
                for (key, value) in ordered {
                    output.push_str(&" ".repeat(indent));
                    output.push_str(key);
                    output.push(':');
                    if matches!(value, Value::Object(v) if !v.is_empty())
                        || matches!(value, Value::Array(v) if !v.is_empty())
                    {
                        output.push('\n');
                        render(value, indent + 2, output)?;
                    } else {
                        output.push(' ');
                        output.push_str(&serde_json::to_string(value)?);
                        output.push('\n');
                    }
                }
            }
            Value::Array(items) if !items.is_empty() => {
                for (index, value) in items.iter().enumerate() {
                    output.push_str(&" ".repeat(indent));
                    output.push_str(&format!("[{index}]:"));
                    if value.is_object() || value.is_array() {
                        output.push('\n');
                        render(value, indent + 2, output)?;
                    } else {
                        output.push(' ');
                        output.push_str(&serde_json::to_string(value)?);
                        output.push('\n');
                    }
                }
            }
            _ => {
                output.push_str(&" ".repeat(indent));
                output.push_str(&serde_json::to_string(value)?);
                output.push('\n');
            }
        }
        Ok(())
    }
    let mut text = "drft operational guide\n".to_owned();
    render(&serde_json::to_value(guide)?, 0, &mut text)?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn json_guide(command: Command) -> Value {
        serde_json::to_value(document_from(command, EXAMPLES).unwrap()).unwrap()
    }
    fn command<'a>(guide: &'a Value, name: &str) -> &'a Value {
        guide["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap()
    }
    fn item<'a>(guide: &'a Value, command_name: &str, group: &str, id: &str) -> &'a Value {
        command(guide, command_name)["syntax"][group]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == id)
            .unwrap()
    }

    #[test]
    fn examples_parse_and_records_cover_visible_commands_exactly_once() {
        let guide = document().unwrap();
        assert_eq!(guide.commands.len(), EXAMPLES.len());
        for example in EXAMPLES {
            Cli::try_parse_from(example.split_whitespace()).unwrap();
        }
        assert!(document_from(Cli::command(), &EXAMPLES[..EXAMPLES.len() - 1]).is_err());
        let mut duplicate = EXAMPLES.to_vec();
        duplicate.push(EXAMPLES[0]);
        assert!(document_from(Cli::command(), &duplicate).is_err());
        assert!(
            document_from(Cli::command().subcommand(Command::new("future")), EXAMPLES).is_err()
        );
        assert!(document_from(Cli::command(), &["drft guide --unknown"]).is_err());
    }

    #[test]
    fn syntax_has_closed_shapes_and_expected_population() {
        let guide = json_guide(Cli::command());
        let syntax_keys = [
            "arguments",
            "constraints",
            "controls",
            "inherits_global_controls",
            "summary",
            "usage",
        ];
        let item_keys = [
            "default",
            "global",
            "help",
            "kind",
            "long",
            "name",
            "possible_values",
            "repeatable",
            "required",
            "short",
            "value",
        ];
        let names = |value: &Value| {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v["name"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(&guide["global_controls"]),
            ["directory", "format", "color"]
        );
        for c in guide["commands"].as_array().unwrap() {
            assert_eq!(
                c["syntax"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                syntax_keys
            );
            assert_eq!(c["syntax"]["inherits_global_controls"], true);
            for group in ["arguments", "controls"] {
                for arg in c["syntax"][group].as_array().unwrap() {
                    assert_eq!(
                        arg.as_object()
                            .unwrap()
                            .keys()
                            .map(String::as_str)
                            .collect::<Vec<_>>(),
                        item_keys
                    );
                }
            }
        }
        for (name, args, controls) in [
            ("init", vec![], vec![]),
            ("config", vec![], vec!["show_ignores"]),
            ("lock", vec!["paths"], vec!["all"]),
            ("graph", vec![], vec!["raw", "max_bytes"]),
            (
                "impact",
                vec!["paths"],
                vec!["depth", "direction", "max_bytes"],
            ),
            (
                "nodes",
                vec!["selectors"],
                vec!["all", "namespaces", "fields", "max_bytes"],
            ),
            (
                "edges",
                vec!["selectors"],
                vec!["all", "namespaces", "fields", "max_bytes"],
            ),
            ("check", vec![], vec![]),
            ("guide", vec![], vec![]),
        ] {
            let syntax = &command(&guide, name)["syntax"];
            assert_eq!(names(&syntax["arguments"]), args, "{name} arguments");
            assert_eq!(names(&syntax["controls"]), controls, "{name} controls");
        }
        assert_eq!(
            item(&guide, "config", "controls", "show_ignores")["required"],
            true
        );
        assert_eq!(item(&guide, "graph", "controls", "raw")["default"], "false");
        assert_eq!(
            item(&guide, "graph", "controls", "raw")["possible_values"],
            json!([])
        );
        assert_eq!(
            item(&guide, "impact", "controls", "depth")["possible_values"],
            json!([])
        );
        assert_eq!(
            item(&guide, "impact", "arguments", "paths")["required"],
            true
        );
        assert_eq!(
            command(&guide, "impact")["syntax"]["constraints"],
            json!([])
        );
        assert_eq!(
            command(&guide, "config")["syntax"]["constraints"],
            json!([
                {"kind":"requires_one_of","subject":"command","related":["show_ignores"]}
            ])
        );
        for (name, operand) in [
            ("lock", "paths"),
            ("nodes", "selectors"),
            ("edges", "selectors"),
        ] {
            assert_eq!(
                command(&guide, name)["syntax"]["constraints"],
                json!([
                    {"kind":"requires_one_of","subject":"command","related":[operand,"all"]},
                    {"kind":"conflicts_with","subject":operand,"related":["all"]}
                ])
            );
        }
    }

    #[test]
    fn clap_mutations_flow_into_guide_without_metadata_edits() {
        let mutated = Cli::command()
            .mut_arg("format", |a| a.default_value("json"))
            .mut_subcommand("impact", |c| {
                c.mut_arg("depth", |a| a.default_value("7"))
                    .mut_arg("direction", |a| {
                        a.value_parser(["inbound", "outbound", "both", "future"])
                    })
                    .mut_arg("paths", |a| a.required(false))
                    .mut_arg("max_bytes", |a| a.long("budget"))
            })
            .mut_subcommand("graph", |c| {
                c.mut_arg("raw", |a| a.conflicts_with("max_bytes"))
            })
            .mut_subcommand("lock", |c| c.mut_group("scope", |g| g.required(false)));
        let guide = json_guide(mutated);
        assert_eq!(guide["output"]["default_format"], "json");
        assert_eq!(item(&guide, "impact", "controls", "depth")["default"], "7");
        assert_eq!(
            item(&guide, "impact", "controls", "direction")["possible_values"],
            json!(["inbound", "outbound", "both", "future"])
        );
        assert_eq!(
            item(&guide, "impact", "arguments", "paths")["required"],
            false
        );
        assert_eq!(
            item(&guide, "impact", "controls", "max_bytes")["long"],
            "budget"
        );
        assert!(
            command(&guide, "graph")["syntax"]["constraints"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v
                    == &json!({"kind":"conflicts_with","subject":"raw","related":["max_bytes"]}))
        );
        assert!(
            command(&guide, "lock")["syntax"]["constraints"]
                .as_array()
                .unwrap()
                .iter()
                .all(|v| v["kind"] != "requires_one_of")
        );
    }

    // Deliberately independent of render_text: parse indentation, record labels,
    // array indices, and scalar JSON tokens back into the semantic document.
    fn normalize_text(text: &str) -> Value {
        fn block(lines: &[&str], cursor: &mut usize, depth: usize) -> Value {
            let array = lines[*cursor].trim_start().starts_with('[');
            let mut object = serde_json::Map::new();
            let mut items = vec![];
            while *cursor < lines.len() {
                let line = lines[*cursor];
                let indent = line.len() - line.trim_start().len();
                if indent < depth {
                    break;
                }
                assert_eq!(indent, depth);
                let (label, token) = line.trim_start().split_once(':').unwrap();
                *cursor += 1;
                let value = if token.is_empty() {
                    block(lines, cursor, depth + 2)
                } else {
                    serde_json::from_str(token.trim()).unwrap()
                };
                if array {
                    assert_eq!(label, format!("[{}]", items.len()));
                    items.push(value);
                } else {
                    assert!(object.insert(label.into(), value).is_none());
                }
            }
            if array {
                Value::Array(items)
            } else {
                Value::Object(object)
            }
        }
        let lines = text.lines().collect::<Vec<_>>();
        assert_eq!(lines[0], "drft operational guide");
        block(&lines, &mut 1, 0)
    }

    #[test]
    fn text_and_json_normalize_to_every_contract_property() {
        let guide = document().unwrap();
        let json = serde_json::to_value(&guide).unwrap();
        let text = render_text(&guide).unwrap();
        assert!(text.find("workflow:").unwrap() < text.find("commands:").unwrap());
        assert_eq!(normalize_text(&text), json);
        // The normalization assertion catches omitted properties, not just a
        // snapshot changing in lockstep with a renderer.
        let missing = text
            .lines()
            .filter(|line| !line.trim_start().starts_with("schema_policy:"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_ne!(normalize_text(&missing), json);
    }
}
