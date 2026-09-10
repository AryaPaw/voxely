import type { ReactNode } from "react";
import { Label } from "../ui/label";

export function SettingsField({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="mb-4 max-w-lg">
      <Label className="mb-1 text-sm">{label}</Label>
      {children}
    </div>
  );
}
