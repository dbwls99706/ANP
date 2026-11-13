use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use anp_core::graph::editor::GraphEditor;
use anp_core::graph::spec::{AnpGraph, NodeKind};
use anp_core::graph::{loader, risk, validate};
use anp_core::node::registry::NodeRegistry;
use anp_core::runtime::engine::AnpEngine;
use anp_planner::generator::AnpPlanner;
use anp_planner::llm::auto_detect_client;

#[derive(Parser)]
#[command(
    name = "anp",
    about = "ANP — AI-native runtime where software is defined as executable graphs",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Execute an ANP graph
    Run {
        /// Path to the .anp.json graph file
        graph: PathBuf,
    },
    /// Validate an ANP graph without executing
    Validate {
        /// Path to the .anp.json graph file
        graph: PathBuf,
    },
    /// Display information about an ANP graph
    Info {
        /// Path to the .anp.json graph file
        graph: PathBuf,
    },
    /// Generate a human-readable explanation of the graph
    Explain {
        /// Path to the .anp.json graph file
        graph: PathBuf,
    },
    /// Output the graph in DOT format for visualization
    Graph {
        /// Path to the .anp.json graph file
        graph: PathBuf,
    },
    /// Initialize a new ANP project
    Init {
        /// Project name
        name: String,
    },
    /// Generate an ANP graph from a natural language prompt using an LLM
    Prompt {
        /// The prompt describing the desired application
        prompt: String,
        /// Save graph to file instead of printing to stdout
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Generate and immediately run the graph
        #[arg(long)]
        run: bool,
        /// Show a human-readable explanation of the generated graph
        #[arg(long)]
        explain: bool,
    },
    /// Edit an existing ANP graph using a natural language instruction
    Edit {
        /// Path to the .anp.json graph file to edit
        graph: PathBuf,
        /// Natural language instruction for how to modify the graph
        instruction: String,
        /// Show the generated operations without applying them
        #[arg(long)]
        dry_run: bool,
    },
    /// Formally verify an ANP graph — prove correctness
    Verify {
        /// Path to the .anp.json graph file
        graph: PathBuf,
        /// Risk certificate to check the decision thresholds against
        #[arg(long, requires = "epsilon")]
        risk: Option<PathBuf>,
        /// Largest acceptable rate of wrong, unescalated runs (with --risk)
        #[arg(long, requires = "risk")]
        epsilon: Option<f64>,
    },
    /// Certify the error rate of a decision workflow and tune its thresholds
    ///
    /// Issue a certificate:   anp certify g.json --traces runs.jsonl --out cert.json
    /// Tune thresholds:       anp certify g.json --cert cert.json --epsilon 0.05 --apply g.json
    /// After editing a node:  anp certify g.json --cert cert.json --fresh new.jsonl --edited node --out cert.json
    #[command(verbatim_doc_comment)]
    Certify {
        /// Path to the .anp.json graph file
        graph: PathBuf,
        /// Labelled calibration runs (JSON lines) sampled from all traffic
        #[arg(long, conflicts_with_all = ["cert", "fresh"])]
        traces: Option<PathBuf>,
        /// Existing certificate to tune thresholds from, or to update after an edit
        #[arg(long)]
        cert: Option<PathBuf>,
        /// Labelled runs of the edited workflow, sampled from traffic through the edited nodes
        #[arg(long, requires_all = ["cert", "edited"])]
        fresh: Option<PathBuf>,
        /// Decision nodes that were edited (comma separated)
        #[arg(long, value_delimiter = ',', requires = "fresh")]
        edited: Vec<String>,
        /// Target error rate; picks the thresholds with the least human review
        #[arg(long)]
        epsilon: Option<f64>,
        /// Probability that the certificate is wrong (spent on the evidence being added)
        #[arg(long, default_value_t = 0.1)]
        delta: f64,
        /// Which node an erroneous run is charged to: min or first_error
        #[arg(long, default_value = "min")]
        rule: String,
        /// Nodes sharing a threshold: level, node or global
        #[arg(long, default_value = "level")]
        modules: String,
        /// Write the certificate to this file
        #[arg(long)]
        out: Option<PathBuf>,
        /// Write the graph with the chosen thresholds to this file (requires --epsilon)
        #[arg(long, requires = "epsilon")]
        apply: Option<PathBuf>,
    },
    /// Show semantic differences between two graphs
    Diff {
        /// Path to the old graph
        old_graph: PathBuf,
        /// Path to the new graph
        new_graph: PathBuf,
    },
    /// Optimize an ANP graph with compiler-like passes
    Optimize {
        /// Path to the .anp.json graph file
        graph: PathBuf,
        /// Save optimized graph to file
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Compile a graph to optimized binary format (.anp.compiled)
    Compile {
        /// Path to the .anp.json graph file
        graph: PathBuf,
        /// Output path for compiled graph
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Optimization level: none, basic, full
        #[arg(long, default_value = "full")]
        opt: String,
    },
    /// Benchmark a graph — measure complexity and predict performance
    Bench {
        /// Path to the .anp.json graph file
        graph: PathBuf,
    },
    /// Type-check a graph — verify port type compatibility
    TypeCheck {
        /// Path to the .anp.json graph file
        graph: PathBuf,
    },
}

