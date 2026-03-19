const papers = [
  {
    title: "First formal verification system for executable graphs",
    desc: "Proving correctness properties (type safety, completeness, acyclicity) on dataflow graphs before execution. No existing graph runtime offers mathematical guarantees.",
    tag: "Verification",
    tagColor: "bg-emerald-500/10 text-emerald-400 border-emerald-500/20",
  },
  {
    title: "First compiler optimization for graph-based programs",
    desc: "Applying classical compiler passes (dead code elimination, constant folding, operator fusion) to executable graph IRs. A new compilation target for AI-generated software.",
    tag: "Optimization",
    tagColor: "bg-amber-500/10 text-amber-400 border-amber-500/20",
  },
  {
    title: "Novel type system for dataflow graphs",
    desc: "A structural type system for port-connected nodes with compile-time inference. Ensures well-typed graph composition without manual annotation.",
    tag: "Type Theory",
    tagColor: "bg-violet-500/10 text-violet-400 border-violet-500/20",
  },
  {
    title: "AI-native programming: from intent to executable graphs",
    desc: "LLM-based program synthesis that targets graph IRs instead of source code. Eliminating the code intermediary between human intent and machine execution.",
    tag: "AI/PL",
    tagColor: "bg-anp-blue/10 text-anp-blue border-anp-blue/20",
  },
];

export default function Research() {
  return (
    <section className="py-32 px-6 bg-anp-darker/50">
      <div className="max-w-5xl mx-auto">
        <div className="text-center mb-16">
          <p className="text-anp-blue font-mono text-sm tracking-widest uppercase mb-4">
            Academic Contributions
          </p>
          <h2 className="text-3xl md:text-5xl font-bold mb-4">
            Research <span className="gradient-text">Frontiers</span>
          </h2>
          <p className="text-gray-400 text-lg max-w-2xl mx-auto">
            ANP introduces techniques with no prior art in graph-based runtimes.
            Each represents a publishable contribution to programming languages and software engineering.
          </p>
        </div>

        <div className="grid md:grid-cols-2 gap-6">
          {papers.map((paper, i) => (
            <div
              key={i}
              className="glass-card rounded-xl p-6 node-card relative overflow-hidden group"
            >
              {/* Subtle gradient accent on hover */}
              <div className="absolute inset-0 bg-gradient-to-br from-anp-blue/5 to-anp-purple/5 opacity-0 group-hover:opacity-100 transition-opacity" />

              <div className="relative z-10">
                <span
                  className={`inline-block px-3 py-1 rounded-full text-xs font-mono font-medium border mb-4 ${paper.tagColor}`}
                >
                  {paper.tag}
                </span>
                <h3 className="font-bold text-lg mb-3 leading-snug">
                  {paper.title}
                </h3>
                <p className="text-gray-400 text-sm leading-relaxed">
                  {paper.desc}
                </p>
              </div>
            </div>
          ))}
        </div>

        {/* Bottom callout */}
        <div className="mt-12 glass-card rounded-xl p-8 text-center border-anp-purple/20">
          <div className="flex items-center justify-center gap-3 mb-3">
            <svg width="24" height="24" viewBox="0 0 24 24" fill="none" className="text-anp-purple">
              <path d="M12 2L15.09 8.26L22 9.27L17 14.14L18.18 21.02L12 17.77L5.82 21.02L7 14.14L2 9.27L8.91 8.26L12 2Z" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" />
            </svg>
            <span className="text-lg font-semibold">Potential Paper Topics</span>
          </div>
          <p className="text-gray-400 text-sm max-w-xl mx-auto mb-6">
            Formal methods for graph-based programming, compiler IR design for AI-generated programs,
            type inference in dataflow systems, and LLM-targeted program representations.
          </p>
          <a
            href="https://github.com/dbwls99706/ANP"
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex items-center gap-2 px-6 py-3 rounded-lg bg-gradient-to-r from-anp-blue/20 to-anp-purple/20 border border-anp-purple/30 text-sm font-medium hover:border-anp-purple/50 transition-colors"
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none">
              <path d="M12 2C6.477 2 2 6.477 2 12C2 16.418 4.865 20.166 8.839 21.489C9.339 21.581 9.5 21.278 9.5 21.017V19.257C6.727 19.857 6.14 17.92 6.14 17.92C5.685 16.776 5.029 16.473 5.029 16.473C4.121 15.842 5.098 15.855 5.098 15.855C6.101 15.926 6.629 16.892 6.629 16.892C7.521 18.428 8.97 17.98 9.54 17.726C9.63 17.075 9.889 16.627 10.175 16.42C7.955 16.208 5.62 15.318 5.62 11.466C5.62 10.386 6.01 9.506 6.649 8.812C6.546 8.602 6.203 7.596 6.747 6.252C6.747 6.252 7.587 6.028 9.497 7.292C10.31 7.108 11.16 7.016 12.003 7.012C12.845 7.016 13.695 7.108 14.51 7.292C16.418 6.028 17.256 6.252 17.256 6.252C17.802 7.596 17.459 8.602 17.356 8.812C17.997 9.506 18.383 10.386 18.383 11.466C18.383 15.33 16.044 16.204 13.816 16.408C14.172 16.672 14.5 17.192 14.5 17.988V21.017C14.5 21.281 14.658 21.587 15.169 21.488C19.138 20.163 22 16.416 22 12C22 6.477 17.523 2 12 2Z" fill="currentColor" />
            </svg>
            Explore the Source
          </a>
        </div>
      </div>
    </section>
  );
}
