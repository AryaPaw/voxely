import { useEffect, useState } from "react";
import { toast } from "sonner";
import { api, type AppSettings } from "../../../lib/api";
import { formatInvokeError, type Messages } from "../../../lib/i18n";
import { reportError } from "../../../lib/system-notify";
import { speechLanguageOptions } from "../../../lib/speech-languages";
import { PageHeader } from "../../../components/settings/PageHeader";
import { SettingsField } from "../../../components/settings/SettingsField";
import { SettingsSwitchRow } from "../../../components/settings/SettingsSwitchRow";
import { Button } from "../../../components/ui/button";
import { Input } from "../../../components/ui/input";
import { SimpleSelect } from "../../../components/ui/simple-select";
import { SECTION_ICONS } from "../sectionNav";

function draftNumber(value: number): string {
  return String(value);
}

function parseDraft(raw: string, fallback: number): number | null {
  const trimmed = raw.trim();
  if (trimmed === "") {
    return null;
  }
  const next = Number(trimmed);
  return Number.isFinite(next) ? next : fallback;
}

type RetryNumberField =
  | "additionalRetries"
  | "connectTimeoutMs"
  | "requestTimeoutMs"
  | "initialRetryDelayMs"
  | "maxRetryDelayMs"
  | "totalOperationTimeoutMs";

function commitRetry(
  field: RetryNumberField,
  raw: string,
  settings: AppSettings,
  onChange: (patch: Partial<AppSettings>) => void,
) {
  const parsed = parseDraft(raw, settings.retry[field]);
  if (parsed == null) {
    return;
  }
  onChange({ retry: { ...settings.retry, [field]: parsed } });
}

