import { EmptyState, PageHeader } from "../components/ui";

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
