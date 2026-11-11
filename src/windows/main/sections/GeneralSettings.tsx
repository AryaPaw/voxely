import type { AppSettings } from "../../../lib/api";
import type { Messages } from "../../../lib/i18n";
import { AppearanceSettings, ResetWindowButton } from "./AppearanceSettings";
import { SettingsGroup } from "../../../components/settings/SettingsGroup";
import { UpdateSettings } from "./UpdateSettings";
import { PageHeader } from "../../../components/settings/PageHeader";
import { SettingsField } from "../../../components/settings/SettingsField";
import { SettingsSwitchRow } from "../../../components/settings/SettingsSwitchRow";
import { SECTION_ICONS } from "../sectionNav";
import { SimpleSelect } from "../../../components/ui/simple-select";

export function GeneralSettings({
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
      <PageHeader icon={SECTION_ICONS.general} title={copy.navGeneral} />
      <SettingsGroup title={copy.settingsInterface}>
        <AppearanceSettings settings={settings} copy={copy} onChange={onChange} />
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
      </SettingsGroup>
      <SettingsGroup title={copy.settingsBehavior}>
        <SettingsSwitchRow
          label={copy.startWithWindows}
          checked={settings.startWithWindows}
          onCheckedChange={(startWithWindows) => onChange({ startWithWindows })}
        />
        <SettingsSwitchRow
          label={copy.closeToTray}
          checked={settings.closeToTray}
          onCheckedChange={(closeToTray) => onChange({ closeToTray })}
        />
        <SettingsField label={copy.settingsWindowSize} action>
          <ResetWindowButton copy={copy} />
        </SettingsField>
        <SettingsSwitchRow
          label={copy.notifications}
          checked={settings.notifications}
          onCheckedChange={(notifications) => onChange({ notifications })}
        />
      </SettingsGroup>
      <SettingsGroup title={copy.settingsUpdates}>
        <SettingsSwitchRow
          label={copy.autoUpdate}
          checked={settings.autoUpdateEnabled ?? true}
          onCheckedChange={(autoUpdateEnabled) => onChange({ autoUpdateEnabled })}
        />
        <SettingsField label={copy.settingsManualUpdate} action>
          <UpdateSettings copy={copy} />
        </SettingsField>
      </SettingsGroup>
    </div>
  );
}
