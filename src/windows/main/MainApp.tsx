import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
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
import { bindEscapeCancel } from "../../lib/escape-cancel";
import { HISTORY_CHANGED, historySearchQuery } from "../../lib/history-sync";
import {
  APP_NAVIGATE,
  sectionFromNavigatePayload,
  sectionFromSearch,
} from "../../lib/window-section";
import { Toaster } from "../../components/ui/sonner";
import { Button } from "../../components/ui/button";
import { FirstRunDisclosure } from "./FirstRunDisclosure";
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

const StatisticsPane = lazy(() =>
  import("./StatisticsPane").then(({ StatisticsPane: Pane }) => ({ default: Pane })),
);

export function MainApp() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [loadError, setLoadError] = useState("");
  const [history, setHistory] = useState<Recording[]>([]);
  const [section, setSection] = useState<Section>(() => sectionFromSearch(window.location.search));
  const [keyConfigured, setKeyConfigured] = useState(false);
  const [query, setQuery] = useState("");
  const [localBuild, setLocalBuild] = useState(false);
  const [version, setVersion] = useState("");
  const [historyHasMore, setHistoryHasMore] = useState(false);
  const [historyError, setHistoryError] = useState<"refresh" | "page" | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [settingsRecovered, setSettingsRecovered] = useState(false);
  const historyCursorRef = useRef<string | null>(null);
  const historyLoadedPageCountRef = useRef(0);
  const historyLoadedQueryRef = useRef<string | undefined>(undefined);
  const historyRequestIdRef = useRef(0);
  const historyPagesInFlightRef = useRef(new Set<string>());
  const queryRef = useRef(query);
  queryRef.current = query;
  const historyQueryRef = useRef(historySearchQuery(query));
  historyQueryRef.current = historySearchQuery(query);
  const settingsRef = useRef<AppSettings | null>(null);
  settingsRef.current = settings;

  function copyForUi() {
    return messagesFor(
      resolveUiLocale(settingsRef.current?.uiLanguage ?? "auto", navigator.language),
    );
  }

  const refreshHistory = useCallback(
    async (reset = true, search = queryRef.current, preserveLoadedPages = false) => {
      const normalizedQuery = historySearchQuery(search);
      if (reset) {
        const requestId = ++historyRequestIdRef.current;
        const canPreservePages =
          preserveLoadedPages && historyLoadedQueryRef.current === normalizedQuery;
        const pagesToLoad = canPreservePages ? Math.max(1, historyLoadedPageCountRef.current) : 1;
        if (!canPreservePages) {
          historyLoadedPageCountRef.current = 0;
        }
        historyLoadedQueryRef.current = normalizedQuery;
        setHistoryError(null);
        setLoadingMore(canPreservePages);
        let cursor: string | null = null;
        let nextCursor: string | null = null;
        let hasMore = false;
        let pagesLoaded = 0;
        const items: Recording[] = [];
        try {
          for (let pageIndex = 0; pageIndex < pagesToLoad; pageIndex += 1) {
            const page = await api.history(cursor, normalizedQuery);
            if (
              requestId !== historyRequestIdRef.current ||
              normalizedQuery !== historyQueryRef.current
            ) {
              return;
            }
            items.push(...page.items);
            pagesLoaded += 1;
            nextCursor = page.nextCursor ?? null;
            hasMore = page.hasMore && nextCursor !== null && nextCursor !== cursor;
            if (!hasMore || !nextCursor) {
              break;
            }
            cursor = nextCursor;
          }

          if (
            requestId !== historyRequestIdRef.current ||
            normalizedQuery !== historyQueryRef.current
          ) {
            return;
          }
          setHistory(uniqueHistoryItems(items));
          historyCursorRef.current = nextCursor;
          setHistoryHasMore(hasMore && nextCursor !== null);
          historyLoadedPageCountRef.current = pagesLoaded;
        } catch {
          if (
            requestId === historyRequestIdRef.current &&
            normalizedQuery === historyQueryRef.current
          ) {
            setHistoryError("refresh");
          }
        } finally {
          if (requestId === historyRequestIdRef.current) {
            setLoadingMore(false);
          }
        }
        return;
      }

      const cursor = historyCursorRef.current;
      const requestId = historyRequestIdRef.current;
      if (!cursor || normalizedQuery !== historyQueryRef.current) {
        return;
      }
      const requestKey = `${requestId}:${cursor}`;
      if (historyPagesInFlightRef.current.has(requestKey)) {
        return;
      }
      historyPagesInFlightRef.current.add(requestKey);
      setLoadingMore(true);
      setHistoryError(null);
      try {
        const page = await api.history(cursor, normalizedQuery);
        if (
          requestId !== historyRequestIdRef.current ||
          normalizedQuery !== historyQueryRef.current ||
          cursor !== historyCursorRef.current
        ) {
          return;
        }
        setHistory((current) => appendUniqueHistoryItems(current, page.items));
        const nextCursor = page.nextCursor ?? null;
        historyCursorRef.current = nextCursor;
        setHistoryHasMore(page.hasMore && nextCursor !== cursor && nextCursor !== null);
        historyLoadedPageCountRef.current += 1;
        historyLoadedQueryRef.current = normalizedQuery;
      } catch {
        if (
          requestId === historyRequestIdRef.current &&
          normalizedQuery === historyQueryRef.current &&
          cursor === historyCursorRef.current
        ) {
          setHistoryError("page");
        }
      } finally {
        historyPagesInFlightRef.current.delete(requestKey);
        if (requestId === historyRequestIdRef.current) {
          setLoadingMore(
            [...historyPagesInFlightRef.current].some((key) => key.startsWith(`${requestId}:`)),
          );
        }
      }
    },
    [],
  );

  const loadSettings = useCallback(async () => {
    try {
      const nextSettings = await api.settings();
      setSettings(nextSettings);
      applyTheme(nextSettings.theme);
      applyUiLocale(resolveUiLocale(nextSettings.uiLanguage ?? "auto", navigator.language));
      setLoadError("");
      try {
        const [runtime, nextVersion] = await Promise.all([api.runtimeInfo(), getVersion()]);
        setVersion(nextVersion);
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
      const locale = resolveUiLocale(settingsRef.current?.uiLanguage ?? "auto", navigator.language);
      setLoadError(formatInvokeError(error, messagesFor(locale)));
    }
  }, []);

  useEffect(() => {
    void loadSettings();
    const unlistenHistory = listen(HISTORY_CHANGED, () => {
      void refreshHistory(true, queryRef.current, true);
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
    const unlistenInsertCancelled = listen<{
      recordingId: string;
      status: "cancelled_before_delivery";
    }>("session://insert-cancelled", (event) => {
      if (event.payload.status === "cancelled_before_delivery") {
        const locale = resolveUiLocale(
          settingsRef.current?.uiLanguage ?? "auto",
          navigator.language,
        );
        toast.warning(messagesFor(locale).insertCancelled);
      }
    });
    const unlistenSettings = listen<AppSettings>("settings://changed", (event) => {
      setSettings((prev) => acceptSavedSettings(prev, event.payload));
    });
    const unbindEscape = bindEscapeCancel(() => {
      void api.cancel().catch(() => undefined);
    });
    return () => {
      unbindEscape();
      void unlistenHistory.then((fn) => fn());
      void unlistenInsert.then((fn) => fn());
      void unlistenInsertCancelled.then((fn) => fn());
      void unlistenSettings.then((fn) => fn());
    };
  }, [loadSettings, refreshHistory]);

  useEffect(() => {
    let active = true;
    void api
      .keyConfigured()
      .then((configured) => {
        if (active) {
          setKeyConfigured(configured);
        }
      })
      .catch((error: unknown) => {
        if (active) {
          const locale = resolveUiLocale(
            settingsRef.current?.uiLanguage ?? "auto",
            navigator.language,
          );
          reportError(formatInvokeError(error, messagesFor(locale)));
        }
      });
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    historyRequestIdRef.current += 1;
    historyCursorRef.current = null;
    setHistoryHasMore(false);
    setLoadingMore(false);
    setHistoryError(null);
    setHistory([]);
    const timer = window.setTimeout(() => {
      void refreshHistory(true, query);
    }, 220);
    return () => window.clearTimeout(timer);
    // Search is debounced; refreshHistory reads the query passed by this effect.
  }, [query, refreshHistory]);

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
        <Button onClick={() => void loadSettings()}>{copyForUi().retryAction}</Button>
      </div>
    );
  }

  if (!settings) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        {copyForUi().loading}
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
    } catch (error) {
      try {
        const persisted = await api.settings();
        setSettings((prev) =>
          prev && (prev.writeSeq ?? 0) > seq ? prev : acceptSavedSettings(prev, persisted),
        );
      } catch {
        setSettings((prev) =>
          shouldKeepOptimistic(prev, seq) ? (prev as AppSettings) : currentSettings,
        );
      }
      reportError(formatInvokeError(error, copy));
    }
  }

  async function acknowledgeFirstRunDisclosure() {
    try {
      const acknowledged = await api.acknowledgeFirstRunDisclosure();
      setSettings((prev) => acceptSavedSettings(prev, acknowledged));
    } catch (error) {
      reportError(formatInvokeError(error, copy));
      throw error;
    }
  }

  return (
    <>
      <div className="flex h-full">
        <SectionNav
          current={section}
          copy={copy}
          localBuild={localBuild}
          version={version}
          onSelect={setSection}
        />
        <main className="flex min-h-0 min-w-0 flex-1 flex-col">
          {!settings.firstRunComplete ? (
            <FirstRunDisclosure
              copy={copy}
              onAcknowledge={acknowledgeFirstRunDisclosure}
              onOpenStorage={() => setSection("historySettings")}
            />
          ) : null}
          {section === "history" ? (
            <HistoryPane
              items={filtered}
              query={query}
              hasMore={historyHasMore}
              historyError={historyError}
              isLoadingMore={loadingMore}
              recovered={settingsRecovered}
              keyConfigured={keyConfigured}
              hotkey={settings.hotkey}
              onQuery={setQuery}
              onClearQuery={() => setQuery("")}
              onLoadMore={() => void refreshHistory(false, queryRef.current)}
              onRefresh={() => refreshHistory(true, queryRef.current, true)}
              onOpenKey={() => setSection("transcription")}
              onOpenSettings={() => setSection("general")}
              copy={copy}
            />
          ) : section === "statistics" ? (
            <div className="min-h-0 flex-1 overflow-auto p-6">
              <Suspense
                fallback={
                  <div
                    className="mx-auto max-w-6xl py-6 text-sm text-muted-foreground"
                    aria-live="polite"
                  >
                    {copy.statisticsLoading}
                  </div>
                }
              >
                <StatisticsPane copy={copy} />
              </Suspense>
            </div>
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
                  onChange={(patch) => void persist({ ...settings, ...patch })}
                />
              ) : null}
              {section === "historySettings" ? (
                <HistorySettings
                  settings={settings}
                  copy={copy}
                  onChange={(patch) => void persist({ ...settings, ...patch })}
                  onDeleteAll={async () => {
                    try {
                      const result = await api.deleteAll();
                      await refreshHistory(true, queryRef.current, true);
                      if (result.failed.length > 0) {
                        toast.warning(
                          copy.historyDeletePartial
                            .replace("{deleted}", String(result.deleted.length))
                            .replace("{failed}", String(result.failed.length)),
                        );
                      } else {
                        toast.success(
                          copy.historyDeleteComplete.replace(
                            "{count}",
                            String(result.deleted.length),
                          ),
                        );
                      }
                    } catch (error) {
                      reportError(formatInvokeError(error, copy));
                    }
                  }}
                  onPreviewRetention={(nextSettings) => api.previewRetentionSettings(nextSettings)}
                  onApplyRetention={async (nextSettings, expectedPreview) => {
                    try {
                      const outgoing = {
                        ...nextSettings,
                        writeSeq: nextWriteSeq(currentSettings.writeSeq ?? 0),
                      };
                      const result = await api.applyRetentionSettings(outgoing, expectedPreview);
                      setSettings((prev) => acceptSavedSettings(prev, result.settings));
                      setSettingsRecovered(false);
                      await refreshHistory(true, queryRef.current, true);
                      if (result.failed.length > 0) {
                        toast.warning(
                          copy.retentionApplyPartial
                            .replace("{deleted}", String(result.deleted.length))
                            .replace("{failed}", String(result.failed.length)),
                        );
                      } else {
                        toast.success(
                          copy.retentionApplied.replace("{count}", String(result.deleted.length)),
                        );
                      }
                      return result;
                    } catch (error) {
                      reportError(formatInvokeError(error, copy));
                      throw error;
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

function uniqueHistoryItems(items: Recording[]): Recording[] {
  const seen = new Set<string>();
  return items.filter((item) => {
    if (seen.has(item.id)) {
      return false;
    }
    seen.add(item.id);
    return true;
  });
}

function appendUniqueHistoryItems(current: Recording[], next: Recording[]): Recording[] {
  const seen = new Set(current.map((item) => item.id));
  const additions = next.filter((item) => {
    if (seen.has(item.id)) {
      return false;
    }
    seen.add(item.id);
    return true;
  });
  return [...current, ...additions];
}
