import { toast } from "sonner";
import { useState } from "react";
import { api, type AppSettings } from "../../../lib/api";
import { formatInvokeError, statusToast, type Messages } from "../../../lib/i18n";
import { reportError } from "../../../lib/system-notify";
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
  const [busy, setBusy] = useState(false);
  const [pendingImport, setPendingImport] = useState<string | null>(null);

  async function transfer(action: "export" | "pick" | "import") {
    if (busy) return;
    setBusy(true);
    try {
      if (action === "export") {
        if (await api.exportSettingsFile()) toast.success(copy.settingsExported);
      } else if (action === "pick") {
        setPendingImport(await api.readSettingsFile());
      } else if (pendingImport !== null) {
        const next = await api.importSettings(pendingImport);
        onSettingsReplaced(next, false);
        setPendingImport(null);
        toast.success(copy.settingsImported);
      }
    } catch (error) {
      reportError(statusToast("error", copy, formatInvokeError(error, copy)));
    } finally {
      setBusy(false);
    }
  }
  async function reset(wipeApiKey: boolean) {
    try {
      const next = await api.resetSettings(wipeApiKey);
      onSettingsReplaced(next, wipeApiKey);
      toast.success(statusToast("success", copy, wipeApiKey ? copy.resetAllDone : copy.resetDone));
    } catch (error: unknown) {
      reportError(statusToast("error", copy, formatInvokeError(error, copy)));
    }
  }

  function openDir(kind: "logs" | "settings") {
    const failed = kind === "logs" ? copy.openLogsFailed : copy.openSettingsFailed;
    const open = kind === "logs" ? api.openLogs : api.openSettingsDir;
    void open().catch(() => {
      reportError(statusToast("error", copy, failed));
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
      <section
        className="mt-6 max-w-lg border-t border-border pt-4"
        aria-labelledby="settings-transfer-heading"
      >
        <h2 id="settings-transfer-heading" className="text-sm font-medium">
          {copy.settingsTransfer}
        </h2>
        <p className="my-2 text-xs text-muted-foreground">{copy.settingsTransferHint}</p>
        <div className="flex gap-2">
          <Button variant="outline" disabled={busy} onClick={() => void transfer("export")}>
            {copy.exportSettings}
          </Button>
          <Button variant="outline" disabled={busy} onClick={() => void transfer("pick")}>
            {copy.importSettings}
          </Button>
        </div>
      </section>
      <AlertDialog
        open={pendingImport !== null}
        onOpenChange={(open) => {
          if (!open && !busy) setPendingImport(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{copy.importSettingsConfirm}</AlertDialogTitle>
            <AlertDialogDescription>{copy.importSettingsHint}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={busy}>{copy.cancel}</AlertDialogCancel>
            <Button disabled={busy} onClick={() => void transfer("import")}>
              {copy.importSettings}
            </Button>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
