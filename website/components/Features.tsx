const features = [
  {
    title: "Formal Verification",
    desc: "anp verify proves graph correctness mathematically. Type safety, completeness, and acyclicity — guaranteed before execution.",
    icon: (
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
        <path d="M9 12L11 14L15 10" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
        <path d="M12 2L3 7V12C3 16.97 7.02 21.64 12 22.5C16.98 21.64 21 16.97 21 12V7L12 2Z" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" />
      </svg>
    ),
    gradient: "from-emerald-500/20 to-emerald-700/10",
    accent: "text-emerald-400",
    border: "hover:border-emerald-500/30",
  },
  {
    title: "Graph Optimizer",
    desc: "Compiler-like optimization passes: dead node elimination, transform fusion, constant folding. Faster graphs, automatically.",
    icon: (
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
        <path d="M13 2L3 14H12L11 22L21 10H12L13 2Z" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" />
      </svg>
    ),
    gradient: "from-amber-500/20 to-amber-700/10",
    accent: "text-amber-400",
    border: "hover:border-amber-500/30",
  },
  {
    title: "AOT Compilation",
    desc: "Pre-computed execution plans with resolved dependencies and optimized scheduling. Instant startup, zero overhead.",
    icon: (
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
        <path d="M12 2V6M12 18V22M6 12H2M22 12H18" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
        <circle cx="12" cy="12" r="4" stroke="currentColor" strokeWidth="2" />
        <circle cx="12" cy="12" r="9" stroke="currentColor" strokeWidth="1.5" strokeDasharray="4 3" />
      </svg>
    ),
    gradient: "from-cyan-500/20 to-cyan-700/10",
    accent: "text-cyan-400",
    border: "hover:border-cyan-500/30",
  },
  {
    title: "Type System",
    desc: "Compile-time type checking for port connections. Catch incompatible wiring before runtime — not during.",
    icon: (
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
        <path d="M7 8L3 12L7 16" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
        <path d="M17 8L21 12L17 16" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
        <path d="M14 4L10 20" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
      </svg>
    ),
    gradient: "from-violet-500/20 to-violet-700/10",
    accent: "text-violet-400",
    border: "hover:border-violet-500/30",
  },
  {
    title: "Semantic Diffing",
    desc: "anp diff understands graph structure, not text. Meaningful diffs that show node additions, rewirings, and config changes.",
    icon: (
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
        <rect x="3" y="3" width="8" height="8" rx="1" stroke="currentColor" strokeWidth="2" />
        <rect x="13" y="13" width="8" height="8" rx="1" stroke="currentColor" strokeWidth="2" />
        <path d="M11 7H13M17 11V13" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
        <path d="M7 11V13M11 17H13" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeDasharray="2 2" />
      </svg>
    ),
    gradient: "from-rose-500/20 to-rose-700/10",
    accent: "text-rose-400",
    border: "hover:border-rose-500/30",
  },
  {
    title: "AI-Native Runtime",
    desc: "104 nodes implemented in Rust. Parallel stage execution, zero garbage collection, O(1) input gathering. Raw performance.",
    icon: (
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
        <path d="M12 2L2 7L12 12L22 7L12 2Z" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" />
        <path d="M2 17L12 22L22 17" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" />
        <path d="M2 12L12 17L22 12" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" />
      </svg>
    ),
    gradient: "from-anp-blue/20 to-anp-purple/10",
    accent: "text-anp-blue",
    border: "hover:border-anp-blue/30",
  },
];

export default function Features() {
  return (
    <section className="py-32 px-6 bg-anp-darker/50">
      <div className="max-w-5xl mx-auto">
        <h2 className="text-3xl md:text-5xl font-bold text-center mb-4">
          What No Competitor <span className="gradient-text">Has</span>
        </h2>
        <p className="text-gray-400 text-center text-lg mb-16 max-w-2xl mx-auto">
          ANP is the first graph runtime with formal verification, compiler optimizations, and a type system.
        </p>

        <div className="grid sm:grid-cols-2 lg:grid-cols-3 gap-6">
          {features.map((feat) => (
            <div
              key={feat.title}
              className={`glass-card rounded-xl p-6 node-card transition-all ${feat.border}`}
            >
              <div
                className={`w-12 h-12 rounded-lg bg-gradient-to-br ${feat.gradient} flex items-center justify-center ${feat.accent} mb-4`}
              >
                {feat.icon}
              </div>
              <h3 className={`font-bold text-lg mb-2 ${feat.accent}`}>{feat.title}</h3>
              <p className="text-gray-400 text-sm leading-relaxed">{feat.desc}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