/// Unified error type for CLI operations.
#[derive(Debug)]
enum CliError {
    /// Generic error with message
    Message(String),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Message(msg) => write!(f, "{}", msg),
        }
    }
}

type CliResult = Result<(), CliError>;

fn create_registry() -> NodeRegistry {
    let mut registry = NodeRegistry::new();
    registry.register_plugin(&anp_nodes::BuiltinPlugin);
    registry
}

fn require_llm_client() -> Result<Box<dyn anp_planner::llm::LlmClient>, CliError> {
    auto_detect_client().map_err(|_| {
        CliError::Message(
            "No LLM API key found.\n\
             Set ANTHROPIC_API_KEY or OPENAI_API_KEY environment variable.\n\n\
             Example:\n  export ANTHROPIC_API_KEY=sk-ant-..."
                .to_string(),
        )
    })
}

fn load_graph(path: &std::path::Path) -> Result<AnpGraph, CliError> {
    loader::load_from_file(path)
        .map_err(|e| CliError::Message(format!("Error loading graph: {}", e)))
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Run { graph: path } => cmd_run(path).await,
        Commands::Validate { graph: path } => cmd_validate(path),
        Commands::Info { graph: path } => cmd_info(path),
        Commands::Explain { graph: path } => cmd_explain(path),
        Commands::Graph { graph: path } => cmd_graph(path),
        Commands::Init { name } => cmd_init(name),
        Commands::Prompt {
            prompt,
            output,
            run,
            explain,
        } => cmd_prompt(prompt, output, run, explain).await,
        Commands::Edit {
            graph,
            instruction,
            dry_run,
        } => cmd_edit(graph, instruction, dry_run).await,
        Commands::Verify {
            graph: path,
            risk,
            epsilon,
        } => cmd_verify(path, risk.zip(epsilon)),
        Commands::Certify {
            graph,
            traces,
            cert,
            fresh,
            edited,
            epsilon,
            delta,
            rule,
            modules,
            out,
            apply,
        } => cmd_certify(CertifyArgs {
            graph,
            traces,
            cert,
            fresh,
            edited,
            epsilon,
            delta,
            rule,
            modules,
            out,
            apply,
        }),
        Commands::Diff {
            old_graph,
            new_graph,
        } => cmd_diff(old_graph, new_graph),
        Commands::Optimize {
            graph: path,
            output,
        } => cmd_optimize(path, output),
        Commands::Compile {
            graph: path,
            output,
            opt,
        } => cmd_compile(path, output, opt),
        Commands::Bench { graph: path } => cmd_bench(path),
        Commands::TypeCheck { graph: path } => cmd_type_check(path),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {}", e);
            ExitCode::FAILURE
        }
    }
}

async fn cmd_run(path: PathBuf) -> CliResult {
    let registry = create_registry();
    let engine = AnpEngine::new(registry);
    engine
        .run_file(&path)
        .await
        .map_err(|e| CliError::Message(e.to_string()))
}

async fn cmd_prompt(
    prompt: String,
    output: Option<PathBuf>,
    run: bool,
    explain: bool,
) -> CliResult {
    let client = require_llm_client()?;
    let registry = create_registry();
    let planner = AnpPlanner::new(client, &registry);

    let graph = planner
        .generate(&prompt)
        .await
        .map_err(|e| CliError::Message(e.to_string()))?;

    let json = serde_json::to_string_pretty(&graph)
        .map_err(|e| CliError::Message(format!("Error serializing graph: {}", e)))?;

    match &output {
        Some(path) => {
            std::fs::write(path, &json)
                .map_err(|e| CliError::Message(format!("Error writing file: {}", e)))?;
            println!("Graph saved to {}", path.display());
        }
        None => {
            println!("{}", json);
        }
    }

    if explain {
        println!();
        explain_graph(&graph);
    }

    if run {
        println!();
        let engine = AnpEngine::new(registry);
        engine
            .run(&graph)
            .await
            .map_err(|e| CliError::Message(format!("Runtime error: {}", e)))?;
    }

    Ok(())
}

