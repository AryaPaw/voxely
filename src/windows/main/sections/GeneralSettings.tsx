import type { AppSettings } from "../../../lib/api";
import type { Messages } from "../../../lib/i18n";
import { HotkeyCapture } from "../../../components/settings/HotkeyCapture";
import { PageHeader } from "../../../components/settings/PageHeader";
import { SettingsField } from "../../../components/settings/SettingsField";
import { SettingsSwitchRow } from "../../../components/settings/SettingsSwitchRow";
import { SECTION_ICONS } from "../sectionNav";

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
      <SettingsField label="Глобальный хоткей">
        <HotkeyCapture value={settings.hotkey} onChange={(hotkey) => onChange({ hotkey })} />
      </SettingsField>
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
      <SettingsSwitchRow
        label={copy.notifications}
        checked={settings.notifications}
        onCheckedChange={(notifications) => onChange({ notifications })}
      />
    </div>
  );
}
