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

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
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
              status: "ok",
              text: "whisper text",
              error: null,
              attempt: 1,
              cost: 0.01,
              latencyMs: 10,
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
});
