import { useRef, useState } from "react";
import { toast } from "sonner";
import { api, type AppSettings } from "../../../lib/api";
import { formatInvokeError, statusToast, type Messages } from "../../../lib/i18n";
import { reportError } from "../../../lib/system-notify";
import { SettingsField } from "../../../components/settings/SettingsField";
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
export function SettingsReset({
  copy,
  onSettingsReplaced,
}: {
  copy: Messages;
  onSettingsReplaced: (settings: AppSettings, wipeApiKey: boolean) => void;
}) {
  const pending = useRef(false);
  const [busy, setBusy] = useState(false);
  async function reset(wipeApiKey: boolean) {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    try {
      const next = await api.resetSettings(wipeApiKey);
      onSettingsReplaced(next, wipeApiKey);
      toast.success(statusToast("success", copy, wipeApiKey ? copy.resetAllDone : copy.resetDone));
    } catch (error: unknown) {
      reportError(statusToast("error", copy, formatInvokeError(error, copy)));
    } finally {
      pending.current = false;
      setBusy(false);
    }
  }

  return (
    <>
      <SettingsField label={copy.settingsResetTarget} description={copy.resetSettingsHint} action>
        <AlertDialog>
          <AlertDialogTrigger asChild>
            <Button variant="outline" disabled={busy}>
              {copy.resetSettings}
            </Button>
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
      </SettingsField>
      <SettingsField
        label={copy.settingsResetCredentialsTarget}
        description={copy.resetAllHint}
        action
      >
        <AlertDialog>
          <AlertDialogTrigger asChild>
            <Button variant="destructive" disabled={busy}>
              {copy.resetAll}
            </Button>
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
      </SettingsField>
    </>
  );
}
