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
        Ключ хранится в Windows Credential Manager. В интерфейсе видно только:{" "}
        {keyConfigured ? "ключ сохранён" : "ключа нет"}.
      </p>
      <SettingsField label="API-ключ">
        <Input
          type="password"
          value={keyDraft}
          autoComplete="off"
          placeholder={keyConfigured ? "Заменить ключ" : "sk-or-…"}
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
              toast.success("Ключ сохранён");
            } catch (error) {
              toast.error(error instanceof Error ? error.message : "Ключ не сохранён");
            }
          }}
        >
          {keyConfigured ? "Заменить ключ" : "Сохранить ключ"}
        </Button>
        <Button
          variant="outline"
          onClick={async () => {
            try {
              toast.success(await api.testConnection());
            } catch (error) {
              toast.error(error instanceof Error ? error.message : "Нет соединения");
            }
          }}
        >
          Проверить соединение
        </Button>
      </div>
      <p className="mb-3 max-w-lg text-sm text-muted-foreground">
        Таймаут одного запроса растёт вместе с длительностью записи, до 15 минут. Общий лимит
        покрывает длинные диктовки и повторы.
      </p>
      <SettingsField label="Модель">
        <Input value={settings.model} onChange={(event) => onChange({ model: event.target.value })} />
      </SettingsField>
      <SettingsField label="Язык">
        <SimpleSelect
          aria-label="Язык"
          value={settings.language}
          onValueChange={(language) => onChange({ language })}
          options={[
            { value: "auto", label: "Авто" },
            { value: "ru", label: "Русский" },
            { value: "en", label: "English" },
          ]}
        />
      </SettingsField>
      <SettingsSwitchRow
        label="Автоматические повторы"
        checked={settings.retry.automaticRetries}
        onCheckedChange={(automaticRetries) =>
          onChange({ retry: { ...settings.retry, automaticRetries } })
        }
      />
      <SettingsField label="Дополнительные попытки (1 запрос + столько повторов)">
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
      <SettingsField label="Таймаут соединения (мс)">
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
      <SettingsField label="Таймаут запроса (мс)">
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
      <SettingsField label="Начальная пауза (мс)">
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
      <SettingsField label="Максимальная пауза (мс)">
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
      <SettingsField label="Общий лимит операции (мс)">
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
