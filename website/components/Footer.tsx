export default function Footer() {
  return (
    <footer className="py-16 px-6 border-t border-white/10">
      <div className="max-w-4xl mx-auto">
        {/* Quick start */}
        <div className="glass-card rounded-xl p-8 mb-12">
          <h3 className="text-2xl font-bold mb-6 text-center">Quick Start</h3>
          <div className="glass-card rounded-lg p-6 max-w-xl mx-auto">
            <pre className="font-mono text-sm text-gray-300 leading-relaxed">
              <span className="text-gray-500"># Clone and build</span>
              {"\n"}
              <span className="text-green-400">$</span> git clone https://github.com/dbwls99706/ANP.git
              {"\n"}
              <span className="text-green-400">$</span> cd ANP && cargo build
              {"\n\n"}
              <span className="text-gray-500"># Run your first graph</span>
              {"\n"}
              <span className="text-green-400">$</span> cargo run -p anp-cli -- run examples/hello.anp.json
              {"\n\n"}
              <span className="text-gray-500"># Generate from prompt</span>
              {"\n"}
              <span className="text-green-400">$</span> anp prompt &quot;REST API that stores user data&quot; --run
            </pre>
          </div>
        </div>

        {/* Footer links */}
        <div className="flex flex-col sm:flex-row items-center justify-between gap-4 text-gray-500 text-sm">
          <div className="flex items-center gap-2">
            <span className="font-bold gradient-text text-base">ANP</span>
            <span>AI Native Programming</span>
          </div>
          <div className="flex items-center gap-6">
            <a
              href="https://github.com/dbwls99706/ANP"
              target="_blank"
              rel="noopener noreferrer"
              className="hover:text-white transition-colors"
            >
              GitHub
            </a>
            <span>MIT License</span>
            <span>v0.7</span>
          </div>
        </div>
      </div>
    </footer>
  );
}
