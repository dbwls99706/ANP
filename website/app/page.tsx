import Hero from "@/components/Hero";
import Comparison from "@/components/Comparison";
import Features from "@/components/Features";
import GraphDemo from "@/components/GraphDemo";
import NodeCatalog from "@/components/NodeCatalog";
import DemoSection from "@/components/DemoSection";
import Architecture from "@/components/Architecture";
import Footer from "@/components/Footer";

export default function Home() {
  return (
    <main>
      <Hero />
      <Comparison />
      <Features />
      <GraphDemo />
      <NodeCatalog />
      <DemoSection />
      <Architecture />
      <Footer />
    </main>
  );
}
