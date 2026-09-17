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
        <PageHeader icon={SECTION_ICONS.history} title={sectionLabel(copy, "history")} />
        <p className="mt-1 text-sm text-muted-foreground">{copy.historyHint}</p>
        {recovered ? (
          <p className="mt-3 rounded-lg bg-muted px-3 py-2 text-sm">{copy.settingsRecovered}</p>
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
          <Button variant="outline" onClick={() => void api.openAudioDir()}>
            <FolderOpen className="h-4 w-4" /> {copy.folder}
          </Button>
          <Button variant="outline" onClick={onOpenSettings}>
            <SettingsIcon className="h-4 w-4" /> {copy.settings}
          </Button>
        </div>
        <div className="mt-5 min-h-0 flex-1 space-y-3 overflow-auto pb-8">
          {items.length === 0 ? (
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
            <Button type="button" variant="outline" onClick={() => onLoadMore?.()}>
              {copy.loadMore}
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
  const [loadAudio, setLoadAudio] = useState(false);
  const [audioMissing, setAudioMissing] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [progress, setProgress] = useState(0);
  const [current, setCurrent] = useState(0);
  const [copied, setCopied] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const pendingPlay = useRef(false);
  const copiedTimer = useRef<number | null>(null);
  const failed = item.status === "failed" || item.status === "interrupted";
  const processing = item.status === "processing";
  const emptySuccess = item.status === "completed" && !(item.transcript ?? "").trim();
  const canRetry = Boolean(item.rawAudioPath || item.processedAudioPath);
  const processingLabel = item.processedAudioPath ? copy.processing : copy.processingAudio;

  useEffect(() => {
    if (!loadAudio) {
      return;
    }
    let cancelled = false;
    void api
      .audioPath(item.id)
      .then((path) => {
        if (cancelled) {
          return;
        }
        if (path) {
          setAudioSrc(convertFileSrc(path.replace(/\\/g, "/")));
          setAudioMissing(false);
        } else {
          setAudioSrc(null);
          setAudioMissing(true);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setAudioSrc(null);
          setAudioMissing(true);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [loadAudio, item.id, item.processedAudioPath, item.rawAudioPath]);

  useEffect(() => {
    if (!pendingPlay.current || !audioSrc) {
      return;
    }
    pendingPlay.current = false;
    void audioRef.current?.play();
  }, [audioSrc]);

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
        void node.play().catch(() => {
          setAudioMissing(true);
          reportError(copy.playbackFailed);
        });
      } else {
        node.pause();
      }
      return;
    }
    pendingPlay.current = true;
    setLoadAudio(true);
  }

  async function retryTranscription() {
    setRetrying(true);
    try {
      await api.retry(item.id);
      await onRefresh();
    } catch (error) {
      reportError(formatInvokeError(error, copy));
    } finally {
      setRetrying(false);
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
                void api.copy(item.transcript ?? "").then(() => {
                  setCopied(true);
                  if (copiedTimer.current !== null) {
                    window.clearTimeout(copiedTimer.current);
                  }
                  copiedTimer.current = window.setTimeout(() => {
                    copiedTimer.current = null;
                    setCopied(false);
                  }, 1400);
                });
              }}
            >
              {copied ? <Check className="h-4 w-4" /> : <Copy className="h-4 w-4" />}
            </Button>
          ) : null}
          {canRetry ? (
            <Button
              type="button"
              size="icon"
              variant="ghost"
              aria-label={copy.retry}
              disabled={retrying || processing}
              onClick={() => void retryTranscription()}
            >
              <RotateCcw className={`h-4 w-4 text-primary${retrying ? " history-spin" : ""}`} />
            </Button>
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
            {copy.transcriptUnavailable}{" "}
            <button
              type="button"
              className="text-primary underline-offset-2 hover:underline"
              onClick={() => void retryTranscription()}
            >
              {copy.retry}
            </button>
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
      <div className="mt-3 flex items-center gap-3">
        <button
          type="button"
          className="history-play"
          aria-label={playing ? copy.pause : copy.listen}
          disabled={audioMissing || !canRetry}
          onClick={togglePlay}
        >
          {playing ? <Pause className="h-3.5 w-3.5" /> : <Play className="h-3.5 w-3.5" />}
        </button>
        <div className="relative h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
          <div
            className="h-full rounded-full bg-primary/70"
            style={{ width: `${progress * 100}%` }}
          />
        </div>
        <div className="shrink-0 text-right text-[11px] whitespace-nowrap tabular-nums text-muted-foreground">
          {duration(current)} / {duration(item.durationMs)}
        </div>
      </div>
      {audioSrc ? (
        <audio
          ref={audioRef}
          src={audioSrc}
          className="hidden"
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
        <dl className="mt-3 grid grid-cols-2 gap-2 text-[11px] text-muted-foreground">
          <div>
            <dt className="inline">{copy.modelLabel}: </dt>
            <dd className="inline">{item.model}</dd>
          </div>
          <div>
            <dt className="inline">{copy.latencyLabel}: </dt>
            <dd className="inline">
              {item.latencyMs == null ? "—" : copy.latencyMs.replace("{n}", String(item.latencyMs))}
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
      ) : null}
    </article>
  );
}
