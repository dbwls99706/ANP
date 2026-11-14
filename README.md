# ANP — AI Native Programming

> **Software is a graph. Code is obsolete.**

Other AI tools generate source code. ANP generates **executable graphs** — no code, no compilation, just intent and execution.

```
Prompt → Graph → Verify → Optimize → Run
```

```bash
$ anp prompt "AI chatbot that answers questions" --run
```

---

## Why ANP

A [METR study](https://metr.org/) found that AI coding assistants make experienced developers **19% slower** due to verification overhead — reading, reviewing, debugging AI-generated code. The problem isn't the AI. The problem is **code as output**.

AI-generated code has **1.7x more bugs** than human-written code. Every line needs human review. Every function needs testing. Every integration needs debugging.

ANP's thesis: **Don't let AI write code. Let AI compose pre-verified components.**

| | AI-Generated Code | ANP Graphs |
|---|---|---|
| Output | Source code (Python, JS, etc.) | Executable graph (JSON) |
| Each unit | Untested, novel code | Pre-tested Rust node |
| Composition | Implicit, fragile | Typed port connections |
| Verification | Manual code review | Automatic graph verification |
| Debugging | Stack traces, print statements | Visual node traces |
| Execution | Compile/interpret | Direct runtime execution |

---

## What No Competitor Has

### Formal Graph Verification

```bash
$ anp verify api.anp.json
✓ Type safety: all port connections verified
✓ Completeness: all required inputs satisfied
✓ Reachability: no dead nodes
✓ Cycle-free: DAG property holds
✓ Security: no path traversal, SSRF protected
```

Mathematical proof that your graph is correct **before** it runs. No unit tests needed.

### Compiler-Level Optimization

```bash
$ anp optimize pipeline.anp.json
- Dead node elimination: removed 2 unreachable nodes
- Transform fusion: merged 3 consecutive transforms → 1
- Edge shortcutting: bypassed 1 passthrough node
  Result: 8 nodes → 5 nodes (37% reduction)
```

The same optimization passes that compilers apply to code — applied to graphs.

### AOT Compilation

```bash
$ anp compile server.anp.json
Compiled execution plan: 4 stages, 12 nodes
Reverse edge index: pre-computed
Stage parallelism: stages 2,3 run concurrently
Output: server.anp.plan (ready for instant startup)
```

Pre-compute the execution plan. Zero startup overhead in production.

### Semantic Graph Diffing

```bash
$ anp diff v1.anp.json v2.anp.json
+ Added: rate_limiter (processor) between http_server → handler
~ Changed: llm_call.config.model "gpt-4" → "claude-sonnet-4-20250514"
- Removed: debug_logger (sink)
  Edges: 2 added, 1 removed, 5 unchanged
```

Not text diff. **Structural, semantic** diffing that understands graph topology.

### Port Type System

Compile-time type checking for graph connections. A `json_parse` output port carries `Object`; connecting it to a port expecting `String` is caught **before execution**. No runtime type errors.

### O(1) Input Gathering

Reverse edge index eliminates O(E) edge scanning per node. When a node needs its inputs, lookup is instant — not a linear scan of every edge in the graph.

### Automatic Parallel Execution

Topological sort identifies independent stages. Nodes within a stage execute concurrently via `futures::join_all`. No manual async/await, no thread pools to configure.

---

## Performance

| Metric | ANP (Rust) | Node-RED (JS) | Langflow (Python) |
|--------|-----------|---------------|-------------------|
| Runtime | Compiled Rust | V8 interpreter | CPython |
| GC pauses | Zero | Yes | Yes |
| Parallelism | Automatic (tokio) | Single-threaded | GIL-limited |
| Input gathering | O(1) reverse index | O(E) scan | O(E) scan |
| HTTP connections | Pooled + reused | Per-request | Per-request |
| Static files | In-memory LRU + ETag | Disk reads | Disk reads |
| Startup | AOT pre-compiled plan | JIT interpretation | JIT interpretation |

---

## 104 Built-in Nodes

Every node is **production-tested Rust** — no npm packages, no pip dependencies, no supply chain risk.

| Category | Count | Nodes |
|----------|-------|-------|
| **I/O & Core** | 16 | console, file_read, file_write, transform, filter, foreach, merge, template, template_engine, conditional, switch, try_catch, parallel, accumulator, debounce, sleep |
| **HTTP & Networking** | 8 | http_server, http_client, cors, redirect, webhook, websocket, sse, static_file |
| **Data Formats** | 9 | json_parse, csv, yaml, xml, toml, markdown, base64, url_encode, gzip |
| **Data Processing** | 8 | diff, group_by, pivot, lookup, math, array, string, regex |
| **AI / ML** | 8 | llm_call, tool_router, embedding, sentiment, summarize, classify, extract, translate |
| **AI Workflow** | 3 | prompt_chain, agent_loop, decision |
| **Auth & Security** | 9 | jwt, api_key, password, totp, encrypt, hash, sanitize, aes, oauth |
| **State & Storage** | 10 | database, memory, cache, queue, priority_queue, session, kv_store, counter, redis, sql_query |
| **Resilience & Flow** | 9 | rate_limit, retry, circuit_breaker, semaphore, deduplicate, throttle, batch, cron, aggregate |
| **UI & Frontend** | 8 | html_template, form, chart, table, markdown_render, paginate, breadcrumb, layout, meta |
| **Observability** | 8 | log, validate, random, uuid, timestamp, env, health_check, request_id |
| **Integration** | 3 | email, image_resize, multipart |
| **Advanced Workflow** | 5 | event_emitter, metrics, state_machine, message_broker, ws |

From a REST API to a real-time dashboard to an autonomous AI agent — 104 nodes compose into anything.

---

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) 1.75+
- `OPENAI_API_KEY` or `ANTHROPIC_API_KEY` (for AI nodes and `anp prompt`)