export function TranscriptionSettings({
  settings,
  copy,
  keyConfigured,
  onConfigured,
  onChange,
}: {
  settings: AppSettings;
  copy: Messages;
  keyConfigured: boolean;
  onConfigured: (value: boolean) => void;
  onChange: (patch: Partial<AppSettings>) => void;
}) {
  const [keyDraft, setKeyDraft] = useState("");
  const [catalog, setCatalog] = useState<Array<{ id: string; name: string }>>([]);
  const [catalogError, setCatalogError] = useState(false);
  const [modelDraft, setModelDraft] = useState(settings.model);
  const [extraDraft, setExtraDraft] = useState(draftNumber(settings.retry.additionalRetries));
  const [connectDraft, setConnectDraft] = useState(draftNumber(settings.retry.connectTimeoutMs));
  const [requestDraft, setRequestDraft] = useState(draftNumber(settings.retry.requestTimeoutMs));
  const [initialDraft, setInitialDraft] = useState(draftNumber(settings.retry.initialRetryDelayMs));
  const [maxDraft, setMaxDraft] = useState(draftNumber(settings.retry.maxRetryDelayMs));
  const [totalDraft, setTotalDraft] = useState(draftNumber(settings.retry.totalOperationTimeoutMs));
  const catalogIds = new Set(catalog.map((item) => item.id));
  const catalogValue = catalogIds.has(settings.model) ? settings.model : "";

  useEffect(() => {
    setModelDraft(settings.model);
  }, [settings.model]);

  useEffect(() => {
    let cancelled = false;
    void api
      .models()
      .then((items) => {
        if (!cancelled) {
          setCatalog(items);
          setCatalogError(false);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setCatalogError(true);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [keyConfigured]);

  return (
    <div>
      <PageHeader icon={SECTION_ICONS.transcription} title={copy.navTranscription} />
      <p className="mb-3 text-sm text-muted-foreground">
        {copy.apiKeyHint} {keyConfigured ? copy.keySaved : copy.keyMissing}.
      </p>
      <SettingsField label={copy.apiKey}>
        <Input
          type="password"
          value={keyDraft}
          autoComplete="off"
          placeholder={keyConfigured ? copy.replaceKey : "sk-or-…"}
          onChange={(event) => setKeyDraft(event.target.value)}
        />
      </SettingsField>
      <div className="mb-4 flex gap-2">
        <Button
          onClick={async () => {
            try {
              await api.storeKey(keyDraft);
              onConfigured(true);
              setKeyDraft("");
              toast.success(copy.keySavedToast);
            } catch (error) {
              reportError(formatInvokeError(error, copy));
            }
          }}
        >
          {keyConfigured ? copy.replaceKey : copy.saveKey}
        </Button>
        <Button
          variant="outline"
          onClick={async () => {
            try {
              const count = await api.testConnection();
              toast.success(copy.modelsFound.replace("{n}", String(count)));
            } catch (error) {
              reportError(formatInvokeError(error, copy));
            }
          }}
        >
          {copy.testConnection}
        </Button>
      </div>
      <p className="mb-3 max-w-lg text-sm text-muted-foreground">{copy.timeoutHint}</p>
      <div className="mb-4">
        <Button type="button" variant="outline" onClick={() => void api.openOpenrouterModels()}>
          {copy.openRouterCatalog}
        </Button>
      </div>
      <SettingsField label={copy.modelLabel}>
        <Input
          aria-label={copy.modelLabel}
          value={modelDraft}
          spellCheck={false}
          autoComplete="off"
          placeholder="openai/gpt-transcribe"
          aria-invalid={!modelDraft.trim()}
          onChange={(event) => setModelDraft(event.target.value)}
          onBlur={() => {
            const next = modelDraft.trim();
            if (!next) {
              setModelDraft(settings.model);
              reportError(copy.modelRequired);
              return;
            }
            onChange({ model: next, customModel: next });
          }}
        />
      </SettingsField>
      <p className="-mt-3 mb-4 max-w-lg text-xs text-muted-foreground">{copy.modelIdHint}</p>
      {catalog.length > 0 ? (
        <SettingsField label={copy.catalogHint}>
          <SimpleSelect
            aria-label={copy.catalogHint}
            value={catalogValue}
            onValueChange={(value) => {
              setModelDraft(value);
              onChange({ model: value, customModel: value });
            }}
            options={catalog.map((item) => ({ value: item.id, label: item.name || item.id }))}
          />
        </SettingsField>
      ) : null}
      {catalogError ? (
        <p className="mb-4 text-sm text-muted-foreground">{copy.catalogUnavailable}</p>
      ) : null}
      <SettingsField label={copy.language}>
        <SimpleSelect
          aria-label={copy.language}
          value={settings.language}
          onValueChange={(language) => onChange({ language })}
          options={speechLanguageOptions(
            copy.speechLanguageAuto,
            copy.dateLocale,
            settings.language,
          )}
        />
      </SettingsField>
      <p className="-mt-3 mb-4 max-w-lg text-xs text-muted-foreground">{copy.speechLanguageHint}</p>
      <SettingsSwitchRow
        label={copy.autoRetries}
        checked={settings.retry.automaticRetries}
        onCheckedChange={(automaticRetries) =>
          onChange({ retry: { ...settings.retry, automaticRetries } })
        }
      />
      <SettingsField label={copy.extraAttempts}>
        <Input
          inputMode="numeric"
          value={extraDraft}
          onChange={(event) => setExtraDraft(event.target.value)}
          onBlur={() => commitRetry("additionalRetries", extraDraft, settings, onChange)}
        />
      </SettingsField>
      <SettingsField label={copy.connectTimeout}>
        <Input
          inputMode="numeric"
          value={connectDraft}
          onChange={(event) => setConnectDraft(event.target.value)}
          onBlur={() => {
            const parsed = parseDraft(connectDraft, settings.retry.connectTimeoutMs);
            if (parsed == null) {
              setConnectDraft(draftNumber(settings.retry.connectTimeoutMs));
              return;
            }
            commitRetry("connectTimeoutMs", connectDraft, settings, onChange);
          }}
        />
      </SettingsField>
      <SettingsField label={copy.requestTimeout}>
        <Input
          inputMode="numeric"
          value={requestDraft}
          onChange={(event) => setRequestDraft(event.target.value)}
          onBlur={() => {
            const parsed = parseDraft(requestDraft, settings.retry.requestTimeoutMs);
            if (parsed == null) {
              setRequestDraft(draftNumber(settings.retry.requestTimeoutMs));
              return;
            }
            commitRetry("requestTimeoutMs", requestDraft, settings, onChange);
          }}
        />
      </SettingsField>
      <SettingsField label={copy.initialDelay}>
        <Input
          inputMode="numeric"
          value={initialDraft}
          onChange={(event) => setInitialDraft(event.target.value)}
          onBlur={() => commitRetry("initialRetryDelayMs", initialDraft, settings, onChange)}
        />
      </SettingsField>
      <SettingsField label={copy.maxDelay}>
        <Input
          inputMode="numeric"
          value={maxDraft}
          onChange={(event) => setMaxDraft(event.target.value)}
          onBlur={() => commitRetry("maxRetryDelayMs", maxDraft, settings, onChange)}
        />
      </SettingsField>
      <SettingsField label={copy.totalLimit}>
        <Input
          inputMode="numeric"
          value={totalDraft}
          onChange={(event) => setTotalDraft(event.target.value)}
          onBlur={() => {
            const parsed = parseDraft(totalDraft, settings.retry.totalOperationTimeoutMs);
            if (parsed == null) {
              setTotalDraft(draftNumber(settings.retry.totalOperationTimeoutMs));
              return;
            }
            commitRetry("totalOperationTimeoutMs", totalDraft, settings, onChange);
          }}
        />
      </SettingsField>
    </div>
  );
}
