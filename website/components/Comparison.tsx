const rows = [
  {
    label: "Traditional",
    steps: ["Code", "Compile", "Run"],
    color: "text-gray-500",
  },
  {
    label: "AI coding tools",
    steps: ["Prompt", "Code", "Compile", "Run"],
    color: "text-yellow-400",
  },
  {
    label: "ANP",
    steps: ["Prompt", "Graph", "Run"],
    color: "text-anp-blue",
    highlight: true,
  },
];

const competitorFeatures = [
  { feature: "Formal Verification", anp: true, nodeRed: false, n8n: false, langflow: false },
  { feature: "Graph Optimizer", anp: true, nodeRed: false, n8n: false, langflow: false },
  { feature: "AOT Compilation", anp: true, nodeRed: false, n8n: false, langflow: false },
  { feature: "Type System", anp: true, nodeRed: false, n8n: false, langflow: false },
  { feature: "Parallel Execution", anp: true, nodeRed: false, n8n: false, langflow: false },
  { feature: "AI-Native Nodes", anp: true, nodeRed: false, n8n: false, langflow: true },
];

function Check() {
  return (
    <svg width="20" height="20" viewBox="0 0 20 20" fill="none" className="text-emerald-400 mx-auto">
      <path d="M5 10L8.5 13.5L15 7" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function Cross() {
  return (
    <svg width="20" height="20" viewBox="0 0 20 20" fill="none" className="text-gray-600 mx-auto">
      <path d="M6 6L14 14M14 6L6 14" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
    </svg>
  );
}

export default function Comparison() {
  return (
    <section className="py-32 px-6">
      <div className="max-w-4xl mx-auto">
        <h2 className="text-3xl md:text-5xl font-bold text-center mb-4">
          What Makes ANP <span className="gradient-text">Different</span>
        </h2>
        <p className="text-gray-400 text-center text-lg mb-16 max-w-2xl mx-auto">
          Why write code if AI generates it? Skip the intermediate step entirely.
        </p>

        <div className="space-y-6 mb-24">
          {rows.map((row) => (
            <div
              key={row.label}
              className={`glass-card rounded-xl p-6 flex flex-col sm:flex-row items-start sm:items-center gap-4 ${
                row.highlight ? "border-anp-blue/50 ring-1 ring-anp-blue/20" : ""
              }`}
            >
              <span className={`font-mono font-semibold w-40 shrink-0 ${row.color}`}>
                {row.label}
              </span>
              <div className="flex items-center gap-3 flex-wrap">
                {row.steps.map((step, i) => (
                  <div key={step} className="flex items-center gap-3">
                    <span
                      className={`px-4 py-2 rounded-lg font-mono text-sm ${
                        row.highlight
                          ? "bg-anp-blue/20 text-anp-blue border border-anp-blue/30"
                          : "bg-white/5 text-gray-300"
                      }`}
                    >
                      {step}
                    </span>
                    {i < row.steps.length - 1 && (
                      <span className="text-gray-600">&rarr;</span>
                    )}
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>

        {/* Competitor comparison table */}
        <h3 className="text-2xl md:text-3xl font-bold text-center mb-4">
          Competitor <span className="gradient-text">Comparison</span>
        </h3>
        <p className="text-gray-400 text-center text-sm mb-10 max-w-xl mx-auto">
          ANP is the only graph runtime with formal verification, compiler optimizations, and a type system.
        </p>

        <div className="glass-card rounded-xl overflow-hidden">
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-white/10">
                  <th className="text-left px-6 py-4 font-semibold text-gray-300">Feature</th>
                  <th className="px-6 py-4 font-semibold text-center">
                    <span className="gradient-text font-bold">ANP</span>
                  </th>
                  <th className="px-6 py-4 font-semibold text-gray-400 text-center">Node-RED</th>
                  <th className="px-6 py-4 font-semibold text-gray-400 text-center">n8n</th>
                  <th className="px-6 py-4 font-semibold text-gray-400 text-center">Langflow</th>
                </tr>
              </thead>
              <tbody>
                {competitorFeatures.map((row, i) => (
                  <tr
                    key={row.feature}
                    className={`border-b border-white/5 ${
                      i % 2 === 0 ? "bg-white/[0.02]" : ""
                    }`}
                  >
                    <td className="px-6 py-4 font-medium text-gray-200">{row.feature}</td>
                    <td className="px-6 py-4">
                      <div className="flex justify-center">
                        <div className="w-7 h-7 rounded-full bg-emerald-500/10 flex items-center justify-center">
                          <Check />
                        </div>
                      </div>
                    </td>
                    <td className="px-6 py-4">
                      <div className="flex justify-center">{row.nodeRed ? <Check /> : <Cross />}</div>
                    </td>
                    <td className="px-6 py-4">
                      <div className="flex justify-center">{row.n8n ? <Check /> : <Cross />}</div>
                    </td>
                    <td className="px-6 py-4">
                      <div className="flex justify-center">{row.langflow ? <Check /> : <Cross />}</div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </section>
  );
}
