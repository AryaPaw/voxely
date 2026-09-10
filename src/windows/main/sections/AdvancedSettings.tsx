import { toast } from "sonner";
import { api, type AppSettings } from "../../../lib/api";
import type { Messages } from "../../../lib/i18n";
import { PageHeader } from "../../../components/settings/PageHeader";
import { SettingsField } from "../../../components/settings/SettingsField";
import { SettingsSwitchRow } from "../../../components/settings/SettingsSwitchRow";
import { Button } from "../../../components/ui/button";
import { SimpleSelect } from "../../../components/ui/simple-select";
import { SECTION_ICONS } from "../sectionNav";

export function AdvancedSettings({
  settings,
  copy,
  onChange,
}: {
  settings: AppSettings;
  copy: Messages;
  onChange: (patch: Partial<AppSettings>) => void;
}) {
  return (
    <div>
      <PageHeader icon={SECTION_ICONS.advanced} title={copy.navAdvanced} />
      <SettingsField label={copy.insertMode}>
        <SimpleSelect
          aria-label={copy.insertMode}
          value={settings.insertionMode}
          onValueChange={(insertionMode) => onChange({ insertionMode })}
          options={[
            { value: "auto", label: copy.insertAuto },
            { value: "sendinput", label: copy.insertSendInput },
            { value: "clipboard", label: copy.insertClipboard },
          ]}
        />
      </SettingsField>
      <p className="mb-4 max-w-lg text-xs text-muted-foreground">{copy.insertHint}</p>
      <SettingsSwitchRow
        label={copy.debugLogs}
        checked={settings.debugLogging}
        onCheckedChange={(debugLogging) => onChange({ debugLogging })}
      />
      <Button
        variant="outline"
        onClick={() => {
          void api.openLogs().catch((error: unknown) => {
            toast.error(error instanceof Error ? error.message : copy.openLogs);
          });
        }}
      >
        {copy.openLogs}
      </Button>
    </div>
  );
}
