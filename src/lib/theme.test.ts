import { accentPreset } from "./accent";
import { describe, expect, it } from "vitest";
import { applyTheme, resolvedTheme, watchSystemTheme } from "./theme";

describe("applyTheme", () => {
  it("sets dark class", () => {
    applyTheme("dark");
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    applyTheme("light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });

  it("resolves system to the current document theme", () => {
    window.matchMedia = () =>
      ({
        matches: false,
        media: "",
        onchange: null,
        addListener: () => undefined,
        removeListener: () => undefined,
        addEventListener: () => undefined,
        removeEventListener: () => undefined,
        dispatchEvent: () => false,
      }) as unknown as MediaQueryList;
    expect(resolvedTheme("light")).toBe("light");
    expect(resolvedTheme("dark")).toBe("dark");
    expect(resolvedTheme("system")).toBe("light");
  });

  it("follows system preference when requested", () => {
    window.matchMedia = () =>
      ({
        matches: true,
        media: "",
        onchange: null,
        addListener: () => undefined,
        removeListener: () => undefined,
        addEventListener: () => undefined,
        removeEventListener: () => undefined,
        dispatchEvent: () => false,
      }) as unknown as MediaQueryList;
    applyTheme("system");
    expect(document.documentElement.classList.contains("dark")).toBe(true);
  });

  it("watches system preference changes", () => {
    const listeners: Array<() => void> = [];
    window.matchMedia = () =>
      ({
        matches: false,
        media: "",
        onchange: null,
        addListener: () => undefined,
        removeListener: () => undefined,
        addEventListener: (_event: string, listener: () => void) => {
          listeners.push(listener);
        },
        removeEventListener: () => undefined,
        dispatchEvent: () => false,
      }) as unknown as MediaQueryList;
    const stop = watchSystemTheme("system");
    expect(listeners).toHaveLength(1);
    listeners[0]();
    expect(document.documentElement.style.getPropertyValue("--primary")).toBe(
      accentPreset().light["--primary"],
    );
    stop();
  });
});

it("retains the chosen accent when system appearance changes", () => {
  let dark = false;
  let changed = () => {};
  window.matchMedia = () =>
    ({
      matches: dark,
      addEventListener: (_event: string, listener: () => void) => {
        changed = listener;
      },
      removeEventListener: () => undefined,
    }) as unknown as MediaQueryList;
  const color = { family: "rose", shade: 500 };
  applyTheme("system", color);
  const stop = watchSystemTheme("system", color);
  expect(document.documentElement.style.getPropertyValue("--primary")).toBe(
    accentPreset(color).light["--primary"],
  );
  dark = true;
  changed();
  expect(document.documentElement.style.getPropertyValue("--primary")).toBe(
    accentPreset(color).dark["--primary"],
  );
  stop();
});
