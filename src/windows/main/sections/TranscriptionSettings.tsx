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

function parseDraft(raw: string): number | null {
  const trimmed = raw.trim();
  if (trimmed === "") {
    return null;
  }
  const next = Number(trimmed);
  return Number.isSafeInteger(next) ? next : null;
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
): boolean {
  const parsed = parseDraft(raw);
  if (parsed == null || !isValidRetryValue(field, parsed, settings)) {
    return false;
  }
  if (parsed !== settings.retry[field]) {
    onChange({ retry: { ...settings.retry, [field]: parsed } });
  }
  return true;
}

function isValidRetryValue(field: RetryNumberField, value: number, settings: AppSettings): boolean {
  switch (field) {
    case "additionalRetries":
      return value >= 0 && value <= 5;
    case "connectTimeoutMs":
      return value >= 8_000;
    case "requestTimeoutMs":
      return value >= 5_000 && value <= settings.retry.totalOperationTimeoutMs;
    case "totalOperationTimeoutMs":
      return value >= 60_000 && value >= settings.retry.requestTimeoutMs;
    default:
      return value >= 0;
  }
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
  const [catalogRevision, setCatalogRevision] = useState(0);
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
    setExtraDraft(draftNumber(settings.retry.additionalRetries));
  }, [settings.retry.additionalRetries]);

  useEffect(() => {
    setConnectDraft(draftNumber(settings.retry.connectTimeoutMs));
  }, [settings.retry.connectTimeoutMs]);

  useEffect(() => {
    setRequestDraft(draftNumber(settings.retry.requestTimeoutMs));
  }, [settings.retry.requestTimeoutMs]);

  useEffect(() => {
    setInitialDraft(draftNumber(settings.retry.initialRetryDelayMs));
  }, [settings.retry.initialRetryDelayMs]);

  useEffect(() => {
    setMaxDraft(draftNumber(settings.retry.maxRetryDelayMs));
  }, [settings.retry.maxRetryDelayMs]);

  useEffect(() => {
    setTotalDraft(draftNumber(settings.retry.totalOperationTimeoutMs));
  }, [settings.retry.totalOperationTimeoutMs]);

  useEffect(() => {
    let cancelled = false;
    setCatalog([]);
    setCatalogError(false);
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
  }, [keyConfigured, catalogRevision]);

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
              setCatalogRevision((revision) => revision + 1);
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
              const model = await api.testConnection();
              toast.success(copy.modelAvailable.replace("{model}", model));
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
          type="number"
          inputMode="numeric"
          min={0}
          max={5}
          step={1}
          value={extraDraft}
          onChange={(event) => setExtraDraft(event.target.value)}
          onBlur={() => {
            if (!commitRetry("additionalRetries", extraDraft, settings, onChange)) {
              setExtraDraft(draftNumber(settings.retry.additionalRetries));
            }
          }}
        />
      </SettingsField>
      <SettingsField label={copy.connectTimeout}>
        <Input
          type="number"
          inputMode="numeric"
          min={8_000}
          step={1}
          value={connectDraft}
          onChange={(event) => setConnectDraft(event.target.value)}
          onBlur={() => {
            if (!commitRetry("connectTimeoutMs", connectDraft, settings, onChange)) {
              setConnectDraft(draftNumber(settings.retry.connectTimeoutMs));
            }
          }}
        />
      </SettingsField>
      <SettingsField label={copy.requestTimeout}>
        <Input
          type="number"
          inputMode="numeric"
          min={5_000}
          max={settings.retry.totalOperationTimeoutMs}
          step={1}
          value={requestDraft}
          onChange={(event) => setRequestDraft(event.target.value)}
          onBlur={() => {
            if (!commitRetry("requestTimeoutMs", requestDraft, settings, onChange)) {
              setRequestDraft(draftNumber(settings.retry.requestTimeoutMs));
            }
          }}
        />
      </SettingsField>
      <SettingsField label={copy.initialDelay}>
        <Input
          type="number"
          inputMode="numeric"
          min={0}
          step={1}
          value={initialDraft}
          onChange={(event) => setInitialDraft(event.target.value)}
          onBlur={() => {
            if (!commitRetry("initialRetryDelayMs", initialDraft, settings, onChange)) {
              setInitialDraft(draftNumber(settings.retry.initialRetryDelayMs));
            }
          }}
        />
      </SettingsField>
      <SettingsField label={copy.maxDelay}>
        <Input
          type="number"
          inputMode="numeric"
          min={0}
          step={1}
          value={maxDraft}
          onChange={(event) => setMaxDraft(event.target.value)}
          onBlur={() => {
            if (!commitRetry("maxRetryDelayMs", maxDraft, settings, onChange)) {
              setMaxDraft(draftNumber(settings.retry.maxRetryDelayMs));
            }
          }}
        />
      </SettingsField>
      <SettingsField label={copy.totalLimit}>
        <Input
          type="number"
          inputMode="numeric"
          min={Math.max(60_000, settings.retry.requestTimeoutMs)}
          step={1}
          value={totalDraft}
          onChange={(event) => setTotalDraft(event.target.value)}
          onBlur={() => {
            if (!commitRetry("totalOperationTimeoutMs", totalDraft, settings, onChange)) {
              setTotalDraft(draftNumber(settings.retry.totalOperationTimeoutMs));
            }
          }}
        />
      </SettingsField>
    </div>
  );
}
