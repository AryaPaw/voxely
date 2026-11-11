import { useId } from "react";
import { Switch } from "../ui/switch";

export function SettingsSwitchRow({
  label,
  checked,
  onCheckedChange,
  description,
}: {
  label: string;
  checked: boolean;
  onCheckedChange: (value: boolean) => void;
  description?: string;
}) {
  const id = useId();
  return (
    <div className="settings-switch-row">
      <div className="settings-switch-label min-w-0 space-y-1">
        <label htmlFor={id} className="text-sm">
          {label}
        </label>
        {description ? (
          <p id={`${id}-hint`} className="text-xs text-muted-foreground">
            {description}
          </p>
        ) : null}
      </div>
      <Switch
        id={id}
        checked={checked}
        onCheckedChange={onCheckedChange}
        aria-describedby={description ? `${id}-hint` : undefined}
      />
    </div>
  );
}
