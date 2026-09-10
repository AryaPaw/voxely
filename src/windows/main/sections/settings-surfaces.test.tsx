import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { messagesFor } from "../../../lib/i18n";
import { AdvancedSettings } from "./AdvancedSettings";
import { AppearanceSettings } from "./AppearanceSettings";
import type { AppSettings } from "../../../lib/api";

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
      />,
    );
    expect(screen.getByRole("combobox", { name: "Text insertion" })).toBeInTheDocument();
    expect(screen.getByText(/Unicode inserts/)).toBeInTheDocument();
    expect(screen.queryByText("SendInput")).not.toBeInTheDocument();
  });
});
