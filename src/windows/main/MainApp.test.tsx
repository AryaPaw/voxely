import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { HISTORY_CHANGED } from "../../lib/history-sync";
import { APP_NAVIGATE } from "../../lib/window-section";
import { MainApp } from "./MainApp";

const listeners = new Map<string, Array<(event: { payload: unknown }) => void>>();

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
  activePresetId: "stt-fast",
  presets: [],
  firstRunComplete: true,
  uiLanguage: "ru",
  autoUpdateEnabled: true,
  writeSeq: 1,
};

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
  listeners.clear();
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
    fireEvent.click(screen.getByRole("button", { name: /Внешний вид/ }));
    expect(await screen.findByRole("combobox", { name: "Тема" })).toBeInTheDocument();
  });

  it("navigates from tray payload and refreshes on history events", async () => {
    render(<MainApp />);
    await screen.findByRole("heading", { name: "История" });
    await waitFor(() => {
      expect(listeners.get(APP_NAVIGATE)?.length).toBeGreaterThan(0);
    });
    listeners.get(APP_NAVIGATE)?.[0]({ payload: "appearance" });
    expect(await screen.findByRole("combobox", { name: "Тема" })).toBeInTheDocument();
    listeners.get(HISTORY_CHANGED)?.[0]({ payload: null });
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
