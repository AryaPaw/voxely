import { useEffect, useRef, useState } from "react";
import type { AppSettings, RetentionApplyResult, RetentionPreview } from "../../../lib/api";
import type { Messages } from "../../../lib/i18n";
import { PageHeader } from "../../../components/settings/PageHeader";
import { SettingsField } from "../../../components/settings/SettingsField";
import { SettingsSwitchRow } from "../../../components/settings/SettingsSwitchRow";
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
import { Button } from "../../../components/ui/button";
import { Input } from "../../../components/ui/input";
import { SimpleSelect } from "../../../components/ui/simple-select";
import { SECTION_ICONS } from "../sectionNav";

const MAX_STORAGE_LIMIT_GB = 17_179_869_183;

type RetentionDraft = {
  retention: string;
  storageLimitGb: string;
  legacyStorageLimit: "500mb" | null;
  unlimited: boolean;
};

function retentionDraftFromSettings(retention: string, storageLimit: string): RetentionDraft {
  const match = /^(\d+)gb$/.exec(storageLimit);
  return {
    retention,
    storageLimitGb: match?.[1] ?? (storageLimit === "500mb" ? "" : "5"),
    legacyStorageLimit: storageLimit === "500mb" ? "500mb" : null,
    unlimited: storageLimit === "unlimited",
  };
}

function storageLimitSetting(draft: RetentionDraft): string | null {
  if (draft.unlimited) return "unlimited";
  if (draft.legacyStorageLimit && draft.storageLimitGb === "") {
    return draft.legacyStorageLimit;
  }
  if (!/^\d+$/.test(draft.storageLimitGb)) return null;
  const gigabytes = Number(draft.storageLimitGb);
  if (!Number.isSafeInteger(gigabytes) || gigabytes < 1 || gigabytes > MAX_STORAGE_LIMIT_GB) {
    return null;
  }
  return `${gigabytes}gb`;
}

function isValidRetentionPreview(preview: RetentionPreview) {
  return (
    Array.isArray(preview.recordingIdsToDelete) &&
    preview.recordingIdsToDelete.length === preview.entriesToDelete &&
    preview.recordingIdsToDelete.every((id) => typeof id === "string" && id.length > 0) &&
    new Set(preview.recordingIdsToDelete).size === preview.recordingIdsToDelete.length &&
    [
      preview.entriesToDelete,
      preview.filesToDelete,
      preview.bytesToFree,
      preview.protectedEntries,
      preview.protectedBytes,
      preview.totalAudioBytes,
    ].every((value) => Number.isSafeInteger(value) && value >= 0)
  );
}

