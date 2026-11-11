import { AccentPicker } from "../../../components/settings/AccentPicker";
import { toast } from "sonner";
import { SettingsField } from "../../../components/settings/SettingsField";
import { Button } from "../../../components/ui/button";
import { Monitor, Sun, Moon } from "lucide-react";
import { RadioGroup, RadioGroupItem } from "../../../components/ui/radio-group";
import { api, type AppSettings } from "../../../lib/api";
import { formatInvokeError, type Messages } from "../../../lib/i18n";
import { reportError } from "../../../lib/system-notify";

export function AppearanceSettings({
  settings,
  copy,
  onChange,
}: {
  settings: AppSettings;
  copy: Messages;
  onChange: (patch: Partial<AppSettings>) => void;
}) {
  return (
    <>
      <SettingsField label={copy.theme} align="center">
        <RadioGroup
          aria-label={copy.theme}
          value={settings.theme}
          onValueChange={(theme) => onChange({ theme })}
          className="grid grid-cols-3 gap-2"
        >
          {[
            { value: "system", label: copy.themeSystem, Icon: Monitor },
            { value: "light", label: copy.themeLight, Icon: Sun },
            { value: "dark", label: copy.themeDark, Icon: Moon },
          ].map(({ value, label, Icon }) => (
            <div
              key={value}
              className={`relative has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring flex cursor-pointer flex-col items-center gap-2 rounded-lg border px-2 py-3 text-xs transition-colors ${settings.theme === value ? "border-ring bg-primary/10 text-foreground" : "border-border text-muted-foreground hover:bg-muted"}`}
            >
              <Icon className="size-4" aria-hidden="true" />
              <span>{label}</span>
              <RadioGroupItem
                className="absolute inset-0 size-full aspect-auto rounded-lg opacity-0 after:hidden"
                value={value}
                aria-label={label}
              />
            </div>
          ))}
        </RadioGroup>
      </SettingsField>
      <SettingsField label={copy.primaryColor} description={copy.primaryColorHint}>
        <AccentPicker
          value={settings.accentColor}
          onChange={(accentColor) => onChange({ accentColor })}
          label={copy.primaryColor}
          shadeLabel={copy.primaryShade}
          resetLabel={copy.resetAccentColor}
        />
      </SettingsField>
    </>
  );
}

export function ResetWindowButton({ copy, id }: { copy: Messages; id?: string }) {
  return (
    <Button
      id={id}
      aria-label={copy.resetWindow}
      type="button"
      variant="outline"
      onClick={() => {
        void api
          .resetMainWindow()
          .then(() => toast.success(copy.resetWindowDone))
          .catch((error: unknown) => reportError(formatInvokeError(error, copy)));
      }}
    >
      {copy.resetWindow}
    </Button>
  );
}
