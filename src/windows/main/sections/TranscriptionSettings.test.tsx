import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import type { AppSettings } from "../../../lib/api";
import { messagesFor } from "../../../lib/i18n";
import { TranscriptionSettings } from "./TranscriptionSettings";

const invoke = vi.fn();

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
}));

vi.mock("../../../lib/system-notify", () => ({
  reportError: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => invoke(cmd, args),
  convertFileSrc: (path: string) => path,
}));

afterEach(() => {
  cleanup();
  invoke.mockReset();
});

function settings(patch: Partial<AppSettings> = {}): AppSettings {
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
    activePresetId: "stt-fast",
    presets: [],
    firstRunComplete: true,
    uiLanguage: "en",
    ...patch,
  };
}

describe("TranscriptionSettings", () => {
  it("saves a key, tests the connection, and restores empty timeout drafts", async () => {
    const onChange = vi.fn();
    const onConfigured = vi.fn();
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "discover_models") {
        return [{ id: "openai/gpt-transcribe", name: "GPT Transcribe" }];
      }
      if (cmd === "store_api_key") {
        return true;
      }
      if (cmd === "test_openrouter") {
        return 3;
      }
      return undefined;
    });
    render(
      <TranscriptionSettings
        settings={settings()}
        copy={messagesFor("en")}
        keyConfigured={false}
        onConfigured={onConfigured}
        onChange={onChange}
      />,
    );
    fireEvent.change(screen.getByPlaceholderText("sk-or-…"), { target: { value: "sk-or-test" } });
    fireEvent.click(screen.getByRole("button", { name: "Save key" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("store_api_key", { key: "sk-or-test" });
    });
    expect(onConfigured).toHaveBeenCalledWith(true);
    expect(toast.success).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Test connection" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("test_openrouter", undefined);
    });
    const extra = screen.getByDisplayValue("2");
    fireEvent.change(extra, { target: { value: "" } });
    fireEvent.blur(extra);
    expect(onChange).not.toHaveBeenCalled();
    const connect = screen.getAllByDisplayValue("8000")[0];
    fireEvent.change(connect, { target: { value: "" } });
    fireEvent.blur(connect);
    expect(screen.getAllByDisplayValue("8000")[0]).toHaveValue("8000");
    fireEvent.change(connect, { target: { value: "9000" } });
    fireEvent.blur(connect);
    expect(onChange).toHaveBeenCalledWith({
      retry: expect.objectContaining({ connectTimeoutMs: 9000 }),
    });
    const total = screen.getByDisplayValue("720000");
    fireEvent.change(total, { target: { value: "" } });
    fireEvent.blur(total);
    expect(screen.getByDisplayValue("720000")).toBeInTheDocument();
    const request = screen.getByDisplayValue("20000");
    fireEvent.change(request, { target: { value: "21000" } });
    fireEvent.blur(request);
    expect(onChange).toHaveBeenCalledWith({
      retry: expect.objectContaining({ requestTimeoutMs: 21000 }),
    });
  });

  it("rejects an empty custom model and shows a catalog error", async () => {
    const { reportError } = await import("../../../lib/system-notify");
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "discover_models") {
        throw new Error("offline");
      }
      return undefined;
    });
    const onChange = vi.fn();
    render(
      <TranscriptionSettings
        settings={settings({ model: "custom/id", customModel: "custom/id" })}
        copy={messagesFor("en")}
        keyConfigured
        onConfigured={() => undefined}
        onChange={onChange}
      />,
    );
    await waitFor(() => {
      expect(screen.getByText(/catalog is unavailable/)).toBeInTheDocument();
    });
    const custom = screen.getByDisplayValue("custom/id");
    fireEvent.change(custom, { target: { value: "   " } });
    fireEvent.blur(custom);
    expect(onChange).not.toHaveBeenCalled();
    expect(reportError).toHaveBeenCalledWith("Enter a model");
  });
});
