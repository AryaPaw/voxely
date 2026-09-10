import { describe, expect, it } from "vitest";
import { overlayBarHeights, drawOverlayWave, OVERLAY_BAR_COUNT } from "./overlay-wave";

describe("overlayBarHeights", () => {
  it("follows a scrolling amplitude history", () => {
    const levels = Array.from({ length: OVERLAY_BAR_COUNT }, (_, index) =>
      index === OVERLAY_BAR_COUNT - 1 ? 0.4 : index === 3 ? 0.1 : 0,
    );
    const bars = overlayBarHeights(levels, true);
    expect(bars).toHaveLength(OVERLAY_BAR_COUNT);
    expect(bars[OVERLAY_BAR_COUNT - 1] ?? 0).toBeGreaterThan(bars[3] ?? 0);
    expect(bars[3] ?? 0).toBeGreaterThan(bars[10] ?? 0);
  });

  it("stays flat when not recording", () => {
    const levels = Array.from({ length: OVERLAY_BAR_COUNT }, () => 0.8);
    expect(overlayBarHeights(levels, false).every((height) => height === 0)).toBe(true);
  });

  it("does not tile a repeating sine", () => {
    const levels = Array.from({ length: OVERLAY_BAR_COUNT }, (_, index) =>
      index < 8 ? 0.5 : 0.02,
    );
    const bars = overlayBarHeights(levels, true);
    const first = bars.slice(0, 8).reduce((total, value) => total + value, 0);
    const later = bars.slice(16, 24).reduce((total, value) => total + value, 0);
    expect(first).toBeGreaterThan(later);
  });
});

describe("drawOverlayWave", () => {
  it("does not throw on an empty canvas", () => {
    const ctx = {
      clearRect: () => undefined,
      fill: () => undefined,
      beginPath: () => undefined,
      moveTo: () => undefined,
      lineTo: () => undefined,
      closePath: () => undefined,
      fillStyle: "",
    };
    expect(() =>
      drawOverlayWave(ctx as unknown as CanvasRenderingContext2D, 120, 16, [0.2, 0.5, 0.2]),
    ).not.toThrow();
  });
});
