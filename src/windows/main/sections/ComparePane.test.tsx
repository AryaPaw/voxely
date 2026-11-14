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
        revision: 1,
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
      return 1;
    }
    if (cmd === "stop_model_compare") {
      return {
        revision: 2,
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
    revision: 1,
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
    textReplacements: { enabled: false, groups: [] },
    activePresetId: "stt-optimized",
    presets: [],
    firstRunComplete: true,
    compareModels: ["openai/gpt-transcribe", "openai/whisper-large-v3"],
  };
}

describe("ComparePane", () => {
  it("does not let a delayed start reply stop a newer take owned by another view", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    let resolveStart!: (nonce: number) => void;
    let active: number | null = null;
    let revision = 0;
    vi.mocked(invoke).mockClear();
    vi.mocked(invoke).mockImplementation((command: string, args) => {
      if (command === "get_model_compare")
        return Promise.resolve(compareState({ revision: ++revision }));
      if (command === "start_model_compare") {
        active = 7;
        return new Promise<number>((resolve) => {
          resolveStart = resolve;
        });
      }
      if (command === "stop_model_compare") {
        if ((args as { nonce?: number } | undefined)?.nonce === active) active = null;
        return Promise.resolve(
          compareState({ revision: ++revision, nonce: active ?? 7, recording: active !== null }),
        );
      }
      return Promise.resolve(undefined);
    });
    const view = render(
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
    view.unmount();
    expect(
      vi.mocked(invoke).mock.calls.filter(([command]) => command === "stop_model_compare"),
    ).toHaveLength(0);
    // A was stopped elsewhere and B began before A's IPC reply was delivered.
    active = 8;
    await act(async () => resolveStart(7));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("stop_model_compare", { nonce: 7 }));
    expect(active).toBe(8);
    expect(
      vi.mocked(invoke).mock.calls.filter(([command]) => command === "stop_model_compare"),
    ).toHaveLength(1);
  });

  it.each([false, true])(
    "keeps cleanup bound to its owner across delayed getter and stop replies, pointerStop=%s",
    async (pointerStop) => {
      const { invoke } = await import("@tauri-apps/api/core");
      let resolveGetter!: (state: CompareState) => void;
      let resolveStop!: (state: CompareState) => void;
      let reads = 0;
      let active: number | null = null;
      vi.mocked(invoke).mockClear();
      vi.mocked(invoke).mockImplementation((command: string, args) => {
        if (command === "get_model_compare") {
          reads += 1;
          return reads === 1
            ? Promise.resolve(compareState())
            : new Promise<CompareState>((resolve) => {
                resolveGetter = resolve;
              });
        }
        if (command === "start_model_compare") {
          active = 7;
          return Promise.resolve(7);
        }
        if (command === "stop_model_compare") {
          if ((args as { nonce?: number } | undefined)?.nonce === active) active = null;
          return new Promise<CompareState>((resolve) => {
            resolveStop = resolve;
          });
        }
        return Promise.resolve(undefined);
      });
      const view = render(
        <ComparePane
          settings={settings()}
          copy={messagesFor("en")}
          keyConfigured
          onChange={() => undefined}
          onOpenKey={() => undefined}
        />,
      );
      await waitFor(() => expect(screen.getByRole("button", { name: "Compare" })).toBeEnabled());
      fireEvent.click(screen.getByRole("button", { name: "Record clip" }));
      await waitFor(() => expect(resolveGetter).toBeTypeOf("function"));
      if (pointerStop) {
        act(() =>
          compareEventHarness.stateHandler?.({
            payload: compareState({ revision: 2, nonce: 7, recording: true }),
          }),
        );
        fireEvent.click(screen.getByRole("button", { name: "Stop" }));
        await waitFor(() => expect(resolveStop).toBeTypeOf("function"));
      }
      view.unmount();
      await waitFor(() => expect(invoke).toHaveBeenCalledWith("stop_model_compare", { nonce: 7 }));
      expect(active).toBeNull();
      active = 8;
      await act(async () =>
        resolveGetter(compareState({ revision: 5, nonce: 8, recording: true })),
      );
      expect(
        vi.mocked(invoke).mock.calls.filter(([command]) => command === "stop_model_compare"),
      ).toHaveLength(1);
      await act(async () => resolveStop(compareState({ revision: 3, nonce: 7, recording: false })));
      expect(active).toBe(8);
      expect(
        vi.mocked(invoke).mock.calls.filter(([command]) => command === "stop_model_compare"),
      ).toHaveLength(1);
    },
  );

  it("keeps one pending start until its getter settles and retains the next take's owner", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    let resolveGetter!: (state: CompareState) => void;
    let reads = 0;
    let starts = 0;
    let active: number | null = null;
    let revision = 0;
    vi.mocked(invoke).mockClear();
    vi.mocked(invoke).mockImplementation((command: string, args) => {
      if (command === "get_model_compare") {
        reads += 1;
        return reads === 2
          ? new Promise<CompareState>((resolve) => {
              resolveGetter = resolve;
            })
          : Promise.resolve(
              compareState({
                revision: ++revision,
                nonce: active ?? 1,
                recording: active !== null,
              }),
            );
      }
      if (command === "start_model_compare") {
        active = 7 + starts++;
        return Promise.resolve(active);
      }
      if (command === "stop_model_compare") {
        if ((args as { nonce?: number } | undefined)?.nonce === active) active = null;
        return Promise.resolve(
          compareState({ revision: ++revision, nonce: 7, recording: active !== null }),
        );
      }
      return Promise.resolve(undefined);
    });
    const view = render(
      <ComparePane
        settings={settings()}
        copy={messagesFor("en")}
        keyConfigured
        onChange={() => undefined}
        onOpenKey={() => undefined}
      />,
    );
    await waitFor(() => expect(screen.getByRole("button", { name: "Compare" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Record clip" }));
    await waitFor(() => expect(resolveGetter).toBeTypeOf("function"));
    act(() =>
      compareEventHarness.stateHandler?.({
        payload: compareState({ revision: ++revision, nonce: 7, recording: true }),
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    await screen.findByRole("button", { name: "Record clip" });
    fireEvent.click(screen.getByRole("button", { name: "Record clip" }));
    expect(starts).toBe(1);
    await act(async () => resolveGetter(compareState({ revision: 2, nonce: 7, recording: true })));
    fireEvent.click(screen.getByRole("button", { name: "Record clip" }));
    await screen.findByRole("button", { name: "Stop" });
    expect(starts).toBe(2);
    expect(active).toBe(8);
    view.unmount();
    await waitFor(() => expect(active).toBeNull());
    expect(invoke).toHaveBeenCalledWith("stop_model_compare", { nonce: 8 });
  });

  it.each([false, true])(
    "pauses late-mounted preview audio on unmount, replaced=%s",
    async (replace) => {
      const { invoke } = await import("@tauri-apps/api/core");
      let resolveInitial!: (state: CompareState) => void;
      vi.mocked(invoke).mockImplementation((command: string) =>
        command === "get_model_compare"
          ? new Promise<CompareState>((resolve) => {
              resolveInitial = resolve;
            })
          : Promise.resolve(undefined),
      );
      const pause = vi.spyOn(HTMLAudioElement.prototype, "pause");
      const view = render(
        <ComparePane
          settings={settings()}
          copy={messagesFor("en")}
          keyConfigured
          onChange={() => undefined}
          onOpenKey={() => undefined}
        />,
      );
      try {
        expect(view.container.querySelector("audio")).toBeNull();
        await waitFor(() => expect(resolveInitial).toBeTypeOf("function"));
        await act(async () => resolveInitial(compareState()));
        const first = view.container.querySelector("audio");
        expect(first).not.toBeNull();
        let current = first;
        if (replace) {
          act(() =>
            compareEventHarness.stateHandler?.({
              payload: compareState({ revision: 2, nonce: 2, listenPath: "second.wav" }),
            }),
          );
          current = view.container.querySelector("audio");
          expect(current).not.toBe(first);
          expect(pause.mock.contexts.filter((node) => node === first)).toHaveLength(1);
        }
        expect(pause.mock.contexts.filter((node) => node === current)).toHaveLength(0);
        view.unmount();
        expect(pause.mock.contexts.filter((node) => node === current)).toHaveLength(1);
      } finally {
        view.unmount();
        pause.mockRestore();
      }
    },
  );

  it("accepts a newer command snapshot after an earlier progress event", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    let resolveRun!: (state: CompareState) => void;
    const pending = new Promise<CompareState>((resolve) => {
      resolveRun = resolve;
    });
    vi.mocked(invoke).mockImplementation((command: string) =>
      command === "get_model_compare"
        ? Promise.resolve(compareState())
        : command === "run_model_compare"
          ? pending
          : Promise.resolve(undefined),
    );
    render(
      <ComparePane
        settings={settings()}
        copy={messagesFor("en")}
        keyConfigured
        onChange={() => undefined}
        onOpenKey={() => undefined}
      />,
    );
    await waitFor(() => expect(screen.getByRole("button", { name: "Compare" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Compare" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("run_model_compare"));
    act(() =>
      compareEventHarness.stateHandler?.({ payload: compareState({ revision: 2, running: true }) }),
    );
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();
    await act(async () =>
      resolveRun(compareState({ revision: 3, running: false, listenPath: "latest.wav" })),
    );
    expect(screen.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument();
    expect(document.querySelector("audio")).toHaveAttribute("src", "latest.wav?n=1");
  });

  it("keeps state subscriptions alive when only the initial snapshot fails", async () => {
    const { listen } = await import("@tauri-apps/api/event");
    const { invoke } = await import("@tauri-apps/api/core");
    const originalListen = vi.mocked(listen).getMockImplementation()!;
    const originalInvoke = vi.mocked(invoke).getMockImplementation()!;
    const unsubscribe = vi.fn();
    vi.mocked(listen).mockImplementation(async (event, handler, options) => {
      await originalListen(event, handler, options);
      return unsubscribe;
    });
    vi.mocked(invoke).mockImplementation((command) =>
      command === "get_model_compare"
        ? Promise.reject(new Error("initial snapshot failed"))
        : Promise.resolve(undefined),
    );
    try {
      const view = render(
        <ComparePane
          settings={settings()}
          copy={messagesFor("en")}
          keyConfigured
          onChange={() => undefined}
          onOpenKey={() => undefined}
        />,
      );
      expect(await screen.findByRole("alert")).toHaveTextContent("initial snapshot failed");
      expect(unsubscribe).not.toHaveBeenCalled();
      act(() =>
        compareEventHarness.stateHandler?.({
          payload: compareState({ revision: 2, running: true }),
        }),
      );
      expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();
      view.unmount();
      expect(unsubscribe).toHaveBeenCalledTimes(2);
    } finally {
      vi.mocked(listen).mockImplementation(originalListen);
      vi.mocked(invoke).mockImplementation(originalInvoke);
    }
  });

  it("releases a partial compare subscription when the other listener fails", async () => {
    const { listen } = await import("@tauri-apps/api/event");
    const original = vi.mocked(listen).getMockImplementation()!;
    const unsubscribe = vi.fn();
    vi.mocked(listen).mockImplementation((event) =>
      event === "compare://state"
        ? Promise.resolve(unsubscribe)
        : Promise.reject(new Error("subscription failed")),
    );
    try {
      const view = render(
        <ComparePane
          settings={settings()}
          copy={messagesFor("en")}
          keyConfigured
          onChange={() => undefined}
          onOpenKey={() => undefined}
        />,
      );
      expect(await screen.findByRole("alert")).toHaveTextContent("subscription failed");
      view.unmount();
      expect(unsubscribe).toHaveBeenCalledTimes(1);
    } finally {
      vi.mocked(listen).mockImplementation(original);
    }
  });

  it("keeps the terminal snapshot when older compare events arrive after it", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const initial = { ...compareState(), revision: 10 };
    vi.mocked(invoke).mockImplementation((command: string) =>
      Promise.resolve(command === "get_model_compare" ? initial : undefined),
    );
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
    const progress = { ...compareState({ nonce: 2, runId: "run-2", running: true }), revision: 11 };
    const terminal = { ...progress, running: false, revision: 12 };
    act(() => compareEventHarness.stateHandler?.({ payload: terminal }));
    expect(screen.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument();
    act(() => compareEventHarness.stateHandler?.({ payload: progress }));
    expect(screen.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument();
    act(() =>
      compareEventHarness.stateHandler?.({ payload: { ...progress, revision: terminal.revision } }),
    );
    expect(screen.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument();
    act(() => compareEventHarness.stateHandler?.({ payload: initial }));
    expect(document.querySelector("audio")).toHaveAttribute("src", "C:/tmp/compare.wav?n=2");
    expect(screen.getByRole("button", { name: "Compare" })).toBeEnabled();
  });

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
      revision: 2,
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
      revision: 1,
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
    let revision = 0;
    let nonce = 1;
    vi.mocked(invoke).mockImplementation(async (cmd: string, args) => {
      if (cmd === "start_model_compare") {
        recording = true;
        nonce += 1;
        return nonce;
      }
      if (
        cmd === "stop_model_compare" &&
        (args as { nonce?: number } | undefined)?.nonce === nonce
      ) {
        recording = false;
      }
      return {
        revision: ++revision,
        recording,
        running: false,
        nonce,
        listenPath: recording
          ? null
          : nonce === 2
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
          revision: 1,
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
      revision: 2,
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
      revision: 3,
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
    let revision = 0;
    let failStart = true;
    vi.mocked(invoke).mockImplementation(async (command: string) => {
      if (command === "get_model_compare") return compareState({ recording, revision: ++revision });
      if (command === "start_model_compare") {
        if (failStart) throw "microphone could not start";
        recording = true;
        return 1;
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
        return compareState({ running: false, revision: 2 });
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
        payload: compareState({ running: true, runId: "run-2", revision: 3 }),
      }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("cancel request failed");
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument();
  });

  it("stops the admitted owner after unmount during pending compare start", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockClear();
    let resolveStart: ((value: number) => void) | undefined;
    let active: number | null = null;
    let revision = 0;
    vi.mocked(invoke).mockImplementation((command: string, args) => {
      if (command === "get_model_compare") return Promise.resolve(compareState());
      if (command === "start_model_compare") {
        return new Promise<number>((resolve) => {
          resolveStart = resolve;
        }).then((nonce) => {
          active = nonce;
          return nonce;
        });
      }
      if (command === "stop_model_compare") {
        if (active === null) return Promise.reject(new Error("compare not started"));
        if ((args as { nonce?: number } | undefined)?.nonce === active) active = null;
        return Promise.resolve(compareState({ revision: ++revision, recording: active !== null }));
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
    fireEvent.click(screen.getByRole("button", { name: "Record clip" }));
    expect(
      vi.mocked(invoke).mock.calls.filter(([command]) => command === "start_model_compare"),
    ).toHaveLength(1);
    unmount();
    expect(
      vi.mocked(invoke).mock.calls.filter(([command]) => command === "stop_model_compare"),
    ).toHaveLength(0);

    await act(async () => {
      resolveStart?.(7);
      await Promise.resolve();
    });
    await waitFor(() => expect(active).toBeNull());
    expect(invoke).toHaveBeenCalledWith("stop_model_compare", { nonce: 7 });
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
        return compareState({ running: true, runId: "run-after-error", revision: 2 });
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
