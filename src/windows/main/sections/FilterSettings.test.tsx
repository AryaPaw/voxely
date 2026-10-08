import { useState, type ReactElement } from "react";
import { TooltipProvider } from "../../../components/ui/tooltip";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AppSettings, DspPreset, DspPreview } from "../../../lib/api";
import { api } from "../../../lib/api";
import { localizedError, messagesFor } from "../../../lib/i18n";
import { FilterSettings } from "./FilterSettings";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "preview_dsp") {
      return {
        originalPath: "a.wav",
        processedPath: "b.wav",
        peak: 0.5,
        rms: 0.1,
        clipCount: 0,
        nonce: 1,
      };
    }
    return [];
  }),
  convertFileSrc: (path: string) => path,
}));

const filterSampleHandlers: Array<(event: { payload: boolean }) => void> = [];

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (event: string, handler: (event: { payload: boolean }) => void) => {
    if (event === "filter://sample") {
      filterSampleHandlers.push(handler);
    }
    return () => undefined;
  }),
}));

afterEach(() => {
  cleanup();
  filterSampleHandlers.length = 0;
  vi.restoreAllMocks();
});

function preset(): DspPreset {
  return {
    id: "stt-fast",
    name: "Fast",
    order: [
      { id: "highpass", kind: "highPass", enabled: true },
      { id: "gain", kind: "gain", enabled: true },
      { id: "limiter", kind: "limiter", enabled: true },
    ],
    highPass: { cutoffHz: 80, sampleRate: 48000 },
    gain: { db: 1.5 },
    compressor: {
      thresholdDb: -18,
      ratio: 3,
      attackMs: 6,
      releaseMs: 120,
      makeupDb: 3,
      sampleRate: 48000,
    },
    expander: {
      thresholdDb: -45,
      ratio: 2,
      attackMs: 8,
      releaseMs: 80,
      makeupDb: 0,
      sampleRate: 48000,
    },
    gate: {
      openThresholdDb: -48,
      closeThresholdDb: -52,
      holdMs: 250,
      releaseMs: 200,
      sampleRate: 48000,
    },
    limiter: { thresholdDb: -1, releaseMs: 40, sampleRate: 48000 },
    rnnoiseMix: 1,
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
    presets: [preset()],
    firstRunComplete: true,
  };
}

function renderAudio(ui: ReactElement) {
  const result = render(<TooltipProvider>{ui}</TooltipProvider>);
  fireEvent.mouseDown(screen.getByRole("tab", { name: "Audio" }), { button: 0 });
  return result;
}

function ReplacementHarness({
  initial = settings(),
  onChange,
}: {
  initial?: AppSettings;
  onChange?: (patch: Partial<AppSettings>) => void;
}) {
  const [value, setValue] = useState(initial);
  return (
    <TooltipProvider>
      <FilterSettings
        settings={value}
        copy={messagesFor("en")}
        onChange={(patch) => {
          onChange?.(patch);
          setValue((previous) => ({ ...previous, ...structuredClone(patch) }));
        }}
      />
    </TooltipProvider>
  );
}

