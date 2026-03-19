"use client";

import { useState, useEffect } from "react";

const COMMAND = "$ anp verify graph.json";
const OUTPUT_LINES = [
  "",
  "  Verifying graph.json...",
  "",
  "  [pass] Type checking: all 12 port connections valid",
  "  [pass] Dead node analysis: 0 unreachable nodes",
  "  [pass] Cycle detection: DAG property holds",
  "  [pass] Completeness: all required inputs satisfied",
  "",
  "  Result: Graph is formally correct.",
];

export default function Hero() {
  const [typed, setTyped] = useState("");
  const [outputIndex, setOutputIndex] = useState(-1);
  const [showCursor, setShowCursor] = useState(true);
  const [commandDone, setCommandDone] = useState(false);

  useEffect(() => {
    let i = 0;
    const interval = setInterval(() => {
      if (i < COMMAND.length) {
        setTyped(COMMAND.slice(0, i + 1));
        i++;
      } else {
        clearInterval(interval);
        setTimeout(() => setCommandDone(true), 400);
      }
    }, 40);
    return () => clearInterval(interval);
  }, []);

  useEffect(() => {
    if (!commandDone) return;
    let line = 0;
    const interval = setInterval(() => {
      if (line < OUTPUT_LINES.length) {
        setOutputIndex(line);
        line++;
      } else {
        clearInterval(interval);
      }
    }, 150);
    return () => clearInterval(interval);
  }, [commandDone]);

  useEffect(() => {
    const interval = setInterval(() => setShowCursor((c) => !c), 530);
    return () => clearInterval(interval);
  }, []);

  return (
    <section className="relative min-h-screen flex items-center justify-center overflow-hidden">
      {/* Background gradient */}
      <div className="absolute inset-0 bg-gradient-to-br from-anp-dark via-[#1a1040] to-anp-darker" />
      <div className="absolute inset-0 opacity-30">
        <div className="absolute top-1/4 left-1/4 w-96 h-96 bg-anp-blue rounded-full blur-[128px]" />
        <div className="absolute bottom-1/4 right-1/4 w-96 h-96 bg-anp-purple rounded-full blur-[128px]" />
      </div>

      <div className="relative z-10 text-center px-6 max-w-5xl mx-auto">
        <p className="text-anp-blue font-mono text-sm tracking-widest uppercase mb-6 animate-fade-in-up">
          AI Native Programming
        </p>

        <h1 className="text-5xl md:text-7xl font-black mb-6 animate-fade-in-up leading-tight">
          Software is a{" "}
          <span className="gradient-text">graph</span>.
          <br />
          Code is{" "}
          <span className="px-2 py-1 rounded-md bg-gradient-to-r from-anp-blue/20 to-anp-purple/20 border border-anp-purple/30 gradient-text">obsolete</span>.
        </h1>

        <p className="text-xl md:text-2xl text-gray-400 max-w-2xl mx-auto mb-12 animate-fade-in-up-delay">
          AI generates executable graphs. The runtime executes them directly.
          No code. No compilation. Just intent and execution.
        </p>

        {/* Terminal */}
        <div className="max-w-2xl mx-auto mb-12 animate-fade-in-up-delay-2">
          <div className="glass-card rounded-xl overflow-hidden">
            <div className="flex items-center gap-2 px-4 py-3 border-b border-white/10">
              <div className="w-3 h-3 rounded-full bg-red-500/80" />
              <div className="w-3 h-3 rounded-full bg-yellow-500/80" />
              <div className="w-3 h-3 rounded-full bg-green-500/80" />
              <span className="text-xs text-gray-500 ml-2 font-mono">terminal</span>
            </div>
            <div className="p-6 text-left font-mono text-sm md:text-base">
              <div>
                <span className="text-green-400">
                  {typed}
                  {!commandDone && (
                    <span className={`${showCursor ? "opacity-100" : "opacity-0"} transition-opacity`}>
                      |
                    </span>
                  )}
                </span>
              </div>
              {commandDone && outputIndex >= 0 && (
                <div className="mt-1">
                  {OUTPUT_LINES.slice(0, outputIndex + 1).map((line, i) => (
                    <div
                      key={i}
                      className={
                        line.includes("[pass]")
                          ? "text-green-400"
                          : line.includes("Result:")
                          ? "text-anp-blue font-semibold"
                          : "text-gray-400"
                      }
                    >
                      {line || "\u00A0"}
                    </div>
                  ))}
                  {outputIndex >= OUTPUT_LINES.length - 1 && (
                    <span className={`text-green-400 ${showCursor ? "opacity-100" : "opacity-0"} transition-opacity`}>
                      |
                    </span>
                  )}
                </div>
              )}
            </div>
          </div>
        </div>

        {/* Flow diagram */}
        <div className="flex items-center justify-center gap-4 md:gap-8 mb-12 animate-fade-in-up-delay-2">
          {["Prompt", "Graph", "Run"].map((step, i) => (
            <div key={step} className="flex items-center gap-4 md:gap-8">
              <div className="glass-card px-6 py-3 rounded-lg">
                <span className="font-mono font-semibold text-lg">{step}</span>
              </div>
              {i < 2 && (
                <svg width="24" height="24" viewBox="0 0 24 24" fill="none" className="text-anp-blue">
                  <path d="M5 12H19M19 12L12 5M19 12L12 19" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
                </svg>
              )}
            </div>
          ))}
        </div>

        {/* CTAs */}
        <div className="flex flex-col sm:flex-row gap-4 justify-center animate-fade-in-up-delay-2">
          <a
            href="https://anp-demo.fly.dev"
            target="_blank"
            rel="noopener noreferrer"
            className="px-8 py-4 rounded-lg bg-gradient-to-r from-anp-blue to-anp-purple text-white font-semibold text-lg hover:opacity-90 transition-opacity"
          >
            Try Live Demo
          </a>
          <a
            href="https://github.com/dbwls99706/ANP"
            target="_blank"
            rel="noopener noreferrer"
            className="px-8 py-4 rounded-lg glass-card text-white font-semibold text-lg hover:bg-white/10 transition-colors"
          >
            View on GitHub
          </a>
        </div>
      </div>
    </section>
  );
}
