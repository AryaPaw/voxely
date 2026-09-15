import { describe, expect, it } from "vitest";
import type { SessionState } from "./api";
import {
  historyStatusLabel,
  overlayHudLabel,
  overlayHudVisible,
  overlayIsBusy,
  overlayLabel,
  overlayRetryDisplayAttempt,
  overlaySttAttempt,
  sessionStatusLabel,
} from "./session-copy";
import { acceptOverlayRevision } from "./overlay-snapshot";
import { formatDuration, formatTime } from "./utils";

const cases: Array<[SessionState, string]> = [
  [{ kind: "idle" }, ""],
  [{ kind: "startingRecording" }, "Запись"],
  [{ kind: "recording" }, "Запись"],
  [{ kind: "stoppingRecording" }, "Сохранение"],
  [{ kind: "saving" }, "Сохранение"],
  [{ kind: "processingAudio" }, "Обработка"],
  [{ kind: "transcribing", attempt: 1 }, "Расшифровка"],
  [{ kind: "transcribing", attempt: 2 }, "Расшифровка (try 2)"],
  [{ kind: "retryWaiting", attempt: 1, delayMs: 500 }, "Расшифровка (try 2)"],
  [{ kind: "retryWaiting", attempt: 2, delayMs: 500 }, "Расшифровка (try 3)"],
  [{ kind: "completed" }, ""],
  [
    { kind: "failed", message: "Нет API-ключа OpenRouter", code: "InvalidApiKey" },
    "Нет API-ключа OpenRouter",
  ],
];

describe("overlayLabel", () => {
  it("numbers HUD retry as the next attempt after a failed STT call", () => {
    expect(overlayRetryDisplayAttempt(1)).toBe(2);
    expect(overlayLabel({ kind: "retryWaiting", attempt: 1, delayMs: 80 })).toBe(
      "Расшифровка (try 2)",
    );
    expect(overlayLabel({ kind: "transcribing", attempt: 1 })).toBe("Расшифровка");
    expect(overlayLabel({ kind: "transcribing", attempt: 2 })).toBe("Расшифровка (try 2)");
    expect(overlaySttAttempt({ kind: "retryWaiting", attempt: 1, delayMs: 80 })).toBe(2);
    expect(overlaySttAttempt({ kind: "transcribing", attempt: 2 })).toBe(2);
  });

  it.each(cases)("%j", (state, label) => {
    expect(overlayLabel(state)).toBe(label);
  });

  it("maps error codes on failed overlay labels", () => {
    expect(overlayLabel({ kind: "failed", message: "legacy", code: "InvalidApiKey" })).toBe(
      "Нет API-ключа OpenRouter",
    );
  });
});

describe("overlayHudVisible", () => {
  it("hides idle and completed so the HUD never shows Ready", () => {
    expect(overlayHudVisible({ kind: "idle" })).toBe(false);
    expect(overlayHudVisible({ kind: "completed" })).toBe(false);
    expect(overlayHudVisible({ kind: "recording" })).toBe(true);
    expect(overlayHudVisible({ kind: "failed", message: "x", code: "InvalidApiKey" })).toBe(true);
    expect(overlayHudLabel(false, { kind: "idle" })).toBe("");
    expect(overlayHudLabel(false, { kind: "recording" })).toBe("");
    expect(overlayHudLabel(true, { kind: "recording" })).toBe("Запись");
    expect(acceptOverlayRevision(1, 2)).toBe(true);
    expect(acceptOverlayRevision(4, 4)).toBe(false);
  });
});

describe("overlayIsBusy", () => {
  it("marks in-progress states", () => {
    expect(overlayIsBusy({ kind: "recording" })).toBe(true);
    expect(overlayIsBusy({ kind: "transcribing", attempt: 1 })).toBe(true);
    expect(overlayIsBusy({ kind: "idle" })).toBe(false);
    expect(overlayIsBusy({ kind: "failed", message: "x", code: "InvalidApiKey" })).toBe(false);
  });
});

describe("historyStatusLabel", () => {
  it("maps known statuses", () => {
    expect(historyStatusLabel("failed")).toBe("Ошибка");
    expect(historyStatusLabel("completed")).toBe("Готово");
    expect(historyStatusLabel("processing")).toBe("Расшифровка");
    expect(historyStatusLabel("interrupted")).toBe("Запись прервана. Можно повторить расшифровку");
    expect(historyStatusLabel("other")).toBe("other");
  });
});

describe("sessionStatusLabel", () => {
  it("covers session kinds", () => {
    expect(sessionStatusLabel({ kind: "idle" })).toBe("Ожидание");
    expect(sessionStatusLabel({ kind: "startingRecording" })).toBe("Запись");
    expect(sessionStatusLabel({ kind: "recording" })).toBe("Запись");
    expect(sessionStatusLabel({ kind: "stoppingRecording" })).toBe("Обработка");
    expect(sessionStatusLabel({ kind: "saving" })).toBe("Обработка");
    expect(sessionStatusLabel({ kind: "processingAudio" })).toBe("Обработка");
    expect(sessionStatusLabel({ kind: "transcribing", attempt: 1 })).toBe("Расшифровка");
    expect(sessionStatusLabel({ kind: "retryWaiting", attempt: 2, delayMs: 1 })).toBe(
      "Расшифровка (try 3)",
    );
    expect(sessionStatusLabel({ kind: "completed" })).toBe("Готово");
    expect(sessionStatusLabel({ kind: "failed", message: "x", code: "InvalidApiKey" })).toBe(
      "Ошибка",
    );
  });
});

describe("formatDuration", () => {
  it("uses spoken second and minute units", () => {
    expect(formatDuration(5000)).toBe("5 сек.");
    expect(formatDuration(10000)).toBe("10 сек.");
    expect(formatDuration(65000)).toBe("1 мин. 5 сек.");
    expect(formatDuration(330000)).toBe("5 мин. 30 сек.");
    expect(formatDuration(300000)).toBe("5 мин.");
  });

  it("uses English units when asked", () => {
    expect(formatDuration(10000, { seconds: "sec.", minutes: "min." })).toBe("10 sec.");
    expect(formatDuration(330000, { seconds: "sec.", minutes: "min." })).toBe("5 min. 30 sec.");
  });
});

describe("formatTime", () => {
  it("returns a locale string", () => {
    expect(formatTime("2026-09-09T12:00:00.000Z").length).toBeGreaterThan(4);
  });
});
