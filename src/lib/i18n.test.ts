import { describe, expect, it } from "vitest";
import {
  formatInvokeError,
  localizedError,
  factoryPresetLabel,
  messagesFor,
  messagesForUiLanguage,
  resolveUiLocale,
  statusToast,
  updateToast,
  appDisplayName,
} from "./i18n";

describe("resolveUiLocale", () => {
  it("uses an explicit choice", () => {
    expect(resolveUiLocale("en", "ru-RU")).toBe("en");
    expect(resolveUiLocale("ru", "en-US")).toBe("ru");
  });

  it("follows the system language when set to auto", () => {
    expect(resolveUiLocale("auto", "ru-RU")).toBe("ru");
    expect(resolveUiLocale("auto", "en-US")).toBe("en");
  });
});

describe("messagesFor", () => {
  it("keeps nav and page titles on one registry", () => {
    expect(messagesFor("ru").navHistory).toBe("История");
    expect(messagesFor("en").navHistory).toBe("History");
    expect(messagesFor("ru").navFilters).toBe(messagesFor("ru").filtersTitle);
    expect(messagesFor("en").navFilters).toBe(messagesFor("en").filtersTitle);
    expect(messagesFor("ru").navAbout).toBe(messagesFor("ru").aboutTitle);
    expect(messagesFor("en").navAbout).toBe(messagesFor("en").aboutTitle);
    expect(messagesFor("ru").historyTitle).toBe(messagesFor("ru").navHistory);
    expect(messagesFor("en").historyTitle).toBe(messagesFor("en").navHistory);
  });

  it("formats incomplete API cost without Russian count inflection", () => {
    expect(messagesFor("ru").statisticsIncomplete.replace("{count}", "1")).toBe(
      "API-попытки без подтвержденной стоимости: 1. Итоговая сумма неполная.",
    );
  });

  it("keeps insert copy free of SendInput", () => {
    expect(messagesFor("en").insertHint).not.toMatch(/SendInput/);
    expect(messagesFor("ru").insertHint).not.toMatch(/SendInput/);
  });

  it("localizes factory DSP preset names by id", () => {
    expect(
      factoryPresetLabel({ id: "stt-fast", name: "Быстрая диктовка" }, messagesFor("en")),
    ).toBe("Fast dictation");
    expect(
      factoryPresetLabel({ id: "stt-optimized", name: "Качество (медленнее)" }, messagesFor("en")),
    ).toBe("Quality (slower)");
    expect(factoryPresetLabel({ id: "custom-1", name: "Custom mic" }, messagesFor("en"))).toBe(
      "Custom mic",
    );
  });

  it("keeps the same keys in RU and EN", () => {
    expect(Object.keys(messagesFor("en")).sort()).toEqual(Object.keys(messagesFor("ru")).sort());
  });

  it("uses the same product name for local and release chrome", () => {
    expect(appDisplayName(messagesFor("en"))).toBe("Voxely");
    expect(appDisplayName(messagesFor("ru"))).toBe("Voxely");
  });

  it("keeps English copy free of Cyrillic except the Russian language name", () => {
    const en = messagesFor("en");
    for (const [key, value] of Object.entries(en)) {
      if (key === "uiRu") {
        continue;
      }
      expect(value, key).not.toMatch(/[А-Яа-яЁё]/);
    }
  });

  it("maps typed error codes on both locales", () => {
    const codes = [
      "MicrophoneUnavailable",
      "AudioCaptureFailed",
      "AudioProcessingFailed",
      "StorageFailed",
      "InvalidApiKey",
      "InvalidModel",
      "RequestValidationFailed",
      "NetworkUnavailable",
      "ConnectionFailed",
      "RequestTimeout",
      "RateLimited",
      "ProviderUnavailable",
      "OpenRouterServerError",
      "ResponseMalformed",
      "RecordingTooLarge",
      "RecordingTruncated",
      "RetryDeadlineExceeded",
      "TextInsertionFailed",
      "Cancelled",
      "HotkeyFailed",
      "IllegalTransition",
      "TranscriptionInProgress",
      "Interrupted",
      "UnknownCode",
    ];
    for (const code of codes) {
      expect(localizedError(code, messagesFor("en")).length).toBeGreaterThan(0);
      expect(localizedError(code, messagesFor("ru")).length).toBeGreaterThan(0);
    }
    expect(localizedError("RecordingTooLarge", messagesFor("en"))).toBe(
      "The audio file exceeds the upload size limit.",
    );
    expect(localizedError("RecordingTooLarge", messagesFor("en"), "Recording truncated")).toBe(
      "Recording was truncated after reaching the audio size or buffer limit.",
    );
    expect(
      localizedError(
        "RecordingTooLarge",
        messagesFor("en"),
        "Recovered capture reached the maximum recording size",
      ),
    ).toBe("Recording was truncated after reaching the audio size or buffer limit.");
    expect(localizedError("RecordingTruncated", messagesFor("ru"))).toBe(
      "Запись обрезана: достигнут лимит размера аудио или буфера.",
    );
    expect(updateToast("none", messagesFor("en"))).toBe("No updates");
    expect(updateToast("available", messagesFor("en"))).toBe("An update is available");
    expect(updateToast("available", messagesFor("ru"))).toBe("Доступно обновление");
    expect(messagesFor("en").installUpdate).toBe("Install update");
    expect(messagesFor("ru").installUpdate).toBe("Установить обновление");
    expect(updateToast("installed", messagesFor("en"))).toBe("Update installed. Restarting…");
    expect(updateToast("busy", messagesFor("en"))).toBe("An update is already running");
    expect(updateToast("deferred", messagesFor("ru"))).toBe(
      "Обновление отложено до конца диктовки",
    );
    expect(updateToast("failed", messagesFor("en"))).toBe("Could not check for updates");
    expect(formatInvokeError("Cancelled", messagesFor("en"))).toBe("Cancelled");
    expect(formatInvokeError({ code: "TranscriptionInProgress" }, messagesFor("ru"))).toBe(
      "Расшифровка уже идёт",
    );
    expect(formatInvokeError({ code: "InvalidApiKey" }, messagesFor("en"))).toBe(
      "OpenRouter API key is missing",
    );
    expect(statusToast("error", messagesFor("en"), messagesFor("en").openSettingsFailed)).toBe(
      "Error: Could not open the settings folder",
    );
    expect(statusToast("success", messagesFor("ru"), messagesFor("ru").resetDone)).toBe(
      "Успех: Настройки сброшены",
    );
  });
});

describe("messagesForUiLanguage", () => {
  it("uses the effective system locale for automatic language and includes compare copy", () => {
    const ru = messagesForUiLanguage("auto", "ru-RU");
    const en = messagesForUiLanguage("auto", "en-US");
    expect(ru.compareRunning).toBe("Выполняется");
    expect(ru.compareRequestCount).toContain("{count}");
    expect(en.compareRunning).toBe("Running");
    expect(en.compareUnknownCost).toBe("Cost unknown");
  });
});
