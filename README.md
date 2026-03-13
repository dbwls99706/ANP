# ANP — AI Native Programming

> Software is a graph. Code is obsolete.

**ANP** is a runtime where software is defined as executable graphs, not source code. AI generates the graphs; the runtime executes them directly.

```
Prompt → LLM Planner → ANP Graph → ANP Runtime → Running Software
```

```bash
$ anp prompt "AI chatbot that answers questions" --run
```

Other AI tools generate source code. ANP generates **executable graphs** — no code, no compilation, just intent and execution.

## What Makes ANP Different

```
Traditional:     Code → Compile → Run
AI coding tools: Prompt → Code → Compile → Run
ANP:             Prompt → Graph → Run
```

The key insight: **AI calling AI**. An `llm_call` node lets graphs contain intelligence. A `tool_router` node lets AI choose its own actions. A `memory` node gives graphs persistent state. Programs that think, reason, remember, and act autonomously — all as a graph.

## Demo: AI Chatbot in Zero Lines of Code

This chatbot has **no source code** — it's a graph where AI calls AI:

```
┌──────────┐  POST /chat  ┌──────────┐  prompt  ┌──────────┐       ┌──────────┐
│  http    ├─────────────▶│ template ├────────▶│ llm_call ├──────▶│ console  │
│  server  │              │          │         │ (GPT-4o) │       │ (stdout) │
│  :3000   │              └──────────┘         └──────────┘       └──────────┘
└──────────┘
   Service                  Processor            Processor            Sink
```

```bash
$ cargo run -p anp-cli -- run examples/ai_chatbot.anp.json

 INFO HTTP server listening port=3000

$ curl -X POST http://localhost:3000/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "What is AI Native Programming?"}'
```

**There is no source code for this chatbot** — only a [graph definition](examples/ai_chatbot.anp.json).

## Demo: REST API with AI Analysis

A REST API where one endpoint uses AI to analyze data — built as a graph:

```bash
$ cargo run -p anp-cli -- run examples/smart_api.anp.json

$ curl -X POST http://localhost:3000/analyze \
  -H "Content-Type: application/json" \
  -d '{"prompt": "Analyze this customer feedback: Great product but shipping was slow"}'
```

## Demo: Data Pipeline

Read → Transform → Filter → Template → Output, with no code:

```bash
$ cargo run -p anp-cli -- run examples/data_pipeline.anp.json

[pipeline] Top performer: Alice (Engineering) — Score: 95
[pipeline] Top performer: Charlie (Design) — Score: 88
```

## Demo: Autonomous AI Agent

An AI agent that **decides its own actions** — the LLM chooses whether to search, calculate, or chat:

```
┌──────────┐        ┌────────┐        ┌─────────────┐    search    ┌──────────┐
│  http    ├───────▶│ memory ├───────▶│ tool_router ├────────────▶│ llm_call │
│  server  │        │        │        │  (AI picks  │  calculate  │ (search) │──▶ console
│  :3000   │        └────────┘        │   the tool) ├────────────▶│ llm_call │
└──────────┘                          │             │   respond   │ (calc)   │
                                      └─────────────┘────────────▶│ llm_call │
                                                                  │ (chat)   │
                                                                  └──────────┘
```

```bash
$ cargo run -p anp-cli -- run examples/ai_agent.anp.json

$ curl -X POST http://localhost:3000/agent \
  -H "Content-Type: application/json" \
  -d '{"prompt": "What is the square root of 144?"}'
# → AI routes to "calculate" tool automatically

$ curl -X POST http://localhost:3000/agent \
  -d '{"prompt": "Tell me a joke"}'
# → AI routes to "respond" tool automatically
```

**No if/else logic. No routing code. The AI decides.** See [ai_agent.anp.json](examples/ai_agent.anp.json).

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (1.75+)
- `OPENAI_API_KEY` or `ANTHROPIC_API_KEY` (for `llm_call` and `anp prompt`)

