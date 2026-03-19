"use client";

import { useState } from "react";

interface Category {
  name: string;
  count: number;
  nodes: string[];
  color: string;
}

const categories: Category[] = [
  {
    name: "I/O & Core",
    count: 16,
    nodes: ["console", "file_read", "file_write", "transform", "filter", "foreach", "merge", "template", "template_engine", "conditional", "switch", "try_catch", "parallel", "accumulator", "debounce", "sleep"],
    color: "#667eea",
  },
  {
    name: "HTTP & Networking",
    count: 8,
    nodes: ["http_server", "http_client", "cors", "redirect", "webhook", "websocket", "sse", "static_file"],
    color: "#22d3ee",
  },
  {
    name: "Data Formats",
    count: 9,
    nodes: ["json_parse", "csv", "yaml", "xml", "toml", "markdown", "base64", "url_encode", "gzip"],
    color: "#a78bfa",
  },
  {
    name: "Data Processing",
    count: 8,
    nodes: ["diff", "group_by", "pivot", "lookup", "math", "array", "string", "regex"],
    color: "#34d399",
  },
  {
    name: "AI / ML",
    count: 8,
    nodes: ["llm_call", "tool_router", "embedding", "sentiment", "summarize", "classify", "extract", "translate"],
    color: "#f472b6",
  },
  {
    name: "AI Workflow",
    count: 2,
    nodes: ["prompt_chain", "agent_loop"],
    color: "#fb923c",
  },
  {
    name: "Auth & Security",
    count: 9,
    nodes: ["jwt", "api_key", "password", "totp", "encrypt", "hash", "sanitize", "aes", "oauth"],
    color: "#ef4444",
  },
  {
    name: "State & Storage",
    count: 10,
    nodes: ["database", "memory", "cache", "queue", "priority_queue", "session", "kv_store", "counter", "redis", "sql_query"],
    color: "#facc15",
  },
  {
    name: "Resilience & Flow",
    count: 9,
    nodes: ["rate_limit", "retry", "circuit_breaker", "semaphore", "deduplicate", "throttle", "batch", "cron", "aggregate"],
    color: "#2dd4bf",
  },
  {
    name: "UI & Frontend",
    count: 8,
    nodes: ["html_template", "form", "chart", "table", "markdown_render", "paginate", "breadcrumb", "layout"],
    color: "#818cf8",
  },
  {
    name: "Observability",
    count: 8,
    nodes: ["log", "validate", "random", "uuid", "timestamp", "env", "health_check", "request_id"],
    color: "#4ade80",
  },
  {
    name: "Integration",
    count: 3,
    nodes: ["email", "image_resize", "multipart"],
    color: "#f97316",
  },
];

export default function NodeCatalog() {
  const [open, setOpen] = useState<string | null>(null);
  const total = categories.reduce((sum, c) => sum + c.count, 0);

  return (
    <section className="py-32 px-6">
      <div className="max-w-5xl mx-auto">
        <h2 className="text-3xl md:text-5xl font-bold text-center mb-4">
          <span className="gradient-text">{total}</span> Built-in Nodes
        </h2>
        <p className="text-gray-400 text-center text-lg mb-16 max-w-2xl mx-auto">
          {categories.length} categories of composable nodes. Each does one thing well — the graph wires them together.
        </p>

        <div className="grid sm:grid-cols-2 lg:grid-cols-3 gap-4">
          {categories.map((cat) => (
            <div
              key={cat.name}
              className="glass-card rounded-xl overflow-hidden node-card cursor-pointer"
              onClick={() => setOpen(open === cat.name ? null : cat.name)}
            >
              <div className="p-5 flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <div
                    className="w-3 h-3 rounded-full shrink-0"
                    style={{ backgroundColor: cat.color }}
                  />
                  <span className="font-semibold">{cat.name}</span>
                </div>
                <div className="flex items-center gap-2">
                  <span
                    className="text-xs font-mono px-2 py-1 rounded-full"
                    style={{ backgroundColor: `${cat.color}20`, color: cat.color }}
                  >
                    {cat.count}
                  </span>
                  <svg
                    width="16"
                    height="16"
                    viewBox="0 0 24 24"
                    fill="none"
                    className={`text-gray-500 transition-transform ${
                      open === cat.name ? "rotate-180" : ""
                    }`}
                  >
                    <path d="M6 9L12 15L18 9" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
                  </svg>
                </div>
              </div>
              {open === cat.name && (
                <div className="px-5 pb-5 flex flex-wrap gap-2">
                  {cat.nodes.map((node) => (
                    <span
                      key={node}
                      className="text-xs font-mono px-3 py-1.5 rounded-lg bg-white/5 text-gray-300"
                    >
                      {node}
                    </span>
                  ))}
                </div>
              )}
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
