import { api } from "../../../lib/api";
import { CUE_KINDS } from "../../../lib/cue-preview";
import { formatInvokeError, type Messages } from "../../../lib/i18n";
import { reportError } from "../../../lib/system-notify";
import { PageHeader } from "../../../components/settings/PageHeader";
import { Button } from "../../../components/ui/button";
import { SECTION_ICONS, sectionLabel } from "../sectionNav";

export function DebugSettings({ copy }: { copy: Messages }) {
  return (
    <div>
      <PageHeader icon={SECTION_ICONS.debug} title={sectionLabel(copy, "debug")} />
      <h2 className="mb-2 text-sm font-medium">{copy.debugCues}</h2>
      <p className="mb-4 max-w-lg text-sm text-muted-foreground">{copy.debugCueIntro}</p>
      <div className="flex flex-wrap gap-2">
        {CUE_KINDS.map((kind) => (
          <Button
            key={kind}
            type="button"
            variant="outline"
            onClick={() => {
              void api.playCue(kind).catch((error: unknown) => {
                reportError(formatInvokeError(error, copy));
              });
            }}
          >
            {kind === "start" ? copy.cueStart : copy.cueStop}
          </Button>
        ))}
      </div>
      <h2 className="mb-2 mt-8 text-sm font-medium">{copy.debugNotify}</h2>
      <p className="mb-4 max-w-lg text-sm text-muted-foreground">{copy.debugNotifyIntro}</p>
      <Button
        type="button"
        variant="outline"
        onClick={() => {
          void api.previewErrorNotification().catch((error: unknown) => {
            reportError(formatInvokeError(error, copy));
          });
        }}
      >
        {copy.debugNotifySend}
      </Button>
    </div>
  );
}
