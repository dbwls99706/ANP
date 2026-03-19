export default function Comparison() {
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

  return (
    <section className="py-32 px-6">
      <div className="max-w-4xl mx-auto">
        <h2 className="text-3xl md:text-5xl font-bold text-center mb-4">
          What Makes ANP <span className="gradient-text">Different</span>
        </h2>
        <p className="text-gray-400 text-center text-lg mb-16 max-w-2xl mx-auto">
          Why write code if AI generates it? Skip the intermediate step entirely.
        </p>

        <div className="space-y-6">
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
      </div>
    </section>
  );
}
