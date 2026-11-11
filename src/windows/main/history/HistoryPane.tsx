import { useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  Check,
  Copy,
  FolderOpen,
  Info,
  Pause,
  Play,
  RotateCcw,
  SlidersHorizontal,
  Square,
  Settings as SettingsIcon,
  Trash2,
} from "lucide-react";
import { api, type Recording } from "../../../lib/api";
import { reportError } from "../../../lib/system-notify";
import { formatInvokeError, localizedError, type Messages } from "../../../lib/i18n";
import { formatDuration, formatTime } from "../../../lib/utils";
import { PageHeader } from "../../../components/settings/PageHeader";
import { Button } from "../../../components/ui/button";
import { Input } from "../../../components/ui/input";
import { Tabs, TabsList, TabsTrigger } from "../../../components/ui/tabs";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "../../../components/ui/alert-dialog";
import { SECTION_ICONS, sectionLabel } from "../sectionNav";

export function HistoryPane({
  items,
  query,
  hasMore = false,
  recovered = false,
  keyConfigured,
  hotkey,
  onQuery,
  onClearQuery,
  onLoadMore,
  onRefresh,
  historyError,
  isLoadingMore = false,
  onOpenKey,
  onOpenSettings,
  copy,
}: {
  items: Recording[];
  query: string;
  hasMore?: boolean;
  recovered?: boolean;
  keyConfigured: boolean;
  hotkey: string;
  onQuery: (value: string) => void;
  onClearQuery?: () => void;
  onLoadMore?: () => void;
  onRefresh: () => Promise<void>;
  historyError?: "refresh" | "page" | null;
  isLoadingMore?: boolean;
  onOpenKey: () => void;
  onOpenSettings: () => void;
  copy: Messages;
}) {
  const [detailsId, setDetailsId] = useState<string | null>(null);

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-background">
      {!keyConfigured ? (
        <div className="flex items-center justify-between gap-3 border-b border-border bg-muted px-6 py-2 text-sm">
          <span>{copy.missingApiKeyBanner}</span>
          <Button size="sm" onClick={onOpenKey}>
            {copy.addApiKey}
          </Button>
        </div>
      ) : null}
      <div className="mx-auto flex min-h-0 w-full max-w-3xl flex-1 flex-col px-6 py-6">
        <PageHeader
          icon={SECTION_ICONS.history}
          title={sectionLabel(copy, "history")}
          description={copy.historyHint}
        />
        {recovered ? (
          <div className="mt-3 flex flex-wrap items-center justify-between gap-3 rounded-lg bg-muted px-3 py-2 text-sm">
            <p>{copy.settingsRecovered}</p>
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={() =>
                void api.openSettingsDir().catch((error: unknown) => {
                  reportError(formatInvokeError(error, copy));
                })
              }
            >
              {copy.settingsRecoveryOpenFolder}
            </Button>
          </div>
        ) : null}
        <div className="mt-4 flex items-center gap-2">
          <Input
            value={query}
            onChange={(e) => onQuery(e.target.value)}
            placeholder={copy.search}
            aria-label={copy.search}
            className="flex-1"
          />
          {query.trim() ? (
            <Button type="button" variant="outline" onClick={() => onClearQuery?.()}>
              {copy.clearSearch}
            </Button>
          ) : null}
          <Button
            variant="outline"
            onClick={() =>
              void api.openAudioDir().catch((error: unknown) => {
                reportError(formatInvokeError(error, copy));
              })
            }
          >
            <FolderOpen className="h-4 w-4" /> {copy.folder}
          </Button>
          <Button variant="outline" onClick={onOpenSettings}>
            <SettingsIcon className="h-4 w-4" /> {copy.settings}
          </Button>
        </div>
        <div className="mt-5 min-h-0 flex-1 space-y-3 overflow-auto pb-8">
          {historyError ? (
            <div
              role="alert"
              className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-destructive/30 bg-destructive/5 px-4 py-3 text-sm"
            >
              <span>{copy.historyLoadFailed}</span>
              <Button
                type="button"
                size="sm"
                variant="outline"
                onClick={() => (historyError === "page" ? onLoadMore?.() : void onRefresh())}
              >
                {copy.retryLoad}
              </Button>
            </div>
          ) : null}
          {items.length === 0 && historyError ? null : items.length === 0 ? (
            <div className="rounded-2xl bg-muted px-5 py-10 text-sm text-muted-foreground">
              {query.trim() ? copy.noSearchResults : copy.emptyHistory.replace("{hotkey}", hotkey)}
            </div>
          ) : (
            items.map((item) => (
              <HistoryCard
                key={item.id}
                item={item}
                copy={copy}
                detailsOpen={detailsId === item.id}
                onToggleDetails={() =>
                  setDetailsId((current) => (current === item.id ? null : item.id))
                }
                onRefresh={onRefresh}
              />
            ))
          )}
          {hasMore ? (
            <Button
              type="button"
              variant="outline"
              disabled={isLoadingMore}
              onClick={() => onLoadMore?.()}
            >
              {isLoadingMore ? copy.loading : copy.loadMore}
            </Button>
          ) : null}
        </div>
      </div>
    </div>
  );
}