### Build & Run

```bash
git clone https://github.com/dbwls99706/ANP.git && cd ANP
cargo build

# Hello world
anp run examples/hello.anp.json

# Data pipeline (5-stage ETL)
anp run examples/data_pipeline.anp.json

# AI chatbot (requires API key)
export OPENAI_API_KEY=sk-...
anp run examples/ai_chatbot.anp.json

# Autonomous AI agent
anp run examples/ai_agent.anp.json
```

### Generate from Prompt

```bash
# Describe intent, get a running program
anp prompt "REST API that stores user data" --run

# Generate with explanation
anp prompt "AI chatbot with memory" --explain
```

### Edit with Natural Language

```bash
anp edit graph.anp.json "add rate limiting before the API handler"
anp edit graph.anp.json "change the LLM to Claude"
```

### Certify a Decision Workflow

A `decision` node commits to a choice when its confidence reaches its threshold and sends
the input to its `uncertain` port (human review) otherwise. `anp certify` turns labelled
calibration runs into a certificate: an upper bound on the rate of wrong, unescalated runs
that holds for every threshold setting at once, so thresholds can be retuned later without
recalibrating.

```bash
# 1. Issue a certificate from labelled runs (one JSON object per line)
anp certify examples/certify/workflow.anp.json \
    --traces examples/certify/traces.jsonl --out cert.json

# 2. Pick the thresholds with the least human review for a 5% error target
anp certify examples/certify/workflow.anp.json --cert cert.json \
    --epsilon 0.05 --apply tuned.anp.json

# 3. Check a graph against the certificate (fails if thresholds are too low
#    or a decision node changed since it was certified)
anp verify tuned.anp.json --risk cert.json --epsilon 0.05

# 4. After editing a node, relabel only the traffic that passes through it
anp certify examples/certify/workflow_edited.anp.json --cert cert.json \
    --fresh examples/certify/fresh_intent_banking.jsonl --edited intent_banking \
    --out cert.json --epsilon 0.05 --apply tuned.anp.json
```

Token probabilities of language models sit very close to 1; pass `--fine-tail` when issuing
the certificate to add the thresholds 0.9999, 0.99999 and 0.999999.

