export function peakDbFs(peak: number): number {
  if (peak <= 1e-9) {
    return Number.NEGATIVE_INFINITY;
  }
  return 20 * Math.log10(peak);
}

export function meterFromPeakDb(db: number): number {
  if (!Number.isFinite(db)) {
    return 0;
  }
  return Math.min(100, Math.max(0, ((db + 60) / 60) * 100));
}

export function previewWarning(peak: number, clipCount: number, clipping: string): string | null {
  if (clipCount > 0 || peak >= 0.999) {
    return clipping;
  }
  return null;
}
