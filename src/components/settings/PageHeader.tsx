import type { LucideIcon } from "lucide-react";

export function PageHeader({ icon: Icon, title }: { icon: LucideIcon; title: string }) {
  return (
    <h1 className="mb-4 flex items-center gap-2 text-lg font-medium">
      <Icon className="h-4 w-4" />
      {title}
    </h1>
  );
}
