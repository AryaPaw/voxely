import { toast } from "sonner";
import { api, type AppSettings } from "../../../lib/api";
import { formatInvokeError, statusToast, type Messages } from "../../../lib/i18n";
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

export function AdvancedSettings({
  settings,
  copy,
  onChange,
  onSettingsReplaced,
}: {
  settings: AppSettings;
  copy: Messages;
  onChange: (patch: Partial<AppSettings>) => void;
  onSettingsReplaced: (settings: AppSettings, wipeApiKey: boolean) => void;
}) {
  async function reset(wipeApiKey: boolean) {
    try {
      const next = await api.resetSettings(wipeApiKey);
      onSettingsReplaced(next, wipeApiKey);
      toast.success(statusToast("success", copy, wipeApiKey ? copy.resetAllDone : copy.resetDone));
    } catch (error: unknown) {
      toast.error(statusToast("error", copy, formatInvokeError(error, copy)));
    }
  }

  function openDir(kind: "logs" | "settings") {
    const failed = kind === "logs" ? copy.openLogsFailed : copy.openSettingsFailed;
    const open = kind === "logs" ? api.openLogs : api.openSettingsDir;
    void open().catch(() => {
      toast.error(statusToast("error", copy, failed));
    });
  }

  return (
    <div>
      <PageHeader icon={SECTION_ICONS.advanced} title={copy.navAdvanced} />
      <SettingsField label={copy.insertMode}>
        <SimpleSelect
          aria-label={copy.insertMode}
          value={settings.insertionMode}
          onValueChange={(insertionMode) => onChange({ insertionMode })}
          options={[
            { value: "unicode", label: copy.insertUnicode },
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
      <div className="mb-4 flex max-w-lg flex-wrap gap-2">
        <Button variant="outline" onClick={() => openDir("logs")}>
          {copy.openLogs}
        </Button>
        <Button variant="outline" onClick={() => openDir("settings")}>
          {copy.openSettingsFolder}
        </Button>
      </div>
      <div className="flex max-w-lg flex-wrap gap-2">
        <AlertDialog>
          <AlertDialogTrigger asChild>
            <Button variant="outline">{copy.resetSettings}</Button>
          </AlertDialogTrigger>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>{copy.resetSettingsConfirm}</AlertDialogTitle>
              <AlertDialogDescription>{copy.resetSettingsHint}</AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>{copy.cancel}</AlertDialogCancel>
              <AlertDialogAction onClick={() => void reset(false)}>
                {copy.resetSettings}
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
        <AlertDialog>
          <AlertDialogTrigger asChild>
            <Button variant="destructive">{copy.resetAll}</Button>
          </AlertDialogTrigger>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>{copy.resetAllConfirm}</AlertDialogTitle>
              <AlertDialogDescription>{copy.resetAllHint}</AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>{copy.cancel}</AlertDialogCancel>
              <AlertDialogAction variant="destructive" onClick={() => void reset(true)}>
                {copy.resetAll}
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </div>
    </div>
  );
}
