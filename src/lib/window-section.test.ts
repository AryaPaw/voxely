import { describe, expect, it } from "vitest";
import { sectionFromSearch } from "./window-section";

describe("sectionFromSearch", () => {
  it("opens history by default", () => {
    expect(sectionFromSearch("")).toBe("history");
    expect(sectionFromSearch("?theme=dark")).toBe("history");
  });

  it("reads a known settings section", () => {
    expect(sectionFromSearch("?section=compare")).toBe("compare");
    expect(sectionFromSearch("?section=debug")).toBe("history");
    expect(sectionFromSearch("?section=debug", true)).toBe("debug");
    expect(sectionFromSearch("?overlay&section=about")).toBe("about");
  });

  it("ignores unknown section names", () => {
    expect(sectionFromSearch("?section=not-a-page")).toBe("history");
  });
});
