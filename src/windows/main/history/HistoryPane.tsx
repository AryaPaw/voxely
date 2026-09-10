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
import type { Messages } from "../../../lib/i18n";
import { formatDuration, formatTime } from "../../../lib/utils";
import { PageHeader } from "../../../components/settings/PageHeader";
import { Button } from "../../../components/ui/button";
import { Input } from "../../../components/ui/input";
import { SECTION_ICONS } from "../sectionNav";

export function HistoryPane({
  items,
  query,
  keyConfigured,
  hotkey,
  onQuery,
  onRefresh,
  onOpenKey,
  onOpenSettings,
  copy,
}: {
  items: Recording[];
  query: string;
  keyConfigured: boolean;
  hotkey: string;
  onQuery: (value: string) => void;
  onRefresh: () => Promise<void>;
  onOpenKey: () => void;
  onOpenSettings: () => void;
  copy: Messages;
}) {
  const [detailsId, setDetailsId] = useState<string | null>(null);

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-bg">
      {!keyConfigured ? (
        <div className="flex items-center justify-between gap-3 border-b border-border bg-surface px-6 py-2 text-sm">
          <span>Нет API-ключа OpenRouter. Запись сохранится, расшифровка не отправится.</span>
          <Button size="sm" onClick={onOpenKey}>
            Добавить ключ
          </Button>
        </div>
      ) : null}
      <div className="mx-auto flex min-h-0 w-full max-w-3xl flex-1 flex-col px-6 py-6">
        <PageHeader icon={SECTION_ICONS.history} title={copy.historyTitle} />
        <p className="mt-1 text-sm text-muted-foreground">{copy.historyHint}</p>
        <div className="mt-4 flex items-center gap-2">
          <Input
            value={query}
            onChange={(e) => onQuery(e.target.value)}
            placeholder={copy.search}
            className="flex-1"
          />
          <Button variant="outline" onClick={() => void api.openAudioDir()}>
            <FolderOpen className="h-4 w-4" /> {copy.folder}
          </Button>
          <Button variant="outline" onClick={onOpenSettings}>
            <SettingsIcon className="h-4 w-4" /> {copy.settings}
          </Button>
        </div>
        <div className="mt-5 min-h-0 flex-1 space-y-3 overflow-auto pb-8">
          {items.length === 0 ? (
            <div className="rounded-2xl bg-surface px-5 py-10 text-sm text-muted-foreground">
              {copy.emptyHistory.replace("{hotkey}", hotkey)}
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
        </div>
      </div>
    </div>
  );
}

function HistoryCard({
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
  const [playing, setPlaying] = useState(false);
  const [progress, setProgress] = useState(0);
  const [current, setCurrent] = useState(0);
  const [copied, setCopied] = useState(false);
  const [retrying, setRetrying] = useState(false);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const failed = item.status === "failed" && !item.transcript;
  const processing = item.status === "processing" && !item.transcript;

  useEffect(() => {
    let cancelled = false;
    void api.audioPath(item.id).then((path) => {
      if (!cancelled) {
        setAudioSrc(path ? convertFileSrc(path.replace(/\\/g, "/")) : null);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [item.id, item.processedAudioPath, item.rawAudioPath]);

  function togglePlay() {
    const node = audioRef.current;
    if (!node) {
      return;
    }
    if (node.paused) {
      void node.play();
    } else {
      node.pause();
    }
  }

  return (
    <article className="history-card">
      <div className="flex items-start justify-between gap-3">
        <div className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
          {formatTime(item.createdAt)}
          <span className="mx-2 opacity-50">|</span>
          {Math.max(0, Math.round(item.durationMs / 1000))} с
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
                  window.setTimeout(() => setCopied(false), 1400);
                });
              }}
            >
              {copied ? <Check className="h-4 w-4" /> : <Copy className="h-4 w-4" />}
            </Button>
          ) : (
            <Button
              type="button"
              size="icon"
              variant="ghost"
              aria-label={copy.retry}
              disabled={retrying}
              onClick={async () => {
                setRetrying(true);
                try {
                  await api.retry(item.id);
                  await onRefresh();
                } finally {
                  setRetrying(false);
                }
              }}
            >
              <RotateCcw className={`h-4 w-4${retrying ? " history-spin" : ""}`} />
            </Button>
          )}
          <Button type="button" size="icon" variant="ghost" aria-label="Сведения" onClick={onToggleDetails}>
            <Info className="h-4 w-4" />
          </Button>
          <Button
            type="button"
            size="icon"
            variant="ghost"
            aria-label="Удалить"
            onClick={async () => {
              await api.deleteItem(item.id);
              await onRefresh();
            }}
          >
            <Trash2 className="h-4 w-4" />
          </Button>
        </div>
      </div>
      <p className="mt-2 text-sm leading-6">
        {failed ? (
          <>
            Расшифровка недоступна.{" "}
            <button
              type="button"
              className="text-sky-400 underline-offset-2 hover:underline"
              onClick={async () => {
                setRetrying(true);
                try {
                  await api.retry(item.id);
                  await onRefresh();
                } finally {
                  setRetrying(false);
                }
              }}
            >
              {copy.retry}
            </button>
          </>
        ) : processing ? (
          <span className="status-live">
            {copy.processing}
            <span className="overlay-ellipsis" aria-hidden="true">
              <span>.</span>
              <span>.</span>
              <span>.</span>
            </span>
          </span>
        ) : (
          (item.transcript ?? copy.processing)
        )}
      </p>
      <div className="mt-3 flex items-center gap-3">
        <button
          type="button"
          className="history-play"
          aria-label={playing ? "Пауза" : "Слушать"}
          disabled={!audioSrc}
          onClick={togglePlay}
        >
          {playing ? <Pause className="h-3.5 w-3.5" /> : <Play className="h-3.5 w-3.5" />}
        </button>
        <div className="relative h-1.5 flex-1 overflow-hidden rounded-full bg-white/10">
          <div
            className="h-full rounded-full bg-white/55"
            style={{ width: `${progress * 100}%` }}
          />
        </div>
        <div className="w-16 text-right text-[11px] tabular-nums text-muted-foreground">
          {formatDuration(current)} / {formatDuration(item.durationMs)}
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
        />
      ) : null}
      {detailsOpen ? (
        <dl className="mt-3 grid grid-cols-2 gap-2 text-[11px] text-muted-foreground">
          <div>Модель: {item.model}</div>
          <div>Задержка: {item.latencyMs ?? "—"} мс</div>
          <div>Стоимость: {item.cost ?? "—"}</div>
          <div>Ошибка: {item.lastErrorCode ?? "—"}</div>
        </dl>
      ) : null}
    </article>
  );
}
