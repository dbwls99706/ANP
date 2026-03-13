#!/usr/bin/env bash
# ANP Demo Script
# Runs all examples and shows the HTTP API in action.
# Usage: ./scripts/demo.sh

set -e
cd "$(dirname "$0")/.."

GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
NC='\033[0m'

print_header() {
    echo ""
    echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${GREEN}  $1${NC}"
    echo -e "${CYAN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo ""
}

print_cmd() {
    echo -e "${YELLOW}\$ $1${NC}"
}

# Build
print_header "Building ANP"
print_cmd "cargo build --quiet"
cargo build --quiet

# Demo 1: Hello World
print_header "Demo 1: Hello World (batch mode)"
print_cmd "anp run examples/hello.anp.json"
cargo run -p anp-cli --quiet -- run examples/hello.anp.json 2>/dev/null

# Demo 2: Data Pipeline
print_header "Demo 2: Data Pipeline (file_read → transform → filter → console)"
print_cmd "anp run examples/pipeline.anp.json"
cargo run -p anp-cli --quiet -- run examples/pipeline.anp.json 2>/dev/null

# Demo 3: Graph Validation
print_header "Demo 3: Graph Validation"
print_cmd "anp validate examples/simple_api.anp.json"
cargo run -p anp-cli --quiet -- validate examples/simple_api.anp.json

# Demo 4: Graph Explanation
print_header "Demo 4: Graph Explanation"
print_cmd "anp explain examples/simple_api.anp.json"
cargo run -p anp-cli --quiet -- explain examples/simple_api.anp.json

# Demo 5: HTTP API Server
print_header "Demo 5: HTTP API Server (service mode)"
print_cmd "anp run examples/simple_api.anp.json"
echo ""

cargo run -p anp-cli --quiet -- run examples/simple_api.anp.json 2>&1 &
SERVER_PID=$!
sleep 1

echo -e "${YELLOW}\$ curl http://localhost:3000/users${NC}"
curl -s http://localhost:3000/users | python3 -m json.tool 2>/dev/null || curl -s http://localhost:3000/users
echo ""

echo -e "${YELLOW}\$ curl -X POST http://localhost:3000/users -d '{\"op\":\"set\",\"key\":\"user:3\",\"value\":{\"name\":\"Charlie\"}}'${NC}"
curl -s -X POST http://localhost:3000/users \
    -H "Content-Type: application/json" \
    -d '{"op":"set","key":"user:3","value":{"name":"Charlie","age":35}}' | python3 -m json.tool 2>/dev/null || true
echo ""

echo -e "${YELLOW}\$ curl http://localhost:3000/users${NC}"
curl -s http://localhost:3000/users | python3 -m json.tool 2>/dev/null || curl -s http://localhost:3000/users
echo ""

echo -e "${YELLOW}\$ curl -X POST http://localhost:3000/users -d '{\"op\":\"get\",\"key\":\"user:1\"}'${NC}"
curl -s -X POST http://localhost:3000/users \
    -H "Content-Type: application/json" \
    -d '{"op":"get","key":"user:1"}' | python3 -m json.tool 2>/dev/null || true
echo ""

kill $SERVER_PID 2>/dev/null
wait $SERVER_PID 2>/dev/null || true

# Demo 6: Init new project
print_header "Demo 6: anp init"
print_cmd "anp init demo-app"
rm -rf demo-app
cargo run -p anp-cli --quiet -- init demo-app
echo ""
print_cmd "cd demo-app && anp run main.anp.json"
(cd demo-app && cargo run -p anp-cli --quiet -- run main.anp.json 2>/dev/null)
rm -rf demo-app

# Done
print_header "Demo Complete!"
echo "ANP v0.1 — No source code. Just graphs and a runtime."
echo ""
