import { useEffect, useState } from "react";
import { api, type AppInfo, type PackSummary } from "../api";
import { Banner, EmptyState, Icon, PageHeader } from "../components/ui";

export default function PacksPage() {
  const [packs, setPacks] = useState<PackSummary[] | null>(null);
  const [errors, setErrors] = useState<string[]>([]);
  const [info, setInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    api.listPacks().then((l) => {
      setPacks(l.packs);
      setErrors(l.errors);
    });
    api.appInfo().then(setInfo);
  }, []);

  return (
    <div className="page">
      <PageHeader
        title="Tests"
        subtitle="Quality test packs check how good a model's answers are: coding, reasoning, summaries and more."
      />

      {errors.map((e, i) => (
        <Banner key={i} icon="warning">
          <span style={{ whiteSpace: "pre-wrap" }}>{e}</span>
        </Banner>
      ))}

      {packs && packs.length === 0 && (
        <EmptyState icon="fact_check" title="No test packs installed yet">
          Built-in packs for coding, reasoning, knowledge and more are coming in an upcoming update. You can already add your
          own: put a pack folder in <code>{info?.packs_dir ?? "~/.infermark/packs"}</code>.
        </EmptyState>
      )}

      {packs && packs.length > 0 && (
        <div className="grid">
          {packs.map((p) => (
            <section className="card" key={p.id}>
              <div className="card-head">
                <span className="avatar">
                  <Icon name={categoryIcon(p.category)} />
                </span>
                <div style={{ minWidth: 0 }}>
                  <h2 className="title-medium">{p.name}</h2>
                  <p className="body-small muted">
                    {p.id} · v{p.version}
                  </p>
                </div>
              </div>
              {p.description && <p className="body-medium muted">{p.description}</p>}
              <div className="row" style={{ gap: 8 }}>
                <span className="chip">{p.category}</span>
                <span className="chip">{p.items} questions</span>
                {p.subjective && <span className="chip warn">Opinion-based</span>}
              </div>
            </section>
          ))}
        </div>
      )}

      <section className="card outlined">
        <div className="card-head">
          <span className="avatar">
            <Icon name="add" />
          </span>
          <h2 className="title-medium">Make your own test pack</h2>
        </div>
        <p className="body-medium muted">
          A pack is just a folder with two text files: <code>pack.toml</code> (name, category, how answers are scored) and{" "}
          <code>items.jsonl</code> (one question per line). No programming needed. Packs are data only and are never run as
          code.
        </p>
      </section>
    </div>
  );
}

function categoryIcon(category: string) {
  const icons: Record<string, string> = {
    coding: "code",
    reasoning: "psychology",
    knowledge: "school",
    summarization: "summarize",
    creative: "edit_note",
    "instruction-following": "rule",
    "structured-output": "data_object",
    "long-context": "article",
  };
  return icons[category] ?? "fact_check";
}
