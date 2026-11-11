import type { ReactNode } from "react";
export function SettingsGroup({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="settings-group" aria-label={title}>
      <h2 className="mb-1 text-sm font-semibold">{title}</h2>
      <div className="settings-group-rows">{children}</div>
    </section>
  );
}
