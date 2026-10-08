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
  vi.clearAllMocks();
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
    textReplacements: { enabled: false, rules: [] },
    activePresetId: "stt-fast",
    presets: [],
    firstRunComplete: true,
    uiLanguage: "en",
    ...patch,
  };
}

describe("TranscriptionSettings", () => {
  it.each([
    ["extraAttempts", "additionalRetries", "3", "-1", 2],
    ["connectTimeout", "connectTimeoutMs", "10000", "7999", 8000],
    ["requestTimeout", "requestTimeoutMs", "30000", "4999", 20000],
    ["initialDelay", "initialRetryDelayMs", "0", "-1", 500],
    ["maxDelay", "maxRetryDelayMs", "9000", "-1", 8000],
    ["totalLimit", "totalOperationTimeoutMs", "800000", "59999", 720000],
  ] as const)(
    "validates and commits %s without changing unrelated retry settings",
    (label, field, valid, invalid, original) => {
      invoke.mockResolvedValue([]);
      const changed = vi.fn();
      const copy = messagesFor("en");
      render(
        <TranscriptionSettings
          settings={settings()}
          copy={copy}
          keyConfigured
          onConfigured={vi.fn()}
          onChange={changed}
        />,
      );
      const input = screen.getByLabelText(copy[label]);
      fireEvent.blur(input);
      expect(changed).not.toHaveBeenCalled();
      fireEvent.change(input, { target: { value: invalid } });
      fireEvent.blur(input);
      expect(input).toHaveValue(original);
      expect(changed).not.toHaveBeenCalled();
      fireEvent.change(input, { target: { value: "0.5" } });
      fireEvent.blur(input);
      expect(input).toHaveValue(original);
      fireEvent.change(input, { target: { value: valid } });
      fireEvent.blur(input);
      expect(changed).toHaveBeenCalledExactlyOnceWith({
        retry: { ...settings().retry, [field]: Number(valid) },
      });
    },
  );

  it.each(["store_api_key", "test_openrouter"])(
    "reports %s failures without reporting success",
    async (command) => {
      const { reportError } = await import("../../../lib/system-notify");
      vi.mocked(reportError).mockClear();
      vi.mocked(toast.success).mockClear();
      invoke.mockImplementation(async (cmd) => {
        if (cmd === command) throw { code: "Unauthorized", message: "denied" };
        return [];
      });
      const configured = vi.fn();
      const copy = messagesFor("en");
      render(
        <TranscriptionSettings
          settings={settings()}
          copy={copy}
          keyConfigured={false}
          onConfigured={configured}
          onChange={vi.fn()}
        />,
      );
      fireEvent.click(
        screen.getByRole("button", {
          name: command === "store_api_key" ? copy.saveKey : copy.testConnection,
        }),
      );
      await waitFor(() => expect(reportError).toHaveBeenCalledOnce());
      expect(configured).not.toHaveBeenCalled();
      expect(toast.success).not.toHaveBeenCalled();
    },
  );
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
        return "openai/gpt-transcribe";
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
    vi.mocked(toast.success).mockClear();
    fireEvent.click(screen.getByRole("button", { name: "Test connection" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("test_openrouter", undefined);
    });
    await waitFor(() => {
      expect(toast.success).toHaveBeenCalledWith(
        "Selected transcription model is available: openai/gpt-transcribe",
      );
    });
    const extra = screen.getByDisplayValue("2");
    fireEvent.change(extra, { target: { value: "" } });
    fireEvent.blur(extra);
    expect(onChange).not.toHaveBeenCalled();
    expect(screen.getByDisplayValue("2")).toHaveValue(2);
    const connect = screen.getAllByDisplayValue("8000")[0];
    fireEvent.change(connect, { target: { value: "" } });
    fireEvent.blur(connect);
    expect(screen.getAllByDisplayValue("8000")[0]).toHaveValue(8000);
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

  it("refreshes the transcription catalog after replacing a configured key", async () => {
    const onConfigured = vi.fn();
    let catalogLoads = 0;
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "discover_models") {
        catalogLoads += 1;
        return catalogLoads === 1
          ? [{ id: "openai/gpt-transcribe", name: "Old key model" }]
          : [{ id: "openai/gpt-transcribe", name: "New key model" }];
      }
      if (cmd === "store_api_key") return true;
      if (cmd === "test_openrouter") return "openai/gpt-transcribe";
      return undefined;
    });
    render(
      <TranscriptionSettings
        settings={settings()}
        copy={messagesFor("en")}
        keyConfigured
        onConfigured={onConfigured}
        onChange={vi.fn()}
      />,
    );

    expect(await screen.findByText("Old key model")).toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText("Replace key"), {
      target: { value: "sk-or-replacement" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Replace key" }));

    await waitFor(() => {
      expect(invoke.mock.calls.filter(([command]) => command === "discover_models")).toHaveLength(
        2,
      );
    });
    expect(invoke).toHaveBeenCalledWith("store_api_key", { key: "sk-or-replacement" });
    expect(onConfigured).toHaveBeenCalledWith(true);
    expect(await screen.findByText("New key model")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Test connection" }));
    await waitFor(() => {
      expect(toast.success).toHaveBeenCalledWith(
        "Selected transcription model is available: openai/gpt-transcribe",
      );
    });
  });

  it("rejects out-of-range drafts and follows accepted or rolled-back settings", () => {
    const onChange = vi.fn();
    const initial = settings();
    const { rerender } = render(
      <TranscriptionSettings
        settings={initial}
        copy={messagesFor("en")}
        keyConfigured
        onConfigured={() => undefined}
        onChange={onChange}
      />,
    );

    const attempts = screen.getByDisplayValue("2");
    fireEvent.change(attempts, { target: { value: "6" } });
    fireEvent.blur(attempts);
    expect(attempts).toHaveValue(2);
    expect(onChange).not.toHaveBeenCalled();

    const request = screen.getByLabelText("Request timeout (ms)");
    fireEvent.change(request, { target: { value: "800000" } });
    fireEvent.blur(request);
    expect(request).toHaveValue(20000);
    expect(onChange).not.toHaveBeenCalled();

    const connect = screen.getByLabelText("Connect timeout (ms)");
    fireEvent.change(connect, { target: { value: "9000" } });
    fireEvent.blur(connect);
    expect(onChange).toHaveBeenCalledWith({
      retry: expect.objectContaining({ connectTimeoutMs: 9000 }),
    });

    rerender(
      <TranscriptionSettings
        settings={settings({
          retry: { ...initial.retry, connectTimeoutMs: 9000 },
        })}
        copy={messagesFor("en")}
        keyConfigured
        onConfigured={() => undefined}
        onChange={onChange}
      />,
    );
    expect(screen.getByLabelText("Connect timeout (ms)")).toHaveValue(9000);

    fireEvent.change(connect, { target: { value: "10000" } });
    fireEvent.blur(connect);
    rerender(
      <TranscriptionSettings
        settings={settings({ retry: initial.retry })}
        copy={messagesFor("en")}
        keyConfigured
        onConfigured={() => undefined}
        onChange={onChange}
      />,
    );
    expect(screen.getByLabelText("Connect timeout (ms)")).toHaveValue(8000);
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

  it("always shows a typed model id and opens the OpenRouter catalog", async () => {
    const onChange = vi.fn();
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "discover_models") {
        return [{ id: "openai/gpt-transcribe", name: "GPT Transcribe" }];
      }
      return undefined;
    });
    render(
      <TranscriptionSettings
        settings={settings()}
        copy={messagesFor("en")}
        keyConfigured
        onConfigured={() => undefined}
        onChange={onChange}
      />,
    );
    const field = await screen.findByLabelText("Model");
    expect(field).toHaveValue("openai/gpt-transcribe");
    fireEvent.change(field, { target: { value: "openai/gpt-4o-transcribe" } });
    fireEvent.blur(field);
    expect(onChange).toHaveBeenCalledWith({
      model: "openai/gpt-4o-transcribe",
      customModel: "openai/gpt-4o-transcribe",
    });
    fireEvent.click(screen.getByRole("button", { name: "OpenRouter catalog" }));
    expect(invoke).toHaveBeenCalledWith("open_openrouter_models", undefined);
  });
});
