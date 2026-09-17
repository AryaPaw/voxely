import { useEffect, useState, type ReactNode } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { CircleAlert, ExternalLink, Github } from "lucide-react";
import { toast } from "sonner";
import { api, type UpdateOutcome } from "../../../lib/api";
import { reportError } from "../../../lib/system-notify";
import { formatInvokeError, updateToast, type Messages, appDisplayName } from "../../../lib/i18n";
import { formatReleaseDate } from "../../../lib/release-meta";
import { PageHeader } from "../../../components/settings/PageHeader";
import { Button } from "../../../components/ui/button";
import { SECTION_ICONS, sectionLabel } from "../sectionNav";

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

function LinkRow({
  icon,
  title,
  hint,
  copy,
  onOpen,
}: {
  icon: ReactNode;
  title: string;
  hint: string;
  copy: Messages;
  onOpen: () => Promise<void>;
}) {
  return (
    <button
      type="button"
      aria-label={`${title}. ${hint}`}
      className="flex min-h-11 w-full items-center gap-3 px-3 py-2.5 text-left transition-colors hover:bg-muted/70 active:bg-muted focus-visible:bg-muted/70 focus-visible:outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
      onClick={() => {
        void onOpen().catch((error: unknown) => {
          reportError(formatInvokeError(error, copy));
        });
      }}
    >
      <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-muted text-foreground">
        {icon}
      </span>
      <span className="min-w-0 flex-1">
        <span className="block text-sm font-medium text-pretty">{title}</span>
        <span className="block text-xs text-muted-foreground">{hint}</span>
      </span>
      <ExternalLink className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
    </button>
  );
}

export function AboutSettings({ copy }: { copy: Messages }) {
  const [version, setVersion] = useState("");
  const [buildDate, setBuildDate] = useState("");
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [updateReady, setUpdateReady] = useState(false);
  const released = buildDate ? formatReleaseDate(buildDate, copy.dateLocale) : "";

  useEffect(() => {
    void getVersion().then(setVersion);
    void api.runtimeInfo().then((info) => setBuildDate(info.buildDate));
  }, []);

  return (
    <div>
      <PageHeader icon={SECTION_ICONS.about} title={sectionLabel(copy, "about")} />
      <div className="mb-8 flex items-center gap-4">
        <img
          src="/favicon.png"
          alt=""
          className="h-16 w-16 rounded-2xl outline outline-1 -outline-offset-1 outline-black/10 dark:outline-white/10"
        />
        <div className="min-w-0">
          <p className="text-lg font-semibold text-balance">{appDisplayName(copy)}</p>
          <p className="text-sm text-pretty text-muted-foreground">{copy.aboutTagline}</p>
          <p className="mt-1 text-sm text-muted-foreground">
            {copy.aboutVersion}{" "}
            <span className="tabular-nums text-foreground">{version || "…"}</span>
            {released ? (
              <>
                <span className="mx-2 opacity-50">|</span>
                {released}
              </>
            ) : null}
          </p>
        </div>
      </div>
      <div className="mb-6 flex max-w-lg flex-wrap gap-2">
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
      <div className="mb-6 max-w-lg overflow-hidden rounded-xl border border-border">
        <LinkRow
          copy={copy}
          icon={<CircleAlert className="size-4" aria-hidden="true" />}
          title={copy.aboutReport}
          hint={copy.aboutReportHint}
          onOpen={() => api.openGithub("issues")}
        />
        <div className="h-px bg-border" />
        <LinkRow
          copy={copy}
          icon={<Github className="size-4" aria-hidden="true" />}
          title={copy.aboutSource}
          hint={copy.githubRepo}
          onOpen={() => api.openGithub()}
        />
      </div>
      <p className="text-sm text-muted-foreground">
        {copy.aboutAuthor}: <span className="text-foreground">{copy.authorName}</span>
      </p>
    </div>
  );
}
