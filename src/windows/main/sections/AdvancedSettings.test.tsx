import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { api, type AppSettings } from "../../../lib/api";
import { messagesFor } from "../../../lib/i18n";
import { reportError } from "../../../lib/system-notify";
import { AdvancedSettings } from "./AdvancedSettings";
vi.mock("sonner", () => ({ toast: { success: vi.fn() } }));
vi.mock("../../../lib/system-notify", () => ({ reportError: vi.fn() }));
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.clearAllMocks();
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
    textReplacements: { enabled: false, rules: [] },
    activePresetId: "stt-optimized",
    presets: [],
    firstRunComplete: true,
    uiLanguage: "en",
    autoUpdateEnabled: true,
  };
}

describe("AdvancedSettings failure paths", () => {
  it.each([false, true])(
    "confirms reset with wipeApiKey=%s and publishes the returned settings",
    async (wipe) => {
      const copy = messagesFor("en");
      const next = settings();
      const reset = vi.spyOn(api, "resetSettings").mockResolvedValue(next);
      const replaced = vi.fn();
      render(
        <AdvancedSettings
          settings={settings()}
          copy={copy}
          onChange={vi.fn()}
          onSettingsReplaced={replaced}
        />,
      );
      const label = wipe ? copy.resetAll : copy.resetSettings;
      fireEvent.click(screen.getByRole("button", { name: label }));
      expect(reset).not.toHaveBeenCalled();
      fireEvent.click(
        within(await screen.findByRole("alertdialog")).getByRole("button", { name: label }),
      );
      await waitFor(() => expect(replaced).toHaveBeenCalledWith(next, wipe));
      expect(reset).toHaveBeenCalledWith(wipe);
      expect(toast.success).toHaveBeenCalledOnce();
    },
  );
  it("reports reset failure and never replaces settings", async () => {
    const copy = messagesFor("en");
    vi.spyOn(api, "resetSettings").mockRejectedValue({ code: "StorageFailed" });
    const replaced = vi.fn();
    render(
      <AdvancedSettings
        settings={settings()}
        copy={copy}
        onChange={vi.fn()}
        onSettingsReplaced={replaced}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: copy.resetAll }));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", { name: copy.resetAll }),
    );
    await waitFor(() => expect(reportError).toHaveBeenCalledOnce());
    expect(replaced).not.toHaveBeenCalled();
    expect(toast.success).not.toHaveBeenCalled();
  });
  it.each(["logs", "settings"] as const)("reports failure opening %s directory", async (kind) => {
    const copy = messagesFor("en");
    vi.spyOn(api, kind === "logs" ? "openLogs" : "openSettingsDir").mockRejectedValue(
      new Error("denied"),
    );
    render(
      <AdvancedSettings
        settings={settings()}
        copy={copy}
        onChange={vi.fn()}
        onSettingsReplaced={vi.fn()}
      />,
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: kind === "logs" ? copy.openLogs : copy.openSettingsFolder,
      }),
    );
    await waitFor(() =>
      expect(reportError).toHaveBeenCalledWith(
        expect.stringContaining(kind === "logs" ? copy.openLogsFailed : copy.openSettingsFailed),
      ),
    );
  });
  it("canceling reset does not call IPC and debug toggle emits a patch", async () => {
    const copy = messagesFor("en");
    const reset = vi.spyOn(api, "resetSettings");
    const changed = vi.fn();
    render(
      <AdvancedSettings
        settings={settings()}
        copy={copy}
        onChange={changed}
        onSettingsReplaced={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("switch", { name: copy.debugLogs }));
    expect(changed).toHaveBeenCalledWith({ debugLogging: true });
    fireEvent.click(screen.getByRole("button", { name: copy.resetSettings }));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", { name: copy.cancel }),
    );
    expect(reset).not.toHaveBeenCalled();
  });
});
