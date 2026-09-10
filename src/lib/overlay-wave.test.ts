import { describe, expect, it } from "vitest";
import {
  overlayBarHeights,
  drawOverlayWave,
  meterPollAllowed,
  overlayAmplitude,
  overlayCancelArmed,
  overlayHoverFromElement,
  OVERLAY_BAR_COUNT,
} from "./overlay-wave";

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

  it("uses measured dt instead of a fixed frame lerp", () => {
    const levels = Array.from({ length: OVERLAY_BAR_COUNT }, () => 1);
    const zeros = Array.from({ length: OVERLAY_BAR_COUNT }, () => 0);
    const slow = overlayBarHeights(levels, true, zeros, OVERLAY_BAR_COUNT, 0.004);
    const fast = overlayBarHeights(levels, true, zeros, OVERLAY_BAR_COUNT, 0.04);
    expect(fast[0] ?? 0).toBeGreaterThan(slow[0] ?? 0);
  });

  it("maps quiet speech higher than a linear gain of two", () => {
    expect(overlayAmplitude(0.08)).toBeGreaterThan(0.3);
    expect(overlayAmplitude(0.08)).toBeGreaterThan(0.08 * 2);
    expect(overlayAmplitude(1)).toBe(1);
    expect(overlayAmplitude(0.04)).toBeLessThan(overlayAmplitude(0.4));
  });
});

describe("overlayCancelArmed", () => {
  it("requires hover before a cancel click", () => {
    expect(overlayCancelArmed(true, false)).toBe(false);
    expect(overlayCancelArmed(true, true)).toBe(true);
    expect(overlayCancelArmed(false, true)).toBe(false);
  });

  it("reads :hover from the element", () => {
    const node = { matches: () => true } as unknown as Element;
    expect(overlayHoverFromElement(node)).toBe(true);
    expect(overlayHoverFromElement(null)).toBe(false);
  });
});

describe("meterPollAllowed", () => {
  it("rejects overlapping polls", () => {
    expect(meterPollAllowed(false)).toBe(true);
    expect(meterPollAllowed(true)).toBe(false);
  });
});

describe("drawOverlayWave", () => {
  it("draws rounded bars instead of a filled polygon", () => {
    const ops: string[] = [];
    const ctx = {
      clearRect: () => undefined,
      fill: () => ops.push("fill"),
      beginPath: () => ops.push("begin"),
      roundRect: () => ops.push("roundRect"),
      rect: () => ops.push("rect"),
      moveTo: () => ops.push("moveTo"),
      lineTo: () => ops.push("lineTo"),
      closePath: () => ops.push("closePath"),
      fillStyle: "",
    };
    drawOverlayWave(ctx as unknown as CanvasRenderingContext2D, 120, 16, [0.2, 0.5, 0.2]);
    expect(ops).toContain("roundRect");
    expect(ops).not.toContain("lineTo");
  });

  it("falls back to rect when roundRect is missing", () => {
    const ops: string[] = [];
    const ctx = {
      clearRect: () => undefined,
      fill: () => ops.push("fill"),
      beginPath: () => ops.push("begin"),
      rect: () => ops.push("rect"),
      fillStyle: "",
    };
    drawOverlayWave(ctx as unknown as CanvasRenderingContext2D, 120, 16, [0.5]);
    expect(ops).toContain("rect");
  });

  it("skips drawing when the canvas has no size", () => {
    const ops: string[] = [];
    const ctx = {
      clearRect: () => ops.push("clear"),
      fill: () => ops.push("fill"),
      fillStyle: "",
    };
    drawOverlayWave(ctx as unknown as CanvasRenderingContext2D, 0, 16, [1]);
    expect(ops).toEqual(["clear"]);
  });

  it("keeps a center spine on silent bars so the wave grows up and down", () => {
    const ops: string[] = [];
    const rects: number[] = [];
    const ctx = {
      clearRect: () => undefined,
      fill: () => ops.push("fill"),
      beginPath: () => ops.push("begin"),
      roundRect: (_x: number, y: number, _w: number, h: number) => {
        ops.push("roundRect");
        rects.push(y, h);
      },
      rect: () => ops.push("rect"),
      fillStyle: "",
    };
    drawOverlayWave(ctx as unknown as CanvasRenderingContext2D, 120, 20, [0, 1, 0]);
    expect(ops.filter((op) => op === "roundRect")).toHaveLength(3);
    expect(rects[0] ?? 0).toBeGreaterThan(0);
    expect(rects[4] ?? 0).toBeCloseTo(rects[0] ?? 0);
    expect(rects[3] ?? 0).toBe(20);
  });
});