async fn cmd_edit(path: PathBuf, instruction: String, dry_run: bool) -> CliResult {
    let mut graph = load_graph(&path)?;
    let client = require_llm_client()?;
    let registry = create_registry();
    let planner = AnpPlanner::new(client, &registry);

    let ops = planner
        .generate_edit(&instruction, &graph)
        .await
        .map_err(|e| CliError::Message(e.to_string()))?;

    if ops.is_empty() {
        println!("No changes needed.");
        return Ok(());
    }

    println!("Operations ({}):", ops.len());
    for op in &ops {
        println!("  {}", op);
    }

    if dry_run {
        return Ok(());
    }

    {
        let mut editor = GraphEditor::new(&mut graph, &registry);
        editor
            .apply_all(ops)
            .map_err(|e| CliError::Message(format!("Error applying operations: {}", e)))?;
    }

    if let Err(errors) = validate::validate(&graph) {
        eprintln!("Error: edited graph has validation errors:");
        for err in &errors {
            eprintln!("  - {}", err);
        }
        return Err(CliError::Message(
            "graph not saved due to validation errors (use --dry-run to see operations without applying)".into(),
        ));
    }

    let json = serde_json::to_string_pretty(&graph)
        .map_err(|e| CliError::Message(format!("Error serializing graph: {}", e)))?;

    std::fs::write(&path, &json)
        .map_err(|e| CliError::Message(format!("Error writing file: {}", e)))?;

    println!();
    println!("Graph saved to {}", path.display());
    Ok(())
}

fn explain_graph(graph: &AnpGraph) {
    println!("This graph defines: {}", graph.metadata.name);
    if !graph.metadata.description.is_empty() {
        println!("{}", graph.metadata.description);
    }
    println!();

    for node in &graph.nodes {
        let kind_label = match node.kind {
            NodeKind::Service => "service",
            NodeKind::Source => "source",
            NodeKind::Processor => "process",
            NodeKind::Sink => "output",
        };
        println!("  [{}] {}", kind_label, describe_node(node));
    }

    if !graph.edges.is_empty() {
        println!();
        println!("Data flow:");
        for edge in &graph.edges {
            let from_port = if edge.from_port == "default" {
                String::new()
            } else {
                format!(".{}", edge.from_port)
            };
            let to_port = if edge.to_port == "default" {
                String::new()
            } else {
                format!(".{}", edge.to_port)
            };
            println!(
                "  {}{} -> {}{}",
                edge.from_node, from_port, edge.to_node, to_port
            );
        }
    }
}

fn cmd_validate(path: PathBuf) -> CliResult {
    let graph = load_graph(&path)?;
    match validate::validate(&graph) {
        Ok(()) => {
            println!("Graph '{}' is valid.", graph.metadata.name);
            Ok(())
        }
        Err(errors) => {
            let mut msg = "Validation errors:\n".to_string();
            for err in &errors {
                msg.push_str(&format!("  - {}\n", err));
            }
            Err(CliError::Message(msg))
        }
    }
}

fn cmd_info(path: PathBuf) -> CliResult {
    let graph = load_graph(&path)?;

    println!("ANP Graph: {}", graph.metadata.name);
    if !graph.metadata.description.is_empty() {
        println!("Description: {}", graph.metadata.description);
    }
    println!("Version: {}", graph.version);
    println!("Nodes: {}", graph.nodes.len());
    println!("Edges: {}", graph.edges.len());
    println!();

    println!("Nodes:");
    for node in &graph.nodes {
        println!(
            "  [{:>9}] {} (type: {})",
            node.kind, node.id, node.node_type
        );
    }

    if !graph.edges.is_empty() {
        println!();
        println!("Edges:");
        for edge in &graph.edges {
            println!(
                "  {} {}.{} -> {}.{}",
                edge.id, edge.from_node, edge.from_port, edge.to_node, edge.to_port
            );
        }
    }

    Ok(())
}

