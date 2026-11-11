import { useEffect, useState, type ReactNode } from "react";
import { Copy, Minus, Square, X } from "lucide-react";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Button } from "../../components/ui/button";
import { appDisplayName, formatInvokeError, type Messages } from "../../lib/i18n";
import { reportError } from "../../lib/system-notify";
import { VersionBadge } from "./VersionBadge";

export function AppFrame({
  children,
  copy,
  version,
  localBuild,
  onAbout,
}: {
  children: ReactNode;
  copy: Messages;
  version: string;
  localBuild: boolean;
  onAbout: () => void;
}) {
  const desktop = isTauri();
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    if (!desktop) return;
    const window = getCurrentWindow();
    let disposed = false;
    let revision = 0;
    let release: (() => void) | undefined;
    const sync = async () => {
      const current = ++revision;
      const value = await window.isMaximized();
      if (!disposed && current === revision) setMaximized(value);
    };
    const fail = (error: unknown) => {
      if (!disposed) reportError(formatInvokeError(error, copy));
    };
    void window
      .onResized(() => {
        void sync().catch(fail);
      })
      .then((unlisten) => {
        if (disposed) unlisten();
        else {
          release = unlisten;
          void sync().catch(fail);
        }
      })
      .catch(fail);
    return () => {
      disposed = true;
      revision++;
      release?.();
    };
  }, [desktop, copy]);
  const run = (action: () => Promise<void>) => {
    void action().catch((error: unknown) => reportError(formatInvokeError(error, copy)));
  };
  return (
    <div className="app-shell" data-maximized={maximized || undefined}>
      <header className="window-titlebar">
        <div className="window-drag-region" data-tauri-drag-region>
          <img src="/favicon.png" alt="" draggable={false} />
          <span>{appDisplayName(copy, localBuild)}</span>
        </div>
        <Button
          variant="ghost"
          className="window-version"
          onClick={onAbout}
          aria-label={`${copy.aboutVersion} ${version}`}
        >
          <VersionBadge version={version} />
        </Button>
        <div className="window-controls">
          <Button
            variant="ghost"
            className="window-control"
            disabled={!desktop}
            aria-label={copy.windowMinimize}
            onClick={() => run(() => getCurrentWindow().minimize())}
          >
            <Minus className="size-3.5" strokeWidth={1.5} />
          </Button>
          <Button
            variant="ghost"
            className="window-control"
            disabled={!desktop}
            aria-label={maximized ? copy.windowRestore : copy.windowMaximize}
            onClick={() => run(() => getCurrentWindow().toggleMaximize())}
          >
            {maximized ? (
              <Copy className="size-3" strokeWidth={1.5} />
            ) : (
              <Square className="size-3" strokeWidth={1.5} />
            )}
          </Button>
          <Button
            variant="ghost"
            className="window-control window-control-close"
            disabled={!desktop}
            aria-label={copy.close}
            onClick={() => run(() => getCurrentWindow().close())}
          >
            <X className="size-4" strokeWidth={1.5} />
          </Button>
        </div>
      </header>
      <div className="app-workspace">{children}</div>
    </div>
  );
}
