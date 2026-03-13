# ANP Demo Video Plan

## Overview

- **Title:** "ANP — Software Without Code: AI-Native Executable Graphs in Rust"
- **Duration:** 2:30 ~ 3:00
- **Target audience:** Developers on Hacker News, Reddit r/rust r/programming, Twitter/X
- **Recording tool:** Terminal recording (asciinema or OBS with terminal)
- **Format:** Split-screen terminal + optional overlay text

---

## Pre-Recording Setup

### Terminal Configuration

```bash
# Clean terminal, larger font (18-20pt), dark theme
# Set a clean PS1 prompt:
export PS1='\[\e[36m\]ANP\[\e[0m\] $ '

# Pre-build to avoid compile noise during recording
cd ANP
cargo build 2>/dev/null

# Set API key (do NOT show the key on screen)
export OPENAI_API_KEY=sk-...

# Optional: suppress noisy INFO logs for cleaner output
export RUST_LOG=warn
```

### Screen Layout

```
┌────────────────────────────────────────────────┐
│  Terminal (main)                                │
│                                                 │
│                                                 │
│                                                 │
│                                                 │
├───────────────────────┬────────────────────────┤
│  (overlay text area)  │  (server logs - Demo 7) │
└───────────────────────┴────────────────────────┘
```

For Demos 4-7 (service mode), use split-screen:
- **Left/Top:** curl commands (the "client")
- **Right/Bottom:** server logs (shows routing decisions)

---

## Script — Shot by Shot

### INTRO (0:00 - 0:12)

**On screen:** Title card or terminal with ASCII art

```
╔══════════════════════════════════════════╗
║  ANP — AI Native Programming             ║
║                                           ║
║  Software is a graph. Code is obsolete.   ║
║                                           ║
║  Prompt → Graph → Run                     ║
╚══════════════════════════════════════════╝
```

**Voiceover / text overlay:**
> "What if software had no source code? Just a graph — and a runtime that executes it."

---

### SCENE 1: Hello World (0:12 - 0:25)

**Text overlay:** `DEMO 1 — Zero Code Hello World`

**Terminal commands:**

```bash
# Show the graph definition (brief!)
cat examples/hello.anp.json | head -20

# Run it
cargo run -p anp-cli -- run examples/hello.anp.json
```

**Voiceover:**
> "Two nodes. One edge. File read to console. No code — just JSON."

**Key moment:** `Hello, ANP!` appears with `nodes_executed=2 nodes_failed=0`

---

### SCENE 2: Data Pipeline (0:25 - 0:50)

**Text overlay:** `DEMO 2 — ETL Pipeline: Read → Transform → Filter → Format`

**Terminal commands:**

```bash
# Show the data
cat examples/sample_data.json

# Run the pipeline
cargo run -p anp-cli -- run examples/data_pipeline.anp.json
```

**Voiceover:**
> "Five employees in. Filter: score above 80. Three top performers out. Template formatting. Five nodes, zero lines of code."

**Key moment:** Three formatted lines appear:
```
[pipeline] Top performer: Alice (Engineering) — Score: 95
[pipeline] Top performer: Charlie (Design) — Score: 88
[pipeline] Top performer: Eve (Engineering) — Score: 91
```

---

### SCENE 3: REST API (0:50 - 1:20)

**Text overlay:** `DEMO 3 — REST API: HTTP Server + Database`

**Split screen: Left = curl commands, Right = server output**

**Terminal 1 (server):**

```bash
cargo run -p anp-cli -- run examples/simple_api.anp.json
# Shows: HTTP server listening port=3000
```

**Terminal 2 (client) — type commands with brief pauses:**

```bash
# Create
curl -s -X POST http://localhost:3000/users \
  -H "Content-Type: application/json" \
  -d '{"op":"set","key":"user:3","value":{"name":"Charlie","age":35}}' | jq .

# Read
curl -s -X POST http://localhost:3000/users \
  -H "Content-Type: application/json" \
  -d '{"op":"get","key":"user:3"}' | jq .

# List
curl -s -X POST http://localhost:3000/users \
  -H "Content-Type: application/json" \
  -d '{"op":"list"}' | jq .
```

**Voiceover:**
> "HTTP server. In-memory database. CRUD API. All defined as a graph. Zero routing code."

**Key moment:** `{"success": true}` → `{"found": true}` → `{"count": 3}`

**Stop server:** `Ctrl+C`

---

### SCENE 4: AI Agent — THE KILLER DEMO (1:20 - 2:20)

**Text overlay:** `DEMO 4 — Autonomous AI Agent: The AI Picks Its Own Tools`

**Brief graph visualization first:**

```bash
# Show the graph structure
cargo run -p anp-cli -- explain examples/ai_agent.anp.json
```

Highlight the data flow:
```
server.agent -> memory -> router
router.search -> search_handler
router.calculate -> calc_handler
router.respond -> chat_handler
```

**Start the agent:**

```bash
cargo run -p anp-cli -- run examples/ai_agent.anp.json
# Shows: HTTP server listening port=3000
```

**Split screen: Left = curl, Right = server logs**

**Sequence of queries (this is the money shot):**