fn cmd_explain(path: PathBuf) -> CliResult {
    let graph = load_graph(&path)?;

    println!("This graph defines: {}", graph.metadata.name);
    if !graph.metadata.description.is_empty() {
        println!("{}", graph.metadata.description);
    }
    println!();

    let services: Vec<_> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Service)
        .collect();
    let sources: Vec<_> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Source)
        .collect();
    let processors: Vec<_> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Processor)
        .collect();
    let sinks: Vec<_> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Sink)
        .collect();

    for node in &services {
        println!(
            "  [service] {} starts as a long-running service",
            describe_node(node)
        );
    }
    for node in &sources {
        println!("  [source] {} produces data", describe_node(node));
    }
    for node in &processors {
        println!("  [process] {} transforms data", describe_node(node));
    }
    for node in &sinks {
        println!("  [output] {} consumes data", describe_node(node));
    }

    if !graph.edges.is_empty() {
        println!();
        println!("Data flow:");
        for edge in &graph.edges {
            let from_port = if edge.from_port == "default" {
                String::new()
            } else {
                format!(".{}", edge.from_port)
            };
            let to_port = if edge.to_port == "default" {
                String::new()
            } else {
                format!(".{}", edge.to_port)
            };
            println!(
                "  {}{} -> {}{}",
                edge.from_node, from_port, edge.to_node, to_port
            );
        }
    }

    Ok(())
}

fn describe_node(node: &anp_core::graph::spec::Node) -> String {
    match node.node_type.as_str() {
        "http_server" => {
            let port = node
                .config
                .get("port")
                .and_then(|v| v.as_u64())
                .unwrap_or(3000);
            format!("HTTP server (port {})", port)
        }
        "database" => "In-memory database".to_string(),
        "console" => "Console output".to_string(),
        "file_read" => {
            let path = node
                .config
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("File reader ({})", path)
        }
        "file_write" => {
            let path = node
                .config
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("File writer ({})", path)
        }
        "transform" => "Data transformer".to_string(),
        "filter" => {
            let field = node
                .config
                .get("field")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let op = node
                .config
                .get("op")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("Filter ({} {})", field, op)
        }
        "llm_call" => {
            let model = node
                .config
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("default");
            format!("LLM call ({})", model)
        }
        "http_client" => {
            let method = node
                .config
                .get("method")
                .and_then(|v| v.as_str())
                .unwrap_or("GET");
            let url = node
                .config
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("HTTP client ({} {})", method, url)
        }
        "template" => "Template renderer".to_string(),
        "conditional" => "Conditional router".to_string(),
        "memory" => "Session memory".to_string(),
        "tool_router" => "AI tool router (agentic)".to_string(),
        "decision" => {
            let question = node
                .config
                .get("question")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            format!("Decision ({})", question)
        }
        "html_template" => "HTML template renderer".to_string(),
        "static_file" => "Static file server".to_string(),
        "foreach" => "ForEach iterator".to_string(),
        "merge" => "Data merger".to_string(),
        other => format!("'{}' node", other),
    }
}

fn cmd_init(name: String) -> CliResult {
    let dir = PathBuf::from(&name);
    if dir.exists() {
        return Err(CliError::Message(format!(
            "directory '{}' already exists",
            name
        )));
    }

    std::fs::create_dir_all(&dir)
        .map_err(|e| CliError::Message(format!("Error creating directory: {}", e)))?;

    let graph_content = format!(
        r#"{{
  "version": "0.1",
  "metadata": {{
    "name": "{}",
    "description": "A new ANP application"
  }},
  "nodes": [
    {{
      "id": "source",
      "node_type": "file_read",
      "kind": "source",
      "config": {{ "path": "data.txt" }},
      "inputs": [],
      "outputs": [{{ "name": "default" }}]
    }},
    {{
      "id": "output",
      "node_type": "console",
      "kind": "sink",
      "config": {{}},
      "inputs": [{{ "name": "default" }}],
      "outputs": []
    }}
  ],
  "edges": [
    {{ "id": "e1", "from_node": "source", "to_node": "output" }}
  ]
}}"#,
        name
    );

    let graph_path = dir.join("main.anp.json");
    std::fs::write(&graph_path, graph_content)
        .map_err(|e| CliError::Message(format!("Error writing graph file: {}", e)))?;

    let data_path = dir.join("data.txt");
    std::fs::write(&data_path, format!("Hello from {}!\n", name))
        .map_err(|e| CliError::Message(format!("Error writing data file: {}", e)))?;

    println!("Created ANP project '{}'", name);
    println!();
    println!("  cd {}", name);
    println!("  anp run main.anp.json");
    Ok(())
}

