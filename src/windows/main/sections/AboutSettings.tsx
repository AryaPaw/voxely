import { useEffect, useState, type ReactNode } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { CircleAlert, ExternalLink, Github } from "lucide-react";
import { toast } from "sonner";
import { api } from "../../../lib/api";
import { reportError } from "../../../lib/system-notify";
import { formatInvokeError, updateToast, type Messages, appDisplayName } from "../../../lib/i18n";
import { APP_RELEASED_ON, formatReleaseDate } from "../../../lib/release-meta";
import { PageHeader } from "../../../components/settings/PageHeader";
import { Button } from "../../../components/ui/button";
import { SECTION_ICONS, sectionLabel } from "../sectionNav";

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
  const [checking, setChecking] = useState(false);
  const released = formatReleaseDate(APP_RELEASED_ON, copy.dateLocale);

  useEffect(() => {
    void getVersion().then(setVersion);
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
            <span className="mx-2 opacity-50">|</span>
            {released}
          </p>
        </div>
      </div>
      <div className="mb-6 max-w-lg">
        <Button
          variant="outline"
          disabled={checking}
          onClick={async () => {
            setChecking(true);
            try {
              const code = await api.checkForUpdates();
              toast.success(updateToast(code, copy));
            } catch (error) {
              reportError(formatInvokeError(error, copy));
            } finally {
              setChecking(false);
            }
          }}
        >
          {checking ? copy.checking : copy.checkUpdates}
        </Button>
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
