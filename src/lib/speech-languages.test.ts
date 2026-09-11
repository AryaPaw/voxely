import { describe, expect, it } from "vitest";
import { SPEECH_LANGUAGES, speechLanguageOptions } from "./speech-languages";

describe("speechLanguageOptions", () => {
  it("keeps auto first and includes the Whisper ISO-639-1 set", () => {
    const options = speechLanguageOptions("Auto-detect", "en-US");
    expect(options[0]).toEqual({ value: "auto", label: "Auto-detect" });
    expect(SPEECH_LANGUAGES).toHaveLength(100);
    expect(options.map((item) => item.value)).toEqual(
      expect.arrayContaining(["ru", "en", "uk", "zh", "ja", "de", "fr", "yue", "haw"]),
    );
    expect(options.filter((item) => item.value === "auto")).toHaveLength(1);
  });

  it("keeps an unknown stored code visible", () => {
    const options = speechLanguageOptions("Auto-detect", "en-US", "xx");
    expect(options.some((item) => item.value === "xx")).toBe(true);
  });
});
