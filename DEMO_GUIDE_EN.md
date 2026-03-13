# ANP Demo Guide — Complete Step-by-Step

This guide walks you through every ANP demo from start to finish. No prior knowledge required.

---

## 0. Prerequisites

### Required: Install Rust

```bash
# Install Rust (takes ~1 minute)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Load environment
source $HOME/.cargo/env

# Verify
rustc --version   # 1.75.0+ required
cargo --version   # 1.75.0+ required
```

### Required: Clone & Build

```bash
# 1. Clone
git clone https://github.com/dbwls99706/ANP.git
cd ANP

# 2. Build (first build takes 1-2 minutes)
cargo build

# On success you'll see:
#   Compiling anp-core v0.1.0
#   Compiling anp-nodes v0.1.0
#   Compiling anp-planner v0.1.0
#   Compiling anp-cli v0.1.0
#   Finished `dev` profile [unoptimized + debuginfo] target(s)
```

### Optional: API Key (for AI demos)

AI demos (chatbot, agent, smart API) require **one** of these:

```bash
# Option A: OpenAI
export OPENAI_API_KEY=sk-xxxxxxxxxxxxxxxxxxxxxxxx

# Option B: Anthropic
export ANTHROPIC_API_KEY=sk-ant-xxxxxxxxxxxxxxxxxxxxxxxx
```

> Demos 1-4 work perfectly **without any API key**.

---

## Demo 1: Hello World (No API key needed)

**What it does:** Reads a file and prints it. The simplest possible graph.

**Graph:**
```
file_read → console
```

### Run

```bash
cargo run -p anp-cli -- run examples/hello.anp.json
```

### Expected Output

```
 INFO starting ANP engine name=Hello ANP
 INFO execution plan created stages=2 entry_nodes=["source"]
 INFO starting batch execution run_id=...
Hello, ANP!
 INFO batch execution completed run_id=... elapsed_ms=0 nodes_executed=2 nodes_failed=0
 INFO ANP engine finished
```

### Key Takeaways
- `nodes_executed=2` — two nodes ran: file_read (source) + console (sink)
- `nodes_failed=0` — zero errors
- `elapsed_ms=0` — sub-millisecond execution
- **Zero lines of source code.** Just a JSON graph.

---

## Demo 2: Data Pipeline — Read, Transform, Filter, Format (No API key)

**What it does:** Reads JSON data, extracts fields, filters by condition, formats with a template, and prints.

**Graph:**
```
file_read → transform → filter (score >= 80) → template → console
```

### Run

```bash
cargo run -p anp-cli -- run examples/data_pipeline.anp.json
```

### Expected Output

```
 INFO starting ANP engine name=AI Data Pipeline
 INFO execution plan created stages=5 entry_nodes=["source"]
 INFO starting batch execution run_id=...
[pipeline] Top performer: Alice (Engineering) — Score: 95
[pipeline] Top performer: Charlie (Design) — Score: 88
[pipeline] Top performer: Eve (Engineering) — Score: 91
 INFO batch execution completed run_id=... elapsed_ms=0 nodes_executed=5 nodes_failed=0
 INFO ANP engine finished
```

### Key Takeaways
- 5 employees in → filter keeps only score >= 80 → 3 out
- `{{name}}`, `{{department}}`, `{{score}}` placeholders are resolved automatically
- Bob (72) and Diana (64) are filtered out
- 5-stage pipeline executed in under 1ms

### Source Data (for reference)

```bash
cat examples/sample_data.json
```

```json
[
  { "name": "Alice", "email": "alice@example.com", "score": 95, "department": "Engineering" },
  { "name": "Bob", "email": "bob@example.com", "score": 72, "department": "Marketing" },
  { "name": "Charlie", "email": "charlie@example.com", "score": 88, "department": "Design" },
  { "name": "Diana", "email": "diana@example.com", "score": 64, "department": "Sales" },
  { "name": "Eve", "email": "eve@example.com", "score": 91, "department": "Engineering" }
]
```

---

## Demo 3: Nested Field Extraction Pipeline (No API key)

**What it does:** Extracts nested fields via dot-path notation and filters by condition.

**Graph:**
```
file_read → transform (extract address.city) → filter (age >= 20) → console
```

### Run

```bash
cargo run -p anp-cli -- run examples/pipeline.anp.json
```

### Expected Output