A trace line lists the decisions of one run with every node committing, in order:
`{"steps": [{"node": "scope", "confidence": 0.93, "correct": true}, ...]}`. `correct` says
whether the decision matches the ground truth; only the first `false` of a run matters.
The example traces are real model outputs on CLINC150. The method and its evaluation are
described in [`paper/`](paper/).

### CLI Commands

```bash
anp run <graph>              # Execute a graph
anp prompt <text>            # Generate graph from natural language
anp edit <graph> <instruction>  # Edit graph with natural language
anp verify <graph>           # Formal verification (types, cycles, reachability)
anp optimize <graph>         # Dead node elimination, transform fusion
anp compile <graph>          # AOT compilation to execution plan
anp diff <a> <b>             # Semantic graph diff
anp certify <graph>          # Certify the error rate of decision nodes, tune thresholds
anp validate <graph>         # Validate without executing
anp info <graph>             # Display graph structure
anp explain <graph>          # Human-readable explanation
anp graph <graph>            # GraphViz DOT output
anp init <name>              # Scaffold a new project
```

---

## Demos

### Hello World — Two Nodes, Zero Code

```
┌──────────┐  default  ┌──────────┐
│file_read  ├─────────▶│ console  │
│ (source)  │          │  (sink)  │
└──────────┘          └──────────┘
```

### AI Chatbot — No Source Code

```
┌──────────┐  POST /chat  ┌──────────┐  prompt  ┌──────────┐       ┌──────────┐
│  http    ├─────────────▶│ template ├────────▶│ llm_call ├──────▶│ console  │
│  server  │              │          │         │ (GPT-4o) │       │ (stdout) │
│  :3000   │              └──────────┘         └──────────┘       └──────────┘
└──────────┘
```

**No routing code. No framework. No dependencies.** Just a [graph definition](examples/ai_chatbot.anp.json).

### Autonomous AI Agent — AI Decides Its Own Actions

```
┌──────────┐       ┌────────┐       ┌─────────────┐  search     ┌──────────┐
│  http    ├──────▶│ memory ├──────▶│ tool_router ├────────────▶│ llm_call │
│  server  │       │        │       │  (AI picks  │  calculate  │ (search) │──▶ console
│  :3000   │       └────────┘       │   the tool) ├────────────▶│ llm_call │
└──────────┘                        │             │  respond    │ (calc)   │
                                    └─────────────┘────────────▶│ llm_call │
                                                                │ (chat)   │
                                                                └──────────┘
```

**No if/else logic. No routing code. The AI decides.** See [ai_agent.anp.json](examples/ai_agent.anp.json).

### More Examples

| Example | Description |
|---------|-------------|
| [`hello.anp.json`](examples/hello.anp.json) | Minimal read + print |
| [`data_pipeline.anp.json`](examples/data_pipeline.anp.json) | 5-stage ETL pipeline |
| [`ai_chatbot.anp.json`](examples/ai_chatbot.anp.json) | LLM chatbot over HTTP |
| [`ai_agent.anp.json`](examples/ai_agent.anp.json) | Autonomous agent with tool routing |
| [`smart_api.anp.json`](examples/smart_api.anp.json) | REST API + AI analysis |
| [`landing_page.anp.json`](examples/landing_page.anp.json) | Full landing page, zero frontend code |
| [`rest_api_with_params.anp.json`](examples/rest_api_with_params.anp.json) | Path params + query params |
| [`data_processor.anp.json`](examples/data_processor.anp.json) | ForEach iteration pipeline |

---

## Architecture

```
┌──────────┐     ┌───────────┐     ┌──────────┐     ┌──────────┐     ┌──────────┐     ┌─────────┐
│  Prompt   │ ──→ │  Planner  │ ──→ │  Graph   │ ──→ │ Verifier │ ──→ │Optimizer │ ──→ │ Runtime │
│  (human)  │     │  (LLM)    │     │  (JSON)  │     │ (proof)  │     │ (passes) │     │ (Rust)  │
└──────────┘     └───────────┘     └──────────┘     └──────────┘     └──────────┘     └─────────┘
```

