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
- **anp-nodes**: 100 built-in node implementations across 12 categories (see Node Catalog below)
- **anp-planner**: LLM integration — generates graphs from prompts, edit ops from instructions
- **anp-cli**: CLI entry point (`anp run`, `anp prompt`, `anp edit`, etc.)

### Node Kinds

| Kind | Behavior | Example |
|------|----------|---------|
| Source | Produces data, no inputs, executes once | file_read, env |
| Processor | Transforms data, has inputs and outputs | transform, filter, template, conditional, merge, foreach, llm_call, http_client, etc. |
| Sink | Consumes data, no outputs | console, file_write, log, email |
| Service | Long-running event loop, triggers downstream | http_server, cron, websocket |

### Node Catalog (100 nodes)

**I/O & Core** (16): console, file_read, file_write, transform, filter, foreach, merge, template, template_engine, conditional, switch, try_catch, parallel, accumulator, debounce, sleep

**HTTP & Networking** (8): http_server, http_client, cors, redirect, webhook, websocket, sse, static_file

**Data Formats** (8): json_parse, csv, yaml, xml, toml, markdown, base64, url_encode

**Data Processing** (8): diff, group_by, pivot, lookup, math, array, string, regex

**AI/ML** (8): llm_call, tool_router, embedding, sentiment, summarize, classify, extract, translate

**AI Workflow** (2): prompt_chain, agent_loop

**Auth & Security** (9): jwt, api_key, password, totp, encrypt, hash, sanitize, aes, oauth

**State & Storage** (10): database, memory, cache, queue, priority_queue, session, kv_store, counter, redis, sql_query

**Resilience & Flow** (9): rate_limit, retry, circuit_breaker, semaphore, deduplicate, throttle, batch, cron, aggregate

**UI & Frontend** (8): html_template, form, chart, table, markdown_render, paginate, breadcrumb, layout, meta

**Observability** (6): log, validate, random, uuid, timestamp, env

**Integration** (3): email, image_resize, multipart

**Advanced Workflow** (5): event_emitter, metrics, state_machine, message_broker, ws

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
- Embedding node: character-frequency text embeddings with cosine similarity/distance
- Sentiment analysis: keyword-based scoring with custom positive/negative word lists
- Summarize node: extractive summarization (sentences, truncate, keywords)
- Classify node: rule-based text classification with configurable keyword categories
- Extract node: pattern-based extraction (emails, URLs, numbers, dates, mentions, hashtags, IPs)
- Translate node: text transformations (reverse, pig_latin, rot13, morse, leetspeak)
- Prompt Chain: multi-step template chaining with context propagation
- Agent Loop: iteration control with max_iterations and termination conditions
- API Key node: validation, generation, and masking
- Password node: FNV hashing, verification, and strength analysis
- TOTP node: time-based OTP generation and verification
- Circuit Breaker: closed/open/half_open state transitions for fault tolerance
- Semaphore node: concurrency limiting with acquire/release/status
- Deduplicate node: key-based deduplication with configurable window/TTL
- Sanitize node: HTML/SQL/XSS sanitization with recursive string processing
- XML/TOML/Markdown nodes: format conversion without external dependencies
- Diff node: JSON diffing, patching, equality checking
- GroupBy/Pivot/Lookup: data reshaping and join operations
- URL Encode node: encode/decode/parse/build URL operations
- Form node: HTML form builder with multiple field types
- Chart node: SVG chart generation (bar, line, pie)
- Table node: HTML table generation with XSS escaping
- Paginate node: array pagination with page metadata
- Breadcrumb/Layout/Meta nodes: HTML page structure generation
- Batch/Throttle nodes: flow control for high-throughput pipelines
- Priority Queue: priority-ordered enqueue/dequeue operations
- Aggregate node: multi-field aggregation pipeline (sum, avg, min, max, count, concat)
- Event Emitter: pub/sub event system within graphs
- Metrics node: counters, gauges, histograms with percentile summaries
- KV Store: namespaced key-value storage with CRUD operations
- Planner catalog: auto-generated from NodeRegistry (100 nodes reflected automatically)
- OAuth node: OAuth 2.0 authorization code flow, client_credentials, token refresh, PKCE, provider presets (Google, GitHub, Microsoft)
- AES node: pure Rust AES-256-CBC encryption/decryption, PKCS7 padding, key derivation from passphrase
- Multipart node: multipart/form-data builder/parser for file uploads, URL-encoded form encoding
- SQL Query node: in-process SQL engine over JSON arrays — SELECT, INSERT, UPDATE, DELETE, WHERE, ORDER BY, GROUP BY, JOIN, aggregates
- Redis node: Redis-compatible in-memory data store — strings, hashes, lists, sets, sorted sets, TTL/EXPIRE, KEYS, INCR
- Message Broker node: topic-based pub/sub with dead letter queues, ack/nack, subscription management, stats
- State Machine node: FSM with states, transitions, guard conditions, context, history, wildcard transitions — overcomes DAG limitation
- WebSocket Enhanced (ws) node: room management, broadcasting, direct messaging, connection tracking, message history, kick

## Roadmap

- v0.1: Core runtime (graph spec, scheduler, executor, built-in nodes)
- v0.2: LLM Planner (`anp prompt` command)
- v0.3: GraphOp API + GraphEditor (`anp edit` command)
- v0.4: AI-native nodes (llm_call, http_client, template, conditional)
- v0.5: Observability, agentic workflows, state management (memory, tool_router, NodeTrace)
- v0.6: 92-node ecosystem — AI/ML, auth, data processing, UI, workflow nodes
- v0.7: 100-node ecosystem — AES crypto, OAuth 2.0, SQL engine, Redis, message broker, FSM, WebSocket rooms ← current
- v1.0: Plugin ecosystem, custom node types, visual graph editor
