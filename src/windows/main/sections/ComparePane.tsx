import { useEffect, useMemo, useRef, useState } from "react";
import {
  DndContext,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  rectSortingStrategy,
  sortableKeyboardCoordinates,
} from "@dnd-kit/sortable";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { api, type AppSettings, type CompareState } from "../../../lib/api";
import { formatInvokeError, type Messages } from "../../../lib/i18n";
import { PageHeader } from "../../../components/settings/PageHeader";
import { Button } from "../../../components/ui/button";
import { SECTION_ICONS, sectionLabel } from "../sectionNav";
import { CompareModelCard } from "./CompareModelCard";
import {
  COMPARE_MODEL_MAX,
  COMPARE_MODEL_MIN,
  compareSlotIds,
  moveCompareModel,
} from "./compare-models";

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
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const models = useMemo(() => settings.compareModels ?? [], [settings.compareModels]);
  const slotIds = useMemo(() => compareSlotIds(models.length), [models.length]);
  const unique = useMemo(() => new Set(models.map((item) => item.trim())), [models]);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );
  const canRun =
    keyConfigured &&
    Boolean(state.listenPath) &&
    !state.recording &&
    !state.running &&
    models.length >= COMPARE_MODEL_MIN &&
    models.length <= COMPARE_MODEL_MAX &&
    unique.size === models.length &&
    [...unique].every((item) => item.length > 0);

  useEffect(() => {
    let cancelled = false;
    const unlisten = listen<CompareState>("compare://state", (event) => {
      setState(event.payload);
    });
    void api
      .getModelCompare()
      .then((next) => {
        if (!cancelled) {
          setState(next);
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setError(formatInvokeError(err, copy));
        }
      });
    return () => {
      cancelled = true;
      void unlisten.then((fn) => fn());
    };
  }, [copy]);

  useEffect(() => {
    const node = audioRef.current;
    return () => {
      node?.pause();
    };
  }, []);

  function patchModels(next: string[]) {
    onChange({ compareModels: next });
  }

  function onDragEnd(event: DragEndEvent) {
    const { active, over } = event;
    if (!over || active.id === over.id) {
      return;
    }
    const from = slotIds.indexOf(String(active.id));
    const to = slotIds.indexOf(String(over.id));
    patchModels(moveCompareModel(models, from, to));
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
        <Button type="button" variant="outline" onClick={() => void api.openOpenrouterModels()}>
          {copy.openRouterCatalog}
        </Button>
        <Button type="button" onClick={() => void toggleRecord()} disabled={state.running}>
          {state.recording ? copy.compareStop : copy.compareRecord}
        </Button>
        <Button type="button" onClick={() => void run()} disabled={!canRun}>
          {copy.compareRun}
        </Button>
        {state.running ? (
          <Button
            type="button"
            variant="outline"
            onClick={() =>
              void api
                .cancelModelCompare()
                .then(setState)
                .catch((err: unknown) => setError(formatInvokeError(err, copy)))
            }
          >
            {copy.cancel}
          </Button>
        ) : null}
        {models.length < COMPARE_MODEL_MAX ? (
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
          ref={audioRef}
          key={state.nonce}
          className="mb-4 w-full max-w-lg"
          controls
          src={compareListenSrc(state.listenPath, state.nonce)}
        />
      ) : (
        <p className="mb-4 text-sm text-muted-foreground">{copy.compareNoClip}</p>
      )}
      <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={onDragEnd}>
        <SortableContext items={slotIds} strategy={rectSortingStrategy}>
          <div className="grid gap-3 md:grid-cols-2">
            {models.map((model, index) => (
              <CompareModelCard
                key={slotIds[index]}
                id={slotIds[index]}
                index={index}
                model={model}
                settings={settings}
                copy={copy}
                slot={compareSlotForModel(state, model)}
                canRemove={models.length > COMPARE_MODEL_MIN}
                onModelChange={(value) => {
                  const next = [...models];
                  next[index] = value;
                  patchModels(next);
                }}
                onRemove={() => patchModels(models.filter((_, item) => item !== index))}
                onMakeDefault={() => {
                  onChange({ model: model.trim() });
                  toast.success(copy.compareDefaultSet);
                }}
              />
            ))}
          </div>
        </SortableContext>
      </DndContext>
    </div>
  );
}

function compareSlotForModel(state: CompareState, model: string) {
  const trimmed = model.trim();
  const runId = state.runId ?? null;
  return (
    state.slots.find((slot) => slot.model === trimmed && (slot.runId ?? null) === runId) ??
    state.slots.find((slot) => slot.slotId && slot.model === trimmed)
  );
}

function compareListenSrc(path: string, nonce: number): string {
  const src = convertFileSrc(path.replace(/\\/g, "/"));
  return `${src}?n=${nonce}`;
}
