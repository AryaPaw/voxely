import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import type { AppSettings } from "../../../lib/api";
import { messagesFor } from "../../../lib/i18n";
import { AppearanceSettings } from "./AppearanceSettings";

const invoke = vi.fn();

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

vi.mock("../../../lib/system-notify", () => ({
  reportError: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => invoke(cmd, args),
}));

afterEach(() => {
  cleanup();
  invoke.mockReset();
});

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
    activePresetId: "stt-fast",
    presets: [],
    firstRunComplete: true,
    uiLanguage: "en",
  };
}

describe("AppearanceSettings", () => {
  it("resets the main window to the default size", async () => {
    invoke.mockResolvedValue(undefined);
    render(
      <AppearanceSettings
        settings={settings()}
        copy={messagesFor("en")}
        onChange={() => undefined}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Reset window size" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("reset_main_window", undefined);
    });
    expect(toast.success).toHaveBeenCalledWith("Window size reset to the default.");
  });
});
