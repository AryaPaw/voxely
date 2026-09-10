import { useId } from "react";
import { Switch } from "../ui/switch";

export function SettingsSwitchRow({
  label,
  checked,
  onCheckedChange,
}: {
  label: string;
  checked: boolean;
  onCheckedChange: (value: boolean) => void;
}) {
  const id = useId();
  return (
    <div className="mb-3 flex max-w-lg items-center justify-between gap-3">
      <label htmlFor={id} className="text-sm">
        {label}
      </label>
      <Switch id={id} checked={checked} onCheckedChange={onCheckedChange} />
    </div>
  );
}
