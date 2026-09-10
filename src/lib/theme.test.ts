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
    stop();
  });
});