describe("FilterSettings", () => {
  it("keeps sample controls usable after start failure", async () => {
    const copy = messagesFor("en");
    let resolvePreview!: (preview: DspPreview) => void;
    const start = vi
      .spyOn(api, "startFilterSample")
      .mockRejectedValue({ code: "AudioCaptureFailed" });
    const preview = vi
      .spyOn(api, "previewDsp")
      .mockImplementation(() => new Promise((resolve) => (resolvePreview = resolve)));
    renderAudio(<FilterSettings settings={settings()} copy={copy} onChange={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: copy.recordSample }));
    await waitFor(() => expect(start).toHaveBeenCalledOnce());
    expect(await screen.findByRole("alert")).toHaveTextContent(
      localizedError("AudioCaptureFailed", copy),
    );
    await waitFor(() => expect(preview).toHaveBeenCalledOnce());
    resolvePreview({
      originalPath: "original.wav",
      processedPath: "processed.wav",
      peak: 0.2,
      rms: 0.1,
      clipCount: 0,
      nonce: 1,
    });
    expect(await screen.findByRole("button", { name: copy.playOriginal })).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent(localizedError("AudioCaptureFailed", copy));
    expect(screen.getByRole("button", { name: copy.recordSample })).toBeEnabled();
    expect(screen.queryByText(copy.recordingSample)).not.toBeInTheDocument();
  });

  it("reports stop failure and releases the busy button so stop can be retried", async () => {
    vi.spyOn(api, "startFilterSample").mockResolvedValue(undefined);
    const stop = vi.spyOn(api, "stopFilterSample").mockRejectedValue({ code: "StorageFailed" });
    vi.spyOn(api, "meter").mockResolvedValue({ peak: 0.1, rms: 0.05, levels: [] });
    const copy = messagesFor("en");
    renderAudio(<FilterSettings settings={settings()} copy={copy} onChange={vi.fn()} />);
    const sampleButton = screen.getByRole("button", { name: copy.recordSample });
    fireEvent.click(sampleButton);
    await waitFor(() => {
      expect(sampleButton).toHaveAccessibleName(copy.stopSample);
      expect(sampleButton).toBeEnabled();
    });
    fireEvent.click(sampleButton);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      localizedError("StorageFailed", copy),
    );
    expect(sampleButton).toHaveAccessibleName(copy.stopSample);
    expect(sampleButton).toBeEnabled();
    expect(stop).toHaveBeenCalledOnce();
    fireEvent.click(sampleButton);
    await waitFor(() => expect(stop).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(sampleButton).toBeEnabled());
  });

  it("localizes a missing preview sample and does not expose playback controls", async () => {
    vi.spyOn(api, "previewDsp").mockRejectedValue("no filter sample");
    const copy = messagesFor("en");
    renderAudio(<FilterSettings settings={settings()} copy={copy} onChange={vi.fn()} />);
    expect(await screen.findByRole("alert")).toHaveTextContent(copy.noFilterSample);
    expect(screen.queryByRole("button", { name: copy.playOriginal })).not.toBeInTheDocument();
  });

  it("handles rejected playback and resets both players when stopped", async () => {
    const play = vi
      .spyOn(HTMLAudioElement.prototype, "play")
      .mockRejectedValue(new Error("blocked"));
    const pause = vi.spyOn(HTMLAudioElement.prototype, "pause").mockImplementation(() => undefined);
    const copy = messagesFor("en");
    const { container } = renderAudio(
      <FilterSettings settings={settings()} copy={copy} onChange={vi.fn()} />,
    );
    fireEvent.click(await screen.findByRole("button", { name: copy.playOriginal }));
    expect(await screen.findByText(copy.playbackFailed)).toBeInTheDocument();
    expect(play).toHaveBeenCalledOnce();
    const clips = container.querySelectorAll("audio");
    clips.forEach((clip) => {
      clip.currentTime = 4;
    });
    fireEvent.click(screen.getByRole("button", { name: copy.stopListen }));
    clips.forEach((clip) => expect(clip.currentTime).toBe(0));
    expect(pause).toHaveBeenCalled();
  });

  it("shows factory preset labels in the active UI language", () => {
    const russianStored = settings();
    russianStored.presets = [
      { ...preset(), id: "stt-fast", name: "Быстрая диктовка" },
      { ...preset(), id: "stt-optimized", name: "Качество (медленнее)" },
    ];
    renderAudio(
      <FilterSettings
        settings={russianStored}
        copy={messagesFor("en")}
        onChange={() => undefined}
      />,
    );
    expect(screen.getByRole("combobox", { name: "Active preset" })).toHaveTextContent(
      "Fast dictation",
    );
    expect(screen.queryByText("Быстрая диктовка")).not.toBeInTheDocument();
    expect(screen.queryByText("Качество (медленнее)")).not.toBeInTheDocument();
    expect(screen.queryByText(/OBS/)).not.toBeInTheDocument();
  });

  it("edits the stored preset instead of micTune sliders", async () => {
    const onChange = vi.fn();
    renderAudio(
      <FilterSettings settings={settings()} copy={messagesFor("en")} onChange={onChange} />,
    );
    expect(screen.getByRole("slider", { name: /Gain 1.5 dB/ })).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Noise reduction" })).toHaveAttribute(
      "aria-checked",
      "false",
    );
    fireEvent.click(screen.getByRole("switch", { name: "Noise reduction" }));
    fireEvent.click(screen.getByRole("switch", { name: "Compressor" }));
    fireEvent.click(screen.getByRole("switch", { name: "Expander" }));
    fireEvent.click(screen.getByRole("switch", { name: "Gate" }));
    fireEvent.click(screen.getByRole("switch", { name: "Limiter" }));
    expect(onChange).toHaveBeenCalled();
    await waitFor(
      () => {
        expect(screen.getByRole("button", { name: "Original" })).toBeInTheDocument();
      },
      { timeout: 1500 },
    );
    fireEvent.click(screen.getByRole("button", { name: "Original" }));
    fireEvent.click(screen.getByRole("button", { name: "After filters" }));
    const clips = document.querySelectorAll("audio");
    expect(clips).toHaveLength(2);
    expect(clips[0]).toHaveAttribute("src", "a.wav?n=1");
    expect(clips[1]).toHaveAttribute("src", "b.wav?n=1");
  });

  it("follows filter sample events from the backend", async () => {
    renderAudio(
      <FilterSettings settings={settings()} copy={messagesFor("en")} onChange={() => undefined} />,
    );
    await waitFor(() => {
      expect(filterSampleHandlers.length).toBeGreaterThan(0);
    });
    filterSampleHandlers[0]({ payload: true });
    expect(await screen.findByRole("button", { name: "Stop" })).toBeInTheDocument();
  });
});