fn cmd_verify(path: PathBuf, risk: Option<(PathBuf, f64)>) -> CliResult {
    let graph = load_graph(&path)?;
    let report = anp_core::graph::verify::verify(&graph);
    println!("{}", report);
    let mut pass = report.overall_pass;
    if let Some((cert_path, epsilon)) = risk {
        if !(epsilon > 0.0 && epsilon < 1.0) {
            return Err(CliError::Message(
                "--epsilon must be between 0 and 1".into(),
            ));
        }
        let check = load_certificate(&cert_path)?.check(&graph, epsilon);
        let mark = if check.pass {
            "\u{2713} PASS"
        } else {
            "\u{2717} FAIL"
        };
        println!("{} Risk: {}", mark, check);
        pass &= check.pass;
    }
    if pass {
        Ok(())
    } else {
        Err(CliError::Message("verification failed".to_string()))
    }
}

struct CertifyArgs {
    graph: PathBuf,
    traces: Option<PathBuf>,
    cert: Option<PathBuf>,
    fresh: Option<PathBuf>,
    edited: Vec<String>,
    epsilon: Option<f64>,
    delta: f64,
    rule: String,
    modules: String,
    out: Option<PathBuf>,
    apply: Option<PathBuf>,
}

fn load_certificate(path: &std::path::Path) -> Result<risk::RiskCertificate, CliError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| CliError::Message(format!("Error reading {}: {}", path.display(), e)))?;
    serde_json::from_str(&text)
        .map_err(|e| CliError::Message(format!("{} is not a certificate: {}", path.display(), e)))
}

fn load_traces(path: &std::path::Path) -> Result<Vec<risk::RunTrace>, CliError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| CliError::Message(format!("Error reading {}: {}", path.display(), e)))?;
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(i, line)| {
            serde_json::from_str(line)
                .map_err(|e| CliError::Message(format!("{} line {}: {}", path.display(), i + 1, e)))
        })
        .collect()
}

fn write_json(path: &std::path::Path, json: Result<String, serde_json::Error>) -> CliResult {
    let json = json.map_err(|e| CliError::Message(format!("Error serializing: {}", e)))?;
    std::fs::write(path, json)
        .map_err(|e| CliError::Message(format!("Error writing {}: {}", path.display(), e)))
}

fn cmd_certify(args: CertifyArgs) -> CliResult {
    let mut graph = load_graph(&args.graph)?;
    let fail = |e: risk::RiskError| CliError::Message(e.to_string());

    let cert = if let Some(traces_path) = &args.traces {
        let rule = match args.rule.as_str() {
            "min" => risk::Rule::Min,
            "first_error" => risk::Rule::FirstError,
            other => {
                return Err(CliError::Message(format!(
                    "unknown --rule '{}'. Use: min, first_error",
                    other
                )))
            }
        };
        let granularity = match args.modules.as_str() {
            "level" => risk::Granularity::Level,
            "node" => risk::Granularity::Node,
            "global" => risk::Granularity::Global,
            other => {
                return Err(CliError::Message(format!(
                    "unknown --modules '{}'. Use: level, node, global",
                    other
                )))
            }
        };
        let traces = load_traces(traces_path)?;
        let cert = risk::certify(&graph, &traces, rule, granularity, args.delta).map_err(fail)?;
        println!(
            "Certified {} decision node(s) in {} module(s) from {} runs.",
            cert.fingerprints.len(),
            cert.modules.len(),
            traces.len()
        );
        cert
    } else if let Some(cert_path) = &args.cert {
        let old = load_certificate(cert_path)?;
        match &args.fresh {
            Some(fresh_path) => {
                let fresh = load_traces(fresh_path)?;
                let cert = old
                    .recertify(&graph, &fresh, &args.edited, args.delta)
                    .map_err(fail)?;
                let kept: u64 = cert.parts[0]
                    .routes
                    .iter()
                    .filter(|r| !r.stale)
                    .map(|r| r.runs)
                    .sum();
                println!(
                    "Recertified after editing {}: kept {} earlier runs, added {} fresh runs.",
                    args.edited.join(", "),
                    kept,
                    fresh.len()
                );
                cert
            }
            None => old,
        }
    } else {
        return Err(CliError::Message(
            "nothing to do: pass --traces to issue a certificate or --cert to use one".into(),
        ));
    };

    let stale = cert.stale_nodes(&graph);
    if !stale.is_empty() {
        return Err(CliError::Message(format!(
            "the certificate does not match the graph for: {}\n\
             Label fresh runs through these nodes and pass --fresh <runs> --edited {}",
            stale.join(", "),
            stale.join(",")
        )));
    }

    if let Some(out) = &args.out {
        write_json(out, serde_json::to_string(&cert))?;
        println!("Certificate written to {}", out.display());
    }

    if let Some(epsilon) = args.epsilon {
        let selection = cert.optimize(epsilon).map_err(fail)?;
        println!("Thresholds for a target error rate of {}:", epsilon);
        println!("{}", cert.describe(&selection));
        if let Some(apply) = &args.apply {
            cert.apply(&mut graph, &selection);
            write_json(apply, serde_json::to_string_pretty(&graph))?;
            println!("Graph with these thresholds written to {}", apply.display());
        }
    } else if args.out.is_none() {
        println!("Pass --out to save the certificate or --epsilon to pick thresholds.");
    }
    Ok(())
}

