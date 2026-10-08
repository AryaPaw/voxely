import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AppSettings, RetentionApplyResult, RetentionPreview } from "../../../lib/api";
import { messagesFor } from "../../../lib/i18n";
import { HistorySettings } from "./HistorySettings";

let scrollIntoViewDescriptor: PropertyDescriptor | undefined;

beforeEach(() => {
  scrollIntoViewDescriptor = Object.getOwnPropertyDescriptor(
    HTMLElement.prototype,
    "scrollIntoView",
  );
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
    configurable: true,
    value: vi.fn(),
  });
});

afterEach(() => {
  cleanup();
  if (scrollIntoViewDescriptor) {
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", scrollIntoViewDescriptor);
  } else {
    delete (HTMLElement.prototype as Partial<HTMLElement>).scrollIntoView;
  }
  scrollIntoViewDescriptor = undefined;
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
    language: "auto",
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
    firstRunComplete: false,
  };
}

describe("HistorySettings", () => {
  function mount(
    patch: Partial<AppSettings> = {},
    preview = vi.fn(async () => retentionPreview),
    apply = vi.fn(async (): Promise<RetentionApplyResult> => ({
      settings: settings(),
      deleted: [],
      failed: [],
    })),
  ) {
    render(
      <HistorySettings
        settings={{ ...settings(), ...patch }}
        copy={messagesFor("en")}
        onChange={vi.fn()}
        onDeleteAll={async () => undefined}
        onPreviewRetention={preview}
        onApplyRetention={apply}
      />,
    );
    return { preview, apply };
  }

  it.each(["", "0", "1.5", "17179869184", "9007199254740992"])(
    "rejects storage limit %s before preview or apply",
    (value) => {
      const { preview, apply } = mount();
      const field = screen.getByLabelText(messagesFor("en").storageLimit);
      fireEvent.change(field, { target: { value } });
      fireEvent.blur(field);
      expect(field).toHaveAttribute("aria-invalid", "true");
      expect(screen.getByRole("alert")).toHaveTextContent(
        messagesFor("en").storageLimitWholeGigabytes,
      );
      expect(preview).not.toHaveBeenCalled();
      expect(apply).not.toHaveBeenCalled();
    },
  );

  it("preserves legacy half-gigabyte settings until the user supplies a new limit", () => {
    const { preview } = mount({ storageLimit: "500mb" });
    const field = screen.getByLabelText(messagesFor("en").storageLimit);
    expect(field).toHaveValue(null);
    expect(screen.getByText(messagesFor("en").storageLimitLegacyHint)).toBeInTheDocument();
    fireEvent.change(field, { target: { value: "2" } });
    expect(preview).not.toHaveBeenCalled();
    fireEvent.blur(field);
    expect(preview).toHaveBeenCalledWith(expect.objectContaining({ storageLimit: "2gb" }));
  });

  it("supports unlimited storage and discards a cancelled change", async () => {
    const { preview, apply } = mount();
    fireEvent.click(screen.getByRole("switch", { name: messagesFor("en").limitUnlimited }));
    expect(screen.getByLabelText(messagesFor("en").storageLimit)).toBeDisabled();
    expect(preview).toHaveBeenCalledWith(expect.objectContaining({ storageLimit: "unlimited" }));
    fireEvent.click(await screen.findByRole("button", { name: messagesFor("en").retentionApply }));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: messagesFor("en").retentionCancel,
      }),
    );
    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
    expect(screen.getByLabelText(messagesFor("en").storageLimit)).toHaveValue(1);
    expect(apply).not.toHaveBeenCalled();
  });

  it.each([
    { ...retentionPreview, recordingIdsToDelete: ["same", "same"] },
    { ...retentionPreview, recordingIdsToDelete: ["", "valid"] },
    { ...retentionPreview, recordingIdsToDelete: ["only-one"] },
    { ...retentionPreview, bytesToFree: -1 },
    { ...retentionPreview, filesToDelete: 0.5 },
  ])("fails closed for invalid deletion preview %j", async (result) => {
    const { apply } = mount(
      {},
      vi.fn(async () => result),
    );
    const field = screen.getByLabelText(messagesFor("en").storageLimit);
    fireEvent.change(field, { target: { value: "2" } });
    fireEvent.blur(field);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      messagesFor("en").retentionPreviewFailed,
    );
    expect(screen.getByRole("button", { name: messagesFor("en").retentionApply })).toBeDisabled();
    expect(apply).not.toHaveBeenCalled();
  });

  it.each(["valid", "malformed", "rejected"] as const)(
    "refreshes preview after failed apply (%s)",
    async (refresh) => {
      const copy = messagesFor("en");
      const preview = vi.fn(async () => retentionPreview);
      preview
        .mockImplementationOnce(async () => retentionPreview)
        .mockImplementationOnce(async () => {
          if (refresh === "rejected") throw new Error("offline");
          return refresh === "malformed"
            ? { ...retentionPreview, entriesToDelete: -1 }
            : { ...retentionPreview, bytesToFree: 0 };
        });
      const apply = vi.fn(async (): Promise<RetentionApplyResult> => {
        throw new Error("changed");
      });
      mount({}, preview, apply);
      const field = screen.getByLabelText(copy.storageLimit);
      fireEvent.change(field, { target: { value: "2" } });
      fireEvent.blur(field);
      await waitFor(() =>
        expect(screen.getByRole("button", { name: copy.retentionApply })).toBeEnabled(),
      );
      fireEvent.click(screen.getByRole("button", { name: copy.retentionApply }));
      fireEvent.click(
        within(await screen.findByRole("alertdialog")).getByRole("button", {
          name: copy.retentionApply,
        }),
      );
      await waitFor(() => expect(preview).toHaveBeenCalledTimes(2));
      expect(apply).toHaveBeenCalledOnce();
      expect(within(screen.getByRole("alertdialog")).getByRole("alert")).toHaveTextContent(
        copy.retentionApplyFailed,
      );
      if (refresh !== "valid")
        expect(screen.getByText(copy.retentionPreviewFailed)).toBeInTheDocument();
      else expect(screen.getByText("0 B")).toBeInTheDocument();
    },
  );
  it("describes original storage and confirms full history deletion", () => {
    const onDeleteAll = vi.fn(async () => undefined);
    render(
      <HistorySettings
        settings={settings()}
        copy={messagesFor("en")}
        onChange={() => undefined}
        onDeleteAll={onDeleteAll}
        onPreviewRetention={async () => retentionPreview}
        onApplyRetention={async () => ({
          settings: settings(),
          deleted: [],
          failed: [],
        })}
      />,
    );

    expect(
      screen.getByText(/retry uses the processed recording when available/i),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Delete all history" }));
    const dialog = screen.getByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete" }));
    expect(onDeleteAll).toHaveBeenCalledOnce();
  });

  it("previews storage impact and applies only after explicit confirmation", async () => {
    const preview = vi.fn(async () => retentionPreview);
    const apply = vi.fn(async (next: AppSettings): Promise<RetentionApplyResult> => ({
      settings: next,
      deleted: ["recording-1"],
      failed: [],
    }));
    render(
      <HistorySettings
        settings={settings()}
        copy={messagesFor("en")}
        onChange={() => undefined}
        onDeleteAll={async () => undefined}
        onPreviewRetention={preview}
        onApplyRetention={apply}
      />,
    );

    fireEvent.click(screen.getByRole("combobox", { name: "Keep recordings" }));
    fireEvent.click(await screen.findByRole("option", { name: "1 day" }));
    expect(await screen.findByText("History entries to delete")).toBeInTheDocument();
    expect(screen.getByText("2", { selector: "dd" })).toBeInTheDocument();
    expect(preview).toHaveBeenCalledWith(expect.objectContaining({ retention: "1d" }));
    expect(apply).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Apply storage settings" }));
    const dialog = await screen.findByRole("alertdialog");
    expect(within(dialog).getByText(/History entries to delete: 2/)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Apply storage settings" }));
    await waitFor(() => expect(apply).toHaveBeenCalledOnce());
    expect(apply).toHaveBeenCalledWith(
      expect.objectContaining({ retention: "1d" }),
      expect.objectContaining({ recordingIdsToDelete: ["recording-1", "recording-2"] }),
    );
  });

  it("fails closed when the retention preview wire fields are malformed", async () => {
    const malformed = {
      entries_to_delete: 2,
      files_to_delete: 3,
      bytes_to_free: 1_048_576,
      protected_entries: 1,
      protected_bytes: 512,
      total_audio_bytes: 2_097_152,
    } as unknown as RetentionPreview;
    const apply = vi.fn(async (): Promise<RetentionApplyResult> => ({
      settings: settings(),
      deleted: [],
      failed: [],
    }));
    render(
      <HistorySettings
        settings={settings()}
        copy={messagesFor("en")}
        onChange={() => undefined}
        onDeleteAll={async () => undefined}
        onPreviewRetention={async () => malformed}
        onApplyRetention={apply}
      />,
    );

    fireEvent.click(screen.getByRole("combobox", { name: "Keep recordings" }));
    fireEvent.click(await screen.findByRole("option", { name: "1 day" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not calculate the effect of this storage change.",
    );
    expect(screen.getByRole("button", { name: "Apply storage settings" })).toBeDisabled();
    expect(apply).not.toHaveBeenCalled();
  });
});

const retentionPreview: RetentionPreview = {
  entriesToDelete: 2,
  recordingIdsToDelete: ["recording-1", "recording-2"],
  filesToDelete: 3,
  bytesToFree: 1_048_576,
  protectedEntries: 1,
  protectedBytes: 512,
  totalAudioBytes: 2_097_152,
};
