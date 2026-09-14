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

  it("keeps the overlay viewport close to the pill", () => {
    const css = readFileSync(resolve("src/styles.css"), "utf8");
    const rust = readFileSync(resolve("src-tauri/src/app/overlay.rs"), "utf8");
    const conf = readFileSync(resolve("src-tauri/tauri.conf.json"), "utf8");
    const overlayHtml = readFileSync(resolve("overlay.html"), "utf8");
    expect(css).toContain("padding: 8px 12px 10px");
    expect(css).toContain("0 0 8px");
    expect(css).not.toContain("0 0 28px");
    expect(rust).toContain("OVERLAY_WIDTH: f64 = 320.0");
    expect(rust).toContain("OVERLAY_HEIGHT: f64 = 72.0");
    expect(conf).toContain('"url": "overlay.html"');
    expect(conf).not.toContain("index.html?overlay=1");
    expect(conf).toContain('"transparent": true');
    expect(conf).toContain("https://ipc.localhost");
    expect(overlayHtml).toContain('src="/src/overlay.tsx"');
    expect(overlayHtml).toContain('data-voxely-window="overlay"');
    expect(overlayHtml).toContain("background: transparent");
  });
});
