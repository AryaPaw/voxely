import { contrastForeground } from "../../lib/accent-color";
import { Check, RotateCcw } from "lucide-react";
import type { CSSProperties } from "react";
import { Button } from "../ui/button";
import {
  ACCENT_FAMILIES,
  ACCENT_SHADES,
  accentPreset,
  DEFAULT_ACCENT,
  type AccentColor,
} from "../../lib/accent";

export function AccentPicker({
  value = DEFAULT_ACCENT,
  onChange,
  label,
  shadeLabel,
  resetLabel,
}: {
  value?: AccentColor;
  onChange: (value: AccentColor) => void;
  label: string;
  shadeLabel: string;
  resetLabel: string;
}) {
  return (
    <div className="space-y-3">
      <div role="group" aria-label={label} className="flex flex-wrap gap-1.5">
        {ACCENT_FAMILIES.map((family) => {
          const selected = value.family === family;
          const color = accentPreset({ family, shade: value.shade }).color;
          return (
            <Button
              type="button"
              key={family}
              variant="outline"
              size="icon-sm"
              aria-label={family}
              title={family}
              aria-pressed={selected}
              onClick={() => onChange({ family, shade: value.shade })}
              className={selected ? "ring-2 ring-ring ring-offset-2 ring-offset-background" : ""}
            >
              <span
                className="flex size-5 items-center justify-center rounded-full"
                style={{ background: color }}
              >
                {selected && (
                  <Check
                    className="size-3.5"
                    style={{
                      color: contrastForeground(color),
                    }}
                  />
                )}
              </span>
            </Button>
          );
        })}
      </div>
      <div role="group" aria-label={shadeLabel} className="flex flex-wrap items-center gap-2">
        <span className="text-xs text-muted-foreground">{shadeLabel}</span>
        {ACCENT_SHADES.map((shade) => {
          const palette = accentPreset({ family: value.family, shade });
          return (
            <Button
              type="button"
              key={shade}
              size="sm"
              variant="default"
              style={
                {
                  "--primary": palette.color,
                  "--primary-foreground": palette.dark["--primary-foreground"],
                  "--primary-hover": palette.color,
                } as CSSProperties
              }
              className={
                value.shade === shade ? "ring-2 ring-ring ring-offset-2 ring-offset-background" : ""
              }
              aria-pressed={value.shade === shade}
              onClick={() => onChange({ ...value, shade })}
            >
              {value.shade === shade && <Check aria-hidden="true" className="size-3" />}
              {shade}
            </Button>
          );
        })}
        <Button
          type="button"
          variant="ghost"
          size="sm"
          disabled={value.family === DEFAULT_ACCENT.family && value.shade === DEFAULT_ACCENT.shade}
          onClick={() => onChange({ ...DEFAULT_ACCENT })}
        >
          <RotateCcw aria-hidden="true" />
          {resetLabel}
        </Button>
      </div>
    </div>
  );
}
