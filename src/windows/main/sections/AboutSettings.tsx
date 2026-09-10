import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { toast } from "sonner";
import { api } from "../../../lib/api";
import { formatInvokeError, updateToast, type Messages } from "../../../lib/i18n";
import { PageHeader } from "../../../components/settings/PageHeader";
import { Button } from "../../../components/ui/button";
import { SECTION_ICONS, sectionLabel } from "../sectionNav";

export function AboutSettings({ copy }: { copy: Messages }) {
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
          <p className="text-lg font-semibold">Voxely</p>
          <p className="text-sm text-muted-foreground">
            {copy.aboutVersion}: {version || "…"}
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
            <button
              type="button"
              className="text-primary underline-offset-2 hover:underline"
              onClick={() => {
                void api.openGithub().catch((error: unknown) => {
                  toast.error(formatInvokeError(error, copy));
                });
              }}
            >
              {copy.githubRepo}
            </button>
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
            toast.error(formatInvokeError(error, copy));
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
