import type { ReactElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render as testingRender,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { TooltipProvider } from "../../components/ui/tooltip";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { invoke } from "@tauri-apps/api/core";
import type { Recording } from "../../lib/api";
import { reportError } from "../../lib/system-notify";
import { HISTORY_CHANGED } from "../../lib/history-sync";
import { APP_NAVIGATE } from "../../lib/window-section";
import { MainApp } from "./MainApp";

function render(ui: ReactElement) {
  return testingRender(<TooltipProvider>{ui}</TooltipProvider>);
}

const listeners = new Map<string, Array<(event: { payload: unknown }) => void>>();
let scrollIntoViewDescriptor: PropertyDescriptor | undefined;

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn() },
  Toaster: () => null,
}));

vi.mock("../../components/ui/sonner", () => ({
  Toaster: () => null,
}));

vi.mock("../../lib/system-notify", () => ({
  reportError: vi.fn(),
}));

const settingsPayload = {
  hotkey: "Ctrl+Shift+Space",
  startWithWindows: false,
  closeToTray: true,
  notifications: true,
  inputDevice: "default",
  keepOriginalRecordings: true,
  theme: "dark",
  language: "ru",
  model: "openai/gpt-transcribe",
  customModel: null,
  insertionMode: "unicode",
  retention: "3d",
  storageLimit: "1gb",
  debugLogging: false,
  retry: {
    automaticRetries: true,
    additionalRetries: 2,
    connectTimeoutMs: 8000,
    requestTimeoutMs: 20000,
    initialRetryDelayMs: 500,
    maxRetryDelayMs: 8000,
    totalOperationTimeoutMs: 720000,
  },
  textReplacements: { enabled: false, rules: [] },
  activePresetId: "stt-fast",
  presets: [],
  firstRunComplete: true,
  uiLanguage: "ru",
  autoUpdateEnabled: true,
  writeSeq: 1,
};

function objectArgs(args: Parameters<typeof invoke>[1]): Record<string, unknown> {
  return args && typeof args === "object" && !Array.isArray(args)
    ? (args as Record<string, unknown>)
    : {};
}

async function defaultInvoke(cmd: string, args?: Parameters<typeof invoke>[1]) {
  const payload = objectArgs(args);
  if (cmd === "get_settings") {
    return settingsPayload;
  }
  if (cmd === "save_settings") {
    return { ...(payload.settings as object), writeSeq: 2 };
  }
  if (cmd === "acknowledge_first_run_disclosure") {
    return { ...settingsPayload, firstRunComplete: true, writeSeq: 2 };
  }
  if (cmd === "list_history_summaries") {
    return { items: [], nextCursor: "c1", total: 0, hasMore: true };
  }
  if (cmd === "api_key_configured") {
    return true;
  }
  if (cmd === "get_runtime_info") {
    return { localBuild: true, buildDate: "2026-09-17", settingsRecovered: true };
  }
  if (cmd === "get_model_compare") {
    return {
      recording: false,
      running: false,
      nonce: 0,
      listenPath: null,
      sttPath: null,
      runId: null,
      slots: [],
    };
  }
  if (cmd === "discover_models") {
    return [];
  }
  if (cmd === "list_microphones") {
    return [{ id: "default", name: "Default", isDefault: true, available: true }];
  }
  if (cmd === "preview_dsp") {
    return {
      originalPath: "a.wav",
      processedPath: "b.wav",
      peak: 0.1,
      rms: 0.1,
      clipCount: 0,
      nonce: 1,
    };
  }
  if (cmd === "get_meter" || cmd === "start_input_meter" || cmd === "stop_input_meter") {
    return undefined;
  }
  return undefined;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function historyItem(id: string, transcript: string): Recording {
  return {
    id,
    createdAt: "2026-01-02T10:00:00.000Z",
    durationMs: 1000,
    rawAudioPath: "a.wav",
    processedAudioPath: "a.processed.wav",
    transcript,
    status: "completed",
    provider: "openrouter",
    model: "openai/gpt-transcribe",
    attemptCount: 1,
    lastErrorCode: null,
    lastErrorMessage: null,
    cost: null,
    generationId: null,
    latencyMs: null,
  };
}

beforeEach(() => {
  vi.mocked(invoke).mockImplementation(defaultInvoke);
});

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn(async () => "0.2.11"),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === "get_settings") {
      return settingsPayload;
    }
    if (cmd === "save_settings") {
      return { ...(args?.settings as object), writeSeq: 2 };
    }
    if (cmd === "acknowledge_first_run_disclosure") {
      return { ...settingsPayload, firstRunComplete: true, writeSeq: 2 };
    }
    if (cmd === "list_history_summaries") {
      return { items: [], nextCursor: "c1", total: 0, hasMore: true };
    }
    if (cmd === "api_key_configured") {
      return true;
    }
    if (cmd === "get_runtime_info") {
      return { localBuild: true, buildDate: "2026-09-17", settingsRecovered: true };
    }
    if (cmd === "get_model_compare") {
      return {
        recording: false,
        running: false,
        nonce: 0,
        listenPath: null,
        sttPath: null,
        runId: null,
        slots: [],
      };
    }
    if (cmd === "discover_models") {
      return [];
    }
    if (cmd === "list_microphones") {
      return [{ id: "default", name: "Default", isDefault: true, available: true }];
    }
    if (cmd === "preview_dsp") {
      return {
        originalPath: "a.wav",
        processedPath: "b.wav",
        peak: 0.1,
        rms: 0.1,
        clipCount: 0,
        nonce: 1,
      };
    }
    if (cmd === "get_meter") {
      return { rms: 0, peak: 0, levels: [] };
    }
    if (cmd === "start_input_meter" || cmd === "stop_input_meter") {
      return undefined;
    }
    return undefined;
  }),
  convertFileSrc: (path: string) => path,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (event: string, handler: (event: { payload: unknown }) => void) => {
    const bucket = listeners.get(event) ?? [];
    bucket.push(handler);
    listeners.set(event, bucket);
    return () => undefined;
  }),
}));