describe("compact replacement editor", () => {
  function sortableRules() {
    const initial = settings();
    initial.textReplacements.rules = [
      { from: "alpha", to: "A", caseSensitive: false },
      { from: "beta", to: "B", caseSensitive: true },
      { from: "gamma", to: "C", caseSensitive: false },
    ];
    const originalRect = HTMLElement.prototype.getBoundingClientRect;
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
      this: HTMLElement,
    ) {
      if (this.tagName !== "LI") return originalRect.call(this);
      const index = Array.from(this.parentElement!.children).indexOf(this);
      return new DOMRect(0, 100 + index * 40, 600, 32);
    });
    return initial;
  }

  async function pickUpFirstRule() {
    const handle = screen.getByRole("button", {
      name: messagesFor("en").textReplacementDrag.replace("{index}", "1"),
    });
    handle.focus();
    fireEvent.keyDown(handle, { key: " ", code: "Space" });
    await waitFor(() => expect(handle).toHaveAttribute("aria-pressed", "true"));
    // KeyboardSensor attaches its document listener on the next task
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    return handle;
  }

  it("reorders complete saved rules through the keyboard sensor and preserves row identity", async () => {
    const initial = sortableRules();
    const onChange = vi.fn();
    render(<ReplacementHarness initial={initial} onChange={onChange} />);
    const firstInput = screen.getByRole("textbox", { name: "Find 1" });
    const handle = await pickUpFirstRule();
    fireEvent.keyDown(document, { key: "ArrowDown", code: "ArrowDown" });
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("2"));
    fireEvent.keyDown(document, { key: " ", code: "Space" });
    await waitFor(() =>
      expect(screen.getByRole("textbox", { name: "Find 1" })).toHaveValue("beta"),
    );
    expect(onChange).toHaveBeenCalledOnce();
    expect(onChange).toHaveBeenLastCalledWith({
      textReplacements: {
        ...initial.textReplacements,
        rules: [
          initial.textReplacements.rules[1],
          initial.textReplacements.rules[0],
          initial.textReplacements.rules[2],
        ],
      },
    });
    expect(screen.getByRole("textbox", { name: "Find 2" })).toBe(firstInput);
    expect(
      screen.getByRole("button", {
        name: messagesFor("en").textReplacementDrag.replace("{index}", "2"),
      }),
    ).toBe(handle);
    fireEvent.change(firstInput, { target: { value: "moved alpha" } });
    expect(screen.getByRole("textbox", { name: "Find 1" })).toHaveValue("beta");
    expect(screen.getByRole("textbox", { name: "Find 2" })).toHaveValue("moved alpha");
    expect(screen.getByRole("button", { name: "Match case, 1" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("cancels a moved drag with Escape without saving or changing the rule order", async () => {
    const initial = sortableRules();
    const onChange = vi.fn();
    render(<ReplacementHarness initial={initial} onChange={onChange} />);
    const firstInput = screen.getByRole("textbox", { name: "Find 1" });
    const handle = await pickUpFirstRule();
    fireEvent.keyDown(document, { key: "ArrowDown", code: "ArrowDown" });
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("2"));
    fireEvent.keyDown(document, { key: "Escape", code: "Escape" });
    await waitFor(() => expect(handle).not.toHaveAttribute("aria-pressed", "true"));
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByRole("textbox", { name: "Find 1" })).toBe(firstInput);
    expect(firstInput).toHaveValue("alpha");
    expect(screen.getByRole("textbox", { name: "Find 2" })).toHaveValue("beta");
    expect(screen.getByRole("textbox", { name: "Find 3" })).toHaveValue("gamma");
    expect(screen.getByRole("status")).toHaveTextContent(
      messagesFor("en").textReplacementDragCancelled,
    );
  });

  it("opens word rules without starting audio preview", () => {
    const preview = vi.spyOn(api, "previewDsp");
    render(<ReplacementHarness />);
    expect(screen.getByRole("tab", { name: "Word replacement" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.queryByRole("button", { name: "Record sample" })).not.toBeInTheDocument();
    expect(preview).not.toHaveBeenCalled();
  });

  it("adds only missing preset rules and keeps custom replacements", () => {
    const initial = settings();
    initial.textReplacements.rules = [{ from: "вы", to: "CUSTOM", caseSensitive: false }];
    render(<ReplacementHarness initial={initial} />);
    fireEvent.click(
      screen.getByRole("button", { name: messagesFor("en").textReplacementRussianPreset }),
    );
    expect(screen.getAllByRole("listitem")).toHaveLength(16);
    expect(screen.getByRole("textbox", { name: "Replace with 1" })).toHaveValue("CUSTOM");
    expect(screen.getByRole("switch", { name: "Enable word replacement" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(
      screen.getByRole("button", { name: messagesFor("en").textReplacementRussianPreset }),
    ).toBeDisabled();
  });

  it("preserves input identity and focus through saved-object edits and deletion of a preceding rule", () => {
    const initial = settings();
    initial.textReplacements.rules = [
      { from: "alpha", to: "A", caseSensitive: false },
      { from: "beta", to: "B", caseSensitive: false },
    ];
    render(<ReplacementHarness initial={initial} />);
    const secondInput = screen.getByRole("textbox", { name: "Find 2" });
    secondInput.focus();
    fireEvent.change(secondInput, { target: { value: "betatron" } });
    expect(screen.getByRole("textbox", { name: "Find 2" })).toBe(secondInput);
    expect(secondInput).toHaveFocus();
    fireEvent.click(screen.getByRole("button", { name: "Remove rule 1" }));
    expect(screen.getByRole("textbox", { name: "Find 1" })).toBe(secondInput);
    expect(secondInput).toHaveValue("betatron");
    expect(secondInput).toHaveFocus();
    fireEvent.click(screen.getByRole("button", { name: "Match case, 1" }));
    expect(screen.getByRole("button", { name: "Match case, 1" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("stops a sample whose start resolves after leaving the audio tab", async () => {
    let resolveStart!: () => void;
    vi.spyOn(api, "startFilterSample").mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveStart = resolve;
        }),
    );
    const stop = vi.spyOn(api, "stopFilterSample").mockResolvedValue({
      originalPath: "a.wav",
      processedPath: "b.wav",
      peak: 0,
      rms: 0,
      clipCount: 0,
      nonce: 1,
    });
    renderAudio(
      <FilterSettings settings={settings()} copy={messagesFor("en")} onChange={vi.fn()} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Record sample" }));
    await waitFor(() => expect(api.startFilterSample).toHaveBeenCalledOnce());
    fireEvent.mouseDown(screen.getByRole("tab", { name: "Word replacement" }), { button: 0 });
    expect(stop).not.toHaveBeenCalled();
    resolveStart();
    await waitFor(() => expect(stop).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("button", { name: "Stop" })).not.toBeInTheDocument();
  });

  it("doesn't issue an idle cleanup stop when reopening the audio tab", async () => {
    const start = vi.spyOn(api, "startFilterSample").mockResolvedValue(undefined);
    const stop = vi.spyOn(api, "stopFilterSample");
    renderAudio(
      <FilterSettings settings={settings()} copy={messagesFor("en")} onChange={vi.fn()} />,
    );
    fireEvent.mouseDown(screen.getByRole("tab", { name: "Word replacement" }), { button: 0 });
    fireEvent.mouseDown(screen.getByRole("tab", { name: "Audio" }), { button: 0 });
    fireEvent.click(screen.getByRole("button", { name: "Record sample" }));
    expect(await screen.findByRole("button", { name: "Stop" })).toBeInTheDocument();
    expect(start).toHaveBeenCalledOnce();
    expect(stop).not.toHaveBeenCalled();
  });

  it("waits for an owned cleanup stop before starting a sample in the reopened tab", async () => {
    const preview = {
      originalPath: "a.wav",
      processedPath: "b.wav",
      peak: 0,
      rms: 0,
      clipCount: 0,
      nonce: 1,
    };
    let resolveStop!: () => void;
    const start = vi.spyOn(api, "startFilterSample").mockResolvedValue(undefined);
    const stop = vi
      .spyOn(api, "stopFilterSample")
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            resolveStop = () => {
              filterSampleHandlers.forEach((handler) => handler({ payload: false }));
              resolve(preview);
            };
          }),
      )
      .mockResolvedValue(preview);
    renderAudio(
      <FilterSettings settings={settings()} copy={messagesFor("en")} onChange={vi.fn()} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Record sample" }));
    await screen.findByRole("button", { name: "Stop" });
    fireEvent.mouseDown(screen.getByRole("tab", { name: "Word replacement" }), { button: 0 });
    await waitFor(() => expect(stop).toHaveBeenCalledOnce());
    fireEvent.mouseDown(screen.getByRole("tab", { name: "Audio" }), { button: 0 });
    fireEvent.click(screen.getByRole("button", { name: "Record sample" }));
    expect(start).toHaveBeenCalledTimes(1);
    resolveStop();
    await waitFor(() => expect(start).toHaveBeenCalledTimes(2));
    expect(await screen.findByRole("button", { name: "Stop" })).toBeInTheDocument();
  });
});
