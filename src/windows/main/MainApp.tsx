import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { api, type AppSettings, type Recording, type SessionState } from "../../lib/api";
import { applyTheme, resolvedTheme, watchSystemTheme } from "../../lib/theme";
import {
  applyUiLocale,
  formatInvokeError,
  localizedError,
  messagesFor,
  resolveUiLocale,
} from "../../lib/i18n";
import { sectionFromSearch } from "../../lib/window-section";
import { Toaster } from "../../components/ui/sonner";
import { Button } from "../../components/ui/button";
import { HistoryPane } from "./history/HistoryPane";
import { SectionNav, type Section } from "./sectionNav";
import { AboutSettings } from "./sections/AboutSettings";
import { AdvancedSettings } from "./sections/AdvancedSettings";
import { AppearanceSettings } from "./sections/AppearanceSettings";
import { AudioSettings } from "./sections/AudioSettings";
import { FilterSettings } from "./sections/FilterSettings";
import { GeneralSettings } from "./sections/GeneralSettings";
import { HistorySettings } from "./sections/HistorySettings";
import { TranscriptionSettings } from "./sections/TranscriptionSettings";
import { ComparePane } from "./sections/ComparePane";
import { DebugSettings } from "./sections/DebugSettings";

export function MainApp() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [loadError, setLoadError] = useState("");
  const [history, setHistory] = useState<Recording[]>([]);
  const [section, setSection] = useState<Section>(() => sectionFromSearch(window.location.search));
  const [keyConfigured, setKeyConfigured] = useState(false);
  const [query, setQuery] = useState("");
  const [localBuild, setLocalBuild] = useState(false);

  async function refreshHistory() {
    const [items, configured] = await Promise.all([api.history(), api.keyConfigured()]);
    setHistory(items);
    setKeyConfigured(configured);
  }

  async function loadSettings() {
    try {
      const [nextSettings, runtime] = await Promise.all([api.settings(), api.runtimeInfo()]);
      setSettings(nextSettings);
      setLocalBuild(runtime.localBuild);
      if (runtime.localBuild) {
        const fromUrl = sectionFromSearch(window.location.search, true);
        if (fromUrl === "debug") {
          setSection("debug");
        }
      } else {
        setSection((current) => (current === "debug" ? "history" : current));
      }
      applyTheme(nextSettings.theme);
      applyUiLocale(resolveUiLocale(nextSettings.uiLanguage ?? "auto", navigator.language));
      setLoadError("");
    } catch (error) {
      setLoadError(
        formatInvokeError(error, messagesFor(resolveUiLocale("auto", navigator.language))),
      );
    }
  }

  useEffect(() => {
    void loadSettings();
    void refreshHistory().catch((error: unknown) => {
      toast.error(
        formatInvokeError(error, messagesFor(resolveUiLocale("auto", navigator.language))),
      );
    });
    const unlisten = listen<SessionState>("session://state", () => {
      void refreshHistory();
    });
    const unlistenInsert = listen<string>("session://insert", (event) => {
      const locale = resolveUiLocale("auto", navigator.language);
      const nextCopy = messagesFor(locale);
      if (event.payload === "copied") {
        toast.success(nextCopy.copiedInsert);
      } else {
        toast.error(localizedError(event.payload, nextCopy));
      }
    });
    return () => {
      void unlisten.then((fn) => fn());
      void unlistenInsert.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (!settings) {
      return;
    }
    applyTheme(settings.theme);
    applyUiLocale(resolveUiLocale(settings.uiLanguage ?? "auto", navigator.language));
    return watchSystemTheme(settings.theme);
  }, [settings]);

  const filtered = useMemo(
    () =>
      history.filter((item) =>
        `${item.transcript ?? ""} ${item.status} ${item.lastErrorMessage ?? ""}`
          .toLowerCase()
          .includes(query.toLowerCase()),
      ),
    [history, query],
  );

  if (loadError && !settings) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 text-sm">
        <p>{loadError}</p>
        <Button onClick={() => void loadSettings()}>
          {messagesFor(resolveUiLocale("auto", navigator.language)).retryAction}
        </Button>
      </div>
    );
  }

  if (!settings) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        {messagesFor(resolveUiLocale("auto", navigator.language)).loading}
      </div>
    );
  }

  const copy = messagesFor(resolveUiLocale(settings.uiLanguage ?? "auto", navigator.language));
  const currentSettings = settings;

  async function persist(next: AppSettings) {
    setSettings(next);
    applyTheme(next.theme);
    try {
      const saved = await api.saveSettings(next);
      setSettings(saved);
      applyTheme(saved.theme);
    } catch (error) {
      setSettings(currentSettings);
      applyTheme(currentSettings.theme);
      toast.error(formatInvokeError(error, copy));
    }
  }

  return (
    <>
      <div className="flex h-full">
        <SectionNav current={section} copy={copy} localBuild={localBuild} onSelect={setSection} />
        <main className="flex min-h-0 min-w-0 flex-1 flex-col">
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
          ) : section === "compare" && settings ? (
            <div className="min-h-0 flex-1 overflow-auto p-6">
              <ComparePane
                settings={settings}
                copy={copy}
                keyConfigured={keyConfigured}
                onChange={(patch) => void persist({ ...settings, ...patch })}
                onOpenKey={() => setSection("transcription")}
              />
            </div>
          ) : (
            <div className="min-h-0 flex-1 overflow-auto p-6">
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
                  copy={copy}
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
                  copy={copy}
                  keyConfigured={keyConfigured}
                  onConfigured={setKeyConfigured}
                  onChange={(patch) =>
                    void persist({ ...settings, ...patch, firstRunComplete: true })
                  }
                />
              ) : null}
              {section === "historySettings" ? (
                <HistorySettings
                  settings={settings}
                  copy={copy}
                  onChange={(patch) => void persist({ ...settings, ...patch })}
                  onDeleteAll={async () => {
                    await api.deleteAll();
                    await refreshHistory();
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
                  copy={copy}
                  onChange={(patch) => void persist({ ...settings, ...patch })}
                  onSettingsReplaced={(next, wipeApiKey) => {
                    setSettings(next);
                    applyTheme(next.theme);
                    if (wipeApiKey) {
                      setKeyConfigured(false);
                    }
                  }}
                />
              ) : null}
              {section === "debug" && localBuild ? <DebugSettings copy={copy} /> : null}
              {section === "about" ? <AboutSettings copy={copy} localBuild={localBuild} /> : null}
            </div>
          )}
        </main>
      </div>
      <Toaster theme={resolvedTheme(settings.theme)} />
    </>
  );
}
