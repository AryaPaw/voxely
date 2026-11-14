import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { ComponentProps } from "react";
import { cn } from "@/lib/utils";

export function SimpleSelect({
  value,
  onValueChange,
  options,
  className,
  ...triggerProps
}: {
  value: string;
  onValueChange: (value: string) => void;
  options: Array<{ value: string; label: string }>;
} & Omit<ComponentProps<typeof SelectTrigger>, "value" | "onChange" | "children">) {
  return (
    <Select value={value} onValueChange={onValueChange}>
      <SelectTrigger {...triggerProps} className={cn("w-full", className)}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {options.map((option) => (
          <SelectItem key={option.value} value={option.value}>
            {option.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
