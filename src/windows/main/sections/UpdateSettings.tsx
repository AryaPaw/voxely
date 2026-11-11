import { useState } from "react";
import { toast } from "sonner";
import { api, type UpdateOutcome } from "../../../lib/api";
import { formatInvokeError, updateToast, type Messages } from "../../../lib/i18n";
import { reportError } from "../../../lib/system-notify";
import { Button } from "../../../components/ui/button";
function announceUpdate(code: UpdateOutcome, copy: Messages) {
  const message = updateToast(code, copy);
  switch (code) {
    case "failed":
      toast.error(message);
      return;
    case "busy":
    case "deferred":
      toast.message(message);
      return;
    case "none":
    case "available":
    case "installed":
      toast.success(message);
      return;
    default: {
      const exhaustive: never = code;
      void exhaustive;
    }
  }
}

export function UpdateSettings({ copy }: { copy: Messages }) {
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [updateReady, setUpdateReady] = useState(false);
  return (
    <div className="flex flex-wrap gap-2">
      <Button
        variant="outline"
        disabled={checking || installing}
        onClick={async () => {
          setChecking(true);
          try {
            const code = await api.checkForUpdates();
            setUpdateReady(code === "available");
            announceUpdate(code, copy);
          } catch (error) {
            reportError(formatInvokeError(error, copy));
          } finally {
            setChecking(false);
          }
        }}
      >
        {checking ? copy.checking : copy.checkUpdates}
      </Button>
      {updateReady ? (
        <Button
          disabled={checking || installing}
          onClick={async () => {
            setInstalling(true);
            try {
              const code = await api.installUpdate();
              if (code === "installed" || code === "none") {
                setUpdateReady(false);
              }
              announceUpdate(code, copy);
            } catch (error) {
              reportError(formatInvokeError(error, copy));
            } finally {
              setInstalling(false);
            }
          }}
        >
          {installing ? copy.installing : copy.installUpdate}
        </Button>
      ) : null}
    </div>
  );
}
