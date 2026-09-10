import { useEffect, useMemo, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { Keyboard, Mic, Settings as SettingsIcon } from "lucide-react";
import { api, type AppSettings, type Recording, type SessionState } from "../../lib/api";
import { messagesFor, resolveUiLocale, type Messages } from "../../lib/i18n";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { Select } from "../../components/ui/select";
import { Switch } from "../../components/ui/switch";
import { hotkeyFromKeyboardEvent } from "../../lib/hotkey";
import { applyTheme } from "../../lib/theme";
import { HistoryPane } from "./HistoryList";
import { FilterSettings } from "./FilterSettings";

type Section =
  | "history"
  | "general"
  | "audio"
  | "filters"
  | "transcription"
  | "historySettings"
  | "appearance"
  | "advanced";

export function MainApp() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [history, setHistory] = useState<Recording[]>([]);
  const [section, setSection] = useState<Section>("history");
  const [keyConfigured, setKeyConfigured] = useState(false);
  const [keyDraft, setKeyDraft] = useState("");
  const [status, setStatus] = useState("");
  const [query, setQuery] = useState("");

  async function refreshHistory() {
    const [items, configured] = await Promise.all([api.history(), api.keyConfigured()]);
    setHistory(items);
    setKeyConfigured(configured);
  }

  async function loadSettings() {
    const nextSettings = await api.settings();
    setSettings(nextSettings);
    applyTheme(nextSettings.theme);
  }

  useEffect(() => {
    void loadSettings();
    void refreshHistory();
    const unlisten = listen<SessionState>("session://state", () => {
      void refreshHistory();
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  const filtered = useMemo(
    () =>
      history.filter((item) =>
        `${item.transcript ?? ""} ${item.status} ${item.lastErrorMessage ?? ""}`
          .toLowerCase()
          .includes(query.toLowerCase()),
      ),
    [history, query],
  );

  if (!settings) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        Загрузка…
      </div>
    );
  }

  const copy = messagesFor(resolveUiLocale(settings.uiLanguage ?? "auto", navigator.language));

  async function persist(next: AppSettings) {
    const saved = await api.saveSettings(next);
    setSettings(saved);
    applyTheme(saved.theme);
  }

  return (
    <div className="flex h-full">
      <nav className="flex w-52 flex-col border-r border-border bg-panel p-3">
        <div className="mb-4 px-1 text-sm font-semibold tracking-tight">Voxely</div>
        <NavBtn current={section} id="history" label={copy.navHistory} onClick={setSection} />
        <div className="mt-4 mb-1 px-2 text-[11px] uppercase tracking-wide text-muted-foreground">
          {copy.settings}
        </div>
        <NavBtn current={section} id="general" label={copy.navGeneral} onClick={setSection} />
        <NavBtn current={section} id="audio" label={copy.navAudio} onClick={setSection} />
        <NavBtn current={section} id="filters" label={copy.navFilters} onClick={setSection} />
        <NavBtn
          current={section}
          id="transcription"
          label={copy.navTranscription}
          onClick={setSection}
        />
        <NavBtn
          current={section}
          id="historySettings"
          label={copy.navStorage}
          onClick={setSection}
        />
        <NavBtn current={section} id="appearance" label={copy.navAppearance} onClick={setSection} />
        <NavBtn current={section} id="advanced" label={copy.navAdvanced} onClick={setSection} />
        <div className="mt-auto text-[11px] text-muted-foreground">
          {settings.hotkey} включает диктовку
        </div>
      </nav>
      <main className="flex min-w-0 flex-1 flex-col">
        {section === "history" ? (
          <HistoryPane
            items={filtered}
            query={query}
            keyConfigured={keyConfigured}
            hotkey={settings.hotkey}
            onQuery={setQuery}
            onRefresh={refreshHistory}
            onOpenKey={() => setSection("transcription")}
            onOpenSettings={() => setSection("general")}
            copy={copy}
          />
        ) : (
          <div className="overflow-auto p-6">
            {section === "general" ? (
              <GeneralSettings
                settings={settings}
                copy={copy}
                onChange={(patch) => void persist({ ...settings, ...patch })}
              />
            ) : null}
            {section === "audio" ? (
              <AudioSettings
                settings={settings}
                onChange={(patch) => void persist({ ...settings, ...patch })}
              />
            ) : null}
            {section === "filters" ? (
              <FilterSettings
                settings={settings}
                copy={copy}
                onChange={(patch) => void persist({ ...settings, ...patch })}
              />
            ) : null}
            {section === "transcription" ? (
              <TranscriptionSettings
                settings={settings}
                keyConfigured={keyConfigured}
                keyDraft={keyDraft}
                status={status}
                onKeyDraft={setKeyDraft}
                onStatus={setStatus}
                onConfigured={setKeyConfigured}
                onChange={(patch) =>
                  void persist({ ...settings, ...patch, firstRunComplete: true })
                }
              />
            ) : null}
            {section === "historySettings" ? (
              <HistorySettings
                settings={settings}
                onChange={(patch) => void persist({ ...settings, ...patch })}
                onDeleteAll={async () => {
                  if (window.confirm("Удалить всю историю? Это нельзя отменить.")) {
                    await api.deleteAll();
                    await refreshHistory();
                  }
                }}
              />
            ) : null}
            {section === "appearance" ? (
              <AppearanceSettings
                settings={settings}
                copy={copy}
                onChange={(patch) => void persist({ ...settings, ...patch })}
              />
            ) : null}
            {section === "advanced" ? (
              <AdvancedSettings
                settings={settings}
                onChange={(patch) => void persist({ ...settings, ...patch })}
              />
            ) : null}
          </div>
        )}
      </main>
    </div>
  );
}

function NavBtn({
  current,
  id,
  label,
  onClick,
}: {
  current: Section;
  id: Section;
  label: string;
  onClick: (id: Section) => void;
}) {
  return (
    <button
      type="button"
      onClick={() => onClick(id)}
      className={`rounded-md px-2 py-1.5 text-left text-sm ${current === id ? "bg-muted font-medium" : "text-muted-foreground hover:bg-muted/60"}`}
    >
      {label}
    </button>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="mb-4 block max-w-lg">
      <div className="mb-1 text-sm">{label}</div>
      {children}
    </label>
  );
}

function HotkeyCapture({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  const [listening, setListening] = useState(false);

  useEffect(() => {
    if (!listening) {
      return;
    }
    void api.setHotkeyCapture(true);
    const onKey = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape") {
        setListening(false);
        void api.setHotkeyCapture(false);
        return;
      }
      const next = hotkeyFromKeyboardEvent(event);
      if (!next) {
        return;
      }
      setListening(false);
      void api.setHotkeyCapture(false).then(() => onChange(next));
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      void api.setHotkeyCapture(false);
    };
  }, [listening, onChange]);

  return (
    <Button
      type="button"
      variant={listening ? "default" : "outline"}
      className="w-full justify-start font-normal"
      onClick={() => setListening((current) => !current)}
    >
      {listening ? "Нажмите сочетание… Esc отмена" : value}
    </Button>
  );
}

function GeneralSettings({
  settings,
  copy,
  onChange,
}: {
  settings: AppSettings;
  copy: Messages;
  onChange: (patch: Partial<AppSettings>) => void;
}) {
  return (
    <div>
      <h1 className="mb-4 text-lg font-medium">{copy.navGeneral}</h1>
      <Field label="Глобальный хоткей">
        <HotkeyCapture value={settings.hotkey} onChange={(hotkey) => onChange({ hotkey })} />
      </Field>
      <div className="mb-3 flex items-center justify-between max-w-lg">
        <span className="text-sm">{copy.startWithWindows}</span>
        <Switch
          checked={settings.startWithWindows}
          onCheckedChange={(v) => onChange({ startWithWindows: v })}
        />
      </div>
      <div className="mb-3 flex items-center justify-between max-w-lg">
        <span className="text-sm">{copy.closeToTray}</span>
        <Switch
          checked={settings.closeToTray}
          onCheckedChange={(v) => onChange({ closeToTray: v })}
        />
      </div>
      <div className="flex items-center justify-between max-w-lg">
        <span className="text-sm">{copy.notifications}</span>
        <Switch
          checked={settings.notifications}
          onCheckedChange={(v) => onChange({ notifications: v })}
        />
      </div>
    </div>
  );
}

function AudioSettings({
  settings,
  onChange,
}: {
  settings: AppSettings;
  onChange: (patch: Partial<AppSettings>) => void;
}) {
  const [devices, setDevices] = useState<Array<{ id: string; name: string }>>([]);
  useEffect(() => {
    void api.mics().then(setDevices);
  }, []);
  return (
    <div>
      <h1 className="mb-4 flex items-center gap-2 text-lg font-medium">
        <Mic className="h-4 w-4" /> Микрофон
      </h1>
      <Field label="Устройство ввода">
        <Select
          value={settings.inputDevice}
          onChange={(e) => onChange({ inputDevice: e.target.value })}
        >
          <option value="default">Системное по умолчанию</option>
          {devices.map((device) => (
            <option key={device.id} value={device.id}>
              {device.name}
            </option>
          ))}
        </Select>
      </Field>
    </div>
  );
}

function TranscriptionSettings({
  settings,
  keyConfigured,
  keyDraft,
  status,
  onKeyDraft,
  onStatus,
  onConfigured,
  onChange,
}: {
  settings: AppSettings;
  keyConfigured: boolean;
  keyDraft: string;
  status: string;
  onKeyDraft: (value: string) => void;
  onStatus: (value: string) => void;
  onConfigured: (value: boolean) => void;
  onChange: (patch: Partial<AppSettings>) => void;
}) {
  return (
    <div>
      <h1 className="mb-4 text-lg font-medium">Расшифровка</h1>
      <p className="mb-3 text-sm text-muted-foreground">
        Ключ хранится в Windows Credential Manager. В интерфейсе видно только:{" "}
        {keyConfigured ? "ключ сохранён" : "ключа нет"}.
      </p>
      <Field label="API-ключ">
        <Input
          type="password"
          value={keyDraft}
          autoComplete="off"
          placeholder={keyConfigured ? "Заменить ключ" : "sk-or-…"}
          onChange={(e) => onKeyDraft(e.target.value)}
        />
      </Field>
      <div className="mb-4 flex gap-2">
        <Button
          onClick={async () => {
            await api.storeKey(keyDraft);
            onConfigured(true);
            onKeyDraft("");
            onStatus("Ключ сохранён");
          }}
        >
          {keyConfigured ? "Заменить ключ" : "Сохранить ключ"}
        </Button>
        <Button
          variant="outline"
          onClick={async () => {
            try {
              onStatus(await api.testConnection());
            } catch (error) {
              onStatus(error instanceof Error ? error.message : "Нет соединения");
            }
          }}
        >
          Проверить соединение
        </Button>
      </div>
      {status ? <p className="mb-4 text-sm">{status}</p> : null}
      <p className="mb-3 max-w-lg text-sm text-muted-foreground">
        Таймаут одного запроса растёт вместе с длительностью записи, до 15 минут. Общий лимит
        покрывает длинные диктовки и повторы.
      </p>
      <Field label="Модель">
        <Input value={settings.model} onChange={(e) => onChange({ model: e.target.value })} />
      </Field>
      <Field label="Язык">
        <Select value={settings.language} onChange={(e) => onChange({ language: e.target.value })}>
          <option value="auto">Авто</option>
          <option value="ru">Русский</option>
          <option value="en">English</option>
        </Select>
      </Field>
      <div className="mb-3 flex items-center justify-between max-w-lg">
        <span className="text-sm">Автоматические повторы</span>
        <Switch
          checked={settings.retry.automaticRetries}
          onCheckedChange={(v) => onChange({ retry: { ...settings.retry, automaticRetries: v } })}
        />
      </div>
      <Field label="Дополнительные попытки (1 запрос + столько повторов)">
        <Input
          type="number"
          min={0}
          max={5}
          value={settings.retry.additionalRetries}
          onChange={(e) =>
            onChange({ retry: { ...settings.retry, additionalRetries: Number(e.target.value) } })
          }
        />
      </Field>
      <Field label="Таймаут соединения (мс)">
        <Input
          type="number"
          value={settings.retry.connectTimeoutMs}
          onChange={(e) =>
            onChange({ retry: { ...settings.retry, connectTimeoutMs: Number(e.target.value) } })
          }
        />
      </Field>
      <Field label="Таймаут запроса (мс)">
        <Input
          type="number"
          value={settings.retry.requestTimeoutMs}
          onChange={(e) =>
            onChange({ retry: { ...settings.retry, requestTimeoutMs: Number(e.target.value) } })
          }
        />
      </Field>
      <Field label="Начальная пауза (мс)">
        <Input
          type="number"
          value={settings.retry.initialRetryDelayMs}
          onChange={(e) =>
            onChange({ retry: { ...settings.retry, initialRetryDelayMs: Number(e.target.value) } })
          }
        />
      </Field>
      <Field label="Максимальная пауза (мс)">
        <Input
          type="number"
          value={settings.retry.maxRetryDelayMs}
          onChange={(e) =>
            onChange({ retry: { ...settings.retry, maxRetryDelayMs: Number(e.target.value) } })
          }
        />
      </Field>
      <Field label="Общий лимит операции (мс)">
        <Input
          type="number"
          value={settings.retry.totalOperationTimeoutMs}
          onChange={(e) =>
            onChange({
              retry: { ...settings.retry, totalOperationTimeoutMs: Number(e.target.value) },
            })
          }
        />
      </Field>
    </div>
  );
}

function HistorySettings({
  settings,
  onChange,
  onDeleteAll,
}: {
  settings: AppSettings;
  onChange: (patch: Partial<AppSettings>) => void;
  onDeleteAll: () => void;
}) {
  return (
    <div>
      <h1 className="mb-4 text-lg font-medium">Хранение</h1>
      <Field label="Хранить записи">
        <Select
          value={settings.retention}
          onChange={(e) => onChange({ retention: e.target.value })}
        >
          <option value="1d">1 день</option>
          <option value="3d">3 дня</option>
          <option value="7d">7 дней</option>
          <option value="30d">30 дней</option>
          <option value="90d">90 дней</option>
          <option value="forever">Всегда</option>
        </Select>
      </Field>
      <Field label="Лимит места">
        <Select
          value={settings.storageLimit}
          onChange={(e) => onChange({ storageLimit: e.target.value })}
        >
          <option value="500mb">500 МБ</option>
          <option value="1gb">1 ГБ</option>
          <option value="5gb">5 ГБ</option>
          <option value="unlimited">Без лимита</option>
        </Select>
      </Field>
      <div className="mb-4 flex items-center justify-between max-w-lg">
        <span className="text-sm">Хранить исходные записи (лучше для прослушивания)</span>
        <Switch
          checked={settings.keepOriginalRecordings}
          onCheckedChange={(v) => onChange({ keepOriginalRecordings: v })}
        />
      </div>
      <Button variant="destructive" onClick={onDeleteAll}>
        Удалить всю историю
      </Button>
    </div>
  );
}

function AppearanceSettings({
  settings,
  copy,
  onChange,
}: {
  settings: AppSettings;
  copy: Messages;
  onChange: (patch: Partial<AppSettings>) => void;
}) {
  const [updateStatus, setUpdateStatus] = useState("");
  const [checking, setChecking] = useState(false);
  return (
    <div>
      <h1 className="mb-4 text-lg font-medium">{copy.navAppearance}</h1>
      <Field label="Тема">
        <Select value={settings.theme} onChange={(e) => onChange({ theme: e.target.value })}>
          <option value="system">Системная</option>
          <option value="light">Светлая</option>
          <option value="dark">Тёмная</option>
        </Select>
      </Field>
      <Field label={copy.uiLanguage}>
        <Select
          value={settings.uiLanguage ?? "auto"}
          onChange={(e) => onChange({ uiLanguage: e.target.value })}
        >
          <option value="auto">{copy.uiAuto}</option>
          <option value="ru">{copy.uiRu}</option>
          <option value="en">{copy.uiEn}</option>
        </Select>
      </Field>
      <div className="mb-3 flex items-center justify-between max-w-lg">
        <span className="text-sm">{copy.autoUpdate}</span>
        <Switch
          checked={settings.autoUpdateEnabled ?? true}
          onCheckedChange={(v) => onChange({ autoUpdateEnabled: v })}
        />
      </div>
      <Button
        variant="outline"
        disabled={checking}
        onClick={async () => {
          setChecking(true);
          try {
            setUpdateStatus(await api.checkForUpdates());
          } catch (error) {
            setUpdateStatus(error instanceof Error ? error.message : copy.checkUpdates);
          } finally {
            setChecking(false);
          }
        }}
      >
        {checking ? copy.checking : copy.checkUpdates}
      </Button>
      {updateStatus ? <p className="mt-3 text-sm text-muted-foreground">{updateStatus}</p> : null}
    </div>
  );
}

function AdvancedSettings({
  settings,
  onChange,
}: {
  settings: AppSettings;
  onChange: (patch: Partial<AppSettings>) => void;
}) {
  return (
    <div>
      <h1 className="mb-4 flex items-center gap-2 text-lg font-medium">
        <SettingsIcon className="h-4 w-4" /> Дополнительно
      </h1>
      <Field label="Вставка текста">
        <Select
          value={settings.insertionMode}
          onChange={(e) => onChange({ insertionMode: e.target.value })}
        >
          <option value="auto">Авто</option>
          <option value="sendinput">SendInput</option>
          <option value="clipboard">Буфер обмена</option>
        </Select>
      </Field>
      <p className="mb-4 max-w-lg text-xs text-muted-foreground">
        Авто — рекомендуемый режим: вставка в то окно, где вы говорили. Буфер обмена надёжнее в
        части приложений, но затирает то, что уже скопировано.
      </p>
      <div className="mb-4 flex items-center justify-between max-w-lg">
        <span className="text-sm">Отладочные логи</span>
        <Switch
          checked={settings.debugLogging}
          onCheckedChange={(v) => onChange({ debugLogging: v })}
        />
      </div>
      <Button variant="outline" onClick={() => void api.openLogs()}>
        Открыть логи
      </Button>
      <p className="mt-4 flex items-center gap-2 text-sm text-muted-foreground">
        <Keyboard className="h-4 w-4" /> Хоткей по умолчанию: Ctrl+Shift+Space.
      </p>
    </div>
  );
}
