const layers = [
  {
    label: "Prompt",
    sub: "Human intent",
    color: "#667eea",
  },
  {
    label: "Planner",
    sub: "LLM generates graph",
    color: "#7c5cbf",
  },
  {
    label: "Graph",
    sub: "JSON specification",
    color: "#764ba2",
  },
  {
    label: "Runtime",
    sub: "Execute directly",
    color: "#9333ea",
  },
];

const crates = [
  { name: "anp-cli", desc: "CLI interface" },
  { name: "anp-planner", desc: "LLM graph generation" },
  { name: "anp-core", desc: "Graph spec, runtime, scheduler" },
  { name: "anp-nodes", desc: "104 built-in nodes" },
];

export default function Architecture() {
  return (
    <section className="py-32 px-6">
      <div className="max-w-5xl mx-auto">
        <h2 className="text-3xl md:text-5xl font-bold text-center mb-16">
          <span className="gradient-text">Architecture</span>
        </h2>

        {/* Flow */}
        <div className="flex flex-col md:flex-row items-center justify-center gap-4 md:gap-0 mb-20">
          {layers.map((layer, i) => (
            <div key={layer.label} className="flex items-center">
              <div
                className="glass-card rounded-xl px-8 py-6 text-center min-w-[160px]"
                style={{ borderColor: `${layer.color}50` }}
              >
                <div className="font-bold text-lg" style={{ color: layer.color }}>
                  {layer.label}
                </div>
                <div className="text-gray-500 text-sm mt-1">{layer.sub}</div>
              </div>
              {i < layers.length - 1 && (
                <svg width="40" height="24" viewBox="0 0 40 24" fill="none" className="hidden md:block mx-2">
                  <path d="M0 12H32M32 12L24 4M32 12L24 20" stroke="#4a5568" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
                </svg>
              )}
            </div>
          ))}
        </div>

        {/* Crates */}
        <div className="grid sm:grid-cols-2 lg:grid-cols-4 gap-4">
          {crates.map((crate) => (
            <div key={crate.name} className="glass-card rounded-xl p-6 text-center node-card">
              <div className="font-mono font-semibold text-anp-blue mb-2">
                {crate.name}
              </div>
              <div className="text-gray-400 text-sm">{crate.desc}</div>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
