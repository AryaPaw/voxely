import { useState } from "react";
import { toast } from "sonner";
import { api, type AppSettings } from "../../../lib/api";
import type { Messages } from "../../../lib/i18n";
import { PageHeader } from "../../../components/settings/PageHeader";
import { SettingsField } from "../../../components/settings/SettingsField";
import { SettingsSwitchRow } from "../../../components/settings/SettingsSwitchRow";
import { Button } from "../../../components/ui/button";
import { Input } from "../../../components/ui/input";
import { SimpleSelect } from "../../../components/ui/simple-select";
import { SECTION_ICONS } from "../sectionNav";

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
              toast.error(error instanceof Error ? error.message : copy.keyNotSaved);
            }
          }}
        >
          {keyConfigured ? copy.replaceKey : copy.saveKey}
        </Button>
        <Button
          variant="outline"
          onClick={async () => {
            try {
              toast.success(await api.testConnection());
            } catch (error) {
              toast.error(error instanceof Error ? error.message : copy.noConnection);
            }
          }}
        >
          {copy.testConnection}
        </Button>
      </div>
      <p className="mb-3 max-w-lg text-sm text-muted-foreground">{copy.timeoutHint}</p>
      <SettingsField label={copy.modelLabel}>
        <Input
          value={settings.model}
          onChange={(event) => onChange({ model: event.target.value })}
        />
      </SettingsField>
      <SettingsField label={copy.language}>
        <SimpleSelect
          aria-label={copy.language}
          value={settings.language}
          onValueChange={(language) => onChange({ language })}
          options={[
            { value: "auto", label: copy.insertAuto },
            { value: "ru", label: copy.uiRu },
            { value: "en", label: copy.uiEn },
          ]}
        />
      </SettingsField>
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
          min={0}
          max={5}
          value={settings.retry.additionalRetries}
          onChange={(event) =>
            onChange({
              retry: { ...settings.retry, additionalRetries: Number(event.target.value) },
            })
          }
        />
      </SettingsField>
      <SettingsField label={copy.connectTimeout}>
        <Input
          type="number"
          value={settings.retry.connectTimeoutMs}
          onChange={(event) =>
            onChange({
              retry: { ...settings.retry, connectTimeoutMs: Number(event.target.value) },
            })
          }
        />
      </SettingsField>
      <SettingsField label={copy.requestTimeout}>
        <Input
          type="number"
          value={settings.retry.requestTimeoutMs}
          onChange={(event) =>
            onChange({
              retry: { ...settings.retry, requestTimeoutMs: Number(event.target.value) },
            })
          }
        />
      </SettingsField>
      <SettingsField label={copy.initialDelay}>
        <Input
          type="number"
          value={settings.retry.initialRetryDelayMs}
          onChange={(event) =>
            onChange({
              retry: { ...settings.retry, initialRetryDelayMs: Number(event.target.value) },
            })
          }
        />
      </SettingsField>
      <SettingsField label={copy.maxDelay}>
        <Input
          type="number"
          value={settings.retry.maxRetryDelayMs}
          onChange={(event) =>
            onChange({
              retry: { ...settings.retry, maxRetryDelayMs: Number(event.target.value) },
            })
          }
        />
      </SettingsField>
      <SettingsField label={copy.totalLimit}>
        <Input
          type="number"
          value={settings.retry.totalOperationTimeoutMs}
          onChange={(event) =>
            onChange({
              retry: { ...settings.retry, totalOperationTimeoutMs: Number(event.target.value) },
            })
          }
        />
      </SettingsField>
    </div>
  );
}
