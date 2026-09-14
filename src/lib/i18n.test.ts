import { describe, expect, it } from "vitest";
import {
  formatInvokeError,
  localizedError,
  factoryPresetLabel,
  messagesFor,
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
    expect(
      factoryPresetLabel({ id: "obs-imported", name: "OBS Imported" }, messagesFor("ru")),
    ).toBe("Импорт из OBS");
    expect(factoryPresetLabel({ id: "custom-1", name: "OBS Mic" }, messagesFor("en"))).toBe(
      "OBS Mic",
    );
  });

  it("keeps the same keys in RU and EN", () => {
    expect(Object.keys(messagesFor("en")).sort()).toEqual(Object.keys(messagesFor("ru")).sort());
  });

  it("uses the same product name for local and release chrome", () => {
    expect(appDisplayName(messagesFor("en"), true)).toBe("Voxely");
    expect(appDisplayName(messagesFor("ru"), true)).toBe("Voxely");
    expect(appDisplayName(messagesFor("en"), false)).toBe("Voxely");
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
    expect(updateToast("none", messagesFor("en"))).toBe("No updates");
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