### Build & Run

```bash
git clone https://github.com/dbwls99706/ANP.git
cd ANP
cargo build

# Hello world
cargo run -p anp-cli -- run examples/hello.anp.json

# Data pipeline
cargo run -p anp-cli -- run examples/data_pipeline.anp.json

# AI chatbot (requires API key)
export OPENAI_API_KEY=sk-...
cargo run -p anp-cli -- run examples/ai_chatbot.anp.json

# REST API
cargo run -p anp-cli -- run examples/simple_api.anp.json

# Autonomous AI agent (requires API key)
cargo run -p anp-cli -- run examples/ai_agent.anp.json
```

### Generate from Prompt

```bash
export ANTHROPIC_API_KEY=sk-ant-...

# Generate and print
cargo run -p anp-cli -- prompt "REST API that stores user data"

# Generate and run
cargo run -p anp-cli -- prompt "read data.txt and print it" --run

# Generate with explanation
cargo run -p anp-cli -- prompt "AI chatbot with memory" --explain
```

### Edit Graphs with Natural Language

```bash
# Add a node
cargo run -p anp-cli -- edit graph.anp.json "add a filter that keeps scores above 90"

# Modify a node
cargo run -p anp-cli -- edit graph.anp.json "change the LLM to use Anthropic Claude"
```

### CLI Commands

```bash
anp run <graph>        # Execute a graph
anp validate <graph>   # Validate without executing
anp info <graph>       # Display graph structure
anp explain <graph>    # Human-readable explanation
anp graph <graph>      # Output GraphViz DOT format
anp init <name>        # Scaffold a new project
anp prompt <text>      # Generate graph from natural language
anp edit <graph> <instruction>  # Edit graph with natural language
```

## Built-in Nodes

### Core I/O

| Node | Kind | Description |
|------|------|-------------|
| `console` | Sink | Print to stdout |
| `file_read` | Source | Read file content |
| `file_write` | Sink | Write data to file |
| `http_server` | Service | HTTP server with route-based output ports |

### Data Processing

| Node | Kind | Description |
|------|------|-------------|
| `transform` | Processor | Dot-path field mapping |
| `filter` | Processor | Condition-based filtering (eq, gt, lt, contains...) |
| `database` | Processor | In-memory key-value store (get/set/delete/list) |
| `template` | Processor | String templates with `{{variable}}` placeholders |
| `conditional` | Processor | Routes data to different ports based on conditions |

### AI & Network

| Node | Kind | Description |
|------|------|-------------|
| `llm_call` | Processor | Calls OpenAI or Anthropic LLMs — **AI invoking AI** |
| `http_client` | Processor | Outbound HTTP requests (GET, POST, PUT, DELETE...) |
| `tool_router` | Processor | **LLM-powered agentic routing** — AI decides which tool to use |
| `memory` | Processor | Session-based conversation history and key-value state |

**13 nodes** that compose into any application — from data pipelines to AI agents to REST APIs.

## How It Works

### 1. Define software as a graph

```json
{
  "nodes": [
    { "id": "read",  "node_type": "file_read", "kind": "source",
      "config": { "path": "data.txt" } },
    { "id": "print", "node_type": "console",   "kind": "sink" }
  ],
  "edges": [
    { "id": "e1", "from_node": "read", "to_node": "print" }
  ]
}
```

### 2. The runtime executes it

```bash
cargo run -p anp-cli -- run graph.anp.json
```

No compilation. No source code. Just a graph and a runtime.

## Architecture

```
┌──────────┐     ┌───────────┐     ┌──────────┐
│  Prompt   │ ──→ │  Planner  │ ──→ │  Graph   │ ──→ Runtime
│  (human)  │     │  (LLM)    │     │  (JSON)  │
└──────────┘     └───────────┘     └──────────┘
```

