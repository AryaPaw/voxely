import { describe, expect, it } from "vitest";
import {
  formatInvokeError,
  localizedError,
  messagesFor,
  resolveUiLocale,
  updateToast,
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
  it("returns English copy", () => {
    expect(messagesFor("en").checkUpdates).toBe("Check for updates");
  });

  it("keeps the same keys in RU and EN", () => {
    expect(Object.keys(messagesFor("en")).sort()).toEqual(Object.keys(messagesFor("ru")).sort());
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
  });
});
