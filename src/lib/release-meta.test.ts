import { describe, expect, it } from "vitest";
import { APP_RELEASED_ON, formatReleaseDate, versionWithReleaseDate } from "./release-meta";

describe("versionWithReleaseDate", () => {
  it("formats the shipped release date in parentheses", () => {
    expect(formatReleaseDate(APP_RELEASED_ON, "en-US")).toBe("Sep 13, 2026");
    expect(versionWithReleaseDate("0.2.5", APP_RELEASED_ON, "en-US")).toBe("0.2.5 (Sep 13, 2026)");
    expect(versionWithReleaseDate("", APP_RELEASED_ON, "en-US")).toBe("…");
  });
});
