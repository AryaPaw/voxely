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
import { formatInvokeError, messagesForUiLanguage, type Messages } from "../../../lib/i18n";
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
  const statusCopy = messagesForUiLanguage(settings.uiLanguage ?? "auto", navigator.language);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const stateEventRevisionRef = useRef(0);
  const recordingRef = useRef(false);
  const startingRef = useRef(false);
  const mountedRef = useRef(true);
  const stopAfterStartRef = useRef(false);
  recordingRef.current = state.recording;
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
    let stopState: (() => void) | undefined;
    let stopError: (() => void) | undefined;
    void (async () => {
      try {
        const [unlistenState, unlistenError] = await Promise.all([
          listen<CompareState>("compare://state", (event) => {
            stateEventRevisionRef.current += 1;
            if (!cancelled) setState(event.payload);
          }),
          listen<string>("compare://error", (event) => {
            if (!cancelled) setError(formatInvokeError(event.payload, copy));
          }),
        ]);
        if (cancelled) {
          unlistenState();
          unlistenError();
          return;
        }
        stopState = unlistenState;
        stopError = unlistenError;

        const snapshotRevision = stateEventRevisionRef.current;
        const next = await api.getModelCompare();
        if (!cancelled && stateEventRevisionRef.current === snapshotRevision) {
          setState(next);
        }
      } catch (err) {
        if (!cancelled) setError(formatInvokeError(err, copy));
      }
    })();
    return () => {
      cancelled = true;
      stopState?.();
      stopError?.();
    };
  }, [copy]);

  useEffect(() => {
    mountedRef.current = true;
    const node = audioRef.current;
    return () => {
      mountedRef.current = false;
      node?.pause();
      if (recordingRef.current || startingRef.current) {
        stopAfterStartRef.current = true;
        void api.stopModelCompare().catch(() => undefined);
      }
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
    const eventRevision = stateEventRevisionRef.current;
    try {
      if (state.recording) {
        const next = await api.stopModelCompare();
        if (stateEventRevisionRef.current === eventRevision) setState(next);
        return;
      }
      startingRef.current = true;
      stopAfterStartRef.current = false;
      await api.startModelCompare();
      if (!mountedRef.current) {
        if (!stopAfterStartRef.current) {
          stopAfterStartRef.current = true;
          await api.stopModelCompare();
        }
        return;
      }
      const next = await api.getModelCompare();
      if (stateEventRevisionRef.current === eventRevision) setState(next);
    } catch (err) {
      if (mountedRef.current) setError(formatInvokeError(err, copy));
    } finally {
      startingRef.current = false;
    }
  }

  async function run() {
    setError("");
    const eventRevision = stateEventRevisionRef.current;
    try {
      const next = await api.runModelCompare();
      if (stateEventRevisionRef.current === eventRevision) setState(next);
    } catch (err) {
      setError(formatInvokeError(err, copy));
    }
  }

  function cancelRun() {
    const eventRevision = stateEventRevisionRef.current;
    void api
      .cancelModelCompare()
      .then((next) => {
        if (stateEventRevisionRef.current === eventRevision) setState(next);
      })
      .catch((err: unknown) => setError(formatInvokeError(err, copy)));
  }

  return (
    <div>
      <PageHeader
        icon={SECTION_ICONS.compare}
        title={sectionLabel(copy, "compare")}
        description={copy.compareIntro}
      />
      <p className="mb-3 text-sm text-muted-foreground">
        {copy.compareRun} –{" "}
        {statusCopy.compareRequestCount.replace("{count}", String(models.length))}
      </p>
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
          <Button type="button" variant="outline" onClick={cancelRun}>
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
      {error ? (
        <p className="mb-3 text-sm text-destructive" role="alert">
          {error}
        </p>
      ) : null}
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
  return state.slots.find(
    (slot) =>
      slot.slotId &&
      slot.model === trimmed &&
      slot.runId === runId &&
      slot.clipNonce === state.nonce,
  );
}

function compareListenSrc(path: string, nonce: number): string {
  const src = convertFileSrc(path.replace(/\\/g, "/"));
  return `${src}?n=${nonce}`;
}
