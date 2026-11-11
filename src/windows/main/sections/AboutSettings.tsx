import { useEffect, useState, type ReactNode } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { CircleAlert, ExternalLink, Github } from "lucide-react";
import { api } from "../../../lib/api";
import { reportError } from "../../../lib/system-notify";
import { formatInvokeError, type Messages, appDisplayName } from "../../../lib/i18n";
import { formatReleaseDate } from "../../../lib/release-meta";
import { PageHeader } from "../../../components/settings/PageHeader";
import { SECTION_ICONS, sectionLabel } from "../sectionNav";
import { UpdateSettings } from "./UpdateSettings";
import { SettingsGroup } from "../../../components/settings/SettingsGroup";
import { SettingsField } from "../../../components/settings/SettingsField";
import { VersionBadge } from "../VersionBadge";

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
  const released = buildDate ? formatReleaseDate(buildDate, copy.dateLocale) : "";

  useEffect(() => {
    void getVersion().then(setVersion);
    void api.runtimeInfo().then((info) => setBuildDate(info.buildDate));
  }, []);

  return (
    <div>
      <PageHeader icon={SECTION_ICONS.about} title={sectionLabel(copy, "about")} />
      <div className="about-identity">
        <img src="/favicon.png" alt="" className="size-14 shrink-0 object-contain" />
        <div className="min-w-0">
          <p className="text-lg font-semibold text-balance">{appDisplayName(copy)}</p>
          <p className="text-sm text-pretty text-muted-foreground">{copy.aboutTagline}</p>
          <div className="mt-3 flex flex-wrap items-center gap-3 text-sm text-muted-foreground">
            <VersionBadge version={version} />
            {released ? (
              <>
                <span>{released}</span>
              </>
            ) : null}
          </div>
        </div>
      </div>
      <SettingsGroup title={copy.settingsUpdates}>
        <SettingsField label={copy.settingsManualUpdate} action>
          <UpdateSettings copy={copy} />
        </SettingsField>
      </SettingsGroup>
      <div className="mt-6 overflow-hidden rounded-xl border border-border">
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
    </div>
  );
}
