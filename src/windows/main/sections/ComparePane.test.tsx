import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AppSettings } from "../../../lib/api";
import { messagesFor } from "../../../lib/i18n";
import { ComparePane } from "./ComparePane";

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
  listen: vi.fn(async () => () => undefined),
}));

afterEach(() => cleanup());

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
    activePresetId: "stt-optimized",
    presets: [],
    firstRunComplete: true,
    compareModels: ["openai/gpt-transcribe", "openai/whisper-large-v3"],
  };
}

describe("ComparePane", () => {
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
  });

  it("adds a slot up to four and can set default", () => {
    const onChange = vi.fn();
    render(
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
    fireEvent.click(screen.getAllByRole("button", { name: "Set as default" })[0]);
    expect(onChange).toHaveBeenCalledWith({ model: "openai/gpt-transcribe" });
  });

  it("reloads the preview after a new take", async () => {
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
});
