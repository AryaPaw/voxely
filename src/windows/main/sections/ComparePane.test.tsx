import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AppSettings, CompareState } from "../../../lib/api";
import { messagesFor } from "../../../lib/i18n";
import { ComparePane } from "./ComparePane";
import { useState } from "react";

const compareEventHarness = vi.hoisted(() => ({
  stateHandler: null as ((event: { payload: unknown }) => void) | null,
  errorHandler: null as ((event: { payload: unknown }) => void) | null,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "get_model_compare") {
      return {
        recording: false,
        running: false,
        listenPath: "C:/tmp/model-compare.listen.wav",
        sttPath: "C:/tmp/model-compare.stt.wav",
        nonce: 1,
        runId: null,
        slots: [],
      };
    }
    if (cmd === "start_model_compare") {
      return undefined;
    }
    if (cmd === "stop_model_compare") {
      return {
        recording: false,
        running: false,
        listenPath: "C:/tmp/model-compare.2.listen.wav",
        sttPath: "C:/tmp/model-compare.2.stt.wav",
        nonce: 2,
        runId: null,
        slots: [],
      };
    }
    return undefined;
  }),
  convertFileSrc: (path: string) => path,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (event: string, handler: (event: { payload: unknown }) => void) => {
    if (event === "compare://state") compareEventHarness.stateHandler = handler;
    if (event === "compare://error") compareEventHarness.errorHandler = handler;
    return () => undefined;
  }),
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

afterEach(() => {
  cleanup();
  compareEventHarness.stateHandler = null;
  compareEventHarness.errorHandler = null;
});

function compareState(overrides: Partial<CompareState> = {}): CompareState {
  return {
    recording: false,
    running: false,
    nonce: 1,
    listenPath: "C:/tmp/compare.wav",
    sttPath: null,
    runId: null,
    slots: [],
    ...overrides,
  };
}

function settings(): AppSettings {
  return {
    hotkey: "Ctrl+Shift+Space",
    startWithWindows: false,
    closeToTray: true,
    notifications: true,
    inputDevice: "default",
    keepOriginalRecordings: true,
    theme: "dark",
    language: "en",
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
    activePresetId: "stt-optimized",
    presets: [],
    firstRunComplete: true,
    compareModels: ["openai/gpt-transcribe", "openai/whisper-large-v3"],
  };
}

