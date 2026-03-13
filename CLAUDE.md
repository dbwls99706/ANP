# ANP — AI Native Programming

## Core Concept

**ANP (AI Native Programming)** is a paradigm where software is defined as executable graphs instead of source code.

```
Prompt → Graph → Run
```

Programs are not written — they are **generated from intent**. The fundamental unit of software is a **graph**, not a file of code.

## Philosophy

- **Graphs are the program.** A JSON graph definition replaces traditional source code.
- **Intent-first development.** Describe what you want; the LLM generates the executable graph.
- **Prompt → Graph → Run.** The entire dev cycle is: describe intent, generate graph, execute.
- **No compilation.** Graphs are interpreted directly by the ANP runtime.
- **Composable nodes.** All behavior comes from composing typed, port-connected nodes.

## Architecture

```
┌──────────┐     ┌───────────┐     ┌──────────┐
│  Prompt   │ ──→ │  Planner  │ ──→ │  Graph   │ ──→ Runtime
│  (human)  │     │  (LLM)    │     │  (JSON)  │
└──────────┘     └───────────┘     └──────────┘
```

### Crate Structure

- **anp-core**: Graph spec, validation, runtime engine, execution scheduler, GraphOp editor
- **anp-nodes**: Built-in node implementations (console, file_io, transform, filter, database, http_server)
- **anp-planner**: LLM integration — generates graphs from prompts, edit ops from instructions
- **anp-cli**: CLI entry point (`anp run`, `anp prompt`, `anp edit`, etc.)

### Node Kinds

| Kind | Behavior | Example |
|------|----------|---------|
| Source | Produces data, no inputs, executes once | file_read |
| Processor | Transforms data, has inputs and outputs | transform, filter, database |
| Sink | Consumes data, no outputs | console, file_write |
| Service | Long-running event loop, triggers downstream | http_server |

### Graph Spec (v0.1)

```json
{
  "version": "0.1",
  "metadata": { "name": "...", "description": "..." },
  "nodes": [{ "id": "...", "node_type": "...", "kind": "...", "config": {} }],
  "edges": [{ "id": "...", "from_node": "...", "to_node": "..." }]
}
```

### GraphOp API (v0.3)

6 atomic operations for graph mutation: `add_node`, `remove_node`, `connect`, `disconnect`, `update_config`, `insert_after`. Transactional `apply_all` with rollback on failure. Cycle detection on every `connect`.

## Development Notes

- Rust workspace, async (tokio), serde JSON, axum HTTP, clap CLI
- All file I/O paths are validated against traversal attacks
- HTTP body limit: 1MB
- LLM client: auto-detects ANTHROPIC_API_KEY or OPENAI_API_KEY
- Graph validation: duplicate IDs, edge refs, port refs, kind rules, DAG cycle detection
- CLI uses proper Result-based error propagation (no process::exit)

## Roadmap

- v0.1: Core runtime (graph spec, scheduler, executor, built-in nodes)
- v0.2: LLM Planner (`anp prompt` command)
- v0.3: GraphOp API + GraphEditor (`anp edit` command) ← current
- v0.4: Template system, graph optimizer
- v0.5: REPL mode, interactive editing
- v1.0: Plugin ecosystem, custom node types
