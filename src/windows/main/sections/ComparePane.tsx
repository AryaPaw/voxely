import { useCallback, useEffect, useMemo, useRef, useState } from "react";
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
import { ExternalLink } from "lucide-react";
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
  revision: 0,
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
  const appliedRevisionRef = useRef(0);
  const startingRef = useRef(false);
  const mountedRef = useRef(true);
  const ownedNonceRef = useRef<number | null>(null);
  const stopRequestsRef = useRef(new Map<number, Promise<CompareState>>());
  const attachAudio = useCallback((node: HTMLAudioElement | null) => {
    if (audioRef.current !== node) audioRef.current?.pause();
    audioRef.current = node;
  }, []);
  const stopCapture = useCallback((nonce: number) => {
    const existing = stopRequestsRef.current.get(nonce);
    if (existing) return existing;
    const request = api
      .stopModelCompare(nonce)
      .then((next) => {
        if (ownedNonceRef.current === nonce) ownedNonceRef.current = null;
        return next;
      })
      .finally(() => {
        stopRequestsRef.current.delete(nonce);
      });
    stopRequestsRef.current.set(nonce, request);
    return request;
  }, []);
  const applySnapshot = useCallback((next: CompareState) => {
    if (!Number.isSafeInteger(next.revision) || next.revision <= appliedRevisionRef.current) return;
    appliedRevisionRef.current = next.revision;
    setState(next);
  }, []);
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
    let subscribed = false;
    const dispose = () => {
      const stateListener = stopState;
      const errorListener = stopError;
      stopState = undefined;
      stopError = undefined;
      stateListener?.();
      errorListener?.();
    };
    void (async () => {
      try {
        stopState = await listen<CompareState>("compare://state", (event) => {
          if (!cancelled) applySnapshot(event.payload);
        });
        if (cancelled) {
          dispose();
          return;
        }
        stopError = await listen<string>("compare://error", (event) => {
          if (!cancelled) setError(formatInvokeError(event.payload, copy));
        });
        if (cancelled) {
          dispose();
          return;
        }

        subscribed = true;
        const next = await api.getModelCompare();
        if (!cancelled) applySnapshot(next);
      } catch (err) {
        if (!subscribed) dispose();
        if (!cancelled) setError(formatInvokeError(err, copy));
      }
    })();
    return () => {
      cancelled = true;
      dispose();
    };
  }, [applySnapshot, copy]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      const nonce = ownedNonceRef.current;
      if (nonce !== null) {
        void stopCapture(nonce).catch(() => undefined);
      }
    };
  }, [stopCapture]);

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
    if (state.recording) {
      try {
        const next = await stopCapture(state.nonce);
        applySnapshot(next);
      } catch (err) {
        if (mountedRef.current) setError(formatInvokeError(err, copy));
      }
      return;
    }
    if (startingRef.current) return;
    startingRef.current = true;
    try {
      const nonce = await api.startModelCompare();
      ownedNonceRef.current = nonce;
      if (!mountedRef.current) {
        await stopCapture(nonce);
        return;
      }
      const next = await api.getModelCompare();
      if (mountedRef.current) applySnapshot(next);
      else if (ownedNonceRef.current === nonce) await stopCapture(nonce);
    } catch (err) {
      if (mountedRef.current) setError(formatInvokeError(err, copy));
    } finally {
      startingRef.current = false;
    }
  }

  async function run() {
    setError("");
    try {
      const next = await api.runModelCompare();
      applySnapshot(next);
    } catch (err) {
      setError(formatInvokeError(err, copy));
    }
  }

  function cancelRun() {
    void api
      .cancelModelCompare()
      .then((next) => {
        applySnapshot(next);
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
          <ExternalLink aria-hidden="true" />
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
          ref={attachAudio}
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
