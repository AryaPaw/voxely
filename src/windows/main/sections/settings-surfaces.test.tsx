import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { api } from "../../../lib/api";
import { messagesFor } from "../../../lib/i18n";
import { AdvancedSettings } from "./AdvancedSettings";
import { AppearanceSettings } from "./AppearanceSettings";
import type { AppSettings } from "../../../lib/api";

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
  },
}));

vi.mock("../../../lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../lib/api")>();
  return {
    ...actual,
    api: {
      ...actual.api,
      openLogs: vi.fn(),
      openSettingsDir: vi.fn(),
      resetSettings: vi.fn(),
    },
  };
});

afterEach(() => cleanup());

function settings(): AppSettings {
  return {
    hotkey: "Ctrl+Shift+Space",
    startWithWindows: false,
    closeToTray: true,
    notifications: true,
    inputDevice: "default",
    keepOriginalRecordings: true,
    theme: "system",
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
    autoUpdateEnabled: true,
  };
}

describe("AppearanceSettings", () => {
  it("does not host the manual update action", () => {
    render(
      <AppearanceSettings
        settings={settings()}
        copy={messagesFor("en")}
        onChange={() => undefined}
      />,
    );
    expect(screen.queryByRole("button", { name: /Check for updates/ })).not.toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Theme" })).toBeInTheDocument();
  });
});

describe("AdvancedSettings", () => {
  it("offers only unicode and clipboard insertion", () => {
    render(
      <AdvancedSettings
        settings={settings()}
        copy={messagesFor("en")}
        onChange={() => undefined}
        onSettingsReplaced={() => undefined}
      />,
    );
    expect(screen.getByRole("combobox", { name: "Text insertion" })).toBeInTheDocument();
    expect(screen.getByText(/Unicode inserts/)).toBeInTheDocument();
    expect(screen.queryByText("SendInput")).not.toBeInTheDocument();
  });

  it("offers settings folder and two reset actions", () => {
    render(
      <AdvancedSettings
        settings={settings()}
        copy={messagesFor("en")}
        onChange={() => undefined}
        onSettingsReplaced={() => undefined}
      />,
    );
    expect(screen.getByRole("button", { name: "Open settings folder" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Reset settings" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Reset everything" })).toBeInTheDocument();
  });

  it("prefixes folder-open failures as errors", async () => {
    vi.mocked(api.openSettingsDir).mockRejectedValueOnce({
      code: "StorageFailed",
      message: "opener failed",
    });
    render(
      <AdvancedSettings
        settings={settings()}
        copy={messagesFor("en")}
        onChange={() => undefined}
        onSettingsReplaced={() => undefined}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Open settings folder" }));
    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith("Error: Could not open the settings folder");
    });
  });
});
