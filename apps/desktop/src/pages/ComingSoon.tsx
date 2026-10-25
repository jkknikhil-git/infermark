import { EmptyState, PageHeader } from "../components/ui";

export function ModelsPage() {
  return (
    <div className="page">
      <PageHeader title="Models" subtitle="Download, manage and choose the local AI models to benchmark." />
      <EmptyState icon="deployed_code" title="Model library is on its way">
        Soon you'll browse open models (Llama, Qwen, Gemma, Phi and more), see which ones fit this device and why, and download
        them with one click. Models you already have in Ollama will show up here too.
      </EmptyState>
    </div>
  );
}

export function ComparePage() {
  return (
    <div className="page">
      <PageHeader title="Compare" subtitle="Put two results side by side." />
      <EmptyState icon="compare_arrows" title="Comparisons are coming">
        Compare runs before and after a driver update, two models on the same device, or your device against another.
      </EmptyState>
    </div>
  );
}