export function HistoryCard({
  item,
  copy,
  detailsOpen,
  onToggleDetails,
  onRefresh,
}: {
  item: Recording;
  copy: Messages;
  detailsOpen: boolean;
  onToggleDetails: () => void;
  onRefresh: () => Promise<void>;
}) {
  const [audioSrc, setAudioSrc] = useState<string | null>(null);
  const [audioVariant, setAudioVariant] = useState<"full" | "api">("full");
  const [audioLoading, setAudioLoading] = useState(false);
  const [audioDuration, setAudioDuration] = useState<number | null>(item.durationMs);
  const [loadAudio, setLoadAudio] = useState(false);
  const [audioMissing, setAudioMissing] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [progress, setProgress] = useState(0);
  const [current, setCurrent] = useState(0);
  const [copied, setCopied] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const pendingPlay = useRef(false);
  const playback = useRef({ generation: 0 }).current;
  const copiedTimer = useRef<number | null>(null);
  const failed = item.status === "failed" || item.status === "interrupted";
  const processing = item.status === "processing";
  const emptySuccess = item.status === "completed" && !(item.transcript ?? "").trim();
  const canRetry = Boolean((item.rawAudioPath || item.processedAudioPath) && item.durationMs > 0);
  const processingLabel = item.processedAudioPath ? copy.processing : copy.processingAudio;
  const busy = processing || retrying;
  const latestStatus =
    item.lastErrorCode === "RecordingTooLarge" || item.lastErrorCode === "RecordingTruncated"
      ? localizedError(item.lastErrorCode, copy, item.lastErrorMessage ?? undefined)
      : failed
        ? item.lastErrorCode
          ? localizedError(item.lastErrorCode, copy, item.lastErrorMessage ?? undefined)
          : (item.lastErrorMessage ?? copy.transcriptionFailed)
        : processing && item.transcript
          ? copy.retryingTranscript
          : null;

  useEffect(() => {
    if (!loadAudio) {
      return;
    }
    let cancelled = false;
    playback.generation++;
    let objectUrl: string | null = null;
    setAudioLoading(true);
    setAudioSrc(null);
    setPlaying(false);
    setCurrent(0);
    setProgress(0);
    setAudioDuration(audioVariant === "full" ? item.durationMs : null);
    const source =
      audioVariant === "api"
        ? api.apiAudio(item.id).then((bytes) => {
            if (cancelled) return null;
            objectUrl = URL.createObjectURL(new Blob([bytes], { type: "audio/wav" }));
            return objectUrl;
          })
        : api
            .audioPath(item.id)
            .then((path) => (path ? convertFileSrc(path.replace(/\\/g, "/")) : null));
    void source
      .then((path) => {
        if (cancelled) {
          return;
        }
        if (path) {
          setAudioSrc(path);
          setAudioMissing(false);
        } else {
          setAudioSrc(null);
          setAudioMissing(true);
          pendingPlay.current = false;
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setAudioSrc(null);
          setAudioMissing(true);
          pendingPlay.current = false;
          reportError(formatInvokeError(error, copy));
        }
      })
      .finally(() => {
        if (!cancelled) setAudioLoading(false);
      });
    return () => {
      cancelled = true;
      playback.generation++;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [
    loadAudio,
    item.id,
    item.processedAudioPath,
    item.rawAudioPath,
    item.attemptCount,
    item.status,
    item.durationMs,
    audioVariant,
    copy,
    playback,
  ]);

  useEffect(() => {
    if (!pendingPlay.current || !audioSrc) {
      return;
    }
    pendingPlay.current = false;
    const node = audioRef.current;
    if (!node) return;
    const generation = ++playback.generation;
    void node.play().catch(() => {
      if (generation !== playback.generation || audioRef.current !== node) return;
      setAudioMissing(true);
      reportError(copy.playbackFailed);
    });
  }, [audioSrc, copy.playbackFailed, playback]);

  useEffect(() => {
    return () => {
      if (copiedTimer.current !== null) {
        window.clearTimeout(copiedTimer.current);
      }
    };
  }, []);

  function togglePlay() {
    const node = audioRef.current;
    if (node) {
      if (node.paused) {
        const generation = ++playback.generation;
        void node.play().catch(() => {
          if (generation !== playback.generation || audioRef.current !== node) return;
          setAudioMissing(true);
          reportError(copy.playbackFailed);
        });
      } else {
        playback.generation++;
        node.pause();
      }
      return;
    }
    pendingPlay.current = true;
    setLoadAudio(true);
  }

  async function retryTranscription() {
    if (retrying || processing) {
      return;
    }
    setRetrying(true);
    try {
      await api.retry(item.id);
    } catch (error) {
      reportError(formatInvokeError(error, copy));
    } finally {
      setRetrying(false);
      try {
        await onRefresh();
      } catch (error) {
        reportError(formatInvokeError(error, copy));
      }
    }
  }

  async function reprocessOriginal() {
    if (retrying || processing || !item.rawAudioPath) {
      return;
    }
    setRetrying(true);
    try {
      await api.manualReprocess(item.id);
    } catch (error) {
      reportError(formatInvokeError(error, copy));
    } finally {
      setRetrying(false);
      try {
        await onRefresh();
      } catch (error) {
        reportError(formatInvokeError(error, copy));
      }
    }
  }

  const errorText = item.lastErrorCode
    ? localizedError(item.lastErrorCode, copy, item.lastErrorMessage ?? undefined)
    : (item.lastErrorMessage ?? "—");
  const duration = (ms: number) =>
    formatDuration(ms, {
      seconds: copy.durationSeconds,
      minutes: copy.durationMinutes,
    });
  // Exact minutes omit seconds, so their label isn't the widest possible one.
  const durationWidthSample = duration(Math.floor(item.durationMs / 60_000) * 60_000 + 59_000);

  return (
    <article className="history-card">
      <div className="flex items-start justify-between gap-3">
        <div className="text-[11px] font-medium tracking-wide text-muted-foreground">
          {formatTime(item.createdAt, copy.dateLocale)}
          <span className="mx-2 opacity-50">|</span>
          {duration(item.durationMs)}
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {item.transcript ? (
            <Button
              type="button"
              size="icon"
              variant="ghost"
              aria-label={copied ? copy.copied : copy.copy}
              onClick={() => {
                void api
                  .copy(item.transcript ?? "")
                  .then(() => {
                    setCopied(true);
                    if (copiedTimer.current !== null) {
                      window.clearTimeout(copiedTimer.current);
                    }
                    copiedTimer.current = window.setTimeout(() => {
                      copiedTimer.current = null;
                      setCopied(false);
                    }, 1400);
                  })
                  .catch((error: unknown) => {
                    reportError(formatInvokeError(error, copy));
                  });
              }}
            >
              {copied ? <Check className="h-4 w-4" /> : <Copy className="h-4 w-4" />}
            </Button>
          ) : null}
          {busy ? (
            <Button
              type="button"
              size="icon"
              variant="ghost"
              aria-label={copy.cancelRetry}
              onClick={() => {
                void api.cancelRetry(item.id).catch((error: unknown) => {
                  reportError(formatInvokeError(error, copy));
                });
              }}
            >
              <Square className="h-4 w-4 text-primary-text" />
            </Button>
          ) : canRetry ? (
            <>
              <Button
                type="button"
                size="icon"
                variant="ghost"
                aria-label={copy.retry}
                disabled={busy}
                onClick={() => void retryTranscription()}
              >
                <RotateCcw className={`h-4 w-4 text-primary-text${busy ? " history-spin" : ""}`} />
              </Button>
              {item.rawAudioPath ? (
                <Button
                  type="button"
                  size="icon"
                  variant="ghost"
                  aria-label={copy.reprocessOriginal}
                  disabled={busy}
                  onClick={() => void reprocessOriginal()}
                >
                  <SlidersHorizontal className="h-4 w-4 text-primary-text" />
                </Button>
              ) : null}
            </>
          ) : null}
          <Button
            type="button"
            size="icon"
            variant="ghost"
            aria-label={copy.details}
            aria-expanded={detailsOpen}
            onClick={onToggleDetails}
          >
            <Info className="h-4 w-4" />
          </Button>
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button type="button" size="icon" variant="ghost" aria-label={copy.delete}>
                <Trash2 className="h-4 w-4" />
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>{copy.deleteItemConfirm}</AlertDialogTitle>
                <AlertDialogDescription>{copy.deleteAllCannotUndo}</AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>{copy.cancel}</AlertDialogCancel>
                <AlertDialogAction
                  variant="destructive"
                  onClick={async () => {
                    try {
                      await api.deleteItem(item.id);
                      await onRefresh();
                    } catch (error) {
                      reportError(formatInvokeError(error, copy));
                    }
                  }}
                >
                  {copy.delete}
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        </div>
      </div>
      <p className="mt-2 whitespace-pre-wrap break-words text-sm leading-6">
        {failed && !item.transcript ? (
          <>
            {copy.transcriptUnavailable}
            {canRetry ? (
              <>
                {" "}
                <button
                  type="button"
                  className="text-primary-text underline-offset-2 hover:underline"
                  disabled={busy}
                  onClick={() => void retryTranscription()}
                >
                  {copy.retry}
                </button>
              </>
            ) : null}
          </>
        ) : processing && !item.transcript ? (
          <span className="status-live">
            {processingLabel}
            <span className="overlay-ellipsis" aria-hidden="true">
              <span>.</span>
              <span>.</span>
              <span>.</span>
            </span>
          </span>
        ) : emptySuccess ? (
          copy.emptyTranscript
        ) : (
          (item.transcript ?? copy.processing)
        )}
      </p>
      {latestStatus ? (
        <p role="status" className="mt-2 text-xs text-destructive">
          {latestStatus}
          {failed && !canRetry ? ` ${copy.retryUnavailable}` : ""}
        </p>
      ) : null}
      <div className="mt-3 flex items-center gap-3">
        <Tabs
          value={audioVariant}
          onValueChange={(value) => {
            playback.generation++;
            audioRef.current?.pause();
            pendingPlay.current = false;
            setAudioMissing(false);
            setAudioVariant(value as "full" | "api");
            setLoadAudio(true);
          }}
        >
          <TabsList aria-label={copy.audioVariant} className="h-7 gap-0 p-0.5">
            <TabsTrigger
              value="full"
              disabled={audioLoading || !canRetry}
              className="h-6 px-2 text-xs disabled:opacity-100"
            >
              {copy.audioFull}
            </TabsTrigger>
            <TabsTrigger
              value="api"
              disabled={audioLoading || !canRetry}
              className="h-6 px-2 text-xs disabled:opacity-100"
              title={copy.audioApiHint}
            >
              {copy.audioForApi}
            </TabsTrigger>
          </TabsList>
        </Tabs>
        <button
          type="button"
          className="history-play"
          aria-label={playing ? copy.pause : copy.listen}
          disabled={audioLoading || audioMissing || !canRetry}
          aria-busy={audioLoading}
          onClick={togglePlay}
        >
          {playing ? <Pause className="h-3.5 w-3.5" /> : <Play className="h-3.5 w-3.5" />}
        </button>
        <div className="relative h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
          <div
            className="h-full rounded-full bg-primary-text"
            style={{ width: `${progress * 100}%` }}
          />
        </div>
        <div className="relative grid shrink-0 text-right text-[11px] whitespace-nowrap tabular-nums text-muted-foreground">
          <span aria-hidden="true" className="invisible col-start-1 row-start-1">
            {durationWidthSample} / {durationWidthSample}
          </span>
          <span className="col-start-1 row-start-1">
            {duration(current)} / {audioDuration === null ? "--:--" : duration(audioDuration)}
          </span>
          {audioLoading ? (
            <span role="status" className="sr-only">
              {copy.loading}
            </span>
          ) : null}
        </div>
      </div>
      {audioSrc ? (
        <audio
          ref={audioRef}
          src={audioSrc}
          className="hidden"
          onLoadedMetadata={(event) => {
            const seconds = event.currentTarget.duration;
            if (Number.isFinite(seconds)) setAudioDuration(seconds * 1000);
          }}
          onPlay={() => setPlaying(true)}
          onPause={() => setPlaying(false)}
          onTimeUpdate={(event) => {
            const node = event.currentTarget;
            setCurrent(node.currentTime * 1000);
            setProgress(node.duration > 0 ? node.currentTime / node.duration : 0);
          }}
          onEnded={() => {
            setPlaying(false);
            setProgress(1);
          }}
          onError={() => {
            setPlaying(false);
            setAudioMissing(true);
            reportError(copy.playbackFailed);
          }}
        />
      ) : null}
      {detailsOpen ? (
        <>
          <dl className="mt-3 grid grid-cols-2 gap-2 text-[11px] text-muted-foreground">
            <div>
              <dt className="inline">{copy.modelLabel}: </dt>
              <dd className="inline">{item.model}</dd>
            </div>
            <div>
              <dt className="inline">{copy.latencyLabel}: </dt>
              <dd className="inline">
                {item.latencyMs == null
                  ? "—"
                  : copy.latencyMs.replace("{n}", String(item.latencyMs))}
              </dd>
            </div>
            <div>
              <dt className="inline">{copy.costLabel}: </dt>
              <dd className="inline">{item.cost ?? "—"}</dd>
            </div>
            <div>
              <dt className="inline">{copy.errorLabel}: </dt>
              <dd className="inline">{errorText}</dd>
            </div>
          </dl>
          {canRetry ? (
            <div className="mt-2 space-y-1 text-[11px] text-muted-foreground">
              <p>
                {item.processedAudioPath ? copy.retryUsesProcessedAudio : copy.retryUsesRawAudio}
              </p>
              {item.rawAudioPath ? <p>{copy.reprocessOriginalHint}</p> : null}
            </div>
          ) : null}
        </>
      ) : null}
    </article>
  );
}
