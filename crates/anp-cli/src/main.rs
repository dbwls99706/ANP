use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use anp_core::graph::spec::NodeKind;
use anp_core::graph::{loader, validate};
use anp_core::node::registry::NodeRegistry;
use anp_core::runtime::engine::AnpEngine;

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
