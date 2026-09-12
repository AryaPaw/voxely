import { useEffect, useMemo, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { api, type AppSettings, type CompareState } from "../../../lib/api";
import { formatInvokeError, type Messages } from "../../../lib/i18n";
import { PageHeader } from "../../../components/settings/PageHeader";
import { Button } from "../../../components/ui/button";
import { Input } from "../../../components/ui/input";
import { SECTION_ICONS, sectionLabel } from "../sectionNav";

const emptyState: CompareState = {
  recording: false,
  running: false,
  nonce: 0,
  listenPath: null,
  sttPath: null,
  runId: null,
  slots: [],
};

export function ComparePane({
  settings,
  copy,
  keyConfigured,
  onChange,
  onOpenKey,
}: {
  settings: AppSettings;
  copy: Messages;
  keyConfigured: boolean;
  onChange: (patch: Partial<AppSettings>) => void;
  onOpenKey: () => void;
}) {
  const [state, setState] = useState<CompareState>(emptyState);
  const [error, setError] = useState("");
  const models = useMemo(() => settings.compareModels ?? [], [settings.compareModels]);
  const unique = useMemo(() => new Set(models.map((item) => item.trim())), [models]);
  const canRun =
    keyConfigured &&
    Boolean(state.listenPath) &&
    !state.recording &&
    !state.running &&
    models.length >= 2 &&
    models.length <= 4 &&
    unique.size === models.length &&
    [...unique].every((item) => item.length > 0);

  useEffect(() => {
    void api
      .getModelCompare()
      .then(setState)
      .catch(() => undefined);
    const unlisten = listen<CompareState>("compare://state", (event) => {
      setState(event.payload);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  function patchModels(next: string[]) {
    onChange({ compareModels: next });
  }

  async function toggleRecord() {
    setError("");
    try {
      if (state.recording) {
        setState(await api.stopModelCompare());
        return;
      }
      await api.startModelCompare();
      setState((current) => ({ ...current, recording: true }));
    } catch (err) {
      setError(formatInvokeError(err, copy));
    }
  }

  async function run() {
    setError("");
    try {
      setState(await api.runModelCompare());
    } catch (err) {
      setError(formatInvokeError(err, copy));
    }
  }

  return (
    <div>
      <PageHeader icon={SECTION_ICONS.compare} title={sectionLabel(copy, "compare")} />
      <p className="mb-4 max-w-lg text-sm text-muted-foreground">{copy.compareIntro}</p>
      <div className="mb-4 flex flex-wrap gap-2">
        <Button type="button" onClick={() => void toggleRecord()} disabled={state.running}>
          {state.recording ? copy.compareStop : copy.compareRecord}
        </Button>
        <Button type="button" onClick={() => void run()} disabled={!canRun}>
          {copy.compareRun}
        </Button>
        {models.length < 4 ? (
          <Button type="button" variant="outline" onClick={() => patchModels([...models, ""])}>
            {copy.compareAddModel}
          </Button>
        ) : null}
      </div>
      {!keyConfigured ? (
        <p className="mb-3 text-sm text-muted-foreground">
          {copy.compareNoKey}{" "}
          <button type="button" className="underline" onClick={onOpenKey}>
            {copy.navTranscription}
          </button>
        </p>
      ) : null}
      {error ? <p className="mb-3 text-sm text-destructive">{error}</p> : null}
      {state.listenPath ? (
        <audio
          key={state.nonce}
          className="mb-4 w-full max-w-lg"
          controls
          src={compareListenSrc(state.listenPath, state.nonce)}
        />
      ) : (
        <p className="mb-4 text-sm text-muted-foreground">{copy.compareNoClip}</p>
      )}
      <div className="grid gap-3 md:grid-cols-2">
        {models.map((model, index) => {
          const slot = state.slots[index];
          return (
            <div key={`${index}-${model}`} className="rounded-lg border border-border p-3">
              <Input
                aria-label={`model-${index + 1}`}
                value={model}
                onChange={(event) => {
                  const next = [...models];
                  next[index] = event.target.value;
                  patchModels(next);
                }}
              />
              <div className="mt-2 flex gap-2">
                {models.length > 2 ? (
                  <Button
                    type="button"
                    variant="ghost"
                    onClick={() => patchModels(models.filter((_, item) => item !== index))}
                  >
                    {copy.compareRemoveModel}
                  </Button>
                ) : null}
                <Button
                  type="button"
                  variant="outline"
                  disabled={!model.trim()}
                  onClick={() => onChange({ model: model.trim() })}
                >
                  {copy.compareMakeDefault}
                </Button>
              </div>
              {slot ? (
                <div className="mt-3 text-sm">
                  <div className="text-muted-foreground">
                    {slot.status === "error"
                      ? copy.compareSlotError
                      : copy.compareAttempt.replace("{value}", String(slot.attempt))}
                    {slot.cost != null ? ` / ${slot.cost}` : ""}
                  </div>
                  {slot.text ? <p className="mt-1 whitespace-pre-wrap">{slot.text}</p> : null}
                  {slot.error ? <p className="mt-1 text-destructive">{slot.error}</p> : null}
                </div>
              ) : null}
            </div>
          );
        })}
      </div>
    </div>
  );
}

function compareListenSrc(path: string, nonce: number): string {
  const src = convertFileSrc(path.replace(/\\/g, "/"));
  return `${src}?n=${nonce}`;
}