### Crate Structure

```
ANP/
├── crates/
│   ├── anp-core/      # Graph spec, validator, scheduler, executor, GraphOp editor, tracing
│   ├── anp-nodes/     # 104 built-in node implementations (pure Rust)
│   ├── anp-planner/   # LLM-powered graph generation from prompts
│   └── anp-cli/       # CLI interface (run, prompt, edit, verify, optimize, compile, diff)
├── examples/          # Example ANP graph files
└── docs/              # Specification
```

### Execution Model

| Mode | Trigger | Behavior |
|------|---------|----------|
| **Batch** | No Service nodes | Source → Processor → Sink, then exit |
| **Service** | Has Service nodes | Start services → event loop → graceful shutdown |

### Node Kinds

| Kind | Behavior | Example |
|------|----------|---------|
| **Source** | Produces data, runs once | file_read, env |
| **Processor** | Transforms data | transform, llm_call, filter, template |
| **Sink** | Consumes data | console, file_write, log |
| **Service** | Long-running event loop | http_server, cron, websocket |

### Security

Path traversal prevention, SSRF protection (scheme-restricted URLs), XSS-safe HTML escaping, body size limits (1MB), URL validation, HTML/SQL/XSS sanitization.

---

## ANP vs Competitors

| Feature | ANP | Node-RED | Langflow | n8n |
|---------|-----|----------|----------|-----|
| Runtime | Rust | JavaScript | Python | JavaScript |
| Formal Verification | Yes | No | No | No |
| Graph Optimizer | Yes | No | No | No |
| AOT Compilation | Yes | No | No | No |
| Port Type System | Yes | No | No | No |
| AI-Native Nodes | 10 nodes | Limited | Yes | Limited |
| Prompt to Graph | Yes | No | No | No |
| Parallel Execution | Automatic | No | No | No |
| Built-in Nodes | 104 (Rust) | ~5000 (community JS) | ~100 | ~400 |
| Zero GC Pauses | Yes | No | No | No |
| Graph Editing via LLM | Yes | No | No | No |

---

## Research Value

**Paper.** *Risk Certificates That Survive Edits: Separable, Route-Indexed Calibration for
Decision Workflows* (Yujin Hong). The manuscript is [`paper/main.pdf`](paper/main.pdf); the
experiment code, data and the script that regenerates every number are in
[`paper/`](paper/README.md), and the method is implemented in `anp certify` and
`anp verify --risk` (see [Certify a Decision Workflow](#certify-a-decision-workflow)).

ANP introduces three novel contributions to the graph-based programming literature:

1. **Formal verification for executable graphs** — type-safe port connections, reachability analysis, and cycle detection as a unified verification pass. No prior graph execution engine offers mathematical correctness guarantees.

2. **Compiler optimization for dataflow graphs** — dead node elimination, transform fusion, and edge shortcutting applied to runtime graphs. Brings decades of compiler theory to a new domain.

3. **LLM-native program synthesis** — instead of generating code that must be parsed, compiled, and debugged, the LLM directly emits the executable representation. The semantic gap between intent and execution is one step, not five.

---

## Roadmap

- **v0.1** — Graph spec, runtime engine, basic nodes, CLI
- **v0.2** — LLM Planner (Prompt → Graph generation)
- **v0.3** — GraphOp API + GraphEditor (`anp edit`)
- **v0.4** — AI-native nodes (llm_call, http_client, template, conditional)
- **v0.5** — Observability, agentic workflows, state management
- **v0.6** — 92-node ecosystem (AI/ML, auth, data, UI, workflow)
- **v0.7** — 103 nodes (AES, OAuth, SQL engine, Redis, FSM, WebSocket rooms, gzip, health checks)
- **v0.8** — Formal verification, graph optimizer, AOT compilation, semantic diff, port type system *← current*
- **v1.0** — Plugin ecosystem, custom node types, visual graph editor

## License

MIT
