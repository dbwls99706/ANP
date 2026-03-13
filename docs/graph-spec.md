# ANP Graph Specification v0.1

## Overview

An ANP Graph is a JSON document that defines a complete executable program as a directed acyclic graph (DAG) of nodes connected by edges.

## Top-Level Structure

```json
{
  "version": "0.1",
  "metadata": {
    "name": "My Application",
    "description": "Optional description"
  },
  "nodes": [...],
  "edges": [...]
}
```

## Nodes

Each node represents a unit of computation.

```json
{
  "id": "unique_id",
  "node_type": "transform",
  "kind": "processor",
  "config": { ... },
  "inputs": [{ "name": "default" }],
  "outputs": [{ "name": "default" }]
}
```

### Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | yes | Unique identifier |
| `node_type` | string | yes | Type of node (e.g., "console", "transform") |
| `kind` | string | yes | One of: "source", "processor", "sink", "service" |
| `config` | object | no | Node-specific configuration |
| `inputs` | array | no | Input port definitions |
| `outputs` | array | no | Output port definitions |

### Node Kinds

| Kind | Behavior |
|------|----------|
| `source` | Produces data. No incoming edges. Executes once. |
| `processor` | Transforms data. Has both inputs and outputs. |
| `sink` | Consumes data. No outgoing edges. |
| `service` | Long-running. Starts in `init()`, triggers pipeline on events. |

## Edges

Each edge connects an output port of one node to an input port of another.

```json
{
  "id": "e1",
  "from_node": "server",
  "from_port": "users_get",
  "to_node": "transform",
  "to_port": "default"
}
```

### Fields

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `id` | string | yes | - | Unique identifier for tracing |
| `from_node` | string | yes | - | Source node ID |
| `from_port` | string | no | "default" | Source output port |
| `to_node` | string | yes | - | Target node ID |
| `to_port` | string | no | "default" | Target input port |

## Validation Rules

1. No duplicate node IDs
2. All edge node references must exist
3. All edge port references must exist on the referenced nodes
4. Sink nodes cannot have outgoing edges
5. Service nodes cannot have incoming edges
6. The graph must be a DAG (no cycles)

## Data Flow

All data between nodes is `serde_json::Value` (dynamic JSON). Nodes receive input as a single JSON value and produce output as a map of port name → JSON value.

## Version History

| Version | Features |
|---------|----------|
| 0.1 | DataFlow edges, Source/Processor/Sink/Service nodes |
| 0.2 (planned) | Control edges, Event edges |
| 0.3 (planned) | Distributed runtime |
