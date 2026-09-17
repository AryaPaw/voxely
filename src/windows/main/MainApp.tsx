import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { reportError } from "../../lib/system-notify";
import { api, type AppSettings, type Recording } from "../../lib/api";
import {
  acceptSavedSettings,
  nextWriteSeq,
  shouldKeepOptimistic,
} from "../../lib/settings-persist";
import { applyTheme, resolvedTheme, watchSystemTheme } from "../../lib/theme";
import {
  applyUiLocale,
  formatInvokeError,
  localizedError,
  messagesFor,
  resolveUiLocale,
} from "../../lib/i18n";
import { HISTORY_CHANGED } from "../../lib/history-sync";
import {
  APP_NAVIGATE,
  sectionFromNavigatePayload,
  sectionFromSearch,
} from "../../lib/window-section";
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
  const [historyHasMore, setHistoryHasMore] = useState(false);
  const [settingsRecovered, setSettingsRecovered] = useState(false);
  const historyCursorRef = useRef<string | null>(null);
  const settingsRef = useRef<AppSettings | null>(null);
  settingsRef.current = settings;

  async function refreshHistory(reset = true) {
    const cursor = reset ? null : historyCursorRef.current;
    const [page, configured] = await Promise.all([
      api.history(cursor, query || undefined),
      api.keyConfigured(),
    ]);
    setHistory((current) => (reset ? page.items : [...current, ...page.items]));
    historyCursorRef.current = page.nextCursor;
    setHistoryHasMore(page.hasMore);
    setKeyConfigured(configured);
  }

  async function loadSettings() {
    try {
      const nextSettings = await api.settings();
      setSettings(nextSettings);
      applyTheme(nextSettings.theme);
      applyUiLocale(resolveUiLocale(nextSettings.uiLanguage ?? "auto", navigator.language));
      setLoadError("");
      try {
        const runtime = await api.runtimeInfo();
        setLocalBuild(runtime.localBuild);
        setSettingsRecovered(Boolean(runtime.settingsRecovered));
        if (runtime.localBuild) {
          const fromUrl = sectionFromSearch(window.location.search, true);
          if (fromUrl === "debug") {
            setSection("debug");
          }
        } else {
          setSection((current) => (current === "debug" ? "history" : current));
        }
      } catch {
        setLocalBuild(false);
        setSection((current) => (current === "debug" ? "history" : current));
      }
    } catch (error) {
      setLoadError(
        formatInvokeError(error, messagesFor(resolveUiLocale("auto", navigator.language))),
      );
    }
  }

  useEffect(() => {
    void loadSettings();
    const unlistenHistory = listen(HISTORY_CHANGED, () => {
      void refreshHistory(true).catch((error: unknown) => {
        reportError(
          formatInvokeError(error, messagesFor(resolveUiLocale("auto", navigator.language))),
        );
      });
    });
    const unlistenInsert = listen<string>("session://insert", (event) => {
      const locale = resolveUiLocale(settingsRef.current?.uiLanguage ?? "auto", navigator.language);
      const nextCopy = messagesFor(locale);
      if (event.payload === "copied") {
        toast.success(nextCopy.copiedInsert);
      } else if (event.payload === "partial") {
        toast.warning(nextCopy.copiedPartial);
      } else {
        toast.error(localizedError(event.payload, nextCopy));
      }
    });
    const unlistenSettings = listen<AppSettings>("settings://changed", (event) => {
      setSettings((prev) => acceptSavedSettings(prev, event.payload));
    });
    return () => {
      void unlistenHistory.then((fn) => fn());
      void unlistenInsert.then((fn) => fn());
      void unlistenSettings.then((fn) => fn());
    };
    // listeners capture latest refreshHistory via query-driven reloads
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    void refreshHistory(true).catch((error: unknown) => {
      reportError(
        formatInvokeError(error, messagesFor(resolveUiLocale("auto", navigator.language))),
      );
    });
    // reload on search only; refreshHistory closes over the latest query
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query]);

  useEffect(() => {
    const unlistenNavigate = listen<string>(APP_NAVIGATE, (event) => {
      setSection(sectionFromNavigatePayload(event.payload, localBuild));
    });
    return () => {
      void unlistenNavigate.then((fn) => fn());
    };
  }, [localBuild]);

  useEffect(() => {
    if (!settings) {
      return;
    }
    applyTheme(settings.theme);
    applyUiLocale(resolveUiLocale(settings.uiLanguage ?? "auto", navigator.language));
    return watchSystemTheme(settings.theme);
  }, [settings]);

  const filtered = history;

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
    const seq = nextWriteSeq(currentSettings.writeSeq ?? 0);
    const outgoing = { ...next, writeSeq: seq };
    setSettings(outgoing);
    applyTheme(outgoing.theme);
    try {
      const saved = await api.saveSettings(outgoing);
      setSettings((prev) => acceptSavedSettings(prev, saved));
      applyTheme(saved.theme);
      setSettingsRecovered(false);
    } catch (error) {
      setSettings((prev) =>
        shouldKeepOptimistic(prev, seq) ? (prev as AppSettings) : currentSettings,
      );
      applyTheme(currentSettings.theme);
      reportError(formatInvokeError(error, copy));
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
              hasMore={historyHasMore}
              recovered={settingsRecovered}
              keyConfigured={keyConfigured}
              hotkey={settings.hotkey}
              onQuery={setQuery}
              onClearQuery={() => setQuery("")}
              onLoadMore={() => void refreshHistory(false)}
              onRefresh={() => refreshHistory(true)}
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
              {section === "about" ? <AboutSettings copy={copy} /> : null}
            </div>
          )}
        </main>
      </div>
      <Toaster theme={resolvedTheme(settings.theme)} />
    </>
  );
}
