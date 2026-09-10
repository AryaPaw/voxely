import { cloneElement, isValidElement, useId, type ReactElement, type ReactNode } from "react";
import { Label } from "../ui/label";

export function SettingsField({ label, children }: { label: string; children: ReactNode }) {
  const id = useId();
  const control = isValidElement(children)
    ? cloneElement(children as ReactElement<{ id?: string }>, { id })
    : children;
  return (
    <div className="mb-4 max-w-lg">
      <Label htmlFor={id} className="mb-1 text-sm">
        {label}
      </Label>
      {control}
    </div>
  );
}
