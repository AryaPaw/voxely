import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { api } from "../../../lib/api";
import { messagesFor } from "../../../lib/i18n";
import { AdvancedSettings } from "./AdvancedSettings";
import { AppearanceSettings } from "./AppearanceSettings";
import { GeneralSettings } from "./GeneralSettings";
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
      runtimeInfo: vi.fn(async () => ({ localBuild: true, buildDate: "2026-10-30" })),
      openLogs: vi.fn(),
      openSettingsDir: vi.fn(),
      resetSettings: vi.fn(),
    },
  };
});

let scrollIntoViewDescriptor: PropertyDescriptor | undefined;
beforeEach(() => {
  scrollIntoViewDescriptor = Object.getOwnPropertyDescriptor(Element.prototype, "scrollIntoView");
  Object.defineProperty(Element.prototype, "scrollIntoView", {
    configurable: true,
    value: vi.fn(),
  });
});
afterEach(() => {
  cleanup();
  if (scrollIntoViewDescriptor) {
    Object.defineProperty(Element.prototype, "scrollIntoView", scrollIntoViewDescriptor);
  } else {
    Reflect.deleteProperty(Element.prototype, "scrollIntoView");
  }
});

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
    textReplacements: { enabled: false, groups: [] },
    activePresetId: "stt-optimized",
    presets: [],
    firstRunComplete: true,
    uiLanguage: "en",
    autoUpdateEnabled: true,
  };
}

describe("AppearanceSettings", () => {
  it("contains visual settings without language or update behavior", () => {
    const onChange = vi.fn();
    render(
      <AppearanceSettings settings={settings()} copy={messagesFor("en")} onChange={onChange} />,
    );
    expect(screen.queryByRole("button", { name: /Check for updates/ })).not.toBeInTheDocument();
    expect(screen.getByRole("radiogroup", { name: "Theme" })).toBeInTheDocument();
    expect(screen.queryByRole("switch", { name: "Automatic updates" })).not.toBeInTheDocument();
    expect(screen.queryByRole("combobox", { name: "Interface language" })).not.toBeInTheDocument();
  });
});

describe("GeneralSettings", () => {
  it("selects themes with accessible icon radios", () => {
    const changed = vi.fn();
    render(<GeneralSettings settings={settings()} copy={messagesFor("en")} onChange={changed} />);
    const theme = screen.getByRole("radiogroup", { name: "Theme" });
    expect(within(theme).getAllByRole("radio")).toHaveLength(3);
    fireEvent.click(within(theme).getByRole("radio", { name: "Light" }));
    expect(changed).toHaveBeenCalledWith({ theme: "light" });
    expect(screen.getByRole("button", { name: "Check for updates" })).toBeInTheDocument();
  });
  it("keeps language and update changes on their existing settings keys", () => {
    const onChange = vi.fn();
    render(<GeneralSettings settings={settings()} copy={messagesFor("en")} onChange={onChange} />);
    fireEvent.click(screen.getByRole("switch", { name: "Automatic updates" }));
    expect(onChange).toHaveBeenCalledWith({ autoUpdateEnabled: false });
    fireEvent.click(screen.getByRole("combobox", { name: "Interface language" }));
    fireEvent.click(screen.getByRole("option", { name: "Русский" }));
    expect(onChange).toHaveBeenCalledWith({ uiLanguage: "ru" });
  });
});

describe("AdvancedSettings", () => {
  it("does not expose retired insertion mode choices", () => {
    render(
      <AdvancedSettings
        settings={settings()}
        copy={messagesFor("en")}
        onChange={() => undefined}
      />,
    );
    expect(screen.queryByRole("combobox", { name: "Text insertion" })).not.toBeInTheDocument();
    expect(screen.queryByText(/Unicode inserts/)).not.toBeInTheDocument();
    expect(screen.queryByText("SendInput")).not.toBeInTheDocument();
  });

  it("keeps dangerous resets out of diagnostics", () => {
    render(
      <AdvancedSettings
        settings={settings()}
        copy={messagesFor("en")}
        onChange={() => undefined}
      />,
    );
    expect(screen.getByRole("button", { name: "Open settings folder" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Reset settings" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Reset everything" })).not.toBeInTheDocument();
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
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Open settings folder" }));
    await waitFor(() => {
      expect(toast.error).toHaveBeenCalledWith("Error: Could not open the settings folder");
    });
  });
});
