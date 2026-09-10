export const OVERLAY_BAR_COUNT = 48;
const LERP = 0.55;
const GAIN = 4.2;

export function overlayBarHeights(
  levels: number[],
  recording: boolean,
  previous: number[] = [],
  count = OVERLAY_BAR_COUNT,
): number[] {
  if (!recording) {
    return Array.from({ length: count }, () => 0);
  }
  return Array.from({ length: count }, (_, index) => {
    const sample = Math.max(0, levels[index] ?? 0);
    const target = Math.min(1, sample * GAIN);
    const prior = previous[index] ?? target;
    return Math.min(1, Math.max(0, prior + (target - prior) * LERP));
  });
}

export function drawOverlayWave(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  bars: number[],
): void {
  ctx.clearRect(0, 0, width, height);
  if (bars.length < 2 || width <= 0 || height <= 0) {
    return;
  }
  const mid = height / 2;
  const span = Math.max(1, height / 2 - 1);
  ctx.beginPath();
  ctx.moveTo(0, mid);
  bars.forEach((value, index) => {
    const x = (index / (bars.length - 1)) * width;
    const y = mid - Math.min(1, Math.max(0, value)) * span;
    ctx.lineTo(x, y);
  });
  for (let index = bars.length - 1; index >= 0; index -= 1) {
    const value = bars[index] ?? 0;
    const x = (index / (bars.length - 1)) * width;
    const y = mid + Math.min(1, Math.max(0, value)) * span;
    ctx.lineTo(x, y);
  }
  ctx.closePath();
  ctx.fillStyle = "#d5dbe6";
  ctx.fill();
}
