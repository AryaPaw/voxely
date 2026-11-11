import { afterEach, describe, expect, it } from "vitest";
import {
  contrast,
  contrastForeground,
  fromHsl,
  makeAccentTokens,
  oklchHex,
  readableTone,
  toHsl,
} from "./accent-color";
import {
  ACCENT_FAMILIES,
  ACCENT_SHADES,
  accentPreset,
  applyAccent,
  applyPreviewAccent,
  isAccentColor,
} from "./accent";
import data from "./accent-presets.json";

afterEach(() => document.documentElement.removeAttribute("style"));
describe("shared accent palette", () => {
  it("keeps text and filled controls readable for every available choice and theme", () => {
    for (const family of ACCENT_FAMILIES)
      for (const shade of ACCENT_SHADES)
        for (const theme of ["light", "dark"] as const) {
          const p = accentPreset({ family, shade }),
            t = p[theme];
          expect(makeAccentTokens(p.color, theme, data.surfaces[theme])).toEqual(t);
          for (const color of Object.values(t)) expect(color).toMatch(/^#[0-9a-f]{6}$/);
          expect(
            contrast(t["--primary-text"], data.surfaces[theme].background),
            `${family}-${shade}/${theme} text`,
          ).toBeGreaterThanOrEqual(4.5);
          expect(t["--primary"]).toBe(p.color);
          expect(t["--primary-foreground"]).toBe(contrastForeground(p.color));
          for (const surface of [data.surfaces[theme].background, data.surfaces[theme].card]) {
            expect(contrast(t["--primary-text"], surface)).toBeGreaterThanOrEqual(4.5);
            expect(contrast(t["--primary-text-hover"], surface)).toBeGreaterThanOrEqual(4.5);
          }
          expect(
            contrast(t["--primary"], t["--primary-foreground"]),
            `${family}-${shade}/${theme} button`,
          ).toBeGreaterThanOrEqual(4.5);
          expect(
            contrast(t["--primary-hover"], t["--primary-foreground"]),
            `${family}-${shade}/${theme} hover`,
          ).toBeGreaterThanOrEqual(4.5);
          expect(
            contrast(t["--sidebar-accent-foreground"], t["--sidebar-active-hover"]),
            `${family}-${shade}/${theme} sidebar hover`,
          ).toBeGreaterThanOrEqual(4.5);
          expect(contrast(t["--sidebar-hover"], data.surfaces[theme].card)).toBeLessThan(
            contrast(t["--sidebar-accent"], data.surfaces[theme].card),
          );
          expect(contrast(t["--sidebar-accent"], data.surfaces[theme].card)).toBeLessThan(
            contrast(t["--sidebar-active-hover"], data.surfaces[theme].card),
          );
          expect(
            contrast(t["--overlay-status"], data.surfaces[theme].overlay),
          ).toBeGreaterThanOrEqual(4.5);
        }
  });
  it("rejects unsupported settings and replaces the whole palette when changing themes", () => {
    for (const value of [
      null,
      [],
      {},
      "sky",
      { family: "constructor", shade: 500 },
      { family: "sky", shade: 50 },
      { family: "sky", shade: NaN },
    ])
      expect(isAccentColor(value)).toBe(false);
    expect(accentPreset()).toBe(accentPreset({ family: "sky", shade: 500 }));
    expect(accentPreset({ family: "red", shade: 50 })).toBe(accentPreset());
    applyAccent("dark", { family: "rose", shade: 600 });
    expect(document.documentElement.style.getPropertyValue("--primary")).toBe(
      accentPreset({ family: "rose", shade: 600 }).dark["--primary"],
    );
    applyAccent("light");
    expect(document.documentElement.style.getPropertyValue("--overlay-recording")).toBe(
      accentPreset().light["--overlay-recording"],
    );
    applyPreviewAccent("dark", "#ff2056");
    expect(document.documentElement.style.getPropertyValue("--primary")).toBe(
      makeAccentTokens("#ff2056", "dark", data.surfaces.dark)["--primary"],
    );
    const previous = document.documentElement.style.cssText;
    applyPreviewAccent("light", "url(unsafe)");
    expect(document.documentElement.style.cssText).toBe(previous);
  });
  it("converts Tailwind OKLCH and handles neutral and saturated hues", () => {
    expect(oklchHex("oklch(0.685 0.169 237.323)")).toMatch(/^#[0-9a-f]{6}$/);
    expect(oklchHex("oklch(100% 0 none)")).toBe("#ffffff");
    expect(() => oklchHex("red")).toThrow();
    for (const color of ["#000000", "#ffffff", "#ff0000", "#00ff00", "#0000ff", "#ff00ff"])
      expect(fromHsl(toHsl(color))).toBe(color);
    expect(contrast("#000000", "#ffffff")).toBe(21);
    expect(contrast(readableTone("#ffffff", "#ffffff", "light"), "#ffffff")).toBeGreaterThanOrEqual(
      4.5,
    );
    expect(contrast(readableTone("#000000", "#000000", "dark"), "#000000")).toBeGreaterThanOrEqual(
      4.5,
    );
  });
});
