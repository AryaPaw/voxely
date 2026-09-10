export const OVERLAY_BAR_COUNT = 28;
const GAIN = 2;
const LERP_TAU_SECONDS = 0.05;

export function overlayBarHeights(
  levels: number[],
  recording: boolean,
  previous: number[] = [],
  count = OVERLAY_BAR_COUNT,
  dtSeconds = 1 / 60,
  reducedMotion = false,
): number[] {
  if (!recording) {
    return Array.from({ length: count }, () => 0);
  }
  const dt = Math.min(0.05, Math.max(0.001, dtSeconds));
  const alpha = reducedMotion ? 1 : 1 - Math.exp(-dt / LERP_TAU_SECONDS);
  return Array.from({ length: count }, (_, index) => {
    const sampleIndex = Math.max(0, levels.length - count) + index;
    const sample = Math.max(0, levels[sampleIndex] ?? 0);
    const target = Math.min(1, sample * GAIN);
    const prior = previous[index] ?? target;
    return Math.min(1, Math.max(0, prior + (target - prior) * alpha));
  });
}

export function meterPollAllowed(inFlight: boolean): boolean {
  return !inFlight;
}

export function drawOverlayWave(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  bars: number[],
): void {
  ctx.clearRect(0, 0, width, height);
  if (bars.length === 0 || width <= 0 || height <= 0) {
    return;
  }
  const gap = 1.5;
  const barWidth = Math.max(2, (width - gap * (bars.length - 1)) / bars.length);
  const radius = Math.min(2.5, barWidth / 2);
  ctx.fillStyle = "#d5dbe6";
  bars.forEach((value, index) => {
    const amplitude = Math.min(1, Math.max(0.08, value));
    const barHeight = Math.max(2, amplitude * height);
    const x = index * (barWidth + gap);
    const y = (height - barHeight) / 2;
    roundedBar(ctx, x, y, barWidth, barHeight, radius);
    ctx.fill();
  });
}

function roundedBar(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  width: number,
  height: number,
  radius: number,
): void {
  const r = Math.min(radius, width / 2, height / 2);
  ctx.beginPath();
  if (typeof ctx.roundRect === "function") {
    ctx.roundRect(x, y, width, height, r);
    return;
  }
  ctx.rect(x, y, width, height);
}