fn cmd_diff(old_path: PathBuf, new_path: PathBuf) -> CliResult {
    let old_graph = load_graph(&old_path)?;
    let new_graph = load_graph(&new_path)?;
    let diff = anp_core::graph::diff::diff(&old_graph, &new_graph);
    if diff.summary.is_identical {
        println!("Graphs are identical.");
    } else {
        println!("{}", diff);
    }
    Ok(())
}

fn cmd_optimize(path: PathBuf, output: Option<PathBuf>) -> CliResult {
    let graph = load_graph(&path)?;
    let result = anp_core::graph::optimize::optimize(&graph);
    println!("{}", result);

    if let Some(out_path) = output {
        let json = serde_json::to_string_pretty(&result.graph)
            .map_err(|e| CliError::Message(format!("Error serializing graph: {}", e)))?;
        std::fs::write(&out_path, &json)
            .map_err(|e| CliError::Message(format!("Error writing file: {}", e)))?;
        println!("Optimized graph saved to {}", out_path.display());
    }
    Ok(())
}

fn cmd_compile(path: PathBuf, output: Option<PathBuf>, opt: String) -> CliResult {
    let graph = load_graph(&path)?;

    let opt_level = match opt.as_str() {
        "none" => anp_core::graph::compile::OptLevel::None,
        "basic" => anp_core::graph::compile::OptLevel::Basic,
        _ => anp_core::graph::compile::OptLevel::Full,
    };

    let compiled =
        anp_core::graph::compile::compile(&graph, Some(&path.display().to_string()), opt_level)
            .map_err(|e| CliError::Message(format!("Compilation failed: {}", e)))?;

    println!("{}", compiled);

    let out_path = output.unwrap_or_else(|| {
        let mut p = path.clone();
        p.set_extension("compiled");
        p
    });

    anp_core::graph::compile::save_to_file(&compiled, &out_path)
        .map_err(|e| CliError::Message(format!("Error saving compiled graph: {}", e)))?;

    println!("\nSaved to {}", out_path.display());
    Ok(())
}

fn cmd_bench(path: PathBuf) -> CliResult {
    let graph = load_graph(&path)?;
    let report = anp_core::graph::bench::benchmark(&graph);
    println!("{}", report);
    Ok(())
}

fn cmd_type_check(path: PathBuf) -> CliResult {
    let graph = load_graph(&path)?;
    let result = anp_core::graph::types::type_check(&graph);
    println!("{}", result);
    if result.is_sound {
        Ok(())
    } else {
        Err(CliError::Message("type check failed".to_string()))
    }
}

fn cmd_graph(path: PathBuf) -> CliResult {
    let graph = load_graph(&path)?;

    println!("digraph \"{}\" {{", graph.metadata.name);
    println!("  rankdir=LR;");
    println!("  node [shape=box, style=rounded];");
    println!();

    for node in &graph.nodes {
        let color = match node.kind {
            NodeKind::Source => "lightblue",
            NodeKind::Processor => "lightyellow",
            NodeKind::Sink => "lightgreen",
            NodeKind::Service => "lightsalmon",
        };
        println!(
            "  \"{}\" [label=\"{}\\n({})\", fillcolor={}, style=\"rounded,filled\"];",
            node.id, node.id, node.node_type, color
        );
    }

    println!();

    for edge in &graph.edges {
        let label = if edge.from_port == "default" && edge.to_port == "default" {
            String::new()
        } else {
            format!(" [label=\"{} -> {}\"]", edge.from_port, edge.to_port)
        };
        println!("  \"{}\" -> \"{}\"{};", edge.from_node, edge.to_node, label);
    }

    println!("}}");
    Ok(())
}