```
 INFO starting ANP engine name=Data Pipeline
 INFO execution plan created stages=4 entry_nodes=["reader"]
 INFO starting batch execution run_id=...
[result] [
  {
    "age": 25,
    "city": "Seoul",
    "name": "Alice"
  },
  {
    "age": 30,
    "city": "Incheon",
    "name": "Charlie"
  }
]
 INFO batch execution completed run_id=... elapsed_ms=0 nodes_executed=4 nodes_failed=0
 INFO ANP engine finished
```

### Key Takeaways
- Input: `{"address": {"city": "Seoul"}}` → transform extracts `address.city` into `city` via **dot-path**
- 4 people in → age >= 20 filter → Alice (25) and Charlie (30) pass; Bob (17) and Diana (19) excluded

---

## Demo 4: REST API Server — HTTP + In-Memory DB (No API key)

**What it does:** A full REST API with CRUD operations. No code — just a graph.

**Graph:**
```
http_server(:3000) → database (in-memory) → console
```

### Run — Two terminals needed!

**Terminal 1: Start the server**

```bash
cargo run -p anp-cli -- run examples/simple_api.anp.json
```

Output:
```
 INFO starting ANP engine name=Simple REST API
 INFO execution plan created stages=3 entry_nodes=["server"]
 INFO HTTP server listening port=3000
```

The server is now running. **Leave this terminal open.**

**Terminal 2: Send API requests**

Open a new terminal and run these commands one by one:

```bash
# 1. List users (pre-populated with 2 users)
curl http://localhost:3000/users

# 2. Create a new user
curl -X POST http://localhost:3000/users \
  -H "Content-Type: application/json" \
  -d '{"op": "set", "key": "user:3", "value": {"name": "Charlie", "age": 35}}'
# Expected: {"op":"set","key":"user:3","success":true}

# 3. Read the user back
curl -X POST http://localhost:3000/users \
  -H "Content-Type: application/json" \
  -d '{"op": "get", "key": "user:3"}'
# Expected: {"op":"get","key":"user:3","value":{"name":"Charlie","age":35},"found":true}

# 4. List all keys
curl -X POST http://localhost:3000/users \
  -H "Content-Type: application/json" \
  -d '{"op": "list"}'
# Expected: {"op":"list","keys":["user:1","user:2","user:3"],"count":3}

# 5. Delete a user
curl -X POST http://localhost:3000/users \
  -H "Content-Type: application/json" \
  -d '{"op": "delete", "key": "user:3"}'
# Expected: {"op":"delete","key":"user:3","existed":true}
```

**Stop server:** `Ctrl+C` in Terminal 1

### Key Takeaways
- **Service mode**: http_server runs as a long-lived event loop
- Full CRUD REST API with zero lines of code
- Initial data (`user:1`, `user:2`) is defined in the database node's config
- Every request is logged to console with `[api]` prefix

---

## Demo 5: AI Chatbot — LLM-Powered HTTP Chat (API key required!)

**What it does:** An HTTP chatbot where the LLM generates responses.

**Graph:**
```
http_server(:3000) → template → llm_call(GPT-4o) → console
```

### Prerequisites

```bash
# Verify API key is set
echo $OPENAI_API_KEY
# Should print sk-... If empty:
export OPENAI_API_KEY=sk-your-key-here
```

### Run — Two terminals needed!

**Terminal 1:**

```bash
cargo run -p anp-cli -- run examples/ai_chatbot.anp.json
```

Output:
```
 INFO starting ANP engine name=AI Chatbot
 INFO HTTP server listening port=3000
```

**Terminal 2: Talk to the chatbot**

```bash
# Question 1
curl -X POST http://localhost:3000/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "What is AI Native Programming?"}'
# Expected: LLM responds with a 2-3 sentence explanation

# Question 2
curl -X POST http://localhost:3000/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "Explain quantum computing in simple terms"}'

# Question 3
curl -X POST http://localhost:3000/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "Write a haiku about programming"}'
```

**Stop server:** `Ctrl+C`

### Key Takeaways
- `template` node extracts `{{message}}` and passes it to the LLM
- `llm_call` node invokes the OpenAI API and returns the response
- **"AI calling AI"** — the graph contains intelligence as a first-class node
- System prompt: "You are a helpful, concise assistant. Answer in 2-3 sentences max."

---

