import { api, type CueKind } from "../../../lib/api";
import { formatInvokeError, type Messages } from "../../../lib/i18n";
import { reportError } from "../../../lib/system-notify";
import { PageHeader } from "../../../components/settings/PageHeader";
import { Button } from "../../../components/ui/button";
import { SECTION_ICONS, sectionLabel } from "../sectionNav";

const CUE_KINDS: CueKind[] = ["start", "stop", "cancel"];

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
            {cueLabel(copy, kind)}
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

function cueLabel(copy: Messages, kind: CueKind): string {
  switch (kind) {
    case "start":
      return copy.debugCueStart;
    case "stop":
      return copy.debugCueStop;
    case "cancel":
      return copy.debugCueCancel;
    default: {
      const _never: never = kind;
      return _never;
    }
  }
}
