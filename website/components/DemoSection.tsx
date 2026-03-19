const endpoints = [
  {
    method: "GET",
    path: "/",
    description: "HTML home page",
    curl: "curl https://anp-demo.fly.dev/",
    color: "text-green-400",
  },
  {
    method: "GET",
    path: "/users",
    description: "List all users (JSON)",
    curl: "curl https://anp-demo.fly.dev/users",
    color: "text-green-400",
  },
  {
    method: "GET",
    path: "/users/:id",
    description: "Get user by ID",
    curl: "curl https://anp-demo.fly.dev/users/1",
    color: "text-green-400",
  },
  {
    method: "POST",
    path: "/users",
    description: "Create new user",
    curl: `curl -X POST https://anp-demo.fly.dev/users \\
  -H "Content-Type: application/json" \\
  -d '{"name":"Dave","email":"dave@example.com"}'`,
    color: "text-yellow-400",
  },
];

export default function DemoSection() {
  return (
    <section className="py-32 px-6 bg-anp-darker/50">
      <div className="max-w-4xl mx-auto">
        <h2 className="text-3xl md:text-5xl font-bold text-center mb-4">
          Live <span className="gradient-text">Demo</span>
        </h2>
        <p className="text-gray-400 text-center text-lg mb-4 max-w-2xl mx-auto">
          A REST API running on Fly.io — defined entirely as an ANP graph. Zero lines of code.
        </p>
        <p className="text-gray-500 text-center text-sm mb-16">
          First request may take a few seconds (cold start from free tier auto-stop).
        </p>

        <div className="space-y-4">
          {endpoints.map((ep) => (
            <div key={ep.path + ep.method} className="glass-card rounded-xl p-6">
              <div className="flex items-start justify-between mb-3">
                <div className="flex items-center gap-3">
                  <span
                    className={`font-mono font-bold text-sm px-3 py-1 rounded-lg ${
                      ep.method === "GET"
                        ? "bg-green-500/10 text-green-400"
                        : "bg-yellow-500/10 text-yellow-400"
                    }`}
                  >
                    {ep.method}
                  </span>
                  <span className="font-mono text-white">{ep.path}</span>
                </div>
                <span className="text-gray-500 text-sm">{ep.description}</span>
              </div>
              <pre className="text-sm font-mono text-gray-400 bg-black/30 rounded-lg p-4 overflow-x-auto">
                {ep.curl}
              </pre>
            </div>
          ))}
        </div>

        <div className="mt-8 text-center">
          <a
            href="https://anp-demo.fly.dev"
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex items-center gap-2 px-6 py-3 rounded-lg bg-gradient-to-r from-anp-blue to-anp-purple text-white font-semibold hover:opacity-90 transition-opacity"
          >
            Open Live Demo
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none">
              <path d="M7 17L17 7M17 7H7M17 7V17" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </a>
        </div>
      </div>
    </section>
  );
}
