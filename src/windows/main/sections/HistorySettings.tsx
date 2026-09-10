import type { AppSettings } from "../../../lib/api";
import type { Messages } from "../../../lib/i18n";
import { PageHeader } from "../../../components/settings/PageHeader";
import { SettingsField } from "../../../components/settings/SettingsField";
import { SettingsSwitchRow } from "../../../components/settings/SettingsSwitchRow";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "../../../components/ui/alert-dialog";
import { Button } from "../../../components/ui/button";
import { SimpleSelect } from "../../../components/ui/simple-select";
import { SECTION_ICONS } from "../sectionNav";

export function HistorySettings({
  settings,
  copy,
  onChange,
  onDeleteAll,
}: {
  settings: AppSettings;
  copy: Messages;
  onChange: (patch: Partial<AppSettings>) => void;
  onDeleteAll: () => void;
}) {
  return (
    <div>
      <PageHeader icon={SECTION_ICONS.historySettings} title={copy.navStorage} />
      <SettingsField label={copy.keepRecordings}>
        <SimpleSelect
          aria-label={copy.keepRecordings}
          value={settings.retention}
          onValueChange={(retention) => onChange({ retention })}
          options={[
            { value: "1d", label: copy.retain1d },
            { value: "3d", label: copy.retain3d },
            { value: "7d", label: copy.retain7d },
            { value: "30d", label: copy.retain30d },
            { value: "90d", label: copy.retain90d },
            { value: "forever", label: copy.retainForever },
          ]}
        />
      </SettingsField>
      <SettingsField label={copy.storageLimit}>
        <SimpleSelect
          aria-label={copy.storageLimit}
          value={settings.storageLimit}
          onValueChange={(storageLimit) => onChange({ storageLimit })}
          options={[
            { value: "500mb", label: copy.limit500mb },
            { value: "1gb", label: copy.limit1gb },
            { value: "5gb", label: copy.limit5gb },
            { value: "unlimited", label: copy.limitUnlimited },
          ]}
        />
      </SettingsField>
      <SettingsSwitchRow
        label={copy.keepOriginals}
        checked={settings.keepOriginalRecordings}
        onCheckedChange={(keepOriginalRecordings) => onChange({ keepOriginalRecordings })}
      />
      <AlertDialog>
        <AlertDialogTrigger asChild>
          <Button variant="destructive">{copy.deleteAllHistory}</Button>
        </AlertDialogTrigger>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{copy.deleteAllConfirm}</AlertDialogTitle>
            <AlertDialogDescription>{copy.deleteAllCannotUndo}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{copy.cancel}</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={onDeleteAll}>
              {copy.delete}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
