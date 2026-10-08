import { useState } from "react";
import { Info } from "lucide-react";
import type { Messages } from "../../lib/i18n";
import { Button } from "../../components/ui/button";

export function FirstRunDisclosure({
  copy,
  onAcknowledge,
  onOpenStorage,
}: {
  copy: Messages;
  onAcknowledge: () => Promise<void>;
  onOpenStorage: () => void;
}) {
  const [pending, setPending] = useState(false);

  async function handleAcknowledge() {
    if (pending) {
      return;
    }
    setPending(true);
    try {
      await onAcknowledge();
    } finally {
      setPending(false);
    }
  }

  return (
    <section
      aria-labelledby="first-run-disclosure-title"
      className="mx-6 mt-5 rounded-xl border border-primary/25 bg-primary/[0.06] p-4 shadow-sm"
    >
      <div className="flex items-start gap-3">
        <Info aria-hidden="true" className="mt-0.5 h-5 w-5 shrink-0 text-primary" />
        <div className="min-w-0 flex-1">
          <h2 id="first-run-disclosure-title" className="text-sm font-semibold">
            {copy.firstRunDisclosureTitle}
          </h2>
          <p className="mt-1 max-w-3xl text-sm leading-6 text-muted-foreground">
            {copy.firstRunDisclosureBody}
          </p>
          <div className="mt-3 flex flex-wrap gap-2">
            <Button type="button" disabled={pending} onClick={() => void handleAcknowledge()}>
              {copy.firstRunDisclosureAccept}
            </Button>
            <Button type="button" variant="outline" onClick={onOpenStorage}>
              {copy.firstRunDisclosureStorage}
            </Button>
          </div>
        </div>
      </div>
    </section>
  );
}