```bash
# 1. Math question
curl -s -X POST http://localhost:3000/agent \
  -H "Content-Type: application/json" \
  -d '{"prompt":"What is the square root of 144?"}' | jq .
```

**Text overlay:** `AI chose: CALCULATE` (highlight the server log showing routing)

```bash
# 2. Casual question
curl -s -X POST http://localhost:3000/agent \
  -H "Content-Type: application/json" \
  -d '{"prompt":"Tell me a joke"}' | jq .
```

**Text overlay:** `AI chose: RESPOND`

```bash
# 3. Factual question
curl -s -X POST http://localhost:3000/agent \
  -H "Content-Type: application/json" \
  -d '{"prompt":"What is the capital of France?"}' | jq .
```

**Text overlay:** `AI chose: SEARCH`

```bash
# 4. Complex calculation
curl -s -X POST http://localhost:3000/agent \
  -H "Content-Type: application/json" \
  -d '{"prompt":"Calculate 15% tip on an $85 dinner bill"}' | jq .
```

**Text overlay:** `AI chose: CALCULATE`

**Voiceover:**
> "No if-else. No routing logic. The AI reads the query. Picks the tool. Routes the data.
> Four different questions. Four different routing decisions. All autonomous.
> This entire agent is one JSON file."

**Stop server:** `Ctrl+C`

---

### SCENE 5: Graph Visualization (2:20 - 2:30)

**Text overlay:** `Visualize Any Graph`

```bash
cargo run -p anp-cli -- graph examples/ai_agent.anp.json
```

**Show the DOT output, then cut to a pre-rendered PNG of the graph from GraphViz.**

**Voiceover:**
> "Every graph can be visualized. Nodes are color-coded by kind."

---

### SCENE 6: Prompt-to-Run (2:30 - 2:50)

**Text overlay:** `Generate from Prompt — One Sentence to Running Software`

```bash
cargo run -p anp-cli -- prompt "read a file and print its contents" --run
```

**Voiceover:**
> "One sentence. The LLM generates the graph. The runtime executes it. Prompt to running software in seconds."

---

### OUTRO (2:50 - 3:00)

**On screen:**

```
ANP — AI Native Programming

  Prompt → Graph → Run

  13 built-in nodes
  111 tests, 0 warnings
  Rust-powered runtime

  github.com/dbwls99706/ANP

  Star ⭐ if this is the future of software.
```

**Voiceover:**
> "ANP. Software is a graph. Code is obsolete."

---

## Post-Production Notes

### Text Overlays to Add in Editing

| Timestamp | Overlay |
|-----------|---------|
| 0:00 | Title: "ANP — AI Native Programming" |
| 0:12 | "DEMO 1: Zero Code Hello World" |
| 0:25 | "DEMO 2: ETL Pipeline" |
| 0:50 | "DEMO 3: REST API — No Code" |
| 1:20 | "DEMO 4: AI Agent — The AI Decides" |
| 1:35 | "AI chose: CALCULATE" (green) |
| 1:45 | "AI chose: RESPOND" (blue) |
| 1:55 | "AI chose: SEARCH" (yellow) |
| 2:20 | "Graph Visualization" |
| 2:30 | "Prompt → Run" |

### Music Suggestions

- Ambient electronic / lo-fi coding music
- Volume low, just enough for atmosphere
- No lyrics

### Upload Targets

| Platform | Format | Title |
|----------|--------|-------|
| YouTube | Full 3min | "ANP: Software Without Code — AI-Native Graphs in Rust" |
| Twitter/X | 60s cut (Scene 4 only) | "AI picks its own tools. No code. Just a graph." |
| Reddit | Full 3min + link to repo | r/rust, r/programming, r/artificial |
| Hacker News | Show HN post + YouTube link | "Show HN: ANP — Runtime where AI agents are executable graphs, not code" |

### Posting Template (Hacker News / Reddit)

```
Show HN: ANP — A runtime where software is executable graphs, not code

ANP (AI Native Programming) is a Rust-based runtime where programs are
JSON graphs executed directly — no source code, no compilation.

Key features:
- 13 built-in nodes (HTTP server, database, LLM calls, agentic routing)
- AI Agent: the LLM autonomously picks which tool to use (no if/else)
- Prompt → Graph → Run: generate apps from natural language
- 111 tests, 0 clippy warnings, SSRF protection, production-grade

Demo video: [link]
Repo: https://github.com/dbwls99706/ANP

The killer demo: send "What is sqrt(144)?" and "Tell me a joke" to
the same endpoint — the AI agent routes each to a different tool
autonomously. Zero routing code.

Built in Rust with tokio, axum, serde. Looking for contributors
interested in building a visual graph editor.
```

---

## Quick Recording Checklist

- [ ] Terminal font size 18-20pt, dark theme
- [ ] Clean prompt (`export PS1='\[\e[36m\]ANP\[\e[0m\] $ '`)
- [ ] Pre-built (`cargo build` done)
- [ ] API key set (not visible on screen)
- [ ] `jq` installed for pretty JSON output
- [ ] Two terminal windows ready for split-screen demos
- [ ] RUST_LOG=warn for cleaner output (optional)
- [ ] Pre-render GraphViz PNG of ai_agent graph
- [ ] Test all commands once before recording
- [ ] Screen recording software ready (OBS / asciinema)