afterEach(() => {
  cleanup();
  if (scrollIntoViewDescriptor) {
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", scrollIntoViewDescriptor);
  } else {
    delete (HTMLElement.prototype as Partial<HTMLElement>).scrollIntoView;
  }
  scrollIntoViewDescriptor = undefined;
  listeners.clear();
  vi.mocked(invoke).mockReset();
  vi.useRealTimers();
});

describe("MainApp", () => {
  it("loads history with settings locale", async () => {
    render(<MainApp />);
    expect(await screen.findByRole("heading", { name: "История" })).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByText(/Записей пока нет/)).toBeInTheDocument();
    });
  });

  it("toasts insert results in the settings language and can open appearance", async () => {
    render(<MainApp />);
    await screen.findByRole("heading", { name: "История" });
    await waitFor(() => {
      expect(listeners.get("session://insert")?.length).toBeGreaterThan(0);
    });
    listeners.get("session://insert")?.[0]({ payload: "copied" });
    expect(toast.success).toHaveBeenCalled();
    listeners.get("session://insert")?.[0]({ payload: "partial" });
    expect(toast.warning).toHaveBeenCalled();
    listeners.get("session://insert")?.[0]({ payload: "InsertFailed" });
    expect(toast.error).toHaveBeenCalled();
    await waitFor(() => {
      expect(listeners.get("session://insert-cancelled")?.length).toBeGreaterThan(0);
    });
    listeners.get("session://insert-cancelled")?.[0]({
      payload: { recordingId: "recording-1", status: "cancelled_before_delivery" },
    });
    expect(toast.warning).toHaveBeenCalledWith(
      "Вставка отменена до передачи текста целевому приложению.",
    );
    fireEvent.click(screen.getByRole("button", { name: /Внешний вид/ }));
    expect(await screen.findByRole("combobox", { name: "Тема" })).toBeInTheDocument();
    const { invoke } = await import("@tauri-apps/api/core");
    fireEvent.keyDown(window, { key: "Escape", bubbles: true });
    expect(invoke).toHaveBeenCalledWith("cancel_dictation");
  });

  it("navigates from tray payload and refreshes on history events", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    let historyLoads = 0;
    vi.mocked(invoke).mockImplementation((cmd: string, args?: Parameters<typeof invoke>[1]) => {
      if (cmd === "list_history_summaries") historyLoads += 1;
      return defaultInvoke(cmd, args);
    });
    render(<MainApp />);
    await screen.findByRole("heading", { name: "История" });
    await waitFor(() => {
      expect(listeners.get(APP_NAVIGATE)?.length).toBeGreaterThan(0);
      expect(historyLoads).toBeGreaterThan(0);
    });
    listeners.get(APP_NAVIGATE)?.[0]({ payload: "appearance" });
    expect(await screen.findByRole("combobox", { name: "Тема" })).toBeInTheDocument();
    const loadsBeforeEvent = historyLoads;
    await act(async () => {
      listeners.get(HISTORY_CHANGED)?.[0]({ payload: null });
    });
    await waitFor(() => expect(historyLoads).toBeGreaterThan(loadsBeforeEvent));
  });

  it("opens each main section and persists a settings toggle", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    render(<MainApp />);
    await screen.findByRole("heading", { name: "История" });
    fireEvent.click(screen.getByRole("button", { name: /Сравнение/ }));
    expect(await screen.findByText(/Один клип/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Общие/ }));
    expect(await screen.findByText("Глобальный хоткей")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("switch", { name: "Запускать вместе с Windows" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("save_settings", expect.anything());
    });
    fireEvent.click(screen.getByRole("button", { name: /Микрофон/ }));
    fireEvent.click(screen.getByRole("button", { name: /Фильтры/ }));
    fireEvent.click(screen.getByRole("button", { name: /Расшифровка/ }));
    fireEvent.click(screen.getByRole("button", { name: /Хранение/ }));
    fireEvent.click(screen.getByRole("button", { name: /Дополнительно/ }));
    fireEvent.click(screen.getByRole("button", { name: /Песочница/ }));
    fireEvent.click(screen.getByRole("button", { name: /О программе/ }));
    expect(await screen.findByText(/0.2.11/)).toBeInTheDocument();
  });

  it("keeps the recovered-settings warning after an ordinary settings save", async () => {
    render(<MainApp />);
    await screen.findByRole("heading", { name: "История" });
    expect(
      await screen.findByText(/Настройки восстановлены со значениями по умолчанию/),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Общие/ }));
    fireEvent.click(screen.getByRole("switch", { name: "Запускать вместе с Windows" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("save_settings", expect.anything());
    });

    fireEvent.click(screen.getByRole("button", { name: /История/ }));
    expect(
      await screen.findByText(/Настройки восстановлены со значениями по умолчанию/),
    ).toBeInTheDocument();
  });

  it("requires explicit first-run disclosure acknowledgement and does not use save_settings", async () => {
    vi.mocked(invoke).mockImplementation(
      async (cmd: string, args?: Parameters<typeof invoke>[1]) => {
        if (cmd === "get_settings") {
          return { ...settingsPayload, firstRunComplete: false };
        }
        return defaultInvoke(cmd, args);
      },
    );
    render(<MainApp />);
    expect(
      await screen.findByRole("heading", { name: "Перед первой диктовкой" }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Настройки хранения" }));
    expect(await screen.findByRole("heading", { name: "Хранение" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Согласен и продолжить" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("acknowledge_first_run_disclosure");
    });
    expect(invoke).not.toHaveBeenCalledWith("save_settings", expect.anything());
    await waitFor(() => {
      expect(
        screen.queryByRole("heading", { name: "Перед первой диктовкой" }),
      ).not.toBeInTheDocument();
    });
  });

  it("ignores stale search results and debounces requests without rechecking the key", async () => {
    const alpha = deferred<{
      items: Recording[];
      nextCursor: null;
      total: number;
      hasMore: false;
    }>();
    const beta = deferred<{
      items: Recording[];
      nextCursor: null;
      total: number;
      hasMore: false;
    }>();
    vi.mocked(invoke).mockImplementation((cmd: string, args?: Parameters<typeof invoke>[1]) => {
      const payload = objectArgs(args);
      if (cmd === "list_history_summaries") {
        if (payload.query === "alpha") {
          return alpha.promise;
        }
        if (payload.query === "beta") {
          return beta.promise;
        }
      }
      return defaultInvoke(cmd, args);
    });
    render(<MainApp />);
    await screen.findByText(/Записей пока нет/);
    vi.useFakeTimers();

    const search = screen.getByRole("textbox", { name: "Поиск по расшифровкам" });
    fireEvent.change(search, { target: { value: "a" } });
    fireEvent.change(search, { target: { value: "alpha" } });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(220);
    });
    expect(invoke).toHaveBeenCalledWith(
      "list_history_summaries",
      expect.objectContaining({ query: "alpha", cursor: null }),
    );

    fireEvent.change(search, { target: { value: "beta" } });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(220);
    });
    await act(async () => {
      beta.resolve({
        items: [historyItem("beta", "beta result")],
        nextCursor: null,
        total: 1,
        hasMore: false,
      });
    });
    expect(screen.getByText("beta result")).toBeInTheDocument();
    await act(async () => {
      alpha.resolve({
        items: [historyItem("alpha", "stale alpha result")],
        nextCursor: null,
        total: 1,
        hasMore: false,
      });
    });
    expect(screen.getByText("beta result")).toBeInTheDocument();
    expect(screen.queryByText("stale alpha result")).not.toBeInTheDocument();
    expect(
      vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === "list_history_summaries"),
    ).toHaveLength(2);
    expect(
      vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd === "api_key_configured"),
    ).toHaveLength(1);
  });

  it("deduplicates history pages and ignores a page that finishes after a refresh", async () => {
    const stalePage = deferred<{
      items: Recording[];
      nextCursor: null;
      total: number;
      hasMore: false;
    }>();
    let firstPage = true;
    vi.mocked(invoke).mockImplementation((cmd: string, args?: Parameters<typeof invoke>[1]) => {
      if (cmd === "list_history_summaries") {
        if (objectArgs(args).cursor === "c1") {
          return stalePage.promise;
        }
        if (firstPage) {
          firstPage = false;
          return Promise.resolve({
            items: [historyItem("one", "first row")],
            nextCursor: "c1",
            total: 2,
            hasMore: true,
          });
        }
        return Promise.resolve({
          items: [historyItem("fresh", "refreshed row")],
          nextCursor: null,
          total: 1,
          hasMore: false,
        });
      }
      return defaultInvoke(cmd, args);
    });
    render(<MainApp />);
    expect(await screen.findByText("first row")).toBeInTheDocument();
    await waitFor(() => expect(listeners.get(HISTORY_CHANGED)?.length).toBeGreaterThan(0));
    const loadMore = screen.getByRole("button", { name: "Показать ещё" });
    fireEvent.click(loadMore);
    fireEvent.click(loadMore);
    expect(
      vi
        .mocked(invoke)
        .mock.calls.filter(
          ([cmd, args]) => cmd === "list_history_summaries" && objectArgs(args).cursor === "c1",
        ),
    ).toHaveLength(1);

    await act(async () => {
      listeners.get(HISTORY_CHANGED)?.[0]({ payload: null });
    });
    expect(await screen.findByText("refreshed row")).toBeInTheDocument();
    await act(async () => {
      stalePage.resolve({
        items: [historyItem("one", "first row"), historyItem("stale", "stale page row")],
        nextCursor: null,
        total: 3,
        hasMore: false,
      });
    });
    expect(screen.getByText("refreshed row")).toBeInTheDocument();
    expect(screen.queryByText("stale page row")).not.toBeInTheDocument();
  });

  it("preserves loaded pages on history change and continues from the refreshed cursor", async () => {
    const historyCursors: Array<string | null> = [];
    let firstPageLoads = 0;
    vi.mocked(invoke).mockImplementation((cmd: string, args?: Parameters<typeof invoke>[1]) => {
      if (cmd === "list_history_summaries") {
        const cursor = objectArgs(args).cursor as string | null;
        historyCursors.push(cursor);
        if (cursor === null) {
          firstPageLoads += 1;
          return Promise.resolve({
            items:
              firstPageLoads === 1
                ? [historyItem("one", "initial first row")]
                : [historyItem("new", "new first row"), historyItem("one", "updated first row")],
            nextCursor: firstPageLoads === 1 ? "old-cursor-1" : "new-cursor-1",
            total: 2,
            hasMore: true,
          });
        }
        if (cursor === "old-cursor-1") {
          return Promise.resolve({
            items: [historyItem("two", "initial second row")],
            nextCursor: "old-cursor-2",
            total: 2,
            hasMore: true,
          });
        }
        if (cursor === "new-cursor-1") {
          return Promise.resolve({
            items: [
              historyItem("one", "overlap duplicate row"),
              historyItem("two", "updated second row"),
            ],
            nextCursor: "new-cursor-2",
            total: 3,
            hasMore: true,
          });
        }
        if (cursor === "new-cursor-2") {
          return Promise.resolve({
            items: [historyItem("three", "third row")],
            nextCursor: null,
            total: 4,
            hasMore: false,
          });
        }
      }
      return defaultInvoke(cmd, args);
    });
    render(<MainApp />);
    expect(await screen.findByText("initial first row")).toBeInTheDocument();
    await waitFor(() => expect(listeners.get(HISTORY_CHANGED)?.length).toBeGreaterThan(0));
    fireEvent.click(screen.getByRole("button", { name: "Показать ещё" }));
    expect(await screen.findByText("initial second row")).toBeInTheDocument();

    await act(async () => {
      listeners.get(HISTORY_CHANGED)?.[0]({ payload: null });
    });

    expect(await screen.findByText("updated first row")).toBeInTheDocument();
    expect(screen.queryByText("initial first row")).not.toBeInTheDocument();
    expect(screen.queryByText("initial second row")).not.toBeInTheDocument();
    expect(screen.queryByText("overlap duplicate row")).not.toBeInTheDocument();
    expect(await screen.findByText("updated second row")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Показать ещё" }));
    expect(await screen.findByText("third row")).toBeInTheDocument();
    expect(historyCursors).toEqual([null, "old-cursor-1", null, "new-cursor-1", "new-cursor-2"]);
  });

  it("shows a failed initial history load and recovers when the user retries", async () => {
    let historyLoads = 0;
    vi.mocked(invoke).mockImplementation((cmd: string, args?: Parameters<typeof invoke>[1]) => {
      if (cmd === "list_history_summaries") {
        historyLoads += 1;
        if (historyLoads === 1) return Promise.reject(new Error("history unavailable"));
        return Promise.resolve({
          items: [historyItem("recovered", "recovered history row")],
          nextCursor: null,
          total: 1,
          hasMore: false,
        });
      }
      return defaultInvoke(cmd, args);
    });
    render(<MainApp />);

    expect(await screen.findByRole("alert")).toHaveTextContent("Не удалось загрузить историю.");
    fireEvent.click(screen.getByRole("button", { name: "Повторить загрузку" }));

    expect(await screen.findByText("recovered history row")).toBeInTheDocument();
    expect(historyLoads).toBe(2);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("retries a failed history page with the same cursor and appends its result", async () => {
    let pageAttempts = 0;
    vi.mocked(invoke).mockImplementation((cmd: string, args?: Parameters<typeof invoke>[1]) => {
      if (cmd === "list_history_summaries") {
        if (objectArgs(args).cursor === "cursor-2") {
          pageAttempts += 1;
          if (pageAttempts === 1) return Promise.reject(new Error("page unavailable"));
          return Promise.resolve({
            items: [historyItem("second", "second history row")],
            nextCursor: null,
            total: 2,
            hasMore: false,
          });
        }
        return Promise.resolve({
          items: [historyItem("first", "first history row")],
          nextCursor: "cursor-2",
          total: 2,
          hasMore: true,
        });
      }
      return defaultInvoke(cmd, args);
    });
    render(<MainApp />);

    expect(await screen.findByText("first history row")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Показать ещё" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Не удалось загрузить историю.");
    fireEvent.click(screen.getByRole("button", { name: "Повторить загрузку" }));

    expect(await screen.findByText("second history row")).toBeInTheDocument();
    expect(pageAttempts).toBe(2);
    expect(screen.queryByRole("button", { name: "Показать ещё" })).not.toBeInTheDocument();
  });

  it("accepts newer settings events and ignores an older revision", async () => {
    render(<MainApp />);
    await screen.findByRole("heading", { name: "История" });
    fireEvent.click(screen.getByRole("button", { name: /Общие/ }));
    const startWithWindows = screen.getByRole("switch", { name: "Запускать вместе с Windows" });
    expect(startWithWindows).not.toBeChecked();
    await waitFor(() => expect(listeners.get("settings://changed")?.length).toBeGreaterThan(0));

    const emitSettings = listeners.get("settings://changed")?.[0];
    await act(async () => {
      emitSettings?.({
        payload: { ...settingsPayload, startWithWindows: true, theme: "light", writeSeq: 3 },
      });
    });
    await waitFor(() => expect(startWithWindows).toBeChecked());
    expect(document.documentElement.dataset.theme).toBe("light");

    await act(async () => {
      emitSettings?.({
        payload: { ...settingsPayload, startWithWindows: false, theme: "dark", writeSeq: 2 },
      });
    });
    expect(startWithWindows).toBeChecked();
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(invoke).not.toHaveBeenCalledWith("save_settings", expect.anything());
  });

  it("rolls back an optimistic setting when both save and settings read-back fail", async () => {
    scrollIntoViewDescriptor = Object.getOwnPropertyDescriptor(
      HTMLElement.prototype,
      "scrollIntoView",
    );
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(),
    });
    let settingsReads = 0;
    vi.mocked(reportError).mockClear();
    vi.mocked(invoke).mockImplementation((cmd: string, args?: Parameters<typeof invoke>[1]) => {
      if (cmd === "get_settings") {
        settingsReads += 1;
        return settingsReads === 1
          ? Promise.resolve(settingsPayload)
          : Promise.reject(new Error("read-back unavailable"));
      }
      if (cmd === "save_settings") return Promise.reject(new Error("write unavailable"));
      return defaultInvoke(cmd, args);
    });
    render(<MainApp />);
    await screen.findByRole("heading", { name: "История" });
    fireEvent.click(screen.getByRole("button", { name: /Внешний вид/ }));
    const theme = screen.getByRole("combobox", { name: "Тема" });
    fireEvent.click(theme);
    fireEvent.click(await screen.findByRole("option", { name: "Системная" }));

    await waitFor(() => expect(reportError).toHaveBeenCalledOnce());
    await waitFor(() => expect(theme).toHaveTextContent("Тёмная"));
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(settingsReads).toBe(2);
  });

  it("reconciles a failed save with persisted settings without applying a stale theme", async () => {
    scrollIntoViewDescriptor = Object.getOwnPropertyDescriptor(
      HTMLElement.prototype,
      "scrollIntoView",
    );
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(),
    });
    const firstSave = deferred<object>();
    const secondSave = deferred<object>();
    let saveCount = 0;
    let persisted = settingsPayload;
    vi.mocked(invoke).mockImplementation((cmd: string, args?: Parameters<typeof invoke>[1]) => {
      if (cmd === "save_settings") {
        saveCount += 1;
        if (saveCount === 1) {
          return firstSave.promise;
        }
        persisted = {
          ...(objectArgs(args).settings as object),
          theme: "light",
          writeSeq: 3,
        } as typeof settingsPayload;
        return secondSave.promise;
      }
      if (cmd === "get_settings" && saveCount > 0) {
        return Promise.resolve(persisted);
      }
      return defaultInvoke(cmd, args);
    });
    render(<MainApp />);
    await screen.findByRole("heading", { name: "История" });
    fireEvent.click(screen.getByRole("button", { name: /Внешний вид/ }));
    const theme = screen.getByRole("combobox", { name: "Тема" });
    fireEvent.click(theme);
    fireEvent.click(await screen.findByRole("option", { name: "Системная" }));
    await waitFor(() => expect(saveCount).toBe(1));
    fireEvent.click(screen.getByRole("combobox", { name: "Тема" }));
    fireEvent.click(await screen.findByRole("option", { name: "Светлая" }));
    await waitFor(() => expect(saveCount).toBe(2));

    await act(async () => {
      secondSave.resolve(persisted);
    });
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("light"));
    await act(async () => {
      firstSave.reject(new Error("autostart unavailable"));
    });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_settings"));
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("shows partial delete results from the backend", async () => {
    vi.mocked(invoke).mockImplementation((cmd: string, args?: Parameters<typeof invoke>[1]) => {
      if (cmd === "delete_all_history") {
        return Promise.resolve({ deleted: ["one"], failed: ["two", "three"] });
      }
      return defaultInvoke(cmd, args);
    });
    render(<MainApp />);
    await screen.findByRole("heading", { name: "История" });
    fireEvent.click(screen.getByRole("button", { name: /Хранение/ }));
    fireEvent.click(screen.getByRole("button", { name: "Удалить всю историю" }));
    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Удалить" }));
    await waitFor(() =>
      expect(toast.warning).toHaveBeenCalledWith("Удалено: 1. Не удалось удалить: 2."),
    );
  });

  it("previews and confirms retention changes, then refreshes history and reports partial results", async () => {
    scrollIntoViewDescriptor = Object.getOwnPropertyDescriptor(
      HTMLElement.prototype,
      "scrollIntoView",
    );
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: vi.fn(),
    });
    vi.mocked(toast.warning).mockClear();
    let historyLoads = 0;
    vi.mocked(invoke).mockImplementation((cmd: string, args?: Parameters<typeof invoke>[1]) => {
      const payload = objectArgs(args);
      if (cmd === "preview_retention_settings") {
        return Promise.resolve({
          entriesToDelete: 4,
          recordingIdsToDelete: ["one", "two", "three", "four"],
          filesToDelete: 5,
          bytesToFree: 1_048_576,
          protectedEntries: 1,
          protectedBytes: 2_048,
          totalAudioBytes: 2_097_152,
        });
      }
      if (cmd === "apply_retention_settings") {
        const next = payload.settings as object;
        return Promise.resolve({
          settings: { ...next, writeSeq: 2 },
          deleted: ["one", "two"],
          failed: ["three"],
        });
      }
      if (cmd === "list_history_summaries") historyLoads += 1;
      return defaultInvoke(cmd, args);
    });
    render(<MainApp />);
    await screen.findByRole("heading", { name: "История" });
    await waitFor(() => expect(historyLoads).toBeGreaterThan(0));
    const beforeApply = historyLoads;
    fireEvent.click(screen.getByRole("button", { name: /Хранение/ }));
    fireEvent.click(screen.getByRole("combobox", { name: "Хранить записи" }));
    fireEvent.click(await screen.findByRole("option", { name: "1 день" }));
    expect(await screen.findByText("Записей истории будет удалено")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Применить настройки хранения" }));
    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Применить настройки хранения" }));

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith(
        "apply_retention_settings",
        expect.objectContaining({
          confirmed: true,
          expectedPreview: expect.objectContaining({
            recordingIdsToDelete: ["one", "two", "three", "four"],
          }),
          settings: expect.objectContaining({ retention: "1d" }),
        }),
      );
      expect(toast.warning).toHaveBeenCalledWith("Удалено записей: 2. Не удалось обработать: 1.");
      expect(historyLoads).toBeGreaterThan(beforeApply);
    });
  });

  it("shows a load error and retries", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    let attempts = 0;
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "get_settings") {
        attempts += 1;
        if (attempts === 1) {
          throw new Error("settings missing");
        }
        return settingsPayload;
      }
      if (cmd === "get_runtime_info") {
        throw new Error("runtime down");
      }
      if (cmd === "list_history_summaries") {
        return { items: [], nextCursor: null, total: 0, hasMore: false };
      }
      if (cmd === "api_key_configured") {
        return true;
      }
      if (cmd === "save_settings") {
        throw new Error("save failed");
      }
      return undefined;
    });
    render(<MainApp />);
    fireEvent.click(await screen.findByRole("button", { name: "Retry" }));
    expect(await screen.findByRole("heading", { name: /История|History/ })).toBeInTheDocument();
  });
});
