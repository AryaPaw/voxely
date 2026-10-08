import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

export function PageHeader({
  icon: Icon,
  title,
  description,
  actions,
}: {
  icon: LucideIcon;
  title: string;
  description?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <header className="mb-5 flex flex-wrap items-start justify-between gap-x-6 gap-y-3">
      <div className="flex min-w-0 items-start gap-3">
        <span className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
          <Icon className="size-4" aria-hidden="true" />
        </span>
        <div className="min-w-0">
          <div className="flex min-h-7 items-center">
            <h1 className="text-xl leading-7 font-semibold tracking-tight">{title}</h1>
          </div>
          {description ? (
            <div className="mt-1 text-sm leading-5 text-muted-foreground">{description}</div>
          ) : null}
        </div>
      </div>
      {actions ? (
        <div className="flex shrink-0 flex-wrap items-center gap-1.5">{actions}</div>
      ) : null}
    </header>
  );
}