export function HistorySettings({
  settings,
  copy,
  onChange,
  onDeleteAll,
  onPreviewRetention,
  onApplyRetention,
}: {
  settings: AppSettings;
  copy: Messages;
  onChange: (patch: Partial<AppSettings>) => void;
  onDeleteAll: () => Promise<void>;
  onPreviewRetention: (settings: AppSettings) => Promise<RetentionPreview>;
  onApplyRetention: (
    settings: AppSettings,
    expectedPreview: RetentionPreview,
  ) => Promise<RetentionApplyResult>;
}) {
  const [deleting, setDeleting] = useState(false);
  const [draft, setDraft] = useState<RetentionDraft>(() =>
    retentionDraftFromSettings(settings.retention, settings.storageLimit),
  );
  const [preview, setPreview] = useState<RetentionPreview | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewFailed, setPreviewFailed] = useState(false);
  const [applyFailed, setApplyFailed] = useState(false);
  const [applyDialogOpen, setApplyDialogOpen] = useState(false);
  const [applying, setApplying] = useState(false);
  const deletePendingRef = useRef(false);
  const previewRequestRef = useRef(0);
  const applyPendingRef = useRef(false);
  const requestedStorageLimit = storageLimitSetting(draft);
  const retentionChanged =
    draft.retention !== settings.retention || requestedStorageLimit !== settings.storageLimit;

  useEffect(() => {
    previewRequestRef.current += 1;
    setDraft(retentionDraftFromSettings(settings.retention, settings.storageLimit));
    setPreview(null);
    setPreviewFailed(false);
    setApplyFailed(false);
  }, [settings.retention, settings.storageLimit]);

  async function updateRetentionDraft(patch: Partial<RetentionDraft>, previewChange = true) {
    const next = { ...draft, ...patch };
    setDraft(next);
    setPreview(null);
    setPreviewFailed(false);
    setApplyFailed(false);
    const requestId = ++previewRequestRef.current;
    const storageLimit = storageLimitSetting(next);
    if (!previewChange || !storageLimit) {
      setPreviewLoading(false);
      return;
    }
    if (next.retention === settings.retention && storageLimit === settings.storageLimit) {
      setPreviewLoading(false);
      return;
    }
    setPreviewLoading(true);
    try {
      const result = await onPreviewRetention({
        ...settings,
        retention: next.retention,
        storageLimit,
      });
      if (requestId === previewRequestRef.current) {
        if (isValidRetentionPreview(result)) {
          setPreview(result);
        } else {
          setPreview(null);
          setPreviewFailed(true);
        }
      }
    } catch {
      if (requestId === previewRequestRef.current) setPreviewFailed(true);
    } finally {
      if (requestId === previewRequestRef.current) setPreviewLoading(false);
    }
  }

  function discardRetentionDraft() {
    previewRequestRef.current += 1;
    setDraft(retentionDraftFromSettings(settings.retention, settings.storageLimit));
    setPreview(null);
    setPreviewLoading(false);
    setPreviewFailed(false);
    setApplyFailed(false);
  }

  async function applyRetentionDraft() {
    const storageLimit = storageLimitSetting(draft);
    if (
      !retentionChanged ||
      !storageLimit ||
      !preview ||
      !isValidRetentionPreview(preview) ||
      applyPendingRef.current
    )
      return;
    applyPendingRef.current = true;
    setApplying(true);
    setApplyFailed(false);
    try {
      await onApplyRetention(
        {
          ...settings,
          retention: draft.retention,
          storageLimit,
        },
        preview,
      );
      setApplyDialogOpen(false);
    } catch {
      setApplyFailed(true);
      try {
        const refreshed = await onPreviewRetention({
          ...settings,
          retention: draft.retention,
          storageLimit,
        });
        if (isValidRetentionPreview(refreshed)) {
          setPreview(refreshed);
          setPreviewFailed(false);
        } else {
          setPreview(null);
          setPreviewFailed(true);
        }
      } catch {
        setPreview(null);
        setPreviewFailed(true);
      }
    } finally {
      applyPendingRef.current = false;
      setApplying(false);
    }
  }

  async function handleDeleteAll() {
    if (deletePendingRef.current) {
      return;
    }
    deletePendingRef.current = true;
    setDeleting(true);
    try {
      await onDeleteAll();
    } finally {
      deletePendingRef.current = false;
      setDeleting(false);
    }
  }

  return (
    <div>
      <PageHeader icon={SECTION_ICONS.historySettings} title={copy.navStorage} />
      <SettingsField label={copy.keepRecordings}>
        <SimpleSelect
          aria-label={copy.keepRecordings}
          value={draft.retention}
          onValueChange={(retention) => void updateRetentionDraft({ retention })}
          options={[
            { value: "1d", label: copy.retain1d },
            { value: "3d", label: copy.retain3d },
            { value: "7d", label: copy.retain7d },
            { value: "30d", label: copy.retain30d },
            { value: "90d", label: copy.retain90d },
            { value: "forever", label: copy.retainForever },
          ]}
        />
      </SettingsField>
      <SettingsField label={copy.storageLimit}>
        <Input
          aria-label={copy.storageLimit}
          type="number"
          min={1}
          max={MAX_STORAGE_LIMIT_GB}
          step={1}
          value={draft.storageLimitGb}
          disabled={draft.unlimited}
          aria-invalid={
            !draft.unlimited && !draft.legacyStorageLimit && storageLimitSetting(draft) === null
          }
          onChange={(event) =>
            void updateRetentionDraft(
              { storageLimitGb: event.currentTarget.value, legacyStorageLimit: null },
              false,
            )
          }
          onBlur={() => void updateRetentionDraft({ storageLimitGb: draft.storageLimitGb })}
        />
      </SettingsField>
      {!draft.unlimited && storageLimitSetting(draft) !== null ? (
        <p className="-mt-3 mb-4 max-w-lg text-xs text-muted-foreground">
          {draft.legacyStorageLimit ? copy.storageLimitLegacyHint : copy.storageLimitHint}
        </p>
      ) : null}
      {!draft.unlimited && !draft.legacyStorageLimit && storageLimitSetting(draft) === null ? (
        <p className="-mt-3 mb-4 max-w-lg text-xs text-destructive" role="alert">
          {copy.storageLimitWholeGigabytes}
        </p>
      ) : null}
      <SettingsSwitchRow
        label={copy.limitUnlimited}
        checked={draft.unlimited}
        onCheckedChange={(unlimited) => void updateRetentionDraft({ unlimited })}
      />
      {retentionChanged ? (
        <div className="mb-4 rounded-lg border border-amber-500/30 bg-amber-500/5 p-4">
          <h2 className="text-sm font-semibold">{copy.retentionPreviewTitle}</h2>
          {previewLoading ? (
            <p className="mt-2 text-sm text-muted-foreground" aria-live="polite">
              {copy.loading}
            </p>
          ) : previewFailed ? (
            <p className="mt-2 text-sm text-destructive" role="alert">
              {copy.retentionPreviewFailed}
            </p>
          ) : preview ? (
            <dl className="mt-3 grid gap-x-5 gap-y-2 text-sm sm:grid-cols-2">
              <PreviewValue
                label={copy.retentionEntriesToDelete}
                value={formatCount(preview.entriesToDelete, copy.dateLocale)}
              />
              <PreviewValue
                label={copy.retentionFilesToDelete}
                value={formatCount(preview.filesToDelete, copy.dateLocale)}
              />
              <PreviewValue
                label={copy.retentionBytesToFree}
                value={formatBytes(preview.bytesToFree, copy.dateLocale)}
              />
              <PreviewValue
                label={copy.retentionTotalAudio}
                value={formatBytes(preview.totalAudioBytes, copy.dateLocale)}
              />
              <PreviewValue
                label={copy.retentionProtectedEntries}
                value={formatCount(preview.protectedEntries, copy.dateLocale)}
              />
              <PreviewValue
                label={copy.retentionProtectedBytes}
                value={formatBytes(preview.protectedBytes, copy.dateLocale)}
              />
            </dl>
          ) : null}
          {applyFailed ? (
            <p className="mt-3 text-sm text-destructive" role="alert">
              {copy.retentionApplyFailed}
            </p>
          ) : null}
          <AlertDialog
            open={applyDialogOpen}
            onOpenChange={(open) => {
              setApplyDialogOpen(open);
              if (!open && !applyPendingRef.current) discardRetentionDraft();
            }}
          >
            <AlertDialogTrigger asChild>
              <Button
                type="button"
                className="mt-4"
                disabled={
                  !preview || !isValidRetentionPreview(preview) || previewLoading || applying
                }
              >
                {copy.retentionApply}
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>{copy.retentionConfirmTitle}</AlertDialogTitle>
                <AlertDialogDescription>{copy.retentionConfirmBody}</AlertDialogDescription>
              </AlertDialogHeader>
              {preview ? (
                <p className="text-sm text-muted-foreground">
                  {copy.retentionEntriesToDelete}:{" "}
                  {formatCount(preview.entriesToDelete, copy.dateLocale)};{" "}
                  {copy.retentionFilesToDelete.toLowerCase()}:{" "}
                  {formatCount(preview.filesToDelete, copy.dateLocale)};{" "}
                  {copy.retentionBytesToFree.toLowerCase()}:{" "}
                  {formatBytes(preview.bytesToFree, copy.dateLocale)}.
                </p>
              ) : null}
              {applyFailed ? (
                <p className="text-sm text-destructive" role="alert">
                  {copy.retentionApplyFailed}
                </p>
              ) : null}
              <AlertDialogFooter>
                <AlertDialogCancel>{copy.retentionCancel}</AlertDialogCancel>
                <AlertDialogAction
                  disabled={applying}
                  onClick={(event) => {
                    event.preventDefault();
                    void applyRetentionDraft();
                  }}
                >
                  {applying ? copy.loading : copy.retentionApply}
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        </div>
      ) : null}
      <div>
        <SettingsSwitchRow
          label={copy.keepOriginals}
          checked={settings.keepOriginalRecordings}
          onCheckedChange={(keepOriginalRecordings) => onChange({ keepOriginalRecordings })}
        />
        <p className="-mt-2 mb-4 max-w-lg text-xs text-muted-foreground">
          {copy.keepOriginalsHint}
        </p>
      </div>
      <AlertDialog>
        <AlertDialogTrigger asChild>
          <Button variant="destructive">{copy.deleteAllHistory}</Button>
        </AlertDialogTrigger>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{copy.deleteAllConfirm}</AlertDialogTitle>
            <AlertDialogDescription>{copy.deleteAllCannotUndo}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{copy.cancel}</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              disabled={deleting}
              onClick={() => void handleDeleteAll()}
            >
              {deleting ? copy.loading : copy.delete}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

function PreviewValue({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="shrink-0 font-medium tabular-nums">{value}</dd>
    </div>
  );
}

function formatCount(value: number, locale: string) {
  return new Intl.NumberFormat(locale).format(value);
}

function formatBytes(value: number, locale: string) {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let amount = Math.max(0, value);
  let unitIndex = 0;
  while (amount >= 1024 && unitIndex < units.length - 1) {
    amount /= 1024;
    unitIndex += 1;
  }
  const maximumFractionDigits = unitIndex === 0 || amount >= 100 ? 0 : amount >= 10 ? 1 : 2;
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits }).format(amount)} ${units[unitIndex]}`;
}
