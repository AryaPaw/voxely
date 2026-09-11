import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AppSettings, DspPreset } from "../../../lib/api";
import { messagesFor } from "../../../lib/i18n";
import { FilterSettings } from "./FilterSettings";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "preview_dsp") {
      return {
        originalPath: "a.wav",
        processedPath: "b.wav",
        originalDataUrl: "data:audio/wav;base64,AA==",
        processedDataUrl: "data:audio/wav;base64,AA==",
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

afterEach(() => cleanup());

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
    activePresetId: "stt-fast",
    presets: [preset()],
    firstRunComplete: true,
  };
}

describe("FilterSettings", () => {
  it("shows factory preset labels in the active UI language", () => {
    const russianStored = settings();
    russianStored.presets = [
      { ...preset(), id: "stt-fast", name: "Быстрая диктовка" },
      { ...preset(), id: "stt-optimized", name: "Качество (медленнее)" },
    ];
    render(
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
  });

  it("edits the stored preset instead of micTune sliders", async () => {
    const onChange = vi.fn();
    render(<FilterSettings settings={settings()} copy={messagesFor("en")} onChange={onChange} />);
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
  });
});
