import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { toast } from "sonner";
import { api } from "../../../lib/api";
import { reportError } from "../../../lib/system-notify";
import { formatInvokeError, updateToast, type Messages, appDisplayName } from "../../../lib/i18n";
import { APP_RELEASED_ON, versionWithReleaseDate } from "../../../lib/release-meta";
import { PageHeader } from "../../../components/settings/PageHeader";
import { Button } from "../../../components/ui/button";
import { SECTION_ICONS, sectionLabel } from "../sectionNav";

function GithubLink({
  label,
  onOpen,
  copy,
}: {
  label: string;
  onOpen: () => Promise<void>;
  copy: Messages;
}) {
  return (
    <button
      type="button"
      className="text-primary underline-offset-2 hover:underline"
      onClick={() => {
        void onOpen().catch((error: unknown) => {
          reportError(formatInvokeError(error, copy));
        });
      }}
    >
      {label}
    </button>
  );
}

export function AboutSettings({ copy, localBuild }: { copy: Messages; localBuild: boolean }) {
  const [version, setVersion] = useState("");
  const [checking, setChecking] = useState(false);

  useEffect(() => {
    void getVersion().then(setVersion);
  }, []);

  return (
    <div>
      <PageHeader icon={SECTION_ICONS.about} title={sectionLabel(copy, "about")} />
      <div className="mb-6 flex items-center gap-4">
        <img src="/favicon.png" alt="" className="h-16 w-16 rounded-2xl" />
        <div>
          <p className="text-lg font-semibold">{appDisplayName(copy, localBuild)}</p>
          <p className="text-sm text-muted-foreground">
            {copy.aboutVersion}: {versionWithReleaseDate(version, APP_RELEASED_ON, copy.dateLocale)}
          </p>
        </div>
      </div>
      <dl className="mb-6 max-w-lg space-y-2 text-sm">
        <div>
          <dt className="inline text-muted-foreground">{copy.aboutAuthor}: </dt>
          <dd className="inline">{copy.authorName}</dd>
        </div>
        <div>
          <dt className="inline text-muted-foreground">{copy.aboutGithub}: </dt>
          <dd className="inline">
            <GithubLink label={copy.githubRepo} copy={copy} onOpen={() => api.openGithub()} />
            <span className="text-muted-foreground"> / </span>
            <GithubLink
              label={copy.githubIssues}
              copy={copy}
              onOpen={() => api.openGithub("issues")}
            />
          </dd>
        </div>
      </dl>
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
  );
}
