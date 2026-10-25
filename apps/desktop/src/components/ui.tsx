import type { ReactNode } from "react";

export function Icon({ name, filled, small }: { name: string; filled?: boolean; small?: boolean }) {
  const cls = ["icon", filled && "filled", small && "small"].filter(Boolean).join(" ");
  return (
    <span className={cls} aria-hidden="true">
      {name}
    </span>
  );
}

export function PageHeader({ title, subtitle, actions }: { title: string; subtitle?: string; actions?: ReactNode }) {
  return (
    <header className="page-header">
      <div className="titles">
        <h1 className="headline-small">{title}</h1>
        {subtitle && <p className="body-medium muted">{subtitle}</p>}
      </div>
      {actions && <div className="row">{actions}</div>}
    </header>
  );
}

export function EmptyState({
  icon,
  title,
  children,
  action,
}: {
  icon: string;
  title: string;
  children?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="empty">
      <Icon name={icon} />
      <h2 className="title-large" style={{ color: "var(--md-sys-color-on-surface)" }}>
        {title}
      </h2>
      {children && <p className="body-medium">{children}</p>}
      {action}
    </div>
  );
}

export function Banner({ icon = "info", error, children }: { icon?: string; error?: boolean; children: ReactNode }) {
  return (
    <div className={error ? "banner error" : "banner"} role={error ? "alert" : "status"}>
      <Icon name={icon} small />
      <div className="body-medium">{children}</div>
    </div>
  );
}

export function Progress({ value }: { value: number }) {
  const pct = Math.round(Math.min(Math.max(value, 0), 1) * 100);
  return (
    <div className="progress" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct}>
      <div style={{ width: `${pct}%` }} />
    </div>
  );
}
