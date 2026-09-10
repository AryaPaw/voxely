import { describe, expect, it } from "vitest";
import { meterFromPeakDb, peakDbFs, previewWarning } from "./dsp-level";

describe("dsp-level", () => {
  it("converts peak to dBFS", () => {
    expect(peakDbFs(1)).toBeCloseTo(0);
    expect(peakDbFs(0.5)).toBeCloseTo(-6.02, 1);
    expect(peakDbFs(0)).toBe(Number.NEGATIVE_INFINITY);
  });

  it("maps dBFS onto a 60 dB meter", () => {
    expect(meterFromPeakDb(0)).toBe(100);
    expect(meterFromPeakDb(-60)).toBe(0);
    expect(meterFromPeakDb(Number.NEGATIVE_INFINITY)).toBe(0);
  });

  it("flags clipping and ignores a quiet signal", () => {
    expect(previewWarning(0.001, 0, "clip")).toBeNull();
    expect(previewWarning(1, 2, "clip")).toBe("clip");
    expect(previewWarning(0.2, 0, "clip")).toBeNull();
  });
});
