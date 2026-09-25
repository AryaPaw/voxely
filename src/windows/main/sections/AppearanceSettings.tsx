import { toast } from "sonner";
import { PageHeader } from "../../../components/settings/PageHeader";
import { SettingsField } from "../../../components/settings/SettingsField";
import { SettingsSwitchRow } from "../../../components/settings/SettingsSwitchRow";
import { Button } from "../../../components/ui/button";
import { SimpleSelect } from "../../../components/ui/simple-select";
import { api, type AppSettings } from "../../../lib/api";
import { formatInvokeError, type Messages } from "../../../lib/i18n";
import { reportError } from "../../../lib/system-notify";
import { SECTION_ICONS } from "../sectionNav";

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
    <div>
      <PageHeader icon={SECTION_ICONS.appearance} title={copy.navAppearance} />
      <SettingsField label={copy.theme}>
        <SimpleSelect
          aria-label={copy.theme}
          value={settings.theme}
          onValueChange={(theme) => onChange({ theme })}
          options={[
            { value: "system", label: copy.themeSystem },
            { value: "light", label: copy.themeLight },
            { value: "dark", label: copy.themeDark },
          ]}
        />
      </SettingsField>
      <SettingsField label={copy.uiLanguage}>
        <SimpleSelect
          aria-label={copy.uiLanguage}
          value={settings.uiLanguage ?? "auto"}
          onValueChange={(uiLanguage) => onChange({ uiLanguage })}
          options={[
            { value: "auto", label: copy.uiAuto },
            { value: "ru", label: copy.uiRu },
            { value: "en", label: copy.uiEn },
          ]}
        />
      </SettingsField>
      <SettingsSwitchRow
        label={copy.autoUpdate}
        checked={settings.autoUpdateEnabled ?? true}
        onCheckedChange={(autoUpdateEnabled) => onChange({ autoUpdateEnabled })}
      />
      <div className="mt-2">
        <Button
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
      </div>
    </div>
  );
}
