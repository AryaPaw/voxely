import { describe, expect, it } from "vitest";
import { formatReleaseDate, versionWithReleaseDate } from "./release-meta";

describe("versionWithReleaseDate", () => {
  it("formats a build ISO day in parentheses", () => {
    expect(formatReleaseDate("2026-09-15", "en-US")).toBe("Sep 15, 2026");
    expect(versionWithReleaseDate("0.2.5", "2026-01-02", "en-US")).toBe("0.2.5 (Jan 2, 2026)");
    expect(versionWithReleaseDate("", "2026-09-15", "en-US")).toBe("…");
  });
});
