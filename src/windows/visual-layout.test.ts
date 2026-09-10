import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

describe("visual tokens", () => {
  it("keeps overlay and history on semantic tokens", () => {
    const css = readFileSync(resolve("src/styles.css"), "utf8");
    expect(css).toContain("--overlay-bg");
    expect(css).toContain(".overlay-pill");
    expect(css).not.toMatch(/\.overlay-pill\s*\{[^}]*#16181e/);
    expect(css).not.toMatch(/\.history-card[^}]*#1a1d24/);
  });

  it("uses the comfort main window size", () => {
    const conf = readFileSync(resolve("src-tauri/tauri.conf.json"), "utf8");
    expect(conf).toContain('"width": 960');
    expect(conf).toContain('"height": 680');
    expect(conf).toContain('"minHeight": 520');
  });
});
