import type { Config } from "tailwindcss";

const config: Config = {
  content: [
    "./app/**/*.{js,ts,jsx,tsx,mdx}",
    "./components/**/*.{js,ts,jsx,tsx,mdx}",
  ],
  theme: {
    extend: {
      colors: {
        anp: {
          purple: "#764ba2",
          blue: "#667eea",
          dark: "#0f0f23",
          darker: "#0a0a1a",
        },
      },
    },
  },
  plugins: [],
};
export default config;
