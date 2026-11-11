import { toast } from "sonner";
import { useState } from "react";
import { api, type AppSettings } from "../../../lib/api";
import { formatInvokeError, statusToast, type Messages } from "../../../lib/i18n";
import { reportError } from "../../../lib/system-notify";
import {
  AlertDialog,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogDescription,
  AlertDialogContent,
  AlertDialogFooter,
  AlertDialogCancel,
} from "../../../components/ui/alert-dialog";
import { SettingsField } from "../../../components/settings/SettingsField";
import { SettingsGroup } from "../../../components/settings/SettingsGroup";
import { Button } from "../../../components/ui/button";
export function SettingsTransfer({
  copy,
  onImported,
}: {
  copy: Messages;
  onImported: (settings: AppSettings) => void;
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
        onImported(next);
        setPendingImport(null);
        toast.success(copy.settingsImported);
      }
    } catch (error) {
      reportError(statusToast("error", copy, formatInvokeError(error, copy)));
    } finally {
      setBusy(false);
    }
  }
  return (
    <>
      <SettingsGroup title={copy.settingsTransfer}>
        <SettingsField label={copy.settingsFile} description={copy.settingsTransferHint} action>
          <div className="flex gap-2">
            <Button variant="outline" disabled={busy} onClick={() => void transfer("export")}>
              {copy.exportSettings}
            </Button>
            <Button variant="outline" disabled={busy} onClick={() => void transfer("pick")}>
              {copy.importSettings}
            </Button>
          </div>
        </SettingsField>
      </SettingsGroup>
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
    </>
  );
}
