import { cleanup, fireEvent, render, screen } from "@testing-library/react";
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
  it("labels search and confirms single delete", () => {
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
    expect(screen.getByRole("button", { name: "Удалить" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Копировать" }));
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
  });
});
