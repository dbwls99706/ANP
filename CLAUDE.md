# ANP — AI Native Programming

## Core Concept

**ANP (AI Native Programming)** is a paradigm where software is defined as executable graphs instead of source code.

```
Prompt → Graph → Run
```

Programs are not written — they are **generated from intent**. The fundamental unit of software is a **graph**, not a file of code.

## Philosophy

- **Why write code if AI generates it?** If an AI is producing the program, there's no reason to output C++, Python, or JavaScript as an intermediate step. The AI can directly produce an executable graph that the runtime interprets — skipping the entire compile/interpret cycle of traditional languages. The graph *is* the executable.
- **Graphs are the program.** A JSON graph definition replaces traditional source code. No syntax to learn, no boilerplate to write.
- **Intent-first development.** Describe what you want; the LLM generates the executable graph.
- **Prompt → Graph → Run.** The entire dev cycle is: describe intent, generate graph, execute. No compilation, no deployment pipeline.
- **No compilation.** Graphs are interpreted directly by the ANP runtime.
- **Composable nodes.** All behavior comes from composing typed, port-connected nodes. Each node does one thing well — the graph wires them together.

## Architecture

```
┌──────────┐     ┌───────────┐     ┌──────────┐
│  Prompt   │ ──→ │  Planner  │ ──→ │  Graph   │ ──→ Runtime
│  (human)  │     │  (LLM)    │     │  (JSON)  │
└──────────┘     └───────────┘     └──────────┘
```

### Crate Structure

- **anp-core**: Graph spec, validation, runtime engine, execution scheduler, GraphOp editor
- **anp-nodes**: Built-in node implementations (console, file_io, transform, filter, database, http_server, llm_call, http_client, template, conditional, memory, tool_router, html_template, static_file, foreach, merge)
- **anp-planner**: LLM integration — generates graphs from prompts, edit ops from instructions
- **anp-cli**: CLI entry point (`anp run`, `anp prompt`, `anp edit`, etc.)

### Node Kinds

| Kind | Behavior | Example |
|------|----------|---------|
| Source | Produces data, no inputs, executes once | file_read |
| Processor | Transforms data, has inputs and outputs | transform, filter, database, llm_call, http_client, template, conditional, memory, tool_router, html_template, static_file, foreach, merge |
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

- Rust workspace, async (tokio), serde JSON, axum HTTP, reqwest HTTP client, clap CLI
- All file I/O paths are validated against traversal attacks
- HTTP body limit: 1MB, HTTP client timeout: 30s (configurable up to 300s)
- URL validation: scheme-restricted (http/https only) to prevent SSRF
- LLM client: auto-detects ANTHROPIC_API_KEY or OPENAI_API_KEY
- Graph validation: duplicate IDs, edge refs, port refs, kind rules, DAG cycle detection
- CLI uses proper Result-based error propagation (no process::exit)
- Template engine: `{{variable}}` syntax with dot-path resolution
- Conditional routing: multi-branch port routing with operator support (eq, neq, gt, gte, lt, lte, contains, exists)
- Memory node: session-based conversation history (max_history limit) + key-value state
- Tool Router: LLM-powered agentic routing — AI decides which tool/port to use
- Execution tracing: per-node timing (NodeTrace), success/failure tracking, output port recording
- HTML Template node: full HTML rendering with `{{variable}}` placeholders, XSS-safe escaping, `{{{raw}}}` for unescaped output
- Static File node: serves CSS/JS/images with auto content-type detection, path traversal prevention
- HTTP Server: supports HTML responses via `_content_type` field from pipeline results
- HTTP Server: path parameters (`/users/:id` → `{"id": "42"}`) and query params (`?key=val` → `{"_query": {"key": "val"}}`)
- ForEach node: graph-native iteration over arrays — replaces for-loops without code
- Merge node: combines multiple JSON objects (shallow, deep, or array strategy)
- Transform node: supports literal values (`'text'`, `42`, `true`, `null`) in addition to dot-paths
- HTTP Client: automatic retry with exponential backoff (configurable retry_count, retry_backoff_ms)
- Pipeline timeout: 30s default per-request timeout prevents hanging pipelines
- Graceful shutdown: SIGINT + SIGTERM handling with connection draining for container deployments

## Roadmap

- v0.1: Core runtime (graph spec, scheduler, executor, built-in nodes)
- v0.2: LLM Planner (`anp prompt` command)
- v0.3: GraphOp API + GraphEditor (`anp edit` command)
- v0.4: AI-native nodes (llm_call, http_client, template, conditional)
- v0.5: Observability, agentic workflows, state management (memory, tool_router, NodeTrace) ← current
- v1.0: Plugin ecosystem, custom node types, visual graph editor
