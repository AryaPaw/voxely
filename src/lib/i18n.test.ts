import { describe, expect, it } from "vitest";
import { messagesFor, resolveUiLocale } from "./i18n";

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
});