```
┌─────────────┐
│  ANP Graph   │  JSON specification
└──────┬──────┘
       ↓
┌──────┴──────┐
│  Validator   │  6 validation rules (cycles, ports, edges...)
└──────┬──────┘
       ↓
┌──────┴──────┐
│  Scheduler   │  Topological sort → ExecutionPlan
└──────┬──────┘
       ↓
┌──────┴──────┐
│  Executor    │  Stage-by-stage parallel execution
└──────┬──────┘
       ↓
┌──────┴──────┐
│    Nodes     │  13 built-in nodes (AI, agents, network, data, I/O)
└─────────────┘
```

### Execution Modes

| Mode | Trigger | Behavior |
|------|---------|----------|
| **Batch** | No Service nodes | Source → Processor → Sink, then exit |
| **Service** | Has Service nodes | Start services → wait for events → Ctrl+C to stop |

### Node Kinds

| Kind | Description | Examples |
|------|-------------|----------|
| **Source** | Produces data (runs once) | `file_read` |
| **Processor** | Transforms data | `transform`, `filter`, `database`, `llm_call`, `http_client`, `template`, `conditional`, `memory`, `tool_router` |
| **Sink** | Consumes data | `console`, `file_write` |
| **Service** | Long-running event loop | `http_server` |

### Key Features

- **AI-in-the-graph** — `llm_call` node enables graphs that think and reason
- **Agentic workflows** — `tool_router` lets AI choose its own actions autonomously
- **Stateful sessions** — `memory` node provides conversation history and persistent state
- **Execution tracing** — per-node timing, success/failure tracking (NodeTrace)
- **Event-driven runtime** — Service nodes trigger downstream pipelines per-event
- **Parallel pipeline execution** — independent stages run concurrently
- **Conditional routing** — branch data to different ports based on conditions
- **Template engine** — `{{variable}}` interpolation with dot-path support
- **HTTP client** — call external APIs from within graphs
- **Transactional graph editing** — 6 atomic GraphOps with rollback on failure
- **LLM Planner** — generate graphs from natural language prompts
- **Security** — SSRF protection, path traversal guard, URL validation, body size limits

## Examples

| Example | Description |
|---------|-------------|
| [`hello.anp.json`](examples/hello.anp.json) | Minimal — read file and print |
| [`pipeline.anp.json`](examples/pipeline.anp.json) | file_read → transform → filter → console |
| [`simple_api.anp.json`](examples/simple_api.anp.json) | http_server → database → console |
| [`ai_chatbot.anp.json`](examples/ai_chatbot.anp.json) | **AI chatbot** — http_server → template → llm_call → console |
| [`smart_api.anp.json`](examples/smart_api.anp.json) | **Smart API** — REST endpoints with AI-powered analysis |
| [`data_pipeline.anp.json`](examples/data_pipeline.anp.json) | **Data pipeline** — file_read → transform → filter → template → console |
| [`ai_agent.anp.json`](examples/ai_agent.anp.json) | **AI Agent** — tool_router → multiple LLM tools (autonomous routing) |

## Project Structure

```
ANP/
├── crates/
│   ├── anp-core/      # Graph spec, runtime engine, node system, GraphOp editor, execution tracing
│   ├── anp-nodes/     # 13 built-in node implementations
│   ├── anp-planner/   # LLM-powered graph generation
│   └── anp-cli/       # CLI interface
├── examples/          # Example ANP graph files
└── docs/              # Graph specification
```

## Roadmap

- **v0.1** — Graph spec, runtime engine, basic nodes, CLI
- **v0.2** — LLM Planner (Prompt → Graph generation)
- **v0.3** — GraphOp API + GraphEditor (`anp edit`)
- **v0.4** — AI-native nodes (llm_call, http_client, template, conditional)
- **v0.5** — Observability, agentic workflows, state management (memory, tool_router, NodeTrace) ← *current*
- **v1.0** — Plugin ecosystem, custom node types, visual graph editor

## License

MIT
