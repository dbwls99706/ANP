use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use anp_core::graph::editor::GraphEditor;
use anp_core::graph::spec::{AnpGraph, NodeKind};
use anp_core::graph::{loader, validate};
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
}

fn create_registry() -> NodeRegistry {
    let mut registry = NodeRegistry::new();
    anp_nodes::register_all(&mut registry);
    registry
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let cli = Cli::parse();

    match cli.command {
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
    }
}

async fn cmd_run(path: PathBuf) {
    let registry = create_registry();
    let engine = AnpEngine::new(registry);

    match engine.run_file(&path).await {
        Ok(()) => {}
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

async fn cmd_prompt(prompt: String, output: Option<PathBuf>, run: bool, explain: bool) {
    // 1. Detect LLM client
    let client = match auto_detect_client() {
        Ok(c) => c,
        Err(_) => {
            eprintln!("No LLM API key found.");
            eprintln!("Set ANTHROPIC_API_KEY or OPENAI_API_KEY environment variable.");
            eprintln!();
            eprintln!("Example:");
            eprintln!("  export ANTHROPIC_API_KEY=sk-ant-...");
            std::process::exit(1);
        }
    };

    // 2. Build registry
    let registry = create_registry();

    // 3. Create planner
    let planner = AnpPlanner::new(client, &registry);

    // 4. Generate graph
    let graph = match planner.generate(&prompt).await {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    // 5. Output JSON
    let json = serde_json::to_string_pretty(&graph).unwrap_or_else(|e| {
        eprintln!("Error serializing graph: {}", e);
        std::process::exit(1);
    });

    match &output {
        Some(path) => {
            std::fs::write(path, &json).unwrap_or_else(|e| {
                eprintln!("Error writing file: {}", e);
                std::process::exit(1);
            });
            println!("Graph saved to {}", path.display());
        }
        None => {
            println!("{}", json);
        }
    }

    // 6. Explain
    if explain {
        println!();
        explain_graph(&graph);
    }

    // 7. Run
    if run {
        println!();
        let engine = AnpEngine::new(registry);
        match engine.run(&graph).await {
            Ok(()) => {}
            Err(e) => {
                eprintln!("Runtime error: {}", e);
                std::process::exit(1);
            }
        }
    }
}

async fn cmd_edit(path: PathBuf, instruction: String, dry_run: bool) {
    // 1. Load existing graph
    let mut graph = match loader::load_from_file(&path) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Error loading graph: {}", e);
            std::process::exit(1);
        }
    };

    // 2. Detect LLM client
    let client = match auto_detect_client() {
        Ok(c) => c,
        Err(_) => {
            eprintln!("No LLM API key found.");
            eprintln!("Set ANTHROPIC_API_KEY or OPENAI_API_KEY environment variable.");
            eprintln!();
            eprintln!("Example:");
            eprintln!("  export ANTHROPIC_API_KEY=sk-ant-...");
            std::process::exit(1);
        }
    };

    // 3. Generate edit operations
    let registry = create_registry();
    let planner = AnpPlanner::new(client, &registry);

    let ops = match planner.generate_edit(&instruction, &graph).await {
        Ok(ops) => ops,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    if ops.is_empty() {
        println!("No changes needed.");
        return;
    }

    // 4. Show operations
    println!("Operations ({}):", ops.len());
    for op in &ops {
        println!("  {}", serde_json::to_string(op).unwrap_or_default());
    }

    if dry_run {
        return;
    }

    // 5. Apply operations
    {
        let mut editor = GraphEditor::new(&mut graph, &registry);
        if let Err(e) = editor.apply_all(ops) {
            eprintln!("Error applying operations: {}", e);
            std::process::exit(1);
        }
    }

    // 6. Validate
    if let Err(errors) = validate::validate(&graph) {
        eprintln!("Warning: edited graph has validation errors:");
        for err in &errors {
            eprintln!("  - {}", err);
        }
    }

    // 7. Save
    let json = serde_json::to_string_pretty(&graph).unwrap_or_else(|e| {
        eprintln!("Error serializing graph: {}", e);
        std::process::exit(1);
    });

    std::fs::write(&path, &json).unwrap_or_else(|e| {
        eprintln!("Error writing file: {}", e);
        std::process::exit(1);
    });

    println!();
    println!("Graph saved to {}", path.display());
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

fn cmd_validate(path: PathBuf) {
    match loader::load_from_file(&path) {
        Ok(graph) => match validate::validate(&graph) {
            Ok(()) => {
                println!("Graph '{}' is valid.", graph.metadata.name);
            }
            Err(errors) => {
                eprintln!("Validation errors:");
                for err in errors {
                    eprintln!("  - {}", err);
                }
                std::process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("Error loading graph: {}", e);
            std::process::exit(1);
        }
    }
}

fn cmd_info(path: PathBuf) {
    let graph = match loader::load_from_file(&path) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    println!("ANP Graph: {}", graph.metadata.name);
    if !graph.metadata.description.is_empty() {
        println!("Description: {}", graph.metadata.description);
    }
    println!("Version: {}", graph.version);
    println!("Nodes: {}", graph.nodes.len());
    println!("Edges: {}", graph.edges.len());
    println!();

    // Node details
    println!("Nodes:");
    for node in &graph.nodes {
        println!(
            "  [{:>9}] {} (type: {})",
            node.kind.to_string(),
            node.id,
            node.node_type
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
}

fn cmd_explain(path: PathBuf) {
    let graph = match loader::load_from_file(&path) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    println!("This graph defines: {}", graph.metadata.name);
    if !graph.metadata.description.is_empty() {
        println!("{}", graph.metadata.description);
    }
    println!();

    // Group nodes by kind
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
        println!("  [service] {} starts as a long-running service", describe_node(node));
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

    // Show flow
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

fn describe_node(node: &anp_core::graph::spec::Node) -> String {
    match node.node_type.as_str() {
        "http_server" => {
            let port = node.config.get("port").and_then(|v| v.as_u64()).unwrap_or(3000);
            format!("HTTP server (port {})", port)
        }
        "database" => "In-memory database".to_string(),
        "console" => "Console output".to_string(),
        "file_read" => {
            let path = node.config.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            format!("File reader ({})", path)
        }
        "file_write" => {
            let path = node.config.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            format!("File writer ({})", path)
        }
        "transform" => "Data transformer".to_string(),
        "filter" => {
            let field = node.config.get("field").and_then(|v| v.as_str()).unwrap_or("?");
            let op = node.config.get("op").and_then(|v| v.as_str()).unwrap_or("?");
            format!("Filter ({} {})", field, op)
        }
        other => format!("'{}' node", other),
    }
}

fn cmd_init(name: String) {
    let dir = PathBuf::from(&name);
    if dir.exists() {
        eprintln!("Error: directory '{}' already exists", name);
        std::process::exit(1);
    }

    std::fs::create_dir_all(&dir).unwrap_or_else(|e| {
        eprintln!("Error creating directory: {}", e);
        std::process::exit(1);
    });

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
    std::fs::write(&graph_path, graph_content).unwrap_or_else(|e| {
        eprintln!("Error writing graph file: {}", e);
        std::process::exit(1);
    });

    let data_path = dir.join("data.txt");
    std::fs::write(&data_path, format!("Hello from {}!\n", name)).unwrap_or_else(|e| {
        eprintln!("Error writing data file: {}", e);
        std::process::exit(1);
    });

    println!("Created ANP project '{}'", name);
    println!();
    println!("  cd {}", name);
    println!("  anp run main.anp.json");
}

fn cmd_graph(path: PathBuf) {
    let graph = match loader::load_from_file(&path) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

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
            format!(
                " [label=\"{} -> {}\"]",
                edge.from_port, edge.to_port
            )
        };
        println!(
            "  \"{}\" -> \"{}\"{};",
            edge.from_node, edge.to_node, label
        );
    }

    println!("}}");
}
