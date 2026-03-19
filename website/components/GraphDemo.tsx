"use client";

import { useState } from "react";

const GRAPH_JSON = `{
  "version": "0.1",
  "metadata": {
    "name": "rest-api-with-params",
    "description": "REST API with path parameters"
  },
  "nodes": [
    {
      "id": "server",
      "node_type": "http_server",
      "kind": "service",
      "config": {
        "port": 3000,
        "routes": [
          { "path": "/", "method": "GET" },
          { "path": "/users", "method": "GET" },
          { "path": "/users/:id", "method": "GET" },
          { "path": "/users", "method": "POST" }
        ]
      }
    },
    {
      "id": "home_page",
      "node_type": "html_template",
      "kind": "processor"
    },
    {
      "id": "db",
      "node_type": "database",
      "kind": "processor"
    }
  ],
  "edges": [
    { "from_node": "server", "to_node": "home_page" },
    { "from_node": "server", "to_node": "db" }
  ]
}`;

interface GraphNode {
  id: string;
  type: string;
  kind: string;
  x: number;
  y: number;
}

const nodes: GraphNode[] = [
  { id: "server", type: "http_server", kind: "service", x: 100, y: 140 },
  { id: "home_page", type: "html_template", kind: "processor", x: 380, y: 60 },
  { id: "db", type: "database", kind: "processor", x: 380, y: 220 },
];

const edges = [
  { from: "server", to: "home_page", label: "GET /" },
  { from: "server", to: "db", label: "CRUD" },
];

const kindColors: Record<string, string> = {
  service: "#667eea",
  processor: "#764ba2",
  source: "#22c55e",
  sink: "#ef4444",
};

export default function GraphDemo() {
  const [active, setActive] = useState<string | null>(null);

  return (
    <section className="py-32 px-6 bg-anp-darker/50">
      <div className="max-w-6xl mx-auto">
        <h2 className="text-3xl md:text-5xl font-bold text-center mb-4">
          The Graph <span className="gradient-text">is</span> the Program
        </h2>
        <p className="text-gray-400 text-center text-lg mb-16 max-w-2xl mx-auto">
          A JSON graph definition replaces traditional source code. No syntax to learn, no boilerplate to write.
        </p>

        <div className="grid md:grid-cols-2 gap-8">
          {/* JSON panel */}
          <div className="glass-card rounded-xl overflow-hidden">
            <div className="flex items-center gap-2 px-4 py-3 border-b border-white/10">
              <div className="w-3 h-3 rounded-full bg-red-500/80" />
              <div className="w-3 h-3 rounded-full bg-yellow-500/80" />
              <div className="w-3 h-3 rounded-full bg-green-500/80" />
              <span className="text-xs text-gray-500 ml-2 font-mono">
                rest_api.anp.json
              </span>
            </div>
            <pre className="p-6 text-sm font-mono text-gray-300 overflow-x-auto leading-relaxed max-h-[480px] overflow-y-auto">
              {GRAPH_JSON}
            </pre>
          </div>

          {/* Visual graph */}
          <div className="glass-card rounded-xl p-6 flex items-center justify-center">
            <svg viewBox="0 0 560 320" className="w-full max-h-[480px]">
              {/* Edges */}
              {edges.map((edge) => {
                const from = nodes.find((n) => n.id === edge.from)!;
                const to = nodes.find((n) => n.id === edge.to)!;
                const fromX = from.x + 120;
                const fromY = from.y + 30;
                const toX = to.x;
                const toY = to.y + 30;
                const midX = (fromX + toX) / 2;
                return (
                  <g key={`${edge.from}-${edge.to}`}>
                    <path
                      d={`M${fromX},${fromY} C${midX},${fromY} ${midX},${toY} ${toX},${toY}`}
                      fill="none"
                      stroke={active === edge.from || active === edge.to ? "#667eea" : "#4a5568"}
                      strokeWidth="2"
                      strokeDasharray={active ? "none" : "none"}
                    />
                    <text x={midX} y={(fromY + toY) / 2 - 8} textAnchor="middle" className="text-xs fill-gray-500 font-mono">
                      {edge.label}
                    </text>
                    {/* Arrow */}
                    <circle cx={toX} cy={toY} r="4" fill={active === edge.to ? "#667eea" : "#4a5568"} />
                  </g>
                );
              })}

              {/* Nodes */}
              {nodes.map((node) => (
                <g
                  key={node.id}
                  onMouseEnter={() => setActive(node.id)}
                  onMouseLeave={() => setActive(null)}
                  className="cursor-pointer"
                >
                  <rect
                    x={node.x}
                    y={node.y}
                    width="140"
                    height="60"
                    rx="8"
                    fill={active === node.id ? `${kindColors[node.kind]}33` : "#1a1a2e"}
                    stroke={kindColors[node.kind]}
                    strokeWidth={active === node.id ? 2 : 1}
                  />
                  <text
                    x={node.x + 70}
                    y={node.y + 25}
                    textAnchor="middle"
                    className="text-sm fill-white font-semibold"
                    style={{ fontFamily: "'JetBrains Mono', monospace", fontSize: "12px" }}
                  >
                    {node.type}
                  </text>
                  <text
                    x={node.x + 70}
                    y={node.y + 45}
                    textAnchor="middle"
                    style={{ fontFamily: "'JetBrains Mono', monospace", fontSize: "10px", fill: kindColors[node.kind] }}
                  >
                    {node.kind}
                  </text>
                </g>
              ))}
            </svg>
          </div>
        </div>
      </div>
    </section>
  );
}