describe("ComparePane", () => {
  it("does not let a late initial snapshot overwrite a newer state event", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    compareEventHarness.stateHandler = null;
    let resolveSnapshot: ((value: unknown) => void) | undefined;
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "get_model_compare") {
        return new Promise((resolve) => {
          resolveSnapshot = resolve;
        });
      }
      return Promise.resolve(undefined);
    });

    render(
      <ComparePane
        settings={settings()}
        copy={messagesFor("en")}
        keyConfigured
        onChange={() => undefined}
        onOpenKey={() => undefined}
      />,
    );
    await waitFor(() => expect(compareEventHarness.stateHandler).toBeTypeOf("function"));

    const emitState = compareEventHarness.stateHandler as unknown as (event: {
      payload: CompareState;
    }) => void;
    const latest: CompareState = {
      recording: false,
      running: false,
      nonce: 2,
      listenPath: "clip-2.wav",
      sttPath: "clip-2.wav",
      runId: null,
      slots: [],
    };
    act(() => emitState({ payload: latest }));
    await act(async () => {
      resolveSnapshot?.({ ...latest, running: true, runId: "stale-run" });
      await Promise.resolve();
    });

    expect(screen.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Compare" })).toBeEnabled();
  });

  it("keeps focus and caret while editing a completed model result", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation(async () => ({
      recording: false,
      running: false,
      nonce: 1,
      listenPath: "clip.wav",
      sttPath: "clip.wav",
      runId: "run",
      slots: [
        {
          slotId: "slot",
          model: "openai/gpt-transcribe",
          clipNonce: 1,
          runId: "run",
          status: "done",
          text: "Completed transcript",
          attempt: 1,
          cost: 0.1,
          latencyMs: 1500,
          error: null,
        },
      ],
    }));
    function Harness() {
      const [current, setCurrent] = useState(settings());
      return (
        <ComparePane
          settings={current}
          copy={messagesFor("en")}
          keyConfigured
          onChange={(patch) => setCurrent((value) => ({ ...value, ...patch }))}
          onOpenKey={() => undefined}
        />
      );
    }
    render(<Harness />);
    expect(await screen.findByText("Completed transcript")).toBeInTheDocument();
    const input = screen.getByRole("textbox", { name: "model-1" }) as HTMLInputElement;
    input.focus();
    for (const value of ["vendor/a", "vendor/ab", "vendor/abc"]) {
      fireEvent.change(input, { target: { value } });
      expect(screen.getByRole("textbox", { name: "model-1" })).toBe(input);
      expect(input).toHaveFocus();
      expect(input.selectionStart).toBe(value.length);
    }
    expect(screen.queryByText("Completed transcript")).not.toBeInTheDocument();
  });
  it("keeps compare disabled without a key", () => {
    render(
      <ComparePane
        settings={settings()}
        copy={messagesFor("en")}
        keyConfigured={false}
        onChange={() => undefined}
        onOpenKey={() => undefined}
      />,
    );
    expect(screen.getByRole("button", { name: "Compare" })).toBeDisabled();
    expect(screen.getByText(/Add an API key/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "OpenRouter catalog" })).toBeInTheDocument();
  });

  it("opens the OpenRouter transcription catalog", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    render(
      <ComparePane
        settings={settings()}
        copy={messagesFor("en")}
        keyConfigured
        onChange={() => undefined}
        onOpenKey={() => undefined}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "OpenRouter catalog" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("open_openrouter_models");
    });
  });

  it("adds a fifth slot and confirms the default model", async () => {
    const { toast } = await import("sonner");
    const onChange = vi.fn();
    const { rerender } = render(
      <ComparePane
        settings={settings()}
        copy={messagesFor("en")}
        keyConfigured
        onChange={onChange}
        onOpenKey={() => undefined}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Add model" }));
    expect(onChange).toHaveBeenCalledWith({
      compareModels: ["openai/gpt-transcribe", "openai/whisper-large-v3", ""],
    });
    fireEvent.click(screen.getByRole("button", { name: "Set as default" }));
    expect(onChange).toHaveBeenCalledWith({ model: "openai/whisper-large-v3" });
    expect(toast.success).toHaveBeenCalledWith("This model is now the default for dictation.");
    rerender(
      <ComparePane
        settings={{ ...settings(), model: "openai/whisper-large-v3" }}
        copy={messagesFor("en")}
        keyConfigured
        onChange={onChange}
        onOpenKey={() => undefined}
      />,
    );
    expect(screen.getByRole("button", { name: "Default" })).toBeDisabled();
    expect(screen.getAllByRole("button", { name: "Drag to reorder" })).toHaveLength(2);
  });

  it("keeps add available past four models", () => {
    const onChange = vi.fn();
    render(
      <ComparePane
        settings={{
          ...settings(),
          compareModels: ["a", "b", "c", "d"],
        }}
        copy={messagesFor("en")}
        keyConfigured
        onChange={onChange}
        onOpenKey={() => undefined}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Add model" }));
    expect(onChange).toHaveBeenCalledWith({ compareModels: ["a", "b", "c", "d", ""] });
  });

  it("reloads the preview after a new take", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    let recording = false;
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "start_model_compare") {
        recording = true;
        return;
      }
      if (cmd === "stop_model_compare") recording = false;
      return {
        recording,
        running: false,
        nonce: cmd === "stop_model_compare" ? 2 : 1,
        listenPath:
          cmd === "stop_model_compare"
            ? "C:/tmp/model-compare.2.listen.wav"
            : "C:/tmp/model-compare.listen.wav",
        sttPath: "clip.wav",
        runId: null,
        slots: [],
      };
    });
    const { container } = render(
      <ComparePane
        settings={settings()}
        copy={messagesFor("en")}
        keyConfigured
        onChange={() => undefined}
        onOpenKey={() => undefined}
      />,
    );
    await waitFor(() => {
      expect(container.querySelector("audio")).toHaveAttribute(
        "src",
        "C:/tmp/model-compare.listen.wav?n=1",
      );
    });
    fireEvent.click(screen.getByRole("button", { name: "Record clip" }));
    fireEvent.click(await screen.findByRole("button", { name: "Stop" }));
    await waitFor(() => {
      expect(container.querySelector("audio")).toHaveAttribute(
        "src",
        "C:/tmp/model-compare.2.listen.wav?n=2",
      );
    });
  });

  it("binds a compare result to the matching model, not the slot index", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "get_model_compare") {
        return {
          recording: false,
          running: false,
          listenPath: "C:/tmp/model-compare.listen.wav",
          sttPath: "C:/tmp/model-compare.stt.wav",
          nonce: 4,
          runId: "run-9",
          slots: [
            {
              slotId: "s-whisper",
              model: "openai/whisper-large-v3",
              status: "done",
              text: "whisper text",
              error: null,
              attempt: 1,
              cost: 0.01,
              latencyMs: 10,
              clipNonce: 4,
              runId: "run-9",
            },
          ],
        };
      }
      return undefined;
    });
    render(
      <ComparePane
        settings={settings()}
        copy={messagesFor("en")}
        keyConfigured
        onChange={() => undefined}
        onOpenKey={() => undefined}
      />,
    );
    expect(await screen.findByText("whisper text")).toBeInTheDocument();
    expect(screen.getByText(/cost 0.01/)).toBeInTheDocument();
    const rows = screen.getAllByRole("textbox");
    expect(rows[0]).toHaveValue("openai/gpt-transcribe");
    expect(rows[1]).toHaveValue("openai/whisper-large-v3");
  });

  it("applies compare state events and ignores an older run response", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    let resolveRun: ((value: unknown) => void) | undefined;
    const initial = compareState();
    const running = compareState({
      running: true,
      nonce: 3,
      runId: "run-3",
      slots: [
        {
          slotId: "slot-1",
          model: "openai/gpt-transcribe",
          status: "running",
          text: null,
          error: null,
          attempt: 1,
          cost: null,
          latencyMs: null,
          clipNonce: 3,
          runId: "run-3",
        },
      ],
    });
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "get_model_compare") return Promise.resolve(initial);
      if (command === "run_model_compare") {
        return new Promise((resolve) => {
          resolveRun = resolve;
        });
      }
      return Promise.resolve(undefined);
    });
    render(
      <ComparePane
        settings={settings()}
        copy={messagesFor("en")}
        keyConfigured
        onChange={() => undefined}
        onOpenKey={() => undefined}
      />,
    );
    await waitFor(() => expect(compareEventHarness.stateHandler).toBeTypeOf("function"));
    fireEvent.click(screen.getByRole("button", { name: "Compare" }));
    await waitFor(() => expect(resolveRun).toBeTypeOf("function"));

    act(() => compareEventHarness.stateHandler?.({ payload: running }));
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Compare" })).toBeDisabled();

    await act(async () => {
      resolveRun?.(compareState({ running: false, runId: "stale-run" }));
      await Promise.resolve();
    });
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();

    const completed = compareState({
      nonce: 3,
      runId: "run-3",
      slots: [
        {
          slotId: "slot-1",
          model: "openai/gpt-transcribe",
          status: "done",
          text: "Event transcript",
          error: null,
          attempt: 1,
          cost: 0.02,
          latencyMs: 900,
          clipNonce: 3,
          runId: "run-3",
        },
      ],
    });
    act(() => compareEventHarness.stateHandler?.({ payload: completed }));
    expect(await screen.findByText("Event transcript")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument();
    expect(screen.getByText(/cost 0.02/)).toBeInTheDocument();
  });

  it("shows initial snapshot and compare event errors to the user", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "get_model_compare") return Promise.reject("initial compare load failed");
      return Promise.resolve(undefined);
    });
    render(
      <ComparePane
        settings={settings()}
        copy={messagesFor("en")}
        keyConfigured
        onChange={() => undefined}
        onOpenKey={() => undefined}
      />,
    );
    expect(await screen.findByRole("alert")).toHaveTextContent("initial compare load failed");
    await waitFor(() => expect(compareEventHarness.errorHandler).toBeTypeOf("function"));

    act(() => compareEventHarness.errorHandler?.({ payload: "microphone disconnected" }));
    expect(screen.getByRole("alert")).toHaveTextContent("microphone disconnected");
  });

  it("keeps the record control usable after start and stop errors", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    let recording = false;
    let failStart = true;
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "get_model_compare") return compareState({ recording });
      if (command === "start_model_compare") {
        if (failStart) throw "microphone could not start";
        recording = true;
        return undefined;
      }
      if (command === "stop_model_compare") throw "microphone could not stop";
      return undefined;
    });
    render(
      <ComparePane
        settings={settings()}
        copy={messagesFor("en")}
        keyConfigured
        onChange={() => undefined}
        onOpenKey={() => undefined}
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Record clip" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("microphone could not start");
    expect(screen.getByRole("button", { name: "Record clip" })).toBeEnabled();

    failStart = false;
    fireEvent.click(screen.getByRole("button", { name: "Record clip" }));
    fireEvent.click(await screen.findByRole("button", { name: "Stop" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("microphone could not stop");
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
  });

  it("updates from cancellation success and leaves the run visible after cancellation error", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    let failCancel = false;
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "get_model_compare") return compareState({ running: true, runId: "run-1" });
      if (command === "cancel_model_compare") {
        if (failCancel) throw "cancel request failed";
        return compareState({ running: false });
      }
      return undefined;
    });
    render(
      <ComparePane
        settings={settings()}
        copy={messagesFor("en")}
        keyConfigured
        onChange={() => undefined}
        onOpenKey={() => undefined}
      />,
    );
    await waitFor(() => expect(compareEventHarness.stateHandler).toBeTypeOf("function"));
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument(),
    );

    failCancel = true;
    act(() =>
      compareEventHarness.stateHandler?.({
        payload: compareState({ running: true, runId: "run-2" }),
      }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("cancel request failed");
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();
  });

  it("stops a recording if it finishes starting after the pane unmounts", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockClear();
    let resolveStart: ((value: unknown) => void) | undefined;
    vi.mocked(invoke).mockImplementation((command: string) => {
      if (command === "get_model_compare") return Promise.resolve(compareState());
      if (command === "start_model_compare") {
        return new Promise((resolve) => {
          resolveStart = resolve;
        });
      }
      return Promise.resolve(undefined);
    });
    const { unmount } = render(
      <ComparePane
        settings={settings()}
        copy={messagesFor("en")}
        keyConfigured
        onChange={() => undefined}
        onOpenKey={() => undefined}
      />,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Record clip" }));
    await waitFor(() => expect(resolveStart).toBeTypeOf("function"));
    unmount();

    await act(async () => {
      resolveStart?.(undefined);
      await Promise.resolve();
    });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("stop_model_compare"));
    const commands = vi.mocked(invoke).mock.calls.map(([command]) => command);
    expect(commands).toContain("stop_model_compare");
    expect(commands.filter((command) => command === "stop_model_compare")).toHaveLength(1);
    expect(commands.filter((command) => command === "get_model_compare")).toHaveLength(1);
  });

  it("shows a failed compare request and accepts a later successful state", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    let failRun = true;
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "get_model_compare") return compareState();
      if (command === "run_model_compare") {
        if (failRun) throw "compare service unavailable";
        return compareState({ running: true, runId: "run-after-error" });
      }
      return undefined;
    });
    render(
      <ComparePane
        settings={settings()}
        copy={messagesFor("en")}
        keyConfigured
        onChange={() => undefined}
        onOpenKey={() => undefined}
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Compare" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("compare service unavailable");
    expect(screen.getByRole("button", { name: "Compare" })).toBeEnabled();

    failRun = false;
    fireEvent.click(screen.getByRole("button", { name: "Compare" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument());
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
