import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "ANP — AI Native Programming",
  description:
    "Software defined as executable graphs, not source code. Prompt → Graph → Run.",
  icons: {
    icon: "/icon.svg",
  },
  openGraph: {
    title: "ANP — AI Native Programming",
    description:
      "Software defined as executable graphs, not source code. AI generates the graphs; the runtime executes them directly.",
    type: "website",
  },
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en">
      <body className="antialiased">{children}</body>
    </html>
  );
}
