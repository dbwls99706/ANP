const features = [
  {
    title: "AI-in-the-Graph",
    desc: "llm_call nodes let graphs think and reason. Programs that contain intelligence — not just logic.",
    icon: (
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
        <path d="M12 2L2 7L12 12L22 7L12 2Z" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" />
        <path d="M2 17L12 22L22 17" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" />
        <path d="M2 12L12 17L22 12" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" />
      </svg>
    ),
  },
  {
    title: "Agentic Workflows",
    desc: "tool_router lets AI choose its own actions autonomously. No if/else routing — the AI decides.",
    icon: (
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
        <circle cx="12" cy="12" r="3" stroke="currentColor" strokeWidth="2" />
        <path d="M12 1V5M12 19V23M23 12H19M5 12H1M20 4L17 7M7 17L4 20M20 20L17 17M7 7L4 4" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
      </svg>
    ),
  },
  {
    title: "No Compilation",
    desc: "Graphs are interpreted directly by the runtime. Change the JSON, run it. Instant feedback loop.",
    icon: (
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
        <path d="M13 2L3 14H12L11 22L21 10H12L13 2Z" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" />
      </svg>
    ),
  },
  {
    title: "103 Built-in Nodes",
    desc: "HTTP servers, databases, AI/ML, auth, data processing, UI — compose them into any application.",
    icon: (
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
        <rect x="3" y="3" width="7" height="7" rx="1" stroke="currentColor" strokeWidth="2" />
        <rect x="14" y="3" width="7" height="7" rx="1" stroke="currentColor" strokeWidth="2" />
        <rect x="3" y="14" width="7" height="7" rx="1" stroke="currentColor" strokeWidth="2" />
        <rect x="14" y="14" width="7" height="7" rx="1" stroke="currentColor" strokeWidth="2" />
      </svg>
    ),
  },
  {
    title: "Full-Stack Web Apps",
    desc: "html_template + static_file + http_server = complete web applications as graphs.",
    icon: (
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
        <rect x="2" y="3" width="20" height="14" rx="2" stroke="currentColor" strokeWidth="2" />
        <path d="M8 21H16M12 17V21" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
      </svg>
    ),
  },
  {
    title: "Container Ready",
    desc: "SIGTERM handling, graceful shutdown, connection draining. Deploy to any container platform.",
    icon: (
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none">
        <path d="M22 12C22 17.5 17.5 22 12 22C6.5 22 2 17.5 2 12C2 6.5 6.5 2 12 2C17.5 2 22 6.5 22 12Z" stroke="currentColor" strokeWidth="2" />
        <path d="M15 9L9 15M9 9L15 15" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
      </svg>
    ),
  },
];

export default function Features() {
  return (
    <section className="py-32 px-6 bg-anp-darker/50">
      <div className="max-w-5xl mx-auto">
        <h2 className="text-3xl md:text-5xl font-bold text-center mb-16">
          Key <span className="gradient-text">Features</span>
        </h2>

        <div className="grid sm:grid-cols-2 lg:grid-cols-3 gap-6">
          {features.map((feat) => (
            <div key={feat.title} className="glass-card rounded-xl p-6 node-card">
              <div className="w-12 h-12 rounded-lg bg-anp-blue/10 flex items-center justify-center text-anp-blue mb-4">
                {feat.icon}
              </div>
              <h3 className="font-bold text-lg mb-2">{feat.title}</h3>
              <p className="text-gray-400 text-sm leading-relaxed">{feat.desc}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}
