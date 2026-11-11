import { cloneElement, isValidElement, useId, type ReactElement, type ReactNode } from "react";
import { Label } from "../ui/label";

export function SettingsField({
  label,
  children,
  description,
  error,
  action = false,
  align = "start",
}: {
  label: string;
  children: ReactNode;
  description?: ReactNode;
  error?: ReactNode;
  action?: boolean;
  align?: "start" | "center";
}) {
  const id = useId();
  const control =
    !action && isValidElement(children)
      ? cloneElement(children as ReactElement<{ id?: string; "aria-describedby"?: string }>, {
          id,
          "aria-describedby": description || error ? `${id}-hint` : undefined,
        })
      : children;
  return (
    <div
      className="settings-field"
      data-action={action || undefined}
      data-align={align}
      role={action ? "group" : undefined}
      aria-labelledby={action ? `${id}-label` : undefined}
      aria-describedby={action && description ? `${id}-description` : undefined}
    >
      {action ? (
        <div className="settings-action-label">
          <p id={`${id}-label`} className="text-sm font-medium">
            {label}
          </p>
          {description ? (
            <p id={`${id}-description`} className="mt-1 text-xs text-muted-foreground">
              {description}
            </p>
          ) : null}
        </div>
      ) : (
        <Label htmlFor={id} className="settings-field-label text-sm">
          {label}
        </Label>
      )}
      <div className="settings-field-control">
        {control}
        {(!action && description) || error ? (
          <p
            id={`${id}-hint`}
            className={`text-xs ${error ? "text-destructive" : "text-muted-foreground"}`}
            role={error ? "alert" : undefined}
          >
            {error || description}
          </p>
        ) : null}
      </div>
    </div>
  );
}
