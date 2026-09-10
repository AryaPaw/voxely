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
      <SettingsField label="Хранить записи">
        <SimpleSelect
          aria-label="Хранить записи"
          value={settings.retention}
          onValueChange={(retention) => onChange({ retention })}
          options={[
            { value: "1d", label: "1 день" },
            { value: "3d", label: "3 дня" },
            { value: "7d", label: "7 дней" },
            { value: "30d", label: "30 дней" },
            { value: "90d", label: "90 дней" },
            { value: "forever", label: "Всегда" },
          ]}
        />
      </SettingsField>
      <SettingsField label="Лимит места">
        <SimpleSelect
          aria-label="Лимит места"
          value={settings.storageLimit}
          onValueChange={(storageLimit) => onChange({ storageLimit })}
          options={[
            { value: "500mb", label: "500 МБ" },
            { value: "1gb", label: "1 ГБ" },
            { value: "5gb", label: "5 ГБ" },
            { value: "unlimited", label: "Без лимита" },
          ]}
        />
      </SettingsField>
      <SettingsSwitchRow
        label="Хранить исходные записи (лучше для прослушивания)"
        checked={settings.keepOriginalRecordings}
        onCheckedChange={(keepOriginalRecordings) => onChange({ keepOriginalRecordings })}
      />
      <AlertDialog>
        <AlertDialogTrigger asChild>
          <Button variant="destructive">Удалить всю историю</Button>
        </AlertDialogTrigger>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Удалить всю историю?</AlertDialogTitle>
            <AlertDialogDescription>Это нельзя отменить.</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Отмена</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={onDeleteAll}>
              Удалить
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
