import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Recording } from "../../../lib/api";
import { messagesFor } from "../../../lib/i18n";
import { reportError } from "../../../lib/system-notify";
import { HistoryCard, HistoryPane } from "./HistoryPane";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "recording_audio_url") {
      return null;
    }
    return undefined;
  }),
  convertFileSrc: (path: string) => path,
}));

vi.mock("../../../lib/system-notify", () => ({ reportError: vi.fn() }));

afterEach(() => {
  cleanup();
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (cmd: string) =>
    cmd === "recording_audio_url" ? null : undefined,
  );
  vi.mocked(reportError).mockClear();
});

function recording(patch: Partial<Recording> = {}): Recording {
  return {
    id: "1",
    createdAt: "2026-01-02T10:00:00.000Z",
    durationMs: 65000,
    rawAudioPath: "a.wav",
    processedAudioPath: null,
    transcript: "hello",
    status: "completed",
    provider: "openrouter",
    model: "openai/gpt-transcribe",
    attemptCount: 1,
    lastErrorCode: "InvalidApiKey",
    lastErrorMessage: "Нет API-ключа OpenRouter",
    cost: 0.01,
    generationId: null,
    latencyMs: 120,
    ...patch,
  };
}

describe("HistoryPane", () => {
  it.each([
    ["Retry transcription", "retry_recording"],
    ["Reprocess original", "manual_reprocess"],
  ])("recovers from failed %s and reports a subsequent refresh failure", async (label, command) => {
    const onRefresh = vi.fn(async () => {
      throw { code: "StorageFailed" };
    });
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === command) throw { code: "NetworkUnavailable" };
      return undefined;
    });
    render(
      <HistoryCard
        item={recording()}
        copy={messagesFor("en")}
        detailsOpen={false}
        onToggleDetails={() => undefined}
        onRefresh={onRefresh}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: label }));
    await waitFor(() => expect(onRefresh).toHaveBeenCalledOnce());
    await waitFor(() => expect(reportError).toHaveBeenCalledTimes(2));
    expect(screen.getByRole("button", { name: label })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "Cancel retry" })).not.toBeInTheDocument();
  });

  it("reports copy failure without showing copied success", async () => {
    vi.mocked(invoke).mockRejectedValueOnce({ code: "StorageFailed" });
    render(
      <HistoryCard
        item={recording()}
        copy={messagesFor("en")}
        detailsOpen={false}
        onToggleDetails={() => undefined}
        onRefresh={async () => undefined}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    await waitFor(() => expect(reportError).toHaveBeenCalledOnce());
    expect(screen.getByRole("button", { name: "Copy" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "Copied" })).not.toBeInTheDocument();
  });

  it("disables playback after an audio-path IPC failure", async () => {
    vi.mocked(invoke).mockRejectedValueOnce({ code: "StorageFailed" });
    const { container } = render(
      <HistoryCard
        item={recording()}
        copy={messagesFor("en")}
        detailsOpen={false}
        onToggleDetails={() => undefined}
        onRefresh={async () => undefined}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Listen" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Listen" })).toBeDisabled());
    expect(container.querySelector("audio")).toBeNull();
  });

  it("retries the initial failed history load without presenting an empty-history success", () => {
    const onRefresh = vi.fn(async () => undefined);
    const copy = messagesFor("en");
    render(
      <HistoryPane
        items={[]}
        query=""
        historyError="refresh"
        keyConfigured
        hotkey="Ctrl+Space"
        onQuery={() => undefined}
        onRefresh={onRefresh}
        onOpenKey={() => undefined}
        onOpenSettings={() => undefined}
        copy={copy}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent(copy.historyLoadFailed);
    expect(
      screen.queryByText(copy.emptyHistory.replace("{hotkey}", "Ctrl+Space")),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: copy.retryLoad }));
    expect(onRefresh).toHaveBeenCalledOnce();
  });

  it("reports recovery folder failure and prevents another page request while loading", async () => {
    const copy = messagesFor("en");
    const onLoadMore = vi.fn();
    vi.mocked(invoke).mockRejectedValueOnce({ code: "StorageFailed" });
    render(
      <HistoryPane
        items={[recording()]}
        query=""
        recovered
        hasMore
        isLoadingMore
        keyConfigured
        hotkey="Ctrl+Space"
        onQuery={() => undefined}
        onLoadMore={onLoadMore}
        onRefresh={async () => undefined}
        onOpenKey={() => undefined}
        onOpenSettings={() => undefined}
        copy={copy}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: copy.settingsRecoveryOpenFolder }));
    await waitFor(() => expect(reportError).toHaveBeenCalledOnce());
    const loading = screen.getByRole("button", { name: copy.loading });
    expect(loading).toBeDisabled();
    fireEvent.click(loading);
    expect(onLoadMore).not.toHaveBeenCalled();
  });

  it("labels search and confirms single delete", async () => {
    render(
      <HistoryPane
        items={[recording()]}
        query=""
        keyConfigured
        hotkey="Ctrl+Shift+Space"
        onQuery={() => undefined}
        onRefresh={async () => undefined}
        onOpenKey={() => undefined}
        onOpenSettings={() => undefined}
        copy={messagesFor("ru")}
      />,
    );
    expect(screen.getByRole("heading", { name: "История" })).toBeInTheDocument();
    expect(screen.getAllByText(/1 мин\. 5 сек\./)).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Удалить" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Копировать" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("copy_transcript", { text: "hello" });
    });
    expect(screen.getByRole("button", { name: "Скопировано" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Повторить расшифровку" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Удалить" }));
    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Удалить" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("delete_history_item", { id: "1" });
    });
  });

  it("keeps retry on completed transcripts and hides it without usable audio", () => {
    const { rerender } = render(
      <HistoryCard
        item={recording({ processedAudioPath: "processed.wav" })}
        copy={messagesFor("ru")}
        detailsOpen={false}
        onToggleDetails={() => undefined}
        onRefresh={async () => undefined}
      />,
    );
    expect(screen.getByRole("button", { name: "Повторить расшифровку" })).toBeEnabled();
    rerender(
      <HistoryCard
        item={recording({ rawAudioPath: null, processedAudioPath: null })}
        copy={messagesFor("ru")}
        detailsOpen={false}
        onToggleDetails={() => undefined}
        onRefresh={async () => undefined}
      />,
    );
    expect(screen.queryByRole("button", { name: "Повторить расшифровку" })).not.toBeInTheDocument();
    rerender(
      <HistoryCard
        item={recording({ status: "failed", durationMs: 0 })}
        copy={messagesFor("ru")}
        detailsOpen={false}
        onToggleDetails={() => undefined}
        onRefresh={async () => undefined}
      />,
    );
    expect(screen.queryByRole("button", { name: "Повторить расшифровку" })).not.toBeInTheDocument();
    expect(screen.getByText(/нет пригодной аудиозаписи/)).toBeInTheDocument();
  });

  it("shows the truncation reason after a completed recording", () => {
    render(
      <HistoryCard
        item={recording({
          lastErrorCode: "RecordingTruncated",
          lastErrorMessage: "Recording truncated",
        })}
        copy={messagesFor("en")}
        detailsOpen={false}
        onToggleDetails={() => undefined}
        onRefresh={async () => undefined}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "Recording was truncated after reaching the audio size or buffer limit.",
    );
  });

  it("shows the upload size limit separately from capture truncation", () => {
    render(
      <HistoryCard
        item={recording({
          status: "failed",
          lastErrorCode: "RecordingTooLarge",
          lastErrorMessage: "Recording is too large to send",
        })}
        copy={messagesFor("en")}
        detailsOpen={false}
        onToggleDetails={() => undefined}
        onRefresh={async () => undefined}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "The audio file exceeds the upload size limit.",
    );
  });

  it("offers reprocessing only when the original exists and keeps it separate from retry", async () => {
    const onRefresh = vi.fn(async () => undefined);
    vi.mocked(invoke).mockClear();
    const { rerender } = render(
      <HistoryCard
        item={recording({ processedAudioPath: "processed.wav" })}
        copy={messagesFor("en")}
        detailsOpen
        onToggleDetails={() => undefined}
        onRefresh={onRefresh}
      />,
    );
    expect(screen.getByText(/Retry sends the saved processed recording/i)).toBeInTheDocument();
    expect(screen.getByText(/Reprocessing uses the saved original/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Reprocess original" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("manual_reprocess", { recordingId: "1" });
      expect(onRefresh).toHaveBeenCalled();
    });

    rerender(
      <HistoryCard
        item={recording({ rawAudioPath: null, processedAudioPath: "processed.wav" })}
        copy={messagesFor("en")}
        detailsOpen={false}
        onToggleDetails={() => undefined}
        onRefresh={onRefresh}
      />,
    );
    expect(screen.queryByRole("button", { name: "Reprocess original" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry transcription" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry transcription" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("retry_recording", { id: "1" }));
  });

  it("shows interrupted as retryable, not processing", () => {
    render(
      <HistoryCard
        item={recording({
          status: "interrupted",
          transcript: null,
          lastErrorMessage: "Запись прервана. Можно повторить расшифровку",
        })}
        copy={messagesFor("ru")}
        detailsOpen
        onToggleDetails={() => undefined}
        onRefresh={async () => undefined}
      />,
    );
    expect(screen.getByText("Расшифровка недоступна.")).toBeInTheDocument();
    expect(screen.getByText(/Повторить расшифровку/)).toBeInTheDocument();
    expect(screen.queryByText("Расшифровка")).not.toBeInTheDocument();
  });

  it("keeps the previous transcript and shows the latest retry error", () => {
    render(
      <HistoryCard
        item={recording({
          status: "failed",
          transcript: "previous successful text",
          rawAudioPath: null,
          processedAudioPath: null,
          lastErrorCode: "NetworkUnavailable",
          lastErrorMessage: "network failed",
        })}
        copy={messagesFor("en")}
        detailsOpen={false}
        onToggleDetails={() => undefined}
        onRefresh={async () => undefined}
      />,
    );
    expect(screen.getByText("previous successful text")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent(/Network unavailable/i);
    expect(screen.getByRole("status")).toHaveTextContent(/no usable audio recording is available/i);
    expect(screen.queryByRole("button", { name: "Retry transcription" })).not.toBeInTheDocument();
  });

  it("toggles details and shows localized latency", () => {
    render(
      <HistoryCard
        item={recording()}
        copy={messagesFor("ru")}
        detailsOpen
        onToggleDetails={() => undefined}
        onRefresh={async () => undefined}
      />,
    );
    expect(screen.getByText(/120 мс/)).toBeInTheDocument();
    expect(screen.getByText("Нет API-ключа OpenRouter")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Сведения" }));
  });

  it("shows audio processing instead of transcription for incomplete DSP", () => {
    render(
      <HistoryCard
        item={recording({
          status: "processing",
          transcript: null,
          processedAudioPath: null,
        })}
        copy={messagesFor("ru")}
        detailsOpen={false}
        onToggleDetails={() => undefined}
        onRefresh={async () => undefined}
      />,
    );
    expect(screen.getByText("Обработка")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Отменить повтор" })).toBeInTheDocument();
  });

  it("does not fetch audio until listen", async () => {
    render(
      <HistoryCard
        item={recording()}
        copy={messagesFor("ru")}
        detailsOpen={false}
        onToggleDetails={() => undefined}
        onRefresh={async () => undefined}
      />,
    );
    expect(invoke).not.toHaveBeenCalledWith("recording_audio_url", expect.anything());
    fireEvent.click(screen.getByRole("button", { name: "Слушать" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("recording_audio_url", { id: "1" });
    });
  });

  it("distinguishes empty history from empty search", () => {
    const { rerender } = render(
      <HistoryPane
        items={[]}
        query=""
        keyConfigured
        hotkey="Ctrl+Shift+Space"
        onQuery={() => undefined}
        onRefresh={async () => undefined}
        onOpenKey={() => undefined}
        onOpenSettings={() => undefined}
        copy={messagesFor("ru")}
      />,
    );
    expect(screen.getByText(/Записей пока нет/)).toBeInTheDocument();
    rerender(
      <HistoryPane
        items={[]}
        query="needle"
        keyConfigured
        hotkey="Ctrl+Shift+Space"
        onQuery={() => undefined}
        onRefresh={async () => undefined}
        onOpenKey={() => undefined}
        onOpenSettings={() => undefined}
        copy={messagesFor("ru")}
      />,
    );
    expect(screen.getByText("Ничего не найдено по этому запросу.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Очистить поиск" })).toBeInTheDocument();
  });

  it("shows a retry action for a failed history page load", () => {
    const onLoadMore = vi.fn();
    render(
      <HistoryPane
        items={[recording()]}
        query=""
        hasMore
        historyError="page"
        keyConfigured
        hotkey="Ctrl+Shift+Space"
        onQuery={() => undefined}
        onLoadMore={onLoadMore}
        onRefresh={async () => undefined}
        onOpenKey={() => undefined}
        onOpenSettings={() => undefined}
        copy={messagesFor("en")}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Retry loading" }));
    expect(onLoadMore).toHaveBeenCalledOnce();
  });

  it("labels an empty successful transcript", () => {
    render(
      <HistoryCard
        item={recording({ transcript: "   " })}
        copy={messagesFor("ru")}
        detailsOpen={false}
        onToggleDetails={() => undefined}
        onRefresh={async () => undefined}
      />,
    );
    expect(screen.getByText("Расшифровка пустая")).toBeInTheDocument();
  });

  it("cancels a history retry without cancelling live dictation", async () => {
    render(
      <HistoryCard
        item={recording({
          status: "processing",
          transcript: null,
          processedAudioPath: "b.wav",
        })}
        copy={messagesFor("ru")}
        detailsOpen={false}
        onToggleDetails={() => undefined}
        onRefresh={async () => undefined}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Отменить повтор" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("cancel_history_retry", { id: "1" });
    });
  });

  it("resets the player on ended and error", async () => {
    const invokeMock = vi.mocked(invoke);
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "recording_audio_url") {
        return "C:/tmp/a.wav";
      }
      return undefined;
    });
    const { container } = render(
      <HistoryCard
        item={recording()}
        copy={messagesFor("ru")}
        detailsOpen={false}
        onToggleDetails={() => undefined}
        onRefresh={async () => undefined}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Слушать" }));
    await waitFor(() => {
      expect(container.querySelector("audio")).not.toBeNull();
    });
    const node = container.querySelector("audio");
    if (node) {
      Object.defineProperty(node, "duration", { configurable: true, value: 2 });
      Object.defineProperty(node, "currentTime", { configurable: true, value: 2, writable: true });
      fireEvent.ended(node);
      fireEvent.error(node);
    }
  });
});