## Demo 6: Smart API — REST Endpoints + AI Analysis (API key required!)

**What it does:** A REST API where one endpoint uses AI to analyze data.

**Graph:**
```
http_server(:3000) ─┬─ /items (POST) ──→ database → console
                    ├─ /items (GET)  ──→ database → console
                    └─ /analyze (POST) ──→ llm_call → console
```

### Run — Two terminals needed!

**Terminal 1:**

```bash
cargo run -p anp-cli -- run examples/smart_api.anp.json
```

**Terminal 2:**

```bash
# 1. Store an item
curl -X POST http://localhost:3000/items \
  -H "Content-Type: application/json" \
  -d '{"op": "set", "key": "item:1", "value": {"name": "Laptop", "price": 999}}'

# 2. List items
curl http://localhost:3000/items

# 3. AI analysis (the killer feature!)
curl -X POST http://localhost:3000/analyze \
  -H "Content-Type: application/json" \
  -d '{"prompt": "Analyze this customer feedback: Great product but shipping was slow"}'
# Expected: {"sentiment": "mixed", "category": "logistics", "summary": "..."}

# 4. Another analysis
curl -X POST http://localhost:3000/analyze \
  -H "Content-Type: application/json" \
  -d '{"prompt": "Summarize: Revenue grew 40% YoY but customer churn increased to 8%"}'
```

**Stop server:** `Ctrl+C`

### Key Takeaways
- Traditional CRUD and AI analysis coexist on the same server
- `/items` routes to the database node; `/analyze` routes to the llm_call node
- Routing is determined by `output_port` in the http_server config — no code

---

## Demo 7: Autonomous AI Agent — LLM Picks Its Own Tools (API key required!)

**This is the killer demo.**

**What it does:** The user sends a question. The AI **autonomously decides** whether to use "search", "calculate", or "respond" — no routing logic, no if/else. The LLM decides.

**Graph:**
```
http_server → memory → tool_router ─┬─ search → llm_call (search handler) → console
                                     ├─ calculate → llm_call (calc handler) → console
                                     └─ respond → llm_call (chat handler) → console
```

### Run — Two terminals needed!

**Terminal 1:**

```bash
cargo run -p anp-cli -- run examples/ai_agent.anp.json
```

Output:
```
 INFO starting ANP engine name=Autonomous AI Agent
 INFO HTTP server listening port=3000
```

**Terminal 2: Send diverse queries to the agent**

```bash
# Query 1: Math question → AI auto-selects "calculate"
curl -X POST http://localhost:3000/agent \
  -H "Content-Type: application/json" \
  -d '{"prompt": "What is the square root of 144?"}'
# Server log shows [agent] with the answer "12"

# Query 2: Casual chat → AI auto-selects "respond"
curl -X POST http://localhost:3000/agent \
  -H "Content-Type: application/json" \
  -d '{"prompt": "Tell me a joke"}'

# Query 3: Factual question → AI auto-selects "search"
curl -X POST http://localhost:3000/agent \
  -H "Content-Type: application/json" \
  -d '{"prompt": "What is the capital of France?"}'

# Query 4: Complex math → "calculate"
curl -X POST http://localhost:3000/agent \
  -H "Content-Type: application/json" \
  -d '{"prompt": "Calculate 15% tip on a $85 dinner bill"}'

# Query 5: Mixed intent → watch which tool the AI picks
curl -X POST http://localhost:3000/agent \
  -H "Content-Type: application/json" \
  -d '{"prompt": "How many seconds are in a year?"}'
```

**Stop server:** `Ctrl+C`

### Key Takeaways
- **No if/else. No routing code. The AI decides.**
- `tool_router` node asks the LLM: "which tool is best for this query?"
- The LLM picks `search`, `calculate`, or `respond`
- Data flows to the chosen output port automatically
- `memory` node maintains conversation history across requests
- Watch the server logs to see which node each request passes through in real-time

---

## Bonus: CLI Utility Commands (No API key)

### Validate a Graph

Check correctness without executing:

```bash
cargo run -p anp-cli -- validate examples/ai_agent.anp.json
```

Output:
```
Graph 'Autonomous AI Agent' is valid.
```

### Graph Info

Summary of nodes and edges:

```bash
cargo run -p anp-cli -- info examples/data_pipeline.anp.json
```

