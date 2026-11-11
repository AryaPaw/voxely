import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { api, type AppSettings } from "../../../lib/api";
import { messagesFor } from "../../../lib/i18n";
import { reportError } from "../../../lib/system-notify";
import { SettingsTransfer } from "./SettingsTransfer";
import { SettingsReset } from "./SettingsReset";
import { AdvancedSettings } from "./AdvancedSettings";
vi.mock("sonner", () => ({ toast: { success: vi.fn() } }));
vi.mock("../../../lib/system-notify", () => ({ reportError: vi.fn() }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: vi.fn(async () => "0.5.0") }));
beforeEach(() => {
  vi.spyOn(api, "runtimeInfo").mockResolvedValue({ localBuild: true, buildDate: "2026-10-30" });
});
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
    textReplacements: { enabled: false, groups: [] },
    activePresetId: "stt-optimized",
    presets: [],
    firstRunComplete: true,
    uiLanguage: "en",
    autoUpdateEnabled: true,
  };
}

describe("AdvancedSettings failure paths", () => {
  function renderTransfer(replaced = vi.fn()) {
    render(<SettingsTransfer copy={messagesFor("en")} onImported={replaced} />);
    return replaced;
  }

  it("exports and reports success only when a file is saved", async () => {
    const save = vi
      .spyOn(api, "exportSettingsFile")
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true);
    renderTransfer();
    fireEvent.click(screen.getByRole("button", { name: "Export" }));
    await waitFor(() => expect(save).toHaveBeenCalledOnce());
    await waitFor(() => expect(screen.getByRole("button", { name: "Export" })).toBeEnabled());
    expect(toast.success).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Export" }));
    await waitFor(() =>
      expect(toast.success).toHaveBeenCalledWith(messagesFor("en").settingsExported),
    );
  });

  it("requires confirmation, cancels without writing and imports only the selected file", async () => {
    vi.spyOn(api, "readSettingsFile").mockResolvedValue("selected-json");
    const next = settings();
    const apply = vi.spyOn(api, "importSettings").mockResolvedValue(next);
    const replaced = renderTransfer();
    fireEvent.click(screen.getByRole("button", { name: "Import" }));
    let dialog = await screen.findByRole("alertdialog");
    expect(apply).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: messagesFor("en").cancel }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
    expect(apply).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Import" }));
    dialog = await screen.findByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Import" }));
    await waitFor(() => expect(replaced).toHaveBeenCalledWith(next));
    expect(apply).toHaveBeenCalledWith("selected-json");
    expect(toast.success).toHaveBeenCalledWith(messagesFor("en").settingsImported);
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("file picker cancellation doesn't open confirmation or replace settings", async () => {
    const pick = vi.spyOn(api, "readSettingsFile").mockResolvedValue(null);
    const replaced = renderTransfer();
    fireEvent.click(screen.getByRole("button", { name: "Import" }));
    await waitFor(() => expect(pick).toHaveBeenCalledOnce());
    await waitFor(() => expect(screen.getByRole("button", { name: "Import" })).toBeEnabled());
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(replaced).not.toHaveBeenCalled();
  });

  it.each(["exportSettingsFile", "readSettingsFile", "importSettings"] as const)(
    "reports %s failure without replacing settings",
    async (method) => {
      vi.spyOn(api, "readSettingsFile").mockResolvedValue("selected-json");
      vi.spyOn(api, method).mockRejectedValue({ code: "StorageFailed" });
      const replaced = renderTransfer();
      fireEvent.click(
        screen.getByRole("button", { name: method === "exportSettingsFile" ? "Export" : "Import" }),
      );
      if (method === "importSettings") {
        fireEvent.click(
          within(await screen.findByRole("alertdialog")).getByRole("button", { name: "Import" }),
        );
      }
      await waitFor(() => expect(reportError).toHaveBeenCalledOnce());
      expect(replaced).not.toHaveBeenCalled();
      expect(toast.success).not.toHaveBeenCalled();
    },
  );
  it.each([false, true])(
    "confirms reset with wipeApiKey=%s and publishes the returned settings",
    async (wipe) => {
      const copy = messagesFor("en");
      const next = settings();
      const reset = vi.spyOn(api, "resetSettings").mockResolvedValue(next);
      const replaced = vi.fn();
      render(<SettingsReset copy={copy} onSettingsReplaced={replaced} />);
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
    render(<SettingsReset copy={copy} onSettingsReplaced={replaced} />);
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
    render(<AdvancedSettings settings={settings()} copy={copy} onChange={vi.fn()} />);
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
      <>
        <AdvancedSettings settings={settings()} copy={copy} onChange={changed} />
        <SettingsReset copy={copy} onSettingsReplaced={vi.fn()} />
      </>,
    );
    fireEvent.click(screen.getByRole("switch", { name: copy.debugLogs }));
    expect(changed).toHaveBeenCalledWith({ debugLogging: true });
    fireEvent.click(screen.getByRole("button", { name: copy.resetSettings }));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", { name: copy.cancel }),
    );
    expect(reset).not.toHaveBeenCalled();
  });
  it("shows build information and retries a failed runtime read without dropping the version", async () => {
    vi.mocked(api.runtimeInfo).mockRejectedValueOnce(new Error("runtime unavailable"));
    const copy = messagesFor("en");
    render(<AdvancedSettings settings={settings()} copy={copy} onChange={vi.fn()} />);
    expect(await screen.findByText("0.5.0")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("runtime unavailable");
    fireEvent.click(screen.getByRole("button", { name: copy.retryAction }));
    expect(await screen.findByText("Voxely (Local build)")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: copy.resetAll })).not.toBeInTheDocument();
  });
});
