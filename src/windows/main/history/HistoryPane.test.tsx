import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Recording } from "../../../lib/api";
import { messagesFor } from "../../../lib/i18n";
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

afterEach(() => cleanup());

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
    expect(screen.getByRole("button", { name: "Повторить расшифровку" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Удалить" }));
    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Удалить" }));
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("delete_history_item", { id: "1" });
    });
  });

  it("keeps retry on completed transcripts and hides it without audio", () => {
    const { rerender } = render(
      <HistoryCard
        item={recording()}
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
    expect(screen.getByText(/недоступна/)).toBeInTheDocument();
    expect(screen.getByText(/Повторить расшифровку/)).toBeInTheDocument();
    expect(screen.queryByText("Расшифровка")).not.toBeInTheDocument();
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