Output:
```
ANP Graph: AI Data Pipeline
Description: Reads data, fetches external API, enriches with AI, filters, and outputs...
Version: 0.1
Nodes: 5
Edges: 4

Nodes:
  [   source] source (type: file_read)
  [processor] parse (type: transform)
  [processor] high_score_filter (type: filter)
  [processor] format_report (type: template)
  [     sink] output (type: console)

Edges:
  e1 source.default -> parse.default
  e2 parse.default -> high_score_filter.default
  e3 high_score_filter.default -> format_report.default
  e4 format_report.default -> output.default
```

### Explain a Graph

Human-readable description:

```bash
cargo run -p anp-cli -- explain examples/ai_agent.anp.json
```

Output:
```
This graph defines: Autonomous AI Agent
An AI agent that autonomously decides which tool to use...

  [service] HTTP server (port 3000) starts as a long-running service
  [process] 'memory' node transforms data
  [process] 'tool_router' node transforms data
  [process] 'llm_call' node transforms data
  [process] 'llm_call' node transforms data
  [process] 'llm_call' node transforms data
  [output] Console output consumes data

Data flow:
  server.agent -> memory
  memory -> router
  router.search -> search_handler
  router.calculate -> calc_handler
  router.respond -> chat_handler
  search_handler -> log
  calc_handler -> log
  chat_handler -> log
```

### Visualize — GraphViz DOT Output

```bash
cargo run -p anp-cli -- graph examples/ai_agent.anp.json
```

Paste the output into [GraphViz Online](https://dreampuf.github.io/GraphvizOnline/) to see the graph visually.

Or if you have graphviz installed locally:

```bash
cargo run -p anp-cli -- graph examples/ai_agent.anp.json > agent.dot
dot -Tpng agent.dot -o agent.png
open agent.png    # macOS
xdg-open agent.png  # Linux
```

### Initialize a New Project

```bash
cargo run -p anp-cli -- init my_project
```

Output:
```
Created ANP project 'my_project'

  cd my_project
  anp run main.anp.json
```

Creates `my_project/` with `main.anp.json` and `data.txt` ready to go.

---

## Bonus: Generate & Edit Graphs with AI (API key required!)

### Generate from Prompt

```bash
# Generate and print
cargo run -p anp-cli -- prompt "read data.txt and print it"

# Generate and immediately run
cargo run -p anp-cli -- prompt "read data.txt and print it" --run

# Generate with explanation
cargo run -p anp-cli -- prompt "AI chatbot with memory" --explain

# Save to file
cargo run -p anp-cli -- prompt "REST API that stores user data" --output my_api.anp.json
```

### Edit with Natural Language

```bash
# Preview changes (dry-run)
cargo run -p anp-cli -- edit examples/data_pipeline.anp.json \
  "change the filter to keep scores above 90" --dry-run

# Apply changes
cargo run -p anp-cli -- edit examples/data_pipeline.anp.json \
  "change the filter to keep scores above 90"
```

---

## Run All Tests (For developers)

```bash
# Full test suite (111 tests)
cargo test

# Static analysis (0 warnings)
cargo clippy -- -D warnings
```

---

## Recommended Demo Order (For presentations / recordings)

1. **Hello World** (10s) — "This is ANP. No code. Just a JSON graph."
2. **Data Pipeline** (20s) — "ETL pipeline in one JSON file. Read, transform, filter, format."
3. **REST API** (30s) — "HTTP server + database, zero code." Show CRUD with curl.
4. **AI Agent** (60s) — **The killer demo.** "The AI picks its own tools." Fire math/search/chat queries and watch autonomous routing.
5. **GraphViz** (10s) — "Visualize the graph structure."
6. **Prompt Generation** (20s) — "Generate a graph from one sentence."

**Total runtime: ~2-3 minutes**

---

## Troubleshooting

### "command not found: cargo"
Rust is not installed. Run: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

### "No LLM API key found"
AI demos require: `export OPENAI_API_KEY=sk-...`

### "Address already in use (port 3000)"
A previous server is still running. Kill it: `lsof -i :3000` then `kill <PID>`, or `Ctrl+C` the old terminal.

### "connection refused" (curl error)
The server hasn't started yet, or you need to run curl from a different terminal. Wait until you see "HTTP server listening" in the server logs.

### Build errors
Try a clean build: `cargo clean && cargo build`
