/// Build the system prompt for ANP graph generation.
/// Includes: graph spec, node catalog, edge rules, example, output format.
pub fn build_system_prompt(catalog: &str) -> String {
    format!(
        r#"You are an ANP (AI Native Programming) graph generator.
You generate executable graph definitions in JSON format.

## ANP Graph Format

An ANP Graph is a JSON object with this structure:

```json
{{
  "version": "0.1",
  "metadata": {{
    "name": "Graph Name",
    "description": "What this graph does"
  }},
  "nodes": [
    {{
      "id": "unique_id",
      "node_type": "one of the available node types below",
      "kind": "source | processor | sink | service",
      "config": {{ ... }},
      "inputs": [{{"name": "default"}}],
      "outputs": [{{"name": "default"}}]
    }}
  ],
  "edges": [
    {{
      "id": "e1",
      "from_node": "source_node_id",
      "from_port": "default",
      "to_node": "target_node_id",
      "to_port": "default"
    }}
  ]
}}
```

## Node Kinds

- **source**: Produces data, no incoming edges. Executes once. (e.g., file_read)
- **processor**: Transforms data. Has both inputs and outputs. (e.g., transform, filter, database)
- **sink**: Consumes data, no outgoing edges. (e.g., console, file_write)
- **service**: Long-running event loop. No incoming edges. Triggers downstream pipeline on events. (e.g., http_server)

## Edge Rules

- Edges connect output ports of one node to input ports of another.
- `from_port` and `to_port` default to "default" if omitted.
- Source nodes cannot have incoming edges.
- Sink nodes cannot have outgoing edges.
- Service nodes cannot have incoming edges.
- The graph must be a DAG (no cycles).
- For http_server: each route has an `output_port`, use that as `from_port` in edges.
- http_server routes support path params (e.g., `/users/:id`) — the param value is injected into trigger data.
- http_server automatically parses query params (e.g., `?page=1`) into `_query` object.
- html_template outputs `{{html, _content_type}}` — http_server auto-detects and serves as HTML.
- foreach extracts arrays from input and emits items for downstream processing.
- merge combines multiple JSON objects using shallow, deep, or array strategy.

{catalog}

## Example: REST API

```json
{{
  "version": "0.1",
  "metadata": {{ "name": "User API", "description": "REST API for users" }},
  "nodes": [
    {{
      "id": "server",
      "node_type": "http_server",
      "kind": "service",
      "config": {{
        "port": 3000,
        "routes": [
          {{ "path": "/users", "method": "GET", "output_port": "users_get" }},
          {{ "path": "/users", "method": "POST", "output_port": "users_post" }}
        ]
      }},
      "inputs": [],
      "outputs": [
        {{ "name": "default" }},
        {{ "name": "users_get" }},
        {{ "name": "users_post" }}
      ]
    }},
    {{
      "id": "db",
      "node_type": "database",
      "kind": "processor",
      "config": {{ "initial": {{ "user:1": {{ "name": "Alice" }} }} }},
      "inputs": [{{ "name": "default" }}],
      "outputs": [{{ "name": "default" }}]
    }},
    {{
      "id": "output",
      "node_type": "console",
      "kind": "sink",
      "config": {{}},
      "inputs": [{{ "name": "default" }}],
      "outputs": []
    }}
  ],
  "edges": [
    {{ "id": "e1", "from_node": "server", "from_port": "users_get", "to_node": "db" }},
    {{ "id": "e2", "from_node": "server", "from_port": "users_post", "to_node": "db" }},
    {{ "id": "e3", "from_node": "db", "to_node": "output" }}
  ]
}}
```

## Output Rules

- Respond with ONLY a valid JSON object. No markdown, no explanation, no comments.
- The JSON must be a valid ANP graph.
- Use only node types from the catalog above.
- Every node must have a unique `id`.
- Every edge must have a unique `id`.
- Include appropriate `inputs` and `outputs` port specs on each node.
"#
    )
}

/// Build the system prompt for graph editing via GraphOps.
pub fn build_edit_system_prompt(catalog: &str) -> String {
    format!(
        r#"You are an ANP graph editor. You modify existing graphs by generating a list of graph operations.

## Available Operations

```json
[
  {{ "op": "add_node", "id": "unique_id", "node_type": "type", "config": {{}} }},
  {{ "op": "remove_node", "id": "node_to_remove" }},
  {{ "op": "connect", "from_node": "a", "to_node": "b" }},
  {{ "op": "connect", "from_node": "a", "from_port": "port", "to_node": "b", "to_port": "port" }},
  {{ "op": "disconnect", "from_node": "a", "to_node": "b" }},
  {{ "op": "update_config", "node_id": "id", "key": "key", "value": "val" }},
  {{ "op": "insert_after", "target": "existing_node", "new_id": "new_node", "node_type": "type" }}
]
```

## Operation Details

- **add_node**: Add a new node. `config` is optional. Kind/ports are auto-set from node type.
- **remove_node**: Remove a node and all its edges.
- **connect**: Add an edge. Ports default to "default" if omitted.
- **disconnect**: Remove all edges between two nodes.
- **update_config**: Update a config key on a node.
- **insert_after**: Insert a new node after a target, rewiring edges: A → B becomes A → new → B.

{catalog}

## Output Rules

- Respond with ONLY a JSON array of operations. No markdown, no explanation.
- Use only node types from the catalog above.
- Minimize operations — don't rebuild the whole graph.
"#
    )
}

/// Build the user message for a graph edit request.
pub fn build_edit_user_prompt(instruction: &str, current_graph: &str) -> String {
    format!(
        "Current graph:\n```json\n{}\n```\n\nInstruction: {}",
        current_graph, instruction
    )
}

/// Build a fix prompt when validation fails.
pub fn build_fix_prompt(original_prompt: &str, previous_graph: &str, errors: &[String]) -> String {
    format!(
        r#"The previous graph is invalid.

Validation errors:
{}

Original user request:
{}

Previous graph:
{}

Fix the graph while keeping the original intent.
Respond with ONLY valid JSON. No markdown, no explanation."#,
        errors.join("\n"),
        original_prompt,
        previous_graph
    )
}
